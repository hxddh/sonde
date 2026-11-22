// The program sonde runs: one at a time, outliving the screen that started
// it. Its output is kept (the last 2 MB) so a terminal mounted later shows
// it from the start; listeners get what arrives after.

import { announce, batch, effect, lifecycle, Notifications, signal, Storage } from "nsh";
import { GlobalShortcut, isAvailable, Tray, Window } from "nsh/desktop";
import { t } from "./i18n";
import { api } from "./native";
import { area, endpoint, go, notify, openRun, report, runsChanged, wasRead } from "./store";
import type { RunResult } from "./types";
import { iconSvg } from "./ui/icon";
import { programOf, scrubCommand } from "./ui/runview";
import type { Policy } from "./ui/runparts";
import { toast } from "./ui/toast";

export interface Running {
  command: string;
  path: string | null;
  endpoint: string;
}

/** The running program, or null. */
export const running = signal<Running | null>(null);
/** How the last run ended. */
export const lastRun = signal<(RunResult & { command: string; killed: boolean; /** its recording is gone: not kept (it sent nothing), or deleted */ removed?: boolean; /** deleted by the user */ deleted?: boolean; endpoint: string }) | null>(null);

/** A run deleted: if it was the last one, it is no longer said to be. */
export function forgetRun(path: string) {
  const last = lastRun();
  if (last?.path === path) lastRun.set({ ...last, removed: true, deleted: true });
}
/** The newest command, for the field. */
export const command = signal("");
/** When the running program started (ms since the epoch). */
export const startedAt = signal<number | null>(null);

/** The commands run before, newest first (kept across launches). */
export const history = signal<string[]>([]);
const HISTORY_KEY = "run.history";
void Storage.getItem(HISTORY_KEY)
  .then((v) => v && history.set(JSON.parse(v)))
  .catch(() => {});
function remember(cmd: string) {
  const next = [cmd, ...history().filter((c) => c !== cmd)].slice(0, 20);
  history.set(next);
  void Storage.setItem(HISTORY_KEY, JSON.stringify(next)).catch(() => {});
}

// While a program runs, the tray says so: a press brings the window back.
if (isAvailable) {
  effect(() => {
    const r = running();
    try {
      Tray.set(
        r
          ? {
              icon: iconSvg("circle-dot"),
              tooltip: t("run.trayTip", { command: programOf(r.command) ?? "" }),
              onClick: () => Window.get("main")?.show(),
              menu: [
                { label: t("run.trayShow"), onSelect: () => Window.get("main")?.show() },
                { label: t("run.stop"), onSelect: stop },
              ],
            }
          : null,
      );
    } catch {
      // no tray on this desktop
    }
  });
}
/** The shortcut that starts the last command, or stops the running one, from
 * any app: ⌘⌥R / Ctrl+Alt+R. */
export const RUN_SHORTCUT = "mod+alt+r";
if (isAvailable) {
  void GlobalShortcut.register(RUN_SHORTCUT, () => {
    if (running()) return stop();
    const last = history()[0];
    const ep = endpoint();
    if (!last || !ep) return notify(t("run.shortcutNothing"), { kind: "warning" });
    start(last, ep);
    announce(t("run.shortcutStarted", { command: scrubCommand(last) }));
  }).catch(() => false);
}

export const cwd = signal("");
/** What the program may do to the store: the command bar's choice, allow unless
 * changed (a project's `[record] policy` in sonde.toml narrows it). */
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
  if (running()) api.runs.resize(cols, rows);
}

async function tellTheUser(r: RunResult, cmd: string, killed: boolean) {
  try {
    const v = await api.runs.view(r.path);
    // a run that made no request leaves nothing to read: it is not kept, unless asked
    if (v.summary.requests === 0) {
      let keep = false;
      toast({ id: "run-empty", kind: "info", title: t("run.emptyNotKept"), action: t("run.keepEmpty"), onAction: () => (keep = true), duration: 8000 });
      setTimeout(() => {
        // kept, or opened meanwhile by any way in: never removed under its reader
        if (keep || wasRead(r.path)) return;
        void api.runs
          .remove(r.path)
          .then(() => {
            // nothing left to open
            const last = lastRun();
            if (last?.path === r.path) lastRun.set({ ...last, removed: true });
            runsChanged();
          })
          .catch(() => {
            // already gone
          });
      }, 8500);
      return;
    }
    const warnings = v.observations.filter((o) => o.level === "warn").length;
    const body = [t("run.bandRequests", { n: v.summary.requests }), t("figure.operationsN", { n: v.operationsTotal }), ...(warnings ? [t("run.toLook", { n: warnings })] : [])].join(" · ");
    const title = killed ? t("run.toastStopped", { command: programOf(cmd) ?? scrubCommand(cmd) }) : t("run.toastEnded", { command: programOf(cmd) ?? scrubCommand(cmd), code: String(r.exit ?? "?") });
    // read out to a screen reader wherever the focus is
    announce(`${title}. ${body}`);
    // in Runs its own view says it; elsewhere a toast does
    if (area() !== "runs")
      toast({
        id: "run-ended",
        kind: r.exit === 0 && !warnings ? "success" : "warning",
        title,
        description: body,
        action: t("run.toastOpen"),
        // this run, whatever ran since
        onAction: () => {
          openRun.set(r.path);
          go("runs");
        },
        duration: 8000,
      });
    // away from the window: a failed run asks for attention
    if (lifecycle() !== "active" && !killed && (r.exit !== 0 || warnings > 0)) {
      try {
        // a failure bounces until seen; warnings ask once
        Window.get("main")?.requestAttention({ critical: r.exit !== 0 && r.exit !== null });
      } catch {
        // no windows here
      }
    }
    // what is left to look at, on the Dock's icon or the taskbar button, until the window is back
    if (lifecycle() !== "active" && warnings > 0) await Notifications.setBadge(warnings).catch(() => {});
    // asked for when the run began (something the user did), not now
    if ((await Notifications.permission()) !== "granted") return;
    // the run in the id: pressed after the app was quit, it still opens that run
    await Notifications.show({ id: `${NOTIFY}${r.path}`, title: t("run.notifyTitle", { command: programOf(cmd) ?? scrubCommand(cmd) }), body });
    if (lifecycle() === "active") await Notifications.setBadge(0).catch(() => {});
  } catch {
    // no notifications here (an unbundled app): the screen says it
  }
}

/** Runs a run's command again, on its endpoint, where it ran before (when the run says): change the program, then
 * its new run (the newest, shown as it goes) says what the change did since the run before. */
export function runAgain(cmd: string, endpoint: string, dir?: string | null) {
  if (running() || !cmd.trim()) return;
  command.set(cmd);
  if (dir) cwd.set(dir);
  start(cmd, endpoint);
}

/** A run asked for while another program runs: said, with its Stop (one program at a time). */
export function busy() {
  notify(t("run.busy"), { kind: "warning", action: t("run.stop"), onAction: stop });
}

/** Runs `cmd` against `endpoint`, its requests recorded; whether it started. */
export function start(cmd: string, endpoint: string): boolean {
  askToNotify();
  if (running() || !cmd.trim()) return false;
  buffer = "";
  killed = false;
  remember(cmd.trim());
  batch(() => {
    lastRun.set(null);
    startedAt.set(Date.now());
    running.set({ command: cmd, path: null, endpoint });
  });
  api.runs
    .run(cmd, cwd() || null, endpoint, policy(), size.cols, size.rows, (e) => {
      if ("started" in e) {
        running.set({ command: cmd, path: e.started.path, endpoint });
        // the run is the newest in the list, and opens there as it goes
        batch(() => {
          openRun.set(e.started.path);
          runsChanged();
        });
      } else if ("data" in e) {
        buffer += e.data;
        if (buffer.length > MAX) {
          // cut at a line, so no escape sequence or surrogate pair is split
          const from = buffer.indexOf("\n", buffer.length - MAX);
          let at = from < 0 ? buffer.length - MAX : from + 1;
          // never start on the second half of a pair
          if (/[\udc00-\udfff]/.test(buffer[at] ?? "")) at += 1;
          buffer = buffer.slice(at);
        }
        for (const l of listeners) l(e.data);
      }
    })
    .then((r) => {
      batch(() => {
        running.set(null);
        lastRun.set({ ...r, command: cmd, killed, endpoint });
      });
      runsChanged();
      // this run's own: a new run may start before the analysis is back
      void tellTheUser(r, cmd, killed);
    })
    .catch((e) => {
      running.set(null);
      report(e);
    });
  return true;
}

/** Ends the running program (one still starting, as soon as it runs): said "Stopped" only when it was. */
export function stop() {
  if (api.runs.kill()) killed = true;
}

/** Text typed into the program's terminal. */
export function input(data: string) {
  if (running()) api.runs.input(data);
}

const NOTIFY = "sonde-run:";

/** The first run asks once whether sonde may say when a run ends (the
 * system asks only when the user did something; starting a run is that). */
function askToNotify() {
  void Notifications.permission()
    .then((p) => (p === "default" ? Notifications.requestPermission() : p))
    .catch(() => {});
}

/** A pressed notification opens the run it told of (the one that
 * started the app too); the badge goes as the window comes back. */
export function followNotifications(open: (path: string) => void) {
  try {
    Notifications.on("press", ({ id }) => id.startsWith(NOTIFY) && open(id.slice(NOTIFY.length)));
  } catch {
    // no notifications here
  }
  effect(() => {
    if (lifecycle() === "active") void Notifications.setBadge(0).catch(() => {});
  });
}

/** The app is quitting: ends the program being recorded (and what it started)
 * and a running trial, and has the recording written as a finished run.
 * Resolves once the file is whole; the system waits 200 ms for it. A trial
 * cut short keeps nothing, as when it is stopped by hand. */
export async function endForQuit(): Promise<void> {
  try {
    await api.runs.quit();
  } catch {
    // nothing was being recorded, or the bridge has gone first: there is nothing left to end
  }
}
