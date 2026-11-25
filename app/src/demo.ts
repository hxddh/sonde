// The demo, from the first minute: one press runs a small export job on the
// built-in demo store (sonde plays it in-process: nothing to install, on any
// system), its run open as it goes, then on its first finding — what was found, the line
// of code that sent it, the fix. "Apply and run again" changes the job's
// code as the fix says and runs it again: the run opens saying the finding
// is fixed, with what it saved measured beside what was predicted.

import { batch } from "nsh";
import { DEMO_COMMAND } from "./command";
import { api } from "./native";
import { busy, launch, running } from "./run";
import { findingOnOpen, go, openRun, paneOnOpen, report, shownRun, startDemo } from "./store";
import type { RunView } from "./types";

/** Whether a run is the demo's job, on the demo store: its finding can be applied. */
export function isDemoJob(h: RunView["header"] | null | undefined): boolean {
  return !!h && h.endpoint === "demo" && (h.command ?? "").trim() === DEMO_COMMAND;
}

/** Plays the demo's job as its file stands: its run opens as it starts, as any program's does, and once it has
 * ended on its first finding, if it is still the run shown (one the user went to meanwhile stays). Whether it ran. */
async function play(cwd: string, command: string): Promise<boolean> {
  go("runs");
  const r = await launch("demo", command, "demo", cwd);
  if (!r) return false;
  if (shownRun() !== r.path) return true;
  batch(() => {
    paneOnOpen.set({ path: r.path, pane: "operations" });
    findingOnOpen.set({ path: r.path, index: 0 });
    openRun.set(r.path);
    go("runs");
  });
  return true;
}

/** The welcome's one press: the demo store started, its job written afresh and run, its run open. */
export async function runDemo(): Promise<boolean> {
  // one thing at work at a time: its job is not written afresh under another
  if (running()) return (busy(), false);
  if (!(await startDemo())) return false;
  try {
    const job = api.runs.demoJob();
    return await play(job.cwd, job.command);
  } catch (e) {
    report(e);
    return false;
  }
}

/** The demo's finding applied to its job's code, and the job run again (in the folder its run ran in). */
export async function applyDemoFix(cwd: string | null | undefined): Promise<boolean> {
  if (running()) return (busy(), false);
  try {
    if (!(await startDemo())) return false;
    // (the folder the run ran in: the job's own, written first where the run does not say)
    const dir = cwd || api.runs.demoJob().cwd;
    api.runs.demoFix();
    return await play(dir, DEMO_COMMAND);
  } catch (e) {
    report(e);
    return false;
  }
}
