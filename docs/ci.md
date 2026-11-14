# CI

A pull request gets one comment: the verdict, what changed against the base branch's run, and one link to the
report. The budget and the baseline are `sonde check`'s ([cli](cli.md#sonde-check)).

## The GitHub Action

```yaml
on:
  push: { branches: [main] }
  pull_request:
permissions: { contents: read, actions: read, pull-requests: write }
jobs:
  s3:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: hxddh/sonde@v19
        with:
          run: python etl.py --day 2
          run-args: --url ${{ vars.S3_URL }}
```

Runs on Linux and macOS runners. Each push to `main` keeps its run as an artifact; a pull request is checked against
the newest one. This repository tests it against stand-ins on each pull request, and live (`action-live.yml`) after
each release and daily, against a real baseline and a real comment.

No store, no secrets: `run-args: --demo` runs the program against the built-in store, so the check holds its
requests and findings (its times are not meaningful: leave `max-waiting` and `max-p95` out).

| Step | Does |
|---|---|
| Install | The release binary (`install.sh`): SHA-256 checked; build provenance checked with `gh attestation verify` (the token signs gh in). `version: path` uses the `sonde` on `PATH`. At a branch or a commit (`@main`, `@<sha>`), the newest release of the major its `Cargo.toml` says, never `latest`; none released yet fails the step. |
| Record | `sonde run -o FILE RUN-ARGS [--name NAME] -- RUN` in `working-directory`. The program's exit status fails the job, after the steps below. |
| Baseline | `auto`: of the newest 5 completed push runs of the same workflow on the base branch (a push, a schedule: the runs of its own event on its own branch, not itself), the first that kept the artifact `artifact` (`gh run list`, `gh run download`). None: a warning, and `check --baseline auto` says `no baseline yet` in every format. |
| Check | `sonde check -f github`: annotations on the run, findings on the code lines that sent them. |
| Report | `sonde share --html RUN --baseline BASE` (`--redact` with `redact: true`): the comparison first, then the run. Kept as the artifact `<artifact>-report`. |
| Keep | The session, as the artifact `<artifact>`: the next runs' baseline (the repository's artifact retention, 90 days by default). |
| Comment | `sonde check -f markdown --report-url URL`: the job's summary, and one pull-request comment found by `<!-- sonde: <artifact> -->` and edited in place. |
| Verdict | Fails when the program failed, else when the check failed (`fail: false` keeps the job green). |

| Input | Default | |
|---|---|---|
| `run` | (required) | The command, as typed. |
| `run-args` | | `sonde run` flags: `--url`, `--region`, `--path-style`, `--policy allow`, `--demo`. |
| `name` | | The program's name (else `[program] name` in `sonde.toml`, else the command). |
| `check-args` | | `sonde check` flags: `--tolerance 5`, `--no-new-warnings`, `--max-requests N`. |
| `config` | found upward | `sonde.toml`. |
| `baseline` | `auto` | `none`, a session path, or an https URL. |
| `artifact` | `sonde` | One per job (a matrix names each). |
| `comment` | `true` | Needs `pull-requests: write`. A fork's pull request has a read-only token: no comment (a warning), the summary still has it. |
| `redact` | `false` | Bucket and key names in the report become tokens. |
| `fail` | `true` | |
| `version` | the action's tag; at a branch or commit, its own major | `19.0.0`, `19`, `latest`, `path`. Tag `@v19` installs the newest 19.x; `@v19.0.0` that release. `latest` is refused at a branch or commit. |
| `working-directory` | `.` | |
| `github-token` | `github.token` | Needs `actions: read` for the baseline. |

Outputs: `passed` (`true`/`false`), `session`, `report-url`, `comment-url`.

## The comment

```markdown
### sonde: past its baseline ❌

**Against the baseline**

- **requests 52 → 72 (+38%)**, past the 57 it may use
- cost 0.00021 USD → 0.00022 USD (+5%)

| kind       | requests | waiting       |
| ---------- | -------: | ------------: |
| HeadObject |  0 → 20  | 0 ms → 118 ms |

**New findings**

- 20 objects were checked with HEAD and then read with GET: the GET alone answers both — `etl.py:19 load`

[The report](https://github.com/o/r/actions/runs/1/artifacts/2)
```

| Part | When |
|---|---|
| Heading | Always: the verdict, as every format closes. |
| `> ❌ The program failed` | `--program-exit` not 0 (the Action passes `sonde run`'s): its status, and the first request the recording policy refused, if one was. |
| Since | Past the baseline over several runs (needs the runs, so local history; not in a fresh runner). |
| `> ⚠️ no baseline yet` | A baseline asked for and none found. |
| Failed rows | Budget rows that failed; never a row that passed. Without a baseline, its failed rows too. |
| Against the baseline | Requests, cost and errors that changed, waiting that moved past 10% and 50 ms; a figure past the baseline in bold with its limit. Kinds of request that changed. |
| New findings, Gone | Against the baseline, as `sonde compare` says them (one under its threshold in the baseline is not new, one under it in the run is not gone); new first, with the code line that sent them. Not when either run failed. |
| Warnings now | `no-new-warnings`: a finding the baseline had as a note, a warning in this run. |
| Trials | Those that broke or degraded; the survived counted. |
| Findings | Without a baseline: the run's first five findings, folded. |
| The report | With `--report-url`. |

## Elsewhere

```sh
sonde run -o run.sonde -- python etl.py
sonde check run.sonde --baseline main.sonde -f markdown --report-url "$URL" > comment.md
sonde share --html run.sonde --baseline main.sonde -o report.html
```

Keep `run.sonde` from the main branch (an artifact, a bucket) and give it, or its https URL, as `--baseline`.
