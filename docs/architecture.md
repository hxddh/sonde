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

`sonde-proxy` does not depend on `sonde-core`: a recording needs no configuration of sonde's, only an endpoint.
Only `sonde-nsh` and `sonde-cli` (feature `tui`) depend on Native Shell. Replacing the UI framework keeps every crate
but `sonde-nsh` and `app/`; the CLI's commands are in `sonde-cli` and the TSX views are the app's, so those are the
other part of the code that is about presentation.

## Threads

- **Engine thread** (Native Shell): the TSX VM and the reactive graph. `sonde/native` functions that wait are
  `async` and hand their work to the runtime's threads with `background`; synchronous ones only read in-memory
  state (the request log, a listing's rows).
- **Tokio runtime**: requests, jobs, the demo server.
- **Jobs** (`sonde-core::jobs`): each long operation is a job with its own `Throttle` (concurrency, requests per
  second), a cancellation token and a broadcast of events. Cancelling drops the job's future, which aborts its
  requests; screens cancel their jobs when they go (`cleanup`). TSX follows a job with `watchJob(id, listener)`,
  which forwards events to a JS callback and resolves with the result.
- **Retries**: `s3raw` retries 5xx, 429 and transport errors with exponential backoff and full jitter, honoring
  `Retry-After`; each attempt is a record with its retry number.

## Data flow of a listing

`new Listing(endpoint, bucket, prefix, "/")` holds the rows in Rust. TSX asks for the window it shows
(`rows(start, 400)`) as the `DataTable` scrolls (`onVisibleRowsChange`), and `loadMore()` fetches the next
ListObjectsV2 page when the table nears its end (`onLoadMore`). 100 000 keys never cross into JS at once.

## The terminal host

`sonde tui` runs the embedded bundle on `nsh-platform-tui` with its own event loop (crossterm events through
`nsh-tui-host::convert`) and an OSC 52 clipboard.

## Run

`Sessions.run` starts a recording endpoint, then the command through the user's shell in a pseudo-terminal
(`portable-pty`: openpty on Unix, ConPTY on Windows) with the endpoint's variables set. A reader thread sends the
output to the engine as it comes (a `Callback`: `{started}`, then `{data}`, decoded across UTF-8 boundaries) and
appends it to the session as `{"log": {time, text}}` lines; Native Shell's `Terminal` draws it and sends keys and
resizes back (`Sessions.input`, `Sessions.resize`). The run resolves with the exit status and the session's path.
`sonde-proxy::logs` turns the chunks into timed lines, which the UI lines up with operations by time.

## The web viewer

`viewer/` is a second Native Shell project built with `--target web`: the same pure views as the app
(`app/src/ui/sessionview.tsx`, `record.tsx`) reading a `session.json` that `sonde share` computes in Rust. No Rust
runs in the browser. `crates/sonde-cli/build.rs` embeds the built site (from `$SONDE_VIEWER_DIR` or
`viewer/.nsh/out/sonde-viewer`); a build without it says so when `sonde share` needs it.

## The app's UI layer

`app/src/ui/` is the styled layer over `nsh/base`: theme tokens (`app/theme.json`, read by the viewer too), icons
(`icon.tsx` draws Lucide data from `lucide.ts`, written by `scripts/lucide.py`), shortcuts as each platform writes
them (`keys.ts`), controls (`kit.tsx`), toasts (`toast.tsx`), the window's toolbar and sidebar (`shell.tsx`), the
palette, and the session views shared with the web viewer (`sessionview.tsx`, `record.tsx`, `profile.tsx`). Pages
compose these; they do not style base components of their own. The standards they follow — the type scale, surfaces,
side panels, when a button is primary, motion and words — are in `app/DESIGN.md`, on top of Native Shell's design
guides.

## The two forms

`App.tsx` branches on `platform.idiom === "terminal"`: windows get a sidebar with three places (Run, Sessions,
Buckets), Settings and Compatibility in windows of their own (`Window.open`), Activity and a bucket's details in
sheets (`setOverlay` in `store.ts` routes those sections there), the command palette and Canvas charts; terminals get
a navigation stack holding every place, the F10 menu bar, keyboard shortcuts, tables and sentences instead of charts.
The same screens serve both, adapting density (`terminal()` in `ui/kit.tsx`).

A bucket's overview (`ui/overview.tsx`) starts the cheap scans as a bucket or prefix opens and keeps what they found
per endpoint, bucket and prefix, shared with the sheets it opens, which reuse those results instead of scanning again.

## A recording, and what is read from it

`sonde-proxy` is the endpoint a program is pointed at (`AWS_ENDPOINT_URL`, with throwaway keys): each request is
checked against the proxy's own secret, re-signed with the real credentials, forwarded and written to the session
file (`.sonde`: a header line, then a JSON line per request and per chunk of the program's output, gzipped once it
is finished).

- **Faults** (`fault.rs`): rules that answer in the store's place — a 503, a 500, a slow answer, a lost answer to a
  write that did happen, a body cut short, a lost race on a conditional write — matched by operation and key, and
  counted per request's attempts, so a rule fails a first attempt and lets its retry through.
- **Trials** (`trial.rs`): `sonde run --trial` and the app's Trial run the program once clean, then once per
  scenario chosen from what the clean run did, and judge each run against the clean one: every failed request sent
  again, nothing more left behind, the same exit. The verdicts are kept beside the session
  (`<name>.trials.json`) and each scenario's run under `trials/`.
- **Operations and findings** (`episode.rs`): requests grouped into operations (a multipart upload, a paged
  listing, an Iceberg or Delta commit, a SlateDB manifest write) and the observations made of them.
- **Signals** (`signals.rs`): rate, errors, latency, bytes and requests in flight, each a figure with its line over
  the run, in one pass over the records.
- **Programs** (`program.rs`): a run reduced to the figures that move when a program changes, checked against a
  budget (`sonde.toml`) or a baseline run: what `sonde check` gates CI on and what the app's history shows.
- **The summary cache** (`summary.rs`): a session's counts, kept beside it as `<name>.summary.json` and used while
  the file's length and time and the cache's version are the same, so a list of sessions reads no session whole.
- **The paged view** (`view.rs`): a session as a window shows it, analysed once and then sent in pages. The view
  carries the totals, signals, profile, findings and the first page of operations; the bridge (`Sessions.view`,
  `Sessions.episodes`) keeps the analysis until the file changes, and asks for later ranges sorted and narrowed in
  Rust, so what crosses to the interface does not grow with the session.

Every figure in text — a size, a count, a length of time — is written by `sonde-fmt`, the same as the app's
`format.ts` writes it (`crates/fmt/cases.json` is the table both are tested against).

## Phrases

What sonde says about a session — operation titles, notes, observations — is built in `sonde-proxy` as phrases
(`phrase.rs`: a key and its arguments) beside the English text, which the same templates produce for the CLI. The
app and the web viewer render the phrases in the reader's language (`app/src/phrases.ts`), falling back to the English
text for a key they have no template for.
