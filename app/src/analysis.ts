// A session's analysis, read off the interface's thread: the window stays
// live while a large session is read, and a growing one is analysed again
// only once the last reading has finished.

import { effect, resource, untracked } from "nsh";
import { api } from "./native";
import type { SessionView } from "./types";

export interface Analysis {
  /** The analysis of the session asked for; null until its first reading ends. */
  view: () => SessionView | null;
  /** The first reading of this session is under way. */
  reading: () => boolean;
  /** Why the session could not be read. */
  error: () => unknown;
}

/** `path`'s analysis, read again each time `tick` changes. */
export function analysis(path: () => string | null, tick: () => unknown = () => 0): Analysis {
  const r = resource(path, (p) => api.sessions.analyse(p));
  let first = true;
  // a tick during a reading is kept for when it ends: the last one (a program
  // that just ended) must not be lost
  let pending = false;
  effect(() => {
    tick();
    if (first) return void (first = false);
    untracked(() => {
      if (!path()) return;
      if (r.loading()) pending = true;
      else r.refetch();
    });
  });
  effect(() => {
    if (r.loading() || !pending) return;
    pending = false;
    untracked(() => path() && r.refetch());
  });
  // the value is kept while another session loads: never show one for another
  const view = () => {
    const v = r();
    return v && v.path === path() ? v : null;
  };
  return { view, reading: () => !!path() && !view() && !r.error(), error: () => r.error() };
}
