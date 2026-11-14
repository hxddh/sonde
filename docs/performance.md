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
cargo run --release -p sonde-proxy --example scale -- 1000000   # a synthetic session of a million requests
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

<a id="a-session-of-a-million-requests-800"></a>

## Large sessions

**A million requests take about 1.1 GiB of memory** to read and analyse; `compare` of two, 1.3 GiB (it reads one
session at a time). Memory grows with the requests: about 1.2 KB each, so 8 GB holds about six million.

Measured 2026-11-14, release build, `/usr/bin/time` (peak resident memory), two runs each within 2%. The sessions are
the budgets' lake job with real headers (`crates/proxy/tests/support`): a million requests, 170 MiB gzipped, 1.6 GiB
of JSON lines.

| A million requests | Time | Peak memory |
| --- | --- | --- |
| `sonde show` | 7.6 s | 1.1 GiB |
| `sonde show -f json` | 36 s | 1.1 GiB |
| `sonde check --baseline` (another million) | 14 s | 1.2 GiB |
| `sonde compare` (two millions) | 19 s | 1.3 GiB |
| `sonde share --html` | 9 s | 1.0 GiB |

Budgets (`crates/proxy/tests/budgets.rs`): CI runs each test optimised in a process of its own and fails it past its
peak memory (what is measured and about an eighth more), which is the same from run to run. Time moves with a shared
runner: it is said in the job's summary and fails only past several times what it takes.

| Budget | Measured | Peak memory, held at | Time, held at |
| --- | --- | --- | --- |
| a million read and analysed (`show`) | 1,141–1,160 MiB, 7.3–7.6 s | 1,300 MiB | 45 s |
| two runs of a million compared | 1,282 MiB, 18 s | 1,450 MiB | 120 s |
| an outage of 300 thousand | 521–527 MiB, 1.7 s | 600 MiB | 30 s |
| small multipart uploads, 300 thousand | 437 MiB, 2.3 s | 500 MiB | 30 s |
| the app's first view of 100 thousand (629 KiB sent) | 236–243 MiB, 0.6–0.7 s | 280 MiB | 6 s |
| a sorted page of operations | 55 ms | | 1.5 s |

How: the file is streamed and parsed on every core; records keep only the headers an analysis reads, with repeated
text shared; operations, signals and profile are computed once, in parallel; JSON is written as it is made; the app
gets a page of operations at a time; `compare` reads one session at a time. The app also caches each
finished run's figures in `sessions/.index.jsonl` and reads a live recording incrementally.

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

Headless `nsh test --trace`, the example session (172 requests), Native Shell `v0.5.0`, three runs:

| The press's commit | Largest commit after it | All commits | Mount items |
|---|---|---|---|
| 207 ms (107 ms binding rows in JS) | 1.17–1.25 s (1.15–1.21 s JS) | 1.35–1.46 s | 13,846 |

The pane shows after about 0.2 s; the rest is JavaScript building every chart's rows in one turn — sonde's to
shorten (build charts as they come into view).

## Long jobs

Probes, multipart scans, capacity and layout scans run in Rust on tokio and stream progress; the UI never blocks.
Per job: 16 requests in flight (`settings.concurrency`), optional `settings.rate-limit`, exponential backoff with
jitter on 503 SlowDown and throttling ([configuration](configuration.md)).

## Not measured

- **macOS app** (cold start, window memory, chart frame time): `nsh package --target macos` runs only on macOS.
- **Real endpoints**: list and probe times follow the endpoint's latency; the request inspector shows each request's time.
