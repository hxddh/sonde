# 12.0 reviewed, 13.0 defined

A review of 12.0.0 before 13.0: the bugs found (by reading every crate and every screen, and by driving the app on
a real window), what Native Shell's newest commit offers, where the interface is rough, and what 13.0 is. Each item
names where it was; the fixes are the 13.0 milestones in the commit log.

## Bugs in 12.0

### Recording, replay, import, CLI

| | Where | What went wrong |
|---|---|---|
| High | `s3raw/client.rs`, `proxy/lib.rs` | The proxy cut every exchange off after 60 s: a long download was truncated, a long upload answered 502. |
| High | `proxy/lib.rs`, `op.rs` | `--no-delete` let `DELETE //bucket/key` through (no bucket parsed, operation "Unknown", forwarded). |
| High | `cmd/record.rs` | `watch --logs` read up to two hours of logs again once it had seen 50,000 files. |
| High | `proxy/replay.rs` | A replay's "the prefix is empty" check was skipped when its listing failed; the cleanup then deleted everything under the prefix. |
| Medium | `proxy/import.rs` | CloudTrail imports dropped `requestParameters` (every multipart upload "not completed", every listing "again and again"); access-log imports counted lifecycle lines as requests and failed a whole file on one short line; HAR imports dropped every request header. |
| Medium | `cmd/sessions.rs` | `sonde import x.json.gz` failed (the app and `watch` read gzip). |
| Medium | `proxy/report.rs` | `share --redact` kept the requester's ARN in `session.json`. |
| Medium | `proxy/chunked.rs` | An `aws-chunked` chunk size near `usize::MAX` panicked the connection. |
| Medium | `proxy/callers.rs` | The Node caller hook's path broke `NODE_OPTIONS` on Windows. |
| Medium | `proxy/op.rs`, `replay.rs` | A replay confined to a prefix still replayed `PUT /bucket?inventory…` (read as CreateBucket). |
| Medium | `proxy/episode.rs`, `profile.rs` | ListObjects v1 pages read as one listing repeated; Compare showed 0 for a prefix outside the other run's top eight. |
| Medium | `cmd/record.rs` | Exit 0 when every replay failed, and when `watch --logs --once` could not list or was over budget; 1 instead of 128+signal. |
| Low | several | Three definitions of a percentile; cost counted requests that never reached the store; `sonde export` panicked on a corrupt latency; "Copy as aws cli" decoded twice; `-f` formats and `--tolerance` accepted where they do nothing; an over-budget pass said nothing without `--alert`. |

### Core, the app's Rust, probes, decoders

| | Where | What went wrong |
|---|---|---|
| High | `sonde-nsh/state.rs`, `sonde-core/sonde.rs`, `config.rs` | A `config.toml` that failed to load (one endpoint's CA file missing, a typo) became an empty configuration, and the next change saved over the file. Saves wrote the copy read at start, so an edit made meanwhile was lost; the temporary file had one name and the umask's mode. |
| High | `sonde-core/jobs.rs`, `probes/runner.rs`, `services/browse.rs` | Cancelling a job dropped it where it stood: probe objects, uploads and temporary buckets (one under Object Lock) stayed, as did a download's partial file. |
| High | `sonde-core/sonde.rs`, `cmd/mod.rs` | Starting the demo replaced a configured endpoint named `demo`, and the next save left it out; `--url`'s endpoint and `--rate`/`--concurrency` were saved by `sonde tui`. |
| High | `decoders/slatedb.rs` | The WAL gap scan walked every integer between the smallest and largest id (two crafted keys: 2^64 steps). |
| High | `probes/suite/*` | Judges ignored failed setup: on a bucket that cannot be written, "list after write" passed with "all 0 writes listed". |
| Medium | `sonde-nsh/sessions.rs` | "Open the code" trusted the folder named by the session file itself, and opened by a deny-list of extensions. |
| Medium | `sonde-nsh/api.rs` | Read-only mode did not cover compatibility probes, which write, delete and create buckets. |
| Medium | `sonde-core/jobs.rs`, `decoders/lakefs.rs` | A job that panicked stayed "running" (a lakeFS name cut inside a character did); finished jobs were never dropped. |
| Medium | `services/mpu.rs`, `sonde.rs`, `exec.rs`, `du.rs`, `browse.rs` | The lifecycle rule always had one id; importing AWS profiles could replace an endpoint; the audit entry was written only after the requests; a capacity scan hid a failed multipart scan and failed whole on 403; downloads of one name overwrote each other. |
| Low | several | STS with a port; `cmd /C` quoting; rotated profile keys; corrupt `titles.json`/`baselines.json` read as empty then overwritten; an unknown recording policy meant "allow"; the demo server panicked on a reversed range. |

### The interface

- The shared page (`sonde share`) showed a function's source instead of "Loading…" and never its error; a press on a
  request's dot opened nothing; an output line selected the wrong row once the table was sorted.
- A run past its p95 budget read `verdict.p95 12% past its baseline`, and its figure had no unit.
- A job's progress bar never passed 1% (a fraction given to a percentage).
- Rename, saved unchanged, renamed the session to its program's name and hid its arguments.
- A session in a window of its own never grew while live, had no toasts or rename dialog, and laid itself out by
  the focused window's size; every toast showed in every window.
- A failed comparison, a denied object, a failed listing of buckets, versions or runs showed "Loading…" forever or
  the empty state's words.
- The bucket tree showed one endpoint's prefixes under another's bucket of the same name; Refresh never read
  prefixes again; a prefix with `#` in its name could not be chosen.
- An empty run opened within eight seconds was deleted under the reader; Run's clock flashed `-1:-1` and had no
  hours; a second run's output was appended under the first.
- `:` then a place did nothing on the terminal's home list.
- Durations: a three-minute upload read `184532 ms`; `1000 ms`, `1024 KiB`, `7200.0 s`; three percent formats;
  `+0%` for a change under half a percent.
- Commands with `KEY=value` reached screen-reader labels, the palette's hints and the Run history unhidden.
- Sorting a column of dates or keys did nothing; the probe matrix's sortable title did not sort.
- Chinese: one concept in several words (桶/存储桶/Bucket, 分片/分段, 任务/作业, 重放/回放); English left in tags and
  labels; `verdict.p95` missing.

## Native Shell at its newest commit

sonde was at `1662115` (Nov 8); the newest is `ec8430f` (106 commits later). No component sonde draws itself has an
upstream replacement: `nsh/base` is still behavior without a look, by design (the guide tells an app to own that
layer), there is no icon component, and Plot is unchanged (no log scale, no press, no brush). What the range gives:

| Take | Why |
|---|---|
| Tooltips on buttons | `Button` dropped its `tooltip` at the old commit: none of sonde's hints or shortcuts ever showed. |
| A column fixed at the end, and presses inside cells | Row actions in the tables that had none. |
| What a drag carries, before the drop | The drop hint names what will open, and refuses what cannot. |
| Fewer test flakes, no theme warnings, culling of far content, faster tables and editors | Free. |
| Links as terminal hyperlinks | Turned off for `s3://` places in `sonde tui`, which are not addresses a terminal can open. |

FINDINGS 2 was out of date (the host has `main_with_args`), but it still loads its bundle from a path, so sonde's
own loop stays. The rest of FINDINGS stands at the newest commit.

## Where the interface is rough

Counted in the code and seen on the screens:

- **The window reflows under the hand.** The sessions list vanished whenever an operation was chosen; a bucket's
  overview tiles appeared as outlines and then went; a search field came and went with the number of sessions; the
  header's chart changed place at 1180 points. Six unrelated breakpoints.
- **One thing drawn several ways.** Six renderings of a figure, five section headings, four disclosures, four back
  controls, three tab controls, three clocks in lists, five dialog footers, nine hand-drawn banners; 23 cards
  written out by hand beside 22 from the kit; eight control heights where the standard allows two.
- **The chart frame is not one frame.** Three left edges, six row pitches, five corner radii, four crosshairs,
  legends that start beside their plots, seven charts without a tooltip, fewer/more in green/red on one chart and
  blue/amber on the next.
- **Rows of unlike controls.** The toolbar's six controls in four shapes; a browse row of four heights; sheets
  whose controls sit on their bottoms.
- **An empty sidebar**: three rows and 224 points of nothing.
- **States that lie.** Loading, empty and failed share words.
- **Words.** `…` on some commands that open something and not on others; Delete/Remove/Clear/Abort;
  Refresh/Scan/Detect/Check; "Req".

## 13.0

One sentence: **every state true, every screen still, one way to draw each thing — and a session's time seen
request by request.**

| Milestone | What |
|---|---|
| M0 | Every bug above, each with a test. |
| M1 | Native Shell at `ec8430f`; tooltips, end-fixed row actions, the drag named before the drop. |
| M2 | The kit made whole and the screens made still: one figure, heading, row, disclosure, notice, dialog footer, page header, state of a resource (loading, empty, failed); two tab controls; three columns that stay; a bucket's four tiles always there; one formatter each for durations, percents, costs and clocks; the sidebar lists the programs with their standing. |
| M3 | Seeing: one chart frame for every chart; a session's requests as lanes (what ran at once, what waited); the Time pane as a board on one time axis with a stretch chosen once; the live view from the first request. |
| M4 | Words (a glossary in both languages), docs, screens. |

Not in 13.0, on purpose: no new place, no new settings page, no new file format.
