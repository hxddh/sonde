// A session's time: what the program waited on, by kind of request, and how
// long its requests took. The charts follow the order a question about S3
// runs: when, how long (each request), which kind and when, what pushed
// back, where the time went (by kind, or by the code that asked), and
// whether a prefix ran into its limits. A stretch picked on the timeline
// zooms every chart drawn over time.

import { batch, effect, Scroll, signal, Text, untracked, View } from "nsh";
import { t } from "../i18n";
import { ms } from "../format";
import type { SessionView } from "../types";
import { Button, Notice, Options, terminal } from "./kit";
import { findingMarks } from "./sessionview";
import { Chains, Chart, Distribution, timed, Flows, Heat, KindsOverTime, Lanes, offset, PrefixMeasure, type PrefixMeasureKind, PrefixUse, Pressure, RequestScatter, SessionShape, shapeSentence, TimeSpent } from "./plots";

/** The Time pane of a session. */
export function TimePane(props: { view: SessionView; onRequest?: (id: number) => void }) {
  const p = () => props.view.profile;
  // a stretch of the session, picked on the timeline by two presses
  const zoomFrom = signal<number | null>(null);
  const zoom = signal<[number, number] | null>(null);
  const pick = (i: number | null) => {
    // pressing the first slice again lets it go
    if (i === null) return zoomFrom.set(null);
    const from = zoomFrom();
    if (from === i && !zoom()) return zoomFrom.set(null);
    if (from === null) return zoomFrom.set(i);
    batch(() => {
      zoom.set([Math.min(from, i), Math.max(from, i)]);
      zoomFrom.set(null);
    });
  };
  const zoomMs = (): [number, number] | null => {
    const z = zoom();
    return z ? [z[0] * p().binMs, (z[1] + 1) * p().binMs] : null;
  };
  const scatterBy = signal<"time" | "size" | "kinds">("time");
  const spentBy = signal<"kind" | "code">("kind");
  const pushedBack = () => p().rate.some((b) => (b.throttled ?? 0) > 0 || (b.retried ?? 0) > 0 || b.client + b.server > 0);
  const measure = signal<PrefixMeasureKind>("requests");
  // the requests the scatter draws: a zoom's, or every one sampled
  const shown = () => {
    const d = zoomMs();
    return d ? p().samples.filter((x) => x.tMs >= d[0] && x.tMs < d[1]).length : p().samples.length;
  };
  // a session of a moment has no shape over time to show
  const shaped = () => p().rate.length > 2 && p().samples.length > 2;
  // a live session's slices change length as it grows: a stretch picked would cover another time
  let binMs: number | undefined;
  effect(() => {
    const b = p().binMs;
    if (binMs !== undefined && b !== binMs) untracked(() => batch(() => (zoom.set(null), zoomFrom.set(null))));
    binMs = b;
  });
  // the board: charts side by side where the pane is wide, one under another where it is not
  const half = terminal() ? "" : "min-w-[420px] flex-1";
  const row = terminal() ? "gap-2" : "flex-row flex-wrap items-stretch gap-4";
  return (
    <Scroll testID="time-pane" class="min-h-0 flex-1" contentClass={terminal() ? "gap-2" : "gap-4 pb-2"}>
      {/* the figures are the session header's; here, a terminal's sentence */}
      <If when={terminal() && timed(p())}>
        <Text class="text-foreground">{shapeSentence(p())}</Text>
      </If>
      {/* a log without times (CloudTrail): when and what, never how long */}
      <If when={!timed(p())}>
        <Notice testID="untimed" text={t("profile.untimed")} />
      </If>
      {/* when: the whole session, its findings marked; the stretch picked here is what every chart over time shows */}
      <If when={!terminal() && shaped()}>
        <Chart
          title={t("timeline.title")}
          text={zoom() ? t("timeline.zoomed", { a: offset(zoomMs()![0]), b: offset(zoomMs()![1]) }) : zoomFrom() !== null ? t("timeline.zoomFrom", { t: offset((zoomFrom() ?? 0) * p().binMs) }) : t("timeline.hint")}
          action={
            <If when={zoom() || zoomFrom() !== null}>
              <Button small variant="outline" icon="x" label={t("timeline.unzoom")} onPress={() => batch(() => (zoom.set(null), zoomFrom.set(null)))} testID="zoom-reset" />
            </If>
          }
        >
          <SessionShape profile={p()} annotate axis marks={findingMarks(props.view)} range={zoom()} slice={zoomFrom()} onSlice={pick} class="mb-5 mt-4 h-28 w-full" testID="time-shape" />
        </Chart>
      </If>
      <View class={row}>
        {/* how long: every request, when it ran or how large it was; a press opens it */}
        <If when={!terminal() && timed(p()) && p().samples.length > 1}>
          <Chart
            class={half}
            title={t("scatter.title")}
            text={scatterBy() === "kinds" ? t("kinds.title") : t("scatter.text", { n: shown(), p95: ms(p().p95Ms ?? 0) })}
            action={<Options testID="scatter-by" value={scatterBy()} items={[{ value: "time", title: t("scatter.byTime") }, { value: "size", title: t("scatter.bySize") }, ...((p().groupRate?.length ?? 0) > 0 ? [{ value: "kinds", title: t("scatter.byKind") }] : [])]} onChange={(v) => scatterBy.set(v as "time" | "size" | "kinds")} />}
          >
            {/* every request, or the busiest kinds each over time (their median to p95) */}
            <If when={scatterBy() === "kinds"} else={<RequestScatter profile={p()} by={scatterBy() === "size" ? "size" : "time"} domainMs={zoomMs()} onRequest={props.onRequest} />}>
              <KindsOverTime profile={p()} domainMs={zoomMs()} />
            </If>
          </Chart>
        </If>
        {/* what ran at once, what waited: each request on its lane */}
        <If when={!terminal() && timed(p()) && p().samples.length > 1}>
          <Chart class={half} testID="lanes-chart" title={t("lanes.title")} text={t("lanes.hint")}>
            <Lanes profile={p()} domainMs={zoomMs()} onRequest={props.onRequest} />
          </Chart>
        </If>
        <If when={timed(p())}>
          <Chart
            class={half}
            title={t("profile.spentTitle")}
            text={t("profile.spentHint")}
            action={
              <If when={(p().callers?.length ?? 0) > 0}>
                <Options testID="spent-by" value={spentBy()} items={[{ value: "kind", title: t("spent.byKind") }, { value: "code", title: t("spent.byCode") }]} onChange={(v) => spentBy.set(v as "kind" | "code")} />
              </If>
            }
          >
            <TimeSpent profile={p()} by={spentBy()} />
          </Chart>
        </If>
        <If when={p().latency?.length}>
          <Chart class={half} title={t("dist.title")} text={t("dist.hint")}>
            <Distribution profile={p()} />
          </Chart>
        </If>
        <If when={pushedBack()}>
          <Chart class={half} title={t("pressure.title")} text={t("pressure.hint")}>
            <Pressure profile={p()} range={zoom()} />
          </Chart>
        </If>
        <If when={p().chains?.length}>
          <Chart class={half} title={t("chains.title")} text={t("chains.hint")}>
            <Chains profile={p()} />
          </Chart>
        </If>
        {/* where by prefix: how many, what they cost, how near the limit; then over time */}
        <If when={p().prefixes.some((x) => x.prefix !== "other")}>
          <Chart class={half} title={t("prefixUse.title")} text={t("prefixUse.hint")} action={<PrefixMeasure profile={p()} value={measure()} onChange={(m) => measure.set(m)} />}>
            <PrefixUse profile={p()} measure={measure()} />
          </Chart>
        </If>
        <If when={p().heat?.length}>
          <Chart class={half} title={t("heat.title")} text={t("heat.hint")}>
            <Heat profile={p()} />
          </Chart>
        </If>
      </View>
      {/* where requests went, operation → prefix → outcome: the widest, so the last */}
      <If when={!terminal() && p().flows?.length}>
        <Chart title={t("flows.title")} text={t("flows.hint")} testID="flows-chart">
          <Flows profile={p()} />
        </Chart>
      </If>
    </Scroll>
  );
}
