# Working on sonde

For coding agents changing sonde itself. Using sonde from an agent: [docs/agents.md](docs/agents.md). The build,
the checks CI runs and the tour: [CONTRIBUTING.md](CONTRIBUTING.md#test).

- **Build into a folder of your own.** `export CARGO_TARGET_DIR=<your folder>/target CARGO_INCREMENTAL=0`, so two
  sessions never share or overwrite a build. The workspace is large: watch the disk.
- **Never run `nsh test` bare.** It runs the app's tests against your own sonde data. Use `bash scripts/ui-tests.sh`.
- **Read the lint scripts' last line** (`scripts/lint-*.py`, `scripts/check-locales.py`): `ok` or `failed`.
- **JSON is a contract.** A `-f json` change updates its schema in `docs/schemas/` and `crates/sonde-cli/tests/schemas.rs`
  in the same commit; a break waits for a major and is said in `docs/contract.md`.
- **Every word in two languages.** What the app says, and the phrases Rust says, are in `app/locales` in English and
  Chinese.
- **The tour** (`scripts/shots.sh`) opens real windows: give it a display of your own (or Xvfb), never the user's.
- **Where decisions live.** `docs/dev/decisions.md` (why, one row each), `docs/dev/architecture.md` (how it fits),
  `CHANGELOG.md` (what changed). Docs say a thing once, briefly, and link to it elsewhere.
