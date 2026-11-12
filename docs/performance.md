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

Synthetic sessions with real headers (a million: 124 MiB gzipped, 1.5 GiB of JSON lines).

17.0 (other builds running, load 3–7):

| | Time, peak memory |
| --- | --- |
| `show`, 1 million | 12 s, 1.11 GiB |
| `show`, 2 million | 26 s, 2.16 GiB |
| `compare`, 2 × 1 million | 24 s, 1.16 GiB |
| `compare`, 2 × 2 million | 39 s, 2.25 GiB |
| `show`, 300 thousand, 1 000-character keys | 2.4 s, 0.40 GiB |

About 1.1 KB a request (8 GB holds about seven million), mostly the record's 472 bytes of fields.

16.0 (not remeasured since):

| | 10 thousand | 100 thousand | 1 million |
| --- | --- | --- | --- |
| `sonde check --baseline` | 0.2 s, 36 MiB | 1.5 s, 236 MiB | 17 s, 2.0 GiB |
| `sonde show -f json` | 0.3 s, 33 MiB | 3.5 s, 212 MiB | 34 s, 1.8 GiB |
| `sonde share --otlp` | 0.3 s, 33 MiB | 3 s, 187 MiB | 32 s, 1.7 GiB |
| `sonde share --html` | 0.1 s, 0.85 MB page | 0.9 s, 0.85 MB page | 9.9 s, 0.85 MB page |
| `sonde sessions`, one such file | at once | at once | at once (5.7 s once, when first counted) |
| The app's first view | 610 KiB | 640 KiB | 634 KiB |

Budgets (`crates/proxy/tests/budgets.rs`, run optimised in CI):

| Budget | Measured |
| --- | --- |
| a million requests read and analysed: 15 s, 1.5 GiB | 11 s, 1.1 GiB |
| two runs of a million compared: 40 s, 1.5 GiB | 28 s, 1.2 GiB |
| an outage, and small multipart uploads, 300 thousand each: 10 s | 2 s |
| the app's first view of 100 thousand: 2 s, under 2 MiB sent | 0.9 s |
| a sorted page of operations: 500 ms | — |

How: the file is streamed and parsed on every core; records keep only the headers an analysis reads, with repeated
text shared; operations, signals and profile are computed once, in parallel; JSON is written as it is made; the app
gets a page of operations at a time; `compare` and `diff` read one session at a time. The app also caches each
finished run's figures in `sessions/.index.jsonl` and reads a live recording incrementally.

## Terminal binary

`scripts/perf.py`, 120×40 pty (v0.1.0):

| Measure | Result |
| --- | --- |
| CLI start (`sonde --version`), mean of 10 | 5 ms |
| TUI start to first frame (`sonde tui --demo`), 3 runs | 66, 59, 68 ms |
| Load 100 000 rows in Browse (flat, *Load all*: 100 ListObjectsV2 pages) | 0.51 s |
| Memory (VmRSS): idle / 100 000 rows loaded / after paging | 36 / 91 / 95 MiB |
| Page Down presses in 10 s, each answered with a frame | 163 (≈ 5 300 rows) |
| `--fps` while paging | 49–50 fps (bounded by the script's key rate), frame 1.15 ms, p95 1.66 ms, ≈ 140 of 4 800 cells written |

Binary: 55.7 MB (x86_64, LTO thin, JS engine and bundle included), linking only `libgcc_s`, `libm`, `libc`
([packaging](packaging.md#the-terminal-binary-has-no-gui-dependency)).

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
