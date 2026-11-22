# Performance

Measured on a Linux x86_64 VM, 4 vCPU (Xeon @ 2.10 GHz), no GPU, release builds. A baseline, not a benchmark:
rerun the commands to compare on your machine.

## How to reproduce

```sh
cargo build --release -p sonde-cli
bash scripts/ui-tests.sh perf                                   # headless UI: 100k rows, 500 scroll frames
cargo run --release -p sonde-proxy --example overhead           # proxy overhead (OVERHEAD_URL, OVERHEAD_BUCKET, AWS keys)
cargo run --release -p sonde-analysis --example scale -- 1000000   # a synthetic run of a million requests
```

The size budgets, each in a process of its own as CI runs them (a test's peak memory is then its own):

```sh
for t in $(cargo test --release -p sonde-analysis --test budgets -- --ignored --list --format terse | sed -n 's/: test$//p'); do
  cargo test --release -p sonde-analysis --test budgets -- --ignored --nocapture --exact "$t"
done
```

The UI measures use the built-in demo endpoint (bucket `demo-large`, 100 000 keys), so they measure sonde, not the
network.

## Recording proxy

The same 1 KiB GET against a local RustFS, 2 000 times direct and 2 000 through sonde (interleaved):

| | p50 | p90 | p99 |
| --- | --- | --- | --- |
| direct | 0.478 ms | 0.694 ms | 1.047 ms |
| through sonde | 0.684 ms | 0.812 ms | 1.108 ms |
| **overhead** | **+0.21 ms** | **+0.12 ms** | **+0.06 ms** |

Covers parsing, re-signing, the record and the extra loopback hop. CI runs it in the `record` job.

## Large runs

**A million requests take about 350 MiB of memory** to read and analyse, in 9 s; `show --since` of two, about 500 MiB
(it reads one run at a time). Memory grows with the requests: about 350 bytes each (a record's 96 bytes and what
it shares, its operation's 88, and the analysis's room), so 8 GB holds about twenty million.

Measured 2026-11-14 (22.0), release build, `/usr/bin/time` (peak resident memory), the commands once, the budgets
twice (their ranges below). The runs are the budgets' lake job with real headers (`crates/analysis/tests/support`):
a million requests, 170 MiB gzipped, 1.6 GiB of JSON lines.

| A million requests | Time | Peak memory |
| --- | --- | --- |
| `sonde show` | 8.5 s | 351 MiB |
| `sonde show -f json` | 13 s | 350 MiB |
| `sonde show -f json --requests` (1.6 GiB of JSON) | 38 s | 349 MiB |
| `sonde check --baseline` (another million) | 17 s | 391 MiB |
| `sonde show --since` (two millions) | 20 s | 510 MiB |

Budgets (`crates/analysis/tests/budgets.rs`): CI runs each test optimised in a process of its own and fails it past
its peak memory (what is measured and about an eighth more), which moves by a few percent from run to run. Time
moves with a shared runner: it is said in the job's summary and fails only past several times what it takes.

| Budget | Measured | Peak memory, held at | Time, held at |
| --- | --- | --- | --- |
| a million read and analysed (`show`) | 333–368 MiB, 8.4–9.1 s | 400 MiB | 45 s |
| two runs of a million compared | 490–548 MiB, 20–21 s | 560 MiB | 120 s |
| an outage of 300 thousand | 198–216 MiB, 1.7–2.0 s | 240 MiB | 30 s |
| small multipart uploads, 300 thousand | 195–199 MiB, 1.9–2.0 s | 270 MiB | 30 s |
| the app's first view of 100 thousand (records whole) | 217–221 MiB, 1.0–1.1 s | 250 MiB | 6 s |
| a sorted page of operations | 48–57 ms | | 1.5 s |
| the Time pane of a million: its rows (bounded by its slices, under 2 MiB) and a tenth reckoned alone | 274–282 MiB, 3.5–3.9 s | 310 MiB | 45 s |

How:
- **The record** (`reqlog::Record`, 96 bytes): its kind, method and bucket are interned `Name`s (2 bytes each); a
  size and the first byte's time take 8 bytes each; its query and headers are one list behind a pointer that records
  holding the same share; what few have (an error's body, a fault's mark) and what no analysis reads (the URL, the
  endpoint, the region, the store's ids) is behind another, there only when needed.
- **The reading**: the file is streamed and parsed on every core; a lean reading (what `show` and `check`
  use) borrows a line's text and makes only what it keeps, the headers an analysis reads; repeated text is shared.
- **The operation** (88 bytes): one request that went through is its record's names in place; any other's phrases,
  notes and outcome are behind one pointer; titles are made when asked for.
- **The analysis**: counts, signals, profile, operations and findings are computed once, one after another (threads
  each kept what they let go: a fifth more memory, for 2.5 s less); work is done on sorted indexes and tables by id,
  not per-request maps and copies; a run's traces keep their counts and words, not the requests they name.
- **The output**: `show -f json` writes the requests only with `--requests`; JSON is written as it is made; the app
  gets a page of operations at a time; `show --since` reads one run at a time. The app also caches each finished
  run's figures in the folder's index and reads a live recording incrementally.

## The plan

A finding's time saved is its requests' parts of the waiting ([findings](../cli.md#findings)), reckoned
in one pass over the run and summed per finding; 29.0 reckoned the run again for each finding (its intervals
collected and sorted, its cost and bytes added up), a fifth more time at a million. Measured 2026-11-22 (30.0) on a
shared 4 vCPU VM (load 3–6, other builds running), the same binaries alternated:

| A million requests | v28.0.0 | v29.0.0 | 30.0 |
| --- | --- | --- | --- |
| the budget's `show` (4 runs each, median, range) | 10.7 s (9.1–11.4) | 11.3 s (9.3–12.8) | 9.6 s (9.3–10.7) |
| its peak memory | 343–352 MiB | 343–348 MiB | 346–354 MiB |
| the scale example's `plan:` (rank, in all, diagnoses; 3 runs) | | 2.0–2.4 s | 0.98–1.09 s |

## Headless UI

`nsh test --trace`, debug host, measured at sonde 0.1.0 (then on the terminal host, gone in 29.0; CI's nightly
`ui-perf` measures the desktop host): Browse › demo-large, flat, load all 100 000
rows, scroll the table 500 frames down and back.

| Measure | Result |
| --- | --- |
| Load 100 000 rows (debug native module) | 3.53 s |
| 500 scroll frames down / back up | 3.14 s (6.3 ms/frame) / 3.16 s |
| Commits during the scroll | 210 |
| Mount items per commit | median 16, p95 17, max 153 (the first frame after *Load all*) |

The DataTable is virtual: JS holds one 400-row window, fetched from Rust's `Listing` by range.

## Opening the Time pane

Headless `nsh test --trace`, the example run (172 requests), three runs each, Native Shell `v0.6.1`, from the press on
Time to the pane at rest:

| Board | The press's commit | Largest commit after it | All commits | Mount items |
|---|---|---|---|---|
| every chart built at once (26.0) | 432–440 ms | 1.13–1.14 s (1.10–1.12 s JS) | 1.57–1.60 s | 17,877 |
| a `List` in the pane's scroll (27.0) | 87–106 ms | 218–254 ms (182–217 ms JS) | 309–364 ms | 4,509 |

The board is a virtualized `List` (`viewport="parent"`, [D-278](decisions.md)): opening the pane builds the charts
of its first screen, and each further line of charts as it scrolls into view.

## Long jobs

Off the UI's thread, each with its own throttle and retries: [architecture](architecture.md#threads).

## Not measured

- **macOS app** (cold start, window memory, chart frame time): `nsh package --target macos` runs only on macOS.
- **Real endpoints**: list and probe times follow the endpoint's latency; the request inspector shows each request's time.
