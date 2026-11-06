# Security model

## Credential isolation

- Credentials are resolved in Rust (`sonde-core::credentials`) and held in `s3raw::Credentials`, whose `Debug`
  prints masks. They are used to sign and for nothing else.
- The TSX interface runs in QuickJS. `sonde/native` exposes endpoints as `EndpointView` (id, name, URL, region,
  vendor, *how* credentials are obtained) and never a key; the endpoint form cannot submit one (`credentials:
  "keep"` preserves configured static keys without reading them).
- Every request record is redacted at creation (`reqlog::redact`): signatures, tokens, SSE-C keys, presign
  parameters, and the secrets services echo in error bodies. The ring buffer, SQLite file, HAR/JSON exports, probe
  reports and fixtures only ever hold redacted records. "Copy as curl" carries no signature.
- `config.toml` is written with mode 0600.

## Network

- Every request is made by Rust (`reqwest` with rustls); the app's `nsh.json` declares no network capability, so
  the TSX VM cannot open a connection (`capabilities.json` in the bundle: `"network": {"hosts": [], "http": []}`).
- sonde has no telemetry, no update check and no background request: each request answers a user action
  (or a scheduled job the user started).
- Custom CAs are added per endpoint (`ca-file`); TLS verification is never disabled.

## Files

The app declares no `fs` capability: TSX cannot read or write files. Rust writes exports to the exports directory,
downloads to the downloads directory, and its state under the data directory.

## Read-only by default

`read-only = true` until the user turns it off. Destructive operations — aborting uploads, deleting versions or
delete markers, deleting objects, replacing a lifecycle configuration — are plans (`sonde-core::guard`):

1. the plan is a dry run listing exactly what would change (and, for lifecycle, the old and new documents);
2. it runs only with read-only mode off;
3. and only with the typed confirmation it names: the number of items, or the bucket name;
4. plans expire after 10 minutes and run once;
5. every run is appended to `<data>/audit.jsonl` (time, action, targets, outcome, request ids, errors).

## Plugin sandbox

Plugins are signed packages verified against roots the user trusts, with revocation lists and rollback
protection. A plugin declaring any capability with a grant other than `sonde.read.*` host calls is refused at
install. It runs in its own engine (no shared VM, no access to sonde's state); the host registers handlers only
for the read-only calls, confined to the bucket and prefix the user chose; credentials stay in Rust.
