// The program sonde runs: one at a time, outliving the screen that started
// it. Its output is kept (the last 2 MB) so a terminal mounted later shows
// it from the start; listeners get what arrives after.

import { announce, batch, effect, lifecycle, Notifications, platform, signal, Storage } from "nsh";
import { GlobalShortcut, isAvailable, Tray, Window } from "nsh/desktop";
import { t } from "./i18n";
import { api } from "./native";
import { area, endpoint, go, notify, openSession, report, sessionsChanged } from "./store";
import type { RunResult } from "./types";
import { iconSvg } from "./ui/icon";
import { programOf } from "./ui/sessionview";
import type { Policy } from "./ui/session";
import { toast } from "./ui/toast";

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
if (isAvailable && platform.idiom !== "terminal") {
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
if (isAvailable && platform.idiom !== "terminal") {
  void GlobalShortcut.register(RUN_SHORTCUT, () => {
    if (running()) return stop();
    const last = history()[0];
    const ep = endpoint();
    if (!last || !ep) return notify(t("run.shortcutNothing"), { kind: "warning" });
    start(last, ep);
    announce(t("run.shortcutStarted", { command: last }));
  }).catch(() => false);
}

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

async function tellTheUser(r: RunResult, cmd: string, killed: boolean) {
  try {
    const v = await api.sessions.analyse(r.path);
    // a run that made no request leaves nothing to read: it is not kept, unless asked
    if (v.summary.requests === 0) {
      let keep = false;
      toast({ id: "run-empty", kind: "info", title: t("run.emptyNotKept"), action: t("run.keepEmpty"), onAction: () => (keep = true), duration: 8000 });
      setTimeout(() => {
        if (keep) return;
        try {
          api.sessions.remove(r.path);
          sessionsChanged();
        } catch {
          // already gone
        }
      }, 8500);
      return;
    }
    const warnings = v.observations.filter((o) => o.level === "warn").length;
    const body = [t("run.bandRequests", { n: v.summary.requests }), t("figure.operationsN", { n: v.episodes.length }), ...(warnings ? [t("run.toLook", { n: warnings })] : [])].join(" · ");
    const title = killed ? t("run.toastStopped", { command: programOf(cmd) ?? cmd }) : t("run.toastEnded", { command: programOf(cmd) ?? cmd, code: String(r.exit ?? "?") });
    // read out to a screen reader wherever the focus is
    announce(`${title}. ${body}`);
    // on Run the band says it; elsewhere a toast does
    if (area() !== "run")
      toast({
        id: "run-ended",
        kind: r.exit === 0 && !warnings ? "success" : "warning",
        title,
        description: body,
        action: t("run.toastOpen"),
        // this run's session, whatever ran since
        onAction: () => {
          openSession.set(r.path);
          go("sessions");
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
    // the session in the id: pressed after the app was quit, it still opens that run
    await Notifications.show({ id: `${NOTIFY}${r.path}`, title: t("run.notifyTitle", { command: programOf(cmd) ?? cmd }), body });
    if (lifecycle() === "active") await Notifications.setBadge(0).catch(() => {});
  } catch {
    // no notifications here (a terminal, an unbundled app): the screen says it
  }
}

/** Runs `cmd` against `endpoint`, its requests recorded. */
export function start(cmd: string, endpoint: string) {
  askToNotify();
  if (running() || !cmd.trim()) return;
  buffer = "";
  killed = false;
  remember(cmd.trim());
  batch(() => {
    lastRun.set(null);
    startedAt.set(Date.now());
    running.set({ command: cmd, path: null, endpoint });
  });
  api.sessions
    .run(cmd, cwd() || null, endpoint, policy(), size.cols, size.rows, (e) => {
      if ("started" in e) {
        running.set({ command: cmd, path: e.started.path, endpoint });
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
        lastRun.set({ ...r, command: cmd, killed });
      });
      sessionsChanged();
      // this run's own: a new run may start before the analysis is back
      void tellTheUser(r, cmd, killed);
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

const NOTIFY = "sonde-run:";

/** The first run asks once whether sonde may say when a run ends (the
 * system asks only when the user did something; starting a run is that). */
function askToNotify() {
  if (platform.idiom === "terminal") return;
  void Notifications.permission()
    .then((p) => (p === "default" ? Notifications.requestPermission() : p))
    .catch(() => {});
}

/** A pressed notification opens the session it told of (the one that
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
