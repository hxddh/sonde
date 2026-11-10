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
- `config.toml` is written with mode 0600 (its temporary file is created with that mode, never widened).

## Network

- Every request is made by Rust (`reqwest` with rustls); the app's `nsh.json` declares no network capability, so
  the TSX VM cannot open a connection (`capabilities.json` in the bundle: `"network": {"hosts": [], "http": []}`).
- sonde has no telemetry and no background request: each request answers a user action (or a job the user
  started). The desktop app looks for an update only when asked (Settings › Updates): Native Shell's updater reads
  the project's `latest.json` on GitHub Releases and checks the archive's SHA-256 before installing it.
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
5. every run is appended to `<data>/audit.jsonl` twice: an entry with `"stage": "started"` before the first
   request (a run cut short is on record; one that cannot be recorded does not start), then the outcome (time,
   action, targets, outcome, request ids, errors).

Probe runs write too (objects under `sonde-probe/` in the scratch bucket, and buckets of their own when allowed):
the app refuses them while read-only mode is on, and each run is in the audit log (`runProbes`: started, then the
outcome with anything cleanup could not remove). A cancelled run still cleans up after the probe it stopped in.

A code place in a session opens only a source or text file (an allow-list of extensions) inside the folder the
session ran in; anything else is shown in the file manager, and a session naming a folder on another machine
(`\\host\share`) opens nothing.

## Run

Run starts the command the user typed, through their shell, with the user's rights — as a terminal would. The
command receives a local endpoint and throwaway keys, never sonde's credentials (`AWS_SESSION_TOKEN`, `AWS_PROFILE`
and the like are removed from its environment). Its output is kept in the session file as printed, so a command
that prints secrets puts them there; records stay redacted. Only the app process can start commands: there is no
listening socket for it, and the TSX VM reaches it through `sonde/native` only.

With "Name the code" (`--callers`) on, a run also gets one small hook per language: `PYTHONPATH`, `NODE_OPTIONS`
and `JAVA_TOOL_OPTIONS` are extended (never replaced). Each hook only adds an `x-sonde-caller` header to requests
going to sonde, which sonde reads and never forwards; a hook that fails leaves the program as it was.

The Runs pane's code change runs `git diff` and `git show` in the run's folder, with commits checked as hex and the
repository's fsmonitor turned off, so a shared session cannot make it run anything else.

## Shared sessions

`sonde share` writes a static site: the viewer and the session's computed document. It holds what the session file
holds and nothing more; `--serve` listens on 127.0.0.1 only and serves the site's own files.
