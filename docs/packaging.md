# Packaging and release

| Artifact | Built by | How |
|---|---|---|
| `sonde-linux-x86_64.tar.gz`, `sonde-linux-aarch64.tar.gz` | `build.yml` on Ubuntu 22.04 (x86_64 and arm64 runners) | `nsh build App.tsx --release` (the bundle), then `cargo build --release -p sonde-cli --features tui` — one binary: CLI and terminal UI, the bundle embedded. |
| `sonde-linux-x86_64-musl.tar.gz` | `build.yml` on Ubuntu 22.04, `--target x86_64-unknown-linux-musl` (musl-tools) | The same binary, statically linked: no libc needed, it runs on Alpine, distroless and old distributions alike (checked in an `alpine` container). `install.sh` takes it on musl systems, or anywhere with `SONDE_STATIC=1`. |
| `sonde-macos-aarch64.tar.gz`, `sonde-macos-x86_64.tar.gz` | `build.yml` on macOS 15 (arm64) and macOS 15 Intel | The same. |
| `sonde-windows-x86_64.zip` | `build.yml` on Windows (`x86_64-pc-windows-msvc`) | The same, `sonde.exe` in a `.zip`. Unsigned. |
| `sonde-macos.dmg` | `build.yml` on macOS 15 | `nsh package app/App.tsx --target macos` (`sonde.app`, AppKit runner with sonde's crate), `scripts/macos-sign.sh` (Developer ID, hardened runtime), `scripts/macos-dmg.sh`, `scripts/macos-notarize.sh` (notarytool, stapled). |
| `sonde-<version>-macos-<arch>.tar.gz` | `build.yml` on macOS 15 | The `.app` archived for the updater. |
| `sonde-<version>-linux-x86_64.tar.gz` | `build.yml` on Ubuntu 24.04 | `nsh package app/App.tsx --target linux`: the desktop app on GTK 4 and WebKitGTK 6 (needs `libgtk-4-1`, `libwebkitgtk-6.0-4`, GStreamer). |
| `sonde-<version>-windows-x86_64.zip` | `build.yml` on Windows | `nsh package app/App.tsx --target windows`. Unsigned. |
| `latest.json` | `release.yml` | `nsh update-manifest` over the desktop archives: what the app's updater reads (version, notes, URL and SHA-256 per platform). |
| `SHA256SUMS` | `release.yml` | Every artifact's SHA-256; each also ships as `<artifact>.sha256`. |
| Build provenance | `release.yml` (`actions/attest-build-provenance`) | Sigstore attestation of each archive, `.dmg`, `.nshpkg`, `latest.json`; `gh attestation verify <file> --repo hxddh/sonde`. |

## What the package tells the system

`app/nsh.json` declares, and `nsh package` writes into each platform's package:

- **The file type.** `fileTypes` names `.sonde`: the app is in Open With, and a double click on a session or a
  drop on the app's icon opens it (`Application.on("openFiles")`, as a drop on the window does). A gzipped
  `.sonde.gz` is not declared: a type is a file's last extension, and sonde does not claim every `.gz`; such a file
  still opens by a drop on the window or File › Open.
- **The icon, per platform.** `icon` is an object: macOS takes `assets/icon-macos.png` (the artwork 824 of the
  canvas's 1024 pixels, inset on a transparent canvas, as macOS draws icons), Windows and Linux
  `assets/icon-fill.png` (edge to edge). Both are made from the one drawing, `assets/icon.png`, by
  `python3 scripts/icons.py` (Pillow); `--check` fails when either is stale.

When the app quits, however it does (`Application.on("quit")`), the program it is recording and a running trial
are ended and the recording is written as a finished session; the system waits 200 ms for it.

## Signing and notarization

Secrets (repository settings): `MACOS_CERTIFICATE_P12` (base64 of the Developer ID Application `.p12`),
`MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` (an
app-specific password). Without them the steps print a notice and the app stays ad hoc signed (as
`nsh package` signs it): the flow runs end to end either way.

## The terminal binary has no GUI dependency

```text
$ ldd target/release/sonde
	linux-vdso.so.1
	libgcc_s.so.1 => /lib/x86_64-linux-gnu/libgcc_s.so.1
	libm.so.6 => /lib/x86_64-linux-gnu/libm.so.6
	libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6
	/lib64/ld-linux-x86-64.so.2
```

No GTK, GLib, Cairo, Pango, WebKit, X11 or Wayland: the terminal backend draws Cells with ratatui. CI checks this
with `ldd` and runs `sonde --version` in `debian:bookworm-slim`.

## Releasing

1. Update `CHANGELOG.md` (a `## [x.y.z] - date` section) and the versions (`Cargo.toml` `workspace.package.version`,
   `app/nsh.json` `version` and `build`).
2. Tag `vx.y.z` and push, or run the Release workflow by hand (Actions › Release › Run workflow, version
   `x.y.z`), which creates the tag at the chosen commit: `release.yml` builds everything, publishes the GitHub release with the changelog section
   `SHA256SUMS` and one `.sha256` per file (what `install.sh` checks), and (with `HOMEBREW_TAP_TOKEN`) writes `Formula/sonde.rb` and `Casks/sonde-app.rb` into
   `hxddh/homebrew-sonde` from `packaging/homebrew/*.rb.in`.

`scripts/shots.sh` (CI's `shots` job) screenshots every place on a real Linux window under Xvfb — wide and at the
least size, light and dark, English and Chinese — into the `sonde-shots` artifact.

The web viewer (`viewer/`) is built first (`nsh package App.tsx --target web`) and embedded in every terminal
binary for `sonde share`.

`install.sh` downloads the binary for the machine, checks its `.sha256` and installs it in `~/.local/bin`
(`SONDE_INSTALL_DIR`, `SONDE_VERSION`). With `gh` signed in it also runs `gh attestation verify`
(`SONDE_VERIFY=1` requires it, `0` skips it). Signing skipped for want of secrets is a warning in a release.

## Signed packages of the app's code

A release that changes only the TSX can reach installed apps as a signed package, without a new app
(Native Shell's OTA updates, `docs/dev/signed-packages.md` there). It is off until the project has keys:

1. Once, offline: `nsh keygen root` (keep `root.secret` off any server) and add `root.pub`'s key to `app/nsh.json`:
   `"updater": { "github": "hxddh/sonde", "packages": { "roots": ["<root.pub>"] } }`.
2. A release key: `nsh keygen release`, then `nsh certify --root root.secret --key release.pub --app dev.sonde.app`
   for its certificate.
3. Repository secrets: `NSH_PACKAGE_KEY` (the release key's `.secret`) and `NSH_PACKAGE_CERT` (its certificate).

The release workflow then signs `sonde-<version>.nshpkg` (`nsh publish`) and names it in `latest.json`; Settings ›
Updates offers it first ("Update and restart"), and the app runs it from its next start, on trial until it proves
healthy. Without the secrets the step is skipped and updates are whole apps, as before.
