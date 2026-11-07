# sonde

**See what your software does to S3.** Run an application through sonde and read its object-storage traffic as
operations — a SlateDB manifest write, an Iceberg commit, a multipart upload — then replay the same traffic on
another endpoint and see where the two disagree.

![Run: a program in its terminal, its S3 requests as operations beside it](docs/images/run.png)

```sh
sonde run -- ./my-app                    # record every S3 request my-app makes
sonde show                               # … as operations, with what stands out
sonde replay session.sonde -e ceph       # the same traffic on another store: what differs?
```

```
00:22:25.074  SlateDB  looked for the newest WAL: probed ids 1…128        8 reqs · 5 ms
00:22:25.085  SlateDB  WAL 1 written empty (fencing marker)              2 reqs · 9 ms  · after 1 lookup that found nothing
01:09:10.149  Iceberg  commit: table metadata v2 · 3 files (7.6 KiB)    20 reqs · 112 ms
01:09:10.597  Iceberg  1 file written, no commit followed (72 B)          3 reqs · 18 ms

116 requests (6 operations) to rustfs in 0.7 s · ↑58.1 KiB ↓62.6 KiB
! files were written with no commit after them: orphans unless a later commit adds them
· 21 files under 5 MiB were uploaded as multipart uploads: 3 requests each where one PUT would do
```

- **Record.** The application talks to a local endpoint with throwaway keys; sonde signs each request with the real
  credentials (profiles, SSO, IMDS, IRSA — they never reach the application), forwards it, and writes a redacted
  session file. `--no-delete` and `--read-only` refuse what you do not want it to do. Checked with boto3, the AWS SDKs
  for JavaScript, Go and Java, and object_store; +0.2 ms per request.
- **Understand.** Requests become operations: multipart uploads, retries, bursts of lookups, paged listings, and the
  steps of SlateDB, Apache Iceberg and Delta Lake, each with the reads and files that led to it. Observations name
  what deserves a look: unfinished uploads, files no commit references, retries, lost conditional races, hot keys.
- **Replay.** The session re-enacted on another endpoint under a fresh prefix — upload ids, ETags and tokens mapped,
  payloads synthetic, cleaned up afterwards — and every answer compared. A store that ignores `If-None-Match`
  shows up as `PutObject recorded 412 → replay 200`.

**Run** is the app's home: type a command and press Return. The program runs in a real terminal on the left; its
requests appear on the right as operations while it works, and when it ends — with a notification — the session
holds both, lined up in time: choose an operation and the lines it printed light up, choose a line and the operations
running then do. **Sessions** keep every run: as operations or as a timeline, replayed on another endpoint,
compared with another session side by side, opened in a window of their own, or shared as a web page with
`sonde share`. **Store** keeps sonde's tools for buckets: browse 100 000+ objects, multipart health, versions,
capacity, 40 compatibility probes, and lenses that decode SlateDB, Iceberg, lakeFS and Woodpecker layouts.
Everything else is in **Settings** — and everything is one ⌘K away in the command palette.

One codebase, three forms: a **desktop app** for macOS, Linux and Windows (it updates itself from GitHub Releases), a
**terminal UI** in one static-libc binary for the host next to your data, and a **headless CLI** for scripts and CI.
The UI is [Native Shell](https://github.com/longbridge/nsh) (TSX on QuickJS); everything else is Rust. Read-only by
default, no telemetry.

## Features

| | |
|---|---|
| **Run** | A command run in the app's terminal (a real PTY: colours, progress bars, input) with its S3 traffic recorded; the output and the operations lined up in time; a notification when it ends. |
| **Recording** | `sonde run -- <command>` or `sonde proxy`, or a recording endpoint started in the app: a local S3 endpoint that re-signs, forwards and records every request; signed aws-chunked bodies, presigned URLs and multipart uploads handled; policies `--no-delete`, `--read-only`. Sessions are redacted JSON lines ([docs/recording.md](docs/recording.md)). |
| **Operations** | Sessions read as operations (multipart uploads, retries, lookups, listings; SlateDB, Iceberg and Delta steps) with observations; `sonde show`, or Sessions — a table with each operation's outcome and place in time, narrowed by what stands out, an inspector down to each request, or a timeline. |
| **Replay** | `sonde replay <session> -e <endpoint>`: the traffic re-enacted on another store under a fresh prefix and compared request by request; `sonde diff` for two files. In the app: Replay from a session, then the differences grouped and both sessions as transcripts side by side. |
| **Share** | `sonde share`: a session as a static web page (operations, observations, output, every request) for anyone with a browser — write it to a folder, host it anywhere (S3 static website hosting works), or `--serve` it. |
| **Endpoints** | Many endpoints, from AWS profiles or by hand (URL, region, path-style, CA file, vendor: AWS, R2, MinIO, Ceph RGW, Alibaba OSS, Tigris, Backblaze B2, Wasabi, GCS XML, other). Connectivity test with STS `GetCallerIdentity` where the vendor has it. |
| **Browse** | Buckets, prefixes and objects (`ListObjectsV2`, paged, 100 000+ rows kept in Rust and scrolled smoothly), HEAD details: every header, user metadata, ETag analysis (`-N` = N parts, consistent part sizes), full-object vs composite checksums, storage class, SSE. Download (streamed, cancellable), presigned URLs, filter and search. |
| **Request inspector** | Every request (ring buffer, 5000 by default, optional SQLite): time, operation, target, status, latency, retries, `x-amz-request-id` / `x-amz-id-2`, redacted headers, raw error body. Filters by status, operation, endpoint. Copy as `aws s3api` or as (unsigned) curl; export JSON or HAR. |
| **Multipart health** | Every incomplete upload with its bytes (summed from ListParts), age and cost at your prices; stale ones flagged (7 days by default); lifecycle `AbortIncompleteMultipartUpload` verdict; generate or apply the rule; abort in bulk. |
| **Versions** | A key's timeline with delete markers highlighted; restore (remove the marker), delete a version; versioning, MFA delete and Object Lock state. |
| **Capacity** | Current vs noncurrent bytes and objects, delete markers, incomplete uploads, by storage class, top-level prefix and size band — and findings that answer "why is it this big". Cached, exportable. |
| **Compatibility matrix** | 40 probes (conditional writes, consistency, CRC32/CRC32C/CRC64NVME/SHA1/SHA256 in full-object and composite modes and as trailers, multipart edge cases, Range, presign expiry and clock skew, metadata limits, Unicode keys, pagination tokens, StartAfter, DeleteObjects limits, version ids, Object Lock, SSE-C, S3 Express append), run on several endpoints in parallel, as a vendor × probe matrix whose cells open on the raw exchanges. Runs are saved; diff two runs to catch silent vendor changes. JSON and Markdown reports. |
| **Layout lens** | Detects and decodes SlateDB (manifests v1/v2: WAL timeline, gaps, writer/compactor epochs and fencing, L0 and sorted runs, GC lag, orphans), Apache Iceberg (snapshots, manifests, live and orphan files, expirable snapshots), lakeFS (metaranges, ranges, path → object mapping) and Milvus Woodpecker (segments, blocks, footers, LAC). turbopuffer and WarpStream formats are not published: sonde measures their prefixes and write cadence only. Replay the logical state at any time. |
| **Command palette** | ⌘K (`:` in the terminal): every place, tab and action — stop the run, open the last session, read-only, theme, language, check for updates. |
| **Safety** | Read-only by default. A destructive operation needs read-only mode off, a dry run, and a typed confirmation (the item count or the bucket name); each run is written to an audit log. Credentials are resolved in Rust and never reach the UI's JavaScript; every log and export is redacted. No telemetry: sonde makes no request you did not ask for. |
| **Languages** | English and 中文. |

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/hxddh/sonde/main/install.sh | sh     # Linux and macOS terminal binary
brew install hxddh/sonde/sonde                                                   # the same, with Homebrew
brew install --cask hxddh/sonde/sonde-app                                        # the macOS app
```

Or download a release: `sonde-linux-x86_64.tar.gz`, `sonde-linux-aarch64.tar.gz`, `sonde-macos-aarch64.tar.gz`,
`sonde-macos-x86_64.tar.gz`, `sonde-macos.dmg`, the desktop app for Linux (`sonde-<version>-linux-x86_64.tar.gz`,
GTK 4) and Windows (`sonde-<version>-windows-x86_64.zip`), with `SHA256SUMS`. The Linux binary needs only libc
(`ldd` in [docs/packaging.md](docs/packaging.md)), so it runs on minimal server images.

## Quick start

```sh
sonde run -- python etl.py            # record an application's S3 traffic (endpoint from --url or the config)
sonde show                            # the newest session, as operations
sonde share --serve                   # … as a web page
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

The home list holds the four places: Run, Sessions, Store, Settings (`1`–`4`). Run works there too — the program
in a terminal inside the terminal. `:` opens the command palette, F10 the menu bar; Tab and the arrows move between
controls, Esc goes back, Ctrl+C copies a selection (OSC 52, over SSH too) or quits.

### The desktop app

A first launch offers the built-in demo, the AWS profiles it finds, or an endpoint by hand. Then it opens on Run.
The toolbar is the window's title bar: where you are, the program running, the endpoint (searchable), the read-only
lock and search (⌘K / Ctrl+K). The sidebar holds the places and recent sessions (⌘1 Run, ⌘2 Sessions, ⌘3 Store,
⌘, Settings; ⌘B hides it). Light and dark, English and 中文, the window's size and place remembered, updates checked
only when you ask.

| | |
|---|---|
| ![A session: its figures, what stands out as filters, operations with outcome and time, the inspector](docs/images/session.png) | ![The timeline, dark](docs/images/timeline-dark.png) |
| ![Store: buckets and prefixes as a tree, objects, an object's details](docs/images/store.png) | ![The command palette](docs/images/palette.png) |

Every place is screenshotted on a real window in CI (`scripts/shots.sh`: wide and at the least size, light and
dark, English and Chinese); the images are the build's `sonde-shots` artifact.

## Documentation

| | |
|---|---|
| [Configuration](docs/configuration.md) | `config.toml`: settings and endpoints |
| [Credentials](docs/credentials.md) | Where credentials come from, and how they stay out of the UI |
| [Recording](docs/recording.md) | Run, `sonde run`, `sonde proxy`, `sonde show`, `sonde share`, session files |
| [CLI](docs/cli.md) | Subcommands, formats, exit codes |
| [Probes](docs/probes.md) | Every probe and the AWS behavior it checks |
| [Decoders](docs/decoders.md) | The layouts and format versions each lens decodes |
| [Security model](docs/security.md) | Credential isolation, read-only mode, what runs where |
| [Performance](docs/performance.md) | Measured numbers |
| [Architecture](docs/architecture.md) | Crates, threads, data flow |
| [Decisions](docs/decisions.md) | Product decisions and why |
| [Packaging](docs/packaging.md) | Builds, signing, notarization, release |
| [FINDINGS.md](FINDINGS.md) | Native Shell gaps met while building sonde, and the workarounds |

## Development

```sh
cargo test --workspace                       # unit tests (SigV4 vectors, redaction, parsers, judges, decoders)
scripts/integration.sh                       # Ceph RGW and RustFS in docker compose (MinIO with MINIO_IMAGE)
scripts/ui-tests.sh                          # nsh test, desktop and terminal forms and the web viewer, headless
cd app && nsh run --target tui               # the TSX app with hot reload
cargo run -p sonde-cli --features tui -- tui --demo
```

Licensed under Apache-2.0.
