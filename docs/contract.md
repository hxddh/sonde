# What is stable

What is listed under **Stable** changes only in a major release; everything else may change in any release.

## Stable

| | What | Documented in | Tested by |
|---|---|---|---|
| Session files | The `.sonde` format: header, records, log lines, versioning | [session-format.md](session-format.md) | `the_format_as_its_spec_says` (`crates/proxy/src/session.rs`) |
| JSON outputs | Every `-f json` output (and `share --json`): required fields, types, meanings | [schemas/](schemas/), one JSON Schema (draft 2020-12) each | `crates/sonde-cli/tests/schemas.rs` (real outputs against the schemas) |
| `sonde.toml` | `[check]` (`max-requests`, `max-waiting`, `max-cost`, `max-errors`, `max-p95`, `expect`, `tolerance`, `no-new-warnings`, `baseline`), `[program]` (`name`), `[record]` (`policy`), `[[scenario]]` (`name`, `fault`, `on`, `prefix`, `times`), and each value's meaning | [cli.md](cli.md) (`check`), [trials.md](trials.md#a-projects-own-scenarios), [recording.md](recording.md#what-a-recorded-program-may-do) | `crates/proxy/src/project.rs` (one reader for CLI and app) |
| Exit codes | Each code's meaning | [cli.md](cli.md#exit-codes) | `crates/sonde-cli/tests/cli.rs` |
| Commands and flags | Subcommands, flags, arguments, `-f` formats, the environment a recorded program gets | [cli.md](cli.md), [recording.md](recording.md) | `crates/sonde-cli/tests/cli.rs` |

| Command | Schema |
|---|---|
| `sonde show -f json` (and `--errors`) | [show](schemas/show.schema.json) |
| `sonde show --summary -f json` | [show-summary](schemas/show-summary.schema.json) |
| `sonde check -f json` | [check](schemas/check.schema.json) |
| `sonde compare -f json` | [compare](schemas/compare.schema.json) |
| `sonde sessions -f json` | [sessions](schemas/sessions.schema.json) |
| `sonde share --json` | [share](schemas/share.schema.json) (`profile` and `signals` not stable) |
| `sonde replay -e E -f json` | [replay](schemas/replay.schema.json) |
| `sonde replay -e A -e B … -f json` | [replay-stores](schemas/replay-stores.schema.json) |
| `sonde diff -f json` | [diff](schemas/diff.schema.json) |
| `sonde endpoints list -f json` | [endpoints-list](schemas/endpoints-list.schema.json) |
| `sonde endpoints test -f json` | [endpoints-test](schemas/endpoints-test.schema.json) |
| `sonde du -f json` | [du](schemas/du.schema.json) |
| `sonde mpu scan -f json` | [mpu-scan](schemas/mpu-scan.schema.json) |
| `sonde layout inspect -f json` | [layout-inspect](schemas/layout-inspect.schema.json) |
| `sonde probe list -f json` | [probe-list](schemas/probe-list.schema.json) |
| `sonde probe -f json` | [probe](schemas/probe.schema.json) |
| `sonde probe runs -f json` | [probe-runs](schemas/probe-runs.schema.json) |
| `sonde probe diff -f json` | [probe-diff](schemas/probe-diff.schema.json) |

`sonde export` writes OpenTelemetry OTLP/JSON; that contract is OpenTelemetry's.

## Not stable

- Text for people: tables, sentences, `-f markdown`, `-f csv`, `-f github`, all of stderr. Use `-f json`.
- The desktop app, the terminal UI, the shared HTML page, the app–Rust bridge.
- Files beside sessions and in the data directory (counts, verdicts, baselines, probe runs, caches); an unreadable
  cache is rebuilt.
- The Rust crates' APIs.
- A finding's text. Its `identity` (what `[check] expect` names) is stable.

## Versions

| Release | Changes |
|---|---|
| **Major** (`18.0.0`) | a stable thing breaks: a required field removed, renamed, retyped or redefined; a new session format version; a `sonde.toml` key or flag removed or redefined; an exit code redefined |
| **Minor** (`17.1.0`) | any other change in behaviour: new commands, flags, fields, `sonde.toml` keys, findings, scenarios; app and text changes |
| **Patch** (`17.0.1`) | fixes where documented behaviour did not hold |

Within a major:

- JSON objects may gain fields; read the ones you know (schemas leave objects open). An `enum` gains values only in
  a minor that says so.
- A session file may gain fields within its version. sonde reads every version up to its own and refuses a newer one
  by name.
- `sonde.toml` may gain keys. An older sonde refuses an unknown key (exit 64) rather than half-reading the file.
- A deprecated flag keeps working, with a note on stderr, until the next major.

A change to a stable thing updates its documentation and its test in the same commit; a test failing on a change
means the change needs a major.
