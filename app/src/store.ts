// App-wide state: endpoints, the current endpoint and bucket, settings,
// errors, and navigation between sections. Rust owns the data; these
// signals mirror it and are refreshed after each change.

import { batch, lifecycle, Picker, signal } from "nsh";
import { api, asError } from "./native";
import { language, t } from "./i18n";
import { toast, type ToastKind } from "./ui/toast";
import type { EndpointView, Settings, SondeError } from "./types";

/** The app's places: Run, Sessions and Buckets ("store"); Activity and
 * Settings, which a window shows over its place (a sheet, a window of their
 * own) and a terminal as places of their own. */
export type Area = "run" | "sessions" | "store" | "activity" | "settings";
/** The tabs of Store: what is in a bucket. */
export type StoreTab = "browse" | "multipart" | "versions" | "capacity" | "layouts" | "probes";
/** The tabs of Settings. */
export type SettingsTab = "settings" | "endpoints";
/** The tabs of Activity: what sonde itself did. */
export type ActivityTab = "requests" | "jobs" | "audit";
/** Where to go: an area, or a tab of one. */
export type Section = "run" | "sessions" | StoreTab | SettingsTab | ActivityTab;

/** Every place, as a terminal lists them. */
export const AREAS: Area[] = ["run", "sessions", "store", "activity", "settings"];
/** The places a window's sidebar holds. */
export const PLACES: Area[] = ["run", "sessions", "store"];
export const STORE_TABS: StoreTab[] = ["browse", "multipart", "versions", "capacity", "layouts", "probes"];
export const SETTINGS_TABS: SettingsTab[] = ["settings", "endpoints"];
export const ACTIVITY_TABS: ActivityTab[] = ["requests", "jobs", "audit"];

export function areaOf(s: Section): Area {
  if (s === "run" || s === "sessions") return s;
  if ((STORE_TABS as string[]).includes(s)) return "store";
  if ((ACTIVITY_TABS as string[]).includes(s)) return "activity";
  return "settings";
}

export const endpoints = signal<EndpointView[]>([]);
export const settings = signal<Settings | null>(null);
export const endpoint = signal<string | null>(null);
export const bucket = signal<string | null>(null);
export const prefix = signal<string>("");
export const section = signal<Section>("run");
export const storeTab = signal<StoreTab>("browse");
/** The detail of a bucket a window shows in a sheet over its objects. */
export const storeSheet = signal<StoreTab | null>(null);
export const settingsTab = signal<SettingsTab>("settings");
export const activityTab = signal<ActivityTab>("requests");
/** The area on screen. */
export const area = () => areaOf(section());
/** The session the Sessions area shows (set by Run when a run ends, by the palette, …). */
export const openSession = signal<string | null>(null);
/** A comparison to open with a session: the run before it (`Run again`). */
export const compareOnOpen = signal<{ before: string; after: string } | null>(null);
/** A pane to show a session on when it opens: a program opens on its runs. */
export const paneOnOpen = signal<{ path: string; pane: "runs" } | null>(null);
/** The request record the inspector should show. */
export const focusRecord = signal<number | null>(null);
/** The key the versions screen shows. */
export const versionKey = signal<string>("");
/** What the current place shows, for the toolbar's breadcrumb (a session's command). */
export const crumb = signal<string | null>(null);
/** Bumped when the list of sessions changes (a run ended, an example opened, a replay). */
/** The window is out of sight (minimized, hidden, another space): polls rest until it is back. */
export const away = () => lifecycle() === "background";

export const sessionsTick = signal(0);
export const sessionsChanged = () => sessionsTick.set(sessionsTick() + 1);
/** Settings › Endpoints opens on its form when set (the welcome's "Add an endpoint"). */
export const endpointsMode = signal<"list" | "add" | "import">("list");
/** The main window's width, as its shell measures it (windowSize() follows
 * whichever window has the focus). */
export const shellWidth = signal(1280);
/** And its height. */
export const shellHeight = signal(820);
/** The newest error, for the banner. */
export const lastError = signal<SondeError | null>(null);
/** A short notice (copied, exported). */
export const notice = signal<string | null>(null);

/** Makes `id` the endpoint; a bucket of another endpoint is no longer chosen. */
function useEndpoint(id: string | null) {
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
    endpoints.set(list);
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

export function readOnly(): boolean {
  return settings()?.["read-only"] ?? true;
}

/** Shows `e` as an error toast (its request id, and the request when there is one); returns it. */
export function report(e: unknown): SondeError {
  const err = asError(e);
  if (err.kind !== "cancelled") {
    lastError.set(err);
    const record = err.recordId;
    toast({
      id: "error",
      kind: "error",
      title: err.message,
      description: err.requestId ? `${t("common.requestId")}: ${err.requestId}` : undefined,
      action: record !== null && record !== undefined ? t("common.showRequest") : undefined,
      onAction: () => showRequest(record),
    });
  }
  return err;
}

/** A short notice (copied, exported, saved), as a toast. */
export function notify(text: string, options: { kind?: ToastKind; action?: string; onAction?: () => void } = {}) {
  notice.set(text);
  toast({ title: text, kind: options.kind ?? "success", action: options.action, onAction: options.onAction });
}

/** Opens a request in Activity. */
export function showRequest(id: number | null | undefined) {
  if (id === null || id === undefined) return;
  focusRecord.set(id);
  go("requests");
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

export function init() {
  try {
    refreshSettings();
    refreshEndpoints();
  } catch (e) {
    report(e);
  }
}

/** Asks for a `.sonde` file (one recorded elsewhere, or shared) and opens it in Sessions. */
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

/**
 * A `sonde://` link (from `sonde show --app`, a shared page, the docs). Its
 * URL is anyone's to make: it only opens what sonde would open itself.
 *   sonde://open?path=/abs/file.sonde   a session file, added as a drop is
 *   sonde://session/<file name>         a session of the sessions folder
 *   sonde://bucket/<endpoint>/<bucket>[/<prefix>]
 */
export function openLink(url: string) {
  const m = /^sonde:\/\/([a-z]+)(\/[^?#]*)?(\?[^#]*)?/i.exec(url.trim());
  if (!m) return;
  const [, what, rest = "", query = ""] = m;
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
    if (path?.toLowerCase().endsWith(".sonde")) openPaths([path]);
  } else if (what === "session") {
    const name = part(rest.slice(1));
    const s = api.sessions.list().find((x) => x.name === name);
    if (!s) return notify(t("link.noSession", { name }), { kind: "warning" });
    batch(() => {
      openSession.set(s.path);
      go("sessions");
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

/** Opens session files dropped on the window; anything else is said to be no session. */
export function openDropped(uris: string[]) {
  openPaths(
    uris.flatMap((uri) => {
      const path = pathOf(uri);
      if (/^file:/i.test(uri)) return [path];
      notify(t("sessions.notSession", { name: uri }), { kind: "warning" });
      return [];
    }),
  );
}

/** What opens as a session: a recording, or traffic sonde reads as one (an
 * S3 server access log, CloudTrail events, a HAR). */
const OPENS = /\.(sonde|log|txt|json|jsonl|har)$/i;

/** Opens session files by path; anything else is said to be no session. */
export function openPaths(paths: string[]) {
  let last: string | null = null;
  for (const path of paths) {
    const name = path.split(/[\\/]/).pop() ?? path;
    if (!OPENS.test(name)) {
      notify(t("sessions.notSession", { name }), { kind: "warning" });
      continue;
    }
    try {
      last = api.sessions.importPath(path);
    } catch (e) {
      // a log or JSON that is no traffic sonde reads: said as plainly as any other file
      if (name.toLowerCase().endsWith(".sonde")) report(e);
      else notify(t("sessions.notSession", { name }), { kind: "warning" });
    }
  }
  if (!last) return;
  const opened = last;
  batch(() => {
    openSession.set(opened);
    sessionsChanged();
    go("sessions");
  });
}

export async function openSessionFile() {
  try {
    // the file is read by sonde, never by the app's script engine: a large
    // CloudTrail dump opens as a small one does
    let path: string | null;
    try {
      path = await api.sessions.pick(t("sessions.fileType"), t("sessions.trafficType"));
    } catch (e) {
      // the picker only where there is no system dialog; any other failure
      // (a file that is no session) is said
      if (!/no file dialog|file chooser|not supported|no trash/i.test(String((e as Error)?.message ?? e))) throw e;
      // no system dialog here (the web): the picker hands the file over
      const [picked] = await Picker.pickFiles({
        filters: [
          { name: t("sessions.fileType"), extensions: ["sonde"] },
          { name: t("sessions.trafficType"), extensions: ["log", "txt", "json", "jsonl", "har"] },
        ],
      });
      path = picked ? api.sessions.import(picked.file.name, await picked.file.text()) : null;
    }
    if (!path) return;
    const opened = path;
    batch(() => {
      openSession.set(opened);
      sessionsChanged();
      go("sessions");
    });
  } catch (e) {
    report(e);
  }
}
