// Presentation helpers: sizes, dates, durations, money.

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
  return n.toLocaleString("en-US");
}

export function date(s: string | null | undefined): string {
  if (!s) return "—";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return s;
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
