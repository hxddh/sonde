# 18.0 reviewed, 19.0 proposed

A review of 18.0.0 after its release, by four readers who did not write it: one hunted bugs with the released
binary against a local S3 (moto); one walked 18.0's own promises as a newcomer, timed, and measured a million
requests; one rendered all 190 pictures of the app (wide, least, dark, Chinese) and judged them against
`app/DESIGN.md`; one looked at the project's health and at what users would miss. The plan it is held to is
[plan-18.0.md](plan-18.0.md).

**Confirmed** means run and seen; **read** means from the code only. The app was rendered on Linux only.

## In short

The three loops close. A HEAD-before-GET removed reads `✓ fixed … saved 20 requests (predicted 20)`; a program
that does not retry goes broke → fixed → `--trial --again` → "broke on 13 Nov, survived since" without a doc; a PR
gets one comment with what changed and one link. `show --by caller` says `82% of the cost is from etl.py:7`.
622 tests pass.

What did not hold:

1. **A newcomer cannot reach it.** The README's install line is a 404 (there is no `main` branch), the Homebrew
   tap and the macOS app's signing were skipped by the release (no secrets), and `sonde run` needs an S3 of
   your own: the built-in store `demo` uses is refused to `run`. The ten-minute promise holds only with moto.
2. **The fix loop says "fixed" when nothing was fixed** — for a run that crashed, and on every run of a program
   whose keys carry a run id or a timestamp, which is most lakehouse and ETL programs.
3. **The CI loop is unpinned**: `uses: hxddh/sonde@v18` installs the *latest* sonde, so 19.0 would run under v18's
   scripts.
4. **Run, the first screen, overlaps itself** at the least window and hides its next steps at every width.

And one beyond 18.0: 18 majors in 7 days, each carrying what the one before removed. **19.0 drops compatibility
with every earlier version** — no aliases, no shims, no migrations, no old names — and is the last break for a
while: from 19.0 the contract (D-138) holds, and a break is announced one version ahead.

## What 18.0 got wrong

### Reaching it

| | Where | What |
|---|---|---|
| High | README, `install.sh`, `docs/ci.md` | `raw.githubusercontent.com/hxddh/sonde/main/install.sh` is 404: the default branch is not `main`. Confirmed. |
| High | `release.yml` | No `HOMEBREW_TAP_TOKEN`: the tap is not updated. No Developer ID: the `.dmg` is refused by Gatekeeper on another Mac. No package key: installed apps cannot update themselves. Confirmed (release log). These need the owner's secrets. |
| High | `sonde run` | No store without an account. `sonde run -e demo` → `no endpoint "demo"`, though `demo` and `tui --demo` have one built in. Confirmed. |
| High | `action/sonde-action.sh:30` | `GITHUB_ACTION_REF=v18` does not match `v*.*.*`, so the Action installs `latest`. Confirmed. |

### The fix loop

| | Where | What |
|---|---|---|
| High | `fixloop.rs` `since_last` | **A run that failed is compared like any other.** A program that crashes after one request: `✓ fixed 20 objects were checked with HEAD … saved 20 requests`; the next normal run: `! back`. Confirmed. |
| High | `profile.rs` `identity` | **A finding's identity holds the literal key.** A program reading `jobs/<ms>/state.json` twelve times, run twice unchanged: `requests 13 (no change)`, then `✓ fixed …/1794558464595/state.json … saved 11` and `! new …/1794558465084/…`; `findings.json` grows each run. Also `obs.listed`, `obs.prefixrate`, `mpu`. Confirmed. |
| Medium | `compare.rs`, `Program::mark_back` | "Back" is judged by the program's latest state, not the runs compared: `show R2` of good → bad → good says `! back`. Confirmed. |
| Medium | `fixloop.rs` `changes`, `measured` | A time-only finding ("requests one after another") reads **less** when noise drops it under its threshold, beside `waiting 183 ms → 206 ms (+13%)`, and is credited with any drop in waiting. Confirmed. |
| Medium | `run` against `show` | `run` and `compare` say `↓ less 30 requests ran one after another`; `show` of that run lists no finding and its JSON `findings` is `[]`; the CI comment then calls it **New**. Confirmed. |
| Medium | `check --init` | It names the program, and the name orphans what was kept under the command: `--set-baseline` sets it for the old name, `--trial --again` finds no trial. Confirmed. |
| Low | `profile.rs` `identity` | `obs.prefixrate.reads` and `.writes` share an identity; fixing one is never told. "Less" is not kept, so open → less → over again reads new. Read. |

### Other analysis

| | Where | What |
|---|---|---|
| High | `session.rs` `fresh_path`, `reqlog::private::create` | **Two runs started in the same second write one file.** The name is checked, then opened with create + truncate. 12 parallel `sonde run`s: 5 files, 7 runs exit 70 (`not a sonde session`). Since 16.0. Confirmed. |
| Medium | `split.rs` `sentence` | A group with 96% of the requests and of the slow ones: "the slow requests are spread: no one group holds half of them". The 1.5× share test cannot pass when one group holds nearly all. Confirmed. |
| Medium | CI comment | When the program fails (a write refused by `no-delete`, exit 1) the comment says `errors 0 → 1`, `requests 41 → 2 (-95%)`, and not that the program failed or why. Confirmed. |
| Medium | `check` `[check] expect` | Matches a substring of the finding's text, which the contract says is not stable; an `expect` naming a removed kind (`obs.retried`) or a typo matches nothing, silently. Confirmed. |
| Low | `fixloop::write`, `trial::record`, `Written::keep` | `findings.json`, `verdicts.json`, `written.json` are read-modify-write without a lock; the app and the CLI can lose each other's entries, and a lost `written.json` entry refuses a legitimate overwrite. Read. |
| Low | `run --again` | `--name ' etl '` is trimmed when stored, not when looked up. With `slowdown` and `slowdown:3` both broke, one is run, silently. Confirmed / read. |
| Low | `du`, `mpu`, `diff`, `replay` | The new form they print drops the arguments (`diff -f json A B` → `sonde compare A B`). Confirmed. |
| Low | `show`, 1M requests | 13.9 s and 1.76 GiB, over the 1.5 GiB budget (17.0: 1.11 GiB; these records carry more headers). `compare` of two: 22.4 s, 1.43 GiB. Confirmed. |
| Low | words | JSON keeps `busyMs` and `episodes`; `compare` prints `1000000` where `show` prints `1,000,000`; `run --help` says no-delete refuses "a write over an object the recording did not write", but earlier recordings' are allowed. Confirmed. |

### The interface

| | Where (picture) | What |
|---|---|---|
| High | `min-02b-ran`, `Run.tsx:163,281` | **Run's ended band overlaps itself**: `⋯` lies on "not found" and Run again covers the run's length. `room()` goes negative. Confirmed. |
| High | `wide-02b-ran` | **Run hides the loop's next step at every width**: "See the whole run" and "Try under faults…" are behind `⋯` even at 1280; Run and Run again stand side by side. Confirmed. |
| High | `wide-03i`, `wide-03l`, `wide-03e` | **"Operation" means two things**: the Operations table's "manifest 1 written", and the Time pane's "Which operations were slow?" over GetObject/PutObject. Group by says "kind of request". Chinese follows (操作 / 请求类型). Confirmed. |
| Medium | `wide-03e`, `min-03e` | Group by's table: mixed header case, no `~` for few requests, the cost unit in every row, clipped at the least window, rows not selectable — and a second, different copy of the Time pane's table. Confirmed. |
| Medium | `wide-03f` → `wide-03g` | Two selections of time: a stretch picked on the Operations strip is not the Time pane's. The Time charts do not follow Group by (known). Confirmed. |
| Medium | `wide-02g`, `wide-02h` | A trial's result repeats one advice paragraph three or four times; Runs draws the verdicts unlike the dialog (badge side, red reasons, a subtitle about a chart not there); the history says "since Nov 13" for runs minutes apart. Confirmed. |
| Medium | `wide-04g` | Size: "See Multipart." (the section is Incomplete uploads), "GLACIER holds 1.7%" beside a chart's 85%, 16 objects in the bars against 8 in the tile, a clipped label. Confirmed. |
| Medium | `min-02h`, `wide-03h`, `wide-02j`, `wide-03b`, `wide-03l` | The header's Requests tile breaks "Request / s"; tiles in 3+2 rows; 18 requests drawn as a 500/s square wave; the session's sentence held to the title's column; Flows' labels on its ribbons. Confirmed. |
| Low | several | "1 requests"; middle cuts that eat a note; "Multipart scan" and "Capacity scan" for sections named otherwise; a layout's "Run" button; a raw UTC timestamp; 1020 B against 1.0 KiB; one failure orange in one chart and red in the next. Confirmed. |
| Low | the tour | No picture shows "Since the run before" (the tour's runs are identical), so 18.0's headline is not gated; the Time charts are shot half-drawn. Confirmed. |

### Health

| | What |
|---|---|
| High | **CI costs hours.** Five macOS jobs a run; push and pull_request each run on a PR branch (83 runs on 12 Nov); a run takes 1.5–2.5 h, a release 4 h waiting for macOS runners, and rebuilds what CI built. The macOS and Windows screenshot jobs gate nothing (only Linux pictures are approved). |
| Medium | The Action is tested against a stand-in `gh` with comments off; nothing fetches a real baseline or writes a real comment. |
| Low | Stale docs: the SQLite request log (`security.md`, `architecture.md`), "refused while read-only" (`recording.md`), "busy prefixes" (`cli.md`), `--callers` in the Action's example though it is now the default. |
| Low | Security is sound (loopback Host check, throwaway key, credentials stay in Rust). The docs should say the policy is a net, not a sandbox: the program runs with the user's rights, and a repository's `sonde.toml` can widen it to `allow` with a warning only. |

## What holds

- The fix loop on stable keys; savings measured against the prediction; first runs open; trial sessions never
  counted as runs.
- `--trial --again` re-runs only broke, degraded and refused scenarios; `no-delete` lets a program write over its
  own earlier writes and refuses others' with a clear message.
- `check --baseline auto` without a baseline warns in every format; removed flags say their replacement.
- Times at a million requests match `performance.md`. The proxy's security model holds.
- The app is calm and consistent: two columns, one inspector, Buckets' sections in one row, every Time chart
  shown with a question for its title, dark and Chinese complete.

## 19.0

**Trust what it says, reach it, and carry nothing from before.** No new place, command or format. Every item closes
something 18.0 opened or removes something kept for an older version.

### 1. Reach it (S)

- The default branch becomes `main`. The Action installs the newest release of its own major (`@v19` → newest
  19.x); tested.
- `sonde run --demo -- <command>`: the built-in store `demo` already has, for your own program — a first run with
  no account, and a CI gate on request counts and findings without secrets (times there say so).
- The owner adds `HOMEBREW_TAP_TOKEN`, a Developer ID and the package key; until then the release says plainly
  what it skipped, on the release page as well as in its log.

### 2. A fix loop to trust (M)

- A failed run is not compared: "the run failed (exit 3); not compared".
- Identity normalises what changes between runs — run ids, UUIDs, timestamps, dates, `vN.metadata.json` — tested
  on fixtures whose keys change every run, before anything else. What 18.x kept of findings starts afresh.
- "Back" and "fixed" are judged from the runs compared, not the latest state; a time-only finding is judged by
  its requests, not by noise; "less" is kept; `.reads` and `.writes` are two findings.
- `run`, `show`, `compare` and the CI comment list the same findings for the same run.
- `check --init` carries the baseline and trial history to the new name; `expect` takes identities only, and one
  it does not know is an error.

### 3. Correct under concurrency and at the edges (S)

- Sessions are created with `create_new` and the next name on a clash; the three state files are written under
  a lock.
- `split` says where the slow requests are when one group holds nearly all; `--again` trims names and runs every
  broke strength; the old commands print their new form with the arguments.
- The CI comment says when the program failed, and why.
- `show` of a million requests back under its budget.

### 4. Run and one word for a kind of request (M)

No new screen; the time pane keeps every chart.

- **Run's ended band in two rows**: the figures, then "See the whole run", "Try under faults…" and the primary
  Run again, labelled at every width; one Run.
- **"Kind of request" everywhere** for GetObject and the like, "operation" only for what the program did. Group by
  → Kind of request is the Time pane's table, one component: same columns and `~`, the cost unit in the header,
  rows that select, columns dropped by rank when narrow.
- **One selection of time**, shared by the Operations strip and the Time pane, shown as one chip; the Time charts
  follow Group by (18.0's gap).
- A trial says each piece of advice once; Runs draws verdicts as the dialog does; the history reads "broke in
  3 of 3 runs, since 08:33".
- Size, tiles, headers and the Low rows above. The tour gains a run that differs from the one before, and waits
  for charts to draw.

### 5. CI in minutes (S)

One run per change; macOS screenshots, `macos-15-intel` and the app's macOS build on release and nightly only;
Dependabot runs lint and unit; the release publishes CI's build of its commit instead of building again. A test
of the Action against a real baseline and comment, scheduled.

### 6. Without the past (S)

What is kept only for an older sonde goes, and the code with it:

- The folded commands `watch`, `export`, `replay`, `diff`, `mpu`, `du` and the refusals of removed flags
  (`--log-out`, `--read-only`, `--no-delete`, `share --json`); `--callers` (the default).
- Reading the files kept beside sessions before 18.0 (`index.rs`), shared pages and verdicts of before 15.0 and
  16.0 (`signals.ts`, `history.tsx`).
- Old words in JSON: `busyMs` → `waitingMs`, `episodes` → `operations`, every schema in `docs/schemas` renewed.
- The changelog's 0.x–18.x sections move to `docs/history/`; the contract restarts at 19.0.

### 7. Words and docs (S)

`compare` writes numbers as `show` does; `run --help` says what no-delete allows; stale docs fixed;
`security.md` says the policy is a net, not a sandbox.

## Not in 19.0

A production sidecar or `/metrics` (`import` reads access logs and CloudTrail), SigV4a, HTTP/2, S3 Express, a query
language (`show --by` and `-f json` cover it), an MCP server, new table formats, drag zoom. The session format
stays at version 1: its `operation` field names an S3 API call, which is what the file is for.

## Done when

- `curl … | sh`, `sonde run --demo -- python etl.py`, a finding fixed and confirmed, in ten minutes, with no
  account.
- A program whose keys change every run, run twice unchanged, reads "no change".
- `@v19` installs 19.x; a PR's CI finishes in under 30 minutes.
- `sonde --help` and `grep -r` of the code find no command, flag, file or word kept for an older version.
- Every picture in the tour, the "since the run before" one included, approved; nothing overlaps at the least
  window.
