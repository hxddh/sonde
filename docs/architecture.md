# Architecture

```
app/                      TSX screens (Native Shell), theme, nsh.json        ─┐ presentation only
viewer/                   the web viewer for `sonde share` (TSX, web target)  ─┤
crates/sonde-nsh          `sonde/native`: #[module] Sonde, Sessions,          ─┤ type conversion
                          #[class] Listing; Run's PTY (portable-pty)          │
crates/sonde-cli          `sonde`: headless subcommands; `sonde tui` host     ─┘
crates/proxy (sonde-proxy) the recording endpoint, sessions, episodes,     ─┐
                          observations, replay and comparison                │
crates/sonde-core         config, vendors, credentials, jobs, guard + audit,  │
                          services: browse, mpu, versions, du, layout, exec;  │ no UI framework
                          demo S3 server                                      │ (≈ 80 % of the code)
crates/probes             40 probes, runner, run store, diff, matrix, reports │
crates/decoders           SlateDB, Iceberg, lakeFS, Woodpecker, cadence       │
crates/s3raw              SigV4, addressing, retries, throttle, XML, ops      │
crates/reqlog             records, redaction, ring buffer, SQLite, HAR        ─┘
```

Dependencies point down: `reqlog ← s3raw ← sonde-core ← probes`, `reqlog ← s3raw ← sonde-proxy`, `decoders ← sonde-core`, and only `sonde-nsh` and
`sonde-cli` (feature `tui`) depend on Native Shell. Replacing the UI framework keeps every crate but `sonde-nsh` and
`app/`.

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

## Phrases

What sonde says about a session — operation titles, notes, observations — is built in `sonde-proxy` as phrases
(`phrase.rs`: a key and its arguments) beside the English text, which the same templates produce for the CLI. The
app and the web viewer render the phrases in the reader's language (`app/src/phrases.ts`), falling back to the English
text for a key they have no template for.
