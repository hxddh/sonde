// A trial as the app follows it: the scenarios it will run, one after the
// other, and what the bridge says of each as it goes (`Sessions.trial`'s
// events), folded into the list the dialog shows.

import { type Key, t } from "./i18n";
import type { Trial, TrialEvent } from "./types";

/** A scenario as people say it ("The store asks to slow down"); one sonde has
 * no words for yet (a project's own) keeps its name. Its id stands beside it
 * in mono where it is listed: what `sonde run --trial` is given. */
const SCENARIOS = ["slowdown", "error", "lost-answer", "fail-complete", "slow", "s3-limits"];
export const scenarioLabel = (name: string): string => (SCENARIOS.includes(name) ? t(`scenario.${name}` as Key) : name);

/** One scenario of a trial: yet to run, running, or done with how the program stood. */
export interface TrialStep {
  scenario: string;
  about: string;
  state: "waiting" | "running" | "done";
  trial?: Trial;
}

/** The scenarios chosen, none started. */
export const trialSteps = (scenarios: { name: string; about: string }[]): TrialStep[] => scenarios.map((s) => ({ scenario: s.name, about: s.about, state: "waiting" }));

/** The list after an event: a scenario starts, or ends with its verdict. The
 * program's output and its recording's start change nothing here; an event
 * for a scenario that was not chosen is left out, and a verdict is never
 * taken back by a late "running". */
export function foldTrial(steps: TrialStep[], e: TrialEvent): TrialStep[] {
  if (!("state" in e)) return steps;
  return steps.map((s) => {
    if (s.scenario !== e.scenario) return s;
    if (e.state === "done") return { ...s, state: "done", trial: e.trial };
    return s.state === "done" ? s : { ...s, state: "running" };
  });
}

/** How far a trial is: scenarios done, of how many. */
export const trialDone = (steps: TrialStep[]) => steps.filter((s) => s.state === "done").length;

/** The worst standing among verdicts: broke, then degraded; none when every one survived or was not met. */
export function worstStanding(trials: { standing: Trial["standing"] }[] | undefined): "broke" | "degraded" | null {
  if (trials?.some((x) => x.standing === "broke")) return "broke";
  return trials?.some((x) => x.standing === "degraded") ? "degraded" : null;
}
