# 17.0 notes: engineering

The engineering stream of 17.0: Windows in CI and a Windows CLI, the timed tests run, the supply chain checked,
fuzz targets, one way to write a figure, and docs/architecture.md brought up to date. For the merge into
CHANGELOG.md and docs/decisions.md.

## Changelog lines

### Added

- **A Windows terminal binary**: `sonde-windows-x86_64.zip` (`sonde.exe`, the CLI and terminal UI), built and
  released with the others.
- **Build provenance**: every archive, `.dmg`, `.nshpkg` and `latest.json` of a release has a Sigstore attestation
  of the workflow and commit that built it. `install.sh` checks it when the GitHub CLI is installed and signed in;
  `SONDE_VERIFY=1` makes the check required, `SONDE_VERIFY=0` skips it.
- **Fuzz targets** (`fuzz/`) for what sonde reads from others: Iceberg's metadata and Avro, SlateDB's manifests and
  SSTables, lakeFS ranges, Woodpecker blocks, access logs, CloudTrail, HAR, S3's XML and a session's lines. CI runs
  each weekly from the samples the tests read.
- `sonde-fmt`: the one writer of a size, a count and a length of time, tested against the same table as the app's
  `format.ts` (`crates/fmt/cases.json`).

### Changed

- **The CLI writes figures as the app does.** A size has one decimal under 100 of its unit and none from 100
  (`512 MiB`), its unit chosen after rounding (`1.0 MiB`, never `1024.0 KiB`), and a tie rounded up; PiB is the
  largest unit. A length of time has two decimals under ten seconds (`2.35 s`), hours from an hour (`1h 40m`, not
  `99m 36s`), and is written the same in `show`, `check`, `compare`, a trial's verdict, the HTML report and the
  findings. Counts are grouped in thousands in `watch`, `import`, the operations list and the report's figures.
- CI runs `cargo test` on Windows, and nightly the timed tests that are ignored in a debug run (the signals of a
  million requests, a large session through the app's bridge) and the app's performance test.
- A budget of a large session missed once in CI is run again before it fails the build, with a warning on the run.
- Every action in the workflows is pinned by its commit, its tag in a comment; Dependabot moves the actions and the
  crates weekly.
- A release without the signing secrets (Developer ID, notarization, the signed package) says so as a warning on
  its run, not a notice.
- quick-xml 0.41: two advisories against 0.37 (a start tag's attributes checked in quadratic time, namespaces
  without a bound) no longer apply to the XML sonde reads from a store.

### Fixed

- `sonde --help` on Windows reads `Usage: sonde`, not `sonde.exe`.
- The trial tests run on Windows: a program's key is its argument, not `env`'s; the tests that need a shell, `kill`
  or `curl` are Unix's.
- Files are checked out with LF on every system (`.gitattributes`), so fixtures read the same on Windows.

### CI

- `supply` job: `cargo deny check` (licences, advisories, sources, bans; `deny.toml`), and the Java callers hook
  rebuilt from `hooks/java` and compared byte for byte with the committed jars.

## Decisions proposed

| Topic | Options | Choice and why |
|---|---|---|
| The Java callers hook's jars | build them in the release and stop committing them; keep them committed and rebuild them in CI | **Committed, rebuilt in CI and compared byte for byte.** `hooks/java/build.sh` already writes jars without timestamps, and a JDK of today rebuilds the committed ones to the same bytes: the check is exact. Committed jars keep `cargo build` working without a JDK, for every contributor and every crate that embeds them; CI proves they are what the sources make, which is what the review asked. |
| Where one formatter lives | in `reqlog` (the lowest crate of the recording side); in `sonde-proxy`'s signals (where "the one ladder" was); a crate of its own | **A crate of its own, `sonde-fmt`, with no dependencies.** `decoders` depends on no other sonde crate and `sonde-core` does not depend on `sonde-proxy`, so only a crate under both reaches every caller. |
| A size, a count and a duration, Rust against TS | Rust's rules (always one decimal, the unit before rounding); the app's rules; a third | **The app's rules, exactly, rounding included** (a tie up, as `toFixed`; the unit after rounding). The app's figures were already the considered ones (D-123's charts and tables use them); the CLI had drifted. A duration keeps the terminal's short words (`3m 05s`, `1h 40m`) for the app's figures (`3 min 05 s`, `1 h 40 min`); the shared table holds both. Counts are grouped in thousands in every language the app writes, so the CLI groups always. |
| Sharing the cases with the app | a JSON fixture the TS test imports; the cases written into the TS test, kept by a Rust test | **Both sides from `crates/fmt/cases.json`**: Rust reads it; the app's test (`desktop.test.tsx`, "one way to write a figure") holds the same cases and a Rust test fails when one is missing from it, so neither side can drift without a test saying so. (Native Shell's test runner was not run here; it imports `../src/format` as the other tests do.) |
| Windows tests | port every test; gate what is Unix by nature | **Port what is sonde's, gate what is the shell's.** A trial's program takes its key as an argument (the test harness reads it as a filter that matches nothing), not through `env`; the tests of signals (`kill`), a grandchild process (`sleep`) and curl (`sh -c`) stay Unix's, as Windows has no such signals and no shell to rely on. |
| A Windows CLI's package | `.tar.gz` like the others; `.zip` | **`.zip`**: what Windows opens without a tool; `sha256` written as the others are (no `*` of binary mode). The TUI feature is built in, as on the other systems. |
| Where the timed tests run | every push; a nightly workflow; the `ui` job | **Nightly** (`.github/workflows/nightly.yml`): the ignored tests are ignored because they take minutes in release, and the performance test is a measure, not a pass or fail of a change. A morning's red run says a speed regressed; the budgets that hold every push stay in `unit`. |
| The memory budget's headroom | raise the limit in the test; run it on a larger runner; run it again once on a miss | **Once again on a miss, with a warning** (the workflow's side; the limit is the memory stream's). A shared runner's memory and speed vary from run to run; a miss that holds twice is a regression, and one that passes the second time is still seen as a warning, so a budget that drifts toward its limit is noticed before it fails. The limit itself should keep a stated share above the measure, set where the measure is made. |
| Pinning actions | by tag; by commit with the tag in a comment | **By commit, the tag in a comment, at the newest release of the major each workflow already used** (no major moved here: Dependabot proposes those one at a time). `dtolnay/rust-toolchain` is pinned at its `stable` and `nightly` branches' commits, which Dependabot does not move: moved by hand. |
| cargo-deny's findings today | fail until each is fixed; ignore with a reason | **Fixed where it is sonde's to fix, ignored with a reason where not.** quick-xml moved to 0.41 (sonde parses a store's XML). rusqlite's bundled SQLite (sonde opens only its own database) waits for the rusqlite bump. Three are in Native Shell's tree (rsa in QuickJS's stdlib, bincode and ttf-parser unmaintained) and go when the pin moves. Duplicate versions warn, not fail: most are Native Shell's. |
| Unsigned releases | fail the release; warn | **Warn in a release, a notice elsewhere.** The project has no Developer ID or package keys yet; a failing release would stop every release until it has. A warning shows on the release's run, where the notice did not. |
| Fuzzing | in every CI run; weekly; by hand | **Weekly, each target a few minutes, its corpus kept in the cache and a crash kept as an artifact.** Fuzzing finds more the longer it runs, and needs nightly; a push should not wait on it. |
