# Packaging and release

| Artifact | Built by | How |
|---|---|---|
| `sonde-linux-x86_64.tar.gz`, `sonde-linux-aarch64.tar.gz` | `build.yml` on Ubuntu 22.04 (x86_64 and arm64 runners) | `nsh build App.tsx --release`, then `cargo build --release -p sonde-cli --features tui`: CLI and terminal UI in one binary, bundle and web viewer embedded. |
| `sonde-linux-x86_64-musl.tar.gz` | `build.yml` on Ubuntu 22.04, `--target x86_64-unknown-linux-musl` (musl-tools) | The same, statically linked: runs on Alpine, distroless, old distributions (checked in `alpine`). `install.sh` takes it on musl systems or with `SONDE_STATIC=1`. |
| `sonde-macos-aarch64.tar.gz`, `sonde-macos-x86_64.tar.gz` | `build.yml` on macOS 15 (arm64) and macOS 15 Intel | The same. |
| `sonde-windows-x86_64.zip` | `build.yml` on Windows (`x86_64-pc-windows-msvc`) | The same, `sonde.exe` in a `.zip`. Unsigned. |
| `sonde-macos.dmg` | `build.yml` on macOS 15 | `nsh package app/App.tsx --target macos` (`sonde.app`, AppKit runner with sonde's crate), `scripts/macos-sign.sh` (Developer ID, hardened runtime), `scripts/macos-dmg.sh`, `scripts/macos-notarize.sh` (notarytool, stapled). |
| `sonde-<version>-macos-<arch>.tar.gz` | `build.yml` on macOS 15 | The `.app` archived for the updater. |
| `sonde-<version>-linux-x86_64.tar.gz` | `build.yml` on Ubuntu 24.04 | `nsh package app/App.tsx --target linux`: the desktop app on GTK 4 and WebKitGTK 6 (needs `libgtk-4-1`, `libwebkitgtk-6.0-4`, GStreamer). |
| `sonde-<version>-windows-x86_64.zip` | `build.yml` on Windows | `nsh package app/App.tsx --target windows`. Unsigned. |
| `latest.json` | `release.yml` | `nsh update-manifest`: what the updater reads (version, notes, URL and SHA-256 per platform). |
| `SHA256SUMS` | `release.yml` | Every artifact's SHA-256; each also ships as `<artifact>.sha256`. |
| Build provenance | `release.yml` (`actions/attest-build-provenance`) | Sigstore attestation of each archive, `.dmg`, `.nshpkg`, `latest.json`; `gh attestation verify <file> --repo hxddh/sonde`. |

## What the package declares

From `app/nsh.json`, written into each platform's package by `nsh package`:

- **File type** `.sonde`: Open With, double click and a drop on the icon open it (`Application.on("openFiles")`).
  `.sonde.gz` is not declared (sonde does not claim every `.gz`); it opens by a drop on the window or File › Open.
- **Icon per platform**: macOS `assets/icon-macos.png` (artwork 824 of 1024 px, inset on transparency), Windows and
  Linux `assets/icon-fill.png` (edge to edge). Both made from `assets/icon.png` by `python3 scripts/icons.py`
  (Pillow); `--check` fails when either is stale.

On quit (`Application.on("quit")`) a recorded program and a running trial are ended and the session is written as
finished; the system waits 200 ms.

## Signing and notarization

Repository secrets: `MACOS_CERTIFICATE_P12` (base64 Developer ID Application `.p12`), `MACOS_CERTIFICATE_PASSWORD`,
`MACOS_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` (app-specific). Without them the steps
print a notice, the app stays ad hoc signed and the flow still runs; on `main`, whose build a release publishes, it is
a warning. Windows binaries are not signed (no Authenticode yet). Each release's notes say what it skipped.

## The terminal binary has no GUI dependency

```text
$ ldd target/release/sonde
	linux-vdso.so.1
	libgcc_s.so.1 => /lib/x86_64-linux-gnu/libgcc_s.so.1
	libm.so.6 => /lib/x86_64-linux-gnu/libm.so.6
	libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6
	/lib64/ld-linux-x86-64.so.2
```

No GTK, GLib, Cairo, Pango, WebKit, X11 or Wayland (the terminal backend draws with ratatui). CI checks with `ldd` and
runs `sonde --version` in `debian:bookworm-slim`.

## CI

| Run | `ci.yml` jobs | `build.yml` builds |
|---|---|---|
| Pull request (a newer push cancels it) | lint, unit (Linux, macOS arm64, Windows), supply, action, integration, record, ui, Linux tour | Linux x86_64 (glibc, musl, desktop) |
| Push to `main`, nightly, Actions › CI › Run workflow | the same, and the macOS and Windows tours | everything a release ships |
| Dependabot crate update | lint, unit, supply | nothing |

Every tour is held to its platform's approved pictures (`app/shots.approved.json`; `scripts/shots-diff.py approve`
on the run's `sonde-shots-<platform>` artifact, then commit); a platform with none approved is reported, not held.
`nightly.yml` runs the timed tests, `fuzz.yml` the fuzzers weekly, `action-live.yml` the Action after each release
and daily.

## Releasing

1. Update `CHANGELOG.md` (`## [x.y.z] - date`) and the versions: `Cargo.toml` `workspace.package.version`,
   `app/nsh.json` `version` and `build`.
2. Merge to `main`, then push tag `vx.y.z` on that commit, or run Actions › Release › Run workflow with `x.y.z` (it
   tags the chosen commit). `release.yml` builds nothing: it waits (up to two hours) for CI's run of that commit on
   `main` (its push, the nightly or one by hand; never a pull request's), refuses it unless it is green — every
   build, test and tour — and publishes that run's files (`scripts/release-wait.sh`). The release has the changelog
   section, what it skipped for want of secrets (signing, notarization, the signed package, Windows' Authenticode),
   `SHA256SUMS` and a `.sha256` per file; each file is attested.
3. `action-tag.yml` then moves the major tag (`v20`) to the release, so `uses: hxddh/sonde@v20` installs it (a
   pre-release does not move it; to repair one: Actions › Action tag › Run workflow with `x.y.z`), and
   `action-live.yml` runs the Action against it.

| | |
|---|---|
| `install.sh` | Downloads the machine's binary, checks its `.sha256`, installs to `~/.local/bin`. `SONDE_INSTALL_DIR`, `SONDE_VERSION` (`20.0.0`, or `20`: the newest 20.x), `SONDE_STATIC=1`. Runs `gh attestation verify` when `gh` is signed in (`SONDE_VERIFY=1` requires it, `0` skips). |
| `scripts/shots.sh` | CI's `shots` job: every place on a real window (Linux under Xvfb, macOS, Windows), wide and smallest, light and dark, English and Chinese, into the `sonde-shots-<platform>` artifact. The screens of measured figures show committed sessions (`app/tests/fixtures`). |

## Signed packages of the app's code

A TSX-only release can reach installed apps as a signed package (Native Shell OTA, `docs/dev/signed-packages.md`
there). Off until the project has keys:

1. Once, offline: `nsh keygen root` (keep `root.secret` off servers); add `root.pub` to `app/nsh.json`:
   `"updater": { "github": "hxddh/sonde", "packages": { "roots": ["<root.pub>"] } }`.
2. A release key: `nsh keygen release`, then `nsh certify --root root.secret --key release.pub --app dev.sonde.app`.
3. Secrets `NSH_PACKAGE_KEY` (the release key's `.secret`) and `NSH_PACKAGE_CERT` (its certificate).

The release then signs `sonde-<version>.nshpkg` (`nsh publish`) and names it in `latest.json`; Settings › Updates
offers it first ("Update and restart"), and it runs on trial from the next start until healthy. Without the secrets
updates are whole apps.
