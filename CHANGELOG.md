# Changelog

All notable changes are listed here. The project follows [Semantic Versioning](https://semver.org).

## [21.0.0] - 2026-11-14

21.0 removes and renames without notice: a break comes in the major that makes it ([contract](docs/contract.md)).

### Changed

- The app, refined on Native Shell's main: every size with its line, Chinese at 12 pt, motion and press feedback,
  colour only on marks, no boxes inside cards, actions on hover, one header with one primary, dialogs of two widths
  that keep their size, a narrower sidebar at the least window.
- Charts: labels off the data, titles across the card, shorter legends of drawn marks only, failure charts fitted to
  their failures, chart icons on hover. Every chart is kept.
- A program's key leaves out only dates, times, UUIDs and ids; when its command changed, the fix loop says so and
  counts no saving.
- `show`, `compare`, `share`, `check` and `import s3://` choose the run and the budget by one rule; a `sonde.toml`
  applies to its own program only.
- `--again` implies `--trial`.

### Removed

- `operations[].kind` is `operations[].shape` (`kind` is only the S3 call); `check --program-exit`.

### Fixed

- A key rewritten between reads, or not found, gets no cache advice; a prefix written between listings is not
  "listed again"; a chain breaks at a request that needs the one before.
- A request the policy refused is named, with its policy and `--policy allow`; SIGTERM is said; `sessions` shows each
  run's exit.
- `-o` refuses an existing file in `import` and `share` too, unless `--force`; nothing to import is bad data (65).
- `compare` of two imports no longer contradicts itself or shows unknown waiting; `check --init` warns on a run of
  no requests.
- Bucket tools: costs under a cent, no empty uploads table, lifecycle rules as readable lines.
- Session files are named by their program; deleting a session no longer breaks the list; Open the demo is offered
  whenever the demo is not in use.
- The tour fails what it cannot reach and is approved on Linux, macOS and Windows; CI runs the tour's variants side
  by side and caches Native Shell's build.

## [20.0.0] - 2026-11-14

20.0 breaks without a 19.x announcing it, by the owner's decision ([contract](docs/contract.md)). It reads 19.0's
sessions. From here a break is announced one version ahead.

### Changed

- One name for each thing: an S3 call is a `kind` (`requests[].kind`, `summary.kinds`, `show --by kind`); sessions
  are written in format 2.
- A program is its name, else its command with what changes between runs (dates, numbers, ids) left out, so a
  job run with a new `--date` keeps its baseline.
- Findings keep their identity across runs when keys hold `key=value` parts, numbers or ids (identity rules v2);
  less, more and open are judged by a finding's count.
- `sonde demo` is `sonde run --demo` (without a command, the tour); `--allow-writes` is `--policy allow`; `prune`
  confirms with `--confirm COUNT`; every flag has help.
- `record-policy` and `name-callers` move to `sonde.toml` (`[record]`); `config.toml` keeps what sonde keeps.
- The contract promises figures, ids and exit codes, not display text.
- The interface: every Time chart says in a sentence what it shows, computed from what it draws; a clean run never
  shows "Broke"; sonde's own answers are one colour; the header folds at the least window.
- Releases publish CI's green build of the same commit; main builds and tours Linux, macOS and Windows; the Action
  at a branch or commit installs its own major, and is tried live after each release.

### Fixed

- `check` fails a run whose program failed, and never checks another program's session.
- `compare` reads more requests as "more", not "less"; after a program's first run it says there is none before.
- `run`: an unknown option or no endpoint exits 64; `-o` refuses an existing file without `--force`; a command
  that cannot start leaves no session; SIGTERM exits 143 and says so.
- Replay never creates or deletes a bucket, and refuses one that is not there before sending anything.
- Control characters in keys and program text are escaped wherever sonde prints.
- `import s3://`: an upload completed in a later pass is not called unfinished; a cut CloudTrail document is bad
  data (65).
- The proxy forwards `.` and `..` key segments as sent; no-delete lets a program remove what it made in the same
  recording.
- Trials say "did not break" only when every scenario was judged; `--again` keeps the trial's flags.
- The tour shows committed sessions where it draws measured figures, so no picture is exempt; budgets gate on
  peak memory.

## [19.0.0] - 2026-11-13

19.0 drops compatibility with every earlier version: no aliases, shims, migrations or old names. The contract
([docs/contract.md](docs/contract.md)) starts here. Releases 0.1 to 18.0: [docs/history/changelog-0-18.md](docs/history/changelog-0-18.md).

### Added

- `sonde run --demo -- <command>`: the program against the built-in store `sonde demo` uses; no account or secrets
  (its times are not meaningful, and it says so).
- `install.sh` takes a major (`SONDE_VERSION=19`: the newest 19.x); the Action's `@v19` installs the newest 19.x.

### Changed

- The fix loop: a finding keeps one identity across runs of a program, however its keys or paths are numbered;
  a failed run is never compared; "fixed" says in which run, and "back" when it returned; "less" is judged by
  requests, not time.
- Run: one band in two rows; "Since the run before" says what was fixed, back or less, with the requests saved
  against those predicted. Trials say each piece of advice once and their history as a sentence.
- One failure is one colour everywhere; tiles never cut a figure; compare keeps five tiles abreast when wide.
- CI runs once per change and reuses its builds at release; macOS and Windows pictures run nightly.

- JSON: `busyMs` is `waitingMs`, `episodes` is `operations` (`episodesTotal` is `operationsTotal`), everywhere;
  every schema in `docs/schemas` follows.
- `compare` writes counts with separators, as `show` does (`1,000,000`).
- A replay always writes under a prefix of its own and removes what it wrote.
- A release made without the signing keys says what it skipped on its page, not only in its log.

### Removed

- The commands folded in 18.0: `watch`, `export`, `replay`, `diff`, `mpu`, `du`.
- `--log-out`, `--read-only`, `--no-delete`, `share --json` (now unknown flags); `run --callers` (the default);
  `compare --replay --in-place` and `--keep`.
- Reading what versions before 18.0 kept: the files beside sessions (`.summary.json`, `.trials.json`, `.exit`),
  `config.toml`'s `read-only`, `reqlog-*` and `stale-upload-days`; the app's reading of pages and verdicts made
  before 15.0 and 16.0.
- Homebrew: the formula, the cask and the tap's update.

### Fixed

- Two runs at once no longer write over one session, index or findings file (each is held while written).
- `show` of a large session takes no more memory than 17.0 did.
- `replay` compares keys and bytes, not only counts; `check --program-exit` fails a job its program failed.
- Exit codes: a closed stdout, `layout --kind` of nothing, `prune` dry run (4); `endpoints add --replace`.
- Two programs of one name are told apart in Programs.

- `uses: hxddh/sonde@v18` found no tag: each release now moves its major tag (`action-tag.yml`).
