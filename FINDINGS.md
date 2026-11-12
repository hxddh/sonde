# Findings from building sonde

sonde was written against Native Shell as it is (now pinned at the tag `v0.5.0`, commit `f4d68f7`, see `Cargo.toml`), without changing it. These are the gaps it
met, each with what sonde does instead. Format follows `examples/hopper/FINDINGS.md`.

## Open

| # | Area | Finding | Workaround in sonde |
|---|---|---|---|
| 1 | Tooling | `nsh check` downloads TypeScript from npm with its own TLS roots: behind a TLS-inspecting proxy it fails (`invalid peer certificate: UnknownIssuer`) and ignores `SSL_CERT_FILE`. | The tarball is fetched with curl into nsh's cache (`~/.cache/nsh/typescript-7.0.2-linux-x64`), or `--tsc` points at a compiler. CI has no proxy. |
| 2 | Terminal | `nsh-tui-host` can be embedded (since `ec8430f`): `main_with_args(options, Args)` takes its options from the program rather than the process's arguments, and `request_exit` ends its run. It still loads the app only from a bundle file's path (`Bundle::read`) or a dev server, never from a bundle in memory; its terminal session (OSC 10/11 colour queries, kitty keyboard, synchronized output) is private, and the capabilities that session finds replace the ones passed in `options` (so an embedder cannot turn `hyperlinks` off). There is no `nsh package --target tui`. | `sonde tui` stays its own host on `nsh-platform-tui` (`crates/sonde-cli/src/cmd/tui.rs`) so that the bundle is embedded with `include_str!` and loaded from memory (one executable, no temporary file to write and run): crossterm events through the public `nsh_tui_host::convert`, an OSC 52 clipboard, and colour depth from `COLORTERM` / `TERM` / `COLORFGBG` instead of OSC queries. |
| 4 | Terminal | `ratatui`'s `Terminal::clear()` asks the terminal for the cursor position; a terminal that does not answer (some multiplexers, test harnesses) fails the call. Switching between two `AppView`s needs a full repaint. | A plain `CSI 2J` and a fresh `Terminal`, whose first frame paints every Cell. |
| 5 | Components | A component body runs once, so a `switch` on a prop inside a component picks one screen forever; an `effect` that reads a signal inside a helper it calls (here `loading()` in `load()`) re-runs on that signal and disposed the Rust `Listing` it had created. Nothing reports either. | Section views are a plain function called in a binding (`{screen(section())}`); the load runs `untracked`. `Listing::load_more` checks `is_disposed()` so a late call rejects instead of panicking. |
| 6 | Text | `numberOfLines` is a style key and `selectable` a `View` prop; the TSX types say so, but both read like `Text` props from other frameworks. | `line-clamp-N` classes; selection on views where wanted. |
| 7 | Generated workspace | The runner workspace `nsh test` writes (`.nsh/rust`) keeps its own Cargo profiles, so the app workspace's `[profile.dev] debug` does not shrink it: debug runners and hosts took 16 GB. | `CARGO_PROFILE_DEV_DEBUG=0` in `scripts/ui-tests.sh` and CI. |

| 8 | Components | A `Tree` item is a folder only while it has children, so a folder whose children load when it opens shows no chevron until they do. | Each unloaded folder carries one disabled placeholder child ("Loading…"), replaced by its prefixes once `onExpand` has fetched them. |
| 9 | Desktop (Linux) | The tray needs a D-Bus session bus (StatusNotifierItem); without one (a bare X server, CI) `Tray.set` warns and shows nothing. | Nothing to do: the tray is a convenience; the toolbar's running pill and the toast say the same. |
| 10 | Desktop | `windowSize()` follows the window with the focus: with a second window (Settings) focused, the main window's layout read the second one's width and folded its sidebar. | The main window's shell measures itself (`onLayout`) and its layout reads that (`shellWidth`). |
| 11 | Capabilities | A capability missing from `nsh.json` (`picker`) is caught only when the call is made, as a rejected promise (`ERR_ACCESS_DENIED`); `nsh check` and `nsh build` say nothing. | A test picks a file (`app.onPick`), so a missing grant fails CI rather than a user. |
| 12 | Testing (macOS) | Under `nsh test --target macos`, a press on a button in a hidden title bar's area (the app's toolbar) never reaches it, though nsh's own picking (`window_drag` in `nsh-platform-macos`) leaves views with press handlers clickable; the same presses work on GTK and Win32. Whether a real click is affected is not verified on hardware here. | The screenshot tour reaches the toolbar's commands by their shortcuts (⌘B, ⌘⇧A, ⌘K), which every one of them has. |
| 13 | Components | `ErrorBoundary` around a reactive child expression (`<ErrorBoundary>{screen(area())}</ErrorBoundary>`) kept the views it had drawn when the expression changed: the old place and the new one stacked. | One boundary per place, keyed (`<Each of={[view()]} key>` around it), so a change of place draws a fresh boundary. |
| 14 | Testing | `nsh test`'s `open` has no option for the user's text size (it has reduced motion, colour scheme, locale), so growing rows (`fontScale()`) cannot be checked in a test. | Rows are sized from `fontScale()`; checked by reading, not by a test. |
| 15 | Text | On GTK, text set in a font the app ships (an imported `.woff2`) or with `tabular-nums` is measured narrower than it is drawn: the last glyph of a short label in a table cell (`00:22:25.049` → `00:22:25.04`) is cut off, with no ellipsis. The platform's fonts without font features measure and draw alike. | The app keeps the platform's monospace and leaves tabular digits out of table cells. |
| 16 | Terminal (Windows) | With `portable-pty` on Windows (ConPTY) the reader of a program's output never reaches end of file after the program exits, while the pseudo-console is open; on Unix it ends when the last process holding the terminal does. A run waiting for its output to end never ends. | When the program exits on Windows, sonde drops the pseudo-console and reads what is left for up to two seconds (`Sessions::run`). |
| 18 | Theme | A `font-*` token keeps only the first family of its list (`nsh-base` `theme.rs` `font_family`): `"SF Mono, Menlo, …, monospace"` became `SF Mono`, which Linux and Windows do not have, so `font-mono` text was set in the proportional default (the lifecycle rule broke inside its tags). | The token starts with the generic `ui-monospace`: SF Mono on macOS, Consolas on Windows, fontconfig's monospace on Linux. |
| 19 | Window (macOS) | `setColorScheme` gives its appearance to the main window's root view and to windows opened later, not to the main NSWindow: the window's backdrop (`"backdrop": "sidebar"`, a sibling of the root) keeps the system's. App dark on a light system: a light sidebar under light text. | While the app's scheme is its own (not System), the shell and sidebar are opaque (`windowMaterial()`); the material shows only when it matches. |
| 20 | Testing | A UI test cannot act on a row of a `Plot`: `app.hover` goes to the plot's middle, where a scatter or the lanes may have no mark, and the plot's keys move no hover under `nsh test`. `open`'s `modules` stands in for a module's functions and a class's static methods, not for a method of an instance (`Listing.loadMore`). | A chart's press is tested through what it calls; the tour's pictures show the charts. A listing that fails is a real one: `Sonde.buckets` is stood in for with a bucket the store does not have. |
| 21 | Components | A conditional in an `If`'s `else` (`else={reading() ? <A /> : <B />}`) is read once: the branch does not change with the signal. An unreadable session stayed an outline for ever. | An `If` inside the `else`. |
| 22 | Desktop (Linux) | On GTK, when the text field that has the focus is unfocused by the program (a dialog opens after another window of the app closed, `Keyboard.dismiss()`, `focus()` on a view, the field's page goes), the window gives its *first* text field the focus, selected whole. The toolbar's endpoint (a `Combobox`) stood ringed with its name selected in every picture after Settings had been open; Settings' and Compatibility's first fields likewise. `focus()` on a focusable `View` does not take the focus away from it: it lands there too. Still so at `v0.5.0`. | Each window's first text field is one nobody sees (`FocusRest` in the kit: read-only, a point wide, hidden from screen readers), on Linux only: the keyboard rests there. It is one silent stop in the Tab order. |
| 23 | Components | `DataTable` has no way to open at its end or to tell where its viewport starts: `initialScrollAtEnd` and `measureScrollPosition()` (0.5.0) are a `List`'s. sonde's operations and output are tables. | A live table follows its end with what `DataTableRef` has: `onVisibleRowsChange` says where the view ended, `scrollToRow` takes it to the newest row while it was at the end (`OperationsTable`'s `follow`). A page read again keeps its rows' indices, so the table keeps its place by itself. |

18, 19, 22 and 23 are drafted as issues for Native Shell in [docs/nsh-issues.md](docs/nsh-issues.md).

## Taken from Native Shell 0.5.0 (sonde 16.0)

| What 0.5.0 added | What sonde does with it |
|---|---|
| `fileTypes` in `nsh.json`, `Application.on("openFiles")` | A `.sonde` file opens from the system (a double click, Open With, a drop on the app's icon), as one dropped on the window does. `.sonde.gz` is not declared: a type is its last extension, and sonde does not claim every `.gz`. |
| `Application.on("quit")` | The program being recorded and a running trial are ended, and the recording is written as a finished session, within the 200 ms the system waits (`Sessions.quit`). |
| `terminal:` as a class variant | 14 `terminal()` conditionals that only chose class strings say it in the class (39 → 25 class picks; those left choose between no classes at all and a set, or change what is drawn). |
| `accessibilityState.invalid`, `accessibilityDescription` | The kit's `Field` has an error state (a refused endpoint id, URL or profile; a confirmation typed wrong): its edge, the reason under it, and both told to a screen reader. A dialog's message is its description (`Modal`'s `describe`). |
| Per-platform icons | `icon` is an object: macOS artwork inset (824 of 1024), Windows and Linux edge to edge; both made from the one drawing by `scripts/icons.py`. |
| `initialScrollAtEnd`, `measureScrollPosition()` | Not usable: they are a `List`'s, and what follows its end in sonde is a `DataTable` (finding 23). |
| Views out of sight made over frames | Nothing to adopt; measured on the Time pane (docs/performance.md). |
| `onWheel`, `onMouseUp` | Not taken, on purpose (D-123). |

## By design

- No styled components: every control (buttons, fields, switches, selects, segmented control, badges) is sonde's own
  look on `nsh/base` behavior (`app/src/ui/kit.tsx`), with theme tokens (`app/theme.json`).
- Charts are Native Shell's `Plot` in windows (one chart system, `app/src/ui/plots/` by family; Capacity's three in
  `app/src/ui/charts.tsx`); in a terminal, as the TUI guide says, the same data is a table and a sentence.

## Fixed

Fixed in Native Shell itself (sonde follows upstream; it does not change Native Shell's source):

| # | Area | What it was | Fixed in |
|---|---|---|---|
| 3 | Desktop (Linux) | Under X11 (Xvfb with openbox), the GTK runner opened the window at 390×844 — a phone's size — not at `nsh.json`'s `window.defaultSize`; sonde set its bounds itself on a first launch. | Native Shell 744d950 (sonde 15.0.0): Linux and Windows packages carry `nsh.json`'s `window`, and the window has it from its first frame. sonde sets no bounds of its own (that code went in 9.1, when the main window's settings moved to `nsh.json`); re-checked at this commit, nothing is left to remove. |
| 17 | Testing | `nsh test` of an app with a Rust crate dropped every `sonde://` link (`app.openWithUrl`, `open`'s `urlScheme`): the headless runner's bundle was built without the app's `schemes`. | Native Shell 1662115 (sonde 8.0.0); `screens.test.tsx` opens a bucket's folder by link again. |
