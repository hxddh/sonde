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
  // (with the session it is of: a page of another run is never kept for this one)
  const page = signal<EpisodePage & { path?: string }>({ total: 0, offset: 0, rows: [] });
  // the last answer wins: a page asked for before a newer query is dropped
  let asked = 0;
  const plain = (q: OpQuery) => !q.sort && !q.descending && q.finding === undefined && !q.slice && !q.text && !q.failed;
  // (the same range of the same order is asked for once: a table that reports its rows again does not ask again)
  let last = "";
  // the range on its way, if one is: a table scrolling on within it waits for it, it does not ask again
  let coming: { offset: number; limit: number } | null = null;
  const fetch = (offset: number, limit: number, again = false) => {
    const q = query();
    // (of this session: a new run in Run's one table, sorted the same and as long, is asked for again)
    const key = JSON.stringify([path(), q, offset, limit]);
    if (key === last && !again) return;
    last = key;
    coming = { offset, limit };
    const n = ++asked;
    const at = path();
    api.sessions
      .episodes(at, { ...q, offset, limit })
      .then((p) => {
        if (n !== asked) return;
        coming = null;
        page.set({ ...p, path: at });
      })
      .catch(() => {
        // the file went, or is being written: the rows held stay until the view says what is there,
        // and the range is not taken as asked (scrolling within it asks again)
        if (n !== asked) return;
        coming = null;
        last = "";
      });
  };
  // the view arrives, or (a live session) arrives again: its first page is the plain order's start
  let seen = -1;
  let seenPath = "";
  effect(() => {
    const v = view();
    if (!v) return;
    untracked(() => {
      const moved = v.episodesTotal !== seen || v.path !== seenPath;
      seen = v.episodesTotal;
      seenPath = v.path;
      const at = page();
      // the view's first page stands in for what is held only where it holds as much: a live
      // session scrolled past its first page keeps its rows, asked for again as it grows
      if (plain(query()) && at.offset === 0 && (at.rows.length <= v.episodes.length || v.path !== at.path)) {
        asked++;
        last = "";
        page.set({ total: v.episodesTotal, offset: 0, rows: v.episodes, path: v.path });
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
        page.set({ total: v.episodesTotal, offset: 0, rows: v.episodes, path: v.path });
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
