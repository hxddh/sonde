# For coding agents

sonde tells an agent what a program does to S3, where in its code, and what to change; then whether the change
worked. Two commands, no prompts, JSON on stdout.

## The loop

```sh
sonde run -o before.sonde -- python3 etl.py      # record (add --demo for the built-in store, no account)
sonde show before.sonde -f json                  # read: findings, each with where and what to change
# edit the code at findings[].callers[].file:line
sonde run -o after.sonde -- python3 etl.py       # the same command, recorded again (`verify` gives it)
sonde show after.sonde -f json                   # since.findings[].state: did it work?
```

`run` leaves stdout to the program and prints its summary on stderr; read `show -f json`, not text (text may change
in any release). `-o` keeps a handle on the run; without it, `show` takes the newest run of the program here.

## The fields

From `show -f json` ([schema](schemas/show.schema.json)):

| Field | What |
|---|---|
| `findings[]` | What to change, those that save the most first. Empty: nothing found. |
| `findings[].kind` | `obs.hot` (a key read again and again), `obs.listed` (a prefix listed again and again), `obs.headget` (HEAD before GET), `obs.serial` (requests one after another), `obs.small`, `obs.rename`, `obs.markers`, `obs.throttled`, … |
| `findings[].callers[]` | Where its requests were sent from: `file` (relative to `run.cwd`), `line`, `function`, `requests`, and `code`, the line itself. Python's boto3, the AWS SDK for JavaScript v3 and for Java 2 name callers. |
| `findings[].fixes[]` | The change for the run's SDK (`sdk`, `code`), with placeholders (`b` the bucket, `k` the key). Adapt it to the code at `callers[]`. |
| `findings[].savesRequests`, `savesMs`, `savesCost` | What acting on it saves, predicted. |
| `findings[].identity` | The same finding across runs; `[check] expect` in `sonde.toml` names one kept on purpose. |
| `verify` | The command that records the program again, in `run.cwd`. |
| `since.findings[].state` | Against the program's run before: `fixed`, `less`, `more`, `new`, `back`, `open`. A fixed one has `measured` and `savedText`: the saving, measured. |
| `since.change.commandChanged`, `workloadChanged` | The command changed, or the workload grew by half or more: what moved is not counted as a fix. Check the job did the same work. |
| `trials[]` | Verdicts under faults (`sonde run --trial -- …`): `standing` `broke` or `degraded`, with `where.caller` and `advice`. |

`--all` adds every operation and each finding's operations and fixes whole; it is large on a large run.

## Exit codes

| Code | |
|---|---|
| `run` | the program's own exit code; 64 a usage error; 70 when the command cannot start |
| `show` | 0; 64 no such run; 65 not a run |
| `check` | 0 within budget; 1 over budget or baseline, a new warning, a trial broke, or the program failed |
| `run --trial` | 1 when a trial broke |

All of them: [cli.md](cli.md#exit-codes).

In CI, `sonde check -f github` writes each finding as an annotation at its line ([ci.md](ci.md)).
