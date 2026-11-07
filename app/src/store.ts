// App-wide state: endpoints, the current endpoint and bucket, settings,
// errors, and navigation between sections. Rust owns the data; these
// signals mirror it and are refreshed after each change.

import { batch, signal } from "nsh";
import { api, asError } from "./native";
import { language, t } from "./i18n";
import { toast, type ToastKind } from "./ui/toast";
import type { EndpointView, Settings, SondeError } from "./types";

/** The four places of the app. */
export type Area = "run" | "sessions" | "store" | "settings";
/** The tabs of Store: what is in a bucket. */
export type StoreTab = "browse" | "multipart" | "versions" | "capacity" | "layouts" | "probes";
/** The tabs of Settings. */
export type SettingsTab = "settings" | "endpoints" | "requests" | "jobs" | "audit";
/** Where to go: an area, or a tab of one. */
export type Section = "run" | "sessions" | StoreTab | SettingsTab;

export const AREAS: Area[] = ["run", "sessions", "store", "settings"];
export const STORE_TABS: StoreTab[] = ["browse", "multipart", "versions", "capacity", "layouts", "probes"];
export const SETTINGS_TABS: SettingsTab[] = ["settings", "endpoints", "requests", "jobs", "audit"];

export function areaOf(s: Section): Area {
  if (s === "run" || s === "sessions") return s;
  return (STORE_TABS as string[]).includes(s) ? "store" : "settings";
}

export const endpoints = signal<EndpointView[]>([]);
export const settings = signal<Settings | null>(null);
export const endpoint = signal<string | null>(null);
export const bucket = signal<string | null>(null);
export const prefix = signal<string>("");
export const section = signal<Section>("run");
export const storeTab = signal<StoreTab>("browse");
export const settingsTab = signal<SettingsTab>("settings");
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

/** Opens the inspector on a record. */
export function showRequest(id: number | null | undefined) {
  if (id === null || id === undefined) return;
  batch(() => {
    focusRecord.set(id);
    settingsTab.set("requests");
    section.set("requests");
  });
}

/** Goes to an area (the tab it had) or to a tab of one. */
export function go(s: Section | Area) {
  batch(() => {
    if (s === "store") s = storeTab();
    else if (s === "settings" && section() !== "settings" && areaOf(section()) !== "settings") s = settingsTab();
    const target = s as Section;
    if ((STORE_TABS as string[]).includes(target)) storeTab.set(target as StoreTab);
    else if ((SETTINGS_TABS as string[]).includes(target)) settingsTab.set(target as SettingsTab);
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
