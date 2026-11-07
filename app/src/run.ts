// The program sonde runs: one at a time, outliving the screen that started
// it. Its output is kept (the last 2 MB) so a terminal mounted later shows
// it from the start; listeners get what arrives after.

import { batch, Notifications, signal } from "nsh";
import { t } from "./i18n";
import { api } from "./native";
import { openSession, report } from "./store";
import type { RunResult } from "./types";
import type { Policy } from "./ui/session";

export interface Running {
  command: string;
  path: string | null;
  endpoint: string;
}

/** The running program, or null. */
export const running = signal<Running | null>(null);
/** How the last run ended. */
export const lastRun = signal<(RunResult & { command: string; killed: boolean }) | null>(null);
/** The newest command, for the field. */
export const command = signal("");
export const cwd = signal("");
export const policy = signal<Policy>("allow");

const MAX = 2 * 1024 * 1024;
let buffer = "";
let killed = false;
const listeners = new Set<(data: string) => void>();

/** Everything the program printed, so far (up to 2 MB). */
export const output = () => buffer;

/** Calls `f` with each piece of output from now on; returns the unsubscribe. */
export function onOutput(f: (data: string) => void): () => void {
  listeners.add(f);
  return () => listeners.delete(f);
}

let size = { cols: 100, rows: 30 };
/** The terminal's size: told to the program when it runs. */
export function resize(cols: number, rows: number) {
  size = { cols, rows };
  if (running()) api.sessions.resize(cols, rows);
}

async function tellTheUser(r: RunResult, cmd: string) {
  try {
    const v = api.sessions.open(r.path);
    const warnings = v.observations.filter((o) => o.level === "warn").length;
    const body = `${v.summary.requests} requests · ${v.episodes.length} operations${warnings ? ` · ${warnings} to look at` : ""}`;
    if ((await Notifications.requestPermission()) !== "granted") return;
    await Notifications.show({ id: "sonde-run", title: t("run.notifyTitle", { command: cmd.split(" ")[0] }), body });
  } catch {
    // no notifications here (a terminal, an unbundled app): the screen says it
  }
}

/** Runs `cmd` against `endpoint`, its requests recorded. */
export function start(cmd: string, endpoint: string) {
  if (running() || !cmd.trim()) return;
  buffer = "";
  killed = false;
  batch(() => {
    lastRun.set(null);
    running.set({ command: cmd, path: null, endpoint });
  });
  api.sessions
    .run(cmd, cwd() || null, endpoint, policy(), size.cols, size.rows, (e) => {
      if ("started" in e) {
        running.set({ command: cmd, path: e.started.path, endpoint });
      } else if ("data" in e) {
        buffer += e.data;
        if (buffer.length > MAX) buffer = buffer.slice(buffer.length - MAX);
        for (const l of listeners) l(e.data);
      }
    })
    .then((r) => {
      batch(() => {
        running.set(null);
        lastRun.set({ ...r, command: cmd, killed });
      });
      void tellTheUser(r, cmd);
    })
    .catch((e) => {
      running.set(null);
      report(e);
    });
}

/** Ends the running program. */
export function stop() {
  killed = true;
  api.sessions.kill();
}

/** Text typed into the program's terminal. */
export function input(data: string) {
  if (running()) api.sessions.input(data);
}

/** Opens the last run in Sessions, lined up with its output. */
export function openLastRun(go: (s: "sessions") => void) {
  const r = lastRun();
  if (!r) return;
  openSession.set(r.path);
  go("sessions");
}
