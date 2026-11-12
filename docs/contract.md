# What is stable

sonde's version says what a user can rely on from one release to the next. What is listed under **Stable** changes
only in a major release; everything else may change in any release.

## Stable

| | What | Where it is written down | What holds it |
|---|---|---|---|
| Session files | The `.sonde` format: its header, records and log lines, and how versions are told apart | [session-format.md](session-format.md) | `the_format_as_its_spec_says` (`crates/proxy/src/session.rs`) |
| JSON outputs | Every `-f json` output (and `share --json`): the fields each schema requires, their types and meanings | [schemas/](schemas/), one JSON Schema (draft 2020-12) per output | `crates/sonde-cli/tests/schemas.rs`: the real outputs, checked against the schemas |
| `sonde.toml` | Its tables and keys: `[check]` (`max-requests`, `max-waiting`, `max-cost`, `max-errors`, `max-p95`, `expect`, `tolerance`, `no-new-warnings`, `baseline`), `[program]` (`name`), `[record]` (`policy`), `[[scenario]]` (`name`, `fault`, `on`, `prefix`, `times`), and what each value means | [cli.md](cli.md) (`check`), [trials.md](trials.md#a-projects-own-scenarios), [recording.md](recording.md#what-a-recorded-program-may-do) | `crates/proxy/src/project.rs`: one reading, for the CLI and the app |
| Exit codes | Each code's meaning | [cli.md](cli.md#exit-codes) | `crates/sonde-cli/tests/cli.rs` |
| Commands and flags | The subcommands, their flags and arguments, the formats each writes (`-f`), and the environment a recorded program is given | [cli.md](cli.md), [recording.md](recording.md) | `crates/sonde-cli/tests/cli.rs` |

The JSON outputs:

| Command | Schema |
|---|---|
| `sonde show -f json` (and `--errors`) | [show](schemas/show.schema.json) |
| `sonde show --summary -f json` | [show-summary](schemas/show-summary.schema.json) |
| `sonde check -f json` | [check](schemas/check.schema.json) |
| `sonde compare -f json` | [compare](schemas/compare.schema.json) |
| `sonde sessions -f json` | [sessions](schemas/sessions.schema.json) |
| `sonde share --json` | [share](schemas/share.schema.json) (its `profile` and `signals` are the page's, not stable) |
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

`sonde export` writes OpenTelemetry's OTLP/JSON, whose contract is OpenTelemetry's.

## Not stable

- The text a command writes for people: tables, sentences, `-f markdown`, `-f csv`, `-f github`, and everything on
  stderr. Read `-f json` instead.
- The desktop app, the terminal UI, the shared HTML page, and the bridge between the app and Rust.
- The files sonde keeps beside sessions and in its data directory (counts, verdicts, baselines, probe runs, caches):
  sonde reads its own, and makes a cache again when it does not read.
- The Rust crates' APIs: they are sonde's parts, not libraries.
- A finding's text and the phrases it is made of. Its `identity` (what `[check] expect` names) is stable.

## Versions

- **Major** (`18.0.0`): a stable thing breaks — a field a schema requires is removed, renamed or changes type or
  meaning; a session format version is added; a `sonde.toml` key or flag is removed or means something else; an exit
  code means something else.
- **Minor** (`17.1.0`): anything else that changes what sonde does — a command, a flag, a field, a `sonde.toml` key,
  a finding, a scenario added; the app and the text changed.
- **Patch** (`17.0.1`): fixes, where what was documented did not hold.

What may be added within a major, and what readers should expect of it:

- An object in a JSON output may gain fields; read the ones you know. A schema leaves objects open for that reason.
  A value an `enum` lists may gain others only in a minor that says so.
- A session file may gain fields within its version; a sonde reads every session version up to its own and
  refuses a newer one by name.
- `sonde.toml` may gain keys. An older sonde refuses a key it does not know (exit 64): a file written for a newer
  sonde says so rather than being half read.
- A deprecated flag keeps working, said on stderr, until the next major.

A change to a stable thing changes its documentation and its test in the same commit; a test that fails on a change
is the contract asking for a major.
