# For coding agents

sonde tells an agent what a program does to S3, where in its code, and what to change; then whether the change
worked. Two commands, no prompts, JSON on stdout.

## The loop

```sh
sonde run -o before.sonde -- python3 etl.py      # record (add --demo for the built-in store, no account)
sonde show before.sonde -f json                  # read: findings, each with where and what to change
# set findings[].fixes[].setting, or edit the code at findings[].callers[].file:line
sonde run -o after.sonde -- python3 etl.py       # the same command, recorded again (`verify` gives it)
sonde show after.sonde -f json                   # since.findings[].state: did it work?
```

`run` leaves stdout to the program and prints its summary on stderr; read `show -f json`, not text (text may change
in any release). `-o` keeps a handle on the run (`sonde runs` and the app list it too); without it, `show` takes the newest run of the program here, and refuses (64) when there is none ([which run](cli.md#which-run)).

## The fields

From `show -f json` ([schema](schemas/show.schema.json)):

| Field | What |
|---|---|
| `findings[]` | What to change, those that save the most first. Empty: nothing found. |
| `findings[].kind` | One of the [kinds](cli.md#findings): `obs.hot` (a key read again and again), `obs.small` (small objects one PUT each), `obs.deletes` (deletes one at a time), `obs.rmw` (written back with no `If-Match`), `obs.polled` (a missing key polled), `obs.throttled`, … `trial` (a fault scenario it did not survive: `sonde run --trial`). `obs.rmw` and `trial` save nothing: a lost update to prevent, a fault to survive. |
| `findings[].callers[]` | Where its requests were sent from: `file` (relative to `run.cwd`), `line`, `function`, `requests`, and `code`, the line itself. Python's boto3, the AWS SDK for JavaScript v3 (CommonJS or ES modules, any Node.js it runs on) and for Java 2 name callers. |
| `findings[].fixes[].setting` | When the requests are an engine's or a tool's (SlateDB, DuckDB, rclone, s5cmd, Trino, Hadoop s3a): the one setting to change, `engine`, `key`, `value`, and `where` (`file:line` under `run.cwd`) when the program's folder sets it now. Set `key` to `value` there; no code to adapt. [Which](cli.md#engine-settings). |
| `findings[].fixes[]` | Otherwise the change for the run's SDK (`sdk`, `code`), with placeholders (`b` the bucket, `k` the key): adapt it to the code at `callers[]`. None when an engine sent the requests and none of its settings acts on it. |
| `findings[].savesRequests`, `savesMs`, `savesCost` | What acting on it saves, predicted. |
| `findings[].identity`, `expected` | The same finding across runs; `expected`: why the project keeps it on purpose (`[check] expect` in `sonde.toml` names it by identity): leave it alone, do not "fix" it. |
| `verify` | The command that records the program again, in `run.cwd`. |
| `since.findings[].state` | Against the program's run before, by counts: `fixed`, `less`, `more`, `new`, `open` (a `trial` finding: against the program's trial before). A fixed or less one has `measured` (the run's wall time `ms` and `cost`, fallen) and `savedText`; `withinNoise` when that is under how much the program's runs vary (`since.change.noiseMs`): not told from noise, run it again. |
| `since.change.commandChanged`, `workloadChanged` | The command changed, or the workload grew or shrank ([cli.md](cli.md)): what moved is not counted as a fix. Check the job did the same work. |
| `trials[]` | Verdicts under faults (`sonde run --trial -- …`): `standing` is `survived`, `degraded`, `broke`, `untested` or `refused` ([trials](trials.md)). What did not survive is a finding (`findings[]`, kind `trial`): why, the request and `callers[]`, and `fixes[]` for the SDK. |

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
