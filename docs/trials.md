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

(pylance 14 writing, indexing, compacting and reading a dataset; `tools/gen_lance.py`.)

## What runs

The command runs once as always (the clean run, an ordinary session), then once per scenario. **The command runs
several times, so it must be safe to run again**: `{trial}` in the command (its first word too), and the variable
`SONDE_TRIAL`, are `clean` for the first run and the scenario's name for each of the others, so each run can have a
place of its own. A command with neither `{trial}` nor a mention of `SONDE_TRIAL` is warned of once: its runs share
a place. Without `--trial` or `--fault`, `{trial}` is left as written and `SONDE_TRIAL` is not set.

- **A trial is read against a clean run that worked.** A clean run that exits non-zero, or is interrupted, runs no
  trial, and says so.
- **Input.** When sonde's input is not a terminal, the clean run's input is kept and every run is fed the same
  bytes; at a terminal, the trial runs read from `/dev/null`.
- **Ctrl-C or SIGTERM** during a trial ends the whole trial: the running command is killed, no verdict is written
  for the interrupted scenario or for any after it, its half-written session is removed, and sonde exits 130 or 143.
  Verdicts of the scenarios that finished before it are kept.
- **Every `sonde run` to a session's path starts clean**: the verdicts and trial sessions of an earlier recording
  of that file are removed first, with `--trial` or without.

Without `--fault`, the scenarios are `sonde.toml`'s if it has any, else chosen from what the clean run did:

| Scenario | What sonde does | Chosen when |
|---|---|---|
| `slowdown` | 503 SlowDown on each request's first attempt | always |
| `error` | 500 InternalError on each request's first attempt | always |
| `lost-answer` | each write reaches the store, and its first answer is replaced by a 500: the write happened and the program is not told | the program writes |
| `fail-complete` | CompleteMultipartUpload fails once | it uploads in parts |
| `slow` | every request waits 300 ms more | `--fault slow` |
| `s3-limits` | past 5,500 reads or 3,500 writes a second under one prefix, 503 SlowDown (S3's documented rate) | `--fault s3-limits` |

`--fault NAME` (repeatable) runs the named ones instead.

A rule matches by **operation and key**, and counts the attempts at one request. "Each request's first attempt" is
therefore the same in a program that sends one request at a time and in one that sends fifty at once: there is no
"the third UploadPart", which would be a race.

**What one request is** (the faults and the verdict use the same definition): its method, operation, bucket, key
and part number. An upload's id is not part of it, so an upload started again under a new id retries the parts of
the first. A request without a key is told apart by its paging and listing parameters (`prefix`, `delimiter`,
`start-after`, `continuation-token`, `marker`, `key-marker`, `upload-id-marker`, `version-id-marker`, `versions`,
`uploads`) and by a hash of its body: two `DeleteObjects` batches are two requests, each failed once, each owed a
retry.

`s3-limits` counts the requests of the second that ends now (a sliding window, not the clock's seconds) for each
prefix, a prefix being the bucket and the key's folder, three levels at most: the same prefix the "near S3's rate
for one prefix" finding uses. S3 partitions by load and does not say where; the folder is the reading sonde keeps
to in both.

## What is read

**A retry is counted against the clean run.** The clean run says how many times the program means to send each
request. A failed request was retried only if the trial run got at least that many answers for it: a program that
writes `state`, polls, and writes `state` again did not retry its lost first write because a second write
followed, and its own sleep is not a backoff. An answer is the store's own and not one that asks for another
attempt (a 404 or a 412 is an answer; a 5xx, a 429 or no response is not).

| Case | Read as |
|---|---|
| A key written twice on purpose | two answers are meant; one is one write never sent again |
| Polling GETs | retried when the run still gets as many answers as the clean run's polls; a loop that counts its polls and is one short gave up on a read (degraded) |
| A scenario with `times = 2` | two failures in a row then an answer: two retries; one retry then nothing: one retried, one request dropped |
| An upload started again under a new id | its parts and its completion are the same requests, retried |
| A request the clean run never sent (a key named by the clock) | meant once: retried by an answer after its failure, never by one before it |
| A failure nothing followed | not retried, whatever the count |

| | |
|---|---|
| **faults, retried** | the requests sonde failed, and how many of them the program sent again (as above) |
| **after … to …** | the wait from a failed answer to the attempt that is its retry: shortest and longest, as they read (waits that round to the same figure print as one). *Jittered* or *in step* is judged among the waits before one attempt number, so a backoff of 1 s, 2 s, 4 s is in step and a randomized one is jittered; with fewer than four waits before the same attempt it is not said, and the range stands alone |
| **none a retry is owed for** | the scenario's fault is a 4xx other than 408 and 429 (a 404, a 403): nothing is counted as retried or given up, and the verdict is the exit's and the state's |
| **N delayed · took … (the clean run …)** | a delay scenario (`slow`): the command's wall time under it against the clean run's |
| **× the requests** | the run's requests against the clean run's |
| **written twice** | writes the store made, whose answer was lost, and which the store made again. A retry the store refused (a CompleteMultipartUpload answered 404 NoSuchUpload because the upload had completed; a conditional write answered 412) is not one |
| **answered 500 and not retried; the store has them** | writes whose answer was lost and which the program never asked about again: the data is there, the program was told it is not |
| **left unfinished** | multipart uploads neither completed nor aborted, beyond the clean run's. An upload whose completion the store made (its answer lost) is complete |

And the standing:

- **survived**: every failed request that is owed a retry was sent again, nothing more was left than a clean run
  leaves, and the program exited as its clean run did.
- **degraded**: it finished, but gave up on a read, or left an upload unfinished.
- **broke**: it exited differently from its clean run, or a write failed, was never sent again, and it exited 0.
  When the exit differs and a retried write was refused by the store, the reason says so: *the retried
  CompleteMultipartUpload was answered 404 NoSuchUpload: the upload had completed*.
- **untested**: the scenario met none of its requests. It is neither a pass nor a failure: `--trial` with only
  untested scenarios exits 0 and says *Nothing was tried*.

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
fault = "503 SlowDown"     # or "500", "404 NoSuchKey", "lost-answer", "delay 200ms"
on = ["PutObject"]         # S3 operation names; "Put*" matches by its start; all when left out
prefix = "db/wal/"         # keys under it; all when left out
times = 2                  # attempts at each request that meet it (1; 0 is every attempt)
```

With scenarios in `sonde.toml`, `--trial` runs those; `--fault flaky-wal` runs one.

`sonde.toml` is checked before anything runs, and what it says wrongly is a configuration error (exit 64), not
passed over: an operation name in `on` that no request can have (`PutObjekt`), a `[[scenarios]]` table or any other
unknown key, a scenario named as one of sonde's own (`slowdown`, `error`, …), a name used twice.

## Limits

- The faults are the proxy's: a program that reaches its store some other way is not tried.
- A response body cut short, and a lost race on a conditional write (a real competing writer), are not scenarios
  yet.
- The app shows a run's trials; starting one is the CLI's.
