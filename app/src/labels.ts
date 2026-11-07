import { type Key, t } from "./i18n";
import type { Section } from "./store";
import type { Verdict } from "./types";
import type { Tone } from "./ui/kit";

const LABEL: Record<Section, Key> = {
  timeline: "nav.timeline",
  endpoints: "nav.endpoints",
  browse: "nav.browse",
  requests: "nav.requests",
  multipart: "nav.multipart",
  versions: "nav.versions",
  capacity: "nav.capacity",
  probes: "nav.probes",
  layouts: "nav.layouts",
  plugins: "nav.plugins",
  jobs: "nav.jobs",
  audit: "nav.audit",
  settings: "nav.settings",
};

export const sectionLabel = (s: Section) => t(LABEL[s]);

export const verdictTone = (v: Verdict): Tone => (v === "pass" ? "pass" : v === "fail" ? "fail" : v === "differs" ? "differs" : v === "unsupported" ? "unsupported" : v === "error" ? "warn" : "neutral");

export const verdictLabel = (v: Verdict) => t(`verdict.${v}` as Key);

export const SYMBOL: Record<Verdict, string> = { pass: "✓", fail: "✗", differs: "≠", unsupported: "∅", error: "!", skipped: "–" };
