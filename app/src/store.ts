// App-wide state: endpoints, the current endpoint and bucket, settings,
// errors, and navigation between sections. Rust owns the data; these
// signals mirror it and are refreshed after each change.

import { batch, signal } from "nsh";
import { api, asError } from "./native";
import { language } from "./i18n";
import type { EndpointView, Settings, SondeError } from "./types";

export type Section =
  | "endpoints"
  | "browse"
  | "requests"
  | "multipart"
  | "versions"
  | "capacity"
  | "probes"
  | "layouts"
  | "plugins"
  | "jobs"
  | "audit"
  | "settings";

export const SECTIONS: Section[] = ["endpoints", "browse", "requests", "multipart", "versions", "capacity", "probes", "layouts", "plugins", "jobs", "audit", "settings"];

export const endpoints = signal<EndpointView[]>([]);
export const settings = signal<Settings | null>(null);
export const endpoint = signal<string | null>(null);
export const bucket = signal<string | null>(null);
export const prefix = signal<string>("");
export const section = signal<Section>("endpoints");
/** The request record the inspector should show. */
export const focusRecord = signal<number | null>(null);
/** The key the versions screen shows. */
export const versionKey = signal<string>("");
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

/** Shows `e` in the banner; returns it. */
export function report(e: unknown): SondeError {
  const err = asError(e);
  if (err.kind !== "cancelled") lastError.set(err);
  return err;
}

let noticeTimer: ReturnType<typeof setTimeout> | undefined;
export function notify(text: string) {
  notice.set(text);
  if (noticeTimer) clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => notice.set(null), 3500);
}

/** Opens the inspector on a record. */
export function showRequest(id: number | null | undefined) {
  if (id === null || id === undefined) return;
  batch(() => {
    focusRecord.set(id);
    section.set("requests");
  });
}

export function go(s: Section) {
  section.set(s);
}

export function chooseBucket(b: string | null) {
  batch(() => {
    bucket.set(b);
    prefix.set("");
  });
}

export function init() {
  try {
    refreshSettings();
    refreshEndpoints();
  } catch (e) {
    report(e);
  }
}
