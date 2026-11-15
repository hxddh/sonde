# Working on sonde

For coding agents changing sonde itself. Using sonde from an agent: [docs/agents.md](docs/agents.md).

- **Build into a folder of your own.** `export CARGO_TARGET_DIR=<your folder>/target CARGO_INCREMENTAL=0`, so two
  sessions never share or overwrite a build. The workspace is large: watch the disk.
- **Never run `nsh test` bare.** It runs the app's tests against your own sonde data. Use `bash scripts/ui-tests.sh`:
  headless, with data folders of its own.
- **The checks** are CONTRIBUTING.md's list: `cargo fmt`, clippy with `-D warnings`, `cargo test --workspace`,
  `cd app && nsh check`, `scripts/ui-tests.sh`, `scripts/check-locales.py`, `scripts/lint-*.py` (read their last
  line: `ok` or `failed`), `action/test.sh`, `scripts/fixtures.py --check`.
- **JSON is a contract.** A `-f json` change updates its schema in `docs/schemas/` and `crates/sonde-cli/tests/schemas.rs`
  in the same commit; a break waits for a major and is said in `docs/contract.md`.
- **Every word in two languages.** What the app says, and the phrases Rust says, are in `app/locales` in English and
  Chinese (`scripts/check-locales.py`).
- **The tour and its pictures.** `sh scripts/shots.sh` opens the app on a real window (Xvfb when there
  is no display; use a display of your own) and takes every place into `app/shots`. CI fails a picture that changed
  and is not approved; approve with `python3 scripts/shots-diff.py approve DIR app/shots.approved.json <platform>`.
- **Where decisions live.** `docs/dev/decisions.md` (why, one row each), `docs/dev/architecture.md` (how it fits),
  `CHANGELOG.md` (what changed). Docs say a thing once, briefly, and link to it elsewhere.
