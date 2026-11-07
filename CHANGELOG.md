# Changelog

All notable changes are listed here. The project follows [Semantic Versioning](https://semver.org).

## [Unreleased]

## [3.0.0] - 2026-11-07

Fewer places, each doing more by itself. The app has three places — Run, Sessions, Buckets — and says what it
noticed without being asked; Settings, sonde's own activity and compatibility move out of the way; and what sonde
says about a session reads in Chinese as well as English.

### Added

- Buckets open on an overview above the objects: incomplete uploads and what they cost, versioning, the layouts
  sonde recognises, the size once measured — each card shown only when it has something to say, filled in as the
  bucket opens, and opening its detail (the uploads and the lifecycle verdict, a key's versions, the size by class
  and prefix, a decoded layout) in a sheet for the prefix on screen. No forms to fill first.
- Activity (toolbar, ⌘⇧A): sonde's own requests, its jobs and the changes it made, in a sheet from the window's edge;
  the toolbar button counts the jobs running.
- Settings in a window of their own (⌘, / Ctrl+,): General and Endpoints.
- Compatibility opens from Settings › Endpoints in its own window.
- Open a `.sonde` file recorded elsewhere or sent to you: ⌘O, the palette, or Sessions.
- Resting on an observation shows why it matters (a card on hover).
- Operations, notes and observations are localised: the analysis is structured phrases (a key and its arguments)
  rendered in the app and the shared web page in English or Chinese. The CLI prints the same English as before.
- Screenshots in CI on macOS and Windows as well as Linux (`sonde-shots-<platform>` artifacts).
- A run's end is announced to screen readers.

### Changed

- The sidebar holds three places: Run, Sessions, Buckets (Store renamed). Activity and Settings left it.
- On macOS, floating surfaces (the palette, sheets, cards, menus) use a material over what is behind them: Liquid
  Glass on macOS 26, a blur before. Overview cards rise into place (not with reduced motion).
- On Run, the status band says a run ended; the toast is for when you are elsewhere.
- Run's introduction is shorter; "Multipart" and "Capacity" read "Incomplete uploads" and "Size".
- The terminal UI keeps every place on its home list (Run, Sessions, Buckets, Activity, Settings) and the bucket
  tools as tabs.

### Removed

- The Requests, Jobs and Audit pages (now Activity), and the Multipart, Versions, Capacity, Layouts and
  Compatibility tabs in a window (now the overview's sheets and the Compatibility window).

## [2.1.0] - 2026-11-07

The desktop app, made with care: a design system before the pages, Native Shell's components used to the full, and
every place checked on a real window, light and dark, English and Chinese, wide and at the least size.

### Added

- A first launch welcomes: the built-in demo, the AWS profiles it finds, or an endpoint by hand, and an example
  session, instead of a Run page with nothing to run against.
- A title bar of the app's own: where you are, the program running, a searchable endpoint switcher, the read-only
  lock, search; the menus behind a button on Linux and Windows.
- Sessions: the figures at the top; what stands out as chips that narrow the operations to those it is about; the
  operations with an outcome dot, family colors and a bar of their place in time; an inspector from an operation to
  its requests to one request (headers and error bodies that fold, the XML highlighted); native context menus.
- A timeline drawn on a Canvas: a time axis, outcome colors, a card on hover, drag to zoom.
- Run: one command bar with recent commands, the working folder and the policy; a band of live figures; a toast when
  it ends; the program in the tray while it runs.
- Store: buckets and their prefixes as a tree; one field that filters and searches; an object's details as an
  inspector with sections that fold; context menus.
- Settings saved as they change, with Undo; endpoints edited in a side sheet, removed after a confirmation.
- The palette: icons, platform shortcuts, recent commands and sessions, endpoints and buckets.
- Icons (Lucide) throughout; toasts for notices and errors; tooltips with shortcuts.
- `scripts/shots.sh` and CI's `shots` job: screenshots of every place as a build artifact.

### Changed

- Shortcuts read as the platform writes them (Ctrl+K on Linux and Windows).
- A narrow window folds the sidebar away and shows the inspector over the content.
- Observations name the operations they are about; operations count their lookups that found nothing.
- The example session is named as a user would have run it.
- The web viewer draws the new session views.

### Fixed

- A remembered window smaller than the least size (a phone-sized first window on Linux) is no longer restored.
- The palette's highlighted row shows.

## [2.0.0] - 2026-11-07

sonde is now one gesture: type a command and watch what it does to your object store. Run is the home, the app has
four places and a command palette, and it runs on macOS, Linux and Windows.

### Added

- **Run**: a command runs in a real terminal inside the app (a PTY; ConPTY on Windows) with its S3 traffic recorded;
  its requests appear beside it as operations while it runs, and a notification says when it ends. It works in
  the terminal UI too.
- The program's output is kept in the session with the time it arrived, and Sessions line it up with the
  operations: choose an operation and the lines it printed light up, choose a line and the operations running then do.
- Sessions: a waterfall of operations; **Replay** on another endpoint from the app (the plan in a dialog, refused
  while read-only); **Compare** two sessions — the differences grouped, and both as transcripts side by side; open a
  session in a window of its own.
- `sonde share`: a session as a static web page anyone can open (operations, observations, output, every request),
  written to a folder or served with `--serve`; `--json` for the computed document alone.
- A command palette (⌘K; `:` in the terminal UI) with every place, tab and action.
- The desktop app for Linux (GTK 4) and Windows, built and released by CI.
- Updates in the desktop app (Settings › Updates): check, download, install and restart, from GitHub Releases; only
  when asked. Releases publish `latest.json`.

### Changed

- Four places — Run, Sessions, Store (the bucket tools as tabs) and Settings (configuration, endpoints, requests,
  jobs, audit) — replace the nine sections; ⌘1–⌘3 and ⌘, reach them, `1`–`4` in the terminal's home list.
- The Timeline became Sessions.
- Native Shell updated to `beeb9fc`.
- The first window opens at 1280×820, big enough for Run's two panes.

### Removed

- The plugin system: signed packages, trust roots, revocation lists, `sonde plugin`, the Plugins screen and the
  Delta lens example. Layout decoders stay built in.

### Fixed

- The demo server outlived only the runtime that started it first; it now runs on a thread of its own.

## [1.0.0] - 2026-11-07

sonde now looks at traffic, not only at buckets: record what an application does to its object store, read it as
operations, and replay it on another endpoint to see where they disagree. The Timeline is the app's home.

### Added

- `sonde run -- <command>`: runs a command against a local S3 endpoint that re-signs every request with sonde's
  credentials, forwards it and records it; `--live` prints requests as they happen.
- `sonde proxy`: the same endpoint, standing, for an application started elsewhere.
- `--no-delete` and `--read-only` for both: refused operations never leave the machine.
- Session files (`.sonde`, redacted JSON lines) and `sonde show`: requests grouped into operations — multipart
  uploads, retries, bursts of lookups, paged listings, and SlateDB, Iceberg and Delta steps (a commit with the reads
  and files that led to it) — followed by observations; `--requests` for one line per request, JSON with all of it.
- `sonde replay <session> -e <endpoint>`: re-enacts a session on another endpoint under a fresh key prefix (upload
  ids, ETags and continuation tokens mapped; payloads synthetic; bucket changes left out), cleans up, and compares
  every answer — exit 2 when anything differs. `sonde diff` compares two session files.
- The Timeline (macOS app and terminal UI): sessions, operations, their requests and the inspector; Record starts a
  local endpoint for the current endpoint and shows what to point an application at; an example session.
- Records keep control documents (DeleteObjects, tagging, lifecycle XML), for replay.
- `sonde-proxy` crate; SDK smoke tests (boto3, JS v3, Go v2, Java v2) run through `sonde run` in CI.

### Changed

- The app opens on the Timeline; in the terminal's home list it is `1`, so the other sections moved down by one.
- Integration tests run on Ceph RGW and RustFS; MinIO is optional (`MINIO_IMAGE`), its public images are gone.

### Fixed

- `install.sh` checks a `.sha256` per file: releases now publish them.
- The IMDSv2 token request sends `Content-Length: 0` (some metadata servers refused it with 411).
- The macOS disk image is sized explicitly (hdiutil sometimes ran out of room).

## [0.1.0] - 2026-11-06

### Added

- Endpoints: configuration (`config.toml`), import from AWS profiles, connectivity test with STS identity.
- Credentials: environment, profiles (static, `credential_process`, SSO, `role_arn`, web identity), IRSA,
  container credentials, IMDSv2; never exposed to the UI.
- `s3raw`: SigV4 (AWS test vectors), path- and virtual-hosted-style, custom CAs, retries with backoff, throttles.
- Request inspector: ring buffer and SQLite, redaction, filters, copy as aws cli / curl, JSON and HAR export.
- Browse: buckets, prefixes, 100 000+ objects, HEAD details (ETag and checksum analysis, SSE), downloads,
  presigned URLs, search.
- Multipart health, versions and delete markers, capacity breakdown.
- 40 compatibility probes, a vendor × probe matrix, saved runs and diffs, JSON and Markdown reports.
- Layout lens: SlateDB, Apache Iceberg, lakeFS, Milvus Woodpecker; cadence for turbopuffer and WarpStream; replay.
- Signed, sandboxed plugins; the Delta Lake lens example.
- Read-only by default; dry runs, typed confirmations and an audit log for destructive operations.
- The macOS app and the terminal UI from one TSX codebase; English and Chinese.
- `sonde` CLI: `probe`, `mpu`, `du`, `layout`, `endpoints`, `plugin`, `tui`.
