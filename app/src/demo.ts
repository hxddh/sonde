// The demo, from the first minute: one press runs a small export job on the
// built-in demo store (sonde plays it in-process: nothing to install, on any
// system) and opens its run on its first finding — what was found, the line
// of code that sent it, the fix. "Apply and run again" changes the job's
// code as the fix says and runs it again: the run opens saying the finding
// is fixed, with what it saved measured beside what was predicted.

import { batch } from "nsh";
import { api } from "./native";
import { findingOnOpen, go, openRun, paneOnOpen, report, runsChanged, startDemo } from "./store";
import type { RunView } from "./types";

/** The command the demo's runs ran (sonde_core::tour::COMMAND). */
export const DEMO_COMMAND = "sonde demo-job export.py";

/** Whether a run is the demo's job, on the demo store: its finding can be applied. */
export function isDemoJob(h: RunView["header"] | null | undefined): boolean {
  return !!h && h.endpoint === "demo" && (h.command ?? "").trim() === DEMO_COMMAND;
}

let playing = false;

/** Plays the demo's job as its file stands, and opens its run on its first finding. Whether it ran. */
async function play(cwd: string, command: string): Promise<boolean> {
  if (playing) return false;
  playing = true;
  try {
    const r = await api.runs.run(command, cwd, "demo", "allow", 100, 30, () => {});
    runsChanged();
    batch(() => {
      paneOnOpen.set({ path: r.path, pane: "operations" });
      findingOnOpen.set({ path: r.path, index: 0 });
      openRun.set(r.path);
      go("runs");
    });
    return true;
  } catch (e) {
    report(e);
    return false;
  } finally {
    playing = false;
  }
}

/** The welcome's one press: the demo store started, its job written afresh and run, its run open. */
export async function runDemo(): Promise<boolean> {
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
