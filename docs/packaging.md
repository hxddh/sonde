# Packaging and release

| Artifact | Built by | How |
|---|---|---|
| `sonde-linux-x86_64.tar.gz`, `sonde-linux-aarch64.tar.gz` | `build.yml` on Ubuntu 22.04 (x86_64 and arm64 runners) | `nsh build App.tsx --release` (the bundle), then `cargo build --release -p sonde-cli --features tui` — one binary: CLI and terminal UI, the bundle embedded. |
| `sonde-macos-aarch64.tar.gz`, `sonde-macos-x86_64.tar.gz` | `build.yml` on macOS 14 and 13 | The same. |
| `sonde-macos.dmg` | `build.yml` on macOS 14 | `nsh package app/App.tsx --target macos` (`sonde.app`, AppKit runner with sonde's crate), `scripts/macos-sign.sh` (Developer ID, hardened runtime), `scripts/macos-dmg.sh`, `scripts/macos-notarize.sh` (notarytool, stapled). |
| `SHA256SUMS` | `release.yml` | Every artifact's SHA-256; each also ships as `<artifact>.sha256`. |

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

`install.sh` downloads the binary for the machine, checks its `.sha256` and installs it in `~/.local/bin`
(`SONDE_INSTALL_DIR`, `SONDE_VERSION`).
