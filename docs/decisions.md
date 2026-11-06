# Decisions

Product and engineering decisions, with the options considered and why. Newest last.

| # | Decision | Options | Choice and reason |
|---|---|---|---|
| D-001 | Language of the core | Rust; TSX with Rust helpers | **Rust for everything but screens.** The core (S3, credentials, jobs, probes, decoders) is testable without a UI and survives a UI change; TSX holds presentation only. |
| D-002 | Native Shell version | crates.io 0.4.0; a git commit | **`longbridge/nsh` at commit `9d12096`** (0.4.0 + main), in `Cargo.toml` (`rev = …`) and in CI's nsh install (`NSH_REV`). The CLI must match the crates it links; a commit pins both. |
| D-003 | SigV4 | `aws-sigv4`; our own | **Our own (`s3raw::sigv4`, ~150 lines)**, tested against the AWS SigV4 test suite and the S3 documentation's examples. It exposes the canonical request and string to sign, which a diagnosis tool wants, and handles `STREAMING-UNSIGNED-PAYLOAD-TRAILER` and deliberate clock skew for probes. |
| D-004 | HTTP | `reqwest` + rustls; hyper | **reqwest with rustls and native roots**: custom CAs per endpoint, no OpenSSL (static-libc binaries). Redirects are not followed: a 301/307 is a finding (wrong region / addressing). |
| D-005 | Where the listing lives | rows in JS; rows in Rust | **Rust (`Listing` class)**; TSX asks for the visible window. 100 000 rows never cross into the VM. |
| D-006 | Rust ↔ TSX types | typed `JsObject` structs; JSON | **JSON (`JsonValue`) mirrored by `app/src/types.ts`.** Deriving NSH traits on core types would make `sonde-core` depend on NSH, which the layering forbids; a second set of structs in `sonde-nsh` would duplicate every type. |
| D-007 | Static keys in the UI | a form field; config file only | **Config file only.** A key typed into TSX would pass through the JS VM; static keys are for local test servers and live in `config.toml` (0600). |
| D-008 | Destructive confirmation | a checkbox; typed text | **Typed text**: the number of items (many) or the bucket name (one item, lifecycle). Plans expire in 10 minutes and run once. |
| D-009 | Hermetic UI tests | mock in TSX; a server | **A demo S3 server in `sonde-core::demo`** (in-process, 127.0.0.1): real HTTP through the real client, with versions, delete markers, uploads, 100 000 synthetic keys and the real decoder samples. It doubles as `sonde tui --demo`. |
| D-010 | The terminal host | `nsh run --target tui` / `nsh-tui-host`; our own loop | **Our own loop on `nsh-platform-tui`** (FINDINGS #2): one binary embeds the bundle, takes sonde's CLI arguments, and runs plugin views in a second engine. |
| D-011 | Plugin views in the macOS app | a custom AppKit host; degrade | **Degrade the presentation** (FINDINGS #3): `nsh package --target macos` builds NSH's runner, which hosts one app; the app manages plugins and opens their views in `sonde tui`. The plugin mechanism (verification, trust, revocation, sandbox) is the same. |
| D-012 | Plugin capability policy | trust the package; check | **Check at install**: only `host` calls under `sonde.read.*`; any other key must grant nothing (`nsh publish` writes every key). Checked again when a plugin opens. |
| D-013 | Second S3 implementation for CI | Ceph RGW; Garage; SeaweedFS | **Ceph RGW (`ceph/daemon demo`)** as the spec asks, plus **RustFS** (MinIO-compatible, starts in seconds) and MinIO in CI. |
| D-014 | Iceberg sample | hand-written JSON/Avro; pyiceberg | **pyiceberg 0.9.1** (`tools/gen_iceberg.py`): a real table with appends, an overwrite, a delete, an orphan and expirable snapshots. |
| D-015 | Woodpecker sample | a running Woodpecker; its codec | **Woodpecker's own codec** (`tools/gen-woodpecker`, Go, module at commit 3a8ce69) laying objects out as its object-storage writer does; running Woodpecker needs etcd and MinIO, which CI has no reason to host. |
| D-016 | lakeFS commits | parse KV dumps; trees only | **Trees only**: commits live in lakeFS's KV store, not in the bucket; each metarange is shown as a committed tree, and the UI says so. |
| D-017 | Closed formats | heuristics; cadence only | **Cadence only** for turbopuffer and WarpStream, with "format not published, not decoded" in the UI. |
| D-018 | Probe verdict "differs" vs "fail" | — | **fail** when a guarantee breaks (a condition ignored, a wrong checksum accepted, wrong bytes); **differs** for other statuses or shapes (409 for 412, uuid version ids). |
| D-019 | Exit codes | 0/1; graded | **Graded** (docs/cli.md): CI can fail on broken guarantees (1) and still report differences (2). |
| D-020 | Linux binary | musl; glibc on an old base | **glibc, built on Ubuntu 22.04** (glibc 2.35): only libc/libm/libgcc_s; QuickJS's C build is untroubled; runs on Debian 12 slim (checked in CI). |
| D-021 | Config path on macOS | `~/.config/sonde`; Application Support | **`~/Library/Application Support/sonde`**, the platform's convention; `$SONDE_CONFIG_DIR` and `$XDG_CONFIG_HOME` override on both. |
| D-022 | App capabilities | `fs` for exports; none | **`clipboard` only**: Rust writes every file; the VM needs no `fs` or `network`. |
