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
import { Button, Card, EMPHASIS, ICON, Row } from "./kit";
import { type FindingState, StateBadge } from "./sessionview";

/** What acting on a finding saved, measured, against the predicted ("saved 40 requests (predicted 42)"); null when nothing was measured. */
export const savedText = (f: FindingChange): string | null => (f.saved ? say([f.saved], f.savedText ?? "") : (f.savedText ?? null));

/** A run against its program's run before it, read off the interface's thread: null for a session that is no run of a program, or while it is read. */
export function sinceLastOf(path: () => string | null) {
  return resource(path, async (p): Promise<SinceLast | null> => {
    if (!p) return null;
    try {
      return await api.sessions.sinceLast(p);
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
  const parts = [`${t("compare.requests")} ${count(c.requests[0])} → ${count(c.requests[1])}`, `${t("compare.waiting")} ${duration(c.busyMs[0])} → ${duration(c.busyMs[1])}`];
  if (c.errors[0] || c.errors[1]) parts.push(`${t("signal.errors")} ${count(c.errors[0])} → ${count(c.errors[1])}`);
  return parts.join(" · ");
}

/** The findings that tell what changed: fixed first, then less, back, new (open ones say nothing new). */
export const changedFindings = (findings: FindingChange[]): FindingChange[] => {
  const rank: Record<FindingState, number> = { fixed: 0, less: 1, back: 2, new: 3, open: 4 };
  return findings.filter((f) => f.state !== "open").sort((a, b) => rank[a.state] - rank[b.state]);
};

/** "Since the run before": the change in figures, then the findings that were fixed or came back
 * (`max` of them), each fixed one with what it saved; `onCompare` opens the two runs side by side.
 * Nothing for a program's first run. */
export function SinceRun(props: { since: SinceLast | null | undefined; max?: number; onCompare?: () => void }) {
  const s = () => props.since;
  // fixed and back say what the reader did (or undid); less, new and open stand on the findings themselves
  const told = () => (s()?.findings ?? []).filter((f) => f.state === "fixed" || f.state === "back");
  const shown = () => told().slice(0, props.max ?? told().length);
  const tally = () =>
    (["fixed", "back", "less", "new"] as const)
      .map((st) => [st, (s()?.findings ?? []).filter((f) => f.state === st).length] as const)
      .filter(([, n]) => n > 0)
      .map(([st, n]) => `${count(n)} ${say([{ key: "fix." + st, args: {} }], st)}`)
      .join(" · ");
  return (
    // nothing to say when nothing changed: the same requests, the same errors, no finding gone, back, less or new
    <If when={s()?.previous && (tally() || (s()!.change && (s()!.change!.requests[0] !== s()!.change!.requests[1] || s()!.change!.errors[0] !== s()!.change!.errors[1]))) ? s() : null}>
      {(x) => (
        <Card pad="none" testID="since-run">
          <Row first>
            <Icon name="git-compare-arrows" />
            <View class="min-w-0 flex-1 gap-1">
              <Text class="text-body text-foreground line-clamp-1">
                {/* emphasis: the section's name before what it says */}
                <Text class={EMPHASIS}>{t("fix.since")}</Text>
                <If when={tally()}>{(n) => <Text class="text-muted-foreground">{` · ${n()}`}</Text>}</If>
              </Text>
              <If when={changeLine(x())}>{(l) => <Text testID="since-change" class="tabular-nums text-small text-muted-foreground line-clamp-2">{l()}</Text>}</If>
            </View>
            <If when={props.onCompare}>
              <Button small variant="outline" icon="git-compare-arrows" label={t("fix.compare")} onPress={() => props.onCompare?.()} testID="since-compare" />
            </If>
          </Row>
          <Each of={shown()} key={(f) => f.identity}>
            {(f, i) => (
              <Row top testID={`since-finding-${i()}`}>
                <View class="min-h-5 shrink-0 justify-center">
                  <StateBadge state={f().state} />
                </View>
                <View class="min-w-0 flex-1 gap-1">
                  <Text class="text-body leading-5 text-foreground line-clamp-2">{say(f().phrases, f().text)}</Text>
                  <If when={savedText(f())}>
                    {(v) => (
                      <View class="flex-row items-center gap-1">
                        <Icon name="trending-down" size={ICON.small} tint="status-ok" />
                        <Text testID={`since-saved-${i()}`} class="tabular-nums text-small text-status-ok">{v()}</Text>
                      </View>
                    )}
                  </If>
                </View>
              </Row>
            )}
          </Each>
        </Card>
      )}
    </If>
  );
}
