// Two runs of a program, before and after a change: the five figures, what changed by kind, by prefix, over time
// and in latency, and the findings gone and new. The app's Compare and a shared page both show it: it reads
// nothing but what it is given.

import { Scroll, Text, View } from "nsh";
import { change, count, duration, percent, rate, costParts } from "../format";
import { t } from "../i18n";
import { say } from "../phrases";
import type { FindingChange, Profile, RunChange, Signals } from "../types";
import { Icon } from "./icon";
import { Card, Empty, Heading, ICON, Notice, Row, Stat, Tiles, SIZES, WIDTH, TEXT } from "./kit";
import { type FindingState, StateBadge } from "./runview";
import { brief, FEW, handful } from "../signals";
import { Chart, cost, DistributionCompare, PrefixChangeMap, RunBars, MARK, Sparkline, TrialOverClean, trialLegend, kindChangeSaid, prefixChangeSaid, latencyChangeSaid, overTimeSaid } from "./plots";

/** What acting on a finding saved, measured, against the predicted ("saved 40 requests (predicted 42)"); null when nothing was measured. */
export const savedText = (f: FindingChange): string | null => (f.saved ? say([f.saved], f.savedText ?? "") : (f.savedText ?? null));

/** The findings that tell what changed: fixed first, then less, new, more (open ones say nothing new). */
export const changedFindings = (findings: FindingChange[]): FindingChange[] => {
  const rank: Record<FindingState, number> = { fixed: 0, less: 1, new: 2, more: 3, open: 4 };
  return findings.filter((f) => f.state !== "open").sort((a, b) => rank[a.state] - rank[b.state]);
};

/** Two runs in five tiles, each fact once: requests, errors, the median
 * latency, the waiting, the cost — the figure before → after, by how much it
 * changed, and both runs' lines over their time (before quiet, after in
 * color). Two equal figures are said once, with no change beside them. Fewer
 * is better and more is worse between two runs of one program; between two
 * programs (`!sameProgram`) nothing is better or worse, and every tile is plain. */
function ChangeTiles(props: { change: RunChange; signals?: [Signals, Signals] | null }) {
  const c = () => props.change;
  const s = () => props.signals ?? null;
  const n = (v: number) => count(Math.round(v));
  type Fact = { id: string; label: string; a: number | null; b: number | null; text: (v: number) => string; detail?: () => string; series?: (x: Signals) => number[]; kind?: "bars" | "line"; /** a percentile of fewer than `FEW` requests in that run: marked `~` */ few?: [boolean, boolean] };
  const of = <T,>(f: (x: Signals) => T): [T, T] | null => (s() ? [f(s()![0]), f(s()![1])] : null);
  const pair = (a: string, b: string) => (a === b ? a : `${a} → ${b}`);
  const oneUnit = () => costParts(c().cost[0], c().currency).per1000 === costParts(c().cost[1], c().currency).per1000;
  const facts = (): Fact[] => {
    const pace = of((x) => (brief(x) || x.rate.value == null ? null : t("signal.perSecond", { n: rate(x.rate.value) })));
    const share = of((x) => percent(x.errors.value ?? 0));
    const p50 = of((x) => x.latency.p50Ms);
    // a percentile of a handful of requests is one of them: marked, in each run that had a handful
    const few = of((x) => x.rate.requests < FEW) ?? undefined;
    const mark = (i: 0 | 1, text: string) => (few?.[i] ? `~${text}` : text);
    const p95 = of((x) => (x.latency.p95Ms == null ? null : duration(x.latency.p95Ms)));
    // "p95" once, before the pair: "p95 ~5.7 ms → 69 ms"
    // (the same in both: one figure, marked where either run had a handful)
    const tails = () => (p95 && p95[0] && p95[1] ? `p95 ${p95[0] === p95[1] ? (few?.[0] || few?.[1] ? `~${p95[0]}` : p95[0]) : `${mark(0, p95[0])} → ${mark(1, p95[1])}`}` : "");
    return [
      { id: "requests", label: t("compare.requests"), a: c().requests[0], b: c().requests[1], text: n, detail: () => (pace && pace[0] && pace[1] ? pair(pace[0], pace[1]) : ""), series: (x) => x.rate.series },
      { id: "errors", label: t("signal.errors"), a: c().errors[0], b: c().errors[1], text: n, detail: () => (share ? pair(share[0], share[1]) : ""), series: (x) => x.errors.series },
      { id: "latency", label: t("signal.latency"), a: p50?.[0] ?? null, b: p50?.[1] ?? null, text: duration, detail: tails, series: (x) => x.latency.series, kind: "line", few },
      { id: "waiting", label: t("compare.waiting"), a: c().waitingMs[0], b: c().waitingMs[1], text: duration, detail: () => t("compare.took", { t: pair(duration(c().wallMs[0]), duration(c().wallMs[1])) }) },
      // (both in one unit: the unit is said once, under the two figures, so they fit and the tile's name is not cut)
      { id: "cost", label: t("compare.cost"), a: c().cost[0], b: c().cost[1], text: (v) => (oneUnit() ? costParts(v, c().currency).figure : cost(v, c().currency)), detail: () => (oneUnit() ? costParts(c().cost[1], c().currency).unit : "") },
    ];
  };
  // a figure kept with its unit: a tile's line wraps between figures, never inside one ("~93 / ms")
  const keep = (text: string) => text.replace(/(\d) (ms|s|min|h|B|KiB|MiB|GiB|USD)\b/g, "$1\u00a0$2");
  const tile = (f: () => Fact) => {
    const known = () => f().a != null && f().b != null;
    const same = () => known() && (Math.abs(f().b! - f().a!) < 1e-9 || f().text(f().a!) === f().text(f().b!));
    const side = (i: 0 | 1, v: number) => (f().few?.[i] ? `~${f().text(v)}` : f().text(v));
    // the same in both runs: one figure, never "a → a" (marked `~` where either run had a handful)
    // (each figure kept with its unit, in the value as in the detail: "~1.0 ms → ~1.3 ms" wraps at the arrow only)
    const value = () => keep(!known() ? t("signal.notTimed") : same() ? (f().few?.[0] || f().few?.[1] ? `~${f().text(f().a!)}` : f().text(f().a!)) : `${side(0, f().a!)} → ${side(1, f().b!)}`);
    // how much it changed, then what else there is to say of it; the same in both runs: said so, no "−0%"
    // (from nothing there is no share to say: the two figures say it)
    // (a figure kept with its unit: a tile's line wraps between figures, never inside one — "~93 / ms")
    const detail = () => keep([!known() ? "" : same() ? t("compare.sameBoth") : f().a === 0 ? "" : change(f().a!, f().b!), f().detail?.() ?? ""].filter(Boolean).join(" · "));
    // a figure of too few requests to tell is no better and no worse: plain
    const tone = () => (!known() || same() || !c().sameProgram || f().few?.[0] || f().few?.[1] ? "plain" : f().b! < f().a! ? "ok" : "fail");
    const lines = () => (f().series && s() && !handful(s()![0]) && !handful(s()![1]) ? [f().series!(s()![0]), f().series!(s()![1])] : null);
    return (
      <Stat
        testID={`compare-${f().id}`}
        label={f().label}
        value={value()}
        detail={detail()}
        tone={tone()}
        // (a tile with no lines to draw has none: its detail then takes a second line before it is cut)
        mark={
          lines() ? (
            <View class="shrink-0 flex-row items-center gap-1" accessibilityLabel="">
              <Sparkline still id={`compare:a:${f().id}`} shape={lines()![0]} kind={f().kind} tone="quiet" class={MARK.pair} />
              <Sparkline still id={`compare:b:${f().id}`} shape={lines()![1]} kind={f().kind} class={MARK.pair} />
            </View>
          ) : undefined
        }
      />
    );
  };
  // (each tile whole at every width: a change is what it says, and its detail is the change's size; given its look, it is as high as its row)
  return <Tiles min={SIZES.compareTile} testID="compare-figures" items={facts().map((_, i) => () => tile(() => facts()[i]))} />;
}

/** Two runs of a program, before and after a change: the figures, by kind of request, and the findings gone and new. */
export function RunChangeView(props: { change: RunChange; inline?: boolean; /** both runs' signals, when they are read: the tiles' lines and second figures */ signals?: [Signals, Signals] | null; /** both runs' profiles, when they are read: the two over time */ profiles?: [Profile, Profile] | null }) {
  const c = () => props.change;
  const n = (v: number) => count(Math.round(v));
  // each finding that changed between the runs: fixed (with what it saved, measured against the predicted), less, new
  const changed = () => changedFindings(c().findings ?? []);
  const findings = () => changed().length;
  // the same requests of each kind, the same errors, no finding gone or new
  const unchanged = () => c().requests[0] === c().requests[1] && c().errors[0] === c().errors[1] && c().groups.every((g) => g.requests[0] === g.requests[1]) && findings() === 0;
  return (
    <Scroll testID="run-change" class={props.inline ? "" : "min-h-0 flex-1"} contentClass="gap-6 pb-4" scrollable={!props.inline}>
      <If when={!c().sameProgram}>
        <Notice testID="other-program" tone="warn" text={t("compare.otherProgram")} />
      </If>
      <ChangeTiles change={c()} signals={props.signals} />
      <If
        when={!unchanged()}
        else={
          <View testID="run-unchanged">
            <Empty icon="circle-check" text={t("compare.same")} />
          </View>
        }
      >
        {/* the two over time on one axis, from each run's start: the later over the earlier, dashed */}
        <If when={props.profiles && props.profiles[0].rate.length > 2 && props.profiles[1].rate.length > 2 ? props.profiles : null}>
          {(pr) => (
            <Chart testID="compare-over-time" title={t("compare.overTime")} text={overTimeSaid(pr()[0], pr()[1])} how={t("compare.overTimeHint")} legend={trialLegend(false, [t("profile.before"), t("profile.after")])}>
              <TrialOverClean trial={pr()[1]} clean={pr()[0]} faults={[]} label={overTimeSaid(pr()[0], pr()[1])} />
            </Chart>
          )}
        </If>
        <Chart title={t("compare.byKind")} text={kindChangeSaid(c())} how={t("compare.byKindHint")}>
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

