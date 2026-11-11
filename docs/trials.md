# Trials: a program against an S3 that misbehaves

`sonde run` shows what a program does to its store. `sonde run --trial` shows what the program does when the store
fails: sonde, which is already in every request's path, answers some requests in the store's place, and reads from
the requests that follow whether the program retried, how long it waited, what it left behind and how it exited.

```sh
sonde run --trial -- python etl.py 's3://scratch/etl-{trial}/'
```

```text
Trial: the command runs 3 more times, each under one scenario.
  survived  slowdown       35 faults, all retried · after 101 ms to 105 ms (in step) · 1.5× the requests
  survived  error          35 faults, all retried · after 102 ms to 105 ms (in step) · 1.5× the requests
  survived  lost-answer    23 faults, all retried · after 102 ms to 104 ms (in step) · 1.4× the requests · 23 written twice

The program did not break.
Each trial is a session:
  sonde show …/sessions/trials/20261110-091555-python.slowdown.sonde
  …
```

(pylance 14 writing, indexing, compacting and reading a dataset; `tools/gen_lance.py`.) And the two scenarios that
are not an error in the store's place, on the same program:

```text
  survived  cut-body       11 faults, all retried · 11 bodies cut: 11 resumed from the cut · after 101 ms to 104 ms (in step) · 1.2× the requests
  survived  lost-race      1 fault, 0 retried · 1 lost write read back and not written again: the store has it (sonde let it through), so no commit is missing
```

## What runs

The command runs once as always (the clean run, an ordinary session), then once per scenario. **The command runs
several times, so it must be safe to run again**: `{trial}` in the command (its first word too), and the variable
`SONDE_TRIAL`, are `clean` for the first run and the scenario's name for each of the others, so each run can have a
place of its own. A command with neither `{trial}` nor a mention of `SONDE_TRIAL` is warned of once: its runs share
a place. Without `--trial` or `--fault`, `{trial}` is left as written and `SONDE_TRIAL` is not set.

**The run's name is taken out of its keys before the runs are compared.** The clean run writes
`etl-clean/state` and the trial `etl-slowdown/state`: the same request. Before a trial's requests are matched to the
clean run's, the run's own name (`clean`, the scenario's) is taken out of the bucket, the key, the query's values
and a control body (`DeleteObjects`), wherever it stands as a whole word: with no letter or digit on either side.
So `etl-clean/`, `clean/x` and `run_clean.json` lose it, and `cleaned/x` or, under the scenario `error`, `errors/x`
are left alone. A key that holds the word by itself in both runs (`error/log` written under the scenario `error`)
is matched as it was written. What this cannot see: a run's name folded into something else (a hash of it, upper
case): those keys differ between the runs, and each request is then read as one the clean run never sent.

- **A trial is read against a clean run that worked.** A clean run that exits non-zero, or is interrupted, runs no
  trial, and says so.
- **Input.** When sonde's input is not a terminal, the clean run's input is kept and every run is fed the same
  bytes (in memory up to 8 MiB, in an unnamed temporary file past it); at a terminal, the trial runs read from `/dev/null`.
- **Ctrl-C or SIGTERM** during a trial ends the whole trial: the running command is killed, with every process it
  started (each run is a process group of its own), no verdict is written for the interrupted scenario or for any
  after it, its half-written session is removed, and sonde exits 130 or 143. Verdicts of the scenarios that
  finished before it are kept. (The clean run at a terminal stays in the terminal's group, so that its program can
  read the keyboard; Ctrl-C reaches it there by itself.)
- **Every `sonde run` to a session's path starts clean**: the verdicts and trial sessions of an earlier recording
  of that file are removed first, with `--trial` or without.

Without `--fault`, the scenarios are `sonde.toml`'s if it has any, else chosen from what the clean run did:

| Scenario | What sonde does | Chosen when |
|---|---|---|
| `slowdown` | 503 SlowDown on each request's first attempt | always |
| `error` | 500 InternalError on each request's first attempt | always |
| `lost-answer` | each write reaches the store, and its first answer is replaced by a 500: the write happened and the program is not told | the program writes |
| `fail-complete` | CompleteMultipartUpload fails once | it uploads in parts |
| `cut-body` | each download's first answer goes out with its status and its whole `Content-Length`; the body stops at its half and the connection drops | it downloaded a body of 64 bytes or more |
| `lost-race` | each commit's first conditional write (`If-None-Match: *` or `If-Match`) is answered 412 though it would have won | it wrote on a condition |
| `slow` | every request waits 300 ms more | `--fault slow` |
| `s3-limits` | past 5,500 reads or 3,500 writes a second under one prefix, 503 SlowDown (S3's documented rate) | `--fault s3-limits` |

`--fault NAME` (repeatable) runs the named ones instead; a name given twice runs once.

A rule matches by **operation and key**, and counts the attempts at one request. "Each request's first attempt" is
therefore the same in a program that sends one request at a time and in one that sends fifty at once: there is no
"the third UploadPart", which would be a race.

**What one request is** (the faults and the verdict use the same definition): its method, operation, bucket, key
and part number. An upload's id is not part of it, so an upload started again under a new id retries the parts of
the first. A request without a key is told apart by its paging and listing parameters (`prefix`, `delimiter`,
`start-after`, `continuation-token`, `marker`, `key-marker`, `upload-id-marker`, `version-id-marker`, `versions`,
`uploads`) and by a hash of its body: two `DeleteObjects` batches are two requests, each failed once, each owed a
retry.

**A body cut short.** Only a `GetObject` answered 2xx with a `Content-Length` of 64 bytes or more is cut (at its
half); an error, a body too small or one without a length goes through as it is and was no attempt met. `Range` is
no part of what one request is, so the read that resumes a cut download is that download's second attempt and is
not cut again. The record keeps the store's 200 (or 206), `injected`, `x-sonde-fault`, and where the body stopped
(`x-sonde-cut-at`, and the bytes that went out as its size).

**A lost race.** A write with `If-None-Match: *` **is sent to the store** and then answered 412: the object there
is the program's own, and sonde writes nothing of its own (no competitor is made up). A write with `If-Match` is
answered 412 without being sent. Attempts are counted **by commit, not by key**: a table's log names each commit by
the next number, so the write after a lost one has another key and is still its retry, and is let through. One
commit family is one folder of SlateDB manifests (`manifest/`), WAL (`wal/`) or compaction states, of Iceberg table
metadata (`metadata/*.metadata.json`), of a Delta log (`_delta_log/*.json`) or of Lance versions (`_versions/`,
whichever way they count); any other key is a family of its own. A write the store itself refuses is the store's
answer and no fault. A 412 sonde answered is not counted among the store's own races in the trial's session.

`s3-limits` counts the requests of the second that ends now (a sliding window, not the clock's seconds) for each
prefix, a prefix being the bucket and the key's folder, three levels at most: the same prefix the "near S3's rate
for one prefix" finding uses. S3 partitions by load and does not say where; the folder is the reading sonde keeps
to in both.

## What is read

**A retry is counted against the clean run.** The clean run says how many times the program means to send each
request. A failed **write** was retried only if the trial run got at least that many answers for it: a program that
writes `state`, polls, and writes `state` again did not retry its lost first write because a second write
followed, and its own sleep is not a backoff. A failed **read** was made good by any answer after it, however many
the clean run got: a loop that polls for a fixed time gets fewer answers when backoff uses the time, and gave up
on nothing. An answer is the store's own and not one that asks for another attempt.

**Which answers ask for another attempt** is one rule, the trial's and the one retries are read by in any session:
no answer, or a 408, 429, 500, 502, 503 or 504 (and S3's `SlowDown` and `RequestTimeout`). A 404, a 412 or a 501 is
an answer, however often it is asked for. **What the clean run itself never got an answer for is not owed**: a store
without `DeleteObjects` answers 501 in the clean run and in the trial, and the program did not break.

| Case | Read as |
|---|---|
| A key written twice on purpose | two answers are meant; one is one write never sent again |
| A key read again and again (a poll) | a failed read answered later was made good. It is *retried* when the answer came sooner than half the shortest wait the clean run ever left between two reads of it, and *read again later* otherwise: a retry or the loop's next round, which the requests do not tell apart. A failed read nothing answered after was given up (degraded) |
| A scenario with `times = 2` | two failures in a row then an answer: two retries; one retry then nothing: one retried, one request dropped |
| An upload started again under a new id | its parts and its completion are the same requests, retried |
| A request the clean run never sent (a key named by the clock) | meant once: retried by an answer after its failure, never by one before it |
| A failure nothing followed | not retried, whatever the count |
| A body cut short | *resumed* when later downloads of the object carry every byte from the cut to the end of what was asked for; *read again whole* when one carries all of it. A key the clean run reads twice must be read twice here too. Nothing after it carrying the rest, and the clean run's exit: the half was taken as the whole (broke). Later reads that start past the cut leave a hole (broke) |
| A lost race | run again when the trial makes as many commits in that log as the clean run did: a conditional write in the same family, answered 2xx, after the 412 |

| | |
|---|---|
| **faults, retried** | the requests sonde failed, and how many of them the program sent again (as above) |
| **read again later** | failed reads of a key the program reads repeatedly, answered later: not called retries (above) |
| **after … to …** | the wait from a failed answer to the attempt that is its retry: shortest and longest, as they read (waits that round to the same figure print as one). **Within one sender only**: a request that several senders had in flight at once (sixteen threads writing one key), here or in the clean run, gives no wait, and neither does a retry sent from another line of code when callers are named; whose attempt follows whose cannot be seen. *Jittered* or *in step* is judged among the waits before one attempt number, so a backoff of 1 s, 2 s, 4 s is in step and a randomized one is jittered; with fewer than four waits before the same attempt it is not said, and of waits under 50 ms it says *too short to tell jitter* (the machine's timing is as large as the wait) |
| **none a retry is owed for** | the scenario's fault is a status no retry is owed for (a 404, a 403, a 501), or the clean run never got an answer for the request either: nothing is counted as retried or given up, and the verdict is the exit's and the state's |
| **N delayed · took … (the clean run …)** | a delay scenario (`slow`): the command's wall time under it against the clean run's |
| **× the requests** | the run's requests against the clean run's |
| **written twice** | writes the store made, whose answer was lost, and which the store made again. Not a delete (made twice, it removed once), not a create sent with `If-None-Match: *` (the store was asked not to), and not a retry the store refused (a CompleteMultipartUpload answered 404 NoSuchUpload because the upload had completed; a conditional write answered 412) |
| **answered 500 and not retried; the store has them** | writes whose answer was lost and which the program never asked about again: the data is there, the program was told it is not |
| **left unfinished** | multipart uploads neither completed nor aborted, beyond the clean run's. An upload whose completion the store made (its answer lost) is complete |
| **N bodies cut: … resumed from the cut, … read again whole** | downloads sonde cut, and how each was made good |
| **fenced** | a SlateDB writer answered 412 that stopped (a non-zero exit) and wrote nothing after: what the format asks of a writer that lost. Its exit is not held against it |
| **N more objects left than the clean run leaves** | after a lost race: objects written without a condition and not removed, beyond the clean run's. They are the lost commit's files unless the commit that followed names them, which sonde does not read (degraded) |
| **sent again with the same If-Match** | the write was run again with the ETag that had just lost: nothing was read in between, and a real writer's new ETag would refuse it again (degraded) |
| **read back and not written again** | after a 412 to a write sonde had let through (`If-None-Match: *`), the program read that key, did not write the commit again, made every other commit the clean run made, wrote the key no other way and exited as its clean run did. The store then holds exactly the clean run's commits and nothing more, so this is **survived**: nothing is missing and nothing is to be changed. It is what a writer that knows its own commit does (Lance reads the manifest back and carries on). What the requests do not show is whether the program knew the write for its own; a program that reads back another writer's commit and carries on the same would read the same here |

And the standing:

- **survived**: every failed request that is owed a retry was sent again, every cut body read to its end, every
  lost race run again (or its write, which the store has, read back; or the writer fenced), nothing more was left than a clean run leaves, and the program exited
  as its clean run did.
- **degraded**: it finished, but gave up on a read, left an upload unfinished, or after a lost race left more
  objects than the clean run or sent the same `If-Match` again.
- **broke**: it exited differently from its clean run; or a write failed, was never sent again, and it exited 0;
  or half a body was taken as the whole; or a lost race was never run again and it exited as its clean run did; or
  the contested key was written over without a condition (the other writer's commit would be gone).
  When the exit differs and a retried write was refused by the store, the reason says so: *the retried
  CompleteMultipartUpload was answered 404 NoSuchUpload: the upload had completed*.
- **untested**: the scenario met none of its requests. It is neither a pass nor a failure: `--trial` with only
  untested scenarios exits 0 and says *Nothing was tried*.

**A verdict that is not "survived" says where.** After the reasons the line names the request it turns on, the line
of code when the run named callers (`sonde run --callers`), and what to change:

```text
  broke     slowdown       2 faults, 0 retried · 1 write failed and was never sent again, and the program exited 0 · at PutObject scratch/etl-slowdown/state (etl.py:12 save) · boto3 1.43 did not send it again: turn its retries on (boto3.client("s3", config=Config(retries={"mode": "standard", "max_attempts": 5})))
```

The request is the first write never sent again; else the first body taken cut; else the first race not run again;
else the first read given up; else, when only the exit differs, the last request that failed. The setting is the
SDK's own, for boto3, the AWS SDKs for Java, Go and JavaScript (the setting that turns retries on, and
nothing else); for another client the rule itself is said. `sonde check` prints the same line in every format, and the
verdicts' file carries `where` (`operation`, `key`, `caller`) and `advice` (a phrase: `key` and `args`).

"Survived" claims only what can be seen from the requests: sonde does not read the program's data back.

## Where the results go

- Each trial is a session of its own, `trials/<run>.<scenario>.sonde` beside the run's session: `sonde show` reads
  it like any other, and `sonde run --trial` ends by naming each file. Its **first** finding says how many requests
  met a fault sonde injected; the 503s sonde answered and the retries after them are not reported as the store's
  ("the store asked for fewer requests" is said only of the store's own 503s). Records carry `injected` (the
  scenario's name); a replay skips the ones sonde failed. Trials are not runs of the program: "the newest session",
  the trend and the baseline pass them over.
- The verdicts are kept beside the run (`<run>.trials.json`); each names its session by file name, never by path.
  `sonde check` prints them as rows (`pass` survived, `warn` degraded, `FAIL` broke, `skip` untested) and **fails
  when the program broke, with a budget or without one**; `-f json` carries them as `trials`, with `passed` and
  `verdict` for the whole. `sonde sessions` shows the worst standing of each run's trials.
- In the app a trial starts from **Try under faults…** — on Run once a run has ended, and on a session's **Runs**
  pane. A dialog says first what will run: the scenarios the run's own requests call for (or the project's), each
  with a box to leave it out; that every run writes where the clean run wrote when the command names no `{trial}`;
  and that nothing runs while sonde is read-only. Then it follows the trial scenario by scenario, and Cancel ends
  it and keeps nothing. The verdicts stay on the run's Runs pane, each with why it is not "survived"; a row opens
  that trial's session, which says under its title that it is a trial (and of which run, with the way back), has no
  Runs of its own and is not run again, renamed or set as a baseline. Requests sonde answered are marked "Injected
  by sonde" wherever requests are listed and on the charts; the sessions list marks a run whose trial broke.
- Deleting a run (`sonde sessions prune`, the app) removes its verdicts and the files in its `trials/` folder that
  are named for it, and nothing else: what a `.trials.json` says is never used as a path.
- `sonde run --trial` exits with the command's own status when the clean run failed (and tries nothing), 130 or 143
  when interrupted, else 1 when a trial broke, else 0.

## A project's own scenarios

```toml
# sonde.toml
[[scenario]]
name = "flaky-wal"
fault = "503 SlowDown"     # or "500", "404 NoSuchKey", "lost-answer", "cut-body", "lost-race", "delay 200ms"
on = ["PutObject"]         # S3 operation names; "Put*" matches by its start; all when left out
prefix = "db/wal/"         # keys under it; all when left out
times = 2                  # attempts at each request that meet it (1; 0 is every attempt)
```

With scenarios in `sonde.toml`, `--trial` runs those; `--fault flaky-wal` runs one.

`sonde.toml` is checked before anything runs, and what it says wrongly is a configuration error (exit 64), not
passed over: an operation name in `on` that no request can have (`PutObjekt`), a `[[scenarios]]` table or any other
unknown key, a scenario named as one of sonde's own (`slowdown`, `error`, …) or `clean` (the name of the run without
faults), a name used twice, a fault with a word too many (`503 Slow Down`: a status and one code), a
`[program] name = ""`.

## Limits

- The faults are the proxy's: a program that reaches its store some other way is not tried.
- **A cut body.** sonde sees which bytes were asked for again, not what the program did with the ones it had. A
  program that reads the half, fails its own checksum and stops with its clean run's exit code reads as "took the
  half as whole". A body sent without `Content-Length` (chunked) is not cut. The cut is at the half, once for each
  object: a program that fails only on a cut in its last bytes, or on a second cut, is not found out.
- **A lost race.** There is no other writer. With `If-None-Match: *` the store holds the program's own write and
  the program is told it lost: a program that then commits the next number has made its commit twice (as it would
  not against a real writer), and one that reads the key back and carries on reads as survived whether or not it
  knew the write for its own. sonde does not read a commit's content, so whether a retry rebased on what it lost
  to, and whether the files a lost commit wrote are named by the next, is not known: only that more objects were
  left. A commit protocol sonde has no family for (numbered keys under another folder name) is read key by key:
  the next number is then a new commit, failed once itself.
- A wait is read only where one sender is seen; with several senders on one request no backoff is printed.
