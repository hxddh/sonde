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
```

(pylance 14 writing, indexing, compacting and reading a dataset; `tools/gen_lance.py`.)

## What runs

The command runs once as always (the clean run, an ordinary session), then once per scenario. **The command runs
several times, so it must be safe to run again**: `{trial}` in an argument, and the variable `SONDE_TRIAL`, are
`clean` for the first run and the scenario's name for each of the others, so each run can have a place of its own.

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

A rule matches by **operation and key**, and counts the attempts at one request (its method, its place, its part
number). "Each request's first attempt" is therefore the same in a program that sends one request at a time and in
one that sends fifty at once: there is no "the third UploadPart", which would be a race.

## What is read

| | |
|---|---|
| **faults, retried** | the requests sonde failed, and how many of them the program sent again |
| **after … to …** | the wait from a failed answer to its retry: shortest and longest. *Jittered* when the waits differ; *in step* when they do not (retries in step reach a throttled store in waves) |
| **× the requests** | the run's requests against the clean run's |
| **written twice** | writes the store made, whose answer was lost, and which were made again. Harmless for a plain PUT; for a conditional write the second attempt is answered 412, which the program must read as its own success |
| **left unfinished** | multipart uploads neither completed nor aborted, beyond the clean run's |

And the standing:

- **survived**: every failed request was sent again, nothing more was left than a clean run leaves, and the program
  exited as its clean run did.
- **degraded**: it finished, but gave up on a read, or left an upload unfinished.
- **broke**: it exited differently from its clean run, or a write failed, was never sent again, and it exited 0.
- **untested**: the scenario met none of its requests.

"Survived" claims only what can be seen from the requests: sonde does not read the program's data back.

## Where the results go

- Each trial is a session of its own (in `trials/` beside the run's session): `sonde show` reads it like any other.
  Its first finding says how many requests met a fault sonde injected, so that its errors and retries are not read
  as the store's. Records carry `injected` (the scenario's name); a replay skips the ones sonde failed.
- The verdicts are kept beside the run (`<session>.trials.json`). `sonde check` prints them as rows and **fails
  when the program broke**; the app shows them on the run's **Runs** pane, each opening its trial's session.
- `sonde run --trial` exits with the command's own status when that is not 0, else 1 when a trial broke.

## A project's own scenarios

```toml
# sonde.toml
[[scenario]]
name = "flaky-wal"
fault = "503 SlowDown"     # or "500", "404 NoSuchKey", "lost-answer", "delay 200ms"
on = ["PutObject"]         # operation names; "Put*" matches by its start; all when left out
prefix = "db/wal/"         # keys under it; all when left out
times = 2                  # attempts at each request that meet it (1; 0 is every attempt)
```

With scenarios in `sonde.toml`, `--trial` runs those; `--fault flaky-wal` runs one.

## Limits

- The faults are the proxy's: a program that reaches its store some other way is not tried.
- A response body cut short, and a lost race on a conditional write (a real competing writer), are not scenarios
  yet.
- The app shows a run's trials; starting one is the CLI's.
