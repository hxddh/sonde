# CLI

`sonde` is the CLI and, built with the `tui` feature, the terminal UI. `sonde help <command>` lists every flag.

## Common

| | |
|---|---|
| Global options | `--config-dir` (`$SONDE_CONFIG_DIR`), `--data-dir` (`$SONDE_DATA_DIR`) |
| Store options | `--url` (an ad hoc endpoint, `adhoc`), `--region` (us-east-1), `--path-style`, `--ca-file`. Not on commands that only read files. sonde's own pace is `concurrency` and `rate-limit` in `config.toml` (`--concurrency`, `--rate` also taken, not listed). |
| `-f`, `--format` | `table` (default), `json`, `markdown`, `csv`, `github` where a command writes them (help lists which). Another is a usage error (64). |
| Streams | The answer on stdout; hints, progress and `run`'s summary on stderr. Ctrl+C cancels the running job. |
| A session argument | A path or a name, resolved in order: a file; `sessions/<name>[.sonde]`; `sessions/trials/<name>` (e.g. `20261108-073328-etl.slowdown`). Not found: usage error naming where it looked. |
| The newest session | The default for `show`, `check`, `share`: newest from `run` or `proxy` (never a replay, import or trial). |
| A program | `[program] name` in `sonde.toml`, or `--name`; else its command with what changes between runs said as what it is (`etl.py --date 2026-11-14` is `etl.py --date {date}`, as a finding's identity says keys) and a trial's place (`{trial}`, `$SONDE_TRIAL`) left out. Runs, baselines, trial verdicts and `no-delete`'s ledger go by it. |
| Size | Sessions are streamed and analysed once. A million requests: under 15 s and 1.5 GB; `compare` reads one session at a time, so two cost what one does ([performance](performance.md)). |
| `.index.jsonl` | One per sessions folder: each session's counts (requests, time, errors, program, figures once checked), its trials' verdicts and its exit code. Read by `sessions`, the app and `check`'s history. Counted again when missing, when the session's length or mtime (ns) changed, or the analysis changed; forgotten with the session. A session with a bad middle line is listed with `!` (`unreadable` in JSON). |

## Record

| Command | Does |
|---|---|
| `sonde demo [--app]` | A tour on the built-in store; `--app` opens the session in the app. |
| `sonde run [-e E \| --demo] [--policy P] [--tls] [--live] [--no-callers] [--name N] [-o FILE] -- CMD…` | Runs `CMD` against a local endpoint that re-signs and records its S3 requests ([recording](recording.md)). Prints the summary as `show` does, ending with the change since the program's run before ([fix loop](#the-fix-loop)); how to point each SDK at sonde when nothing was sent. Exits with the command's status (128 + signal). |
| ↳ `--demo` | The built-in store `sonde demo` uses (buckets `demo`, `demo-large`, `demo-layouts`), in sonde's process, fresh each run: a first run with no account, or a CI check of requests and findings with no secrets. Its times are not meaningful (said on stderr). |
| ↳ `--policy no-delete\|read-only\|allow` | Default `[record] policy` in `sonde.toml`, else `no-delete` ([what it refuses](recording.md#what-a-recorded-program-may-do)). |
| ↳ `--tls` | Also serve TLS, with a certificate the program is told to trust (`[record] tls = true`). |
| ↳ `--live` | Each request on stderr as it happens. |
| ↳ `--no-callers` | Do not name the code line that sent each request (named by default where the language allows, [below](#which-code-sent-a-request)). |
| ↳ `spark-submit`, `flink run` | Given s3a's endpoint on their command line. |
| `sonde run --trial [--fault S …] -- CMD…` | The clean run, then one per fault scenario, each with a verdict ([trials](trials.md)). `{trial}` in the command and `SONDE_TRIAL` are `clean`, then the scenario's name. A failed or interrupted clean run is not tried. Ctrl-C / SIGTERM ends the trial (130 / 143), no verdict for the interrupted scenario. Exit: the command's status if not 0, else 1 if a trial broke. A run to a session path first removes that session's earlier verdicts and trials; the program's history of them is kept. |
| ↳ `--again` | With `--trial`: a fresh clean run, then only the scenarios whose last verdict for this program was broke, degraded or refused (`verdicts.json` beside its runs). Nothing left: says so, exit 0. Never tried: 64. |
| ↳ `--fault S` | Repeatable: `slowdown`, `error`, `skew`, `lost-answer`, `fail-complete`, `complete-500`, `cut-body`, `slow-first-byte`, `lost-race`, `stale-list`, `slow`, `s3-limits`, or a `sonde.toml` scenario. `slowdown:N` / `error:N` (1–5) fail the first N attempts. |
| `sonde proxy [--listen ADDR] [--allow-remote] [--policy P] [--tls] [-o FILE]` | The same endpoint, standing (default `127.0.0.1:9000`): prints the variables to set, then each request; its summary at Ctrl-C. Loopback only unless `--allow-remote`. |
| ↳ `--rotate MIN`, `--alert URL` | Left running: a status line for the last minute (rate, errors, p95) instead of each request, each new warning finding, a new session file every `--rotate` minutes (the finished one gzipped). A file over `sonde.toml`'s budget is said on stderr. `--alert` posts warnings and over-budget files to a webhook as `{"text": …}` (Slack, Teams, Mattermost). |
| `sonde import s3://B/PREFIX [-e E] [--follow [--every 15m]] [--alert URL]` | Production from its S3 access logs or CloudTrail files (GET and LIST only, no proxy): what arrived since the last import of that prefix, a session of program `logs of s3://…` (history, baseline, `check`) held to the budget; `--follow` keeps reading, a session a pass, until Ctrl-C. Position is kept. One pass exits 1 over budget, 70 when the logs cannot be listed, none could be read, the prefix holds no files or none is a log; nothing new since the last pass is said, exit 0. |
| `sonde import FILE\|FOLDER [-o FILE]` | An S3 access log, CloudTrail (`{"Records": …}` or one event a line) or HAR, gzipped or not, as a session. A folder is every file under it, in name order, as one session. Skipped lines and events (not requests, unreadable, a HAR's other hosts, another service's CloudTrail events) are counted on stderr. Named by the store's host. Whole-second log times are spread evenly over their second (`wholeSeconds` in the header, a line in `show`), so in-flight counts, the waiting, "one after another" and a throttling finding's in-flight figure are not given. Times not recorded (CloudTrail) are "—". Upload parts with no create in the file are not "never completed". A HAR's `timings.wait` is the first byte. |

## Read a run

| Command | Does |
|---|---|
| `sonde sessions [list] [-f json\|csv\|markdown]` | Sessions, newest first: program (name, else command; cut in the middle to the terminal's width), requests, duration, errors, worst trial verdict. |
| `sonde sessions prune --keep N [--yes]` | Delete all but the newest N (baselines kept); a dry run without `--yes` (exit 4 when it would delete any). |
| `sonde show [FILE] [--requests] [--errors] [--summary] [--all] [--by DIM] [--app] [-f json]` | The summary, one shape for `run`, `show` and `demo`: head line (requests, operations, SDK, endpoint, duration, waiting, cost, errors, not found); the findings, each with its saving, code lines (`at file:line`) and the fix for the detected SDK; "Acting on N findings"; one table by kind of request (requests, errors, p95, waiting and its share, cost share; `*` under 20 requests when others have more); the change since the program's run before. |
| ↳ `--all` | Each operation on its line first; the table with p50 and max and every kind; the four signals (rate, errors, latency, bytes), first byte, cost and the prefixes past half of S3's request limit. |
| ↳ `--requests` | Each request on its line, then the summary. Past 2,000 lines: the first and last hundred and a count (`--all` prints all). |
| ↳ `--errors` | Only the operations (or `--requests`) that failed, no summary (JSON: `operations`, `requests`; the summary stays the session's). |
| ↳ `--by kind\|prefix\|size\|caller\|sdk\|status` | The requests split instead of the table by kind: requests, errors and share, p50, p95, p99, waiting share, cost per group (most waiting first; past 12, `other`). Size bands: `0 B`, `< 64 KiB`, `64 KiB-1 MiB`, `1-8 MiB`, `8 MiB+` (the larger of sent and received). Then one sentence: where the slow requests (the slowest tenth, at or over p90; 3 at least) concentrate, narrowed by prefix, size or kind when one holds most of them, and where the cost does (`80% of the slow requests (over 120 ms) are under b/wal/, over 8 MiB.`). A group is named when it holds half of them and 1.5× its share of the requests. `-f json`: [show-by](schemas/show-by.schema.json). |
| ↳ "Acting on N findings" | The session recomputed without the requests those findings name (findings naming none are left out, and the line says so). |
| ↳ `-f json` | Streamed: `operations`, `findings`, `requests`, `session`, `summary`, `whatIf`, `trials`. `requests[].kind` is the S3 call (`GetObject`); `operations` are the program's. Each finding's fixes by SDK; `summary.kinds[]` (each `kind`) has `errorShare`, `p50Ms`, `p95Ms`, `p99Ms`, `maxMs`, `few`. `--summary -f json`: `summary`, `waiting` (`waitingMs`, `byKind`, `p95Ms`, `peakInFlight`), `cost`, `whatIf`, `findings`, `trials`, no requests. A trial's run says first that its faults were sonde's. |
| `sonde compare [RUN [RUN]] [-f json]` | Two runs: requests, waiting, time, errors and cost by kind of request; each finding fixed, less, back or new ([fix loop](#the-fix-loop)). One run: it and its program's run before; none: the newest run and the one before. Unchanged kinds are counted, not listed. Different programs compare by figures only (said). A session and its replay (either order): every answer compared, with each kind's p50 and p95 on both stores. |
| `sonde compare FILE --replay E [--replay E2 …] [--bucket A=B] [--prefix P] [--yes] [-o FILE]` | Re-enacts a session on another endpoint under a prefix of its own (never on the recorded keys), compares every answer, then removes what it wrote under its prefix (objects, and on a versioned bucket every version and delete marker; exit 70 if anything remains). Several: replayed on each at once, side by side (differing answers, each kind's p95 against the recording; `-f markdown\|csv` too); exit 70 for a store it could not replay on. [Recording](recording.md#replay-on-another-endpoint). |
| `sonde check [FILE] [options] [-f json\|markdown\|github]` | A run against a budget, a baseline and its trials. Exit 1 when over budget, past the baseline, or a trial broke. [Details](#sonde-check). |
| `sonde share [[BEFORE] FILE] [-o DIR] [--serve [--port N]] [--html [--baseline FILE\|auto\|URL]] [--redact]` | A static site: the viewer and `session.json` (default `./<session>-share/`). `--serve`: on 127.0.0.1 until Ctrl-C. `--html`: one self-contained file (findings, time, timeline, operations at `#op-N`; past 2,000 operations, the first 2,000 and every one a finding links to, with a count). Titled by the program. Two runs, or `--baseline` (as `check` takes it): the comparison first (requests, waiting, cost, errors before and after; kinds that changed; findings gone), then the run, its new findings first and marked; two runs without `--html` is a usage error. `--redact`: bucket and key names become tokens (same name, same token; layout names, numbers, extensions kept); headers, bodies, URLs, request ids, output and arguments dropped. |
| `sonde share --otlp [FILE] [-o FILE \| --to URL]` | OTLP/JSON traces: a span per operation, its requests under it; `service.name` is the program. `--to` posts to a collector (e.g. `http://localhost:4318/v1/traces`) in parts of about 3 MiB, an operation's spans kept together. |

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
| `--program-exit CODE` | The program's exit status: one not 0 heads the comment (`programExit` in JSON), with the request the policy refused. |
| `--init` | Write a `sonde.toml` this run passes: counts with 10% room, times commented out beside their measured values, a `[program] name` from the command when there is none (`etl` for `python etl.py --day 2`); the baseline, trial verdicts and findings kept under the command move to that name. No trials. |

- Output names the session and program first. A trial row is `pass` / `warn` / `FAIL` / `skip` (survived / degraded / broke / untested). A failed row says by how much (`+23%`) over its limit or the baseline's figure.
- Closing verdict, in every format: `within budget` / `over budget`, `within its baseline` / `past its baseline` (joined with `and`), then `, and broke under a trial`; trials alone: `broke under a trial` / `did not break under its trials`.
- No baseline set yet: the budget alone decides; said in every format: `no baseline yet` on stderr and in the verdict, `noBaseline` in JSON, a quoted line in markdown, a `::warning` with `-f github`. It names a baseline of the same program under other arguments if there is one. A baseline of another program is named on stderr.
- Past a baseline over several runs: since which run, and the commits around it (`git`, else `GITHUB_SHA`), once (`past the baseline since a1b2c3d, 2 runs ago; within at 9f8e7d6`; this run the first past: `the run before, at 9f8e7d6, was within it`).
- `-f json`: `trials`, `passed` (whole check), `verdict`, `check` (rows, each with `passed`). `-f markdown`: a pull-request comment ([what it says](ci.md#the-comment)). `-f github`: Actions annotations, findings on the line that sent their requests.
- `--baseline auto` with none set and no budget: nothing checked, exit 0. Usage error (64): nothing to check (no
  budget, baseline or trials); `--tolerance` or `--no-new-warnings` without a baseline; a `sonde.toml` that does not parse or has an unknown key; an unreadable `baselines.json` (for `auto`, `--set-baseline`, `sessions prune`; never treated as empty).

## Buckets

| Command | Does |
|---|---|
| `sonde probe -e A -e B -b BUCKET [--only checksum.] [--temp-buckets] [--no-slow] [-f json\|markdown\|csv] [-o FILE]` | Probes each endpoint in parallel, saves the run. Writes and deletes under `sonde-probe/` in the named bucket (naming it is consent); audited. |
| `sonde probe list` / `runs` / `diff RUN:EP RUN:EP` | The catalog; saved runs; changes between two. |
| `sonde bucket size -e E -b B [-p PREFIX] [--no-multipart] [--cached]` | What a bucket holds and why it is so big: versions, classes, prefixes, sizes. |
| `sonde bucket uploads -e E -b B [-p PREFIX]` | Incomplete uploads: bytes, age, cost (stale past 7 days); the lifecycle verdict. |
| `sonde bucket abort-uploads -e E -b B [--older-than-days N] [--confirm COUNT]` | A dry run; with `--confirm` aborts (audited). |
| `sonde bucket lifecycle -e E -b B [--days 7] [--confirm BUCKET]` | Adds an AbortIncompleteMultipartUpload rule: dry run, then `--confirm` (audited). |
| `sonde layout inspect -e E -b B [-p PREFIX] [--kind K] [--at RFC3339]` / `--dir PATH` | Detects and decodes a layout (`slatedb`, `iceberg`, `lakefs`, `woodpecker`, `turbopuffer`, `warpstream`, `generic`); `--kind` on a prefix that is not that layout says so, exit 4; `--at` gives the state at a time. |
| `sonde endpoints list` / `test [ID]` / `import [PROFILES…]` / `add ID URL [--profile P] [--replace]` | Endpoints. `import` takes `~/.aws/config` profiles not yet configured. `add` refuses an id taken (64) unless `--replace`; it takes `--region`, `--path-style`, `--ca-file`; credentials from a profile or the default chain, never the command line. |

## More

| Command | Does |
|---|---|
| `sonde tui [--demo] [--no-mouse]` | The terminal UI: `--demo` starts on the built-in store; `--no-mouse` leaves the mouse to the terminal. (Its developer flags are not listed and not in the [contract](contract.md).) |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Done; nothing to report |
| 1 | A probe failed (a guarantee broke), or a layout has errors; `check` (or one `import s3://…` pass) is over its budget or baseline, or the run broke under a trial; `run --trial` when a trial broke |
| 2 | Probes behaved differently from AWS (and none failed); a replay answered differently |
| 3 | Probes could not run (errors only) |
| 4 | Findings worth attention: stale uploads or no abort rule, layout warnings, a changed probe diff, a dry run |
| 64 | Usage or configuration error (an unknown command or option too, and a `-f` format the command does not write; `--help` and `--version` exit 0) |
| 65 | Bad data: not a session (no header, a newer format, a bad line in its middle). A torn last line or a cut gzip stream is read up to the cut (said on stderr) |
| 70 | The operation failed (connection, credentials, permission); a replay could not remove what it wrote |
| 77 | Refused: the confirmation did not match, or a replay without `--yes` and no terminal to ask |
| 130, 143 | `run --trial` interrupted by Ctrl-C or SIGTERM (`run` otherwise exits as its command did) |

A failed request prints its error, request id and the request line from the log. A closed standard output
(`sonde check | head`) ends the output, not the command: the exit code is the command's own.

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
and `[record]` (`policy`, `name-callers`, `tls`); [configuration](configuration.md) says which file holds what. Any
other key is an error.

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
# findings kept on purpose, never a new warning: a finding's `identity`
# (as `show -f json` and the app's "Expected" give it) or a kind for all of
# it. Anything else is an error (exit 64)
expect = ["obs.hot|slate/db/gc/manifest.boundary", "obs.listed"]
```


## Which code sent a request

sonde reads an `x-sonde-caller: file:line function` header (never forwarded), the SDK and version from the user
agent, and the trace from `traceparent`. `sonde run` adds the header before signing (off: `--no-callers`, or
`[record] name-callers = false` in `sonde.toml`), naming the program's first own frame; work handed to a thread pool (boto3's transfer
manager) is named by the line that handed it:

| Language | SDK | How |
|---|---|---|
| Python | boto3 / botocore | a `sitecustomize` first on `PYTHONPATH` (a program's own still runs); it imports nothing until the program imports botocore |
| Node.js | AWS SDK for JavaScript v3 | `NODE_OPTIONS=--require …/sonde-caller.cjs`: a middleware on each client |
| Java | AWS SDK for Java 2 | `JAVA_TOOL_OPTIONS=-javaagent:…/sonde-agent.jar`: a global `ExecutionInterceptor`, only when the SDK is on the class path (not a Spring Boot jar's own loader) |

The Java hook's sources are in `hooks/java` (`build.sh` rebuilds its jars). Other SDKs can add the header with their
own request hook. The hooks are written whole (a file renamed into place) and only when changed, so runs at once
never load half of one.

## The fix loop

A finding keeps one identity across a program's runs: its kind and what it is about (key, prefix), never counts. What
changes from run to run in a key is said as what it is, so `jobs/1794558464595/state.json` is
`obs.hot|b/jobs/{n}/state.json`: `{uuid}`, `{date}`, `{time}`, `{n}` (a number of 9 digits or more, a metadata
version `v{n}.metadata.json`, a Hadoop `_temporary/` attempt's numbers), `{id}` (long hex or mixed ids). Between a
run and the program's run before it that did not fail:

| State | When | Says |
|---|---|---|
| fixed | gone at any threshold | what it saved, measured, against the predicted: `saved 20 requests (predicted 20)` |
| less | still there, under its threshold | what it saved so far |
| back | not in the run before; fixed by it or an earlier run | |
| new | in neither run before, at any threshold | |
| open | in both (under its threshold in the run before, too) | counted: `2 still open` |

A finding of time alone (requests one after another) is judged by its requests: as many of them one after another,
under its threshold, is open. A run that failed (its exit code is not 0) is not compared: `the run failed (exit 3);
not compared`, and nothing kept changes; the next run is compared with the last that did not.

Measured: of the requests the finding named, those the later run no longer sends (by kind and key); for a finding
of time, the fall in its own waiting, at most the fall in the run's. Each program's findings (open, less, fixed with
its saving, back) are kept in `findings.json` beside its runs (as `verdicts.json` and `baselines.json`), brought up
to date by `run`, `show` and the app when they read the program's newest run. `run`, `show` (`since` in JSON),
`compare` and `check -f markdown` say a run's findings the same way.

