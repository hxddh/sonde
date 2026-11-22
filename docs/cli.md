# CLI

`sonde` is the CLI. `sonde help <command>` lists every flag.

## Common

| | |
|---|---|
| Global options | `--config-dir` (`$SONDE_CONFIG_DIR`), `--data-dir` (`$SONDE_DATA_DIR`) |
| Store options | `--url` (an ad hoc endpoint, `adhoc`), `--region` (else an AWS URL's, `s3.<region>.amazonaws.com`; else `AWS_REGION`, `AWS_DEFAULT_REGION`; else us-east-1; `endpoints add` takes it so too), `--path-style`, `--ca-file`. Not on commands that only read files. sonde's own pace is `concurrency` and `rate-limit` in `config.toml` (`--concurrency`, `--rate` also taken, not listed). |
| `-f`, `--format` | `text` (default) or `json`; `check` also writes `markdown` and `github`. A command that writes only text has none. |
| Confirmation | What changes a store or deletes runs (`runs prune`, `bucket abort-uploads`, `bucket lifecycle`, `compare --replay`) is a dry run, exit 4; `--yes` does it. |
| Streams | The answer on stdout; hints, progress and the summary of `run` and `import` on stderr. Ctrl+C cancels the running job. A failed request prints its error, request id and the request line from the log. Keys, commands and other text sonde did not write reach the terminal with their control characters written out (`\e`, `\x07`, `\u{9b}`): a key cannot set the window's title or move the cursor. JSON escapes them as JSON does (`\u007f`, `\u202e`). A closed standard output (`sonde check \| head`) ends the output, not the command: the exit code is the command's own. |
| A run argument | A path or a name, resolved in order: a file; `runs/<name>[.sonde]`; `runs/trials/<name>` (e.g. `20261108-073328-etl.slowdown`). Not found: usage error naming where it looked. |
| Which run | When none is named: [below](#which-run). |
| A program | `--name`, else `sonde.toml`'s `[program] name` for the program it `runs`, else what the command runs: the script an interpreter runs (`etl` for `python3 etl.py 5`), a module (`pytest` for `python -m pytest`), the command a shell is given (`aws` for `sh -c 'aws s3 cp …'`), a container's image (`aws-cli` for `docker run amazon/aws-cli …`), else the program (`load` for `./load --all`; `python3` for inline code, `python3 -c …`). Its arguments are not part of it: a run with others is another run of it, and the [fix loop](#the-fix-loop) says the command changed. Runs, baselines and trial verdicts go by it. |
| Size | Runs are streamed and analysed once; `compare` reads one run at a time, so two cost what one does. Memory grows with the requests ([what a million cost](dev/performance.md#large-runs)). |
| `.index.jsonl` | All that is kept beside the runs, one file per runs folder: each run's counts, its trials' verdicts, its exit code and title; each program's baseline, findings and trial history; the files imported in the app; where each `import s3://` got to ([how](dev/architecture.md#a-recording-and-what-is-read-from-it)). A run that cannot be read is listed with `!` (`unreadable` in JSON). |

### Which run

When none is named, `show`, `compare`, `share` and `check` take:

1. the newest run of `[program] name`;
2. else the newest recorded in this folder (the run's `cwd`).

None is a usage error (64), never another folder's run: name the run (`sonde runs` lists them).

Only runs from `run` or `proxy` count (never a replay, import or trial), in the runs folder or written elsewhere with
`-o` (as `sonde runs` lists them). `check` says which it took. A `sonde.toml` is applied only to its own program's
runs (by its name, else recorded in its folder or below): `check` of another's says so and leaves its `[check]` out
(one given with `--config` is applied).

## Record

| Command | Does |
|---|---|
| `sonde` | Three lines: the tour, a real run, where to read more. |
| `sonde run [-e E \| --demo] [--policy P] [--tls] [--live] [--no-callers] [--name N] [-o FILE [--force]] -- CMD…` | Runs `CMD` against a local endpoint that re-signs and records its S3 requests ([recording](recording.md)). Prints the summary as `show` does, ending with the change since the program's run before ([fix loop](#the-fix-loop)); when nothing was sent, how to point the program's own client at sonde, that client first. Exits with the command's status (128 + signal; 143 when sonde is sent SIGTERM, said as such). With no endpoint configured it goes where an AWS SDK would: the default credential chain and region (`AWS_REGION`, the profile's, else us-east-1; `AWS_ENDPOINT_URL` if set), said on stderr. An unknown option before `--`, or no endpoint and no AWS credentials, is a usage error (64); a command that cannot start leaves no run (70). |
| ↳ `-o FILE` | Without it: `runs/<time>-<name>.sonde`, named by the program. With it: the run there. A file there is refused (64) unless `--force`. Recorded beside it and put in place at the end: two runs given one file each keep their own (the second under a fresh name, said). |
| ↳ `--demo` | The built-in store (buckets `demo`, `demo-large`, `demo-layouts`), in sonde's process, fresh each run: a first run with no account, or a CI check of requests and findings with no secrets. Its times are not meaningful (said on stderr). Without a command, the tour: a small export job (written to `<data>/tour/export.py`) played by sonde as boto3 sends it, recorded and shown with its finding, the line that sent it and the fix; then the job with that fix (`export-fixed.py`) recorded again, ending `✓ fixed … (predicted …)`. Two runs of program `tour` (with `-o FILE`, the fixed one beside it as `FILE-fixed.sonde`, refused when there as `FILE` is). The app's welcome demo runs the same job (`sonde demo-job export.py`, played in the app), opens its run on the finding, and *Apply and run again* writes the fix into `export.py` and runs it again. |
| ↳ `--policy allow\|no-delete\|read-only` | What the program may do: [what each refuses, and which applies](recording.md#what-a-recorded-program-may-do). |
| ↳ `--tls` | Also serve TLS, with a certificate the program is told to trust (`[record] tls = true`). |
| ↳ `--live` | Each request on stderr as it happens. |
| ↳ `--no-callers` | Do not name the code line that sent each request (named by default where the language allows, [below](#which-code-sent-a-request)). |
| ↳ `spark-submit`, `flink run` | Given s3a's endpoint on their command line ([recording](recording.md#spark-flink-trino-and-hadoop-s3a)). |
| ↳ `docker run`, `podman run` | Given the host's network and sonde's variables ([recording](recording.md#what-the-program-is-given)). |
| `sonde run --trial [--fault S …] -- CMD…` | The clean run, then one per fault scenario, each with a verdict ([trials](trials.md)). `{trial}` in the command and `SONDE_TRIAL` are `clean`, then the scenario's name. A failed or interrupted clean run is not tried. Ctrl-C / SIGTERM ends the trial (130 / 143), no verdict for the interrupted scenario. Exit: the command's status if not 0, else 1 if a trial broke. A run to a path that holds a run first removes that run's earlier verdicts and trials; the program's history of them is kept. |
| ↳ `--again` | `--trial` implied: a fresh clean run, then only the scenarios whose last verdict for this program was broke, degraded or refused. Nothing left: says so, exit 0. Never tried: 64. |
| ↳ `--fault S` | Repeatable: `slowdown`, `error`, `skew`, `lost-answer`, `fail-complete`, `complete-500`, `cut-body`, `slow-first-byte`, `lost-race`, `stale-list`, `slow`, `s3-limits`, or a `sonde.toml` scenario. `slowdown:N` / `error:N` (1–5) fail the first N attempts. |
| `sonde proxy [--listen ADDR] [--allow-remote] [--policy P] [--tls] [-o FILE]` | The same endpoint, standing (default `127.0.0.1:9000`): prints the variables to set, then each request; its summary at Ctrl-C. Loopback only unless `--allow-remote` (then anyone holding the printed keys). |
| `sonde import s3://B/PREFIX [-e E]` | Production from its S3 access logs or CloudTrail files (GET and LIST only, no proxy): what arrived since the last import of that prefix (the first, the last day's), a run of program `logs of s3://…` (history, baseline, `check`), then shown. Where it got to is kept; an upload not completed yet waits (up to a day of the logs' time) and is read with the import that completes it. Exit 70 when the logs cannot be listed, none could be read, the prefix holds no files or none is a log; nothing new since the last import is said, exit 0. |
| `sonde import FILE\|FOLDER [-o FILE [--force]]` | An S3 access log, CloudTrail (`{"Records": …}` or one event a line) or HAR, gzipped or not, as a run. A folder is every file under it, in name order, as one run. Skipped lines and events (not requests, unreadable, a HAR's other hosts, another service's CloudTrail events) are counted on stderr. Named by the store's host. Whole-second log times are spread evenly over their second (`wholeSeconds` in the header, a line in `show`), so in-flight counts, the waiting, "one after another" and a throttling finding's in-flight figure are not given. Times not recorded (CloudTrail) are "—". Upload parts with no create in the file are not "never completed". A HAR's `timings.wait` is the first byte; its status 0 (aborted, blocked) is no answer, and not priced. |

## Read a run

| Command | Does |
|---|---|
| `sonde runs [-f json]` | Runs, newest first, with those written elsewhere with `-o` (by their path; never pruned): program (name, else command; cut in the middle to the terminal's width), the client that sent its requests (from their user agents: `boto3`, `Lance`, `Iceberg (pyiceberg)`; two when each sent a tenth), requests, duration, errors, the program's exit code (blank when not known: a proxy, a replay, an import; a table leaves the column out when none is known), worst trial verdict. |
| `sonde runs prune --keep N [--yes]` | Delete all but the newest N (baselines kept): a dry run (exit 4 when it would delete any), then `--yes`. |
| `sonde show [FILE] [--requests] [--errors] [--all] [--by DIM] [--app] [-f json]` | The summary, one shape for `run` and `show`: head line (requests, and operations when they are not nearly the requests; SDK, endpoint, duration, waiting, cost from a cent and [a month](#money-a-month), errors, not found); the findings, those that save the most money first, then time, then the rest, each with its saving (a month too), code lines (`at file:line`) and the fix for the detected SDK; their total, `in all (N findings): …`; one table by kind of request (requests, errors, p95, waiting and its share, cost share; `*` under 20 requests when others have more); the change since the program's run before. |
| ↳ `--all` | Each operation on its line first; the table with p50 and max and every kind; the four signals (rate, errors, latency, bytes), first byte, cost and the prefixes past half of S3's request limit. |
| ↳ `--requests` | Each request on its line, then the summary. Past 2,000 lines: the first and last hundred and a count (`--all` prints all). `-f json`: the requests whole, left out otherwise. |
| ↳ `--errors` | Only the operations (or `--requests`) that failed, no summary (JSON: `operations`, `requests`; the summary stays the run's). |
| ↳ `--by kind\|prefix\|size\|caller\|sdk\|status\|key` | The requests split instead of the table by kind: requests, errors and share, p50, p95, p99, waiting share (not for an imported log, whose waiting is not known), cost per group (most waiting first; past 12, `other`). `caller`: the code that sent them; an imported log's, who sent them (the access log's requester, the CloudTrail identity). `key`: each object (bucket/key), the hot keys first. Size bands: `0 B`, `< 64 KiB`, `64 KiB-1 MiB`, `1-8 MiB`, `8 MiB+` (the larger of sent and received). Then one sentence: where the slow requests (the slowest tenth, at or over p90; 3 at least) concentrate, narrowed by prefix, size or kind when one holds most of them, and where the cost does (`80% of the slow requests (over 120 ms) are under b/wal/, over 8 MiB.`). A group is named when it holds half of them and 1.5× its share of the requests. `-f json`: [show-by](schemas/show-by.schema.json). |
| ↳ `in all (N findings)` | The findings' total: the run recomputed without the requests they name (findings naming none are left out, and the line says which count). |
| ↳ `-f json` | The answer, its size not growing with the run: `run`, `summary`, `operationsTotal`, `waiting`, `cost`, `month`, `whatIf`, `findings` (each with its first 20 operations), `verify` (the command that records the program again), `since`, `trials`. `--all` adds every operation and each finding's operations and fixes whole; `--errors` the failed operations; `--requests` the requests, streamed. Every field: [show](schemas/show.schema.json); what an agent reads: [agents.md](agents.md). A trial's run says first that its faults were sonde's. |
| `sonde compare [RUN [RUN]] [-f json]` | Two runs: requests, waiting, time, errors and cost by kind of request (the two endpoints named when they differ: `a.sonde on rfs → b.sonde on demo`, `endpoints` in JSON); each finding fixed, less, more, back or new, and each kind of failure whose count moved ([fix loop](#the-fix-loop)). One run: it and its program's run before; none: the run [as above](#common) and the one before; a program's first run has none before it (said, exit 0). Unchanged kinds are counted, not listed. Different programs compare by figures only (said). A run and its replay (either order): every answer compared, with each kind's p50 and p95 on both stores. |
| `sonde compare FILE --replay E [--replay E2 …] [--bucket A=B] [--prefix P] [--yes] [-o FILE]` | A dry run (what it would send, where), then with `--yes` re-enacts a run on another endpoint under a prefix of its own (never on the recorded keys; what the run read and had not written is written there first), compares every answer, lists what the program relies on with the store's verdict on each, and each store's request cost; then removes what it wrote under its prefix (objects, and on a versioned bucket every version and delete marker; exit 70 if anything remains). Several: replayed on each at once, side by side; exit 70 for a store it could not replay on. [Recording](recording.md#replay-on-another-endpoint). |
| `sonde check [FILE] [options] [-f json\|markdown\|github]` | A run against a budget, a baseline and its trials. Exit 1 when its program failed, nothing reached sonde, over budget, over the baseline, or a trial broke. [Details](#sonde-check). |
| `sonde share [[BEFORE] FILE] [--baseline FILE\|auto\|URL] [-o FILE [--force]] [--redact]` | One HTML file (default `./<run>.html`; refused when there unless `--force`, as every `-o` file is): the app's views read-only — figures, findings, operations, output, the Time pane with every chart, a request in full — the viewer and the run in the file, nothing fetched (it opens from a disk as from a web server); it holds what the run holds, so share it as you would the run. Titled by the program. Two runs, or `--baseline` (as `check` takes it): it opens on what changed (the app's Compare: figures before and after, kinds and prefixes that changed, the two over time, latency, findings gone and new). `--redact`: bucket and key names become tokens (same name, same token; layout names, numbers, extensions kept); headers, bodies, URLs, request ids, output and arguments dropped. |

### sonde check

| Option | |
|---|---|
| `--max-requests N`, `--max-waiting T`, `--max-errors N`, `--max-transfer SIZE` | Budget. Times as `2s`, `250ms` or ms; sizes as `10GiB`, `500MB` or bytes (the bytes the store sent). An error is a request that failed after its retries (a 503 retried into a 200 is none; a miss is none). |
| `--baseline FILE\|auto\|https://…\|none` | The run is held to its requests, waiting, cost and errors with `--tolerance` of room, and to no warning it did not have. `auto`: the baseline set for the run's program (in the index, shared with the app). A URL is fetched (CI artifact, presigned). `none`: no baseline, not sonde.toml's either. A file that is not there, or an `http://` URL: 64. |
| `--set-baseline` | Make this run its program's baseline, in the runs folder or written elsewhere with `-o` (refused for a run that failed: 64). |
| `--tolerance PCT` | Growth allowed over the baseline (10; 0 or more). |
| `--config FILE` | Default: `sonde.toml` here or in a folder above, up to the repository root. Its `[check]` takes each option above by its name (`max-requests = 5000`, `max-waiting = "2s"`, `baseline = "auto"`); flags win. |
| `expect` | `sonde.toml` only: findings kept on purpose, each with why, `{ "obs.hot\|data/conf.json" = "rewritten by the scheduler", "obs.listed" = "…" }`: a finding's `identity` (as `show -f json` and the app's "Expected" give it) or a kind for all of it. Never a new warning; `show`, `run` and `check` say the reason with the finding (`expected` in JSON). |
| `--report-url URL` | `-f markdown` only: the link the comment ends with (the HTML report). |
| `--init` | Write a `sonde.toml` (or `--config`'s file) this run passes: requests with `--tolerance` of room, `max-errors` the run's errors, waiting commented out beside its measured value. No trials. Refused (64) when the file is there, or for a run that failed or sent nothing. Prints the workflow step, `uses: hxddh/sonde@vN` at this sonde's major version. |

- Text and JSON say each figure `within` or `over` (by how much, `+23%`, over its limit or the baseline's figure);
  markdown says a figure over its baseline in bold, `past the N it may use`. A trial is `survived`, `degraded`,
  `broke`, `untested` or `refused`, as `run --trial` says it. Output names the run and program first.
- Closing verdict, in every format: `within budget`, `within its baseline`, `within budget and its baseline`,
  `over budget`, `over its baseline` or `over budget and over its baseline`, then `, and broke under a trial`.
  Trials alone: `broke under a trial`, `did not break under its trials`, or `nothing was tried`; nothing at all:
  `nothing checked`. A program that failed comes first: `the program failed (exit 3); within budget` (alone when
  nothing was checked). A run of no request fails as `nothing reached sonde`, against a budget or a baseline (its
  client went around sonde; `run` says so too).
- No baseline set yet: the budget alone decides; said in every format: `no baseline yet` on stderr and in the verdict, `noBaseline` in JSON, a quoted line in markdown, a `::warning` with `-f github`. A baseline of another program is named on stderr.
- Over a baseline for several runs: since which run, and the commits around it (`git`, else `GITHUB_SHA`), once (`over the baseline since a1b2c3d, 2 runs ago; within at 9f8e7d6`; this run the first past: `the run before, at 9f8e7d6, was within it`).
- `-f json`: `trials`, `passed` (whole check), `verdict`, `check` (rows, each with `ok`). `-f markdown`: a pull-request comment ([what it says](ci.md#the-comment)). `-f github`: Actions annotations, findings on the line that sent their requests.
- `--baseline auto` with none set and no budget: nothing checked, exit 0. Usage error (64): nothing to check (no
  budget, baseline or trials); `--tolerance` without a baseline; a `sonde.toml` that does not parse or has an unknown key.

## Buckets

| Command | Does |
|---|---|
| `sonde probe -e A -e B -b BUCKET [--only checksum.] [--temp-buckets] [--no-slow] [-f json] [-o FILE]` | Probes each endpoint in parallel, saves the run; what did not pass says why in its row. Writes and deletes under `sonde-probe/` in the named bucket (naming it is consent); audited. |
| `sonde probe list` / `runs` / `diff RUN:EP RUN:EP` | The catalog; saved runs; changes between two. |
| `sonde bucket size -e E -b B [-p PREFIX] [--no-multipart] [--cached]` | What a bucket holds and why it is so big: versions, classes, prefixes, sizes. |
| `sonde bucket uploads -e E -b B [-p PREFIX] [-f json]` | Incomplete uploads: bytes, age, cost (stale past 7 days), or that there are none; the lifecycle verdict. |
| `sonde bucket abort-uploads -e E -b B [--older-than-days N] [--yes]` | A dry run; with `--yes` aborts (audited). |
| `sonde bucket lifecycle -e E -b B [--days 7] [--yes]` | Adds an AbortIncompleteMultipartUpload rule, the bucket's other rules kept: a dry run (its rules and the one to add, each in a line), then `--yes` (audited). |
| `sonde layout -e E -b B [-p PREFIX] [--kind K] [--at RFC3339]` / `--dir PATH` | Detects and decodes a layout (`slatedb`, `iceberg`, `lakefs`, `woodpecker`, `turbopuffer`, `warpstream`, `generic`); `--kind` on a prefix that is not that layout says so, exit 4; `--at` gives the state at a time. |
| `sonde endpoints list` / `test [ID]` / `import [PROFILES…]` / `add ID URL [--profile P] [--replace]` | Endpoints. `import` takes `~/.aws/config` profiles not yet configured. `add` refuses an id taken (64) unless `--replace`; it takes `--region`, `--path-style`, `--ca-file`; credentials from a profile or the default chain, never the command line. |

## More

| Command | Does |
|---|---|
| `sonde completions SHELL` / `sonde man DIR` | Shell completions (`bash`, `zsh`, `fish`, `powershell`, `elvish`) to stdout; the manual pages (`sonde.1`, one per command) into `DIR`. Neither is listed in `--help`. The CLI's archives carry both (`completions/`, `man/man1/`); `install.sh` puts the pages beside the binary (`share/man/man1`). |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Done; nothing to report |
| 1 | A probe failed (a guarantee broke), or a layout has errors; `check`: the program failed, nothing reached sonde, over its budget or baseline, or the run broke under a trial; `run --trial` when a trial broke |
| 2 | Probes behaved differently from AWS (and none failed); a replay answered differently |
| 3 | Probes could not run (errors only) |
| 4 | Findings worth attention: stale uploads or no abort rule, layout warnings, a changed probe diff, a dry run |
| 64 | Usage or configuration error (an unknown command or option too; `--help` and `--version` exit 0) |
| 65 | Bad data: not a run (no header, another format version, a bad line in its middle), an imported CloudTrail document cut short (said where), or an import with nothing in it (an empty file or folder, a log of no S3 requests). A torn last line or a cut gzip stream is read up to the cut (said on stderr) |
| 70 | The operation failed (connection, credentials, permission); a replay could not remove what it wrote |
| 77 | Refused by sonde before anything was sent: an endpoint it protects, a plan changed since its dry run |
| 130, 143 | `run --trial` interrupted by Ctrl-C or SIGTERM (`run` otherwise exits as its command did) |

## In CI

[The GitHub Action, the comment, and the same in any CI](ci.md); `sonde.toml`'s tables:
[configuration](configuration.md#sondetoml). Without an artifact kept from the main branch: set a baseline with
`sonde check --set-baseline` (or in the app) and keep the data directory between jobs (a cache).

## Which code sent a request

sonde reads an `x-sonde-caller: file:line function` header (never forwarded), the SDK and version from the user
agent, and the trace from `traceparent`. `sonde run` adds the header before signing (off: `--no-callers`, or
`[record] name-callers = false` in `sonde.toml`), naming the program's first own frame; work handed to a thread pool (boto3's transfer
manager) is named by the line that handed it:

| Language | SDK | How |
|---|---|---|
| Python | boto3 / botocore | a `sitecustomize` first on `PYTHONPATH` (a program's own still runs); it imports nothing until the program imports botocore |
| Node.js | AWS SDK for JavaScript v3 | `NODE_OPTIONS=--require …/sonde-caller.cjs`: a middleware on each client, in CommonJS and ES modules (`import`) alike, on any Node.js the SDK runs on (no flag but `--require`: Node.js loads the SDK as CommonJS either way); not in a program bundled into one file |
| Java | AWS SDK for Java 2 | `JAVA_TOOL_OPTIONS=-javaagent:…/sonde-agent.jar`: a global `ExecutionInterceptor`, only when the SDK is on the class path (not a Spring Boot jar's own loader) |

The Java hook's sources are in `hooks/java` (`build.sh` rebuilds its jars). Other SDKs can add the header with their
own request hook. The hooks are written whole (a file renamed into place) and only when changed, so runs at once
never load half of one.

## Findings

Each finding is one of these kinds (`kind` in JSON, what `[check] expect` names), with what it saves (requests, time,
money a month), the line that sent it and the fix: the [engine's setting](#engine-settings) when an engine sent its
requests, else the code for the SDK that sent them (boto3, Java, Go, JavaScript, Rust's object_store):

| Kind | Found when | Fix |
|---|---|---|
| `obs.hot` | a key read 10 times or more between its writes, each stretch answered the same (found, whole, one ETag), saving 1% of the run's requests or cost | keep what was read |
| `obs.listed` | a prefix listed (and answered) 5 times or more with nothing written under it between | list once |
| `obs.crawl` | 20 prefixes or more each listed fewer than 5 times (partition discovery, a tree walked folder by folder), saving 1% of the run's requests or cost (one finding of the run, `obs.crawl|`) | a manifest, or the table format's metadata |
| `obs.rangeagain` | the same byte range of an object read 10 times or more past its first read with nothing written to it between (a Parquet footer read by each query), saving 1% of the run's requests or cost (one finding of the run) | keep each range once read |
| `obs.wholeread` | objects of 1 MiB or more read whole that the run also reads by range (evidence it uses parts), what they read past their ranges 16 MiB or more and a tenth of the run's bytes out; saves bytes, and egress when charged (one finding of the run) | read the ranges used |
| `obs.smallreads` | 500 objects or more under 1 MiB each read whole one GET each, a fifth of the run's requests; the saving is the GETs left once they are shards of 64 MiB, not their bytes, which the shards still read (one finding of the run) | shard into tar or WebDataset files |
| `obs.headget` | HEAD then GET of the same object, 3 times or more | GET alone |
| `obs.small` | 50 objects or more under 64 KiB written one PUT each, to folders of two or more, a fifth of the run's cost; or 3 multipart uploads completed with one part under 5 MiB, where a PUT would do (one finding of the run, `obs.small|`) | batch into fewer, larger objects; one PUT |
| `obs.deletes` | 20 objects or more deleted one request each, saving 1% of the run's requests | `DeleteObjects` (1,000 a request) |
| `obs.rmw` | a key read and written back with no `If-Match`, 3 times or more (of an import, said as not known whether) | a conditional write |
| `obs.polled` | a key asked for 5 times or more while not there (404), saving 1% against backing off from a second to a minute | back off, a waiter, an event |
| `obs.throttled` | requests throttled (503, 429), with how many were in flight (a warning when one gave up after its retries or a tenth were throttled); or a prefix near S3's rate for reads or writes | adaptive retries; keys spread over prefixes |
| `obs.serial` | requests one after another (none reading or listing what the one before wrote) that 8 at once would finish 100 ms sooner | at once |
| `obs.unfinished` | multipart uploads never completed or aborted | a lifecycle rule |
| `obs.orphans` | a table's files written with no commit after them | the table's cleanup |
| `obs.rename`, `obs.markers` | Hadoop s3a: commits by rename, directory markers deleted | the magic committer; keep markers |

### Engine settings

When a finding's requests are an engine's (its table format's files) or a tool's (its user agent), the fix is that
engine's own setting, its value, and the `file:line` in the program's folder that sets it now (`where`, when sonde
finds it there). No SDK code is offered for a program that does not call the SDK itself.

| Engine | Finding | Setting |
|---|---|---|
| SlateDB (`Db`, a writer; `DbReader`) | the manifest listed or read again and again, more often than its default | `manifest_poll_interval`: `1s` (`Settings`, `SlateDb.toml`) or `10s` (`DbReaderOptions`), with the period seen |
| SlateDB | small WAL files, flushed more often than every 500 ms | `flush_interval = 1s` (a write waits up to it to be durable) |
| DuckDB | HEAD before GET | `SET enable_http_metadata_cache = true` |
| DuckDB | a file read again and again | `SET enable_external_file_cache = true` (1.3+, on by default there) |
| rclone | HEAD before GET; small files in parts; throttled | `--s3-no-head-object`; `--s3-upload-cutoff 200Mi`; `--tpslimit` half the peak a second |
| s5cmd | throttled with many in flight | `--numworkers` under the count throttled |
| Trino (`app/Trino` in its user agent) | throttled | `s3.retry-mode=ADAPTIVE` (native S3 file system) |
| Hadoop s3a (Spark too) | commits by rename; markers deleted | `fs.s3a.committer.name = magic`; `fs.s3a.directory.marker.retention = keep` (the default from 3.4.0) |
| Delta, Lance, Iceberg | orphans | `vacuum` (delta-rs; `VACUUM` in Spark), `cleanup_old_versions`, `remove_orphan_files`, on the table's own path |

Polars, pyiceberg and delta-rs (besides its `vacuum`) have no setting that acts on these findings, so they get none;
nor do Hudi and Paimon yet.

## The fix loop

A finding keeps one identity across a program's runs: its kind and what it is about (key, prefix), never counts, and
never what changes from run to run in a key: the value of a `name=value` folder is `{v}`, a UUID `{uuid}`, a date
`{date}` (with a time `{time}`), a word of 6 or more hex letters and digits or a long mixed id `{id}`, every other run
of digits `{n}`. So `in/year=2026/month=11/day=13/hour=10/conf.json` is `obs.hot|b/in/year={v}/month={v}/day={v}/hour={v}/conf.json`,
`part-00000-<uuid>-c000.snappy.parquet` is `part-{n}-{uuid}-c{n}.snappy.parquet`, `job-3fa2b1c9` is `job-{id}`.
Two findings of one run that read the same so (`t00/` and `t01/` listed again and again) keep their keys as they are.
HEAD-then-GET is about the folder its objects are in. A read written back, deletes one at a time and small writes are
about their bucket (`obs.rmw|data`): one finding a run, whatever key or folder its text names as its counts move.

These rules have a version (5: a read written back, single deletes and small writes are about their bucket), kept with each program's findings: findings kept under older rules are
made again by these when read; two that become one are let go. `[check] expect` entries are read by the rules of now.

Between a run and the program's run before it that did not fail, by its counts (times listed, read, throttled):

| State | When | Says |
|---|---|---|
| fixed | gone at any threshold (over it in the run before, or under it and kept as less) | what it saved, measured, against the predicted: `saved 20 requests (predicted 20)` |
| less | still there (at any threshold), fewer | what it saved so far, `· 92 → 41` |
| more | still there (at any threshold), more | `↑ more … · 92 → 216` |
| back | under its threshold in the run before, or fixed by it or an earlier run; over it now | |
| new | in neither run before, at any threshold; every finding of a program's first run | |
| open | as much in both, over its threshold or not | counted, `N still open, as above` (each is listed with its line and fix above; `compare` lists none: `N still open`) |

Each kind of failure (status and S3 code, `no answer`; throttling is its own finding) is compared too: one whose count
moved is a finding of the change, `obs.errors|500 InternalError`, new, more, less or fixed.

A program whose command changed between the two runs (beyond a trial's place; `sh -c X` and `bash -c X` are `X`) says so first (`the command changed (--limit 200 → 20)`, `commandChanged` in JSON), and so does each finding fixed, less or more, with no saving counted: a smaller workload is not a fix. So does a workload that grew by half or more with the same command (`the workload grew (requests 120 → 402, 3.4×)`, `workloadChanged`), or shrank by a third or more below what fixing the earlier run's findings would leave (`the workload shrank`): a fix that read all 400 files, not 30, or 30 files, not 400, is not counted.

A finding of time alone (requests one after another) is judged by its requests: about as many of them (a fifth
either way) is open, whatever their timing. A run that failed (its exit code is not 0) is not compared: `the run failed (exit 3);
not compared`, and nothing kept changes; the next run is compared with the last that did not. Nor is a run that recorded
no request (`nothing reached sonde: the run recorded no request; not compared`, `since.empty` in JSON): what did not reach
sonde is not fixed, and it is never the run before. Runs that end out of
order (several at once) each add their findings; an older run changes nothing a newer one said.

Measured: of the requests the finding named, those the later run no longer sends (by kind and key); for a finding
of time, the fall in its own waiting, at most the fall in the run's. Each program's findings (open, less, fixed with
its saving, back) are kept in its runs folder's index, brought up
to date by `run`, `show` and the app when they read the program's newest run. `run`, `show` (`since` in JSON),
`compare` and `check -f markdown` say a run's findings the same way.

## Cost

A run's cost is its requests and its bytes out at its endpoint's prices: the vendor's list prices (AWS, GCS, OSS,
R2, B2, Wasabi, Tigris; a store of one's own is free) unless `[settings.prices]` says others ([configuration](configuration.md#configtoml)).
Bytes out are billed as egress only when read from outside the store's region, which sonde cannot see: unless the
endpoint says (`egress = true` or `false`), they are said and not counted (`read 12 GiB ≈ 1.08 USD egress if read
from outside the region`). Apart, what the run wrote and left stored costs a month (`left 2.0 GiB stored ≈ 0.05 USD a
month`), and on a bucket that keeps versions (a write's answer has a version id), the versions its overwrites and
deletes made noncurrent, until a lifecycle rule expires them. JSON: `cost` (`requests`, `transfer`, `storage`); the
Time pane's "What did the run cost, and when?" draws the three over time. `check --max-transfer` holds the bytes out.

## Money a month

A run's cost is its requests and charged bytes out ([cost](#cost)). An imported log's is also said a month at the rate of its own
span (a minute at least): `≈ 37.58 USD a month at this rate`. A program's run is said a month when its `sonde.toml`
says how often it runs, `[program] runs-per-month = 200` (for the program `runs` names, or every program run in its
folder): `≈ 4.10 USD a month at 200 runs a month`. Each finding's saving and the findings' total are then a month too.
Without either, the cost is the run's own. JSON: `month` (`times`, `from`: `log` or `runs`, `cost`), each finding's
`savesCost` (times `month.times`: a month).
