// A run's analysis, read off the interface's thread: the window stays
// live while a large run is read, and a growing one is analysed again
// only once the last reading has finished.

import { computed, effect, resource, untracked } from "nsh";
import { api } from "./native";
import type { RunView } from "./types";

export interface Analysis {
  /** The analysis of the run asked for; null until its first reading ends. */
  view: () => RunView | null;
  /** The first reading of this run is under way. */
  reading: () => boolean;
  /** Why the run could not be read. */
  error: () => unknown;
}

/** `path`'s analysis, read again each time `tick` changes. */
export function analysis(path: () => string | null, tick: () => unknown = () => 0): Analysis {
  const r = resource(path, (p) => api.runs.view(p));
  let first = true;
  // a tick during a reading is kept for when it ends: the last one (a program
  // that just ended) must not be lost
  let pending = false;
  // by its value: what the tick is read from (the list of runs) changing is no tick
  const ticked = computed(tick);
  effect(() => {
    ticked();
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
  // the value is kept while another run loads: never show one for another.
  // Read again (a live run growing), the last reading stays until the next is in:
  // the pane is not emptied and drawn again under its reader
  let last: RunView | null = null;
  const view = () => {
    const v = r();
    if (v && v.path === path()) last = v;
    return last && last.path === path() ? last : null;
  };
  return { view, reading: () => !!path() && !view() && !r.error(), error: () => r.error() };
}
