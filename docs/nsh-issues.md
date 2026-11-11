# Native Shell issues found by sonde

Drafts for `longbridge/nsh`, from sonde 9.0 at Native Shell `1662115`. sonde works around both
(FINDINGS 18, 19) and does not change Native Shell's source.

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

**What happens.** A Plot reports `onHoverChange` only. Picking a slice needs the plot wrapped in a Button (two focus
stops; Enter on the plot does nothing), and a range (zoom) needs two presses on that button.

**Expected.** `onPress` with the row under the pointer, a `selectedIndex`, and a brush that reports a range.

## A Plot's value function cannot say "no value"

**What happens.** Marks are typed `(d, i) => number`; the docs say a row without a value draws no line, but
returning one takes `undefined as unknown as number`.

**Expected.** `number | null | undefined`.

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
