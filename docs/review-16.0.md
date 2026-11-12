# 16.0 reviewed, 17.0 proposed

A review of 16.0.0 after its release, by four readers who did not write it: the Rust hunted for bugs with the
released binary against a fake store and small programs; the CLI used as a newcomer would use it, and sessions of
ten thousand to two million requests measured; every picture looked at and the interface counted against its own
standard; and the whole looked at for its health — structure, tests, CI, security, dependencies, process — and
for what its users would expect that it does not do. Then what 17.0 should be. 16.0's own review is
[review-15.0.md](history/review-15.0.md).

Marked **confirmed** where it was run and seen, **read** where it is from the code only. The app was not run (no
`nsh` here): its findings are from the 16.0 pictures in `docs/images` and from the code.

## In short

16.0 kept almost every promise. All of 15.0's "The CLI, used" rows are fixed but one, every performance claim
measured holds within 10 %, the two quadratic analyses are gone, the gzip limit and dot segments hold on every
reader, replay cleans up on a store without `DeleteObjects`, `cargo test` passes and clippy says nothing.

Three things did not hold:

1. **The verdict's oldest bug is back in a common key.** With `{trial}` in a key that also holds the word `clean`
   (or the scenario's own name) as a word of its own, a program that never retries is "survived" again.
2. **"Sessions of any size" is sessions up to RAM**, and `compare` was left out of the stream entirely.
3. **The pictures still show rough places**, and the README's own trial picture shows a dialog that says
   "press a scenario" with nothing to press.

And one thing beyond 16.0's scope: sixteen majors in seven days, with no declared contract for the `.sonde`
file, the JSON outputs, `sonde.toml` or the flags. The version number no longer tells a user anything.

## What 16.0 got wrong

### The verdict

| | Where | What |
|---|---|---|
| High | `proxy/trial.rs` `unplace`, `judge` | **A key with `{trial}` that also holds the run's name as a word is never matched to the clean run.** `unplace` replaces every free-standing occurrence of the run's own name. In the clean run that name is `clean`, so `etl-clean/clean/state` becomes `etl-{trial}/{trial}/state`; in the trial it is the scenario's, so `etl-slowdown/clean/state` becomes `etl-{trial}/clean/state`. No request matches, and the as-written fallback covers only keys without `{trial}`. A program that writes `…/clean/state` twice and swallows errors: `survived · 1 fault, retried · after 7 ms`; with `…/raw/state`: broke. `clean` is the commonest folder word in a lake (raw/clean layers); `etl-$SONDE_TRIAL/error.json` under `--fault error` does the same. Confirmed. |
| High | same, `cut-body` | The same mismatch turns "took half the body as the whole" into `survived · 1 body cut: 1 read again whole` when the key holds `clean` and `{trial}`. On `data/big`: broke. Confirmed. |
| Medium | `trial.rs` `retry_owed`, `answered` | **Trials judge a retry owed by the status only.** Episodes, signals and `docs/trials.md` count S3's `SlowDown` and `RequestTimeout` by their code, and S3 sends `RequestTimeout` as 400: a scenario `fault = "400 RequestTimeout"` is "none a retry is owed for", and a program that drops the write survives. D-115's "one rule for trials, episodes and signals" is not one rule. Confirmed. |
| Medium | `trial.rs`, lost-race | For a key that is not a table's log (`_SUCCESS`, a marker, a lock) a writer that takes 412 on `If-None-Match: *` as "already there" is "broke · the commit was never made" — but sonde let the write through, so the store holds it. A retry with the same condition gets a real 412, so only reading the key back survives. The scenario should be offered only where a 412 means something was lost (a commit family), or judged by whether the program's belief matches the store. Confirmed. |
| Low | `sonde-nsh/sessions.rs` `asks_cursor` | The Windows cursor query `ESC[6n` is looked for in each chunk separately; split across two reads, it is never answered and a trial from the app hangs at its start — 16.0's Windows fix, undone by a chunk boundary. Needs a carry of the last three bytes. Read. |

### Reading sessions

| | Where | What |
|---|---|---|
| Medium | `cmd/record.rs` `compare`, `profile.rs` `compare_runs` | **`compare` was not moved to the stream.** It opens both sessions whole (`sessions::open`) and holds them together: 4.7 GiB for two runs of a million requests, 9.4 GiB and 65 s for two of two million; two of three million do not fit in 15 GB. Neither the changelog, `performance.md` nor the budgets test names it. `diff` reads whole as well. Confirmed. |
| Medium | every reader | **What streams is the output, not the memory.** Every reading command holds about 1.75 KB a request for the whole session (4.5 KB with 1 000-character keys): 3.4 GiB at two million, so the ceiling on a 15 GB machine is about eight million. "Sessions of any size" (README, changelog headline) is true up to RAM ÷ 1.75 KB. Confirmed. |
| Medium | `compare` against `show` | **"Waiting" means two things.** For one session `show` says UploadPart waited 78m 54s (79 %); `compare` says 5975.6 s, nearly the whole run — the span of the kind, not its share. GetObject: 9m 59s against 5750.7 s. Confirmed. |
| Low | `summary.rs` `Counter::finish` | The cache's stamp (length, time) is taken after the file is read: a rewrite between keeps the old counts under the new stamp. Times are compared to the millisecond, which a 1–2 s filesystem (HFS+, exFAT) cannot tell. And the cache is versioned by `COUNTED_VERSION = 1`, not by the analysis: a stored run's warnings outlive a change to them. Read. |
| Low | `summary.rs`, `session.rs` | `sessions` skips a malformed line in silence and lists the file; `show` refuses the same file with exit 70 (an internal error; 65, bad data, is meant). A `.sonde` whose gzip stream is cut does not open at all, while a cut plain line is tolerated. Confirmed. |
| Low | `sonde-nsh/sessions.rs` `episode`, `episode_row`, `records`, … | These bridge calls are synchronous: on a cache miss (more than 8 sessions open, or a live file that grew) they re-analyse the whole session on the calling thread. While a recording is live a later page comes from a newer analysis whose indices moved, so rows can repeat or be missed. Read. |

### The CLI, used

| | Where | What |
|---|---|---|
| Low | `show`, app tiles | **"A run under a second … draws no sparkline" is not done**: `show` of a 63 ms demo, or of one 1 ms request, still draws four sparklines beside "no rate: under a second"; in the app (`signals.ts`) the line goes only under 20 requests, so the README's 467 ms session has five. Confirmed in both. |
| Low | `show`, `check`, `compare`, findings | **One fact, two forms, still**: "2,877" in the table and "2877 requests throttled" in the finding; `check` and `compare` have no separators; the run is "99m 36s" in `show` and "5976.2 s" in `check`. `show --errors` prints "no errors: none of its operations failed" over "3,398 errors". Confirmed. |
| Low | `compare` | Drops three of nine kinds of request without "… and 3 more". Confirmed. |
| Low | `check` | `check --init` writes `[program] name` and the next `check` advises adding it (the name binds at recording; say "from your next run"). Its p95 is 2.3× the measured one where the others are +12 %. `-f markdown` omits "(no baseline yet)". The no-baseline sentence goes to stderr as well. Confirmed. |
| Low | table writer | A column holding "≤" or "→" is padded one short (counts bytes, not characters). "1 requests", "1 more kinds". A CloudTrail import says "latency not recorded" and then prints tables of 0.0 ms. Confirmed. |

### The interface

The structural work 16.0 claimed is in the code: one time axis per pane, Compare says each fact once, Chinese
headings have their weight, ages read "86 d", Run's actions keep one row, `plots.tsx` is split by family, and the
four lints pass. The pictures still show rough places.

| | Where (picture) | What |
|---|---|---|
| High | `trial.png`, `Run.tsx:252` | **The README's trial picture says "Press a scenario to open its session", and nothing can be pressed**: Run passes no `onOpen` to `TrialDialog`, so every finished row is disabled. Only the Runs pane opens one. Confirmed in the code. |
| Medium | `session-min.png` | At the least window the findings' room is a fixed 68 pt; the third finding is cut through its line of text, and nothing says it scrolls. |
| Medium | `activity.png`, `uploads.png`, `palette.png` | Still cut with room beside them: Activity's target ("demo-la… anifest") beside an empty Operation column; an upload's key cut at its end beside ~100 pt of empty Started; the "Recognised layouts" tile cuts its figure. The layout lint checks titles and the widest formatted cell, not paths, tiles or column objects. |
| Medium | `runs.png` | Compare's dumbbells have no legend for which dot is before; "p95 5.7 ms → 69 ms" of seven requests has no "few" mark; the treemap's grey figures on saturated purple are hard to read. |
| Medium | `spread.png`, `plots/traffic.tsx` | The sizes chart labels each bar and draws a peak scale — DESIGN says one or the other, and the chart lint, which lists Sizes among charts of counts, demands both. The lint contradicts the standard. |
| Medium | `flows.png` | The Sankey's middle labels sit over their own node bars. |
| Medium | `session-zh.png`, `zh.json` | 失败 is the word for a broke trial and for a failed request, job and budget. |
| Low | several | A red "2 errors" for two orange 4xx rows; two primaries 90 pt apart on Run; the page behind a side sheet reflows; selected segments read inverted in dark; middle ellipses leave a gap; the trial dial offers 1–3 where the CLI takes 1–5; a screen reader hears nothing as a verdict arrives; `paged.ts` never re-asks a range whose fetch failed, and a live view past row 500 is replaced by the first page. |
| Low | process | **No picture is checked in for most of what 16.0 changed**: the trial plan and its dial, a running trial, a trial over its clean run, a finding's advice in its row, the least window beside the inspector. The tour writes to `app/shots/`, which is ignored. `App.tsx` is read by no lint (two `font-semibold`, a hand-made header). `share-html*.png` are from 9.1. |

Counted again, as 15.0's review counted:

| | 14.0 | 15.0 found | Today |
|---|---|---|---|
| Renderings of a figure | 5 | 2; Compare showed both | 2; each fact once |
| Headings by hand | 13 | 0 bold, 26 `font-medium` | 0 in `src`; 2 `font-semibold` in the unlinted `App.tsx` |
| The chart frame, pitch, legend, tooltip | Time only | one each; 4 tick schemes, 5 thicknesses, 5 radii | one each; 1 tick function; 4 + 3 thicknesses, 8 radii, 8 heights |
| Layout thresholds | 5 | 4 + 3 under other names | 4 `BREAK`, 6 `SIZES`, and literals the lint cannot see (68/96/104, 160/220/340, 52+196+92+104, 480/320) |
| Writers of a length of time | 5 | 4 | 1 |
| Locale keys passing by a prefix | | 87 of 973 | 95 of 1 037, in 17 listed families; en and zh equal |
| Largest files | `plots.tsx` 1 922 | `kit.tsx` 1 297 | plots 2 390 in 8 files (+24 %); `kit.tsx` 1 490; `Sessions.tsx` 852 |

What a reader used to Grafana, Datadog or Honeycomb still misses: a time range shared by every pane and kept in a
link; a breakdown by any attribute (prefix, status, size, SDK, thread) and "what is different about the slow
ones"; a threshold line from the budget on its chart; more than two runs overlaid; one key's conversation followed
across operations; a chart's data out as CSV. (17.0's M7 takes the threshold, the range and the CSV; 18.0 the rest.)

## Large sessions, measured

Release binary, 4 cores, 15 GB; sessions made by a port of the budgets test's own generator, plus hot-key,
distinct-key, long-key and large-header shapes.

| | 100k | 1M | 2M | 16.0 said (1M) | 15.0 (1M) |
|---|---|---|---|---|---|
| `show` | 0.7 s, 209 MiB | 7.6 s, 1.74 GiB | 14.9 s, 3.4 GiB | 7 s, 1.8 GiB | 45 s, 7.3 GB |
| `check --baseline` | 1.5 s, 247 MiB | 16.3 s, 2.0 GiB | 23.6 s, 3.4 GiB | 17 s, 2.0 GiB | 99 s, 11 GB |
| `show -f json`, `share --json`, `export` | 2.6–2.8 s, ~200 MiB | 30–33 s, ~1.7 GiB | 63 s, 3.4 GiB | ~32 s | could not |
| `share --html` | 0.85 MB page | 10 s, 0.84 MB page | | 0.85 MB | 328 MB |
| **`compare`** | 2.2 s, 516 MiB | **23.8 s, 4.7 GiB** | **65 s, 9.4 GiB** | not said | |
| `sessions`, 4.1M requests in four files | | first call 14.8 s, 26 MiB; then at once; after a `touch` only that file | | | 24 s, 4.8 GB each call |

The outage and one-part-upload shapes take 2.0–2.6× per doubling from 300 thousand to 2.4 million: no quadratic
is left. Memory is linear in requests for every reader.

## Health

| | Where | What |
|---|---|---|
| Medium | `run.ts:87`, `sessions.rs:148`, `record.rs:275`; README "Safe by default" | **The read-only switch does not reach Run.** It guards sonde's own acts (aborts, deletes, probes, trials are refused under it), but a program recorded through Run or `sonde run` may write and delete with sonde's credentials by default (`allow`). The README's "read-only until you turn it off" reads as covering it. And `--no-delete` lets overwrites, bucket policies and versioning suspension through (`op.rs:152`). Read. |
| Medium | `callers.rs:131`, `hooks/java` | The Java callers hook is a committed JAR, injected into the user's JVM, never rebuilt or checked in CI. Releases carry checksums from the same origin as the binaries, no signature or attestation; signing and notarisation are skipped in silence without secrets; actions are pinned by tag. Read. |
| Medium | `ci.yml`, `build.yml` | No `cargo test` on Windows and no Windows CLI binary, while the Windows app ships and 16.0 fixed a Windows-only hang. The UI perf test and two timing tests never run in CI; the memory budget has ~10 % headroom; five commits in five days fixed test timing. Read. |
| Medium | `cmd/record.rs` (4 440 lines), `sonde-nsh/sessions.rs` (3 205) | `record.rs` holds nine commands, the `sonde.toml` schema and the budget logic. `sonde.toml` is parsed twice: strictly by serde in the CLI, by hand in the app — a file one accepts the other can refuse. Read. |
| Medium | the project | **Sixteen majors between 6 and 12 November, the `.sonde` format still at version 1, and no declared contract** for the file, the JSON outputs, `sonde.toml`, the exit codes or the flags. Twenty "work in progress, may not build" commits are on the main line. The major number says nothing a user can act on. |
| Low | several | Sessions and the audit log are written with the umask (only `config.toml` is 0600). Bytes are written by five functions that round differently (Rust "1024.0 KiB", TS "1.0 MiB"). The CLI is English only: Chinese lives in TS. No fuzzing of the decoders, importers or XML parser, which read untrusted input. 51 crates in two or more versions; reqwest, quick-xml, rand, toml, rusqlite a major behind; `flatbuffers` unused. `architecture.md` names neither trials, faults, signals nor the cache. Read. |

What is sound: the proxy binds to loopback, checks Host, signs with a per-proxy key and compares in constant
time, so neither another local user nor a web page can borrow the credentials; `share --redact` left nothing
identifying; `unsafe` is forbidden; about 520 Rust tests and 70 UI tests, the new 16.0 paths among them; the
budgets test runs in CI.

What its users would expect and do not find: Spark, Trino and Flink through `fs.s3a.*` (the audience of the
Iceberg and Delta decoders; `docs/recording.md` says "set by hand"); S3 Express One Zone (`CreateSession`'s session
credentials are refused by the proxy); SigV4a; TLS and HTTP/2 on the local endpoint; Hudi, Paimon, DuckLake and
S3 Tables; a sidecar with `/metrics` for production. eBPF capture is not recommended: it loses both the credential
isolation and the faults.

## 17.0

**A verdict without exceptions, a contract, and the reach of a lake.**

| | What | Size |
|---|---|---|
| **M0: 16.0.1, now** | `unplace` replaces the run's name only where the command's template had `{trial}` (or both names on both sides), with tests for `clean/`, `clean-state` and a scenario named in the key. One retry rule for trials, episodes and signals — the same function, code and status. Lost-race offered only on a commit family. Run's trial dialog opens a scenario's session. A carried `ESC[6n`. `show` and the app draw no sparkline under a second. | S |
| **M1: a contract** | Declare what is stable and version it: the `.sonde` format (a schema, `version`, and a reader that refuses a newer one by name), `-f json` outputs (a JSON Schema each, checked in CI against the fixtures), `sonde.toml` (one parser, in `sonde-proxy`, for CLI and app), the exit codes and the flags. From then on, a major only when one of them breaks; everything else is a minor. `docs/contract.md`. | M |
| **M2: safe by default, said truthfully** | Run and `sonde run` follow a recording policy chosen in Settings and in `sonde.toml`, defaulting to `no-delete`; `no-delete` refuses overwrites of existing objects that the session did not create, bucket policies and versioning changes; the README says what read-only covers. Sessions, the audit log and caches 0600. The JAR built in CI and its hash checked; releases signed (Sigstore attestations), actions pinned by SHA, `cargo-deny`. | M |
| **M3: memory, not only output** | `compare` and `diff` read as a stream. The lean record halved: keys and prefixes interned, times and sizes packed; a budget of 1 KB a request kept by the test, and a run of ten million in 8 GB. "Waiting" is one measure, shared by `show` and `compare`. The cache keyed by the analysis's version and stamped before it reads. A cut gzip read up to its cut. | L |
| **M4: the reach of a lake** | Spark, Trino and Flink: `sonde run` writes `fs.s3a.*` / `s3.*` for the JVM engines it recognises, and a recipe each, checked in the `record` CI job with Spark. S3 Express One Zone: `CreateSession` passed through, its session credentials honoured. TLS on the local endpoint (a key made on first run) for SDKs that require it. Paimon and Hudi's timeline as layouts and operations. | L |
| **M5: the rough places, seen** | Every row above under "The interface". The tour's pictures of every 16.0 and 17.0 screen checked in (or attached to a release) so a review can see them. The lints learn what they missed: `App.tsx`, numbers in column objects, a bound that cuts a row, a path cut at its end, the sizes chart's exception. One word for a broke trial (未通过 or 中断). Compare's legend and its few marks. One byte and count formatter, shared by Rust and TS through the phrases. | M |
| **M6: Windows and the line of releases** | `cargo test` on Windows in CI, a Windows CLI binary, the UI perf test in CI. The main line builds at every commit: work in progress on a branch, squashed. `record.rs` split into one module per command, and the project file and budgets moved into `sonde-proxy`. Fuzz targets for the decoders, importers and XML. | M |
| **M7: small things with a large reach** | A budget drawn on its chart: `sonde.toml`'s p95, error share and waiting as a threshold line on latency, error rate and waiting, with how far the run is from it. A range kept in a link: the slices chosen go into a `sonde://` link and a shared page, so who opens it sees the same time (D-123 stands: no drag, no wheel). A chart's data out as CSV, from the rows Rust already holds. Four scenarios that production has met: a clock skewed (`RequestTimeTooSkewed`), a `CompleteMultipartUpload` answered 500 though it completed, a first byte that comes very late, a listing that does not yet show a write. Chinese in the CLI: the phrases' templates move to Rust with M5's one formatter, so `show`, `check` and a trial speak the reader's language. | S each |

M7 comes after M0 to M6 and is the first thing cut if they run long; of it, the budget on its chart goes first.

**For 18.0**, once M1's contract says what a session holds:

- a breakdown by any attribute (prefix, status, size, SDK, thread) and "what is different about the slow ones",
  the largest thing users of other tools miss;
- more than two runs overlaid;
- one key's conversation followed across operations;
- a sidecar for production (an image, `/metrics`, a Kubernetes example), which needs M3's memory as well;
- DuckLake, S3 Tables, Iceberg's REST catalog and v3's deletion vectors;
- SigV4a and HTTP/2 on the local endpoint.

**Not in 17.0 or 18.0**:

- drag or wheel zoom (D-123 stands) and a query language;
- eBPF and Azure gateways;
- a replay at a multiple of its speed, which would make sonde a load tester;
- prices of other clouds, a table that goes stale;
- an MCP server for agents, which waits for the JSON schemas of M1 to have held a while.

The risk, named: M1 is a promise, and after it a careless change costs a major. It is written from what the
outputs are today, checked by tests that fail on any change to a schema, and released only with M0 and M2 so the
first contract describes a verdict and a default that are right.
