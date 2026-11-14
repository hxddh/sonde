# Contributing

sonde is a Rust workspace (`crates/`) and an app written in TSX on [Native Shell](https://github.com/longbridge/nsh)
(`app/`, its bridge `crates/sonde-nsh`; the share viewer `viewer/`). How it is put together:
[architecture](docs/dev/architecture.md); why: [decisions](docs/dev/decisions.md).

## Build

Rust 1.95 or newer (the app and `sonde tui` need 1.97), and the `nsh` CLI at the commit `Cargo.toml` pins:

```sh
cargo install --locked --git https://github.com/longbridge/nsh --rev <nsh-core's rev in Cargo.toml> nsh-cli
cargo build -p sonde-cli                 # the CLI
(cd app && nsh run App.tsx)              # the app
cargo build -p sonde-cli --features tui  # the CLI with `sonde tui` (needs `cd app && nsh build App.tsx` first)
```

On Linux the app needs GTK 4 and WebKitGTK 6 (`libgtk-4-dev libwebkitgtk-6.0-dev` and GStreamer's `-dev` packages).

## Test

What CI's lint, unit and ui jobs run:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo shear                              # no unused dependencies
python3 scripts/fixtures.py --check      # session fixtures in the current format
(cd app && nsh check)
bash scripts/ui-tests.sh                 # the app's tests, headless, with data folders of their own
python3 scripts/check-locales.py && python3 scripts/lint-kit.py && python3 scripts/lint-layout.py && python3 scripts/lint-charts.py
bash action/test.sh                      # the GitHub Action's scripts
scripts/integration.sh                   # against Ceph RGW and RustFS in Docker (CI's integration job)
```

Run the app's tests through `scripts/ui-tests.sh`, not `nsh test` alone: it keeps them off your own sonde data.
Every word the app shows is in `app/locales` in English and Chinese. When the session format changes, set
`RENAMES` in `scripts/fixtures.py`, run it, and commit the fixtures.

## The tour and its pictures

`sh scripts/shots.sh` opens the app on a real window (Linux under Xvfb when there is no display) and takes every
place, wide and at its least size, light and dark, English and Chinese, into `app/shots`. CI runs it on every pull
request (Linux) and on `main` (macOS and Windows too), and fails a screen that changed and is not approved.

To approve a change: download the run's `sonde-shots-<platform>` artifact, then

```sh
python3 scripts/shots-diff.py approve DIR app/shots.approved.json <platform>
```

and commit `app/shots.approved.json`.

## Moving the Native Shell pin

The pin is in one place, `Cargo.toml`'s `nsh-*` entries; CI reads it from there (`.github/actions/nsh`).

1. Set every `rev = "…"` of `longbridge/nsh` in `Cargo.toml` to the new commit, then `cargo update -p nsh-core`.
2. Install that commit's `nsh` (above), and run the checks and the tour; approve the pictures that changed.
3. Say why in [decisions](docs/dev/decisions.md) (D-207 is the pin) and what changed in
   [FINDINGS](docs/dev/FINDINGS.md) and [nsh-issues](docs/dev/nsh-issues.md).

## Releasing

Update `CHANGELOG.md` and the versions (`Cargo.toml`, `app/nsh.json`, `viewer/nsh.json`), merge to `main`, then
push the tag `vX.Y.Z` (or run Actions › Release). The release publishes what CI's green run of that commit built:
[packaging](docs/dev/packaging.md#releasing) has the steps, the files and the signing secrets.
