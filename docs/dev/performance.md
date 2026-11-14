# Performance

Measured on a Linux x86_64 VM, 4 vCPU (Xeon @ 2.10 GHz), no GPU, release builds. A baseline, not a benchmark:
rerun the commands to compare on your machine.

## How to reproduce

```sh
# release binary with the terminal UI and embedded bundle
(cd app && nsh build App.tsx --release)
cargo build --release -p sonde-cli --features tui

python3 scripts/perf.py target/release/sonde                    # terminal binary in a pty (pip install pyte)
(cd app && nsh test tests/perf.test.tsx --target tui --trace)   # headless UI: 100k rows, 500 scroll frames
cargo run --release -p sonde-proxy --example overhead           # proxy overhead (OVERHEAD_URL, OVERHEAD_BUCKET, AWS keys)
cargo run --release -p sonde-proxy --example scale -- 1000000   # a synthetic run of a million requests
cargo test --release -p sonde-proxy --test budgets -- --ignored # the size budgets (CI runs them)
```

The UI measures use the built-in demo endpoint (`sonde tui --demo`, bucket `demo-large`, 100 000 keys), so they
measure sonde, not the network.

## Recording proxy

The same 1 KiB GET against a local RustFS, 2 000 times direct and 2 000 through sonde (interleaved):

| | p50 | p90 | p99 |
| --- | --- | --- | --- |
| direct | 0.478 ms | 0.694 ms | 1.047 ms |
| through sonde | 0.684 ms | 0.812 ms | 1.108 ms |
| **overhead** | **+0.21 ms** | **+0.12 ms** | **+0.06 ms** |

Covers parsing, re-signing, the record and the extra loopback hop. CI runs it in the `record` job.

<a id="a-run-of-a-million-requests-800"></a>

## Large runs

**A million requests take about 350 MiB of memory** to read and analyse, in 9 s; `compare` of two, about 500 MiB
(it reads one run at a time). Memory grows with the requests: about 350 bytes each (a record's 96 bytes and what
it shares, its operation's 88, and the analysis's room), so 8 GB holds about twenty million.

Measured 2026-11-14 (22.0), release build, `/usr/bin/time` (peak resident memory), the commands once, the budgets
twice (their ranges below). The runs are the budgets' lake job with real headers (`crates/proxy/tests/support`):
a million requests, 170 MiB gzipped, 1.6 GiB of JSON lines.

| A million requests | Time | Peak memory |
| --- | --- | --- |
| `sonde show` | 8.5 s | 351 MiB |
| `sonde show -f json` | 13 s | 350 MiB |
| `sonde show -f json --requests` (1.6 GiB of JSON) | 38 s | 349 MiB |
| `sonde check --baseline` (another million) | 17 s | 391 MiB |
| `sonde compare` (two millions) | 20 s | 510 MiB |
| `sonde share` | not yet measured | |

Budgets (`crates/proxy/tests/budgets.rs`): CI runs each test optimised in a process of its own and fails it past its
peak memory (what is measured and about an eighth more; a million's is the 400 MiB 22.0 set out for), which moves
by a few percent from run to run. Time moves with a shared runner: it is said in the job's summary and fails only
past several times what it takes.

| Budget | Measured (21.0) | Measured | Peak memory, held at | Time, held at |
| --- | --- | --- | --- | --- |
| a million read and analysed (`show`) | 1,107–1,126 MiB, 8.4–9.3 s | 333–368 MiB, 8.4–9.1 s | 400 MiB | 45 s |
| two runs of a million compared | 1,269–1,357 MiB, 23–25 s | 490–548 MiB, 20–21 s | 560 MiB | 120 s |
| an outage of 300 thousand | 511–517 MiB, 2.1–2.2 s | 199–211 MiB, 1.8–1.9 s | 240 MiB | 30 s |
| small multipart uploads, 300 thousand | 430–441 MiB, 2.6–3.1 s | 234–235 MiB, 2.8 s | 270 MiB | 30 s |
| the app's first view of 100 thousand (records whole) | 234–242 MiB, 0.7–1.0 s | 217–221 MiB, 1.0–1.1 s | 250 MiB | 6 s |
| a sorted page of operations | 52 ms | 48–57 ms | | 1.5 s |
| the Time pane of a million: its rows (bounded by its slices, under 2 MiB) and a tenth reckoned alone | 711–715 MiB, 6.6–7.4 s | 270–272 MiB, 4.9 s | 310 MiB | 45 s |

How:
- **The record** (`reqlog::Record`, 96 bytes): its kind, method and bucket are interned `Name`s (2 bytes each); a
  size and the first byte's time take 8 bytes each; its query and headers are one list behind a pointer that records
  holding the same share; what few have (an error's body, a fault's mark) and what no analysis reads (the URL, the
  endpoint, the region, the store's ids) is behind another, there only when needed.
- **The reading**: the file is streamed and parsed on every core; a lean reading (what `show`, `check` and `compare`
  use) borrows a line's text and makes only what it keeps, the headers an analysis reads; repeated text is shared.
- **The operation** (88 bytes): one request that went through is its record's names in place; any other's phrases,
  notes and outcome are behind one pointer; titles are made when asked for.
- **The analysis**: counts, signals, profile, operations and findings are computed once, one after another (threads
  each kept what they let go: a fifth more memory, for 2.5 s less); work is done on sorted indexes and tables by id,
  not per-request maps and copies; a run's traces keep their counts and words, not the requests they name.
- **The output**: `show -f json` writes the requests only with `--requests`; JSON is written as it is made; the app
  gets a page of operations at a time; `compare` reads one run at a time. The app also caches each finished
  run's figures in the folder's index and reads a live recording incrementally.

## Terminal binary

`scripts/perf.py`, 120×40 pty, 20.0.0's `sonde-linux-x86_64` (CI's build), two runs:

| Measure | Result |
| --- | --- |
| CLI start (`sonde --version`), mean of 10 | 5 ms |
| TUI start to first frame (`sonde tui --demo`) | 110–180 ms |
| Load 100 000 rows in Buckets (flat, *Load all*: 100 ListObjectsV2 pages) | 0.57–0.63 s |
| Memory (VmRSS): idle / 100 000 rows loaded / after paging | 51 / 106 / 109 MiB |
| Page Down presses in 10 s, each answered with a frame | 151–154 (≈ 5 000 rows) |
| `--fps` while paging | 46 fps (bounded by the script's key rate), frame 2.0 ms, p95 3.0 ms, ≈ 160 cells written |

Binary: 79.7 MB (LTO thin, JS engine, the app's bundle and the viewer included), linking only `libgcc_s`, `libm`,
`libc` ([packaging](packaging.md#the-terminal-binary-has-no-gui-dependency)).

## Headless UI

`nsh test --target tui --trace`, debug host (v0.1.0): Browse › demo-large, flat, load all 100 000 rows, scroll the
table 500 frames down and back.

| Measure | Result |
| --- | --- |
| Load 100 000 rows (debug native module) | 3.53 s |
| 500 scroll frames down / back up | 3.14 s (6.3 ms/frame) / 3.16 s |
| Commits during the scroll | 210 |
| Mount items per commit | median 16, p95 17, max 153 (the first frame after *Load all*) |

The DataTable is virtual: JS holds one 400-row window, fetched from Rust's `Listing` by range.

## Opening the Time pane

Headless `nsh test --trace`, the example run (172 requests), Native Shell `v0.5.0`, three runs:

| The press's commit | Largest commit after it | All commits | Mount items |
|---|---|---|---|
| 207 ms (107 ms binding rows in JS) | 1.17–1.25 s (1.15–1.21 s JS) | 1.35–1.46 s | 13,846 |

The pane shows after about 0.2 s; the rest is JavaScript building every chart's rows in one turn — sonde's to
shorten (build charts as they come into view).

## Long jobs

Probes, multipart scans, capacity and layout scans run in Rust on tokio and stream progress; the UI never blocks.
Per job: 16 requests in flight (`settings.concurrency`), optional `settings.rate-limit`, exponential backoff with
jitter on 503 SlowDown and throttling ([configuration](../configuration.md)).

## Not measured

- **macOS app** (cold start, window memory, chart frame time): `nsh package --target macos` runs only on macOS.
- **Real endpoints**: list and probe times follow the endpoint's latency; the request inspector shows each request's time.
