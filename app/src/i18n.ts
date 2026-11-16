// The app's words, in English and Chinese: `locales/en.json` and
// `locales/zh.json`, read by Native Shell's i18n (`t("key", { n: 3 })` fills
// `%{n}`). The setting picks the language, or follows the system's.

import { effect, locale, signal } from "nsh";
import { setLocale, t as translate } from "nsh/i18n";
import { count } from "./format";

/** Every key the app has words for (English has them all). */
export type Key = keyof typeof import("../locales/en.json");

/** `auto`, `en` or `zh` (the setting). */
export const language = signal<string>("auto");

const systemLanguage = () => (locale().toLowerCase().startsWith("zh") ? "zh" : "en");

/** The language shown. */
export function lang(): "en" | "zh" {
  const l = language();
  return l === "zh" || l === "en" ? l : systemLanguage();
}

// the setting, as Native Shell's language: `null` follows the system's
effect(() => {
  const l = language();
  setLocale(l === "en" || l === "zh" ? l : null);
});

function lookup(key: string, vars: Record<string, string | number> | undefined): string | null {
  const s = translate(key, vars ? { ...vars, locale: lang() } : { locale: lang() });
  // a key nothing has words for reads as `<locale>.<key>`
  return s === `${lang()}.${key}` ? null : s;
}

/** The words of `key` as written (`%{name}` unfilled), or null where no language has them: a phrase Rust
 * said ([`phrases.ts`](./phrases.ts)), filled there. */
export function words(key: string): string | null {
  lang();
  return lookup(key, undefined);
}

/** The arguments that are counts: a figure of four digits or more among them is grouped as every count is
 * ("listed 87,139 times", beside "87,139 requests"); ids, years, ports, exit codes and versions are other
 * arguments, left as written. The same for the app's words (`t`) and Rust's phrases ([`phrases.ts`](./phrases.ts)). */
export const COUNTS = new Set(["n", "of", "count", "requests", "reads", "writes", "objects", "total", "was", "clean"]);

/** An argument as it is said: a count grouped ("1,500"), anything else as given. */
export function grouped(k: string, v: string | number): string | number {
  if (!COUNTS.has(k)) return v;
  if (typeof v === "number") return Number.isInteger(v) && Math.abs(v) >= 1000 ? count(v) : v;
  return /^\d{4,}$/.test(v) ? count(Number(v)) : v;
}

/** The words for `key`, filled from `vars`. A count of one (`n: 1`) reads
 * `<key>.one` where the language has it ("1 request", not "1 requests").
 * A count is grouped wherever it is said ("1,500 requests"), whoever wrote
 * the figure. */
export function t(key: Key, vars?: Record<string, string | number>): string {
  // read so a view that shows it follows the setting at once
  lang();
  const filled = vars && Object.fromEntries(Object.entries(vars).map(([k, v]) => [k, grouped(k, v)]));
  if (vars && Number(vars.n) === 1) {
    const one = lookup(`${key}.one`, filled);
    if (one !== null) return one;
  }
  return lookup(key, filled) ?? key;
}
