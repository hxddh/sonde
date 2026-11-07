# Architecture

```
app/                      TSX screens (Native Shell), theme, nsh.json        ─┐ presentation only
crates/sonde-nsh          `sonde/native`: #[module] Sonde, Sessions,          ─┤ type conversion
                          #[class] Listing                                    │
                          plugins (nsh-package verification, host calls)      │
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
`nsh-tui-host::convert`), an OSC 52 clipboard and a second `AppView` for plugin views.

## The two forms

`App.tsx` branches on `platform.idiom === "terminal"`: windows get the sidebar / list / detail layout and a
Canvas for charts; terminals get a navigation stack, the F10 menu bar, keyboard shortcuts, tables and sentences
instead of charts. The same screens serve both, adapting density (`terminal()` in `ui/kit.tsx`).
