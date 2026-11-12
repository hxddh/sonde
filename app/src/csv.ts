// A chart's rows as CSV: what it draws, as numbers in one unit each (named
// in the column's title), for a spreadsheet. RFC 4180: a field with a comma,
// a quote or a line break is quoted, its quotes doubled; lines end in CRLF.

/** The rows a chart draws: its columns' titles and a row of plain values each. */
export interface ChartRows {
  columns: string[];
  rows: (string | number | null)[][];
}

const field = (v: string | number | null) => {
  if (v === null || (typeof v === "number" && !Number.isFinite(v))) return "";
  // numbers as a sheet reads them: no separators, a dot for a decimal, at most six places
  const s = typeof v === "number" ? `${Math.round(v * 1e6) / 1e6}` : v;
  return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
};

/** The rows as CSV text, its header first. */
export const toCsv = (t: ChartRows) => [t.columns, ...t.rows].map((r) => r.map(field).join(",")).join("\r\n") + "\r\n";
