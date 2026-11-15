# sonde

**See what your software does to S3.** sonde records a program's object-storage requests through a local endpoint
and reads them as operations — an Iceberg commit, a SlateDB manifest write, a multipart upload — with what stands
out, what it cost, and whether the program survives when the store misbehaves.

![Run: a program in its terminal, its S3 requests as operations beside it](docs/images/run.png)

## What it does

| | |
|---|---|
| **Record** | `sonde run -- <command>` records every S3 request the program makes. |
| **Read** | Requests become operations: multipart uploads, retries, listings, the commits of Iceberg, Delta, Hudi, Paimon, SlateDB and Lance. A table per operation (count, errors, p50/p95/p99) and charts over time. |
| **Find and fix** | Hot keys, small files, deletes one at a time, writes back with no `If-Match`, a missing key polled, HEAD before GET, throttling, orphaned files, rename-based commits, requests one after another — each with what fixing it saves, a fix for your SDK and the line that sent it. Run again: each finding reads **fixed** (with the saving measured), **less**, **back** or **new**. |
| **Try under faults** | `--trial` runs the program again while sonde answers 503s, lost answers, cut downloads, clock skew, stale listings… and says whether it **survived**, **degraded** or **broke**, and what to change. `--again` tries only what did not survive. |
| **Gate CI** | `sonde check` fails a job over its budget or its baseline; the GitHub Action (`hxddh/sonde@v24`) keeps one PR comment with what changed and a link to the report ([ci](docs/ci.md)). |
| **Why slow, why costly** | `show --by kind\|prefix\|size\|caller\|sdk` splits the table and says where the slow and costly requests are. |
| **Compare and replay** | `compare` two runs, or a run replayed on another store (`--replay`) to see where answers differ. |
| **Import** | S3 access logs, CloudTrail and HAR files read the same way. |
| **Look at stores** | Browse buckets, find incomplete uploads, run 40 compatibility probes across endpoints, decode table layouts. |

A desktop app (macOS, Linux, Windows), a terminal UI and a CLI, in English and Chinese.

| | |
|---|---|
| ![A run: its figures, what stands out, operations](docs/images/run-view.png) | ![A trial: what the program did under each fault](docs/images/trial.png) |
| ![Time: one timeline, each chart in its frame](docs/images/time.png) | ![Buckets: objects and their details](docs/images/store.png) |

## Where it helps

- **Building on S3**: what a lake engine or SDK really sends, and why it is slow or costly.
- **Before production**: does the program retry, back off and finish its commits when the store fails?
- **In CI**: catch a change that doubles requests or latency before it ships.
- **Choosing a store**: how another store answers your traffic, against AWS's behaviour (CI runs the probes and replays against RustFS and Ceph).
- **After an incident**: read production access logs as operations.

## How it works

```
program ──(AWS_ENDPOINT_URL, a key of sonde's)──▶ sonde on 127.0.0.1 ──(your real credentials)──▶ S3
                                                        │
                                                        └─▶ the run's file (.sonde): one line per request
```

- sonde starts an endpoint on loopback and runs the program with `AWS_ENDPOINT_URL` and a key made for that run.
  It checks the program's signature, signs again with your real credentials and forwards the request. The real
  credentials never reach the program.
- A run keeps each request's method, key, headers, status, timing and size — never object data.
- Analysis is in Rust and reads a run as a stream. In a trial, sonde answers chosen requests itself.
- **Depends on**: nothing at run time (one static binary). The program needs an S3 client that honours
  `AWS_ENDPOINT_URL` (current AWS SDKs and CLI do; Spark, Flink and containers are configured by sonde; others are
  said when nothing reaches sonde, [recording](docs/recording.md#what-the-program-is-given)). The app is built on
  [Native Shell](https://github.com/longbridge/nsh).

More: [architecture](docs/dev/architecture.md), [security](docs/security.md).

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/hxddh/sonde/main/install.sh | sh   # Linux, macOS
```

Or download from [Releases](https://github.com/hxddh/sonde/releases): terminal binaries for Linux, macOS and
Windows; the app as `.dmg`, a Linux tarball (GTK 4) and a Windows zip. Checksums in `SHA256SUMS`.

## Use

```sh
sonde run --demo                        # the tour: a finding, its line of code, its fix, the fix measured
sonde run -- python etl.py              # your program's requests, sent on with your AWS credentials
sonde run -- python etl.py              # after a change: what is fixed, and what it saved
sonde run --trial -- python etl.py 's3://scratch/{trial}/'   # the same under faults
sonde check --init && sonde check       # a budget for CI
```

Another store: `sonde endpoints add ID URL`, or `--url URL` for one run. The built-in one: `sonde run --demo -- …`.

Every command, its formats and exit codes: [docs/cli.md](docs/cli.md).

## Good to know

- **What a recorded program may do**: everything it would do without sonde. A trial records under `no-delete` (no
  deletes but of what it made, no aborts, lifecycle, bucket policy or versioning changes); `--policy` chooses
  ([recording](docs/recording.md#what-a-recorded-program-may-do)). A safety net, not a sandbox: the program runs with
  your rights ([security](docs/security.md#run)).
- **A trial runs the program again**, once per scenario: point it at scratch paths with `{trial}` in a key, or
  every run writes over the last ([trials](docs/trials.md)).
- sonde's own destructive actions (aborting uploads, deleting versions) are each a dry run, then `--yes`, and are
  written to an audit log.
- Not supported yet: S3 Express One Zone (refused with a reason), SigV4a, HTTP/2 to the endpoint.
- Memory grows with a run's requests ([what a million cost](docs/dev/performance.md#large-runs)).
- No telemetry. Credentials stay in sonde; logs and shared pages can be redacted.
- What is stable between versions — run files, JSON outputs, `sonde.toml` and `config.toml` ([configuration](docs/configuration.md)), flags, exit codes — is in
  [docs/contract.md](docs/contract.md).

## Documentation

[CLI](docs/cli.md) · [For coding agents](docs/agents.md) · [CI](docs/ci.md) · [Recording](docs/recording.md) · [Trials](docs/trials.md) · [Configuration](docs/configuration.md) ·
[Credentials](docs/credentials.md) · [Security](docs/security.md) · [Contract](docs/contract.md) ·
[Probes](docs/probes.md) · [Decoders](docs/decoders.md) · [Changelog](CHANGELOG.md)

To build, test or release sonde: [CONTRIBUTING.md](CONTRIBUTING.md), and for contributors [docs/dev](docs/dev)
([run format](docs/dev/run-format.md), [architecture](docs/dev/architecture.md),
[performance](docs/dev/performance.md), [packaging](docs/dev/packaging.md), [decisions](docs/dev/decisions.md)).

Apache-2.0.
