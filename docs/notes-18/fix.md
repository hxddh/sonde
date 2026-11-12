# 18.0 stream "fix": the fix loop

## Changelog lines

### Added

- **The fix loop.** A finding keeps one identity across a program's runs (its kind and what it is about). `sonde
  run` and `show` end with the change since the program's run before: requests, waiting, cost, and each finding
  fixed (with what it saved, measured, against the predicted), less, new or back. Kept per program in
  `findings.json` beside its runs (as `verdicts.json`).
- `sonde compare` with no runs compares the newest run with the one before it; with one run, that run with its
  program's run before it. `-f json` has `findings` (state, predicted, measured).
- Each finding in the terminal prints its saving, its code line and the fix for the detected SDK.
- Bridge: `Sessions.sinceLast(path)`; `compareRuns` measures what each fixed finding saved.

### Changed

- **`show` is short and fixed** (the same for `run` and `demo`): head line with the SDK, waiting and cost; findings
  first; one table by kind (requests, errors, p95, waiting, cost share). The four signals, first byte, cost and
  prefix tables, and each operation's line, are behind `--all`. `--errors` prints only what failed; `--requests`
  the requests, then the summary.
- **Code lines are named by default** in `sonde run` (Python, Node.js, Java; `--no-callers` or `name-callers =
  false` to stop). `--callers` stays, hidden. The Python hook imports nothing until the program imports botocore
  (start-up 170 ms → 20 ms a process), and names the parts boto3's transfer manager uploads from its threads.
  Hook files are written whole and only when changed.

### Fixed

- `compare` said a finding was fixed when it only fell under its threshold: fixed now means gone at any threshold;
  under it is "less", with what it saved so far.

## Decisions

| Topic | Options | Choice and why |
|---|---|---|
| A finding's identity across runs | run ids; text; kind + what it is about | **Kind + what it is about** (key, prefix, format note; `profile::finding_identity`, unchanged): counts and ids move every run. Tested on five recorded runs of one boto3 program (`tests/fixtures/etl-*.sonde`). |
| Fixed or less | below the threshold is fixed; the pattern gone at any threshold | **Gone at any threshold**: the same detection run with each threshold at its least (`episode::traces`); still found there is "less". |
| Measured saving | the predicted figure again; total request change; per finding, the requests it named that the later run no longer asks | **Per finding, by what its requests asked** (kind, bucket, key, prefix), at most what it named; time findings: the fall in waiting. Other changes in the run do not count toward it. |
| Where states live | in each session; a cache beside it; one file per folder | **`findings.json` beside its runs, per program** (as `baselines.json`, `verdicts.json`, `written.json`; merge: the run's own folder, so `-o` runs keep theirs): open, fixed (since, measured), back. Taken in only from a run newer than the last (showing an old run changes nothing). |
| Who brings it up to date | `run` only; any reader of the newest run | **`run`, `show` and the bridge** when they read a program's newest run: an app run updates it the first time it is shown. |
| `compare` with fewer runs | required pair; newest two of the newest program | **None: the newest run and the one before; one: that run and the one before.** |
| Callers by default | opt-in (D-073); on for `run` | **On** for `run` and the setting: the fix loop needs the line. Safe: lazy Python hook, atomic hook files, header only to sonde. |
| Default `show` | operations then a long summary | **Summary only**, one table; operations, signals and their tables behind `--all`. |
| Words | "before → after", ✓ / ! | **fixed, less, back, new, open** (phrases `fix.*`, en and zh), "since the run before", "kind" for a request's kind. |

## For the app (not done here: TSX)

- After a run (Run screen) and in a session's header: call `Sessions.sinceLast(path)`; show the top findings with
  their state badge (`fix.fixed` …) and `saved` (a phrase) for fixed and less; "N still open".
- Compare view: read `RunChange.findings` (state, `saved`) instead of `resolved`/`appeared` (kept for now: fixed
  only / new and back).
- One "if you act on these" figure: keep the findings bar's `whatIf`, drop the headline's second one.
- Settings: "Name the code" now defaults on; move it to the Run screen as the plan says.
