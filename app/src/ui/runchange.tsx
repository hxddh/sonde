// Two runs of a program, before and after a change: what changed over time, by kind, by prefix and in latency, each
// chart's sentence its figures before → after, and the findings gone and new. The app's Compare and a shared page both show it: it reads
// nothing but what it is given.

import { Scroll, Text, View } from "nsh";
import { count, duration } from "../format";
import { t } from "../i18n";
import { say } from "../phrases";
import type { FindingChange, Profile, RunChange } from "../types";
import { Icon } from "./icon";
import { Card, Empty, Heading, ICON, Notice, Row, WIDTH, TEXT } from "./kit";
import { type FindingState, StateBadge } from "./runview";
import { Chart, cost, DistributionCompare, PrefixChangeMap, RunBars, TrialOverClean, trialLegend, kindChangeSaid, prefixChangeSaid, latencyChangeSaid } from "./plots";

/** What acting on a finding saved, measured, against the predicted ("saved 40 requests (predicted 42)"); null when nothing was measured. */
export const savedText = (f: FindingChange): string | null => (f.saved ? say([f.saved], f.savedText ?? "") : (f.savedText ?? null));

/** The findings that tell what changed: fixed first, then less, new, more (open ones say nothing new). */
export const changedFindings = (findings: FindingChange[]): FindingChange[] => {
  const rank: Record<FindingState, number> = { fixed: 0, less: 1, new: 2, more: 3, open: 4 };
  return findings.filter((f) => f.state !== "open").sort((a, b) => rank[a.state] - rank[b.state]);
};

/** A figure in two runs: "18 → 8", or one figure where both are the same (never "8 → 8"). */
const pair = (a: string, b: string) => (a === b ? a : `${a} → ${b}`);

/** The two runs' requests and how long each took: "18 → 8 requests, 325 → 185 ms" (the over-time chart's sentence). */
export const figuresSaid = (c: RunChange): string => t("compare.figures", { requests: pair(count(c.requests[0]), count(c.requests[1])), took: pair(duration(c.wallMs[0]), duration(c.wallMs[1])) });

/** What else moved, each "before → after": the waiting on the store, errors (where either run had any), the cost. */
export function restSaid(c: RunChange): string {
  const parts = [`${t("compare.waiting")} ${pair(duration(c.waitingMs[0]), duration(c.waitingMs[1]))}`];
  if (c.errors[0] || c.errors[1]) parts.push(`${t("signal.errors")} ${pair(count(c.errors[0]), count(c.errors[1]))}`);
  parts.push(`${t("compare.cost")} ${pair(cost(c.cost[0], c.currency), cost(c.cost[1], c.currency))}`);
  return parts.join(" · ");
}

/** Two runs of a program, before and after a change: the figures, by kind of request, and the findings gone and new. */
export function RunChangeView(props: { change: RunChange; inline?: boolean; /** both runs' profiles, when they are read: the two over time */ profiles?: [Profile, Profile] | null }) {
  const c = () => props.change;
  // each finding that changed between the runs: fixed (with what it saved, measured against the predicted), less, new
  const changed = () => changedFindings(c().findings ?? []);
  const findings = () => changed().length;
  const overTime = () => (props.profiles && props.profiles[0].rate.length > 2 && props.profiles[1].rate.length > 2 ? props.profiles : null);
  // the same requests of each kind, the same errors, no finding gone or new
  const unchanged = () => c().requests[0] === c().requests[1] && c().errors[0] === c().errors[1] && c().groups.every((g) => g.requests[0] === g.requests[1]) && findings() === 0;
  return (
    <Scroll testID="run-change" class={props.inline ? "" : "min-h-0 flex-1"} contentClass="gap-6 pb-4" scrollable={!props.inline}>
      <If when={!c().sameProgram}>
        <Notice testID="other-program" tone="warn" text={t("compare.otherProgram")} />
      </If>
      <If
        when={!unchanged()}
        else={
          <View testID="run-unchanged">
            <Empty icon="circle-check" text={t("compare.same")} />
          </View>
        }
      >
        {/* the two over time on one axis, from each run's start: the later over the earlier, dashed; its sentence the
            requests and the time each took, then the waiting, errors and cost (no tiles of the same figures) */}
        <If when={overTime()}>
          {(pr) => (
            <Chart testID="compare-over-time" title={t("compare.overTime")} text={`${figuresSaid(c())} · ${restSaid(c())}`} how={t("compare.overTimeHint")} legend={trialLegend(false, [t("profile.before"), t("profile.after")])}>
              <TrialOverClean trial={pr()[1]} clean={pr()[0]} faults={[]} label={figuresSaid(c())} />
            </Chart>
          )}
        </If>
        {/* (runs too short to draw over time: their figures lead the sentence by kind) */}
        <Chart testID="compare-by-kind" title={t("compare.byKind")} text={overTime() ? kindChangeSaid(c()) : `${figuresSaid(c())} · ${restSaid(c())} · ${kindChangeSaid(c())}`} how={t("compare.byKindHint")}>
          <RunBars change={c()} neutral={!c().sameProgram} />
        </Chart>
        {/* where it changed: each prefix by the later run's requests, colored by how they moved */}
        <If when={(c().prefixes?.length ?? 0) > 1}>
          <Chart title={t("prefixChange.title")} text={prefixChangeSaid(c())} how={t("prefixChange.hint")}>
            <PrefixChangeMap prefixes={c().prefixes!} neutral={!c().sameProgram} />
          </Chart>
        </If>
        {/* both runs' latency: where the curves part says for which requests it changed */}
        <If when={c().latency && c().latency![0].length > 1 && c().latency![1].length > 1}>
          <Chart title={t("cdf.title")} text={latencyChangeSaid(c())} how={t("cdf.hint")} legend={[{ label: t("profile.before"), swatch: "border-muted-foreground/70", line: "dashed" }, { label: t("profile.after"), swatch: "border-chart-1", line: "solid" }]}>
            <DistributionCompare latency={c().latency!} names={[t("profile.before"), t("profile.after")]} />
          </Chart>
        </If>
        <If when={findings() > 0}>
          <View class="gap-2">
            <Heading>{t("obs.title")}</Heading>
            <Card pad="none">
              <Each of={changed()} key={(f) => f.identity}>
                {(f, i) => (
                  <Row top first={i() === 0} testID={`compare-finding-${i()}`}>
                    <View class={`${WIDTH.badge} min-h-5 shrink-0 flex-row items-center`}>
                      <StateBadge state={f().state} />
                    </View>
                    <View class="min-w-0 flex-1 gap-1">
                      <Text class="text-body leading-5 text-foreground">{say(f().phrases, f().text)}</Text>
                      {/* less: as it read in the run before */}
                      <If when={f().state === "less" ? f().was : null}>{(w) => <Text class={TEXT.note}>{t("fix.was", { text: say(w().phrases, w().text) })}</Text>}</If>
                      <If when={savedText(f())}>
                        {(v) => (
                          <View class="flex-row items-center gap-1">
                            <Icon name="trending-down" size={ICON.small} tint="status-ok" />
                            <Text testID={`compare-saved-${i()}`} class={`tabular-nums ${TEXT.note}`}>{v()}</Text>
                          </View>
                        )}
                      </If>
                    </View>
                  </Row>
                )}
              </Each>
            </Card>
          </View>
        </If>
      </If>
    </Scroll>
  );
}

