// Presentation helpers: sizes, dates, durations, money. Dates and counts
// follow the app's language and the user's clock (12 or 24 hours).

import { localeSettings } from "nsh";
import { lang } from "./i18n";

/** The locale to write dates and numbers in: the app's language, the user's region where it matches. */
function locale(): string {
  const want = lang();
  const own = localeSettings().languages.find((l) => l.toLowerCase().startsWith(want));
  return own ?? (want === "zh" ? "zh-CN" : "en-US");
}

const formats = new Map<string, Intl.DateTimeFormat>();
function dateFormat(): Intl.DateTimeFormat | null {
  const { hourCycle, timeZone } = localeSettings();
  const key = `${locale()}|${hourCycle}|${timeZone}`;
  let f = formats.get(key);
  if (!f) {
    try {
      f = new Intl.DateTimeFormat(locale(), { year: "numeric", month: "short", day: "numeric", hour: "numeric", minute: "2-digit", hourCycle, timeZone });
    } catch {
      return null;
    }
    formats.set(key, f);
  }
  return f;
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
  if (m < 1) return "now";
  if (m < 60) return `${m} min`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h} h`;
  return `${Math.round(h / 24)} d`;
}

export function days(d: number | null | undefined): string {
  if (d === null || d === undefined) return "—";
  return d < 1 ? `${Math.round(d * 24)} h` : `${d.toFixed(1)} d`;
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
