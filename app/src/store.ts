// App-wide state: endpoints, the current endpoint and bucket, settings,
// errors, and navigation between sections. Rust owns the data; these
// signals mirror it and are refreshed after each change.

import { batch, cleanup, Clipboard, computed, type JSX, keymap, lifecycle, Picker, signal, untracked, type ViewRef } from "nsh";
import { rangeOf, rangeOnOpen } from "./range";
import { api, asError } from "./native";
import { language, t } from "./i18n";
import { bytes } from "./format";
import { core, endpointName } from "./phrases";
import { toast, type ToastKind } from "./ui/toast";
import type { EndpointView, Settings, SondeError } from "./types";

/** The app's places: Runs and Buckets ("store"); Activity and Settings,
 * which the window shows over its place (a sheet, a window of their own). */
export type Area = "runs" | "store" | "activity" | "settings";
/** The tabs of Store: what is in a bucket. */
export type StoreTab = "browse" | "capacity" | "multipart" | "layouts";
/** The tabs of Settings. */
export type SettingsTab = "settings" | "endpoints";
/** The tabs of Activity: what sonde itself did. */
export type ActivityTab = "jobs" | "audit";
/** Where to go: an area, or a tab of one. */
export type Section = "runs" | StoreTab | SettingsTab | ActivityTab;

/** Every place, as the palette lists them. */
export const AREAS: Area[] = ["runs", "store", "activity", "settings"];
export const STORE_TABS: StoreTab[] = ["browse", "capacity", "multipart", "layouts"];
export const SETTINGS_TABS: SettingsTab[] = ["settings", "endpoints"];
export const ACTIVITY_TABS: ActivityTab[] = ["jobs", "audit"];

export function areaOf(s: Section): Area {
  if (s === "runs") return s;
  if ((STORE_TABS as string[]).includes(s)) return "store";
  if ((ACTIVITY_TABS as string[]).includes(s)) return "activity";
  return "settings";
}

const stored = signal<EndpointView[]>([]);
/** The endpoints, each under the name the reader sees (the built-in demo's is in the app's language). */
export const endpoints = computed(() => stored().map((e) => ({ ...e, name: endpointName(e), credentials: core(e.credentialsText, e.credentials) })));
export const settings = signal<Settings | null>(null);
export const endpoint = signal<string | null>(null);
export const bucket = signal<string | null>(null);
export const prefix = signal<string>("");
export const section = signal<Section>("runs");
export const storeTab = signal<StoreTab>("browse");
/** The detail of a bucket a window shows in a sheet over its objects. */
export const settingsTab = signal<SettingsTab>("settings");
export const activityTab = signal<ActivityTab>("jobs");
/** The area on screen. */
export const area = () => areaOf(section());
/** The run the Runs place shows (set by the palette, a notification, a link, …). */
export const openRun = signal<string | null>(null);
/** The run the main window's Runs place has on screen, while it is there: one that ends there needs no toast. */
export const shownRun = signal<string | null>(null);
/** A command of what is on screen (a run's Try under faults…, a Buckets section's Scan again): its place shows
 * it (a button, or behind ⋯), the palette lists it while it is shown, and its shortcut runs it. `run` is given
 * the button a menu would open under, where there is one. */
export type PlaceCommand = { id: string; label: string; icon: import("./ui/lucide").IconName; shortcut?: string; disabled?: boolean; run: (anchor?: ViewRef) => void };
/** The commands on screen, by the group the palette lists them under. */
export const shownCommands = signal<{ group: () => string; list: () => PlaceCommand[] }[]>([]);
/** Offers `list` while the component that calls it is on screen: in the palette under `group`, and each by its
 * shortcut (bound for the commands the list holds as it is first read; one disabled does nothing). */
export function offerCommands(group: () => string, list: () => PlaceCommand[]) {
  const entry = { group, list };
  shownCommands.set([...untracked(shownCommands), entry]);
  cleanup(() => shownCommands.set(untracked(shownCommands).filter((e) => e !== entry)));
  const keys = [...new Set(untracked(list).flatMap((c) => (c.shortcut ? [c.shortcut] : [])))];
  if (keys.length)
    keymap.bind(
      Object.fromEntries(
        keys.map((k) => [
          k,
          () => {
            const c = untracked(list).find((x) => x.shortcut === k);
            if (c && !c.disabled) c.run();
          },
        ]),
      ),
    );
}
/** The runs read since launch, whatever way led to them (the Runs
 * place, a window of their own, a comparison): every reader says so here. */
const read = new Set<string>();
export const reading = (path: string) => void read.add(path);
/** Whether `path` is being read or was, or is on its way to being: what
 * tidies up after a run asks this one question before it removes a run. */
export const wasRead = (path: string) => read.has(path) || openRun() === path;
/** The command bar asked for (⌘N, File › New Run, the sidebar's Runs pressed in Runs): Runs shows it, whatever run
 * it showed. */
export const newRunAsked = signal(false);
export function newRun() {
  batch(() => (openRun.set(null), newRunAsked.set(true)));
  go("runs");
}
/** A pane to show a run on when it opens: a trial tried again opens its fresh run on its runs; the demo's, on its operations. */
export const paneOnOpen = signal<{ path: string; pane: "runs" | "operations" } | null>(null);
/** A finding to open with a run (its index): the demo's run opens on its first. */
export const findingOnOpen = signal<{ path: string; index: number } | null>(null);
/** Bumped when the list of runs changes (a run ended, an example opened, a replay). */
/** The window is out of sight (minimized, hidden, another space): polls rest until it is back. */
export const away = () => lifecycle() === "background";

export const runsTick = signal(0);
export const runsChanged = () => runsTick.set(runsTick() + 1);
/** Settings › Endpoints opens on its form when set (the welcome's "Add an endpoint"). */
export const endpointsMode = signal<"list" | "add" | "import">("list");
/** The main window's width, as its shell measures it (windowSize() is the app's first window's, wherever it is
 * read: a dialog in another window measures its own). */
export const shellWidth = signal(1280);
/** The window's height, as its shell is laid out: what a surface over it may take. */
export const shellHeight = signal(820);
/** What the sidebar lists under the places: the current place's own list
 * (its runs, its buckets), given by the place
 * while it is on screen. One left column for the whole window. */
export const sideList = signal<{ key: string; render: () => JSX.Element } | null>(null);
/** Gives the sidebar a place's list while the place is on screen; returns
 * what takes it back. */
export function showInSidebar(key: string, render: () => JSX.Element): () => void {
  const mine = { key, render };
  sideList.set(mine);
  return () => {
    if (sideList() === mine) sideList.set(null);
  };
}

/** Makes `id` the endpoint; a bucket of another endpoint is no longer chosen
 * (one step: nothing lists the old bucket on the new endpoint). */
export function useEndpoint(id: string | null) {
  if (id === endpoint()) return;
  batch(() => {
    endpoint.set(id);
    bucket.set(null);
    prefix.set("");
  });
}

export function refreshEndpoints() {
  const list = api.endpoints();
  batch(() => {
    stored.set(list);
    if (endpoint() && !list.some((e) => e.id === endpoint())) useEndpoint(null);
    if (!endpoint() && list.length > 0) useEndpoint(list[0].id);
  });
}

export function refreshSettings() {
  const s = api.settings();
  batch(() => {
    settings.set(s);
    language.set(s.language);
  });
}

/** What went wrong, in a line: what sonde says, but that one thing is at work already in the app's own words. */
export const errorTitle = (err: SondeError): string => (err.kind === "busy" ? t("run.busy") : err.message);

/** Shows `e` as an error toast: what went wrong, then the store's own word for it (its code and
 * status) and the request id to quote; returns it. */
export function report(e: unknown): SondeError {
  const err = asError(e);
  if (err.kind === "busy") toast({ id: "error", kind: "warning", title: errorTitle(err) });
  else if (err.kind !== "cancelled") {
    const code = [err.code, err.status ? `${err.status}` : null].filter(Boolean).join(" ");
    const said = [code && !err.message.includes(err.code ?? code) ? code : null, err.requestId ? `${t("common.requestId")}: ${err.requestId}` : null].filter(Boolean).join(" · ");
    toast({ id: "error", kind: "error", title: errorTitle(err), description: said || undefined });
  }
  return err;
}

/** A short notice (copied, exported, saved), as a toast. */
export function notify(text: string, options: { kind?: ToastKind; action?: string; onAction?: () => void } = {}) {
  toast({ title: text, kind: options.kind ?? "success", action: options.action, onAction: options.onAction });
}

/** Copies `text` and says so: the one way anything is copied. */
export function copy(text: string, said: string = t("common.copied")) {
  void Clipboard.writeText(text).then(() => notify(said), report);
}

/** Where a window shows Settings and Activity instead of a place: set by the
 * desktop shell; true when it took `s`. */
let overlay: ((s: Section) => boolean) | null = null;
export function setOverlay(f: (s: Section) => boolean) {
  overlay = f;
}

/** Goes to an area (the tab it had) or to a tab of one. */
export function go(s: Section | Area) {
  batch(() => {
    if (s === "store") s = storeTab();
    else if (s === "activity") s = activityTab();
    else if (s === "settings" && areaOf(section()) !== "settings") s = settingsTab();
    const target = s as Section;
    if ((STORE_TABS as string[]).includes(target)) storeTab.set(target as StoreTab);
    else if ((SETTINGS_TABS as string[]).includes(target)) settingsTab.set(target as SettingsTab);
    else if ((ACTIVITY_TABS as string[]).includes(target)) activityTab.set(target as ActivityTab);
    if (overlay?.(target)) return;
    section.set(target);
  });
}

export function chooseBucket(b: string | null) {
  batch(() => {
    bucket.set(b);
    prefix.set("");
  });
}

/** Starts the built-in demo server and makes it the endpoint. */
export async function startDemo(): Promise<boolean> {
  try {
    const e = await api.startDemo();
    refreshEndpoints();
    useEndpoint(e.id);
    return true;
  } catch (e) {
    report(e);
    return false;
  }
}

/** Opens the example (a SlateDB program's run, built in) in Runs. */
export function openExample() {
  try {
    openRun.set(api.runs.example());
    runsChanged();
    go("runs");
  } catch (e) {
    report(e);
  }
}

export function init() {
  try {
    refreshSettings();
    refreshEndpoints();
  } catch (e) {
    report(e);
  }
}

/** Asks for a `.sonde` file (one recorded elsewhere, or shared) and opens it in Runs. */
/** A local path from a `file:` URL (a file manager's drop). */
export function pathOf(uri: string): string {
  let rest = uri.replace(/^file:/i, "");
  // file://host/share/x is a UNC path; file:///x and file://localhost/x are local
  const unc = /^\/\/(?!localhost\/)[^/]+\//i.test(rest);
  rest = rest.replace(/^\/\/(localhost)?(?=\/)/i, "");
  let p: string;
  try {
    p = decodeURIComponent(rest);
  } catch {
    // a stray % is part of the name; the other escapes still decode
    p = rest.replace(/(%[0-9A-Fa-f]{2})+/g, (m) => {
      try {
        return decodeURIComponent(m);
      } catch {
        return m;
      }
    });
  }
  if (unc) return `\\\\${p.replace(/^\/\//, "").replace(/\//g, "\\")}`;
  // file:///C:/x on Windows
  return /^\/[A-Za-z]:/.test(p) ? p.slice(1) : p;
}

/** A path a link may open: a run file (`.sonde`, `.sonde.gz`) on this
 * machine. A UNC path (`\\host\share`, `//host/share`) would have Windows
 * reach that host, with the user's credentials, before anything is read. */
export function linkable(path: string): boolean {
  return /\.sonde(\.gz)?$/i.test(path) && !/^[\\/]{2}/.test(path);
}

/**
 * A `sonde://` link (from `sonde show --app`, a shared page, the docs). Its
 * URL is anyone's to make: it only opens what sonde would open itself.
 *   sonde://open?path=/abs/file.sonde   a run file, added as a drop is
 *   sonde://run/<file name>         a run of the runs folder
 *   …&range=120-480 / ?range=120-480     either, on its Time pane over that stretch (ms from its start)
 *   sonde://bucket/<endpoint>/<bucket>[/<prefix>]
 */
export function openLink(url: string) {
  const m = /^sonde:\/\/([a-z]+)(\/[^?#]*)?(\?[^#]*)?/i.exec(url.trim());
  if (!m) return;
  const [, what, rest = "", query = ""] = m;
  // a stretch of the run's time: its Time pane opens over it
  const range = rangeOf(query);
  const part = (s: string) => {
    try {
      return decodeURIComponent(s);
    } catch {
      return s;
    }
  };
  if (what === "open") {
    // the value after the first "=", decoded once
    const kv = query.slice(1).split("&").find((x) => x.startsWith("path="));
    const path = kv === undefined ? undefined : part(kv.slice(5).replace(/\+/g, "%20"));
    if (!path || !linkable(path)) return;
    if (range) rangeOnOpen.set({ name: path.split(/[\\/]/).pop() ?? path, range });
    void openPaths([path]);
  } else if (what === "run") {
    const name = part(rest.slice(1));
    const s = api.runs.list().find((x) => x.name === name);
    if (!s) return notify(t("link.noRun", { name }), { kind: "warning" });
    batch(() => {
      if (range) rangeOnOpen.set({ name, range });
      openRun.set(s.path);
      go("runs");
    });
  } else if (what === "bucket") {
    const [ep, b, ...p] = rest.slice(1).split("/").map(part);
    const folder = p.filter(Boolean).join("/");
    if (!endpoints().some((e) => e.id === ep)) return notify(t("link.noEndpoint", { name: ep ?? "" }), { kind: "warning" });
    if (!b) return;
    batch(() => {
      endpoint.set(ep);
      chooseBucket(b);
      if (folder) prefix.set(`${folder}/`);
      go("store");
    });
  }
}

/** A second launch handed to the running app (nsh.json's `singleInstance`):
 * the files among its arguments open here, as a drop does. Its `sonde://`
 * links come through `UrlScheme` like every other link, and options are not
 * read. Anyone can start the app with any arguments: only what a drop would
 * open is opened. */
export function openLaunch(args: string[], cwd: string) {
  const files = once(launchFiles(args, cwd));
  if (files.length) void openPaths(files);
}

/** The files the system opened the app with (a double click on a `.sonde`,
 * Open With, a drop on the app's icon; those that launched it included):
 * opened as a drop on the window is. */
export function openSystemFiles(paths: string[]) {
  const files = once(paths.filter(Boolean));
  if (files.length) void openPaths(files);
}

/** Of `paths`, those not handed over a moment ago. Where the app runs once,
 * a second launch with a file says so twice (its arguments, and the file
 * the system opened it with): the file opens once, not as two runs. */
const handed = new Map<string, number>();
export function once(paths: string[], now = Date.now(), within = 2000): string[] {
  for (const [p, at] of handed) if (now - at > within) handed.delete(p);
  const fresh = [...new Set(paths)].filter((p) => !handed.has(p));
  for (const p of fresh) handed.set(p, now);
  return fresh;
}

/** The files a launch names: its arguments that are no option and no link, a
 * relative one read from the directory the launch was started in. */
export function launchFiles(args: string[], cwd: string): string[] {
  const absolute = (p: string) => /^([\\/]|[A-Za-z]:[\\/])/.test(p);
  const sep = cwd.includes("\\") && !cwd.includes("/") ? "\\" : "/";
  return args
    .filter((a) => a && !a.startsWith("-"))
    .flatMap((a) => (/^file:/i.test(a) ? [pathOf(a)] : /^[a-z][a-z0-9+.-]*:\/\//i.test(a) ? [] : [absolute(a) || !cwd ? a : `${cwd.replace(/[\\/]+$/, "")}${sep}${a}`]));
}

/** Opens run files dropped on the window; anything else is said to be no run. */
export function openDropped(uris: string[]) {
  void openPaths(
    uris.flatMap((uri) => {
      const path = pathOf(uri);
      if (/^file:/i.test(uri)) return [path];
      notify(t("runs.notRun", { name: uri }), { kind: "warning" });
      return [];
    }),
  );
}

/** What opens as a run: a recording, or traffic sonde reads as one (an
 * S3 server access log, CloudTrail events, a HAR), gzipped or not. */
const OPENS = /\.(sonde|log|txt|json|jsonl|har)(\.gz)?$/i;

/** Of what a drag carries, how many would open as runs and how many would not: said before the drop. */
export function wouldOpen(uris: string[]): { opens: number; others: number } {
  const opens = uris.filter((u) => /^file:/i.test(u) && OPENS.test(pathOf(u))).length;
  return { opens, others: uris.length - opens };
}

/** Opens run files by path, each read off the interface's thread; anything else is said to be no run. */
export async function openPaths(paths: string[]) {
  let last: string | null = null;
  for (const path of paths) {
    const name = path.split(/[\\/]/).pop() ?? path;
    if (!OPENS.test(name)) {
      notify(t("runs.notRun", { name }), { kind: "warning" });
      continue;
    }
    try {
      last = await api.runs.importPath(path);
    } catch (e) {
      // a log or JSON that is no traffic sonde reads: said as plainly as any other file
      if (name.toLowerCase().endsWith(".sonde")) report(e);
      else notify(t("runs.notRun", { name }), { kind: "warning" });
    }
  }
  if (!last) return;
  const opened = last;
  batch(() => {
    openRun.set(opened);
    runsChanged();
    go("runs");
  });
}

export async function openRunFile() {
  try {
    // the file is read by sonde, never by the app's script engine: a large
    // CloudTrail dump opens as a small one does
    let path: string | null;
    try {
      path = await api.runs.pick(t("runs.fileType"), t("runs.trafficType"));
    } catch (e) {
      // the picker only where there is no system dialog; any other failure
      // (a file that is no run) is said
      if (!/no file dialog|file chooser|not supported|no trash/i.test(String((e as Error)?.message ?? e))) throw e;
      // no system dialog here (the web): the picker hands the file over
      const [picked] = await Picker.pickFiles({
        filters: [
          { name: t("runs.fileType"), extensions: ["sonde"] },
          { name: t("runs.trafficType"), extensions: ["log", "txt", "json", "jsonl", "har"] },
        ],
      });
      // (a file larger than the picker reads comes without its bytes: said, not opened as an empty one)
      if (picked && !picked.file) throw new Error(t("runs.fileTooLarge", { name: picked.name, size: bytes(picked.size) }));
      path = picked?.file ? api.runs.import(picked.file.name, await picked.file.text()) : null;
    }
    if (!path) return;
    const opened = path;
    batch(() => {
      openRun.set(opened);
      runsChanged();
      go("runs");
    });
  } catch (e) {
    report(e);
  }
}
