# CLI

`sonde` is the CLI and, with no GUI libraries, the terminal UI. Global options: `--config-dir`, `--data-dir`,
`--url`, `--region`, `--path-style`, `--ca-file` (an ad hoc endpoint `adhoc`), `--rate`, `--concurrency`,
`--log-out <file.har|file.json>` (the run's request log, redacted). Output formats: `--format table` (default),
`json`, `markdown`, `csv` where they apply. Ctrl+C cancels the running job (its requests stop).

| Command | Does |
|---|---|
| `sonde probe -e A -e B -b BUCKET [--only checksum.] [--temp-buckets] [--no-slow] [-f json\|markdown\|csv] [-o FILE]` | Runs the probes on each endpoint in parallel; saves the runs. |
| `sonde probe list` / `sonde probe runs` / `sonde probe diff RUN:EP RUN:EP` | The catalog, saved runs, changes between two runs. |
| `sonde mpu scan -e E -b B [-p PREFIX]` | Incomplete uploads, their bytes, age, cost; the lifecycle verdict. |
| `sonde mpu abort -e E -b B [--older-than-days N] [--confirm COUNT]` | A dry run; with `--confirm` (and read-only off) aborts. |
| `sonde mpu lifecycle -e E -b B [--days 7] [--confirm BUCKET]` | Adds an AbortIncompleteMultipartUpload rule: dry run, then with `--confirm`. |
| `sonde du -e E -b B [-p PREFIX] [--no-multipart] [--cached]` | The capacity breakdown and findings. |
| `sonde layout inspect -e E -b B [-p PREFIX] [--kind slatedb] [--at RFC3339]` / `--dir PATH` | Detects and decodes layouts; `--at` replays the state at a time. |
| `sonde endpoints list` / `test [ID]` / `import [PROFILES…]` | Endpoints. |
| `sonde tui [--demo] [--fps] [--no-mouse] [--bundle FILE]` | The terminal UI. |
| `sonde plugin list` / `trust LABEL root.pub` / `untrust ID` / `install PKG` / `enable` / `disable` / `remove` / `revoke LIST` | Plugins. |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Done; nothing to report |
| 1 | A probe failed (a guarantee broke), or a layout has errors |
| 2 | Probes behaved differently from AWS (and none failed) |
| 3 | Probes could not run (errors only) |
| 4 | Findings worth attention: stale uploads or no abort rule, layout warnings, a changed probe diff, a dry run |
| 64 | Usage or configuration error |
| 70 | The operation failed (connection, credentials, permission) |
| 77 | Refused: read-only mode, or the confirmation did not match |

A failed request prints its readable error, its request id and the request line from the log.
