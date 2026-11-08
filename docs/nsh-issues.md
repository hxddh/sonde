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
