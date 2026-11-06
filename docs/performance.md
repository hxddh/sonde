# Performance

All numbers below were measured on the development machine for v0.1.0:
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
