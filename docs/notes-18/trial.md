# Stream: trial

## Changelog lines

- Fixed: under `no-delete`, a second `sonde run` of a program that rewrites its own outputs was refused. A program may write over what its earlier recordings wrote (`written.json` beside its runs, per program and endpoint).
- Fixed: a trial could not be run again: its clean run was refused. Keys under a `{trial}` place are the trial's own.
- Changed: `skew` is offered only when every signed request's SDK retries it (AWS SDKs for Java, JavaScript, from the user agent); never to boto3/botocore unless named with `--fault skew`.
- Added: `sonde run --trial --again`: a fresh clean run, then only the scenarios whose last verdict was broke, degraded or refused. A trial that left any prints the command.
- Added: verdicts are kept per program (`verdicts.json`), not removed when a run is recorded again; each verdict says `broke on 12 Nov, survived since` in `run`, `check` (all formats) and `show`. JSON: `history` on each trial; `show -f json` has `trials`.
- Added: a new verdict, `refused`: the recording policy refused what the program did under the scenario. Never `broke`; `check` marks it `warn`.
- App bridge: `Sessions.trialAgain(path, policy, cols, rows, onEvent)`; verdicts carry `history`; `Trial.standing` may be `refused`.

## Decisions

| Topic | Options | Choice and why |
|---|---|---|
| What a program may write over under no-delete | allow everything a program's earlier runs wrote; allow under prefixes it created; default to `allow` | **The objects it wrote, kept per program and endpoint** in `written.json` beside its runs (newest 100,000). Exact keys, so another program's data and anything there before stay refused. |
| `{trial}` keys | per-invocation id in `{trial}`; trials default to `allow`; the place is the trial's own | **Keys under a `{trial}` place are the trial's own**, read from the command's words (up to the last `{trial}`, past `=` and `scheme://`, ending at a word's end). Keys stay stable for matching against the clean run; works for trials recorded before 18.0. |
| When `skew` is offered | always when signed; never by default; by SDK | **When every signed request's SDK retries it** (Java, JavaScript). For botocore it was a permanent "broke" no code could change. Asked by name, it runs. |
| Name of "try again what broke" | `--retry-failed`; `--failed`; `--again` | **`--again`, with `--trial`**: reads as "trial again"; `--retry-failed` would be read as SDK retries. |
| What `--again` tries | broke only; broke and degraded; and refused | **Broke, degraded and refused**: refused was never judged, so trying it again after the fix is the loop. |
| Where verdict history lives | in each `<run>.trials.json`; one file per folder | **`verdicts.json` per folder, keyed by program then scenario as asked**: survives re-recording a run to the same path and deleting runs; the newest 50 per scenario. |
| What `check` judges | the run's own trial; the program's latest per scenario | **The run's own trial, each row with its history**: a check is of one run; `--again` re-tries everything that did not survive, so the run carries what matters. |
| A refusal by the policy during a trial | broke; untested; its own verdict | **`refused`, its own**, when the trial had more policy refusals than its clean run and would be degraded or broke. Not counted as tried; `check` warns, never fails. Policy refusals carry `x-sonde-policy`. |
| History words | a phrase key for the app; the app formats it | **CLI says it in English; JSON carries `history`** (dates are the app's to format in the reader's language). |

## UI needs (interface stream)

- Runs pane / trial dialog: a "Try again what did not survive" action calling `Native.sessions.trialAgain` (events: the clean run's `{started}`/`{data}`, then the trial's). Shown when any verdict is broke, degraded or refused.
- Each verdict row: "broke on 12 Nov, survived since" from `trial.history` (format dates in the reader's language).
- The `refused` standing: label `trial.refused` exists (en/zh); `STANDING.refused = "warn"` in `ui/history.tsx`; advice `trial.advice.policy` and reason `trial.why.policy` have both languages.
- The trial dialog's read-only refusal and "{trial}" warning are unchanged.
