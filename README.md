# sonde

**Diagnose S3 and S3-compatible object storage.** sonde is not another file browser: it shows what the S3 API
actually did — every request with its headers, status, request id and raw error body; what multipart uploads,
versions and delete markers are costing you; why a bucket is as big as it is; how a vendor differs from AWS,
probe by probe; and what a SlateDB, Iceberg, lakeFS or Woodpecker layout on the bucket says about itself.

One codebase, two forms:

- a **macOS app** — a sidebar of endpoints and buckets, a list, a detail pane; light and dark; menus and shortcuts;
- a **terminal UI** in one static-libc binary, for the bastion host next to your data (SSH, IMDS and IRSA
  credentials work as they do for the AWS CLI) — and the same binary is a **headless CLI** for scripts and CI.

The UI is [Native Shell](https://github.com/longbridge/nsh) (TSX screens on QuickJS); everything else is Rust.

## New: see what your software does to S3

```sh
sonde run -- ./my-app          # every S3 request my-app makes, re-signed by sonde and recorded
```

```
00:22:25.053  200    16 ms   ↑130 B  PutObject      slate/db/manifest/00000000000000000001.manifest  if-none-match: *
00:22:25.074  404     4 ms           HeadObject     slate/db/wal/00000000000000000001.sst
00:22:25.112  200     2 ms   ↓250 B  ListObjectsV2  slate/  prefix=db/compactions/

172 requests to rustfs in 0.5 s · 116 not found · ↑36.5 KiB ↓22.7 KiB
```

The application gets a local endpoint and throwaway keys; sonde holds the real credentials. `--no-delete` and
`--read-only` refuse what you do not want the application to do. Sessions are redacted files you can attach to a bug
report. This is the base of sonde 1.0 — [docs/recording.md](docs/recording.md).

## Features

| | |
|---|---|
| **Endpoints** | Many endpoints, from AWS profiles or by hand (URL, region, path-style, CA file, vendor: AWS, R2, MinIO, Ceph RGW, Alibaba OSS, Tigris, Backblaze B2, Wasabi, GCS XML, other). Connectivity test with STS `GetCallerIdentity` where the vendor has it. |
| **Browse** | Buckets, prefixes and objects (`ListObjectsV2`, paged, 100 000+ rows kept in Rust and scrolled smoothly), HEAD details: every header, user metadata, ETag analysis (`-N` = N parts, consistent part sizes), full-object vs composite checksums, storage class, SSE. Download (streamed, cancellable), presigned URLs, filter and search. |
| **Request inspector** | Every request (ring buffer, 5000 by default, optional SQLite): time, operation, target, status, latency, retries, `x-amz-request-id` / `x-amz-id-2`, redacted headers, raw error body. Filters by status, operation, endpoint. Copy as `aws s3api` or as (unsigned) curl; export JSON or HAR. |
| **Multipart health** | Every incomplete upload with its bytes (summed from ListParts), age and cost at your prices; stale ones flagged (7 days by default); lifecycle `AbortIncompleteMultipartUpload` verdict; generate or apply the rule; abort in bulk. |
| **Versions** | A key's timeline with delete markers highlighted; restore (remove the marker), delete a version; versioning, MFA delete and Object Lock state. |
| **Capacity** | Current vs noncurrent bytes and objects, delete markers, incomplete uploads, by storage class, top-level prefix and size band — and findings that answer "why is it this big". Cached, exportable. |
| **Compatibility matrix** | 40 probes (conditional writes, consistency, CRC32/CRC32C/CRC64NVME/SHA1/SHA256 in full-object and composite modes and as trailers, multipart edge cases, Range, presign expiry and clock skew, metadata limits, Unicode keys, pagination tokens, StartAfter, DeleteObjects limits, version ids, Object Lock, SSE-C, S3 Express append), run on several endpoints in parallel, as a vendor × probe matrix whose cells open on the raw exchanges. Runs are saved; diff two runs to catch silent vendor changes. JSON and Markdown reports. |
| **Layout lens** | Detects and decodes SlateDB (manifests v1/v2: WAL timeline, gaps, writer/compactor epochs and fencing, L0 and sorted runs, GC lag, orphans), Apache Iceberg (snapshots, manifests, live and orphan files, expirable snapshots), lakeFS (metaranges, ranges, path → object mapping) and Milvus Woodpecker (segments, blocks, footers, LAC). turbopuffer and WarpStream formats are not published: sonde measures their prefixes and write cadence only. Replay the logical state at any time. |
| **Plugins** | Decoder and probe views as signed Native Shell packages (`.nshpkg`), verified against trusted roots and revocation lists, sandboxed: no network, files or processes — only read-only `sonde.read.*` host calls. An example: a Delta Lake `_delta_log` lens (`plugins/delta-lens`). |
| **Safety** | Read-only by default. A destructive operation needs read-only mode off, a dry run, and a typed confirmation (the item count or the bucket name); each run is written to an audit log. Credentials are resolved in Rust and never reach the UI's JavaScript; every log and export is redacted. No telemetry: sonde makes no request you did not ask for. |
| **Languages** | English and 中文. |

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/hxddh/sonde/main/install.sh | sh     # Linux and macOS terminal binary
brew install hxddh/sonde/sonde                                                   # the same, with Homebrew
brew install --cask hxddh/sonde/sonde-app                                        # the macOS app
```

Or download a release: `sonde-linux-x86_64.tar.gz`, `sonde-linux-aarch64.tar.gz`, `sonde-macos-aarch64.tar.gz`,
`sonde-macos-x86_64.tar.gz`, `sonde-macos.dmg`, with `SHA256SUMS`. The Linux binary needs only libc
(`ldd` in [docs/packaging.md](docs/packaging.md)), so it runs on minimal server images.

## Quick start

```sh
sonde tui --demo                      # the terminal UI on a built-in demo server (no account needed)
sonde endpoints import                # endpoints for your ~/.aws/config profiles
sonde endpoints test prod             # reachability, credentials source, STS identity
sonde tui                             # the terminal UI on your endpoints
```

Headless, for scripts and CI ([docs/cli.md](docs/cli.md), with exit codes):

```sh
sonde probe -e minio -e r2 -b scratch-bucket --format markdown -o report.md   # exit 1 on a failed probe
sonde mpu scan -e prod -b logs                                                # exit 4 when stale uploads or no rule
sonde du -e prod -b logs --format json
sonde layout inspect -e prod -b lake -p warehouse/db/events/                  # detect and decode
sonde --url http://127.0.0.1:9000 --path-style probe -b scratch --only checksum.
```

### The terminal UI

```text
  sonde  File  Edit  View
‹ Back Browse Demo (built-in) › demo-large                                                    Read-only
 Demo (built-in) ▾            demo-large ▾
demo-large                                                         100,000 items  [ ] Flat  Load all  Refresh
 Name                                              Size  Modified         Class            MPU
▸ logs/
```

F10 opens the menu bar, Tab and the arrows move between controls, Esc goes back, `1`–`9` open sections from the
home list, Ctrl+C copies a selection (OSC 52, over SSH too) or quits, F12 closes a plugin view.

### The macOS app

A sidebar (endpoint, buckets, sections), the section's list and a detail pane; the menu bar (⌘1–⌘9 sections, ⌘,
settings, ⌘I requests, ⌘J jobs), light and dark appearance, and the window's size and place remembered.
Screenshots are produced by CI on macOS (`nsh test --target macos`); see [docs/packaging.md](docs/packaging.md).

## Documentation

| | |
|---|---|
| [Configuration](docs/configuration.md) | `config.toml`: settings and endpoints |
| [Credentials](docs/credentials.md) | Where credentials come from, and how they stay out of the UI |
| [Recording](docs/recording.md) | `sonde run`, `sonde proxy`, `sonde show`, session files |
| [CLI](docs/cli.md) | Subcommands, formats, exit codes |
| [Probes](docs/probes.md) | Every probe and the AWS behavior it checks |
| [Decoders](docs/decoders.md) | The layouts and format versions each lens decodes |
| [Plugins](docs/plugins.md) | Writing, signing and installing plugins |
| [Security model](docs/security.md) | Credential isolation, read-only mode, the plugin sandbox |
| [Performance](docs/performance.md) | Measured numbers |
| [Architecture](docs/architecture.md) | Crates, threads, data flow |
| [Decisions](docs/decisions.md) | Product decisions and why |
| [Packaging](docs/packaging.md) | Builds, signing, notarization, release |
| [FINDINGS.md](FINDINGS.md) | Native Shell gaps met while building sonde, and the workarounds |

## Development

```sh
cargo test --workspace                       # unit tests (SigV4 vectors, redaction, parsers, judges, decoders)
scripts/integration.sh                       # Ceph RGW and RustFS in docker compose (MinIO with MINIO_IMAGE)
scripts/ui-tests.sh                          # nsh test, desktop and terminal forms, headless
cd app && nsh run --target tui               # the TSX app with hot reload
cargo run -p sonde-cli --features tui -- tui --demo
```

Licensed under Apache-2.0.
