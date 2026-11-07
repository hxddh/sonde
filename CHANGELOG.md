# Changelog

All notable changes are listed here. The project follows [Semantic Versioning](https://semver.org).

## [Unreleased]

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
