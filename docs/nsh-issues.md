# Native Shell issues found by sonde

Drafts for `longbridge/nsh`, begun in sonde 9.0 at Native Shell `1662115` and read again at `744d950` (15.0) and at
the tag `v0.5.0` (16.0, 81 commits later: Plot, the table, the sheet and the terminal are as they were),
where the three Plot requests (press and selection, a log scale, a row without a value) still stand, each with what
it would remove. sonde works around the first two (FINDINGS 18, 19) and does not change Native Shell's source.

## A theme font token keeps only the first family of its list

**What happens.** `nsh.json`'s theme sets `"font-mono": "SF Mono, Menlo, Consolas, monospace"`. On Linux and
Windows, text with `font-mono` is set in the proportional default font.

**Why.** `nsh-base/src/theme.rs` `font_family` keeps the first family of a `font-*` value
(`value.split(',').next()`): the token becomes `SF Mono`, which only macOS has. The platforms resolve a whole list
(`nsh-platform-linux/src/text.rs` `resolve_family`, `nsh-platform-windows/src/text.rs` `family_names`), but they
never see it.

**Expected.** The token keeps the list, as `fontFamily` does (`nsh_core::font_families`), so the first family a
platform has is used.

**Repro.** A theme with `"font-mono": "SF Mono, monospace"`; `<Text class="font-mono">0123 iiii WWWW</Text>` on
GTK: proportional.

## setColorScheme leaves the main window's backdrop in the system's appearance (macOS)

**What happens.** With `"backdrop": "sidebar"` in `nsh.json` and the system in light mode,
`setColorScheme("dark")` darkens the app's views but the window's material stays light: a light sidebar under
light text.

**Why.** `nsh-platform-macos/src/system.rs` `set_appearance` sets the appearance on the main window's root view and
calls `window::set_appearance`, which goes through `surface.windows`: windows opened with `Window.open`. The main
NSWindow is not among them, and the backdrop's NSVisualEffectView is a sibling of the root (`window.rs` `set_look`),
so it keeps the NSWindow's (the system's) appearance. The test agent's `set_dark` (`agent.rs`) sets the NSWindow's
and NSApplication's, which is why `nsh test --target macos` does not show it.

**Expected.** `setColorScheme` sets the main NSWindow's appearance (or its content container's), as it does for
the windows it opens.

**Repro.** macOS in light mode, `"backdrop": "sidebar"`, a transparent root, `setColorScheme("dark")`.

## A Button cannot say it is expanded

**What happens.** A disclosure button (sonde's "What stands out" over a `Collapsible`) has no way to tell a screen
reader whether its region is open: `ButtonProps` takes `selected` but no `accessibilityState`.

**Expected.** As `Collapsible`'s documentation asks of its trigger (`accessibilityState={{ expanded: open }}`),
`Button` takes `accessibilityState` (or an `expanded` prop).

**Repro.** `<Button accessibilityState={{ expanded: true }} />`: TS2322.


## A macOS test window larger than the screen is captured cut off, even when opened smaller

**What happens.** On the macOS CI runner (a 1024×768 screen), `open("../App.tsx", { width: 1024, height: 688 })`
then `app.screenshot()` writes a 1280×820 image whose content is 1024 points wide, offset 128 points from the left
and cut off on both sides: the app is laid out for one size and drawn into a window of another.

**Expected.** `open`'s size is the window's and the screenshot's on every target, or the window is fitted to the
screen and the app laid out for the size it got.

**Repro.** `nsh test tests/shots.test.tsx --target macos` on a GitHub `macos-15` runner with
`SONDE_SHOT_MAX=1024x688`; any screenshot.

## Plot has no press, selection or brush

Asked again at `744d950` (15.0): Plot there differs from the old pin by a doc comment. And at `v0.5.0` (16.0): Plot is
unchanged. (0.5.0's `onMouseUp` and `onWheel` are a view's, not a plot's: a drag over a plot would still be sonde's
own arithmetic from pixels to rows.)

**What happens.** A Plot reports `onHoverChange` only. Picking a slice needs the plot wrapped in a Button (two focus
stops; Enter on the plot does nothing), and a range (zoom) needs two presses on that button.

**Expected.** `onPress` with the row under the pointer, a `selectedIndex`, and a brush that reports a range.

**What sonde does today.** Five charts (the lanes, the session's shape, the waterfall, the trend of runs, the
scatter) stand in a `Button` that reads the row last hovered and acts on it; the selection is drawn by the app
(every other slice in the `dim` tone); a zoom is two presses kept in two signals of the Time pane.

A UI test cannot press a row either: `app.hover` goes to the plot's middle, where a scatter may have no dot.

**It would remove** about 70 lines: 49 of button around those five plots (`app/src/ui/plots.tsx`, counted) and
about 20 of the two-press zoom (`app/src/ui/profile.tsx` `pick`, `zoomFrom`).

## A Plot's value function cannot say "no value"

Asked again at `744d950` (15.0) and at `v0.5.0` (16.0).

**What happens.** Marks are typed `(d, i) => number`; the docs say a row without a value draws no line, but
returning one takes `undefined as unknown as number`.

**Expected.** `number | null | undefined`.

**What sonde does today.** The cast, seven times in `app/src/ui/plots.tsx`: the rule under the moment another chart
rests on (five charts), the dashed rules of the findings, the gaps of the latency band, the dots of the retries.

**It would remove** no line and seven casts: each `(undefined as unknown as number)` becomes `null`.

## The Terminal component has no find, links or marks

**What happens.** `TerminalRef` has `write`, `reset`, `text` and `focus` only: no search, no link detection, no way
to mark a line or scroll to it, so a program's output cannot point at the operation a line belongs to.

## Notifications have no actions

**What happens.** `ShowNotificationOptions` has `id`, `title`, `body` and `silent`: no buttons ("Open session",
"Run again") and no grouping.

## A view cannot be copied as an image

**What happens.** `Clipboard.writeImage(src)` takes an existing image; there is no snapshot of a view or a Plot, so
"Copy chart" for a ticket cannot be built.

## Files can be dropped in, not dragged out

**What happens.** `onFileDrop` exists; there is no drag source, so a session or an object cannot be dragged to the
file manager or a chat.

## No represented file, window tabs or recent documents

**What happens.** `WindowAttributes` has no represented file or tabs, and there is no system recent-documents list,
so an app keeps its own Open Recent.

## File dialogs and the file manager are Rust-only

**What happens.** `reveal_path`, `move_to_trash`, `pick_save_path` and `open_path` are in `nsh_core::element` for an
app's Rust; JS has `Picker.pickFiles` only. sonde calls them from its own module (sessions to the trash, a shared
page's folder shown).

**Expected.** A JS module for them, behind a capability.


## Plot has no log scale, and no horizontal rule

**What happens.** Scales are `point`, `linear` and `band`; latencies spread over decades are drawn as their logs and
the ticks format `10 ** v`. `Rule` is drawn per row (vertical); a horizontal reference line (a p95) is a `Grid`
with one value.

**Expected.** `{ type: "log" }` for x and y, and `Rule` (or `Grid`) taking a label for a reference line.

Asked again at `744d950` (15.0) and at `v0.5.0` (16.0).

**What sonde does today.** The scatter (time or size against latency) and the comparison of two distributions take
the logarithm of every value before the plot sees it (`log10`), work out the decades for the grid and the ticks
(`decades`), and turn each tick back with `10 ** v`.

**It would remove** about 17 lines of `app/src/ui/plots.tsx` (`log10`, `decades`, the conversions of rows, domains
and ticks), and with them the tooltip's need to keep the value beside its logarithm.

The three Plot requests together come to about 90 lines and seven casts, measured; the review of 14.0 had guessed
about 300.

## A Sankey's labels cannot sit beside its nodes

**What happens.** `nodeLabel` draws on the node; on a middle column it overlaps the ribbons. There is no placement.

**Expected.** `nodeLabelPlacement`: `inside`, `outside` (left of the first column, right of the others), `auto`.

## A Button has no context menu event

**What happens.** `onContextMenu` is on views and tables, not on `Button`; a pressable row that also offers a menu
wraps the button in a view.

## A Plot's tooltip is kept inside the plot

**What happens.** `tooltipContent` is placed inside the plot's edges; on a short chart (a 56-point pressure strip)
a tooltip of two lines is taller than the plot and is cut, and near the top it covers the marks it describes.

**Expected.** A tooltip in the window's overlay layer, flipped and shifted to stay inside the window, as `Tooltip`
and `Popover` are.

## A sortable title cannot keep its sort button beside it

**What happens.** The title text of a `DataTable` column is `flex-1`, so the sort chevrons sit at the column's far
end; a wide name column has its title at the start and its button a long way off.

**Expected.** The button right after the title (start-aligned), before it (end-aligned), or a `headerContent` part
whose title the app can size.

## A DataTable column taken from the middle leaves the titles a column off

**What happens.** When `columns` loses one that is not last (a narrow table dropping a column), the cells follow the
new columns but the titles keep the old places for a frame or more: "Operation" over the requests.

**Expected.** Column state kept by `key`, not by place.

## Icons in a Sheet in a second window do not paint

**What happens.** A `Sheet` opened in a window other than the main one (Settings) draws its text and controls but
not its SVG icons: no close button, no chevrons on a `Select`. The same components paint in the window itself.

**Expected.** The sheet's layer paints icons as its window does; and a way to place it below the window's own bar.

## On GTK a window gives its first text field the focus when the focused one is unfocused by the program

**What happens.** (Linux, GTK 4; at `744d950` and at `v0.5.0`.) A window whose focused `Input` loses the focus by
the program's doing, not the user's, gives the focus to the first text field in the window, with its text selected:

- `Keyboard.dismiss()` while an input has the focus;
- `ref.focus()` on a `focusable` `View`;
- a `Dialog` opening after another window of the app (Settings) was opened and closed;
- the focused input's page going away.

sonde's toolbar begins with a `Combobox` (the endpoint): after any of these it stood with a focus ring and its
name selected, through every later screen. In a second window the first `NumberInput` took it the same way.

**Repro.** A window with `<Combobox … />` first and an `<Input />` further down. Type in the input, then call
`Keyboard.dismiss()`: the combobox is focused and its query selected. Under `nsh test --target linux` (Xvfb,
openbox) the screenshot shows it.

**Expected.** Unfocusing leaves no text field focused (as on macOS), and `focus()` on a focusable view takes the
keyboard to that view.

**What sonde does.** Each window's first text field is an invisible, read-only one (`FocusRest`), on Linux only.

## DataTable cannot open at its end, or tell where its viewport starts

**What happens.** 0.5.0 gave `List` `initialScrollAtEnd` and `measureScrollPosition()`. `DataTable` has neither:
its ref has `scrollToRow`, `visibleRows()` and `onVisibleRowsChange`. A table of rows that arrive (a log, live
requests) opens at its top, and following its end is the app's bookkeeping.

**Expected.** The same two on `DataTable`: open at the end and follow it while rows are added, and a viewport
start (`{ row, offset }`) that `scrollToRow` can restore.

**What it would remove.** `OperationsTable`'s `follow` in sonde (where the view ended, how many rows there were,
a `scrollToRow` a tick later).
