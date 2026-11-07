# Changelog

All notable changes are listed here. The project follows [Semantic Versioning](https://semver.org).

## [Unreleased]

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
