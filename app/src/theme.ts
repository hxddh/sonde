// The theme's high-contrast variant: the same tokens with stronger borders and
// secondary text, applied while the app runs (Native Shell's setTheme).

import { setTheme, type Theme } from "nsh";
import base from "../theme.json";

const STRONGER: Record<"light" | "dark", Record<string, string>> = {
  light: {
    border: "oklch(0.62 0 0)",
    input: "oklch(0.55 0 0)",
    "sidebar-border": "oklch(0.62 0 0)",
    "muted-foreground": "oklch(0.36 0 0)",
    "bar-track": "oklch(0.84 0 0)",
    ring: "oklch(0.45 0.2 256)",
  },
  dark: {
    border: "oklch(0.58 0 0)",
    input: "oklch(0.64 0 0)",
    "sidebar-border": "oklch(0.58 0 0)",
    "muted-foreground": "oklch(0.86 0 0)",
    "bar-track": "oklch(0.38 0 0)",
    ring: "oklch(0.75 0.15 256)",
  },
};

export function highContrast(): Theme {
  const v = base.cssVars as { theme: Record<string, string>; light: Record<string, string>; dark: Record<string, string> };
  return { cssVars: { theme: v.theme, light: { ...v.light, ...STRONGER.light }, dark: { ...v.dark, ...STRONGER.dark } } };
}

/** The theme as the setting says: high contrast, or the one the app was built with. */
export function applyContrast(on: boolean) {
  try {
    setTheme(on ? highContrast() : null);
  } catch {
    // a value the platform cannot read: the built theme stays
  }
}
