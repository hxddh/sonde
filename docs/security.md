# Security model

What sonde protects and how: credentials stay in Rust, the interface has no network or file access, sonde's own
destructive actions are gated, and a recorded program is limited by a policy.

## Credential isolation

- Credentials are resolved in Rust (`sonde-core::credentials`), held in `s3raw::Credentials` (its `Debug` masks
  them), and used only to sign.
- The TSX interface (QuickJS) sees endpoints as `EndpointView`: id, name, URL, region, vendor, and *how* credentials
  are obtained, never a key. The endpoint form cannot submit a key (`credentials: "keep"` keeps static keys without
  reading them).
- Every request record is redacted when created (`reqlog::redact`): signatures, tokens, SSE-C keys, presign
  parameters, secrets echoed in error bodies ([credentials](credentials.md#redaction)). The ring buffer, SQLite file,
  HAR/JSON exports, probe reports and fixtures hold only redacted records. "Copy as curl" has no signature.
- `config.toml` is written with mode 0600 (its temporary file is created so).

## Network

- All requests are made in Rust (`reqwest`, rustls). The app's `nsh.json` declares no network capability
  (`"network": {"hosts": [], "http": []}`), so the TSX VM cannot connect.
- No telemetry, no background requests: every request answers a user action or a job the user started.
- The app checks for updates only when asked (Settings › Updates): it reads `latest.json` on GitHub Releases and
  verifies the archive's SHA-256 before installing.
- Custom CAs per endpoint (`ca-file`); TLS verification is never disabled.

## Files

- The app declares no `fs` capability: TSX cannot read or write files. Rust writes exports to the exports directory,
  downloads to the downloads directory, and state under the data directory.
- On Unix, files holding a program's traffic are `0600` regardless of umask: sessions, each sessions folder's
  `.index.jsonl` (counts, trial verdicts), baselines, titles, `bucket size`'s cache, `audit.jsonl`. Files from earlier
  versions keep their mode.
- Files the user asks for elsewhere (`-o`, `sonde share`, exports) get normal permissions.
- A `sonde://open` link opens only a `.sonde` or `.sonde.gz` file, never a network path (`\\host\share`,
  `//host/share`).
- A code location in a session opens only a source or text file (allow-listed extensions) inside the session's run
  folder; anything else is shown in the file manager; a network path opens nothing.

## One safety model

What a recorded program may do is the recording policy ([Run](#run)), and nothing else. sonde's own destructive
operations (aborting uploads, deleting versions, delete markers or objects, replacing a lifecycle configuration) are
never a switch away: each is a plan (`sonde-core::guard`):

1. A plan is a dry run listing exactly what would change (for lifecycle, old and new documents).
2. It runs only with its typed confirmation: the item count or the bucket name.
3. Plans expire after 10 minutes and run once.
4. Each run is appended to `<data>/audit.jsonl` twice: `"stage": "started"` before the first request (if that cannot
   be written, the run does not start), then the outcome (time, action, targets, outcome, request ids, errors).

Probes write too (under `sonde-probe/` in the scratch bucket, and their own buckets when allowed):

- They run only on the scratch bucket the user names (in the app or on the command line): naming it is the consent.
- Each run is audited (`runProbes`: started, then the outcome, with anything cleanup could not remove).
- A cancelled run still cleans up after the probe it stopped in.

## Run

- Run starts the user's command through their shell with their rights, like a terminal.
- The command gets a local endpoint and throwaway keys, never sonde's credentials; `AWS_SESSION_TOKEN`,
  `AWS_PROFILE` and similar are removed.
- What it may do is the recording policy ([recording](recording.md#what-a-recorded-program-may-do)):
  - `no-delete` (default): no deletes, aborts, lifecycle rules, bucket policies, versioning suspension, or overwrites
    of objects the recording did not write. Overwrites are refused by the store via `If-None-Match: *`; a store that
    ignores it overwrites anyway, so use `read-only` there.
  - `read-only`: GET and HEAD only.
  - `allow` (`--allow-writes`, `[record] policy = "allow"`): everything.
- `sonde.toml` may set the policy when no flag does (sonde warns on stderr when it allows deletes); in the app it may
  only narrow the chosen one.
- The program's output is kept as printed, so printed secrets end up in the session; records stay redacted.
- Only the app process starts commands: no listening socket; the TSX VM reaches it only through `sonde/native`.
- "Name the code" (`--callers`) extends, never replaces, `PYTHONPATH`, `NODE_OPTIONS` and `JAVA_TOOL_OPTIONS` with a
  hook that adds an `x-sonde-caller` header to requests to sonde. sonde reads it and never forwards it; a failing
  hook leaves the program unchanged.
- The Runs pane's code change runs `git diff` and `git show` in the run's folder with commits checked as hex and
  fsmonitor off, so a shared session cannot make it run anything else.

## Shared sessions

`sonde share` writes a static site: the viewer and the computed document, holding only what the session file holds.
`--serve` listens on 127.0.0.1 and serves only the site's files.
