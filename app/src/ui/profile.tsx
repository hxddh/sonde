// A session's time: what the program waited on, by kind of request, and how
// long its requests took. The charts follow the order a question about S3
// runs: when, how long (each request), which kind and when, what pushed
// back, where the time went (by kind, or by the code that asked), and
// whether a prefix ran into its limits. A stretch picked on the timeline
// zooms every chart drawn over time.

import { batch, effect, type JSX, Scroll, signal, Text, untracked, View } from "nsh";
import { t } from "../i18n";
import { count, duration } from "../format";
import type { SessionView } from "../types";
import { Button, Notice, Options, terminal } from "./kit";
import { findingMarks } from "./sessionview";
import { Chains, Chart, Distribution, timed, Flows, focusOf, HALF, Heat, KindsOverTime, Lanes, type LegendItem, PrefixMeasure, type PrefixMeasureKind, PrefixUse, Pressure, requestsOf, RequestScatter, SessionShape, shapeSentence, TimeSpent } from "./plots";

/** A chart of the pane: when it has anything to show, its frame's words and
 * control, and its body. The pane is a list of these drawn by one loop, so
 * no chart has a frame of its own. `wide` takes the board's whole width;
 * the others go two abreast where there is room. */
interface ChartSpec {
  id: string;
  when: () => boolean;
  wide?: boolean;
  title: () => string;
  text?: () => string;
  action?: () => JSX.Element;
  legend?: () => LegendItem[];
  testID?: string;
  body: () => JSX.Element;
}

/** The Time pane of a session. */
export function TimePane(props: { view: SessionView; onRequest?: (id: number) => void }) {
  const p = () => props.view.profile;
  // a stretch of the session, picked on the timeline by two presses
  const zoomFrom = signal<number | null>(null);
  const zoom = signal<[number, number] | null>(null);
  const pick = (i: number | null) => {
    // pressing the first slice again lets it go: a stretch is two slices at least
    // (one slice is no shape over time: the timeline and the charts would have nothing to draw)
    if (i === null) return zoomFrom.set(null);
    const from = zoomFrom();
    if (from === i) return zoomFrom.set(null);
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
  // the moment the pointer rests on in one chart, marked in the others: this pane's own
  const focus = focusOf();
  const unzoom = () => batch(() => (zoom.set(null), zoomFrom.set(null)));
  const graphic = () => !terminal();
  // in the order a question about S3 runs: when, how long, what ran at once, where the time
  // went, how it was spread, what pushed back, what waited, where by prefix, when and where, and
  // (the widest, so the last) where requests went
  const charts: ChartSpec[] = [
    {
      // the whole session, its findings marked; the stretch picked here is what every chart over time shows
      id: "timeline",
      wide: true,
      when: () => graphic() && shaped(),
      title: () => t("timeline.title"),
      text: () => (zoom() ? t("timeline.zoomed", { a: duration(zoomMs()![0]), b: duration(zoomMs()![1]) }) : zoomFrom() !== null ? t("timeline.zoomFrom", { t: duration((zoomFrom() ?? 0) * p().binMs) }) : t("timeline.hint")),
      action: () => (
        <If when={zoom() || zoomFrom() !== null}>
          <Button small variant="outline" icon="x" label={t("timeline.unzoom")} onPress={unzoom} testID="zoom-reset" />
        </If>
      ),
      // what the numbers over the slices are
      legend: () => (findingMarks(props.view).length > 0 ? [{ label: t("shape.findingMarks"), swatch: "bg-warn" }] : []),
      body: () => <SessionShape profile={p()} annotate axis marks={findingMarks(props.view)} range={zoom()} slice={zoomFrom()} onSlice={pick} focus={focus} class="mb-6 mt-4 h-28 w-full" testID="time-shape" />,
    },
    {
      // every request, when it ran or how large it was (a press opens it), or the busiest kinds each over time
      id: "scatter",
      when: () => graphic() && timed(p()) && p().samples.length > 1,
      title: () => t("scatter.title"),
      // above what a session keeps of its requests, the dots are of a sample: said
      text: () => (scatterBy() === "kinds" ? t("kinds.title") : `${t("scatter.text", { n: shown(), p95: duration(p().p95Ms ?? 0) })}${p().samples.length < requestsOf(p()) ? ` · ${t("plots.sampled", { n: count(p().samples.length), of: count(requestsOf(p())) })}` : ""}`),
      action: () => <Options testID="scatter-by" value={scatterBy()} items={[{ value: "time", title: t("scatter.byTime") }, { value: "size", title: t("scatter.bySize") }, ...((p().groupRate?.length ?? 0) > 0 ? [{ value: "kinds", title: t("scatter.byKind") }] : [])]} onChange={(v) => scatterBy.set(v as "time" | "size" | "kinds")} />,
      body: () => (
        <If when={scatterBy() === "kinds"} else={<RequestScatter profile={p()} by={scatterBy() === "size" ? "size" : "time"} domainMs={zoomMs()} onRequest={props.onRequest} />}>
          <KindsOverTime profile={p()} domainMs={zoomMs()} />
        </If>
      ),
    },
    {
      // what ran at once, what waited: each request on its lane
      id: "lanes",
      testID: "lanes-chart",
      when: () => graphic() && timed(p()) && p().samples.length > 1,
      title: () => t("lanes.title"),
      text: () => t("lanes.hint"),
      body: () => <Lanes profile={p()} domainMs={zoomMs()} onRequest={props.onRequest} focus={focus} />,
    },
    {
      id: "spent",
      when: () => timed(p()),
      title: () => t("profile.spentTitle"),
      text: () => t("profile.spentHint"),
      action: () => (
        <If when={(p().callers?.length ?? 0) > 0}>
          <Options testID="spent-by" value={spentBy()} items={[{ value: "kind", title: t("spent.byKind") }, { value: "code", title: t("spent.byCode") }]} onChange={(v) => spentBy.set(v as "kind" | "code")} />
        </If>
      ),
      body: () => <TimeSpent profile={p()} by={spentBy()} />,
    },
    { id: "distribution", when: () => !!p().latency?.length, title: () => t("dist.title"), text: () => t("dist.hint"), body: () => <Distribution profile={p()} /> },
    { id: "pressure", when: pushedBack, title: () => t("pressure.title"), text: () => t("pressure.hint"), body: () => <Pressure profile={p()} range={zoom()} /> },
    { id: "chains", when: () => !!p().chains?.length, title: () => t("chains.title"), text: () => t("chains.hint"), body: () => <Chains profile={p()} /> },
    {
      // where by prefix: how many, what they cost, how near the limit
      id: "prefixes",
      when: () => p().prefixes.some((x) => x.prefix !== "other"),
      title: () => t("prefixUse.title"),
      text: () => t("prefixUse.hint"),
      action: () => <PrefixMeasure profile={p()} value={measure()} onChange={(m) => measure.set(m)} />,
      body: () => <PrefixUse profile={p()} measure={measure()} />,
    },
    { id: "heat", when: () => !!p().heat?.length, title: () => t("heat.title"), text: () => t("heat.hint"), body: () => <Heat profile={p()} range={zoom()} /> },
    { id: "flows", wide: true, testID: "flows-chart", when: () => graphic() && !!p().flows?.length, title: () => t("flows.title"), text: () => t("flows.hint"), body: () => <Flows profile={p()} /> },
  ];
  return (
    <Scroll testID="time-pane" class="min-h-0 flex-1" contentClass={terminal() ? "gap-2" : "flex-row flex-wrap items-stretch gap-4 pb-2"}>
      {/* the figures are the session header's; here, a terminal's sentence */}
      <If when={terminal() && timed(p())}>
        <Text class="text-foreground">{shapeSentence(p())}</Text>
      </If>
      {/* a log without times (CloudTrail): when and what, never how long */}
      <If when={!timed(p())}>
        <View class="w-full">
          <Notice testID="untimed" text={t("profile.untimed")} />
        </View>
      </If>
      {/* the board: every chart in the one frame, by one loop */}
      <Each of={charts.filter((c) => c.when())} key={(c) => c.id}>
        {(c) => (
          <Chart class={terminal() ? "" : c().wide ? "w-full" : HALF} testID={c().testID} title={c().title()} text={c().text?.()} action={c().action?.()} legend={c().legend?.()}>
            {c().body()}
          </Chart>
        )}
      </Each>
    </Scroll>
  );
}
