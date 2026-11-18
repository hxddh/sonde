# What is stable

What is listed under **Stable** changes only in a major release; everything else may change in any release.

The contract starts at 19.0 (D-138). A break comes only in a major, in the major that makes it, and is said in its
[changelog](../CHANGELOG.md); nothing removed lives on as an alias.

## Stable

| | What | Documented in | Tested by |
|---|---|---|---|
| Run files | The `.sonde` format: header, records, log lines, versioning | [run-format.md](dev/run-format.md) | `the_format_as_its_spec_says` (`crates/analysis/src/run.rs`) |
| JSON outputs | Every `-f json` output: required fields, types, meanings | [schemas/](schemas/), one JSON Schema (draft 2020-12) each | `crates/sonde-cli/tests/schemas.rs` (real outputs against the schemas) |
| `sonde.toml` | `[check]` (`max-requests`, `max-waiting`, `max-errors`, `max-transfer`, `expect`, `tolerance`, `baseline`), `[program]` (`name`, `runs`, `runs-per-month`), `[record]` (`policy`, `name-callers`, `tls`), `[[scenario]]` (`name`, `fault`, `on`, `prefix`, `times`), and each value's meaning | [configuration.md](configuration.md#sondetoml), [cli.md](cli.md) (`check`), [trials.md](trials.md#a-projects-own-scenarios), [recording.md](recording.md#what-a-recorded-program-may-do) | `crates/analysis/src/project.rs` (one reader for CLI and app) |
| `config.toml` | Its place, `[settings]` (`concurrency`, `rate-limit`, `theme`, `language`, `exports-dir`, `downloads-dir`, `prices`), `[[endpoint]]` (`id`, `name`, `url`, `region`, `vendor`, `path-style`, `ca-file`, `buckets`, `sign-payload`, `credentials`, `egress`), and each value's meaning | [configuration.md](configuration.md#configtoml), [credentials.md](credentials.md) | `crates/sonde-core/src/config.rs` |
| Exit codes | Each code's meaning | [cli.md](cli.md#exit-codes) | `crates/sonde-cli/tests/cli.rs` |
| Commands and flags | Subcommands, flags, arguments, `-f` formats, the environment a recorded program gets | [cli.md](cli.md), [recording.md](recording.md) | `crates/sonde-cli/tests/cli.rs` |

| Command | Schema |
|---|---|
| `sonde show -f json` (and `--all`, `--errors`, `--requests`) | [show](schemas/show.schema.json) |
| `sonde show --by DIM -f json` | [show-by](schemas/show-by.schema.json) |
| `sonde check -f json` | [check](schemas/check.schema.json) |
| `sonde compare -f json` | [compare](schemas/compare.schema.json) |
| `sonde runs -f json` | [runs](schemas/runs.schema.json) |
| `sonde compare FILE --replay E -f json` | [compare-replay](schemas/compare-replay.schema.json) |
| `sonde compare FILE --replay A --replay B … -f json` | [compare-replay-stores](schemas/compare-replay-stores.schema.json) |
| `sonde compare ORIGINAL REPLAY -f json` | [compare-answers](schemas/compare-answers.schema.json) |
| `sonde endpoints list -f json` | [endpoints-list](schemas/endpoints-list.schema.json) |
| `sonde endpoints test -f json` | [endpoints-test](schemas/endpoints-test.schema.json) |
| `sonde bucket size -f json` | [bucket-size](schemas/bucket-size.schema.json) |
| `sonde bucket uploads -f json` | [bucket-uploads](schemas/bucket-uploads.schema.json) |
| `sonde layout -f json` | [layout](schemas/layout.schema.json) |
| `sonde probe list -f json` | [probe-list](schemas/probe-list.schema.json) |
| `sonde probe -f json` | [probe](schemas/probe.schema.json) |
| `sonde probe runs -f json` | [probe-runs](schemas/probe-runs.schema.json) |
| `sonde probe diff -f json` | [probe-diff](schemas/probe-diff.schema.json) |

## Not stable

- Text for people: `-f text`, `-f markdown`, `-f github`, all of stderr. Use `-f json`.
- Display text inside JSON: each property a schema says is "Display text, not stable": the `*Text` phrase arrays,
  a finding's and an operation's sentences (`text`, `phrases`, `title`, `notes`), `show --by`'s sentence, and `layout
  -f json`'s `headline`, `facts` and `tables` (which may also be absent). Read the figures and ids beside them.
- The desktop app, the terminal UI (its developer flags `--bundle`, `--fps`, `--log-file` too, left out of help), the
  shared HTML page, the app–Rust bridge.
- The runs folder's index and the data directory's other files (probe runs, caches); an unreadable
  cache is rebuilt.
- The Rust crates' APIs.
- A finding's text. Its `identity` (what `[check] expect` names) is made by versioned rules ([cli.md](cli.md#the-fix-loop)).

## Versions

| Release | Changes |
|---|---|
| **Major** (`25.0.0`) | a stable thing breaks: a required field removed, renamed, retyped or redefined; a new run format version; a `sonde.toml` key or flag removed or redefined; an exit code redefined |
| **Minor** (`24.1.0`) | any other change in behaviour: new commands, flags, fields, `sonde.toml` keys, findings, scenarios; app and text changes |
| **Patch** (`24.0.2`) | fixes where documented behaviour did not hold |

Within a major:

- JSON objects may gain fields; read the ones you know (schemas leave objects open). An `enum` gains values only in
  a minor that says so.
- A run's file may gain fields within its version. sonde reads its own format version and refuses any other by
  name.
- `sonde.toml` may gain keys. An older sonde refuses an unknown key (exit 64) rather than half-reading the file.

A change to a stable thing updates its documentation and its test in the same commit; a test failing on a change
means the change needs a major.
