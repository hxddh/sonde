# Performance

All numbers below were measured on the development machine (the UI numbers for v0.1.0, the recording
proxy for v1.0.0):
a Linux x86_64 VM, 4 vCPU (Xeon @ 2.10 GHz), no GPU, in a 120×40
pseudo-terminal. They are a baseline, not a benchmark suite: rerun the
commands below on your machine to compare.

## How to reproduce

```sh
# the release binary, with the terminal UI and the embedded bundle
(cd app && nsh build App.tsx --release)
cargo build --release -p sonde-cli --features tui

# 1. the real binary in a pty: start time, memory, 100k rows, --fps
pip install pyte
python3 scripts/perf.py target/release/sonde

# 2. the headless UI test: 100k rows in the DataTable, 500 scroll frames,
#    with commit tracing (from app/)
nsh test tests/perf.test.tsx --target tui --trace
```

Both use the built-in demo endpoint (`sonde tui --demo`): an in-process S3
server whose `demo-large` bucket holds 100 000 synthetic keys, so the numbers
measure sonde and not the network. Against a real endpoint the 100k load is
bound by ListObjectsV2 round trips (100 pages of 1 000).

## Terminal binary (`scripts/perf.py`, release build)

| Measure | Result |
| --- | --- |
| CLI start (`sonde --version`), mean of 10 | 5 ms |
| TUI start to first frame (`sonde tui --demo`), 3 runs | 66, 59, 68 ms |
| Load 100 000 rows in Browse (flat, *Load all*: 100 ListObjectsV2 pages, parsed in Rust) | 0.51 s |
| Memory (VmRSS): idle on Home / 100 000 rows loaded / after paging | 36 / 91 / 95 MiB |
| Page Down presses sent in 10 s, each one answered with a frame | 163 (≈ 5 300 rows travelled) |
| `--fps` overlay while paging | 49–50 fps, frame 1.15 ms, p95 1.66 ms (apply 0.11, draw 0.91, flush 0.12), ≈ 140 cells/frame |

The fps figure is bounded by the key rate of the script (one Page Down every
~60 ms), not by sonde: a frame costs ~1.2 ms, p95 under 2 ms, and only the
changed cells (~140 of 4 800) are written to the terminal.

Binary size: 55.7 MB for `sonde` (x86_64, release, LTO thin, JS engine and UI bundle included). It links only `libgcc_s`, `libm` and `libc` (see
[packaging](packaging.md#the-terminal-binary-has-no-gui-dependency)).

## Headless UI test (`nsh test --target tui --trace`, debug host)

The test opens Browse › demo-large, switches to flat listing, loads all
100 000 rows, then scrolls the object DataTable 5 000 points down and back
(500 frames each way).

| Measure | Result |
| --- | --- |
| Load 100 000 rows (debug build of the native module) | 3.53 s |
| 500 scroll frames down | 3.14 s (159 frames/s, 6.3 ms/frame) |
| 500 scroll frames back up | 3.16 s |
| Commits during the scroll (trace) | 210 |
| Mount items per commit (trace) | median 16, p95 17, max 153 |

The DataTable is virtual: only the visible rows exist as views, and rows are
fetched from the Rust `Listing` by index range (`rows(start, count)`), so the
JS holds one 400-row window regardless of the listing size. The 153-item commit is
the first frame after *Load all* (the header, the footer and the first page of
rows); every scroll commit after it touches ~16 items.

## Recording proxy (`sonde run`, `sonde proxy`)

The same 1 KiB GET against a local RustFS, 2 000 times directly and 2 000
times through the proxy (interleaved rounds, release build):

| | p50 | p90 | p99 |
| --- | --- | --- | --- |
| direct | 0.478 ms | 0.694 ms | 1.047 ms |
| through sonde | 0.684 ms | 0.812 ms | 1.108 ms |
| **overhead** | **+0.21 ms** | **+0.12 ms** | **+0.06 ms** |

The overhead covers parsing, re-signing (one HMAC chain), the record and the
extra loopback hop. Reproduce with
`cargo run --release -p sonde-proxy --example overhead` (it reads
`OVERHEAD_URL`, `OVERHEAD_BUCKET` and the AWS key variables); CI runs it in
the `record` job.

## A session of a million requests (8.0.0)

`cargo run --release -p sonde-proxy --example scale -- 1000000` writes a
synthetic session of a million requests (a mix of PUT, HEAD, LIST and GET over
97 prefixes), compresses it, reads it back and analyses it, timing each step
(same machine, release build):

| Step | Time |
| --- | --- |
| Write 1 000 000 records (375 MiB of JSON lines) | 1.3 s |
| Compress (gzip level 6) | 1.7 s → 10 MiB (35×) |
| Read back | 1.5 s |
| Operations | 1.6 s |
| Findings | 1.6 s |
| Where the time went (profile, chains) | 1.8 s |
| Peak memory | 1.6 GiB |

The app reads a session once per version of its file and keeps the analysis;
a live recording is read in steps (only the lines appended since the last
look), and each finished run's figures are kept in a small file beside it
(`sessions/<name>.summary.json`), so a program's history of 30 runs reads no
session twice.

## Sessions of any size (16.0.0)

The 8.0.0 figures above are of records without headers. A recorded request has
fifteen or so (a user agent, a signature, ids): a million of them are 124 MiB
compressed and 1.5 GiB of JSON lines. 15.0 read that in 45 s and 7.3 GB, could
not write it as JSON at all, and took minutes on two shapes (an outage whose
failures are never retried; small files uploaded in parts). 16.0, same
sessions, same machine (4 cores), release build:

| | 10 thousand | 100 thousand | 1 million | 15.0 at 1 million |
| --- | --- | --- | --- | --- |
| `sonde show` | 0.1 s, 30 MiB | 0.7 s, 203 MiB | 7.4 s, 1.8 GiB | 45 s, 7.3 GB |
| `sonde check --baseline` | 0.2 s, 36 MiB | 1.5 s, 236 MiB | 17 s, 2.0 GiB | 99 s, 11 GB |
| `sonde show -f json` | 0.3 s, 33 MiB | 3.5 s, 212 MiB | 34 s, 1.8 GiB | could not (over 12 GB) |
| `sonde share --json`, `sonde export` | 0.3 s, 33 MiB | 3 s, 187 MiB | 32 s, 1.7 GiB | could not |
| `sonde share --html` | 0.1 s, 0.85 MB page | 0.9 s, 0.85 MB page | 9.9 s, 0.85 MB page | 328 MB page |
| `sonde sessions`, one such file | at once | at once | at once (5.7 s the one time it is counted) | 24 s, 4.8 GB, every call |
| The app's first view | 610 KiB | 640 KiB | 634 KiB | 351 MB |

What changed: a session is read as a stream and parsed on every core, never
held as text; the analyses read a lean record (the headers no finding reads
are not kept, and a header's name and value are shared between the records
that repeat them); the operations, the signals and the profile are worked out
at once on separate threads, once per command; the two quadratic loops are
linear (300 thousand requests of either shape: 1.9 s, where 100 thousand
took 374 s and 22 s); JSON is written as it is
made, a request at a time from the file; the app is sent a page of operations
and asks for the rest in ranges.

The budgets are a test, `crates/proxy/tests/budgets.rs`, which CI runs
optimised (`cargo test --release -p sonde-proxy --test budgets -- --ignored`):
a million requests read and analysed in 15 s and 2 GiB of peak memory (8 s and
1.75 GiB here), an outage and small uploads of 300 thousand each in 10 s (2 s),
the app's first view of 100 thousand in 2 s (0.9 s).

## Memory, not only output (17.0.0)

Same generator and machine, release build. 17.0 times were taken with other builds running (load 3–7).

| | 16.0 | 17.0 |
| --- | --- | --- |
| `show`, 1 million | 7.6 s, 1.74 GiB | 12 s, 1.11 GiB |
| `show`, 2 million | 14.9 s, 3.4 GiB | 26 s, 2.16 GiB |
| `compare`, 2 × 1 million | 23.8 s, 4.74 GiB | 24 s, 1.16 GiB |
| `compare`, 2 × 2 million | 65 s, 9.4 GiB | 39 s, 2.25 GiB |
| `show`, 300 thousand, 1 000-character keys | 4.5 KB a request | 2.4 s, 0.40 GiB (1.4 KB) |

About 1.1 KB a request: 8 GB holds about seven million. How: headers kept by name (only those an analysis
reads); repeated text (operation, method, endpoint, bucket, region, query, key) shared; titles said from their
phrases; `compare` and `diff` read one session at a time. What is left is mostly the record's 472 bytes of fields.

Budgets test: a million in 15 s and 1.5 GiB (measured 11 s, 1.1 GiB); two runs of a million compared in 40 s and
1.5 GiB (28 s, 1.2 GiB).

## Opening the Time pane

The Time pane builds its charts when it is opened. Measured headless with `nsh test --trace` on the example
session (172 requests), from the press on Time until the pane is still, three runs each:

| Native Shell | The press's own commit | The largest commit after it | All commits | Mount items |
|---|---|---|---|---|
| `744d950` (15.0) | 212 ms (121 ms binding rows in JS) | 1.17 to 1.20 s (1.15 s of it JS) | 1.37 to 1.40 s | 13,940 |
| `v0.5.0` (16.0) | 207 ms (107 ms) | 1.17 to 1.25 s (1.15 to 1.21 s JS) | 1.35 to 1.46 s | 13,846 |

0.5.0 makes views out of sight over several frames. It did not move this: the one long call is JavaScript
building every chart's rows in one turn, before any view is made, and it is as long as it was. The pane is shown
after about 0.2 s either way; the second it takes to be whole is sonde's own to shorten (charts built as they come
into view), not Native Shell's.

## Long jobs

Jobs (probes, multipart scan, capacity, layout scans) run in Rust on tokio and
stream progress events to the UI; the UI never blocks on them.
Concurrency is per job (default 16 requests in flight, `settings.concurrency`)
and optionally rate-limited (`settings.rate-limit`); 503 SlowDown and
throttling errors back off exponentially with jitter (see
[configuration](configuration.md)).

## Not measured here

- **macOS app** (GUI cold start, window memory, Canvas chart frame time): the
  development machine is Linux, and `nsh package --target macos` runs only on
  macOS. The release workflow builds the app on a macOS runner; numbers for it
  are to be added from that runner.
- **Real endpoints**: list and probe times depend on the endpoint's latency;
  the request inspector shows the time of every request.
