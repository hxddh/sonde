// The fix loop in the app: a finding followed across a program's runs.
// A run's findings carry their state against the program's run before
// (new, back, less; open says nothing new and is left unmarked), and what
// the run before had that this one has not is said once, beside them:
// "since the run before", each fixed finding with what it saved, measured,
// against what was predicted.

import { resource, Text, View } from "nsh";
import { count, duration } from "../format";
import { t } from "../i18n";
import { say } from "../phrases";
import { api } from "../native";
import type { FindingChange, SinceLast } from "../types";
import { Icon } from "./icon";
import { Crumb, ICON, TEXT } from "./kit";
import { savedText } from "./runchange";


/** A run against its program's run before it, read off the interface's thread: null for a run that is no run of a program, or while it is read. */
export function sinceLastOf(path: () => string | null) {
  return resource(path, async (p): Promise<SinceLast | null> => {
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

/** "Since the run before", in one sentence: what the findings did (fixed, back, less, more, new) and how the
 * figures changed, then a link that opens the two runs side by side (`onCompare`), where each fixed finding
 * says what it saved. Nothing for a program's first run, nor when nothing changed. */
export function SinceRun(props: { since: SinceLast | null | undefined; onCompare?: () => void }) {
  const s = () => props.since;
  // the one finding named: the first that came back or is new, else the one fixed (several fixed: each in Compare)
  const named = (): FindingChange | null => {
    const f = s()?.findings ?? [];
    const fixed = f.filter((x) => x.state === "fixed");
    return f.find((x) => x.state === "back") ?? f.find((x) => x.state === "new") ?? (fixed.length === 1 ? fixed[0] : null);
  };
  // what the findings did, counted: fixed, back, less, more, new (open ones say nothing new); the named one's
  // words beside its count, and a fixed one's measured saving after them
  const tally = () =>
    (["fixed", "back", "less", "more", "new"] as const)
      .map((st) => [st, (s()?.findings ?? []).filter((f) => f.state === st)] as const)
      .filter(([, of]) => of.length > 0)
      .map(([st, of]) => {
        const said = `${count(of.length)} ${say([{ key: "fix." + st, args: {} }], st)}`;
        const one = named();
        const what = one && one.state === st ? t("fix.named", { what: said, finding: say(one.phrases, one.text) }) : said;
        const saved = st === "fixed" && of.length === 1 ? savedText(of[0]) : null;
        return saved ? `${what}, ${saved}` : what;
      })
      .join(" · ");
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
