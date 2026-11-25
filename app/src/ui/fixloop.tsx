// The fix loop in the app: a finding followed across a program's runs.
// A run's findings carry their state against the program's run before
// (new, less; open says nothing new and is left unmarked), and what the run
// before had that this one has not is said once, beside them: "since the
// run before", a fixed finding with the time it saved, measured, against
// what was predicted ("1.2 s less (predicted 1.5 s)", or "within noise").

import { resource, Text, View } from "nsh";
import { count, duration } from "../format";
import { t } from "../i18n";
import { say } from "../phrases";
import { api } from "../native";
import type { FindingChange, SinceLast } from "../types";
import { Icon } from "./icon";
import { Crumb, ICON, TEXT } from "./kit";
import { savedText } from "./runchange";


/** A run against its program's run before it, read off the interface's thread: null for a run that is no run of a program, or while it is read.
 * Read again when `live` turns (a run opened as it starts has nothing to compare until it ends). */
export function sinceLastOf(path: () => string | null, live: () => boolean = () => false) {
  return resource(() => { const p = path(); return p ? `${live() ? 1 : 0}${p}` : null; }, async (k): Promise<SinceLast | null> => {
    const p = k?.slice(1);
    if (!p) return null;
    try {
      return await api.runs.sinceLast(p);
    } catch {
      return null;
    }
  });
}

/** Each finding's state by its identity, once there is a run before to hold it to. */
export function statesOf(s: SinceLast | null | undefined): Record<string, FindingChange> {
  if (!s?.previous) return {};
  return Object.fromEntries(s.findings.map((f) => [f.identity, f]));
}

/** What changed since the run before, in a line: requests and waiting, each "before → after". */
function changeLine(s: SinceLast): string | null {
  const c = s.change;
  if (!c) return null;
  // a figure's "before → after" is never broken across lines (the line wraps between its words)
  // (the same in both runs: one figure, never "a → a")
  const part = (label: string, a: string, b: string) => `${label}\u00a0${(a === b ? a : `${a} → ${b}`).replace(/ /g, "\u00a0")}`;
  const parts = [part(t("compare.requests"), count(c.requests[0]), count(c.requests[1])), part(t("compare.waiting"), duration(c.waitingMs[0]), duration(c.waitingMs[1]))];
  // errors only where they changed: "Errors 2 → 2" says nothing
  if (c.errors[0] !== c.errors[1]) parts.push(part(t("signal.errors"), count(c.errors[0]), count(c.errors[1])));
  return parts.join(" · ");
}

/** The findings of a state, counted, in words ("1 finding smaller", "2 new"). */
const COUNTED = { fixed: "fix.fixedN", less: "fix.lessN", more: "fix.moreN", new: "fix.newN" } as const;

/** What the findings did since the run before: fixed, less, more, new (open ones say nothing new), each counted
 * ("1 finding smaller"), but the one named — the one fixed, else the first that is new (several: each in Compare) —
 * by its state and its words, and a fixed one's measured saving ("fixed: … — 1.2 s less (predicted 1.5 s)"). */
export function findingsSaid(findings: FindingChange[]): string {
  const fixed = findings.filter((x) => x.state === "fixed");
  const one = fixed.length === 1 ? fixed[0] : (findings.find((x) => x.state === "new") ?? null);
  return (["fixed", "less", "more", "new"] as const)
    .map((st) => [st, findings.filter((f) => f.state === st)] as const)
    .filter(([, of]) => of.length > 0)
    .map(([st, of]) => {
      if (!one || one.state !== st || of.length > 1) return t(COUNTED[st], { n: of.length });
      const word = say([{ key: "fix." + st, args: {} }], st);
      const saved = st === "fixed" ? savedText(one) : null;
      const finding = say(one.phrases, one.text);
      return saved ? t("fix.namedSaved", { what: word, finding, saved }) : t("fix.named", { what: word, finding });
    })
    .join(" · ");
}

/** "Since the run before", in one sentence: what the findings did (fixed, less, more, new) and how the
 * figures changed, then a link that opens the two runs side by side (`onCompare`), where each fixed finding
 * says what it saved. Nothing for a program's first run, nor when nothing changed. */
export function SinceRun(props: { since: SinceLast | null | undefined; onCompare?: () => void }) {
  const s = () => props.since;
  const tally = () => findingsSaid(s()?.findings ?? []);
  const changed = (x: SinceLast) => !!tally() || (!!x.change && (x.change.requests[0] !== x.change.requests[1] || x.change.errors[0] !== x.change.errors[1]));
  const sentence = (x: SinceLast) => [tally(), changeLine(x)].filter(Boolean).join(" · ");
  return (
    <If when={s()?.previous && changed(s()!) ? s() : null}>
      {(x) => (
        <View testID="since-run" class="flex-row items-center gap-2">
          <Icon name="git-compare-arrows" size={ICON.small} tint="muted-foreground" />
          <Text testID="since-change" class={`min-w-0 shrink tabular-nums ${TEXT.note} line-clamp-2`}>
            {t("fix.since", { what: sentence(x()) })}
          </Text>
          <If when={props.onCompare}>
            <Crumb testID="since-compare" href="sonde://compare" onOpen={() => props.onCompare?.()} label={t("fix.compare")}>
              <Text class="text-small text-primary">{t("fix.compare")}</Text>
            </Crumb>
          </If>
        </View>
      )}
    </If>
  );
}
