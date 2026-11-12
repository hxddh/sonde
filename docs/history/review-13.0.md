# 13.0 reviewed, 14.0 proposed

A review of 13.0.0 after its release, by readers who did not write it: the Rust changes for regressions, the
rewritten interface for bugs and for its own standards, and the product from outside, used as a new user would.
Then what 14.0 should be. 13.0's own review is [review-12.0.md](review-12.0.md).

## The release, checked

`v13.0.0` is at `a85ce59`, the commit the default branch holds: 20 assets (the desktop app for Linux, macOS and
Windows, the macOS disk image, the terminal binary for five targets, `SHA256SUMS`, `latest.json` at 13.0.0 for three
platforms). The Linux binary downloaded from the release matches its checksum, says `sonde 13.0.0` and runs its demo.

## What 13.0 got wrong

### Fixes that fixed one path of several

| | Where | What is still wrong |
|---|---|---|
| High | `s3raw/client.rs` `send_stream` | The app's own downloads are still cut at 60 s: only the proxy got the streaming client. |
| High | `cmd/probe.rs`, `cmd/mod.rs` | `sonde probe -e A -e B` returns on the first endpoint that fails or is interrupted and leaves the other run's objects, uploads and buckets. |
| High | `proxy/replay.rs` `skip_reason` | A prefix-confined replay of a session recorded by 12.0 or earlier still sends `PUT /bucket?inventory…`: it trusts the name stored in the session. |
| Medium | `probes/suite/*` | "A probe whose setup failed is an error" holds for about 9 of 29 probes; the range, pagination, multipart and checksum judges still say Differs. |
| Medium | `cmd/record.rs` | `watch --logs --once` exits 0 when every log file fails to download. |
| Medium | `proxy/import.rs` | The store's side of a copy (`REST.COPY.OBJECT_GET`) is still counted as a second request. |
| Medium | `app/src/run.ts` | An empty run is still deleted under its reader from four of five ways of opening it. |
| Low | CLI | `-f` is checked on five commands; `du`, `mpu`, `probe`, `layout`, `endpoints`, `sessions` still print a table for formats they do not write. |

### Regressions of 13.0's own

| | Where | What |
|---|---|---|
| High | `sonde-core/sonde.rs` | A save that takes in an outside edit of `config.toml` keeps the cached clients: the app shows the new URL and sends to the old store until restart. |
| Medium | `sonde-core/jobs.rs` | Keeping only the 100 newest finished jobs drops results a screen still reads (a layout's time slider goes blank after 100 downloads). |
| Medium | `probes/runner.rs` | Cleanup says nothing was left behind when it could not list. |
| Medium | `services/du.rs` | A capacity scan without `s3:ListBucketVersions` now reports 0 B of old versions as a fact. |
| Medium | `proxy/lib.rs` | A response cut by the idle limit is recorded as a clean 200 with fewer bytes; `shutdown`'s "not everything drained" is ignored by every caller. |
| Medium | the sidebar | With the sidebar hidden, Buckets cannot show a failed listing or choose a bucket, and Sessions says "Choose a session" with nothing to choose from. A place's list is outside the place's error boundary. |
| Medium | `Sessions.tsx` | Leaving Sessions and coming back forgets the session, the search and Runs/Programs. A short window shows a chosen finding's fix in a 68-point scroller. |
| Medium | `plots.tsx` `Lanes` | Several requests share a cell in a long session (96 slices): the two shades are lost, hover picks one at random, a failure can be painted over. Above 4,000 requests the lanes and the dots are of a sample, and nothing says so. The legend has no 4xx. |
| Medium | `profile.tsx` | The heat map does not follow the chosen stretch; a stretch of one slice blanks the timeline; a stretch with one request collapses two charts. |
| Low | several | The rename dialog opens in every window; `−0%` and `0% past its baseline` are back in two places; a command's secrets are still read out from the Run history; "Took" columns are too narrow for `59 min 59 s`; "Copy table" copies mixed units. |

### Older, and found now

- **Keys with `.` or `..` segments go to the wrong path.** The URL library collapses `%2E%2E` as it does `..`, so
  `a/../b` is sent as `b` while the signature covers the real key; through the proxy a program's own such request is
  rewritten too. The compatibility probe for special keys can never pass, and blames the store.
- **A Parquet read is called Delta or Iceberg by its file name** (`part-*.parquet`, `*/data/*.parquet`), with no
  table log seen. Spark, Hive, Hudi and DuckDB all write such names.
- The lifecycle plan replaces a bucket's rules when the fetched document has no splice point it recognises.
- Gzip is read without a size limit.

### The interface against its own standards

The kit is mostly true now (40 cards from the kit to 7 by hand; two tab controls; one banner in ten places). What
is left: five renderings of a figure where the standard says two; eleven heading sites in six looks; three rows;
the findings' own copy of the disclosure; six layout thresholds where the standard names two, and of `BREAK`'s three
keys two are unused; the chart frame is used on the Time pane only (Capacity, Compare, the trends and the waterfall
have their own); two identical chart tooltips; `charts.tsx` still mirrors constants `plots.tsx` does not export;
four names for one duration; about forty locale keys nothing uses; the shared page was not moved to the new session
page. A screenshot 450 ms after the Time pane opens can catch the charts mid-entrance, black; a change of language
logs "a kept row is gone" twice.

### Tests that prove less than they say

Five of 13.0's tests pass with their fix reverted or assert a helper's truth table
(`a_listing_that_goes_tells_the_load_under_way`, `a_body_that_stops_moving_is_cut_after_the_timeout`, the CLI
read-only probe test, the two exit-code tests). No test covers a stalled upload.

## The product, from outside

- **The demo does not show the product.** 53 requests, 53 operations: no multipart upload, no retry, no format
  step, and `sonde show` prints 53 lines (twelve the same) before the findings.
- **`check` reads wrong.** It never names the run it judged; "newest" means one thing to `sessions` and another to
  `show` and `check`; `warnings 0 ≤ 0` on a run with a warning; a 10% tolerance that is 100% for small costs; FAIL
  without by how much.
- **Import is one file, in memory.** S3 delivers logs as thousands of small files.
- **`diff` on two ordinary runs prints `0 compared: 0 the same`**; `compare` on two identical runs says `+0%` and
  never "no change".
- **Three recorders** (`run`, `proxy`, `watch`), eighteen commands in one list, a help line from before 1.0.
- **What acting on the findings would save is arithmetic** (`fixes.rs`), stated as fact, and nothing can test it.

What the product cannot answer today, in order of how much it matters: whether a program survives its store
failing (nothing injects a fault; a retry is seen only when the store happens to fail); what an engine other than
SlateDB, Iceberg or Delta is doing; what a workload costs beyond request prices; anything about connections.

## 14.0

**Run your program against an S3 that misbehaves, and sonde says whether it survived.**

Thirteen versions observed. sonde already sits in every request's path, already knows a multipart upload from a
conditional write from a commit, and already reads retries, unfinished uploads, lost races and orphaned files as
operations. It has never made one of them happen. 14.0 does:

| | What | Size |
|---|---|---|
| Faults | Rules in the proxy, by operation and key: 503 SlowDown, 500, added latency, a rate per prefix at S3's documented figures. | M |
| Hard faults | The answer lost after the store acted (the ambiguous write); a body cut short; a failed CompleteMultipartUpload. | M |
| Verdicts | What the program did about it, from what the store saw: retried or not, how many times, how far apart (backoff and jitter measured), how much it multiplied the load, what it left behind, whether it wrote twice, how it exited. One line a scenario: survived, degraded, broke. | M |
| `sonde run --trial` | One clean run, then one run per scenario chosen from what the clean run did (it uploaded in parts: fail the completion; it wrote conditionally: lose the race). Nothing to configure. | L |
| CI | `[[scenario]]` in `sonde.toml`; `check` rows such as `pass  survives SlowDown`. | S |
| The app | A run's trials beside it on the Runs pane; a scenario's session is a session like any other, its injected requests marked. No new place. | M |

What goes, so that it is not more: `--no-delete` and `--read-only` become two rules of the same engine; `proxy`
folds into `watch`; `diff` into `compare`; `export --otlp` into `show -f otlp`; the estimated "acting on N findings"
line gives way to a measured one wherever a trial ran. Three commands fewer.

The risks, named: a rule that counts ("the third UploadPart") is a race in a concurrent program, so rules match by
operation and key and any choice is seeded; "survived" can only mean "the store's state is whole and the program
exited 0"; a trial re-runs the command, so it needs a prefix of its own or it writes into real data; records gain
an `injected` field that old sessions lack. Cut in this order if it does not fit: choosing scenarios automatically
(keep the named ones), then the cut-short body.

**Before it (M0)**: everything under "What 13.0 got wrong", each with a test that fails without its fix, and the
ten small things from the outside reading (the demo shows operations; `check` names what it judged; `import` takes
a folder; Parquet is not called Delta without a log; `diff` and `compare` say what they are).

**Not in 14.0**: more engines (beyond no longer guessing); a dashboard or a daemon for production; a cost advisor;
another re-layout; a new place in the app.

The two directions not taken, and why: *understanding every engine that talks to S3* is breadth of the kind 5.0 to
7.0 gave, needs a recorded fixture per engine, and gives users of the three known engines nothing; *sonde left
running in production* puts an HTTP/1.1 proxy that buffers bodies in a path it was not built for, and lands beside
the cloud's own tools, which D-025 already declined.
