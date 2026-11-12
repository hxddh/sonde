// A stretch of a session's time kept in a link: the slices chosen on the
// Time pane (D-123: chosen by two presses, never dragged or wheeled) go into
// a `sonde://` link and a shared page's address as milliseconds from the
// session's start, so whoever opens it sees the same time. Slices are a
// session's own (a live one's change length): the link holds times, and
// each reader turns them into its own slices.

import { signal } from "nsh";

/** A stretch in ms from the session's start: `[from, to)`. */
export type Range = [number, number];

/** `range=120-480` as a link's query or a page's hash writes it; null for anything else. */
export function rangeOf(text: string | null | undefined): Range | null {
  const m = /(?:^|[?&#])range=(\d+(?:\.\d+)?)-(\d+(?:\.\d+)?)(?:$|&)/.exec(text ?? "");
  if (!m) return null;
  const a = Number(m[1]);
  const b = Number(m[2]);
  return Number.isFinite(a) && Number.isFinite(b) && b > a ? [a, b] : null;
}

/** The query part for a stretch: `range=120-480` (whole ms). */
export const rangeParam = (r: Range) => `range=${Math.floor(r[0])}-${Math.ceil(r[1])}`;

/** A stretch as slices of `binMs` (the first and the last slice it covers), within `bins`; null where it covers less than two. */
export function slicesOf(r: Range, binMs: number, bins: number): [number, number] | null {
  if (!(binMs > 0) || bins < 2) return null;
  const a = Math.max(0, Math.min(bins - 1, Math.floor(r[0] / binMs)));
  const b = Math.max(0, Math.min(bins - 1, Math.ceil(r[1] / binMs) - 1));
  return b > a ? [a, b] : null;
}

/** A link's stretch, waiting for its session (by file name: a file opened by its path is
 * added to the sessions folder under the same name) to open on its Time pane. */
export const rangeOnOpen = signal<{ name: string; range: Range } | null>(null);

/** The stretch each open session's Time pane shows, by path: what "Copy link to this time" puts in the link. */
export const shownRange = signal<Record<string, Range | null>>({});
