// The app's words, in English and Chinese: `locales/en.json` and
// `locales/zh.json`, read by Native Shell's i18n (`t("key", { n: 3 })` fills
// `%{n}`). The setting picks the language, or follows the system's.

import { effect, locale, signal } from "nsh";
import { setLocale, t as translate } from "nsh/i18n";

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

/** The words for `key`, filled from `vars`. A count of one (`n: 1`) reads
 * `<key>.one` where the language has it ("1 request", not "1 requests"). */
export function t(key: Key, vars?: Record<string, string | number>): string {
  // read so a view that shows it follows the setting at once
  lang();
  if (vars && Number(vars.n) === 1) {
    const one = lookup(`${key}.one`, vars);
    if (one !== null) return one;
  }
  return lookup(key, vars) ?? key;
}
