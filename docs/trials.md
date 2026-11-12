# Trials

`sonde run --trial` runs a program once normally (the **clean run**), then once per fault scenario. In each
scenario sonde answers chosen requests in the store's place, then reads from the requests that follow whether the
program retried, how long it waited, what it left behind and how it exited. Each scenario gets a verdict:
**survived**, **degraded**, **broke** or **untested**.

```sh
sonde run --trial -- python etl.py 's3://scratch/etl-{trial}/'
sonde run --fault slowdown:3 --fault cut-body -- python etl.py 's3://scratch/etl-{trial}/'
```

```text
Trial: the command runs 3 more times, each under one scenario.
  survived  slowdown       35 faults, all retried · after 101 ms to 105 ms (in step) · 1.5× the requests
  survived  error          35 faults, all retried · after 102 ms to 105 ms (in step) · 1.5× the requests
  survived  lost-answer    23 faults, all retried · after 102 ms to 104 ms (in step) · 1.4× the requests · 23 written twice
  survived  cut-body       11 faults, all retried · 11 bodies cut: 11 resumed from the cut · after 101 ms to 104 ms (in step) · 1.2× the requests
  survived  lost-race      1 fault, 0 retried · 1 lost write read back and not written again: the store has it (sonde let it through), so no commit is missing
```

(pylance writing, indexing, compacting and reading a dataset; `tools/gen_lance.py`.)

## What runs

- **The command must be safe to run again.** `{trial}` anywhere in the command (the first word too) and the
  variable `SONDE_TRIAL` are `clean` in the clean run and the scenario's name in each other run. Use them to give
  each run its own place. A command with neither is warned about once: its runs share a place.
- Without `--trial` or `--fault`, `{trial}` is left as written and `SONDE_TRIAL` is unset.
- A clean run that exits non-zero or is interrupted runs no trial, and says so.
- **Input**: when sonde's stdin is not a terminal, every run gets the same bytes (in memory up to 8 MiB, then a
  temporary file). At a terminal, trial runs read `/dev/null`.
- **Ctrl-C / SIGTERM** ends the whole trial: the running command and its process group are killed, the interrupted
  scenario and later ones get no verdict, its partial session is removed, and sonde exits 130 / 143. Finished
  verdicts are kept. (The clean run at a terminal stays in the terminal's group so it can read the keyboard.)
- Every `sonde run` to a session path first removes the earlier recording's verdicts and trial sessions.

### Matching a trial's requests to the clean run's

A trial request matches a clean-run request when they differ only by the run's name standing in for `clean`, as a
whole word (no letter or digit beside it), in bucket, key, query values or control body (`DeleteObjects`).

| Trial (scenario) | Matches clean run |
|---|---|
| `etl-slowdown/state` | `etl-clean/state` |
| `etl-slowdown/clean/state` | `etl-clean/clean/state` |
| `etl-error/error.json` (`error`) | `etl-clean/error.json` |
| `etl-slowdown/state` | never `etl-clean/raw` |
| `cleaned/x`, `errors/x` (`error`) | themselves (not whole words) |

- A request identical in both runs (`error/log` under `error`, a key without `{trial}`) is matched as written first.
- Which places carry the run's name is read from the keys, so a program may build keys from `SONDE_TRIAL` freely.
- A name transformed (hashed, upper-cased) is not seen: such requests read as ones the clean run never sent.

## Scenarios

Without `--fault`, the scenarios are `sonde.toml`'s if it has any, else chosen from what the clean run did.
`--fault NAME` (repeatable; duplicates run once) runs the named ones.

| Scenario | What sonde does | Chosen when |
|---|---|---|
| `slowdown` | 503 SlowDown on each request's first attempt | always |
| `error` | 500 InternalError on each request's first attempt | always |
| `skew` | 403 RequestTimeTooSkewed on each request's first attempt | it signs requests (any but a presigned URL) |
| `lost-answer` | each write reaches the store; its first answer is replaced by a 500 | it writes |
| `fail-complete` | CompleteMultipartUpload fails once (500) | it uploads in parts |
| `complete-500` | each CompleteMultipartUpload completes in the store; its first answer is a 500 | it uploads in parts |
| `cut-body` | each download's first answer sends its status and full `Content-Length`, then the body stops at half and the connection drops | it downloaded a body ≥ 64 bytes |
| `slow-first-byte` | the first download is held 10 s before being forwarded (one download in all) | it downloaded |
| `lost-race` | each commit's first conditional write (`If-None-Match: *` or `If-Match`) is answered 412 though it would have won | it commits on a condition: a create in a table's log, or a write with `If-Match`. Not a create of a key of its own (`_SUCCESS`, a lock) |
| `stale-list` | a listing (`ListObjects`, `ListObjectsV2`) leaves out the keys this run wrote, each key once | it listed, after a write, the folder it wrote in |
| `slow` | every request waits 300 ms more | only with `--fault slow` |
| `s3-limits` | 503 SlowDown past 5,500 reads or 3,500 writes a second per prefix | only with `--fault s3-limits` |

### Fault details

- **Pressure**: `--fault slowdown:N` / `error:N` (N = 1 to 5) fail each request's first N attempts, to see a
  program give up. The verdict row reads `slowdown:3`; the session and `{trial}` keep `slowdown`. No other built-in
  scenario takes a number; project scenarios set `times`. The app's dialog offers the same count.
- **One request** is method, operation, bucket, key and part number. Upload id is not part of it (an upload
  restarted under a new id retries the first one's parts). Key-less requests are told apart by `prefix`,
  `delimiter`, `start-after`, `continuation-token`, `marker`, `key-marker`, `upload-id-marker`,
  `version-id-marker`, `versions`, `uploads` and a hash of the body (two `DeleteObjects` batches are two requests).
- Rules match by operation and key and count attempts per request, so concurrency does not change which attempts fail.
- **cut-body**: only a 2xx `GetObject` with `Content-Length` ≥ 64 is cut, at half; anything else passes and is not
  counted as met. `Range` is not part of a request's identity, so the resuming read is the second attempt and is not
  cut. The record keeps the store's 200/206, `injected`, `x-sonde-fault`, `x-sonde-cut-at`, and the bytes sent as
  its size.
- **lost-race**: an `If-None-Match: *` write **is sent to the store**, then answered 412 (the object there is the
  program's own; sonde invents no competitor). An `If-Match` write is answered 412 without being sent. Attempts are
  counted per commit family, not key, so the next-numbered write is the retry and goes through. Families: SlateDB
  `manifest/`, `wal/`, compaction states; Iceberg `metadata/*.metadata.json`; Delta `_delta_log/*.json`; Lance
  `_versions/`. Any other key is its own family. A 412 from the store itself is not a fault; sonde's 412s are not
  counted among the store's races.
- **s3-limits**: a sliding one-second window per prefix (bucket plus up to three folder levels of the key), the same
  prefix as the "near S3's rate for one prefix" finding.

## How results are read

### Retries

- The clean run says how many times the program sends each request.
- A failed **write** counts as retried only if the trial run got at least as many answers for it as the clean run did.
- A failed **read** is made good by any later answer.
- An answer is the store's own, and not one that asks for another attempt.
- **Retryable** (one rule for trials, retries in any session and signals): no answer; 408, 429, 500, 502, 503, 504;
  S3 codes `SlowDown`, `RequestTimeout`, `RequestTimeTooSkewed` at any status. A 404, 412, 501 or 403
  `AccessDenied` is an answer.
- A request the clean run never got an answer for is not owed a retry (e.g. a store answering `DeleteObjects` with
  501 in both runs).

| Case | Read as |
|---|---|
| A key written twice on purpose | two answers expected; one is a write never sent again |
| A key read repeatedly (a poll) | a failed read answered later is made good: *retried* if answered sooner than half the shortest gap the clean run left between reads of it, else *read again later*. A failed read never answered after is given up (degraded) |
| A project scenario with `times = 2` | two failures then an answer: two retries; one retry then nothing: one retried, one dropped |
| An upload restarted under a new id | its parts and completion are the same requests, retried |
| A request the clean run never sent | expected once; retried only by an answer after its failure |
| A failure nothing followed | not retried |
| A body cut short | *resumed* if later downloads cover every byte from the cut to the end asked for; *read again whole* if one carries all of it. A key read twice in the clean run must be read twice here. Nothing covers the rest and the exit matches the clean run: half taken as whole (broke). Later reads starting past the cut leave a hole (broke) |
| A lost race | run again if the trial makes as many commits in that log as the clean run: a 2xx conditional write in the same family after the 412 |
| A lost race on a create of the program's own key the store made | owed nothing. Taken as made or tried again: *survived* by the exit. Written over without the condition: *broke* |
| A lost answer not sent again, then the key read (more often than the clean run reads it) | neither retried nor dropped: it asked the store |
| CompleteMultipartUpload answered 500 though it completed (`complete-500`, `lost-answer`) | retried and answered 404 NoSuchUpload: taking it as done (or reading the object) with the clean exit is *survived*; reporting failure is *broke*, with the reason. Not retried but the object read: *survived*. Not retried, not read, exit 0: *broke*. Restarted from `CreateMultipartUpload`: *degraded* |
| A stall (`slow-first-byte`, or a project delay ≥ 5 s) | cut short if the program closed the connection while held (`x-sonde-given-up`) or resent the request; its retry is read as any failure's. Waited out: *degraded*. A delay under 5 s (`slow`) only slows |
| A key left out of a listing (`stale-list`) | compared per key with the clean run: read as often is *survived*; read less often is *broke*; written again is *degraded*. Nothing done with it in either run and no relisting: *untested* |

### What the verdict line reports

| Term | Meaning |
|---|---|
| **faults, retried** | requests sonde failed, and how many the program sent again |
| **read again later** | failed reads of a polled key answered later; not called retries |
| **after … to …** | shortest and longest wait from a failed answer to its retry (equal when they round the same). Only within one sender: no wait is given when several senders had the request in flight, or callers show the retry came from another line. *Jittered* or *in step* is judged among waits before the same attempt number (1 s, 2 s, 4 s is in step); not said with fewer than four such waits; under 50 ms it says *too short to tell jitter* |
| **none a retry is owed for** | the fault was a non-retryable status, or the clean run never got an answer either; the verdict is the exit's and the state's |
| **N delayed · took … (the clean run …)** | `slow`: wall time against the clean run's |
| **× the requests** | requests against the clean run's |
| **written twice** | writes the store made, whose answer was lost, made again. Excludes deletes, creates sent with `If-None-Match: *`, and retries the store refused (404 NoSuchUpload, 412) |
| **answered 500 and not retried; the store has them** | lost-answer writes never asked about again: stored, but the program was told otherwise |
| **left unfinished** | multipart uploads neither completed nor aborted, beyond the clean run's |
| **N bodies cut: … resumed from the cut, … read again whole** | cut downloads and how each was made good |
| **fenced** | a SlateDB writer answered 412 that exited non-zero and wrote nothing after; its exit is not held against it |
| **N more objects left than the clean run leaves** | after a lost race, unconditional writes not removed beyond the clean run's (degraded) |
| **sent again with the same If-Match** | the write resent with the ETag that just lost, nothing read in between (degraded) |
| **N creates answered 412 and taken as made** | races on a create of the program's own key, not written over |
| **N lost answers not sent again but read back** | see the cases above |
| **N held before their first byte: N cut short and sent again** | stalled requests, and those the program gave up on and resent |
| **waited out N stalls of 10.0 s** | stalls the program waited through (degraded) |
| **N keys this run wrote left out of a listing: N listed again** | keys a stale listing hid, and those a later listing showed |
| **never read as the clean run read them** | hidden keys read less often than in the clean run (broke) |
| **written again: the program took them for missing** | hidden keys written again (degraded) |
| **made again from its first part** | an upload the store had completed, started again (degraded) |
| **read back and not written again** | after a 412 to a write sonde let through, the program read the key, did not rewrite it, made every other clean-run commit and exited as the clean run: **survived** (Lance does this) |

### Verdicts

| Verdict | When |
|---|---|
| **survived** | every owed retry was sent; every cut body read to its end; every lost race run again (or read back, fenced, or taken as made); every stall cut short; every hidden key still read; nothing left beyond the clean run; same exit as the clean run |
| **degraded** | finished, but gave up a read, left an upload unfinished, waited out a stall, re-uploaded a completed upload, rewrote a hidden key, or after a lost race left extra objects or resent the same `If-Match` |
| **broke** | a different exit from the clean run; a write failed, never resent, and exit 0; half a body taken as whole; a lost race never run again with the clean exit; the contested key written over without a condition; a hidden key never read |
| **untested** | the scenario met none of its requests, or hidden keys mattered to nothing after. `--trial` with only untested scenarios exits 0 with *Nothing was tried* |

When the exit differs and a retried write was refused by the store, the reason says so (*the retried
CompleteMultipartUpload was answered 404 NoSuchUpload: the upload had completed*).

"Survived" claims only what the requests show; sonde does not read the program's data back.

### Where it broke, and what to change

A verdict other than survived names the request, the line of code (with `sonde run --callers`) and advice:

```text
  broke     slowdown       2 faults, 0 retried · 1 write failed and was never sent again, and the program exited 0 · at PutObject scratch/etl-slowdown/state (etl.py:12 save) · boto3 1.43 did not send it again: turn its retries on (boto3.client("s3", config=Config(retries={"mode": "standard", "max_attempts": 5})))
```

- The request named, first that applies: a write never resent; a body taken cut; a race not run again; a listing
  gone on without (or rewritten); a read given up; a stall waited out; else (only the exit differs) the last failed
  request.
- Advice: the SDK's retry setting (boto3, AWS SDKs for Java, Go, JavaScript); for a skewed clock, sign again using
  the answer's `Date`; for a stall, a per-attempt timeout; for a stale listing, keep the written keys or list again.
  Other clients get the rule itself.
- `sonde check` prints the same line in every format; the verdicts file carries `where` (`operation`, `key`,
  `caller`) and `advice` (`key`, `args`).

## Where the results go

- Each scenario's session: `trials/<run>.<scenario>.sonde` beside the run's session; `sonde run --trial` lists them.
  - Its first finding counts the requests that met an injected fault; sonde's 503s and their retries are not
    reported as the store's.
  - Records carry `injected` (the scenario name); a replay skips the requests sonde failed.
  - Trials are not runs: "newest session", trend and baseline skip them.
- Verdicts: `<run>.trials.json` beside the run; each names its session by file name, never path.
- `sonde check` prints them as rows (`pass` survived, `warn` degraded, `FAIL` broke, `skip` untested) and **fails
  when the program broke**, with or without a budget. `-f json` has `trials`, plus `passed` and `verdict`.
- `sonde sessions` shows each run's worst verdict.
- Deleting a run (`sonde sessions prune`, the app) removes its verdicts and its own files in `trials/`; paths in a
  `.trials.json` are never followed.
- Exit: the command's own status if the clean run failed; 130 / 143 if interrupted; 1 if a trial broke; else 0.

### In the app

- **Try under faults…** on Run (after a run ends) and on a session's **Runs** pane.
- The dialog lists the scenarios the run calls for (or the project's), each with a box to leave it out; warns when
  the command has no `{trial}`; and refuses while sonde is read-only. Cancel ends the trial and keeps nothing.
- Verdicts stay on the Runs pane with their reasons; a row opens that trial's session, marked as a trial of its run
  (cannot be run again, renamed or made a baseline).
- Requests sonde answered are marked "Injected by sonde" in lists and charts; the sessions list marks runs whose
  trial broke.

## A project's own scenarios

```toml
# sonde.toml
[[scenario]]
name = "flaky-wal"
fault = "503 SlowDown"     # or "500", "404 NoSuchKey", "lost-answer", "cut-body", "lost-race", "stale-list", "delay 200ms"
on = ["PutObject"]         # S3 operation names; "Put*" matches a prefix; all when omitted
prefix = "db/wal/"         # keys under it; all when omitted
times = 2                  # attempts per request that fail (default 1; 0 = every attempt)
```

- With scenarios in `sonde.toml`, `--trial` runs those; `--fault flaky-wal` runs one.
- A status fault is 400–599 with at most one code; `delay` takes `200ms` or `2s` (seconds up to 3600).
- `sonde.toml` is checked before anything runs. These are configuration errors (exit 64): an `on` name no request
  can have (`PutObjekt`); `[[scenarios]]` or any unknown key; a name that is built-in or `clean`; a duplicate name;
  a fault with an extra word (`503 Slow Down`); `[program] name = ""`.

## Limits

- Only traffic through the proxy is tried.
- Trial runs use the run's policy (`no-delete` by default): each may rewrite what the clean run wrote; a program that
  deletes or overwrites an earlier run's output needs `--allow-writes`
  ([recording](recording.md#what-a-recorded-program-may-do)).
- **cut-body**: sonde sees which bytes are requested again, not what the program did with the half. A program that
  fails its own checksum and exits with the clean run's code reads as "took the half as whole". Chunked bodies
  (no `Content-Length`) are not cut. One cut, at half, per object.
- **lost-race**: there is no other writer. With `If-None-Match: *` the store holds the program's write, so a program
  that then commits the next number has committed twice, and one that reads back and carries on reads as survived
  whether or not it knew the write was its own. sonde does not read commit contents (rebase, file references); it
  sees only extra objects. Commit protocols outside the known families are read key by key.
- Waits are reported only for a single sender per request.
- **skew**: sonde answers 403 RequestTimeTooSkewed but does not move the clock; a client that resends without
  re-signing passes here but would fail on a truly skewed store. AWS SDKs for Java 2 and JavaScript 3 correct their
  clock and re-sign; botocore (boto3, AWS CLI) does not retry it.
- **slow-first-byte**: one download, the first to arrive. A program that waits 10 s reads the same as one that waits
  forever. Delays are before forwarding; a mid-body stall is not tried.
- **stale-list**: sonde sees later requests for each key, not what the listing was used for (a printed count, a
  manifest): those read as *untested* or by key reads alone. A program deleting what a listing did not show
  (orphan clean-up) deletes other keys, which are not counted. S3 has been strongly consistent since December 2020;
  other stores and caches may not be.
