// App-wide state: endpoints, the current endpoint and bucket, settings,
// errors, and navigation between sections. Rust owns the data; these
// signals mirror it and are refreshed after each change.

import { batch, Picker, signal } from "nsh";
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
/** The request record the inspector should show. */
export const focusRecord = signal<number | null>(null);
/** The key the versions screen shows. */
export const versionKey = signal<string>("");
/** What the current place shows, for the toolbar's breadcrumb (a session's command). */
export const crumb = signal<string | null>(null);
/** Bumped when the list of sessions changes (a run ended, an example opened, a replay). */
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

export function refreshEndpoints() {
  const list = api.endpoints();
  batch(() => {
    endpoints.set(list);
    if (endpoint() && !list.some((e) => e.id === endpoint())) endpoint.set(null);
    if (!endpoint() && list.length > 0) endpoint.set(list[0].id);
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
    endpoint.set(e.id);
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
  const p = decodeURIComponent(uri.replace(/^file:\/\/(localhost)?/, ""));
  // file:///C:/x on Windows
  return /^\/[A-Za-z]:/.test(p) ? p.slice(1) : p;
}

/** Opens session files dropped on the window; anything else is said to be no session. */
export function openDropped(uris: string[]) {
  let last: string | null = null;
  for (const uri of uris) {
    const path = pathOf(uri);
    const name = path.split(/[\\/]/).pop() ?? path;
    if (!uri.startsWith("file:") || !name.endsWith(".sonde")) {
      notify(t("sessions.notSession", { name }), { kind: "warning" });
      continue;
    }
    try {
      last = api.sessions.importPath(path);
    } catch (e) {
      report(e);
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
    const [picked] = await Picker.pickFiles({ filters: [{ name: t("sessions.fileType"), extensions: ["sonde"] }] });
    if (!picked) return;
    const path = api.sessions.import(picked.file.name, await picked.file.text());
    batch(() => {
      openSession.set(path);
      sessionsChanged();
      go("sessions");
    });
  } catch (e) {
    report(e);
  }
}
