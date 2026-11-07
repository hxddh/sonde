# Findings from building sonde

sonde was written against Native Shell as it is (commit `beeb9fc`), without changing it. These are the gaps it
met, each with what sonde does instead. Format follows `examples/hopper/FINDINGS.md`.

## Open

| # | Area | Finding | Workaround in sonde |
|---|---|---|---|
| 1 | Tooling | `nsh check` downloads TypeScript from npm with its own TLS roots: behind a TLS-inspecting proxy it fails (`invalid peer certificate: UnknownIssuer`) and ignores `SSL_CERT_FILE`. | The tarball is fetched with curl into nsh's cache (`~/.cache/nsh/typescript-7.0.2-linux-x64`), or `--tsc` points at a compiler. CI has no proxy. |
| 2 | Terminal | `nsh-tui-host` has no embedding entry beyond `main_with_options`, which parses the process's arguments and loads a bundle from a path; its terminal session (OSC 10/11 colour queries, kitty keyboard, synchronized output) is private. There is no `nsh package --target tui`. | `sonde tui` is its own host on `nsh-platform-tui` (`crates/sonde-cli/src/cmd/tui.rs`): the bundle embedded with `include_str!`, crossterm events through the public `nsh_tui_host::convert`, an OSC 52 clipboard, and colour depth from `COLORTERM` / `TERM` / `COLORFGBG` instead of OSC queries. |
| 3 | Desktop (Linux) | Under X11 (Xvfb with openbox), the GTK runner opened the window at 390×844 — a phone's size — not at `nsh.json`'s `window.defaultSize`. | On its first launch the app sets its bounds itself (1280×820) when the window is narrower than 900; later launches restore the remembered placement. |
| 4 | Terminal | `ratatui`'s `Terminal::clear()` asks the terminal for the cursor position; a terminal that does not answer (some multiplexers, test harnesses) fails the call. Switching between two `AppView`s needs a full repaint. | A plain `CSI 2J` and a fresh `Terminal`, whose first frame paints every Cell. |
| 5 | Components | A component body runs once, so a `switch` on a prop inside a component picks one screen forever; an `effect` that reads a signal inside a helper it calls (here `loading()` in `load()`) re-runs on that signal and disposed the Rust `Listing` it had created. Nothing reports either. | Section views are a plain function called in a binding (`{screen(section())}`); the load runs `untracked`. `Listing::load_more` checks `is_disposed()` so a late call rejects instead of panicking. |
| 6 | Text | `numberOfLines` is a style key and `selectable` a `View` prop; the TSX types say so, but both read like `Text` props from other frameworks. | `line-clamp-N` classes; selection on views where wanted. |
| 7 | Generated workspace | The runner workspace `nsh test` writes (`.nsh/rust`) keeps its own Cargo profiles, so the app workspace's `[profile.dev] debug` does not shrink it: debug runners and hosts took 16 GB. | `CARGO_PROFILE_DEV_DEBUG=0` in `scripts/ui-tests.sh` and CI. |

## By design

- No styled components: every control (buttons, fields, switches, selects, segmented control, badges) is sonde's own
  look on `nsh/base` behavior (`app/src/ui/kit.tsx`), with theme tokens (`app/theme.json`).
- Charts are drawn on `Canvas` in windows; in a terminal, as the TUI guide says, the same data is a table and a
  sentence (`app/src/ui/charts.tsx`).

## Fixed

Nothing yet: none of the above was changed in Native Shell (by the rules of this project).
