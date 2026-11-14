# Native Shell issues found by sonde

Drafts for `longbridge/nsh`, last checked at tag `v0.5.0` (all still stand). sonde works around them in its own code
(FINDINGS 18, 19) and does not patch Native Shell. The three Plot requests together would remove about 90 lines and
seven casts (measured).

## A theme font token keeps only the first family of its list

- **What**: `"font-mono": "SF Mono, Menlo, Consolas, monospace"` sets `font-mono` text in the proportional default
  font on Linux and Windows.
- **Why**: `nsh-base/src/theme.rs` `font_family` keeps `value.split(',').next()` (`SF Mono`, macOS only). The
  platforms resolve lists (`nsh-platform-linux/src/text.rs` `resolve_family`, `nsh-platform-windows/src/text.rs`
  `family_names`) but never get one.
- **Expected**: the token keeps the list, as `fontFamily` does (`nsh_core::font_families`).
- **Repro**: `"font-mono": "SF Mono, monospace"`; `<Text class="font-mono">0123 iiii WWWW</Text>` on GTK is proportional.

## setColorScheme leaves the main window's backdrop in the system's appearance (macOS)

- **What**: with `"backdrop": "sidebar"` and the system light, `setColorScheme("dark")` darkens the views but not the
  window material: a light sidebar under light text.
- **Why**: `nsh-platform-macos/src/system.rs` `set_appearance` sets the root view and calls `window::set_appearance`,
  which covers only `Window.open` windows. The backdrop's NSVisualEffectView is the root's sibling (`window.rs`
  `set_look`), so it keeps the NSWindow's appearance. The test agent's `set_dark` (`agent.rs`) sets NSWindow and
  NSApplication, so `nsh test --target macos` hides it.
- **Expected**: `setColorScheme` sets the main NSWindow's appearance too.
- **Repro**: macOS light mode, `"backdrop": "sidebar"`, a transparent root, `setColorScheme("dark")`.

## A Button cannot say it is expanded

- **What**: `ButtonProps` has `selected` but no `accessibilityState`, so a disclosure button cannot report open/closed.
- **Expected**: `accessibilityState` (or `expanded`) on `Button`, as `Collapsible`'s docs ask of its trigger.
- **Repro**: `<Button accessibilityState={{ expanded: true }} />`: TS2322.

## Plot has no press, selection or brush

- **What**: Plot reports only `onHoverChange`. Picking a slice needs a wrapping Button (two focus stops; Enter on the
  plot does nothing); a zoom needs two presses. A UI test cannot press a row (`app.hover` hits the plot's middle).
  0.5.0's `onMouseUp`/`onWheel` are a view's, not a plot's.
- **Expected**: `onPress` with the row, `selectedIndex`, a brush reporting a range.
- **sonde today**: five charts in a `Button` acting on the last hovered row; selection drawn by the app (`dim`); zoom
  as two presses in the Time pane.
- **Would remove**: about 70 lines (49 in `app/src/ui/plots.tsx`, about 20 in `profile.tsx` `pick`, `zoomFrom`).

## A Plot's value function cannot say "no value"

- **What**: marks are `(d, i) => number`; no value needs `undefined as unknown as number`.
- **Expected**: `number | null | undefined`.
- **Would remove**: seven casts in `app/src/ui/plots.tsx`.

## Plot has no log scale, and no horizontal rule

- **What**: scales are `point`, `linear`, `band`. `Rule` is vertical per row; a horizontal reference (p95) is a
  one-value `Grid`.
- **Expected**: `{ type: "log" }` for x and y; a labelled horizontal `Rule` (or `Grid`).
- **sonde today**: the scatter and distribution comparison take `log10` of every value, compute decades, format ticks
  as `10 ** v`.
- **Would remove**: about 17 lines of `app/src/ui/plots.tsx`, and the tooltip's copy of the raw value.

## A Plot's tooltip is kept inside the plot

- **What**: on a short chart (a 56-point strip) a two-line tooltip is cut, and near the top covers its marks.
- **Expected**: the tooltip in the window's overlay layer, flipped and shifted like `Tooltip` and `Popover`.

## A Sankey's labels cannot sit beside its nodes

- **What**: `nodeLabel` draws on the node, over the ribbons on middle columns.
- **Expected**: `nodeLabelPlacement`: `inside`, `outside`, `auto`.

## The Terminal component has no find, links or marks

- **What**: `TerminalRef` has only `write`, `reset`, `text`, `focus`: output cannot point at its operation.

## Notifications have no actions

- **What**: `ShowNotificationOptions` has `id`, `title`, `body`, `silent`: no buttons, no grouping.

## A view cannot be copied as an image

- **What**: `Clipboard.writeImage(src)` needs an existing image; no snapshot of a view or Plot ("Copy chart").

## Files can be dropped in, not dragged out

- **What**: `onFileDrop` exists; no drag source for a session or object.

## No represented file, window tabs or recent documents

- **What**: `WindowAttributes` has neither, and there is no system recent list; apps keep their own Open Recent.

## File dialogs and the file manager are Rust-only

- **What**: `reveal_path`, `move_to_trash`, `pick_save_path`, `open_path` are in `nsh_core::element`; JS has only
  `Picker.pickFiles`. sonde calls them from its own module.
- **Expected**: a JS module behind a capability.

## A Button has no context menu event

- **What**: `onContextMenu` is on views and tables, not `Button`; a row with a menu wraps its button in a view.

## A sortable title cannot keep its sort button beside it

- **What**: a `DataTable` column title is `flex-1`, so the chevrons sit at the column's far end.
- **Expected**: the button beside the title, or a `headerContent` part the app can size.

## A DataTable column taken from the middle leaves the titles a column off

- **What**: when `columns` loses a non-last column, cells follow at once but titles keep old places for a frame or
  more.
- **Expected**: column state kept by `key`, not position.

## DataTable cannot open at its end, or tell where its viewport starts

- **What**: `List` has `initialScrollAtEnd` and `measureScrollPosition()` (0.5.0); `DataTable` has only
  `scrollToRow`, `visibleRows()`, `onVisibleRowsChange`.
- **Expected**: the same two on `DataTable`: open at and follow the end; a viewport start (`{ row, offset }`) that
  `scrollToRow` can restore.
- **Would remove**: `OperationsTable`'s `follow` bookkeeping.

## Icons in a Sheet in a second window do not paint

- **What**: a `Sheet` in a non-main window (Settings) draws text and controls but no SVG icons (no close button, no
  `Select` chevrons).
- **Expected**: icons paint as in the window; and a way to place the sheet below the window's bar.

## On GTK, a programmatic unfocus focuses the first text field

- **What** (Linux, GTK 4): when the focused `Input` loses focus by the program, the first text field in the window
  takes it with its text selected. Triggers: `Keyboard.dismiss()`; `ref.focus()` on a `focusable` `View`; a `Dialog`
  opened after another window was opened and closed; the focused input's page going away.
- **Repro**: `<Combobox … />` first, an `<Input />` below; type in the input, `Keyboard.dismiss()`: the combobox is
  focused, its query selected (visible under `nsh test --target linux`).
- **Expected**: no text field focused (as on macOS); `focus()` on a focusable view takes the keyboard there.
- **sonde today**: an invisible read-only first field (`FocusRest`) per window, Linux only.
