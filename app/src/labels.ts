import { type Key, t } from "./i18n";
import type { Area, Section } from "./store";
import type { Verdict } from "./types";
import type { Tone } from "./ui/kit";

const LABEL: Record<Section, Key> = {
  run: "nav.run",
  sessions: "nav.sessions",
  browse: "nav.browse",
  multipart: "nav.multipart",
  versions: "nav.versions",
  capacity: "nav.capacity",
  layouts: "nav.layouts",
  probes: "nav.probes",
  settings: "nav.general",
  endpoints: "nav.endpoints",
  requests: "nav.requests",
  jobs: "nav.jobs",
  audit: "nav.audit",
};

const AREA: Record<Area, Key> = {
  run: "nav.run",
  sessions: "nav.sessions",
  store: "nav.store",
  settings: "nav.settings",
  activity: "nav.activity",
};

export const areaLabel = (a: Area) => t(AREA[a]);
export const sectionLabel = (s: Section) => t(LABEL[s]);

export const verdictTone = (v: Verdict): Tone => (v === "pass" ? "pass" : v === "fail" ? "fail" : v === "differs" ? "differs" : v === "unsupported" ? "unsupported" : v === "error" ? "warn" : "neutral");

export const verdictLabel = (v: Verdict) => t(`verdict.${v}` as Key);

export const SYMBOL: Record<Verdict, string> = { pass: "✓", fail: "✗", differs: "≠", unsupported: "∅", error: "!", skipped: "–" };

/** A vendor's name: the product's own (an S3 name, the same in every language), or "Other" in the reader's. */
export const vendorName = (v: { id?: string; vendor?: string; label?: string; vendorLabel?: string }) => ((v.vendor ?? v.id) === "other" ? t("endpoints.vendorOther") : (v.vendorLabel ?? v.label ?? ""));
