# 18.0 plan

**Fewer things, each finished.** sonde does not lack features; it lacks finished paths through them. 18.0 closes
three loops, answers "why slow, why costly", and removes what a user never needs. Nothing new gets its own place.

Evidence: a newcomer walkthrough of 17.0.0 (six journeys, CLI and every app screen) and an audit of every
command, screen, chart, setting and finding.

## 17.0.1 first: what 17.0 broke

| | What | Fix |
|---|---|---|
| High | **A second run fails.** `no-delete` refuses overwriting what the program's *previous* run wrote, so a plain re-run crashes. | no-delete lets a program overwrite what earlier recordings of the same program wrote (kept per program). |
| High | **A trial cannot be tried again.** The same rule refuses the second trial's clean run. | Same fix, and keys under `{trial}` are the trial's own. |
| Medium | `compare` marks a finding fixed when it only dropped under its threshold. | Fixed means gone at any threshold; otherwise "less". |
| Medium | `check --baseline auto` with no baseline passes in silence in CI. | Say so in every format; `-f github` warns. |
| Low | `skew` is a permanent "broke" for boto3, which does not retry it, and fails `check`. | Offered only when the SDK retries it, or asked for by name. |

## 18.0

### 1. The fix loop (M)

Finding → change → run again → **fixed, and what it saved**.

- A finding keeps one identity across a program's runs: *open*, *fixed* (with the measured saving against the one
  predicted), *back*.
- `sonde run` ends with the change since the program's previous run; `compare` with no arguments takes the last two.
- Each finding prints its saving, the fix for the detected SDK and its line of code in the terminal (`--callers`
  on by default when the language supports it).
- In the app, Run shows the top findings after a run; one "if you act on these" figure, not two.

### 2. The trial loop (M)

Broke → advice → change → **try only what broke → survived**.

- "Try again what broke": a fresh clean run and only the scenarios that broke or degraded.
- Verdicts are kept per program across runs: a scenario reads "broke on 12 Nov, survived since".
- "Try under faults" from a session's header too.
- A refusal by the recording policy is its own outcome, never "broke".

### 3. The CI loop (M)

A PR comment a reviewer can act on, with one link to the report.

- A GitHub Action (`hxddh/sonde-action`): record, check against the base branch's baseline (fetched, not local),
  update one comment, attach the HTML report.
- The comment: the verdict, only what changed (`HeadObject 0 → 20`), new findings first, one link. No repeated rows.
- The shared page shows the comparison, not one run.

### 4. Why slow, why costly (M)

- `sonde show --by prefix|size|caller|sdk` and one **Group by** control on the operations table: the same table,
  split, with one sentence ("80% of the slow requests are under `wal/`, over 8 MiB").
- A request's inspector shows that key's history. No new place.

### 5. Less (M)

| | 17.0 | 18.0 |
|---|---|---|
| CLI commands | 19 | 14 |
| Places and panes a newcomer meets | 22 | 14 |
| App settings | 15 | 8 |
| Finding kinds shown | 16 | 11 |

- **Fold**: `diff` and `replay` into `compare` (`--replay`); `watch` into `proxy`; `export` into `share --otlp`;
  `du` and `mpu` into `bucket`. Each old command prints its new form for one version.
- **One safety model**: the recording policy only. Read-only mode, the toolbar lock and the "Writes enabled" badge
  go; sonde's own destructive acts keep their dry run and typed confirmation.
- **The Time pane keeps every chart, all shown by default.** 18.0 refines them (below), it does not cut them.
- **Findings folded**: `obs.retried` (retrying is right), `obs.slow`, `obs.races`, `obs.shared`, `obs.injected`.
- **Store tools in one place**: buckets, capacity, uploads, versions, layouts and compatibility under Buckets,
  reached from a finding; not five entries.
- **Files**: no `.summary.json` and trial files beside each session (one index per folder).

### 6. One set of words (S)

One word each, in the CLI, the app, the docs and Chinese: *waiting* (not busy), *run* (a recording of a program;
*session* is its file), *operation* (not episode), *request* (not record), *finding* (not observation or warning).

### 7. Refined, and kept so (M)

- **The screenshot tour gates a merge**: a changed picture needs a look and an approval (today it may fail).
- **One interaction grammar** in DESIGN.md: select → inspect → act; one inspector; every action by keyboard.
- `show` is short and fixed: head line with the SDK, findings first, one table.
- Known flaws: the gap in middle-ellipsised names, the dark prefix heat map, bars of zero.
- The Time pane, every chart kept: one reading order (traffic, latency, errors, where), each chart's title a
  question it answers, and every chart tied to the selection and to Group by.

## Not in 18.0

A production sidecar or `/metrics`, a query language, drag zoom, new table formats, SigV4a, HTTP/2, S3 Express,
ten million requests in 8 GB, an MCP server. Each widens the product before its core paths are finished.

## Done when

- A newcomer installs, records, fixes their first finding and sees it confirmed **in under ten minutes**.
- A broke scenario can be fixed and shown survived without reading a doc.
- A PR shows one comment with what changed and one link.
- The counts in "Less" are met, and every screen has an approved picture (wide, least, dark, Chinese).

## Risks

Removing commands and read-only mode breaks scripts and habits: each removal is in the changelog, and the old
command says its replacement for one version (contract: a major, D-138). The fix loop needs a finding's identity
to be stable across runs: it is tested on recorded fixtures before the UI is built on it.
