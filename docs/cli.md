# CLI

`sonde` is the CLI and, built with the `tui` feature, the terminal UI. `sonde help <command>` lists every flag.

## Common

| | |
|---|---|
| Global options | `--config-dir` (`$SONDE_CONFIG_DIR`), `--data-dir` (`$SONDE_DATA_DIR`) |
| Store options | `--url` (an ad hoc endpoint, `adhoc`), `--region` (us-east-1), `--path-style`, `--ca-file`, `--rate` (requests/s per job), `--concurrency` (in flight per job), `--log-out FILE.har\|.json` (sonde's own requests, redacted; refused on `run`, `proxy`, `watch`). Not on commands that only read files. |
| `-f`, `--format` | `table` (default), `json`, `markdown`, `csv`, `github` where a command writes them (help lists which). Another is a usage error (64). |
| Streams | The answer on stdout; hints, progress and `run`'s summary on stderr. Ctrl+C cancels the running job. |
| A session argument | A path or a name, resolved in order: a file; `sessions/<name>[.sonde]`; `sessions/trials/<name>` (e.g. `20261108-073328-etl.slowdown`). Not found: usage error naming where it looked. |
| The newest session | The default for `show`, `check`, `share`, `export`: newest from `run`, `proxy` or `watch` (never a replay, import or trial). |
| A program | `[program] name` in `sonde.toml`, or `--name`; else the whole command line. Runs are grouped and baselines kept by it. |
| Size | Sessions are streamed and analysed once. A million requests: under 15 s and 1.5 GB; `compare`/`diff` read one session at a time, so two cost what one does ([performance](performance.md)). |
| `<name>.summary.json` | Beside each session: requests, time, errors, program, figures once checked. Read by `sessions`, the app and `check`'s history. Rebuilt when missing, when the session's length or mtime (ns) changed, or the analysis changed; removed with the session. A session with a bad middle line is listed with `!` (`unreadable` in JSON). |

## Record

| Command | Does |
|---|---|
| `sonde demo [--app]` | A tour on the built-in store; `--app` opens the session in the app. |
| `sonde run [-e E] [--policy P] [--tls] [--live] [--callers] [--name N] [-o FILE] -- CMD…` | Runs `CMD` against a local endpoint that re-signs and records its S3 requests ([recording](recording.md)). Prints the summary (and how to point each SDK at sonde when nothing was sent). Exits with the command's status (128 + signal). |
| ↳ `--policy no-delete\|read-only\|allow` | Default `[record] policy` in `sonde.toml`, else `no-delete` ([what it refuses](recording.md#what-a-recorded-program-may-do)). Short forms `--no-delete`, `--read-only`, `--allow-writes`. |
| ↳ `--tls` | Also serve TLS, with a certificate the program is told to trust (`[record] tls = true`). |
| ↳ `--live` | Each request on stderr as it happens. |
| ↳ `--callers` | Name the code line that sent each request ([below](#which-code-sent-a-request)). |
| ↳ `spark-submit`, `flink run` | Given s3a's endpoint on their command line. |
| `sonde run --trial [--fault S …] -- CMD…` | The clean run, then one per fault scenario, each with a verdict ([trials](trials.md)). `{trial}` in the command and `SONDE_TRIAL` are `clean`, then the scenario's name. A failed or interrupted clean run is not tried. Ctrl-C / SIGTERM ends the trial (130 / 143), no verdict for the interrupted scenario. Exit: the command's status if not 0, else 1 if a trial broke. A run to a session path first removes that session's earlier verdicts and trials. |
| ↳ `--fault S` | Repeatable: `slowdown`, `error`, `skew`, `lost-answer`, `fail-complete`, `complete-500`, `cut-body`, `slow-first-byte`, `lost-race`, `stale-list`, `slow`, `s3-limits`, or a `sonde.toml` scenario. `slowdown:N` / `error:N` (1–5) fail the first N attempts. |
| `sonde proxy [--listen ADDR] [--allow-remote] [--policy P] [--tls] [-o FILE]` | The same endpoint, standing (default `127.0.0.1:9000`): prints the variables to set, then each request; its summary at Ctrl-C. Loopback only unless `--allow-remote`. |
| `sonde watch [--listen ADDR] [--rotate MIN] [--alert URL] …` | A proxy left running: a status line for the last minute (rate, errors, p95), each new warning, a new session file every `--rotate` minutes (60; the finished one gzipped). A file over `sonde.toml`'s budget is said on stderr. `--alert` posts warnings and over-budget files to a webhook as `{"text": …}` (Slack, Teams, Mattermost). |
| `sonde watch --logs s3://B/PREFIX [-e E] [--every 15m] [--once] [--alert URL]` | Production from its S3 access logs or CloudTrail files, read as they arrive (GET and LIST only, no proxy). Each pass is a session of program `logs of s3://…` (history, baseline, `check`), held to the budget; position is kept. `--once` reads what is new and exits: 1 over budget, 70 when the logs cannot be listed or none read. |
| `sonde import FILE\|FOLDER [-o FILE]` | An S3 access log, CloudTrail (`{"Records": …}` or one event a line) or HAR, gzipped or not, as a session. A folder is every file under it, in name order, as one session. Skipped lines (not requests, unreadable, a HAR's other hosts) are counted on stderr. Named by the store's host. Whole-second log times are spread evenly over their second (`wholeSeconds` in the header, a line in `show`), so in-flight counts, "one after another" and a throttling finding's in-flight figure are not given. A HAR's `timings.wait` is the first byte. |

## Read a session

| Command | Does |
|---|---|
| `sonde sessions [list] [-f json\|csv\|markdown]` | Sessions, newest first: program (name, else command; cut in the middle to the terminal's width), requests, duration, errors, worst trial verdict. |
| `sonde sessions prune --keep N [--yes]` | Delete all but the newest N (baselines kept); a dry run without `--yes`. |
| `sonde show [FILE] [--requests] [--errors] [--summary] [--all] [--by DIM] [--app] [-f json]` | Operations (or `--requests`) and findings, then the summary: head line (requests, operations, the SDK from the user agent, duration, errors, not found); per-operation table (requests, errors and share, not found, p50, p95, p99, max; `*` under 20 requests); the four signals (rate with peak in flight, errors, latency, bytes) and cost; waiting by kind; findings. |
| ↳ long sessions | Past 2,000 lines: the first and last hundred and a count (`--all` prints all). The summary is always printed. |
| ↳ `--errors` | Only what failed (JSON too: `episodes`, `records`); the summary stays the session's. |
| ↳ `--by operation\|prefix\|size\|caller\|sdk\|status` | The operations table split instead: requests, errors and share, p50, p95, p99, waiting share, cost per group (most waiting first; past 12, `other`). Size bands: `0 B`, `< 64 KiB`, `64 KiB-1 MiB`, `1-8 MiB`, `8 MiB+` (the larger of sent and received). Then one sentence: where the slow requests (the slowest tenth, at or over p90; 3 at least) concentrate, narrowed by prefix, size or operation when one holds most of them, and where the cost does (`80% of the slow requests (over 120 ms) are under b/wal/, over 8 MiB.`). A group is named when it holds half of them and 1.5× its share of the requests. `-f json`: [show-by](schemas/show-by.schema.json). |
| ↳ "Acting on N findings" | The session recomputed without the requests those findings name (findings naming none are left out, and the line says so). |
| ↳ `-f json` | Streamed. Each finding's fixes by SDK; `summary.operations[]` has `errorShare`, `p50Ms`, `p95Ms`, `p99Ms`, `maxMs`, `few`. `--summary -f json`: `summary`, `waiting` (`byKind`), `p95Ms`, `peakInFlight`, `cost`, `whatIf`, `observations`, no requests. |
| `sonde compare BEFORE AFTER [-f json]` | Two runs: requests, waiting, time, errors and cost by kind of request; findings gone and new. Unchanged kinds are counted, not listed. Different programs compare by figures only (said). |
| `sonde check [FILE] [options] [-f json\|markdown\|github]` | A run against a budget, a baseline and its trials. Exit 1 when over budget, past the baseline, or a trial broke. [Details](#sonde-check). |
| `sonde share [[BEFORE] FILE] [-o DIR] [--serve [--port N]] [--json] [--html [--baseline FILE\|auto\|URL]] [--redact]` | A static site: the viewer and `session.json` (default `./<session>-share/`). `--serve`: on 127.0.0.1 until Ctrl-C. `--json`: the document only, streamed. `--html`: one self-contained file (findings, time, timeline, operations at `#op-N`; past 2,000 operations, the first 2,000 and every one a finding links to, with a count). Titled by the program. Two runs, or `--baseline` (as `check` takes it): the comparison first (requests, waiting, cost, errors before and after; kinds that changed; findings gone), then the run, its new findings first and marked; two runs without `--html` is a usage error. `--redact`: bucket and key names become tokens (same name, same token; layout names, numbers, extensions kept); headers, bodies, URLs, request ids, output and arguments dropped. |
| `sonde export [FILE] [-o FILE \| --to URL]` | OTLP/JSON traces (`--otlp`, the default): a span per operation, its requests under it; `service.name` is the program. `--to` posts to a collector (e.g. `http://localhost:4318/v1/traces`) in parts of about 3 MiB, an operation's spans kept together. |

### sonde check

| Option | |
|---|---|
| `--max-requests N`, `--max-waiting T`, `--max-cost X`, `--max-errors N`, `--max-p95 T` | Budget. Times as `2s`, `250ms` or ms. |
| `--baseline FILE\|auto\|https://…` | `auto`: the baseline set for the run's program (`sessions/baselines.json`, shared with the app). A URL is fetched (CI artifact, presigned). |
| `--set-baseline` | Make this run its program's baseline. |
| `--tolerance PCT` | Growth allowed over the baseline (10). |
| `--no-new-warnings` | Fail on a warning the baseline did not have. |
| `--config FILE` | Default: `sonde.toml` here or in a folder above, up to the repository root. Flags win. |
| `--report-url URL` | `-f markdown` only: the link the comment ends with (the HTML report). |
| `--init` | Write a `sonde.toml` this run passes: counts with 10% room, times commented out beside their measured values, a `[program] name` from the command when there is none (`etl` for `python etl.py --day 2`). No trials. |

- Output names the session and program first. A trial row is `pass` / `warn` / `FAIL` / `skip` (survived / degraded / broke / untested). A failed row says by how much (`+23%`) over its limit or the baseline's figure.
- Closing verdict, in every format: `within budget` / `over budget`, `within its baseline` / `past its baseline` (joined with `and`), then `, and broke under a trial`; trials alone: `broke under a trial` / `did not break under its trials`.
- No baseline set yet: the budget alone decides; said in every format: `no baseline yet` on stderr and in the verdict, `noBaseline` in JSON, a quoted line in markdown, a `::warning` with `-f github`. It names a baseline of the same program under other arguments if there is one. A baseline of another program is named on stderr.
- Past a baseline over several runs: since which run, and the commits around it (`git`, else `GITHUB_SHA`), once (`past the baseline since a1b2c3d, 2 runs ago; within at 9f8e7d6`; this run the first past: `the run before, at 9f8e7d6, was within it`).
- `-f json`: `trials`, `passed` (whole check), `verdict`, `check` (rows, each with `passed`). `-f markdown`: a pull-request comment ([what it says](ci.md#the-comment)). `-f github`: Actions annotations, findings on the line that sent their requests.
- `--baseline auto` with none set and no budget: nothing checked, exit 0. Usage error (64): nothing to check (no
  budget, baseline or trials); `--tolerance` or `--no-new-warnings` without a baseline; a `sonde.toml` that does not parse or has an unknown key; an unreadable `baselines.json` (for `auto`, `--set-baseline`, `sessions prune`; never treated as empty).

## Replay on another store

| Command | Does |
|---|---|
| `sonde replay FILE -e E [-e E2 …] [--bucket A=B] [--prefix P \| --in-place] [--keep] [--yes] [-o FILE]` | Re-enacts a session on another endpoint, compares every answer, then removes what it wrote under its prefix (objects, and on a versioned bucket every version and delete marker; exit 70 if anything remains). Several `-e`: replayed on each at once, side by side (differing answers, each kind's p95 against the recording; `-f markdown\|csv` too); exit 70 for a store it could not replay on. [Recording](recording.md#replay-on-another-endpoint). |
| `sonde diff ORIGINAL REPLAY [-f json]` | A session against its replay, with each kind's p50 and p95 on both stores. Two ordinary runs: usage error (use `compare`). |

## The store itself

| Command | Does |
|---|---|
| `sonde probe -e A -e B -b BUCKET [--only checksum.] [--temp-buckets] [--no-slow] [-f json\|markdown\|csv] [-o FILE]` | Probes each endpoint in parallel, saves the run. Writes and deletes under `sonde-probe/` in the named bucket (naming it is consent; read-only mode applies to the app, not this command); audited. |
| `sonde probe list` / `runs` / `diff RUN:EP RUN:EP` | The catalog; saved runs; changes between two. |
| `sonde mpu scan -e E -b B [-p PREFIX]` | Incomplete uploads: bytes, age, cost; the lifecycle verdict. |
| `sonde mpu abort -e E -b B [--older-than-days N] [--confirm COUNT]` | A dry run; with `--confirm` (read-only off) aborts. |
| `sonde mpu lifecycle -e E -b B [--days 7] [--confirm BUCKET]` | Adds an AbortIncompleteMultipartUpload rule: dry run, then `--confirm`. |
| `sonde du -e E -b B [-p PREFIX] [--no-multipart] [--cached]` | Capacity breakdown and findings. |
| `sonde layout inspect -e E -b B [-p PREFIX] [--kind K] [--at RFC3339]` / `--dir PATH` | Detects and decodes a layout (`slatedb`, `iceberg`, `lakefs`, `woodpecker`, `turbopuffer`, `warpstream`, `generic`); `--at` gives the state at a time. |
| `sonde endpoints list` / `test [ID]` / `import [PROFILES…]` / `add ID URL [--profile P]` | Endpoints. `import` takes `~/.aws/config` profiles not yet configured. `add` takes `--region`, `--path-style`, `--ca-file`; credentials from a profile or the default chain, never the command line. |

## More

| Command | Does |
|---|---|
| `sonde tui [--demo] [--fps] [--no-mouse] [--bundle FILE] [--log-file FILE]` | The terminal UI. `--fps`: frame-rate line (C-h p). `--bundle`: run another `bundle.js`. |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Done; nothing to report |
| 1 | A probe failed (a guarantee broke), or a layout has errors; `check` (or a `watch --logs --once` pass) is over its budget or baseline, or the run broke under a trial; `run --trial` when a trial broke |
| 2 | Probes behaved differently from AWS (and none failed); a replay answered differently |
| 3 | Probes could not run (errors only) |
| 4 | Findings worth attention: stale uploads or no abort rule, layout warnings, a changed probe diff, a dry run |
| 64 | Usage or configuration error (an unknown command or option too, and a `-f` format the command does not write; `--help` and `--version` exit 0) |
| 65 | Bad data: not a session (no header, a newer format, a bad line in its middle). A torn last line or a cut gzip stream is read up to the cut (said on stderr) |
| 70 | The operation failed (connection, credentials, permission); a replay could not remove what it wrote |
| 77 | Refused: read-only mode (`read-only = false` under `[settings]` in `config.toml` turns it off), the confirmation did not match, or a replay without `--yes` and no terminal to ask |
| 130, 143 | `run --trial` interrupted by Ctrl-C or SIGTERM (`run` otherwise exits as its command did) |

A failed request prints its error, request id and the request line from the log.

## In CI

[The GitHub Action and the comment](ci.md). By hand:

```sh
sonde run -- python etl.py                                       # into the data directory's sessions/
sonde check --baseline auto --no-new-warnings                    # exit 1 when it grew or warns anew
sonde check --max-requests 5000 --max-cost 0.05 -f json > check.json
sonde share --html --redact --baseline auto -o run.html          # the run against its baseline, a page anyone opens
```

Set a baseline with `sonde check --set-baseline` (or in the app) and keep the data directory between jobs (a cache),
or point `--baseline` at a session kept from the main branch (path or URL).

`sonde.toml` has four tables: `[program]`, `[check]`, `[[scenario]]` ([trials](trials.md#a-projects-own-scenarios))
and `[record]` (`policy`, `tls`). Any other key is an error.

```toml
[program]
name = "nightly-etl"      # runs and baseline go by it, whatever the arguments

[check]
max-requests = 5000
max-waiting = "2s"        # or ms as a number
max-cost = 0.05
max-errors = 0
max-p95 = "250ms"
tolerance = 10            # % a run may grow over its baseline
no-new-warnings = true
baseline = "auto"         # or a session's path, or an https URL
# findings kept on purpose, never a new warning: a finding's identity
# (as the app's "Expected" copies it), its kind, or words of its text
expect = ["obs.hot|slate/db/gc/manifest.boundary", "obs.listed"]
```


## Which code sent a request

sonde reads an `x-sonde-caller: file:line function` header (never forwarded), the SDK and version from the user
agent, and the trace from `traceparent`. `sonde run --callers` (or the "Name the code" setting) adds the header
before signing, naming the program's first own frame:

| Language | SDK | How |
|---|---|---|
| Python | boto3 / botocore | a `sitecustomize` first on `PYTHONPATH` (a program's own still runs) |
| Node.js | AWS SDK for JavaScript v3 | `NODE_OPTIONS=--require …/sonde-caller.cjs`: a middleware on each client |
| Java | AWS SDK for Java 2 | `JAVA_TOOL_OPTIONS=-javaagent:…/sonde-agent.jar`: a global `ExecutionInterceptor`, only when the SDK is on the class path (not a Spring Boot jar's own loader) |

The Java hook's sources are in `hooks/java` (`build.sh` rebuilds its jars). Other SDKs can add the header with their
own request hook.
