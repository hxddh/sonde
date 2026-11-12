# 17.0, contract and safety (M1, and M2's Rust and docs)

Notes for the merge: changelog lines, decisions to record, and what CI needs. Not a page for users.

## Changelog lines

### Added

- **A declared contract** ([docs/contract.md](../contract.md)): the session format, every `-f json` output, the keys
  of `sonde.toml`, the exit codes and the flags are stable; a major only when one of them breaks, a minor otherwise.
- **The session format, written down** ([docs/session-format.md](../session-format.md)): each field of the header,
  a record and a log line, required or not, and how versions are told apart. A session's header names the sonde that
  wrote it (`sonde`); a newer format is refused by name: `written by sonde 18.0.0, format 2; this sonde (17.0.0)
  reads up to format 1`.
- **A JSON Schema for every `-f json` output** in [docs/schemas](../schemas/) (draft 2020-12, eighteen of them), and
  a test that holds the real outputs to them: `show`, `show --summary`, `check`, `compare`, `diff`, `replay` (one
  endpoint and several), `sessions`, `share --json`, `endpoints list` and `test`, `du`, `mpu scan`,
  `layout inspect`, `probe`, `probe list`, `runs` and `diff`.
- `[record] policy` in `sonde.toml`, `--policy allow|no-delete|read-only` and `--allow-writes` on `run`, `proxy` and
  `watch`; the app's Settings › Safety has the policy Run starts with (`record-policy`).

### Changed

- **A recorded program may not delete by default.** `sonde run`, `sonde proxy`, `sonde watch` and the app's Run
  record under `no-delete` unless a flag, `sonde.toml` or the setting says otherwise (it was `allow`). A program
  that deletes or writes over what it finds needs `--allow-writes`.
- **`no-delete` keeps what was there.** It also refuses a write over an object the recording did not write
  (`PutObject` and `CompleteMultipartUpload` go with `If-None-Match: *`, a `CopyObject`'s target is looked for
  first, a write on `If-Match` is refused), bucket policies (`PUT`/`DELETE ?policy`) and versioning suspended. What
  the recording wrote it may write again; a trial's runs may write again what its clean run wrote.
- **`sonde.toml` is read once, the same way**, by the CLI and the app (`sonde_proxy::project`): a file one takes the
  other takes, and a key neither knows is refused by both.
- **Files at rest are their user's alone**: sessions, their counts, trial verdicts, baselines, titles, the request
  log's SQLite file, `du`'s cache and `audit.jsonl` are made `0600` on Unix.
- In the app's project, `[record] policy` narrows Run's choice and never widens it.
- README's "Safe by default" says what read-only mode covers and what the recording policy does.

### Fixed

- A `sonde://open` link opens only a `.sonde` or `.sonde.gz` file, never a path on another machine
  (`\\host\share`), which Windows would reach with the user's credentials.

## Decisions to record

| Topic | Options | Choice and why |
|---|---|---|
| How no-delete finds an object already there | `If-None-Match: *` added to the write; a HEAD before every write; both | `If-None-Match: *` on `PutObject` and `CompleteMultipartUpload` (atomic, no extra request, no time added to the recording); a HEAD of sonde's own for `CopyObject`, whose destination conditions stores do not all take, not recorded and not timed. A store that ignores the header writes over: said in security.md, `read-only` advised there. The store's 412 comes back as sonde's 403 naming the key, as every other refusal. |
| Which objects no-delete lets a recording write again | none; those it created; those it wrote | Those it wrote with a 2xx (`Written`): a key written twice is a program's own. A trial's runs share the clean run's (CLI: one set across the runs; app: taken from the clean session's successful writes, which that run's policy already let through). |
| What a trial is recorded under | always `allow`; the run's policy | The run's policy: a trial is the program run again, and what the clean run wrote it may write again, so a program that only writes its own keys is tried under `no-delete`. A program that deletes needs `--allow-writes` for the clean run anyway. |
| Where the default policy may be widened | flag, sonde.toml, app setting | CLI: flag, else `[record] policy`, else `no-delete`; widening by `sonde.toml` is said on stderr. App: the setting (Run's choice starts there), and a project's `[record] policy` only narrows it — the app's setting is the person's standing choice, and a cloned repository should not lift it. |
| Read-only mode and Run | read-only forces Run to read-only; separate | Separate: read-only mode (on by default) covers sonde's own acts; coupling it would make every first Run read-only. The settings and docs say which covers what. |
| `sonde proxy` and `watch` | keep `allow`; the same default as `run` | The same: they hand out the same credentials. The demo (sonde's own store) records under `allow`. |
| A `"schema": "sonde/<command>/1"` field in each JSON output | add it; not | Not: some outputs are arrays, the schema's `$id` and sonde's major already name the version, and a field in every output is a change to every consumer for no information they lack. |
| Validating schemas in tests | the `jsonschema` crate; a small checker | A small checker in the test (≈150 lines) of the keywords the schemas use; a keyword outside it fails the test, so no schema promises what is not checked. No new dependency tree. |
| How strict a schema is | closed objects; open objects | Open (`additionalProperties` unset): adding a field is a minor. Required fields are those always written; optional ones are listed and typed. `share --json`'s `profile` and `signals` are left as objects: the page's, not the contract's. |
| Session format version | stay at 1; 2 for the `sonde` field | 1: the `sonde` field is optional and older readers pass over it. |
| `.sonde` readers and a newer version | refuse; read what is understood | Refuse, by name, in every reader that opens a session (`Session::read`); `sessions` still lists such a file from its header. |
| Files at rest | chmod after write; made 0600 | Made 0600 (`reqlog::private`), so no window where they are readable; a file made earlier keeps its mode; exports and `-o` files the user asked for keep the umask. The data directory itself is not changed. |

## CI

- No new job is needed: the schema check (`crates/sonde-cli/tests/schemas.rs`), the format test, the policy tests
  (`crates/proxy/tests/policy.rs`) and the 0600 tests are ordinary `cargo test`. The 0600 tests run on Unix only.
- `scripts/sdk-smoke.sh` now runs `sonde run --allow-writes` (the smoke programs delete what they wrote): the
  `record` job needs no change.

## Not done here

- The JAR built in CI, signed releases, actions pinned by SHA, `cargo-deny` (M2's CI part: another stream).
- `no-delete` letting a recording delete what it wrote itself (a scratch file a program removes): left refused, as
  the name says; worth a decision.
- Run's policy label "Block deletes" now also blocks overwrites; the UI stream may want to say so in the picker.
- `PutBucketReplication`, object-lock retention shortened under a governance bypass, ACLs: let through by
  `no-delete` (they remove no data by themselves); listed in recording.md.
- A UI test for `sonde://open` with a UNC path: `linkable` is a small pure function in `app/src/store.ts`.
