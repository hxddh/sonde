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
      - uses: hxddh/sonde@v32
        with:
          run: python etl.py --day 2
          run-args: --url ${{ vars.S3_URL }}
```

Runs on Linux and macOS runners. Each push to `main` keeps its run as an artifact; a pull request is checked against
the newest one. With none kept (the pull request that adds the workflow, an artifact expired, a run in a format this
sonde no longer reads), the pull request's merge-base is recorded in the same job: `git merge-base origin/BASE HEAD`
(a shallow checkout is fetched whole for it), which for actions/checkout's default checkout (the pull request merged
into its base) is the base branch's newest commit when GitHub made the merge, and for the pull request's own branch
checked out is where it left the base. It is checked out beside this checkout (a `git worktree`), `setup` run there,
then the same command on the same runner, so its times compare; its run is kept apart, in a folder and data folder of
its own, never the run's "run before" nor in the runner's history. The job then takes
about twice as long, and the program runs twice against the store. When it cannot be recorded, a warning says why and
the run is held to its budget alone. This repository tests it against stand-ins on each pull request, and live
(`action-live.yml`) after each release and daily, against a real baseline, a merge-base and a real comment.

No store, no secrets: `run-args: --demo` runs the program against the built-in store, so the check holds its
requests and findings; its times are not meaningful, so no check holds a demo run's waiting.

To start, `sonde check RUN --init` writes a `sonde.toml` from a run, its findings known by count
(`[check.findings]`, but a fault such as a read written back, which fails until fixed): a pull request that adds a
finding, or more of one, fails, with no baseline yet; one that fixes a finding is told to tighten it. It prints this
step with the run's own command and store.

| Step | Does |
|---|---|
| Install | The release binary of `version` (`install.sh`): SHA-256 checked; build provenance checked with `gh attestation verify` (the token signs gh in). `version: path` uses the `sonde` on `PATH`. |
| Record | `sonde run -o FILE RUN-ARGS [--name NAME] -- RUN` in `working-directory`; RUN with shell syntax (`&&`, `|`, `>`, `cd x &&`, `$`) is `bash -c RUN`, all of it recorded; a plain RUN (`docker run …`, `spark-submit …`, a glob expanded) is run itself, so sonde sets it up. The program is named by what runs (`etl` of `python etl.py`). The program's exit status fails the job, after the steps below. |
| Baseline | `auto`: of the newest 6 completed runs of the same workflow, the first that kept the artifact `artifact` in a format this sonde reads (`gh run list`, `gh run download`). A pull request looks at push runs on its base branch; a push or a schedule at the earlier runs of its own event on its own branch. None kept, on a pull request: its merge-base, recorded in the same job (above). None: a warning with why, and no `--baseline` (sonde.toml's `[check] baseline` applies, else the budget alone). |
| Check | `sonde check -f github`: annotations on the run, findings on the code lines that sent them. |
| Report | `sonde share RUN --baseline BASE` in `working-directory` (`--redact` with `redact: true`): one HTML file, the comparison first, then the run. Kept as the artifact `<artifact>-report`. |
| Keep | The run, as the artifact `<artifact>`: the next runs' baseline (the repository's artifact retention, 90 days by default). |
| Comment | `sonde check -f markdown` (the report's link from `SONDE_REPORT_URL`): the job's summary, and one pull-request comment found by `<!-- sonde: <artifact> -->` and edited in place. |
| Verdict | Fails when the program failed, else when the check failed (`fail: false` keeps the job green). |

| Input | Default | |
|---|---|---|
| `run` | (required) | The command, as typed; with shell syntax it is run by bash (`bash -c`). A folder goes in `working-directory`, not `cd x &&`: the annotations' file paths are taken from where the command starts. |
| `run-args` | | `sonde run` flags: `--url`, `--region`, `--path-style`, `--policy no-delete`, `--demo`. Words as a shell splits them (`'…'`, `"…"`, `\`), nothing run or expanded (`$VAR`, `$(…)` are text). |
| `name` | | The program's name (else `[program] name` in `sonde.toml` for the program it `runs`, else what the command runs). |
| `check-args` | | `sonde check` flags: `--tolerance 5`, `--max-requests N`. Words as `run-args`'. |
| `config` | found upward | `sonde.toml`. |
| `baseline` | `auto` | `none` (no baseline, not sonde.toml's either), a run's path, or an https URL. |
| `setup` | | What the merge-base's checkout needs before RUN, run there in `working-directory` (`npm ci`, `pip install -r requirements.txt`): it has none of this checkout's untracked files. Fails: held to the budget, said. |
| `artifact` | `sonde` | One per job (a matrix names each). |
| `comment` | `true` | Needs `pull-requests: write`. A fork's pull request has a read-only token: no comment (a warning), the summary still has it. |
| `redact` | `false` | Bucket and key names in the report become tokens. |
| `fail` | `true` | |
| `version` | the action's tag; at a branch or commit, its own major | `32.0.0`, `32`, `32.0` (the newest 32.0.x), `latest`, `path`. Tag `@v32` installs the newest 32.x; `@v32.0.0` that release. At a branch or a commit (`@main`, `@<sha>`): the newest release of the major its `Cargo.toml` says (none released yet fails the step); `latest` is refused there. |
| `working-directory` | `.` | |
| `github-token` | `github.token` | Needs `actions: read` for the baseline. |

Outputs: `passed` (`true`/`false`), `file` (the run's), `report-url`, `comment-url`.

## The comment

```markdown
### sonde: over its baseline ❌

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
| `> ❌ The program failed` | the run's program exited other than 0 (kept with the run): its status, and the first request the recording policy refused, if one was. |
| Since | Over the baseline for several runs (needs the runs, so local history; not in a fresh runner). |
| `> ⚠️ no baseline yet` | A baseline asked for and none found. |
| Failed rows | Budget rows that failed; never a row that passed. Without a baseline, its failed rows too. |
| Against the baseline | Requests, cost and errors that changed, waiting that moved past 10% and 50 ms; a figure over the baseline in bold with its limit. Kinds of request that changed. |
| New findings, Gone | Against the baseline, as `sonde show --since` says them (one under its threshold in the baseline is not new, one under it in the run is not gone); new first, with the code line that sent them. Not when either run failed. |
| Warnings now | A finding the baseline had as a note, a warning in this run (a baseline always holds the run to no new warning). |
| Trials | Those that broke or degraded; the survived counted. |
| Expected | Findings `[check] expect` keeps on purpose, each with its reason (said here, not as new). |
| Findings | Without a baseline: the run's first five findings, folded. |
| The report | With `SONDE_REPORT_URL` set. |

## Elsewhere

Three commands, in any CI:

```sh
sonde run -o run.sonde -- python etl.py
SONDE_REPORT_URL="$URL" sonde check run.sonde --baseline main.sonde -f markdown > comment.md
sonde share run.sonde --baseline main.sonde -o report.html
```

Keep `run.sonde` from the main branch (an artifact, a bucket) and give it, or its https URL, as `--baseline`.
In GitLab CI:

```yaml
s3:
  image: python:3.14
  script:
    - curl -fsSL https://raw.githubusercontent.com/hxddh/sonde/main/install.sh | sh
    - export PATH="$HOME/.local/bin:$PATH"
    - sonde run -o run.sonde -- python etl.py
    # the newest run kept by the default branch's last green pipeline, if there is one
    - 'curl -fsSL -o main.sonde -H "JOB-TOKEN: $CI_JOB_TOKEN"
        "$CI_API_V4_URL/projects/$CI_PROJECT_ID/jobs/artifacts/$CI_DEFAULT_BRANCH/raw/run.sonde?job=$CI_JOB_NAME"
        || rm -f main.sonde'
    - sonde share run.sonde $([ -f main.sonde ] && echo --baseline main.sonde) -o report.html
    - sonde check run.sonde --max-errors 0 $([ -f main.sonde ] && echo --baseline main.sonde) -f markdown > check.md
  after_script:
    - cat check.md
  artifacts:
    when: always
    paths: [run.sonde, check.md, report.html]
```

Each run is kept as the job's artifact; the default branch's newest is the next runs' baseline (fetched with the job's
token, so a private project works too). `check` fails the job when the run is past its budget or baseline.
`--max-errors 0` is the budget the first pipeline, with no baseline yet, is held to: without a budget or a baseline
`check` has nothing to check and exits 64, failing the job. Jenkins takes the same three commands.
