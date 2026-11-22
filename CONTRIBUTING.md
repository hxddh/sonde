# Contributing

sonde is a Rust workspace (`crates/`) and an app written in TSX on [Native Shell](https://github.com/longbridge/nsh)
(`app/`, its bridge `crates/sonde-nsh`; the share viewer `viewer/`). How it is put together:
[architecture](docs/dev/architecture.md); why: [decisions](docs/dev/decisions.md).

## Build

Rust 1.97 or newer, and the `nsh` CLI at the commit `Cargo.toml` pins:

```sh
cargo install --locked --git https://github.com/longbridge/nsh --rev <nsh-core's rev in Cargo.toml> nsh-cli
cargo build -p sonde-cli                 # the CLI
(cd app && nsh run App.tsx)              # the app
```

On Linux the app needs GTK 4 and WebKitGTK 6 (`libgtk-4-dev libwebkitgtk-6.0-dev` and GStreamer's `-dev` packages).

## Test

What CI runs (`ci.yml`'s lint, unit, ui, action and integration jobs; the rest: [packaging](docs/dev/packaging.md#ci)):

```sh
# lint
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo shear                              # no unused dependencies
python3 scripts/fixtures.py --check      # run fixtures in the current format
(cd app && nsh check)
python3 scripts/lint-layout.py && python3 scripts/lint-kit.py && python3 scripts/check-locales.py && python3 scripts/lint-charts.py
python3 scripts/check-readme-images.py   # docs/images are the tour's approved linux pictures (needs Pillow); @vN is Cargo.toml's major
(cd app && nsh build App.tsx 2>&1 | tee build.log; ! grep -E "^warn" build.log)   # no warnings
# unit
cargo test --workspace --locked
python3 scripts/gen-probe-docs.py --check   # docs/probes.md is what the catalog makes
# ui
bash scripts/ui-tests.sh                 # the app's tests, headless, with data folders of their own
# action (then the action itself, with this commit's sonde)
shellcheck action/*.sh install.sh scripts/release-wait*.sh scripts/changelog-section*.sh && actionlint
bash action/test.sh                      # the GitHub Action's scripts
bash scripts/release-wait.test.sh
bash scripts/changelog-section.test.sh   # the release notes, English and Chinese
# integration
scripts/integration.sh ceph rustfs       # against Ceph RGW and RustFS in Docker
```

Run the app's tests through `scripts/ui-tests.sh`, not `nsh test` alone: it keeps them off your own sonde data.
Every word sonde says is in `app/locales` in English and Chinese: the app's, and the phrases Rust says (`phrase.*`,
`core.*`; Rust reads the English there). When the run format changes, set `RENAMES` in `scripts/fixtures.py`, run
it, and commit the fixtures.

## The tour and its pictures

`sh scripts/shots.sh` opens the app on a real window (Linux under Xvfb when there is no display) and takes every
place, wide and at its least size, light and dark, English and Chinese, into `app/shots`. CI runs it on every pull
request (Linux) and on `main` (macOS and Windows too), and fails a screen that changed and is not approved (a
platform with no approved pictures is reported, not held).

To approve a change: download the run's `sonde-shots-<platform>` artifact, then

```sh
python3 scripts/shots-diff.py approve DIR app/shots.approved.json <platform>
```

and commit `app/shots.approved.json`. The README's pictures (`docs/images`) are then taken from the approved tour
(`SONDE_README=1 sh scripts/shots.sh` copies them; `scripts/check-readme-images.py` fails until they are).

## Moving the Native Shell pin

The pin is in one place, `Cargo.toml`'s `nsh-*` entries; CI reads it from there (`.github/actions/nsh`).

1. Set every `rev = "…"` of `longbridge/nsh` in `Cargo.toml` to the new commit, then `cargo update -p nsh-core`.
2. Install that commit's `nsh` (above), and run the checks and the tour; approve the pictures that changed.
3. Say why in [decisions](docs/dev/decisions.md) (D-207 is the pin) and what changed in
   [FINDINGS](docs/dev/FINDINGS.md) and [nsh-issues](docs/dev/nsh-issues.md).

## Releasing

Update `CHANGELOG.md` and the versions, merge to `main`, then push the tag `vX.Y.Z` (or run Actions › Release).
The release publishes what CI's green run of that commit built: [packaging](docs/dev/packaging.md#releasing) has
the steps, every file that holds the version, the artifacts and the signing secrets.
