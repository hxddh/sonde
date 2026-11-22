# Architecture

```
app/                      TSX screens (Native Shell), theme, nsh.json        ─┐ presentation
viewer/                   the web viewer for `sonde share` (TSX, web target)  │
crates/sonde-nsh          `sonde/native`: #[module] Sonde, Runs,              │ and the bridges
                          #[class] Listing; Run's PTY (portable-pty)          │ to it
crates/sonde-cli          `sonde`: headless subcommands; `sonde tui` host     ─┘
crates/proxy (sonde-proxy) the recording endpoint: re-signing, policy,       ─┐
                          faults, TLS, caller hooks                           │
crates/analysis           runs (the file, its index and counts), operations,  │
  (sonde-analysis)        findings, signals, the paged view, programs and     │
                          budgets, trials, replay, comparison, import, the    │
                          HTML report                                         │
crates/sonde-core         config, vendors, credentials, jobs, guard + audit,  │ no UI framework
                          services: browse, mpu, versions, du, layout, exec;  │ (about three
                          demo S3 server                                      │ fifths of the
crates/probes             52 probes, runner, run store, diff, matrix, reports │ code, the app's
crates/decoders           SlateDB, Iceberg, lakeFS, Woodpecker, cadence       │ TSX counted)
crates/s3raw              SigV4, addressing, retries, throttle, XML, ops      │
crates/reqlog             records, redaction, ring buffer, HAR                │
crates/fmt (sonde-fmt)    a size, a count, a length of time; its English words ─┘
hooks/java                the Java caller hook's sources (its jars are built into sonde-proxy)
fuzz/                     cargo-fuzz targets for what sonde reads from others (not in the workspace)
```

Dependencies point down:

```
sonde-fmt ← decoders ← sonde-core ← probes
sonde-fmt ← sonde-analysis, sonde-core, sonde-cli
reqlog ← s3raw ← sonde-core
reqlog ← s3raw ← sonde-proxy ← sonde-analysis
sonde-core, probes, decoders, sonde-proxy, sonde-analysis ← sonde-nsh, sonde-cli
sonde-nsh ← sonde-cli (optional, feature `tui`)
```

- `sonde-proxy` needs no sonde configuration, only an endpoint.
- Only `sonde-nsh` and `sonde-cli` (feature `tui`) depend on Native Shell: another UI framework replaces only
  `sonde-nsh` and `app/`.

## Threads

| Thread | Does |
|---|---|
| Engine (Native Shell) | The TSX VM and reactive graph. `sonde/native` calls that wait are `async` and run on the runtime (`background`); synchronous ones only read memory (the request log, a listing's rows). |
| Tokio runtime | Requests, jobs, the demo server. |
| Jobs (`sonde-core::jobs`) | Each long operation: its own `Throttle` (concurrency, rate), a cancellation token, a broadcast of events. Cancelling drops the future and its requests; screens cancel theirs on `cleanup`. TSX follows one with `watchJob(id, listener)`. |

Retries (`s3raw`): 500, 502, 503, 504, 429, a 400 `RequestTimeout`, a 403 `RequestTimeTooSkewed` (signed again) and
transport errors, exponential backoff with full jitter, honouring `Retry-After`; each attempt is a record with its
retry number.

## Data paths

| Path | How |
|---|---|
| A listing | `new Listing(endpoint, bucket, prefix, "/")` holds rows in Rust; TSX asks for the visible window (`rows(start, 400)`, on `onVisibleRowsChange`); `loadMore()` fetches the next ListObjectsV2 page (`onLoadMore`). |
| Run | `Runs.run` starts an endpoint, then the command through the user's shell in a pty (`portable-pty`: openpty, ConPTY). A reader thread sends output to the engine (`{started}`, then `{data}`, UTF-8-safe) and appends `{"log": {time, text}}` lines; `Terminal` draws it and sends keys and resizes back (`Runs.input`, `Runs.resize`). Resolves with the exit status and run path. `sonde-analysis::logs` makes timed lines, lined up with operations by time. |
| `sonde tui` | The embedded bundle on `nsh-platform-tui`, with its own event loop (crossterm via `nsh-tui-host::convert`) and an OSC 52 clipboard. |
| The web viewer | `viewer/`, built with `--target web`: the app's pure views (`runview.tsx`, `record.tsx`) over a run's JSON computed in Rust; no Rust in the browser. `crates/analysis/build.rs` embeds it from `$SONDE_VIEWER_DIR` or `viewer/.nsh/out/sonde-viewer`; without it `sonde share` says so. Native Shell's page links everything (strict CSP since 0.6: styles in `nsh-page.<digest>.css`, modules importing each other's files, no import map); `share.rs` sets that stylesheet inline and its script points each file at a `blob:` URL, so the one file opens from a disk. |

## A recording, and what is read from it

The proxy (`sonde-proxy`) re-signs each request, answers in the store's place under a fault (`fault.rs`: 503,
500, slow, lost answer, cut body, lost race, per request's attempts), and hands each record to the `.sonde` file
(a header line, a JSON line per request or output chunk; gzipped when finished). Then, in `sonde-analysis`:

| Module | Does |
|---|---|
| `trial.rs` | a clean run, then one per scenario, each judged against the clean run (verdicts in the index, `trials/`); what did not survive is a finding of the clean run (`trial\|<scenario>`) |
| `trial_plan.rs` | what a trial of a recorded run would try, said before it runs (the app's plan) |
| `episode.rs` | requests → operations and findings |
| `signals.rs` | rate, errors, latency, bytes, in flight, in one pass |
| `program.rs` | a run's figures against a budget (`sonde.toml`) or a baseline: `sonde check`, the app's history |
| `fixloop.rs` | a finding across a program's runs: fixed (gone at any threshold; the run's time and cost saved, against its noise), less, more, new, open, by counts; kept per program in the runs folder's `.index.jsonl` |
| `summary.rs` | counts kept in the folder's index, valid while file length, time and cache version match |
| `index.rs` | one `.index.jsonl` per runs folder: counts, verdicts, exit codes; an appended log, compacted |
| `view.rs` | analysed once, sent in pages (`Runs.view`, `Runs.episodes`): what crosses to the UI does not grow |
| `phrase.rs` | each sentence as a key and arguments; its words, English included, are `phrase.<key>` of `app/locales` (read by `sonde-fmt`'s `words`), said per language by the app and viewer |

The index is written under the folder's `.index.lock`. A run's counts (requests, time, errors, program, figures once
checked) are reckoned again when missing, when the run's length or mtime (ns) changed, or when the analysis changed;
a run's entry goes with the run.

Every size, count and duration in text comes from `sonde-fmt`, tested with the app's `format.ts` against
`crates/fmt/cases.json`.

## The app

- **UI layer** (`app/src/ui/`): the styled layer over `nsh/base` — theme tokens (`app/theme.json`, read by the
  viewer too), icons (`icon.tsx`, Lucide data in `lucide.ts` from `scripts/lucide.py`), controls and shortcut labels
  (`kit.tsx`), toasts, toolbar and sidebar (`shell.tsx`), palette, and the run views shared with the
  viewer (`runview.tsx`, `record.tsx`, `profile.tsx`). Pages compose these and never style base components.
  Standards: `app/DESIGN.md`.
- **Two forms**: `App.tsx` branches on `platform.idiom === "terminal"`.

  | Window | Terminal |
  |---|---|
  | sidebar with Run, Runs, Buckets | a navigation stack with every place |
  | Settings, a run in their own windows (`Window.open`) | the F10 menu bar, keyboard shortcuts |
  | Activity, bucket details in sheets (`setOverlay` in `store.ts`) | |
  | command palette, Canvas charts | tables and sentences |

  The same screens serve both, adapting density (`terminal()` in `ui/kit.tsx`).
- **Bucket overview** (`ui/overview.tsx`): starts the cheap scans when a bucket or prefix opens and keeps results per
  endpoint, bucket and prefix, reused by the sheets it opens.
