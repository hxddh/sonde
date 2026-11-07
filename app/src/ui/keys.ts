// Shortcuts as people read them on their platform: ⌘K on macOS, Ctrl+K on
// Windows and Linux. One spelling ("mod+k") for keymap, menus and labels.

import { platform } from "nsh";

const mac = () => platform.os === "macos" || platform.os === "ios";

const MAC: Record<string, string> = { mod: "⌘", cmd: "⌘", ctrl: "⌃", alt: "⌥", shift: "⇧", enter: "↩", escape: "⎋", backspace: "⌫", up: "↑", down: "↓", left: "←", right: "→" };
const PC: Record<string, string> = { mod: "Ctrl", cmd: "Ctrl", ctrl: "Ctrl", alt: "Alt", shift: "Shift", enter: "Enter", escape: "Esc", backspace: "Backspace", up: "↑", down: "↓", left: "←", right: "→" };

/** "mod+shift+k" → "⇧⌘K" on macOS, "Ctrl+Shift+K" elsewhere. */
export function keyLabel(shortcut: string): string {
  const parts = shortcut.toLowerCase().split("+");
  const names = mac() ? MAC : PC;
  const shown = parts.map((p) => names[p] ?? (p.length === 1 ? p.toUpperCase() : p[0].toUpperCase() + p.slice(1)));
  if (mac()) {
    // macOS orders modifiers ⌃⌥⇧⌘ and writes them together
    const order = ["⌃", "⌥", "⇧", "⌘"];
    const mods = shown.slice(0, -1).sort((a, b) => order.indexOf(a) - order.indexOf(b));
    return [...mods, shown[shown.length - 1]].join("");
  }
  return shown.join("+");
}
