# CLI

`sonde` is the CLI and, with no GUI libraries, the terminal UI. Global options (under their own heading in each
command's help): `--config-dir`, `--data-dir`, and, for the commands that talk to a store, `--url`, `--region`,
`--path-style`, `--ca-file` (an ad hoc endpoint `adhoc`), `--rate`, `--concurrency`, `--log-out
<file.har|file.json>` (sonde's own requests of the run, redacted; refused on `run`, `proxy` and `watch`, whose
requests are the session file). Commands that only read files (`show`, `sessions`, `compare`, `check`, `diff`,
`share`, `export`, `import`) do not list the store's options. Output formats: `--format table` (default), `json`,
`markdown`, `csv`, `github` where they apply (real Markdown tables and RFC 4180 CSV); each command's help lists the
ones it writes, and a format a command does not write is a usage error (64). The answer goes to stdout; hints,
progress and `run`'s summary (which shares the terminal with the program) go to stderr. Ctrl+C cancels the running
job (its requests stop).

**A session argument** is a path or a name, for every command that takes one (`show`, `check`, `compare`, `diff`,
`replay`, `share`, `export`). A name resolves in this order: a file of that path; a session in the data directory's
`sessions/` (`20261108-073328-etl`, with or without `.sonde`); a trial's session in `sessions/trials/`
(`20261108-073328-etl.slowdown`, as `sonde run --trial` and `sonde check` name it — no `trials/` in front). A name
that is no session is a usage error that says where it was looked for. **The newest session** (a bare `show`,
`check`, `share`, `export`) is the newest one recorded by `run`, `proxy` or `watch`: not a replay, an import or a
trial, which are read by name.

**Sessions of any size.** A session is read as a stream and analysed once; `show`, `check`, `share` and `export`
hold what the analysis reads of each request and write a request in full from the file as its turn comes. A million
requests are read and analysed in under ten seconds and 2 GB (a budget a test holds: [performance.md](performance.md)). Beside each finished
session in `sessions/` is what was counted of it (`<name>.summary.json`: its requests, time, errors, program, and its
figures once a check reckoned them): `sonde sessions`, the app's list and a check's look at earlier runs read that
file and not the session. It is made when the session is recorded, again when it is missing or the session changed,
and removed with the session.

**A program's name.** Runs are grouped, and a baseline kept, by program. A program is its name when it has one —
`name = "nightly-etl"` under `[program]` in `sonde.toml`, or `sonde run --name nightly-etl` — and else its whole
command line, so that without a name a changed argument or interpreter path is another program. `sonde check` says
which program it judged and whether that is its name or its command.

| Command | Does |
|---|---|
| `sonde demo [--app]` | A tour: a small program's requests to the built-in store, recorded and explained (`--app` opens it in the app). |
| `sonde run [--no-delete\|--read-only] [--live] [--callers] [-o FILE] -- COMMAND…` | Runs a command against a local endpoint that re-signs and records its S3 requests; prints the session's summary (and, when the program sent nothing, how each common SDK is pointed at sonde); exits with the command's status (128 + the signal's number when a signal ended it, as a shell says it). `--callers` names the code that sent each request in a Python, Node.js or Java program (see below). |
| `sonde run --trial [--fault SCENARIO …] [--name NAME] -- COMMAND…` | The clean run, then the command again under each fault scenario (chosen from what it did, or `sonde.toml`'s `[[scenario]]`, or the ones named), and whether it survived each: see [trials.md](trials.md). `{trial}` in the command (and `SONDE_TRIAL`) is `clean`, then the scenario's name; only when a trial runs. A clean run that fails or is interrupted is not tried. Ctrl-C or SIGTERM ends the whole trial (130, 143) and writes no verdict for the scenario it interrupted. Exits with the command's status when that is not 0, else 1 when a trial broke. Every `sonde run` to a session path first removes that session's earlier verdicts and trial sessions. `--fault` takes a scenario's name — `slowdown`, `error`, `skew`, `lost-answer`, `fail-complete`, `complete-500`, `cut-body`, `slow-first-byte`, `lost-race`, `stale-list`, `slow`, `s3-limits` — or one of `sonde.toml`'s; `--fault slowdown:3` (or `error:3`) fails each request's first three attempts, 1 to 5. |
| `sonde proxy [--listen ADDR] [--allow-remote] [--no-delete\|--read-only] [-o FILE]` | The same endpoint, standing: prints the variables to set, then every request. It listens on loopback only unless `--allow-remote`. |
| `sonde watch [--listen ADDR] [--rotate MIN] [--alert URL] …` | A proxy left running: one status line for the last minute (requests a second, errors, p95), each new warning as it appears, and a new session file every `--rotate` minutes (60), the finished one compressed. A finished file over `sonde.toml`'s budget is said on stderr (the last one at Ctrl-C too); `--alert` posts each new warning, and each such file, to a webhook as `{"text": …}` (Slack, Teams, Mattermost). |
| `sonde watch --logs s3://BUCKET/PREFIX [--every 15m] [--once] [-e ENDPOINT] [--alert URL]` | Production through its logs, no proxy in the path: the S3 server access logs or CloudTrail files written under the prefix are read as they arrive (GET and LIST only), each pass kept as a session of the program `logs of s3://…` (so it has a history, a baseline and `sonde check`), its new warnings said and posted, the pass held to `sonde.toml`'s budget. Where it got to is kept; `--once` reads what is new and exits (cron, CI): 1 when the pass is over the budget, 70 when the logs could not be listed or none of the files listed could be read. |
| `sonde sessions [list] [-f json\|csv\|markdown]` / `sonde sessions prune --keep N [--yes]` | The recorded sessions, newest first, each with its program (its name when it has one, else its command: in a terminal a long command is cut in its middle to fit the width, whole in a file, a pipe and JSON), requests, how long it took, its errors and how its trials stood (the worst of them); prune deletes all but the newest N (baselines kept), a dry run without `--yes`, and says so when there was nothing to remove. |
| `sonde show [FILE] [--requests] [--errors] [--summary] [--all] [--app] [-f json]` | A session (default: the newest) as operations (or requests) and observations, then its summary, each fact once: a head line (requests, operations, how long, errors, lookups that found nothing); a table per operation (requests, errors and their share, not found, p50, p95, p99, max — a percentile of fewer than 20 requests is marked `*`); the four signals (rate with the most in flight at once, errors, latency, bytes) and the cost; where the waiting went by kind of request; the findings. Past 2,000 lines it prints the first and the last hundred and says how many are between (`--all` prints them, `-f json` writes them); the summary is always there. `--errors` keeps to what failed, in JSON too (`episodes` and `records`; the summary stays the session's), and on a session without one says there are none. "Acting on N findings" is the session with the requests those findings name taken out and reckoned again (their own waiting, their own price; a finding that names no request is left out, and the line says which it counts when not all). JSON is written as it is made (a session of any size) and carries each finding's fixes by SDK and, in `summary.operations[]`, each operation's `errorShare`, `p50Ms`, `p95Ms`, `p99Ms`, `maxMs` (and `few: true` under 20 requests); `--summary -f json` is the summary alone (`summary`, `waiting` with `byKind`, `p95Ms`, `peakInFlight`, `cost`, `whatIf`, `observations`), without the requests. |
| `sonde replay FILE -e E [-e E2 …] [--bucket A=B] [--prefix P\|--in-place] [--keep] [--yes] [-o FILE]` | Re-enacts a session on another endpoint, cleans up, compares every answer. The cleanup removes what the replay wrote under its prefix: the objects and, on a bucket that keeps versions, every version and delete marker; when anything is still there afterwards it says how much and exits 70. With several `-e`, replays on each at once and prints them side by side: answers that differ and each kind's p95 against the recording (`-f markdown\|csv` too); a store it could not be replayed on exits 70. |
| `sonde import FILE\|FOLDER [-o FILE]` | Production traffic as a session (a folder is every file under it, in the order of their names, as one session; S3 delivers logs as thousands of small files): an S3 server access log, CloudTrail data events (`{"Records": …}` or one a line) or a HAR (gzipped or not), with the same operations, findings and figures as a recording; what is left out is counted on stderr (lines that are not requests or do not read, a HAR's entries for other hosts). The session is named by the store's host where the source says it. A log's times are whole seconds (an access log's, CloudTrail's): a second's requests are spread evenly over it in the log's order, the session says so (`wholeSeconds` in its header, a line in `show`), and what depends on where in its second a request began is not said of it — how many were in flight at once, requests "one after another", and the in-flight figure of a throttling finding. A HAR's `timings.wait` is the request's first byte. |
| `sonde export [FILE] [-o FILE \| --to URL]` | A session as OpenTelemetry traces (OTLP/JSON): each operation a span, its requests under it, written as it is made; `service.name` is the program's name when it has one, else its command's. `--to` posts it to a collector (`http://localhost:4318/v1/traces`) in requests of about 3 MiB (collectors refuse more than 4), an operation's spans kept together, and says in how many parts. |
| `sonde diff ORIGINAL REPLAY [-f json]` | Compares a session with its replay (two ordinary runs are a usage error, 64: that is `sonde compare`), and each kind's p50 and p95 on both stores (`replay` prints the same table). |
| `sonde check [FILE] [--config sonde.toml] [--baseline FILE\|auto\|URL] [--set-baseline] [--max-requests N] [--max-waiting MS] [--max-cost X] [--max-errors N] [--max-p95 MS] [--tolerance PCT] [--no-new-warnings] [--init] [-f json\|markdown\|github]` | A run against a budget, a baseline run and its trials: exits 1 when it is over either, or when a trial broke — the trials alone are a check, no budget needed. It closes by saying which, in every format: `within budget` or `over budget` for a budget's rows, `within its baseline` or `past its baseline` for a baseline's (both when both: `over budget and past its baseline`), then `, and broke under a trial`; with trials alone `broke under a trial` or `did not break under its trials`. `-f json` carries `trials`, `passed` (the whole check: every row, and no trial that broke), `verdict`, and `check` with its rows and their own `passed` (true when there are none: a check of trials alone has nothing failed inside it). A trial's row is `pass` (survived), `warn` (degraded), `FAIL` (broke) or `skip` (untested). It names the session and the program it judged first; a failed row says by how much (`+23%`): over its limit for a budget's row, over the baseline's own figure for a baseline's row. `--max-waiting` and `--max-p95` take a time as `sonde.toml` writes it (`2s`, `250ms`, or ms). The budget may live with the code in `sonde.toml` (here or in a folder above, to the repository's root); flags win over it. `--baseline auto` is the baseline set for the run's program (in the app or with `--set-baseline`; `sessions/baselines.json`, shared with the app). With none set yet (a first run in CI) the budget is still checked and decides the exit; "no baseline yet" is said on stderr with the program it is of, in the closing sentence (`within budget (no baseline yet)`) and as `noBaseline` in JSON — and when a baseline is set for the same program run with other arguments it names that one and how to give the program a name they share. With no budget either, nothing is checked and it exits 0. an `https://` baseline (a CI artifact, a presigned URL) is fetched; a baseline of another program is named on stderr; a `baselines.json` that is there and does not read is a configuration error (64) for `--baseline auto`, `--set-baseline` and `sonde sessions prune`, never read as empty or written over. `-f markdown` is a pull request's comment, its tables' columns lined up as text too; `-f github` writes GitHub Actions annotations (failed verdicts as errors, findings on the line of code that sent their requests). Past a baseline, it says since which run and the commits around it, once (in `-f github` on the first row the baseline failed); the earlier runs' figures come from the small files kept beside them. `--init` writes a `sonde.toml` this run passes: the counts as limits, the limits of time commented out with the measured value beside them (time varies from one identical run to the next), and, when the program has no name, a `[program] name` made from its command (`etl` for `python etl.py --day 2`) so that its runs and its baseline go by it whatever its arguments; it says what it wrote. It writes nothing about trials: `sonde run --trial` tries the program, a `[[scenario]]` is the project's own to add. Nothing to check (no budget, no baseline, no trials) is a usage error (64), as are `--tolerance` and `--no-new-warnings` without a baseline, and a `sonde.toml` that does not read or has a key sonde does not know. |
| `sonde compare BEFORE AFTER [-f json]` | Two runs of a program, before and after a change: requests, waiting, time, errors and request cost, by kind of request, and the findings gone and new. Two different programs compare by their figures only (it says so). |
| `sonde share [FILE] [-o DIR] [--serve [--port N]] [--json] [--html] [--redact]` | A session (default: the newest) as a static web page: sonde's viewer and the computed `session.json`; `--serve` on 127.0.0.1 until Ctrl-C. `--json` is written as it is made. `--html` writes one self-contained HTML file instead (findings, where the time went, a timeline, operations at `#op-N`): of a long session the first 2,000 operations and every one a finding links to, and it says how many it left out and how to see them. A page is titled by the program's name when it has one, its command under it. `--redact` replaces bucket and key names with tokens (the same name, the same token; layout names, numbers and file types kept) and drops headers, bodies, URLs, request ids, the program's output and its arguments. |
| `sonde probe -e A -e B -b BUCKET [--only checksum.] [--temp-buckets] [--no-slow] [-f json\|markdown\|csv] [-o FILE]` | Runs the probes on each endpoint in parallel; saves the runs. A run writes and deletes under `sonde-probe/` in the scratch bucket named on the command line (that is its consent: read-only mode holds the app's window, not this command), and is in the audit log. |
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
| 1 | A probe failed (a guarantee broke), or a layout has errors; `check` (or a `watch --logs --once` pass) is over its budget or baseline, or the run broke under a trial; `run --trial` when a trial broke |
| 2 | Probes behaved differently from AWS (and none failed); a replay answered differently |
| 3 | Probes could not run (errors only) |
| 4 | Findings worth attention: stale uploads or no abort rule, layout warnings, a changed probe diff, a dry run |
| 64 | Usage or configuration error (an unknown command or option too, and a `-f` format the command does not write; `--help` and `--version` exit 0) |
| 70 | The operation failed (connection, credentials, permission); a replay could not remove what it wrote |
| 130, 143 | `run --trial` interrupted by Ctrl-C or SIGTERM (`run` otherwise exits as its command did) |
| 77 | Refused: read-only mode (`read-only = false` under `[settings]` in `config.toml` turns it off), the confirmation did not match, or a replay without `--yes` and no terminal to ask |

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
between jobs (a cache), or point `--baseline` at a session kept as an artifact of the main branch (a path, or its
URL).

The budget can be kept with the code, in `sonde.toml`:

```toml
[program]
name = "nightly-etl"      # what its runs and its baseline go by, whatever the arguments

[check]
max-requests = 5000
max-waiting = "2s"        # or ms as a number
max-cost = 0.05
max-errors = 0
max-p95 = "250ms"         # 95% of the requests within it
tolerance = 10            # % a run may grow over its baseline
no-new-warnings = true
baseline = "auto"         # or a session's path, or an https URL
# findings kept on purpose: never a new warning (a finding's identity, as the
# app's "Expected" copies it, its kind, or words of its text)
expect = ["obs.hot|slate/db/gc/manifest.boundary", "obs.listed"]
```

`sonde check -f markdown` writes the verdicts, what is new since the baseline and the run's findings (with the
code that sent their requests) as Markdown, for a pull request's comment:

```sh
sonde check -f markdown > comment.md || status=$?
gh pr comment "$PR" --body-file comment.md
exit ${status:-0}
```

Or as annotations on the pull request's diff, each finding on the line of code that sent its requests:

```yaml
      - run: sonde run -- python etl.py
      - run: sonde check -f github
```

`sonde.toml` has three tables, `[program]`, `[check]` and `[[scenario]]` ([trials.md](trials.md)); any other key
is an error. `sonde check --init` writes a first `sonde.toml` from a run (its counts with 10% of room, its times
commented out for a person to set, a `[program] name` when the program has none) and prints these steps. With
`baseline = "auto"` the first run in CI passes or fails by the budget alone and says there is no baseline yet.
Past its baseline, `sonde check` says since which run, and the commits of that run and the last one within: each
run records the commit its code was at (`git`, or `GITHUB_SHA` in CI), so the change between the two is what to look
at.

## Production, from its logs

`sonde watch --logs s3://my-logs/s3/ --every 15m` reads the S3 server access logs (or CloudTrail files) as they are
written, through an endpoint's credentials, read only. Each pass is a session of the program `logs of s3://…`, read
like any run: findings, time, cost, a history in the app and `sonde check` against a baseline. `--alert URL` posts each
new warning and each pass over `sonde.toml`'s budget; `--once` reads what is new and exits, for cron.

## Which code sent a request

A finding names the place in the program its requests came from when the requests say so: sonde reads a
`x-sonde-caller: file:line function` header (and never forwards it to the store), the SDK and its version from the
user agent, and the trace from `traceparent`. `sonde run --callers` (or the "Name the code" setting) adds the
header itself, before each request to sonde is signed, naming the first frame of the program's own code:

| Language | SDK | How |
|---|---|---|
| Python | boto3 / botocore | a `sitecustomize` first on `PYTHONPATH` (a program's own still runs) |
| Node.js | AWS SDK for JavaScript v3 | `NODE_OPTIONS=--require …/sonde-caller.cjs`: a middleware on each client |
| Java | AWS SDK for Java 2 | `JAVA_TOOL_OPTIONS=-javaagent:…/sonde-agent.jar`: a global `ExecutionInterceptor` |

A Java program whose SDK comes from a class loader of its own (a Spring Boot jar) is left as it is: the agent adds
the interceptor only when the SDK is on the class path. The Java hook's sources are in `hooks/java` (`build.sh`
rebuilds its two jars). Other SDKs add the header with their own request hook.
