# Resolved findings

Moved from [FINDINGS](../dev/FINDINGS.md) and [nsh-issues](../dev/nsh-issues.md) once resolved or no longer met.

## Fixed in Native Shell

Fixed in Native Shell itself (sonde follows upstream; it does not change Native Shell's source):

| # | Area | What it was | Fixed in |
|---|---|---|---|
| 3 | Desktop (Linux) | Under X11 (Xvfb with openbox), the GTK runner opened the window at 390×844 — a phone's size — not at `nsh.json`'s `window.defaultSize`; sonde set its bounds itself on a first launch. | Native Shell 744d950 (sonde 15.0.0): Linux and Windows packages carry `nsh.json`'s `window`, and the window has it from its first frame. sonde sets no bounds of its own (that code went in 9.1, when the main window's settings moved to `nsh.json`); re-checked at this commit, nothing is left to remove. |
| 10 | Desktop | `windowSize()` followed the window with the focus: with a second window (Settings) focused, the main window's layout read the second one's width and folded its sidebar. | Native Shell 0.7.0 (#2923, sonde 32.0): it is the app's first window's, whatever has the focus. The main window's shell still measures itself (`shellWidth`); a dialog in Settings' or a run's window sizes from its own window (`Modal`). |
| 17 | Testing | `nsh test` of an app with a Rust crate dropped every `sonde://` link (`app.openWithUrl`, `open`'s `urlScheme`): the headless runner's bundle was built without the app's `schemes`. | Native Shell 1662115 (sonde 8.0.0); `screens.test.tsx` opens a bucket's folder by link again. |

## Taken from Native Shell 0.7.0 (sonde 32.0)

| What 0.7.0 added | What sonde does with it |
|---|---|
| Window Model v2: `closeOnPress`, `closePress`, `Application.on("reopen")` | Closing the main window quits on every platform, asking first while sonde is at work (one question however often close or Quit is pressed); Settings and run windows go with it. The Dock's icon and a later launch bring the window forward. `setQuitRequestOptions({ mainWindowClose })` and the macOS-only branch are gone. |
| `TitleBar` (`nsh/base`) | The toolbar and the sidebar's strip (kit's `TitleRow`): they step aside of the window's controls where they meet them, in place of reading `titleBar()` by hand. |
| DataTable `grow`, `autoFit`, `onRowActivate` | The operation's title and a `SimpleTable`'s filling column take the room the others leave; the output's times fit their words; Enter opens the focused row of the operations, the output, Objects and every `SimpleTable`. |
| Button `busy` | A loading button is said busy to a screen reader. |

## Taken from Native Shell 0.5.0 (sonde 16.0)

| What 0.5.0 added | What sonde does with it |
|---|---|
| `fileTypes` in `nsh.json`, `Application.on("openFiles")` | A `.sonde` file opens from the system (a double click, Open With, a drop on the app's icon), as one dropped on the window does. `.sonde.gz` is not declared: a type is its last extension, and sonde does not claim every `.gz`. |
| `Application.on("quit")` | The program being recorded and a running trial are ended, and the recording is written as a finished run, within the 200 ms the system waits (`Runs.quit`). |
| `terminal:` as a class variant | 14 `terminal()` conditionals that only chose class strings say it in the class (39 → 25 class picks; those left choose between no classes at all and a set, or change what is drawn). |
| `accessibilityState.invalid`, `accessibilityDescription` | The kit's `Field` has an error state (a refused endpoint id, URL or profile; a confirmation typed wrong): its edge, the reason under it, and both told to a screen reader. A dialog's message is its description (`Modal`'s `describe`). |
| Per-platform icons | `icon` is an object: macOS artwork inset (824 of 1024), Windows and Linux edge to edge; both made from the one drawing by `scripts/icons.py`. |
| `initialScrollAtEnd`, `measureScrollPosition()` | Not usable: they are a `List`'s, and what follows its end in sonde is a `DataTable` ([finding 23](../dev/FINDINGS.md)). |
| Views out of sight made over frames | Nothing to adopt; measured on the Time pane ([performance](../dev/performance.md)). |
| `onWheel`, `onMouseUp` | Not taken, on purpose (D-123). |

## No longer met: icons in a Sheet in a second window

From [nsh-issues](../dev/nsh-issues.md).

- **What**: a `Sheet` in a non-main window (Settings) draws text and controls but no SVG icons (no close button, no
  `Select` chevrons).
- **Expected**: icons paint as in the window; and a way to place the sheet below the window's bar.
- **Not re-checked** at `e4749c9`, where a Sheet became a dialog's modal root: sonde opens no Sheet in a second window
  any more.
