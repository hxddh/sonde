# Architecture

```
app/                      TSX screens (Native Shell), theme, nsh.json        ─┐ presentation
viewer/                   the web viewer for `sonde share` (TSX, web target)  │
crates/sonde-nsh          `sonde/native`: #[module] Sonde, Sessions,          │ and the bridges
                          #[class] Listing; Run's PTY (portable-pty)          │ to it
crates/sonde-cli          `sonde`: headless subcommands; `sonde tui` host     ─┘
crates/proxy (sonde-proxy) the recording endpoint, faults and trials,       ─┐
                          sessions and their summary cache, episodes,         │
                          observations, signals, the paged view, programs     │
                          and budgets, replay, comparison, import, OTLP,      │
                          the HTML report                                     │
crates/sonde-core         config, vendors, credentials, jobs, guard + audit,  │ no UI framework
                          services: browse, mpu, versions, du, layout, exec;  │ (about three
                          demo S3 server                                      │ fifths of the
crates/probes             40 probes, runner, run store, diff, matrix, reports │ code, the app's
crates/decoders           SlateDB, Iceberg, lakeFS, Woodpecker, cadence       │ TSX counted)
crates/s3raw              SigV4, addressing, retries, throttle, XML, ops      │
crates/reqlog             records, redaction, ring buffer, SQLite, HAR        │
crates/fmt (sonde-fmt)    how a size, a count and a length of time are written ─┘
hooks/java                the Java caller hook's sources (its jars are built into sonde-proxy)
fuzz/                     cargo-fuzz targets for what sonde reads from others (not in the workspace)
```

Dependencies point down:

```
sonde-fmt ← decoders ← sonde-core ← probes
sonde-fmt ← sonde-proxy, sonde-core, sonde-cli
reqlog ← s3raw ← sonde-core
reqlog ← s3raw ← sonde-proxy
sonde-core, probes, decoders, sonde-proxy ← sonde-nsh, sonde-cli
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

Retries (`s3raw`): 5xx, 429 and transport errors, exponential backoff with full jitter, honouring `Retry-After`;
each attempt is a record with its retry number.

## Data paths

| Path | How |
|---|---|
| A listing | `new Listing(endpoint, bucket, prefix, "/")` holds rows in Rust; TSX asks for the visible window (`rows(start, 400)`, on `onVisibleRowsChange`); `loadMore()` fetches the next ListObjectsV2 page (`onLoadMore`). |
| Run | `Sessions.run` starts an endpoint, then the command through the user's shell in a pty (`portable-pty`: openpty, ConPTY). A reader thread sends output to the engine (`{started}`, then `{data}`, UTF-8-safe) and appends `{"log": {time, text}}` lines; `Terminal` draws it and sends keys and resizes back (`Sessions.input`, `Sessions.resize`). Resolves with the exit status and session path. `sonde-proxy::logs` makes timed lines, lined up with operations by time. |
| `sonde tui` | The embedded bundle on `nsh-platform-tui`, with its own event loop (crossterm via `nsh-tui-host::convert`) and an OSC 52 clipboard. |
| The web viewer | `viewer/`, built with `--target web`: the app's pure views (`sessionview.tsx`, `record.tsx`) over a `session.json` computed in Rust; no Rust in the browser. `crates/sonde-cli/build.rs` embeds it from `$SONDE_VIEWER_DIR` or `viewer/.nsh/out/sonde-viewer`; without it `sonde share` says so. |

## A recording, and what is read from it

The proxy re-signs each request and appends it to the `.sonde` file (a header line, a JSON line per request or
output chunk; gzipped when finished). Then, in `sonde-proxy`:

| Module | Does |
|---|---|
| `fault.rs` | answers in the store's place (503, 500, slow, lost answer, cut body, lost race), per request's attempts |
| `trial.rs` | a clean run, then one per scenario, each judged against the clean run (`<name>.trials.json`, `trials/`) |
| `episode.rs` | requests → operations and findings |
| `signals.rs` | rate, errors, latency, bytes, in flight, in one pass |
| `program.rs` | a run's figures against a budget (`sonde.toml`) or a baseline: `sonde check`, the app's history |
| `fixloop.rs` | a finding across a program's runs: fixed (gone at any threshold, measured saving), less, back, new, open; kept in `findings.json` |
| `summary.rs` | counts kept as `<name>.summary.json`, valid while file length, time and cache version match |
| `view.rs` | analysed once, sent in pages (`Sessions.view`, `Sessions.episodes`): what crosses to the UI does not grow |
| `phrase.rs` | each sentence as a key and arguments beside its English; the app and viewer render them per language (`app/src/phrases.ts`), falling back to English |

Every size, count and duration in text comes from `sonde-fmt`, tested with the app's `format.ts` against
`crates/fmt/cases.json`.

## The app

- **UI layer** (`app/src/ui/`): the styled layer over `nsh/base` — theme tokens (`app/theme.json`, read by the
  viewer too), icons (`icon.tsx`, Lucide data in `lucide.ts` from `scripts/lucide.py`), shortcuts (`keys.ts`),
  controls (`kit.tsx`), toasts, toolbar and sidebar (`shell.tsx`), palette, and the session views shared with the
  viewer (`sessionview.tsx`, `record.tsx`, `profile.tsx`). Pages compose these and never style base components.
  Standards: `app/DESIGN.md`.
- **Two forms**: `App.tsx` branches on `platform.idiom === "terminal"`.

  | Window | Terminal |
  |---|---|
  | sidebar with Run, Sessions, Buckets | a navigation stack with every place |
  | Settings, Compatibility in their own windows (`Window.open`) | the F10 menu bar, keyboard shortcuts |
  | Activity, bucket details in sheets (`setOverlay` in `store.ts`) | |
  | command palette, Canvas charts | tables and sentences |

  The same screens serve both, adapting density (`terminal()` in `ui/kit.tsx`).
- **Bucket overview** (`ui/overview.tsx`): starts the cheap scans when a bucket or prefix opens and keeps results per
  endpoint, bucket and prefix, reused by the sheets it opens.
