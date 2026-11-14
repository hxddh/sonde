// A session's operations as their table reads them: a window of rows over
// however many there are, sorted and narrowed where they are kept. The app
// asks the bridge for pages as the table scrolls (`paged.ts`: a session of a
// hundred thousand operations is never whole in the interface); a shared
// page holds its operations whole and answers from them, here. The table,
// the inspector and what lines output up with operations all read through
// one `Operations`.

import { batch, computed, signal, untracked } from "nsh";
import type { Episode, EpisodeQuery, EpisodeRow, SessionView, WholeSession } from "./types";

/** What narrows and orders the operations: a query without its range. */
export type OpQuery = Omit<EpisodeQuery, "offset" | "limit">;

export interface Operations {
  /** How many operations the query matches: the table's rows. */
  total(): number;
  /** The rows held now: `rows()[k]` is the table's row `start() + k`. */
  start(): number;
  rows(): EpisodeRow[];
  /** The table shows rows `from` up to `to`: have them held, with room either side. */
  show(from: number, to: number): void;
  /** How they are sorted and narrowed, and a change of it. */
  query(): OpQuery;
  ask(q: OpQuery): void;
  /** One operation by its index, with every request's id (the inspector's). */
  episode(index: number): EpisodeRow | null;
  /** The row the table shows an operation at; -1 where the query leaves it out. */
  rowOf(index: number): number;
  /** The operation a request belongs to. */
  ownerOf(recordId: number): number | null;
  /** The operations running at a moment (an output line's), else the nearest one. */
  at(time: string): number[];
  /** The first `limit` rows in the query's order (to copy the table), and how many there are in all. */
  first(limit: number): Promise<{ rows: EpisodeRow[]; total: number }>;
}

const time = (iso: string) => new Date(iso).getTime();

/** An operation of a whole session as a row: its index, how many requests, whether it is a lookup that found nothing. */
export const rowOf = (e: Episode, index: number): EpisodeRow => ({ ...e, index, requests: e.records.length, ...(e.shape === "probe" || e.title.includes("found nothing") ? { miss: true as const } : {}) });

/** A whole session (a shared page's) in the shape a view has: its operations as rows, its span, its families, each finding's count and start. */
export function viewOfWhole<T extends WholeSession>(d: T): Omit<T, "operations"> & SessionView {
  const episodes = d.operations.map(rowOf);
  const formats: Record<string, number> = {};
  const kinds: Record<string, number> = {};
  let a = Infinity;
  let b = -Infinity;
  for (const e of episodes) {
    if (e.format) formats[e.format] = (formats[e.format] ?? 0) + 1;
    kinds[e.shape] = (kinds[e.shape] ?? 0) + 1;
    a = Math.min(a, time(e.start));
    b = Math.max(b, time(e.end));
  }
  return {
    ...d,
    operations: episodes,
    operationsTotal: episodes.length,
    span: episodes.length ? { start: new Date(a).toISOString(), end: new Date(b).toISOString() } : null,
    families: { formats, kinds },
    observations: d.observations.map((o) => ({ ...o, operationsTotal: o.operations.length, firstStart: o.operations.length ? episodes[o.operations[0]]?.start : undefined })),
  };
}

/** A whole session's operations, answered from memory as the bridge answers a paged one: the same order (ties by index), the same narrowing. */
export function wholeOperations(view: () => SessionView): Operations {
  const query = signal<OpQuery>({});
  const all = () => view().operations;
  const t0 = computed(() => all().reduce((m, e) => Math.min(m, time(e.start)), Infinity));
  const order = computed(() => {
    const q = query();
    const v = view();
    let list = q.finding === undefined ? all().map((_, i) => i) : [...(v.observations[q.finding]?.operations ?? [])];
    if (q.slice) list = list.filter((i) => time(all()[i].start) - t0() < q.slice![1] && time(all()[i].end) - t0() >= q.slice![0]);
    if (q.failed) list = list.filter((i) => all()[i].outcome.kind !== "ok");
    const text = q.text?.trim().toLowerCase();
    if (text) list = list.filter((i) => `${all()[i].title ?? ""} ${all()[i].notes.join(" ")}`.toLowerCase().includes(text));
    const by = (i: number) => (q.sort === "duration" ? time(all()[i].end) - time(all()[i].start) : q.sort === "requests" ? all()[i].requests : time(all()[i].start));
    const dir = q.descending ? -1 : 1;
    if (q.sort || q.descending) list.sort((x, y) => (by(x) - by(y)) * dir || x - y);
    return list;
  });
  const rows = computed(() => order().map((i) => all()[i]));
  return {
    total: () => order().length,
    start: () => 0,
    rows,
    show() {},
    query,
    ask: (q) => batch(() => query.set(q)),
    episode: (index) => all()[index] ?? null,
    rowOf: (index) => untracked(order).indexOf(index),
    ownerOf: (id) => {
      const i = all().findIndex((e) => e.records.includes(id));
      return i < 0 ? null : i;
    },
    at(at) {
      const moment = time(at);
      const running = all().flatMap((e, i) => (time(e.start) <= moment && moment <= time(e.end) ? [i] : []));
      if (running.length) return running;
      let best = -1;
      let gap = Infinity;
      all().forEach((e, i) => {
        const d = moment < time(e.start) ? time(e.start) - moment : moment - time(e.end);
        if (d < gap) (gap = d), (best = i);
      });
      return best >= 0 ? [best] : [];
    },
    first: async (limit) => ({ rows: untracked(rows).slice(0, limit), total: untracked(order).length }),
  };
}
