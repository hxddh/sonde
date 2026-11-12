# 17.0 notes: engineering

For merging into CHANGELOG.md and docs/decisions.md.

## Changelog lines

### Added

- **A Windows terminal binary**, `sonde-windows-x86_64.zip`.
- **Build provenance**: a Sigstore attestation of every release file; `install.sh` checks it when `gh` is signed
  in (`SONDE_VERIFY=1` requires it, `0` skips it).
- **Fuzz targets** (`fuzz/`): Iceberg metadata and Avro, SlateDB, lakeFS, Woodpecker, access logs, CloudTrail,
  HAR, S3's XML, a session's lines. Run weekly in CI.
- `sonde-fmt`: the one writer of sizes, counts and durations, tested against the app's table
  (`crates/fmt/cases.json`).

### Changed

- **The CLI writes figures as the app does**: `512 MiB` (no decimal from 100), `1.0 MiB` not `1024.0 KiB`, ties
  rounded up, PiB the largest unit; `2.35 s`, `1h 40m` not `99m 36s`; counts grouped in thousands everywhere.
- CI tests on Windows; nightly runs the ignored timing tests and the app's perf test.
- A large-session budget missed once is run again, with a warning.
- Actions pinned by commit; Dependabot weekly for actions and crates.
- A release without signing secrets says so as a warning.
- quick-xml 0.41 (two advisories against 0.37).

### Fixed

- An Iceberg Avro file of a few bytes could make sonde reserve gigabytes (found by fuzzing).
- `sonde --help` on Windows says `Usage: sonde`, not `sonde.exe`.
- Checkouts use LF on every system (`.gitattributes`).

### CI

- `supply` job: `cargo deny check` (`deny.toml`); the Java callers hook rebuilt from `hooks/java` and compared byte
  for byte with the committed jars.

## Decisions proposed

| Topic | Options | Choice and why |
|---|---|---|
| Java hook jars | build in release, stop committing; keep committed, verify in CI | **Committed, rebuilt and compared in CI.** `build.sh` is already reproducible (byte-identical with today's JDK); committed jars keep `cargo build` free of a JDK. |
| Where the formatter lives | `reqlog`; `sonde-proxy::signals`; own crate | **`sonde-fmt`, no dependencies**: `decoders` and `sonde-core` cannot reach `sonde-proxy`. |
| Whose rounding | Rust's; the app's | **The app's, exactly** (ties up as `toFixed`, unit after rounding). The CLI keeps short duration words (`3m 05s`; app: `3 min 05 s`). |
| Sharing cases with TS | TS imports JSON; cases in the TS test, kept by a Rust test | **Rust reads `cases.json`; the app's test holds the same cases and a Rust test fails if one is missing.** Native Shell's runner was not run here. |
| Windows tests | port all; gate what is Unix | **Port sonde's, gate the shell's**: a trial program's key is an argument (not `env`); `kill`, `sleep`, `sh -c` tests stay Unix. |
| Windows package | `.tar.gz`; `.zip` | **`.zip`**, TUI included. |
| Timed tests | every push; nightly | **Nightly**: minutes in release, a measure rather than a pass/fail of a change. |
| Memory budget headroom | raise the limit; bigger runner; retry once | **Retry once with a warning** (workflow side; the limit is the memory stream's). |
| Action pins | tag; commit + tag comment | **Commit, newest release of the current major.** `dtolnay/rust-toolchain` pinned at branch commits, moved by hand. |
| cargo-deny findings | fail; ignore with reason | **Fix ours** (quick-xml), **ignore with reason** the rest: rusqlite's SQLite (own files only; goes with the rusqlite bump), three in Native Shell's tree. Duplicates warn. |
| Unsigned releases | fail; warn | **Warn**: no keys yet; failing would block every release. |
| Fuzzing | every push; weekly | **Weekly**, corpus cached, crashes kept as artifacts. |

## README wish

The Install line's terminal-binary list should name `sonde-windows-x86_64.zip` (already done on the README
rewrite branch, per the owner); a one-line `gh attestation verify` hint under Install would help.
