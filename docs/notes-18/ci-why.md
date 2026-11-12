# 18.0 notes: the CI loop, why slow and costly

## Changelog lines

### Added

- **A GitHub Action** (`uses: hxddh/sonde@v18`, [ci.md](../ci.md)): installs the release binary (checksum and build
  provenance), records the command, fetches the base branch's run (the workflow's artifact), checks against it,
  keeps one pull-request comment up to date and attaches the HTML report.
- **`sonde show --by operation|prefix|size|caller|sdk|status`**: the operations table split (requests, errors, p50,
  p95, p99, waiting share, cost) and one sentence on where the slow and costly requests are (`80% of the slow
  requests (over 120 ms) are under b/wal/, over 8 MiB.`). JSON: `show-by.schema.json`.
- **`sonde share --html BEFORE AFTER`**, or `--baseline FILE|auto|URL`: the page is the comparison first, then the
  run with its new findings first.
- `sonde check --report-url URL` (`-f markdown`): the comment's one link.
- The SDK (from the user agent) in `show`'s and `run`'s head line.
- The app's bridge: `Sessions.split(path, by)` (Group by) and `Sessions.keyHistory(path, bucket, key)` (a key's
  requests, every operation, for the inspector).

### Changed

- **`check -f markdown`, the pull-request comment**: the verdict, the rows that failed (never those that passed),
  against a baseline only what changed (figures, kinds of request `HeadObject 0 → 20`, findings new then gone),
  the trials that did not survive, one link.
- `check --baseline auto` with no baseline is said in every format: a quoted line in markdown, a `::warning` with
  `-f github`, the verdict `nothing checked (no baseline yet)` in JSON and github too (it printed plain text).
- `check`'s since line: `past the baseline since a1b2c3d, 2 runs ago; within at 9f8e7d6`; this run first past:
  `the run before, at 9f8e7d6, was within it`; nothing when no earlier run is known (was "since pr2.sonde (this
  run)").
- `check --init` prints the Action as its CI step.

## Decision rows

| D-1xx | The CI baseline | data-directory cache; a URL the user keeps; the workflow's own artifact | **The newest push run's artifact on the base branch** (`gh run download`): no storage to set up, no secret; a missing one is a warning and `no baseline yet`, never a silent pass. |
| D-1xx | The pull-request comment | every row; only failures; what changed | **Verdict, failed rows, what changed against the baseline (figures, kinds, findings new first), one link**: a reviewer reads why, not a table that passed. One comment, found by its marker, edited in place. |
| D-1xx | Where slow requests concentrate | a chart per dimension; a table per dimension; one table and a sentence | **One table split by a chosen dimension, one sentence**: slow is the slowest tenth (p90, 3 at least); a group is named when it holds half of them and 1.5× its share of requests, narrowed by a second dimension holding 60%; the cost likewise. |
| D-1xx | The action's place | a repository of its own (`hxddh/sonde-action`); this repository's root | **This repository's root (`hxddh/sonde@v18`)**: versioned with the binary it installs, tested by this CI. |

## UI needs (app, not done here)

- Group by on the operations table: a control with operation, prefix, size, caller, SDK, status; the split table
  from `api.sessions.split(path, by)` (`T.Split`), same columns as the table; the sentence from `phrases` (zh in
  `phrases.ts`) above it. Charts on Time follow the chosen dimension where they can.
- The request inspector: the key's history from `api.sessions.keyHistory(path, bucket, key)` (`T.KeyRequest[]`),
  each row selecting its operation (`operationIndex`).
- The header shows `view.sdk` ("from boto3 1.44").
- The check preview (Programs) uses `check_markdown`, which now leaves passed rows out.
