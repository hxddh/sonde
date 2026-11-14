# Trials

`sonde run --trial` runs a program once normally (the **clean run**), then once per fault scenario, with sonde
answering chosen requests in the store's place. From the requests that follow it judges each scenario
**survived**, **degraded**, **broke** or **untested**.

```sh
sonde run --trial -- python etl.py 's3://scratch/etl-{trial}/'
sonde run --fault slowdown:3 --fault cut-body -- python etl.py 's3://scratch/etl-{trial}/'
sonde run --trial --again -- python etl.py 's3://scratch/etl-{trial}/'   # only what did not survive
```

```text
Trial: the command runs 3 more times, each under one scenario.
  survived  slowdown       35 faults, all retried · after 101 ms to 105 ms (in step) · 1.5× the requests
  survived  lost-answer    23 faults, all retried · after 102 ms to 104 ms (in step) · 1.4× the requests · 23 written twice
  survived  cut-body       11 faults, all retried · 11 bodies cut: 11 resumed from the cut · after 101 ms to 104 ms (in step) · 1.2× the requests
  survived  lost-race      1 fault, 0 retried · 1 lost write read back and not written again: the store has it (sonde let it through), so no commit is missing
```

## What runs

- **The command must be safe to run again.** `{trial}` anywhere in it and `SONDE_TRIAL` are `clean`, then the
  scenario's name. A command with neither is warned about once. Without `--trial`/`--fault`, `{trial}` stays as
  written and `SONDE_TRIAL` is unset.
- A clean run that exits non-zero or is interrupted runs no trial.
- **stdin**: when not a terminal, every run gets the same bytes (in memory up to 8 MiB, then a temporary file); at a
  terminal, trial runs read `/dev/null`.
- **Ctrl-C / SIGTERM**: kills the command's process group, removes the interrupted scenario's partial session, gives
  it and later ones no verdict, exits 130 / 143. Finished verdicts are kept. (The clean run at a terminal stays in
  the terminal's group, to read the keyboard.)
- Every `sonde run` to a session path first removes the earlier recording's verdicts and trial sessions. The
  program's verdict history is kept.
- Under `no-delete`, every run may write over what the program wrote before and over keys under its `{trial}` places
  ([recording](recording.md#what-a-recorded-program-may-do)): a trial can be run again as it is.

**Matching a trial's requests to the clean run's**: they match when they differ only by the scenario's name in place
of `clean`, as a whole word (no letter or digit beside it), in bucket, key, query values or control body.

| Trial (scenario) | Matches |
|---|---|
| `etl-slowdown/state` | `etl-clean/state`, never `etl-clean/raw` |
| `etl-slowdown/clean/state` | `etl-clean/clean/state` |
| `etl-error/error.json` (`error`) | `etl-clean/error.json` |
| `cleaned/x`, `errors/x` (`error`) | themselves |

A request identical in both runs is matched as written first. A transformed name (hashed, upper-cased) is not seen:
such requests read as ones the clean run never sent.

## Scenarios

Without `--fault`: `sonde.toml`'s scenarios if any, else chosen from what the clean run did. `--fault NAME` is
repeatable; duplicates run once.

| Scenario | What sonde does | Chosen when |
|---|---|---|
| `slowdown` | 503 SlowDown on each request's first attempt | always |
| `error` | 500 InternalError on each request's first attempt | always |
| `skew` | 403 RequestTimeTooSkewed on each request's first attempt | every signed request's SDK (user agent) retries it: the AWS SDKs for Java, JavaScript. Never botocore (boto3, AWS CLI): only `--fault skew` |
| `lost-answer` | each write reaches the store; its first answer becomes 500 | it writes |
| `fail-complete` | CompleteMultipartUpload fails once (500) | it uploads in parts |
| `complete-500` | each CompleteMultipartUpload completes; its first answer is 500 | it uploads in parts |
| `cut-body` | each download's first answer: status and full `Content-Length`, then the body stops at half and the connection drops | it downloaded a body ≥ 64 bytes |
| `slow-first-byte` | the first download held 10 s before forwarding (one in all) | it downloaded |
| `lost-race` | each commit's first conditional write (`If-None-Match: *`, `If-Match`) answered 412 | it commits on a condition (a create in a table's log, or `If-Match`), not a create of its own key (`_SUCCESS`, a lock) |
| `stale-list` | `ListObjects`/`ListObjectsV2` leave out keys this run wrote, each key once | it listed a folder after writing in it |
| `slow` | every request 300 ms slower | only `--fault slow` |
| `s3-limits` | 503 SlowDown past 5,500 reads or 3,500 writes a second per prefix | only `--fault s3-limits` |

- **`slowdown:N` / `error:N`** (1–5): fail each request's first N attempts. The verdict row reads `slowdown:3`; the
  session is `<run>.slowdown-3x.sonde` and `{trial}` keeps `slowdown`; two strengths are two runs, and `--again`
  runs each that stands broke. No other built-in takes a number (project scenarios set `times`); the app offers the
  same count.
- **One request** = method, kind, bucket, key, part number (not upload id: a restarted upload retries the same
  parts). Key-less requests add `prefix`, `delimiter`, `start-after`, `continuation-token`, `marker`, `key-marker`,
  `upload-id-marker`, `version-id-marker`, `versions`, `uploads` and the body's hash. Attempts are counted per
  request, so concurrency does not change which fail.
- **cut-body**: only a 2xx `GetObject` with `Content-Length` ≥ 64, at half; anything else passes and is not counted.
  `Range` is not part of identity, so the resume is the second attempt and is not cut. The record keeps the store's
  200/206, `injected`, `x-sonde-fault`, `x-sonde-cut-at`, and the bytes sent as its size.
- **lost-race**: an `If-None-Match: *` write is sent to the store, then answered 412; an `If-Match` write is
  answered 412 unsent. Attempts count per commit family, so the next-numbered write is the retry and passes.
  Families: SlateDB `manifest/`, `wal/`, compaction states; Iceberg `metadata/*.metadata.json`; Delta
  `_delta_log/*.json`; Lance `_versions/`; any other key is its own. A store's own 412 is not a fault.
- **s3-limits**: a sliding one-second window per prefix (bucket plus up to three folder levels), as the "near S3's
  rate for one prefix" finding.

## How results are read

**Retries**

- The clean run says how many times each request is sent. A failed **write** is retried only if the trial got at
  least as many answers for it; a failed **read** is made good by any later answer.
- An answer is the store's own and not retryable. **Retryable** (one rule for trials, sessions and signals): no
  answer; 408, 429, 500, 502, 503, 504; `SlowDown`, `RequestTimeout`, `RequestTimeTooSkewed` at any status. 404,
  412, 501 and 403 `AccessDenied` are answers.
- A request the clean run never got an answer for is owed no retry.

| Case | Read as |
|---|---|
| A key written twice on purpose | two answers expected |
| A polled key | a failed read answered later is *retried* if sooner than half the clean run's shortest gap between reads of it, else *read again later*; never answered after: given up (degraded) |
| `times = 2` | two failures then an answer: two retries; one retry then nothing: one retried, one dropped |
| An upload restarted under a new id | the same requests, retried |
| A request the clean run never sent | expected once |
| A failure nothing followed | not retried |
| A body cut short | *resumed* if later downloads cover the cut to the end; *read again whole* if one carries all of it (a key read twice in the clean run must be read twice). Rest never read, clean exit: half taken as whole (broke). Later reads starting past the cut: a hole (broke) |
| A lost race | run again if the trial makes as many commits in that log as the clean run (a 2xx conditional write in the family after the 412) |
| A lost race on a create of the program's own key | owed nothing; taken as made or tried again: *survived* by the exit; written over without the condition: *broke* |
| A lost answer not resent, then the key read more than in the clean run | neither retried nor dropped |
| CompleteMultipartUpload answered 500 though done | retried and 404 NoSuchUpload, then taken as done (or object read) with the clean exit: *survived*; reported as failure: *broke*. Not retried but object read: *survived*. Not retried, not read, exit 0: *broke*. Restarted from `CreateMultipartUpload`: *degraded* |
| A stall (`slow-first-byte`, project delay ≥ 5 s) | cut short if the program closed the connection (`x-sonde-given-up`) or resent; waited out: *degraded*. Under 5 s (`slow`) only slows |
| A key left out of a listing | per key against the clean run: read as often *survived*; less *broke*; written again *degraded*; untouched in both and no relisting *untested* |

**The verdict line**

| Term | Meaning |
|---|---|
| **faults, retried** | requests sonde failed; how many were sent again |
| **read again later** | failed polled reads answered later |
| **after … to …** | shortest and longest wait from failure to retry, single sender only. *Jittered* / *in step* compared among waits before the same attempt number (1 s, 2 s, 4 s is in step); needs four waits; under 50 ms: *too short to tell jitter* |
| **none a retry is owed for** | non-retryable fault, or no clean-run answer; the verdict is the exit's and state's |
| **N delayed · took … (the clean run …)** | `slow`: wall time against the clean run's |
| **× the requests** | against the clean run |
| **written twice** | writes whose answer was lost, made again (not deletes, `If-None-Match: *` creates, or store-refused retries: 404 NoSuchUpload, 412) |
| **answered 500 and not retried; the store has them** | lost-answer writes never asked about again |
| **left unfinished** | multipart uploads neither completed nor aborted, beyond the clean run's |
| **N bodies cut: … resumed from the cut, … read again whole** | how each cut download was made good |
| **fenced** | a SlateDB writer answered 412 that exited non-zero and wrote nothing after; exit not held against it |
| **N more objects left than the clean run leaves** | after a lost race (degraded) |
| **sent again with the same If-Match** | resent with the losing ETag, nothing read between (degraded) |
| **N creates answered 412 and taken as made** | races on the program's own key, not written over |
| **N lost answers not sent again but read back** | see the cases |
| **N held before their first byte: N cut short and sent again** | stalls, and those given up and resent |
| **waited out N stalls of 10.0 s** | (degraded) |
| **N keys this run wrote left out of a listing: N listed again** | hidden keys, and those a later listing showed |
| **never read as the clean run read them** | (broke) |
| **written again: the program took them for missing** | (degraded) |
| **made again from its first part** | a completed upload started again (degraded) |
| **read back and not written again** | after a let-through 412: read the key, made every other clean-run commit, same exit: survived (Lance) |

**Verdicts**

| Verdict | When |
|---|---|
| **survived** | every owed retry sent; every cut body read to its end; every lost race run again (or read back, fenced, taken as made); every stall cut short; every hidden key still read; nothing left beyond the clean run; same exit |
| **degraded** | finished, but gave up a read, left an upload unfinished, waited out a stall, re-uploaded a completed upload, rewrote a hidden key, or after a lost race left extra objects or resent the same `If-Match` |
| **broke** | another exit; a failed write never resent with exit 0; half a body taken as whole; a lost race never run again with the clean exit; the contested key overwritten unconditionally; a hidden key never read |
| **untested** | no request met the fault, or hidden keys mattered to nothing. Only untested: exit 0, *Nothing was tried* |
| **refused** | the recording policy refused more of the trial's requests than of the clean run's, and the verdict would be degraded or broke: *refused by the recording policy*, advice to give each run its place or `--allow-writes`. Not judged; never broke |

When the exit differs after the store refused a retried write, the reason says so (*the retried
CompleteMultipartUpload was answered 404 NoSuchUpload: the upload had completed*). "Survived" claims only what the
requests show; sonde does not read data back.

**Where it broke, and what to change**: a verdict other than survived names one request (first that applies: a
write never resent, a cut body taken, a race not run again, a listing gone on without or rewritten, a read given up,
a stall waited out, else the last failed request), its code line (named by default) and advice: the SDK's retry
setting (boto3, AWS SDKs for Java, Go, JavaScript); for skew, re-sign using the answer's `Date`; for a stall, a
per-attempt timeout; for a stale listing, keep the written keys or list again; else the rule itself.

```text
  broke     slowdown       2 faults, 0 retried · 1 write failed and was never sent again, and the program exited 0 · at PutObject scratch/etl-slowdown/state (etl.py:12 save) · boto3 1.43 did not send it again: turn its retries on (boto3.client("s3", config=Config(retries={"mode": "standard", "max_attempts": 5})))
```

## Trying again

| | |
|---|---|
| History | Each verdict is added to its program's in `verdicts.json` beside its runs: per scenario as asked (`slowdown:3`), when, standing, run; the newest 50. Kept when a run is recorded again or deleted. |
| The line | Once a scenario has two verdicts: `broke on 12 Nov, survived since`, or `survived since 10 Nov` when it never changed. In `run`, `check`, `show` (*Under faults*), Markdown and GitHub output. JSON: `history` on each trial. |
| `--again` | `sonde run --trial --again -- CMD`: a fresh clean run, then the scenarios whose last verdict was broke, degraded or refused. A trial that left any ends by printing this command. |
| App | `Sessions.trialAgain(path, …)`: the same, from a run of the program. |

## Where the results go

| | |
|---|---|
| Sessions | `trials/<run>.<scenario>.sonde` beside the run. Records carry `injected`; the session says first how many requests met a fault (sonde's 503s are not the store's); replay skips injected requests. Not runs: excluded from "newest", trend, baseline. |
| Verdicts | in the sessions folder's `.index.jsonl`; sessions named by file name, never path. `where` (`kind`, `key`, `caller`) and `advice` (`key`, `args`). |
| `sonde check` | rows `pass` / `warn` (degraded, refused) / `FAIL` / `skip`; **fails when the program broke**, budget or not. JSON: `trials`, `passed`, `verdict`. |
| `sonde show` | *Under faults*: the run's verdicts with their history. JSON: `trials`. |
| `sonde sessions` | each run's worst verdict |
| Deleting a run | removes its verdicts and its own `trials/` files; a verdict's session name is never followed as a path |
| Exit | the command's status if the clean run failed; 130 / 143 interrupted; 1 if a trial broke; else 0 |

**In the app**: *Try under faults…* on Run (after a run) and on a session's Runs pane. The dialog lists the
scenarios with a box to leave each out, warns without `{trial}`; what the program may do is the dialog's policy; Cancel keeps nothing.
Verdicts stay on the Runs pane; a row opens its trial session (cannot be run again, renamed or made a baseline).
Injected requests are marked "Injected by sonde"; the sessions list marks runs whose trial broke.

## A project's own scenarios

```toml
# sonde.toml
[[scenario]]
name = "flaky-wal"
fault = "503 SlowDown"     # or "500", "404 NoSuchKey", "lost-answer", "cut-body", "lost-race", "stale-list", "delay 200ms"
on = ["PutObject"]         # kinds of request; "Put*" matches a prefix; all when omitted
prefix = "db/wal/"         # keys under it; all when omitted
times = 2                  # attempts per request that fail (default 1; 0 = every attempt)
```

- With scenarios here, `--trial` runs those; `--fault flaky-wal` runs one.
- A status fault: 400–599 with at most one code. `delay`: `200ms` or `2s`, up to 3600 s.
- Checked before anything runs; configuration errors (64): an `on` name no request can have (`PutObjekt`);
  `[[scenarios]]` or any unknown key; a built-in name or `clean`; a duplicate name; an extra word (`503 Slow Down`);
  `[program] name = ""`.

## Limits

- Only traffic through the proxy is tried.
- Trials use the run's policy (`no-delete` by default); a program that deletes, or writes over what another program
  wrote, needs `--allow-writes` ([recording](recording.md#what-a-recorded-program-may-do)). `SONDE_TRIAL` names no
  place sonde can read: use `{trial}`.
- **cut-body**: sonde sees which bytes are asked for again, not what the program did with the half (a checksum
  failure with the clean exit reads as "took the half as whole"). Chunked bodies are not cut. One cut per object.
- **lost-race**: no other writer exists; with `If-None-Match: *` the store holds the program's write. Commit contents
  are not read, only extra objects counted. Unknown commit protocols are read key by key.
- Waits are reported only for a single sender per request.
- **skew**: the clock is not moved; a client resending without re-signing passes here but would fail on a skewed
  store. Java and JavaScript SDKs re-sign; botocore (boto3, AWS CLI) does not retry it, so it is not offered to it.
- **slow-first-byte**: one download, the first; waiting 10 s reads the same as waiting forever. No mid-body stall.
- **stale-list**: what a listing was used for (a count, a manifest) is not seen; deletes of what a listing hid are not
  counted. S3 is strongly consistent (since December 2020); other stores and caches may not be.
