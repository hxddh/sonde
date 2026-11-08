# CLI

`sonde` is the CLI and, with no GUI libraries, the terminal UI. Global options: `--config-dir`, `--data-dir`,
`--url`, `--region`, `--path-style`, `--ca-file` (an ad hoc endpoint `adhoc`), `--rate`, `--concurrency`,
`--log-out <file.har|file.json>` (the run's request log, redacted). Output formats: `--format table` (default),
`json`, `markdown`, `csv` where they apply (real Markdown tables and RFC 4180 CSV). The answer goes to stdout; hints,
progress and `run`'s summary (which shares the terminal with the program) go to stderr. Ctrl+C cancels the running
job (its requests stop). A session argument is a path or a name in the data directory's `sessions/`
(`20261108-073328-etl`, with or without `.sonde`).

| Command | Does |
|---|---|
| `sonde demo [--app]` | A tour: a small program's requests to the built-in store, recorded and explained (`--app` opens it in the app). |
| `sonde run [--no-delete\|--read-only] [--live] [-o FILE] -- COMMAND…` | Runs a command against a local endpoint that re-signs and records its S3 requests; prints the session's summary (and, when the program sent nothing, how each common SDK is pointed at sonde); exits with the command's status. |
| `sonde proxy [--listen ADDR] [--allow-remote] [--no-delete\|--read-only] [-o FILE]` | The same endpoint, standing: prints the variables to set, then every request. It listens on loopback only unless `--allow-remote`. |
| `sonde watch [--listen ADDR] [--rotate MIN] …` | A proxy left running: one status line for the last minute (requests a second, errors, p95), each new warning as it appears, and a new session file every `--rotate` minutes (60), the finished one compressed. |
| `sonde sessions [list] [-f json\|csv\|markdown]` / `sonde sessions prune --keep N [--yes]` | The recorded sessions, newest first; prune deletes all but the newest N (baselines kept), a dry run without `--yes`. |
| `sonde show [FILE] [--requests] [--errors] [--summary] [--app] [-f json]` | A session (default: the newest) as operations (or requests) and observations, and where its time went: the time it waited on its store by kind of request, the peak in flight and the requests' cost; and what acting on every finding would leave (requests, waiting, cost). JSON carries each finding's fixes by SDK. |
| `sonde replay FILE -e E [-e E2 …] [--bucket A=B] [--prefix P\|--in-place] [--keep] [--yes] [-o FILE]` | Re-enacts a session on another endpoint, cleans up, compares every answer. With several `-e`, replays on each at once and prints them side by side: answers that differ and each kind's p95 against the recording. |
| `sonde import FILE [-o FILE]` | Production traffic as a session: an S3 server access log, CloudTrail data events (`{"Records": …}` or one a line) or a HAR, with the same operations, findings and figures as a recording. |
| `sonde export [FILE] --otlp [-o FILE \| --to URL]` | A session as OpenTelemetry traces (OTLP/JSON): each operation a span, its requests under it; `--to` posts it to a collector (`http://localhost:4318/v1/traces`). |
| `sonde diff ORIGINAL REPLAY [-f json]` | Compares a session with its replay, and each kind's p50 and p95 on both stores (`replay` prints the same table). |
| `sonde check [FILE] [--baseline FILE\|auto] [--set-baseline] [--max-requests N] [--max-waiting MS] [--max-cost X] [--max-errors N] [--tolerance PCT] [--no-new-warnings] [-f json]` | A run against a budget and a baseline run: exits 1 when it is over either (for CI). `--baseline auto` is the baseline set for the run's program (in the app or with `--set-baseline`; `sessions/baselines.json`, shared with the app); a baseline of another program is named on stderr. Nothing to check is a usage error (64). |
| `sonde compare BEFORE AFTER [-f json]` | Two runs of a program, before and after a change: requests, waiting, time, errors and request cost, by kind of request, and the findings gone and new. |
| `sonde share [FILE] [-o DIR] [--serve [--port N]] [--json] [--html] [--redact]` | A session (default: the newest) as a static web page: sonde's viewer and the computed `session.json`; `--serve` on 127.0.0.1 until Ctrl-C. `--html` writes one self-contained HTML file instead (findings, where the time went, a timeline, every operation at `#op-N`). `--redact` replaces bucket and key names with tokens (the same name, the same token; layout names, numbers and file types kept) and drops headers, bodies, URLs, request ids, the program's output and its arguments. |
| `sonde probe -e A -e B -b BUCKET [--only checksum.] [--temp-buckets] [--no-slow] [-f json\|markdown\|csv] [-o FILE]` | Runs the probes on each endpoint in parallel; saves the runs. |
| `sonde probe list` / `sonde probe runs` / `sonde probe diff RUN:EP RUN:EP` | The catalog, saved runs, changes between two runs. |
| `sonde mpu scan -e E -b B [-p PREFIX]` | Incomplete uploads, their bytes, age, cost; the lifecycle verdict. |
| `sonde mpu abort -e E -b B [--older-than-days N] [--confirm COUNT]` | A dry run; with `--confirm` (and read-only off) aborts. |
| `sonde mpu lifecycle -e E -b B [--days 7] [--confirm BUCKET]` | Adds an AbortIncompleteMultipartUpload rule: dry run, then with `--confirm`. |
| `sonde du -e E -b B [-p PREFIX] [--no-multipart] [--cached]` | The capacity breakdown and findings. |
| `sonde layout inspect -e E -b B [-p PREFIX] [--kind slatedb] [--at RFC3339]` / `--dir PATH` | Detects and decodes layouts; `--at` replays the state at a time. |
| `sonde endpoints list` / `test [ID]` / `import [PROFILES…]` / `add ID URL [--profile P]` | Endpoints; `add` takes the global `--region`, `--path-style`, `--ca-file`, and credentials from a profile or the default chain (never a key on the command line). |
| `sonde tui [--demo] [--fps] [--no-mouse] [--bundle FILE]` | The terminal UI. |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Done; nothing to report |
| 1 | A probe failed (a guarantee broke), or a layout has errors |
| 2 | Probes behaved differently from AWS (and none failed); a replay answered differently |
| 3 | Probes could not run (errors only) |
| 4 | Findings worth attention: stale uploads or no abort rule, layout warnings, a changed probe diff, a dry run |
| 64 | Usage or configuration error (an unknown command or option too; `--help` and `--version` exit 0) |
| 70 | The operation failed (connection, credentials, permission) |
| 77 | Refused: read-only mode, the confirmation did not match, or a replay without `--yes` and no terminal to ask |

A failed request prints its readable error, its request id and the request line from the log.

## In CI

```sh
# record the job (into the data directory's sessions/), then hold it to its program's baseline
sonde run -- python etl.py
sonde check --baseline auto --no-new-warnings                    # exit 1 when it grew or warns anew
sonde check --max-requests 5000 --max-cost 0.05 -f json > check.json
sonde share --html --redact -o run.html                          # an artifact anyone can open
```

A run becomes its program's baseline with `sonde check --set-baseline` (or in the app); keep the data directory
between jobs (a cache), or point `--baseline` at a session kept as an artifact of the main branch.
