// Presentation helpers: sizes, dates, durations, money. Dates and counts
// follow the app's language and the user's clock (12 or 24 hours).

import { localeSettings } from "nsh";
import { lang, t } from "./i18n";

/** The locale to write dates and numbers in: the app's language, the user's region where it matches. */
function locale(): string {
  const want = lang();
  const own = localeSettings().languages.find((l) => l.toLowerCase().startsWith(want));
  return own ?? (want === "zh" ? "zh-CN" : "en-US");
}

const formats = new Map<string, Intl.DateTimeFormat>();
function dateFormat(parts: "full" | "day" | "time" = "full"): Intl.DateTimeFormat | null {
  const { hourCycle, timeZone } = localeSettings();
  const key = `${parts}|${locale()}|${hourCycle}|${timeZone}`;
  let f = formats.get(key);
  if (!f) {
    const day = { year: "numeric", month: "short", day: "numeric" } as const;
    const time = { hour: "numeric", minute: "2-digit", hourCycle } as const;
    try {
      f = new Intl.DateTimeFormat(locale(), { ...(parts !== "time" ? day : {}), ...(parts !== "day" ? time : {}), timeZone });
    } catch {
      return null;
    }
    formats.set(key, f);
  }
  return f;
}

const p2 = (x: number) => String(x).padStart(2, "0");

/** A day as the reader writes it: "7 Nov 2026". */
export function day(s: string | null | undefined): string {
  if (!s) return "—";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return s;
  return dateFormat("day")?.format(d) ?? `${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())}`;
}

/** A time of day as the reader's clock writes it: "14:05" or "2:05 PM". */
export function hm(s: string | null | undefined): string {
  if (!s) return "—";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return s;
  return dateFormat("time")?.format(d) ?? `${p2(d.getHours())}:${p2(d.getMinutes())}`;
}

export function bytes(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return i === 0 ? `${n} B` : `${v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`;
}

export function count(n: number): string {
  try {
    return n.toLocaleString(locale());
  } catch {
    return String(n);
  }
}

/** A moment (ISO 8601 or an HTTP date) as the reader writes dates: "7 Nov 2026, 14:05". */
export function date(s: string | null | undefined): string {
  if (!s) return "—";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return s;
  const f = dateFormat();
  if (f) return f.format(d);
  const p = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function ago(s: string | null | undefined): string {
  if (!s) return "—";
  const ms = Date.now() - new Date(s).getTime();
  if (Number.isNaN(ms)) return s;
  const m = Math.round(ms / 60000);
  if (m < 1) return t("time.now");
  if (m < 60) return t("time.minutes", { n: m });
  const h = Math.round(m / 60);
  if (h < 48) return t("time.hours", { n: h });
  return t("time.days", { n: Math.round(h / 24) });
}

export function days(d: number | null | undefined): string {
  if (d === null || d === undefined) return "—";
  return d < 1 ? t("time.hours", { n: Math.round(d * 24) }) : t("time.days", { n: d.toFixed(1) });
}

export function money(v: number, currency: string): string {
  return `${v < 0.01 && v > 0 ? "<0.01" : v.toFixed(2)} ${currency}`;
}

export function ms(v: number): string {
  return v < 10 ? `${v.toFixed(1)} ms` : `${Math.round(v)} ms`;
}

export function pct(part: number, whole: number): string {
  return whole === 0 ? "0%" : `${((part * 100) / whole).toFixed(1)}%`;
}

/** A long key cut in its middle, so the end that tells keys apart stays:
 * `slate/db/wal/…0000127.wal`. */
export function middle(text: string, max = 48): string {
  if (text.length <= max) return text;
  const tail = Math.ceil((max - 1) * 0.55);
  return `${text.slice(0, max - 1 - tail)}…${text.slice(-tail)}`;
}
