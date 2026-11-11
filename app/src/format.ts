// Presentation helpers: sizes, dates, durations, money. Dates and counts
// follow the app's language and the user's clock (12 or 24 hours).

import { localeSettings } from "nsh";
import { lang, t } from "./i18n";

/** The locale to write dates and numbers in: the app's language, the user's region where it matches. */
export function locale(): string {
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
  // the unit after rounding: 1,048,575 B is 1.0 MiB, not 1024 KiB
  if (i > 0 && i < units.length - 1 && Number(v.toFixed(v >= 100 ? 0 : 1)) >= 1024) {
    v /= 1024;
    i++;
  }
  return i === 0 ? `${Math.round(n)} B` : `${v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`;
}

/** Bytes sent and received, the one way everywhere: "↑ 78.1 KiB ↓ 75.1 KiB". */
export function transfer(up: number | null | undefined, down: number | null | undefined, each: (n: number) => string = bytes): string {
  return `↑ ${each(up ?? 0)} ↓ ${each(down ?? 0)}`;
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

export function money(v: number, currency: string): string {
  return `${v < 0.01 && v > 0 ? "<0.01" : v.toFixed(2)} ${currency}`;
}

/** A length of time, the one way everywhere. As measured (the default):
 * 0.4 ms, 84 ms, 2.35 s, 41.2 s, 3 min 05 s, 2 h 05 min — the unit chosen
 * after rounding, so nothing reads "1000 ms" or "60.0 s". As an `age` (how
 * long ago, how old): whole and coarse, in the reader's words — now, 5 min,
 * 3 h, 86 d. As a `counter` (a run's time so far): 0:07, 12:40, 1:05:09.
 * No other function writes one. */
export function duration(v: number, as: "measured" | "age" | "counter" = "measured"): string {
  if (!Number.isFinite(v)) return "—";
  if (as === "age") {
    const m = Math.round(v / 60000);
    if (m < 1) return t("time.now");
    if (m < 60) return t("time.minutes", { n: m });
    const h = Math.round(m / 60);
    return h < 48 ? t("time.hours", { n: h }) : t("time.days", { n: count(Math.round(h / 24)) });
  }
  if (as === "counter") {
    const s = Math.max(0, Math.floor(v / 1000));
    const h = Math.floor(s / 3600);
    return h ? `${h}:${p2(Math.floor((s % 3600) / 60))}:${p2(s % 60)}` : `${Math.floor(s / 60)}:${p2(s % 60)}`;
  }
  const a = Math.abs(v);
  if (a === 0) return "0 ms";
  if (a < 9.95) return `${v.toFixed(1)} ms`;
  if (a < 999.5) return `${Math.round(v)} ms`;
  if (a < 9995) return `${(v / 1000).toFixed(2)} s`;
  if (a < 59950) return `${(v / 1000).toFixed(1)} s`;
  const s = Math.round(a / 1000);
  const sign = v < 0 ? "−" : "";
  if (s < 3600) return `${sign}${Math.floor(s / 60)} min ${p2(s % 60)} s`;
  const m = Math.round(s / 60);
  return `${sign}${Math.floor(m / 60)} h ${p2(m % 60)} min`;
}

/** How long ago a moment was, as an age: "now", "5 min", "3 h", "86 d". */
export const since = (iso: string | null | undefined): number => (iso ? Date.now() - new Date(iso).getTime() : Number.NaN);

/** A share as people read it: whole percents, "<1%" for a sliver, "0%" for none. */
export function percent(fraction: number): string {
  if (!Number.isFinite(fraction) || fraction === 0) return "0%";
  const r = Math.round(fraction * 100);
  if (r === 0) return fraction > 0 ? "<1%" : ">−1%";
  return `${r}%`;
}

export function pct(part: number, whole: number): string {
  return whole === 0 ? "0%" : percent(part / whole);
}

/** How many times over, to one decimal: "1.5" (the caller writes its "×"). */
export function factor(x: number): string {
  return Number.isFinite(x) ? x.toFixed(1) : "—";
}

/** So many a second, as a live counter reads: a decimal under ten, whole from there. */
export function rate(perSecond: number): string {
  return Number.isFinite(perSecond) ? (perSecond < 10 ? perSecond.toFixed(1) : count(Math.round(perSecond))) : "—";
}

/** A change from `a` to `b` as a signed share of `a`: "+12%", "−3%", "0%"
 * only for no change at all ("+<1%" for one too small to round). */
export function change(a: number, b: number): string {
  if (a === b) return "0%";
  if (a === 0) return "+∞";
  const sign = b > a ? "+" : "−";
  const r = Math.round((Math.abs(b - a) * 100) / Math.abs(a));
  return r === 0 ? `${sign}<1%` : `${sign}${r}%`;
}

/** What a run's requests cost, as people read it: a session's often cost
 * thousandths of a cent, which read as a thousand runs' worth
 * ("0.28 USD per 1,000 runs"). */
export function cost(v: number, currency: string): string {
  if (!Number.isFinite(v) || v < 0) return `— ${currency}`;
  if (v > 0 && v < 0.01) {
    const k = v * 1000;
    const s = k < 0.01 ? Number(k.toPrecision(2)).toString() : k.toFixed(2);
    return t("cost.perThousand", { v: s, cur: currency });
  }
  return `${v.toFixed(2)} ${currency}`;
}

/** A moment on the reader's clock, to the precision its list needs:
 * `14:02`, `14:02:11` or `14:02:11.204` (operations and output). */
export function clock(iso: string, precision: "m" | "s" | "ms" = "ms"): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const hms = `${p2(d.getHours())}:${p2(d.getMinutes())}`;
  if (precision === "m") return hms;
  const s = `${hms}:${p2(d.getSeconds())}`;
  return precision === "s" ? s : `${s}.${String(d.getMilliseconds()).padStart(3, "0")}`;
}

/** A long key cut in its middle, so the end that tells keys apart stays:
 * `slate/db/wal/…0000127.wal`. */
export function middle(text: string, max = 48): string {
  if (text.length <= max) return text;
  const tail = Math.ceil((max - 1) * 0.55);
  return `${text.slice(0, max - 1 - tail)}…${text.slice(-tail)}`;
}

/** The characters this locale groups thousands with and marks decimals with. */
export function separators(): { group: string; decimal: string } {
  const parts = new Intl.NumberFormat(locale()).formatToParts(12345.6);
  return {
    group: parts.find((p) => p.type === "group")?.value ?? ",",
    decimal: parts.find((p) => p.type === "decimal")?.value ?? ".",
  };
}
