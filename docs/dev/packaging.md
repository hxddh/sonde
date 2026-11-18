# Packaging and release

| Artifact | Built by | How |
|---|---|---|
| `sonde-linux-x86_64.tar.gz`, `sonde-linux-aarch64.tar.gz` | `build.yml` on Ubuntu 22.04 (x86_64 and arm64 runners) | `nsh build App.tsx --release`, then `cargo build --release -p sonde-cli --features tui`: CLI and terminal UI in one binary, bundle and web viewer embedded. Dynamically linked: needs glibc 2.35 or newer. There is no static aarch64 build: `install.sh` refuses aarch64 on musl or an older glibc, saying so. |
| `sonde-linux-x86_64-musl.tar.gz` | `build.yml` on Ubuntu 22.04, `--target x86_64-unknown-linux-musl` (musl-tools) | The same, statically linked: runs on Alpine, distroless, old distributions (checked in `alpine`). `install.sh` takes it on musl systems, with `SONDE_STATIC=1`, or when the glibc build cannot start (glibc older than 2.35). |
| `sonde-macos-aarch64.tar.gz`, `sonde-macos-x86_64.tar.gz` | `build.yml` on macOS 26 (arm64) and macOS 26 Intel | The same. |
| `sonde-windows-x86_64.zip` | `build.yml` on Windows (`x86_64-pc-windows-msvc`) | The same, `sonde.exe` (signed) in a `.zip`. |
| `sonde-macos.dmg` | `build.yml` on macOS 26 | `nsh package app/App.tsx --target macos` (`sonde.app`, AppKit runner with sonde's crate), then signed and notarized (below), and `scripts/macos-dmg.sh`. |
| `sonde-<version>-macos-<arch>.tar.gz` | `build.yml` on macOS 26 | The signed, stapled `.app` archived for the updater. |
| `sonde-<version>-linux-x86_64.tar.gz` | `build.yml` on Ubuntu 24.04 | `nsh package app/App.tsx --target linux`: the desktop app on GTK 4 and WebKitGTK 6 (needs `libgtk-4-1`, `libwebkitgtk-6.0-4`, GStreamer). |
| `sonde-<version>-windows-x86_64.zip` | `build.yml` on Windows | `nsh package app/App.tsx --target windows`, its `sonde.exe` signed, zipped again. |
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

On quit (`Application.on("quit")`) a recorded program and a running trial are ended and the run is written as
finished; the system waits 200 ms.

## Signing and notarization

Ready for when the keys exist. `ci.yml` passes `build.yml` these repository secrets by name, no others:

| Platform | Secrets | Steps |
|---|---|---|
| macOS | `MACOS_CERTIFICATE_P12` (base64 Developer ID Application `.p12`), `MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGNING_IDENTITY`; `APPLE_API_KEY` (an App Store Connect API key's `.p8`, as text or base64), `APPLE_API_KEY_ID`, `APPLE_API_ISSUER` | `scripts/macos-sign.sh` signs the `.app` inside out (nested code first, hardened runtime, timestamp); `scripts/macos-notarize.sh` notarizes it (notarytool) and staples it; the updater's archive and the `.dmg` are made from the stapled app; the `.dmg` is signed, notarized and stapled. |
| Windows | `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_SIGNING_PROFILE` | Azure Trusted Signing (`.github/actions/sign-windows`) signs `sonde.exe` and the app's `sonde.exe` before they are zipped, and checks each signature. |

A step whose secrets are absent is skipped and says so: a notice, or a warning on `main`, whose build a release
publishes. Each release's notes say what it skipped.

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
| Pull request, Dependabot's too (a newer push cancels it) | lint, unit (Linux, macOS arm64, Windows), budgets, supply, action, integration, record, ui, Linux tour | Linux x86_64 (glibc, musl, desktop) |
| Push to `main`, nightly, Actions › CI › Run workflow | the same, and the macOS and Windows tours; nightly and by hand, `timed` and `ui-perf` | everything a release ships |

A tour's variants run side by side (`shots`); `shots-check` holds each platform's pictures to its approved ones
([the tour](../../CONTRIBUTING.md#the-tour-and-its-pictures)). Rust's caches are a few
shared keys (`debug`, `release`, `lint`, `app`, …), each saved by one job of `main`'s runs, so a pull request's
never push out the ones every run reads. The Native Shell commit is read from `Cargo.toml` (`.github/actions/nsh`);
the viewer's web runtime is kept by that commit and Rust's version. `fuzz.yml` runs the fuzzers weekly,
`action-live.yml` the Action after each release and daily.

## Releasing

1. Update `CHANGELOG.md` (`## [x.y.z] - date`; a `### 中文` part at the section's end gives the Chinese notes the app
   shows in Chinese, the English otherwise: `scripts/changelog-section.sh x.y.z zh`) and the versions: `Cargo.toml` `workspace.package.version`,
   `app/nsh.json` `version` and `build`, `viewer/nsh.json` `version`; a new major also the action's `@vN` in
   `README.md`, `docs/ci.md` and `action.yml`.
2. Merge to `main`, then push tag `vx.y.z` on that commit, or run Actions › Release › Run workflow with `x.y.z` (it
   tags the chosen commit). `release.yml` builds nothing: it waits (up to two hours) for CI's run of that commit on
   `main` (its push, the nightly or one by hand; never a pull request's), refuses it unless it is green — every
   build, test and tour — and publishes that run's files (`scripts/release-wait.sh`). The release has the changelog
   section (`latest.json` carries its Chinese part as `notes.zh.md`), what it skipped for want of secrets (signing, notarization, Windows' Authenticode, the signed package),
   `SHA256SUMS` and a `.sha256` per file; each file is attested.
3. `action-tag.yml` then moves the major tag (`v24`) to the release, so `uses: hxddh/sonde@v24` installs it (a
   pre-release does not move it; to repair one: Actions › Action tag › Run workflow with `x.y.z`), and
   `action-live.yml` runs the Action against it.

| | |
|---|---|
| `install.sh` | Downloads the machine's binary, checks its `.sha256`, installs to `~/.local/bin`. `SONDE_INSTALL_DIR`, `SONDE_VERSION` (`24.0.1`; `24`: the newest 24.x; `24.0`: the newest 24.0.x), `SONDE_STATIC=1`; the static build, too, when the glibc one cannot start. Installs nothing that does not start (`sonde --version`). Runs `gh attestation verify` when `gh` is signed in (`SONDE_VERIFY=1` requires it, `0` skips). |
| `scripts/shots.sh` | CI's `shots` job, [the tour](../../CONTRIBUTING.md#the-tour-and-its-pictures). The screens of measured figures show committed runs (`app/tests/fixtures/runs`). |

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
