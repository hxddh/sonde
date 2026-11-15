// What sonde says, in the reader's language. Rust gives each title, note, finding, verdict, plan, job and probe
// sentence as phrases (a key and its arguments) beside its English. Their words are in the locale files, the one
// place a phrase lives in every language: a run's under `phrase.<key>` (sonde-proxy), a service's under
// `core.<key>` (decoders, sonde-core, probes); Rust reads the English ones too. A phrase with no words in the
// locale falls back to the English text, whole.

import { type Key, t, words } from "./i18n";
import type { CorePhrase, EpisodeRow, Observation, Phrase } from "./types";

/** `text` said from the locale's `<family>.<key>` words, each phrase filled with its arguments in one pass (an
 * argument's own `%{n}` is not read as a placeholder); `fallback` where a phrase has no words. */
function sayFrom(family: "phrase" | "core", text: { key: string; args?: Record<string, string> }[] | undefined | null, fallback: string): string {
  if (!text?.length) return fallback;
  let out = "";
  for (const p of text) {
    const tpl = words(`${family}.${p.key}`);
    if (tpl === null) return fallback;
    const args = p.args ?? {};
    out += tpl.replace(/%\{(\w+)\}/g, (whole, k: string) => (Object.prototype.hasOwnProperty.call(args, k) ? args[k] : whole));
  }
  return out;
}

/** What sonde's services say (a finding, a verdict, a plan, a job, a probe's sentence), in the reader's language:
 * its phrases, else its English whole. The one way a page shows a sentence a service wrote. */
export const core = (text: CorePhrase[] | undefined | null, en: string | null | undefined): string => sayFrom("core", text, en ?? "");

/** An endpoint's name as the reader sees it: the built-in demo's is the app's own word, in its language. */
export const endpointName = (e: { id: string; name: string }): string => (e.id === "demo" ? t("endpoints.demoName") : e.name);

/** `text` (a run's phrases) in the reader's language; `fallback` (Rust's own English, where it was sent beside
 * them) when a phrase has no words. */
export const say = (text: Phrase[] | undefined, fallback: string | undefined): string => sayFrom("phrase", text, fallback ?? "");

export const episodeTitle = (e: Pick<EpisodeRow, "titleText" | "title">) => say(e.titleText, e.title);

/** An episode's notes, in the reader's language, less those nearly every operation of its format has
 * (`shared`, a conditional create on every SlateDB write): a table says those once, not on each row; the
 * inspector, given none, says every note of its operation. */
export function episodeNotes(e: Pick<EpisodeRow, "notes" | "notesText">, shared: string[]): string[] {
  return e.notes.flatMap((n, i) => (shared.includes(n) ? [] : [say(e.notesText?.[i] ? [e.notesText[i]] : undefined, n)]));
}

/** The notes nearly every operation has, in the reader's language (from an operation of `rows` that carries
 * each, else as the run gave it): said once over a table that leaves them out of its rows. */
export function sharedNotes(rows: Pick<EpisodeRow, "notes" | "notesText">[], shared: string[]): string[] {
  return shared.map((n) => {
    const e = rows.find((r) => r.notes.includes(n));
    const i = e ? e.notes.indexOf(n) : -1;
    return say(e && i >= 0 && e.notesText?.[i] ? [e.notesText[i]] : undefined, n);
  });
}

export const observationText = (o: Observation) => say(o.phrases, o.text);

/** What acting on a finding would save, in the reader's language. */
export const observationSaving = (o: Observation): string | null => (o.saving ? say([o.saving], o.savingText ?? "") : null);

/** Why an observation matters, for the card it shows on hover: the key of its first phrase. */
export const observationWhy = (o: Observation): string | null => {
  const key = o.phrases?.[0]?.key;
  if (!key) return null;
  const k = `why.${key === "obs.orphans.one" ? "obs.orphans" : key.startsWith("obs.prefixrate") ? "obs.prefixrate" : key}`;
  const s = t(k as Key);
  return s === k ? null : s;
};
