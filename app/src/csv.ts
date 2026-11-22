// A chart's rows: what it draws, as numbers in one unit each (named in the
// column's title), for a screen reader's table in a chart's place.

/** The rows a chart draws: its columns' titles and a row of plain values each. */
export interface ChartRows {
  columns: string[];
  rows: (string | number | null)[][];
}
