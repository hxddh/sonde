// A session's operations from the bridge, in pages: what the app's tables
// read (`operations.ts` has the interface, and the whole-session source a
// shared page uses).

import { effect, signal, untracked } from "nsh";
import { api } from "./native";
import type { Operations, OpQuery } from "./operations";
import type { EpisodePage, EpisodeRow, SessionView } from "./types";

/** Rows asked for at a time, and how near the held rows' edge the table may come before more are asked for. */
export const PAGE = 500;
const WINDOW = 3 * PAGE;
const MARGIN = PAGE / 2;

const same = (a: OpQuery, b: OpQuery) => JSON.stringify(a) === JSON.stringify(b);

/** A session's operations from the bridge, in pages: `view`'s first page at
 * once, the rest as the table scrolls, sorts or narrows. A growing session's
 * view moves `episodesTotal`: the rows held are asked for again. */
export function pagedOperations(path: () => string, view: () => SessionView | null): Operations {
  const query = signal<OpQuery>({});
  const page = signal<EpisodePage>({ total: 0, offset: 0, rows: [] });
  // the last answer wins: a page asked for before a newer query is dropped
  let asked = 0;
  const plain = (q: OpQuery) => !q.sort && !q.descending && q.finding === undefined && !q.slice && !q.text && !q.failed;
  // (the same range of the same order is asked for once: a table that reports its rows again does not ask again)
  let last = "";
  // the range on its way, if one is: a table scrolling on within it waits for it, it does not ask again
  let coming: { offset: number; limit: number } | null = null;
  const fetch = (offset: number, limit: number, again = false) => {
    const q = query();
    const key = JSON.stringify([q, offset, limit]);
    if (key === last && !again) return;
    last = key;
    coming = { offset, limit };
    const n = ++asked;
    api.sessions
      .episodes(path(), { ...q, offset, limit })
      .then((p) => {
        if (n !== asked) return;
        coming = null;
        page.set(p);
      })
      .catch(() => {
        // the file went, or is being written: the rows held stay until the view says what is there
      });
  };
  // the view arrives, or (a live session) arrives again: its first page is the plain order's start
  let seen = -1;
  effect(() => {
    const v = view();
    if (!v) return;
    untracked(() => {
      const moved = v.episodesTotal !== seen;
      seen = v.episodesTotal;
      const at = page();
      if (plain(query()) && at.offset === 0) {
        asked++;
        last = "";
        page.set({ total: v.episodesTotal, offset: 0, rows: v.episodes });
      } else if (moved) fetch(at.offset, Math.max(PAGE, at.rows.length), true);
    });
  });
  return {
    total: () => page().total,
    start: () => page().offset,
    rows: () => page().rows,
    show(from, to) {
      // (a table with no window of its own reports every row: what it needs first is its start)
      to = Math.min(to, from + PAGE);
      const at = untracked(page);
      // held (or on its way), with room to scroll into: nothing to ask
      const within = (offset: number, count: number, margin: number) => from >= offset && to <= offset + count && (from - offset >= margin || offset === 0) && (offset + count - to >= margin || offset + count >= at.total);
      if (within(at.offset, at.rows.length, MARGIN)) return;
      if (coming && within(coming.offset, coming.limit, 0)) return;
      const mid = Math.floor((from + to) / 2);
      fetch(Math.max(0, mid - WINDOW / 2), WINDOW);
    },
    query,
    ask(q) {
      if (same(q, untracked(query))) return;
      query.set(q);
      const v = untracked(view);
      if (plain(q) && v) {
        asked++;
        last = "";
        page.set({ total: v.episodesTotal, offset: 0, rows: v.episodes });
      } else fetch(0, PAGE, true);
    },
    episode(index) {
      try {
        return api.sessions.episode(path(), index);
      } catch {
        return null;
      }
    },
    rowOf(index) {
      try {
        return api.sessions.episodeRow(path(), untracked(query), index);
      } catch {
        return -1;
      }
    },
    ownerOf(recordId) {
      try {
        return api.sessions.episodeOf(path(), recordId);
      } catch {
        return null;
      }
    },
    at(time) {
      try {
        return api.sessions.episodesAt(path(), time);
      } catch {
        return [];
      }
    },
    async first(limit) {
      const out: EpisodeRow[] = [];
      let total = 0;
      // (a call answers with 5000 at most)
      while (out.length < limit) {
        const p = await api.sessions.episodes(path(), { ...untracked(query), offset: out.length, limit: Math.min(5000, limit - out.length) });
        total = p.total;
        out.push(...p.rows);
        if (!p.rows.length || out.length >= p.total) break;
      }
      return { rows: out, total };
    },
  };
}
