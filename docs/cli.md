# CLI

`sonde` is the CLI and, with no GUI libraries, the terminal UI. Global options: `--config-dir`, `--data-dir`,
`--url`, `--region`, `--path-style`, `--ca-file` (an ad hoc endpoint `adhoc`), `--rate`, `--concurrency`,
`--log-out <file.har|file.json>` (the run's request log, redacted). Output formats: `--format table` (default),
`json`, `markdown`, `csv` where they apply. Ctrl+C cancels the running job (its requests stop).

| Command | Does |
|---|---|
| `sonde run [--no-delete\|--read-only] [--live] [-o FILE] -- COMMAND…` | Runs a command against a local endpoint that re-signs and records its S3 requests; prints the session's summary; exits with the command's status. |
| `sonde proxy [--listen ADDR] [--no-delete\|--read-only] [-o FILE]` | The same endpoint, standing: prints the variables to set, then every request. |
| `sonde show [FILE] [--requests] [--errors] [--summary] [-f json]` | A session (default: the newest) as operations (or requests) and observations. |
| `sonde replay FILE -e E [--bucket A=B] [--prefix P\|--in-place] [--keep] [--yes] [-o FILE]` | Re-enacts a session on another endpoint, cleans up, compares every answer. |
| `sonde diff ORIGINAL REPLAY` | Compares a session with its replay. |
| `sonde share [FILE] [-o DIR] [--serve [--port N]] [--json]` | A session (default: the newest) as a static web page: sonde's viewer and the computed `session.json`; `--serve` on 127.0.0.1 until Ctrl-C. |
| `sonde probe -e A -e B -b BUCKET [--only checksum.] [--temp-buckets] [--no-slow] [-f json\|markdown\|csv] [-o FILE]` | Runs the probes on each endpoint in parallel; saves the runs. |
| `sonde probe list` / `sonde probe runs` / `sonde probe diff RUN:EP RUN:EP` | The catalog, saved runs, changes between two runs. |
| `sonde mpu scan -e E -b B [-p PREFIX]` | Incomplete uploads, their bytes, age, cost; the lifecycle verdict. |
| `sonde mpu abort -e E -b B [--older-than-days N] [--confirm COUNT]` | A dry run; with `--confirm` (and read-only off) aborts. |
| `sonde mpu lifecycle -e E -b B [--days 7] [--confirm BUCKET]` | Adds an AbortIncompleteMultipartUpload rule: dry run, then with `--confirm`. |
| `sonde du -e E -b B [-p PREFIX] [--no-multipart] [--cached]` | The capacity breakdown and findings. |
| `sonde layout inspect -e E -b B [-p PREFIX] [--kind slatedb] [--at RFC3339]` / `--dir PATH` | Detects and decodes layouts; `--at` replays the state at a time. |
| `sonde endpoints list` / `test [ID]` / `import [PROFILES…]` | Endpoints. |
| `sonde tui [--demo] [--fps] [--no-mouse] [--bundle FILE]` | The terminal UI. |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Done; nothing to report |
| 1 | A probe failed (a guarantee broke), or a layout has errors |
| 2 | Probes behaved differently from AWS (and none failed); a replay answered differently |
| 3 | Probes could not run (errors only) |
| 4 | Findings worth attention: stale uploads or no abort rule, layout warnings, a changed probe diff, a dry run |
| 64 | Usage or configuration error |
| 70 | The operation failed (connection, credentials, permission) |
| 77 | Refused: read-only mode, the confirmation did not match, or a replay without `--yes` and no terminal to ask |

A failed request prints its readable error, its request id and the request line from the log.
