// A session's time: what the program waited on, by kind of request, and how
// long its requests took. The board is four groups, in the order a question
// about S3 runs — Traffic (when, how many at once, how much), Latency (how
// long, of which kind, where the time went), Errors (what failed and what
// was answered), Where (which prefixes, at what cost, on which path) — each
// a few charts in the one frame. A stretch picked on the timeline zooms
// every chart drawn over time; a signal pressed in the session's header
// brings its chart into view.

import { batch, effect, type JSX, Scroll, type ScrollRef, signal, Text, untracked, View } from "nsh";
import { t } from "../i18n";
import { count, duration } from "../format";
import { type SignalId, signalsOf } from "../signals";
import type { SessionView, Signals } from "../types";
import { Badge, Button, Heading, Notice, Options, Row, terminal } from "./kit";
import { findingMarks, StatusMark } from "./sessionview";
import { BytesOverTime, Chains, Chart, CostBy, Distribution, ErrorsOverTime, failureLegend, FirstByte, timed, Flows, focusOf, HALF, Heat, KindsOverTime, Lanes, LatencyBand, type LegendItem, PrefixMeasure, type PrefixMeasureKind, PrefixUse, requestsOf, RequestScatter, SessionShape, shapeSentence, Sizes, Throughput, TimeSpent } from "./plots";

/** The board's groups, in order: never more than these four. */
const GROUPS = ["traffic", "latency", "errors", "where"] as const;
type Group = (typeof GROUPS)[number];

/** A chart of the pane: its group, when it has anything to show, its frame's
 * words and control, and its body. The pane is a list of these drawn by one
 * loop, so no chart has a frame of its own. `wide` takes the board's whole
 * width; the others go two abreast where there is room. */
interface ChartSpec {
  id: string;
  group: Group;
  when: () => boolean;
  wide?: boolean;
  title: () => string;
  text?: () => string;
  action?: () => JSX.Element;
  legend?: () => LegendItem[];
  testID?: string;
  body: () => JSX.Element;
}

/** The chart a signal's tile opens. */
const SIGNAL_CHART: Record<SignalId, string> = { rate: "timeline", errors: "errors", latency: "latency", bytes: "bytes", inFlight: "lanes" };

/** Every answer the session got: its status, S3's code for it, how many, how
 * many of them a fault scenario's, and when the first and the last came. */
function Statuses(props: { signals: Signals }) {
  const rows = () => props.signals.statuses.slice(0, 12);
  const span = (x: Signals["statuses"][number]) => (!props.signals.timed ? "" : x.count > 1 ? `+${duration(x.firstMs)} – +${duration(x.lastMs)}` : `+${duration(x.firstMs)}`);
  if (terminal()) {
    return (
      <View testID="statuses">
        <Each of={rows()}>{(x) => <Text class="text-foreground">{[x().status ?? t("statuses.noAnswer"), x().code ?? "", `× ${count(x().count)}`, x().injected ? t("statuses.injected", { n: count(x().injected) }) : "", span(x())].filter(Boolean).join("  ")}</Text>}</Each>
      </View>
    );
  }
  return (
    <View testID="statuses">
      <Each of={rows()} key={(x) => `${x.status}:${x.code}`}>
        {(x, i) => (
          <Row first={i() === 0} class="px-0">
            <View class="w-16 shrink-0">
              <StatusMark status={x().status} text={x().status === null ? "—" : undefined} />
            </View>
            <Text class="min-w-0 flex-1 font-mono text-small text-foreground line-clamp-1">{x().code ?? (x().status === null ? t("statuses.noAnswer") : "")}</Text>
            <If when={x().injected > 0}>
              <Badge tone="info" label={t("statuses.injected", { n: count(x().injected) })} />
            </If>
            <Text class="w-28 shrink-0 text-right tabular-nums text-caption text-muted-foreground line-clamp-1">{span(x())}</Text>
            <Text class="w-12 shrink-0 text-right tabular-nums text-small text-foreground">{count(x().count)}</Text>
          </Row>
        )}
      </Each>
    </View>
  );
}

/** The Time pane of a session. */
export function TimePane(props: { view: SessionView; onRequest?: (id: number) => void; /** a signal pressed in the header: its chart comes into view (`n` tells one press from the next) */ reveal?: { id: SignalId; n: number } | null }) {
  const p = () => props.view.profile;
  // the four signals, as Rust counted them (an older share: what its profile holds of them)
  const sig = () => signalsOf(props.view)!;
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
  const sizesBy = signal<"count" | "speed">("count");
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
  const overTime = () => timed(p()) && shaped();
  const failed = () => sig().errors.count + sig().errors.retriesTotal > 0;
  const charts: ChartSpec[] = [
    // --- traffic: when, how many at once, how much
    {
      // the whole session, its findings marked; the stretch picked here is what every chart over time shows
      id: "timeline",
      group: "traffic",
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
      // what ran at once, what waited: each request on its lane
      id: "lanes",
      group: "traffic",
      testID: "lanes-chart",
      when: () => graphic() && timed(p()) && p().samples.length > 1,
      title: () => t("lanes.title"),
      text: () => t("lanes.hint"),
      body: () => <Lanes profile={p()} domainMs={zoomMs()} onRequest={props.onRequest} focus={focus} injected={props.view.injected} />,
    },
    {
      // bodies a second, each way
      id: "bytes",
      group: "traffic",
      when: () => overTime() && sig().bytes.upBytes + sig().bytes.downBytes > 0,
      title: () => t("bytesTime.title"),
      text: () => t("bytesTime.hint"),
      legend: () => [{ label: t("bytesTime.up"), swatch: "bg-chart-1" }, { label: t("bytesTime.down"), swatch: "bg-chart-4" }],
      body: () => <BytesOverTime signals={sig()} range={zoom()} focus={focus} />,
    },
    {
      // how large the bodies were, or how fast each size moved
      id: "sizes",
      group: "traffic",
      when: () => sig().sizes.request.length + sig().sizes.response.length > 1,
      title: () => t("sizes.title"),
      text: () => (sizesBy() === "speed" ? t("sizes.hintSpeed") : t("sizes.hint")),
      action: () => (
        <If when={sig().sizes.throughput.length > 0}>
          <Options testID="sizes-by" value={sizesBy()} items={[{ value: "count", title: t("sizes.byCount") }, { value: "speed", title: t("sizes.bySpeed") }]} onChange={(v) => sizesBy.set(v as "count" | "speed")} />
        </If>
      ),
      legend: () => (sizesBy() === "speed" ? [] : [{ label: t("sizes.sent"), swatch: "bg-chart-1" }, { label: t("sizes.received"), swatch: "bg-chart-4" }]),
      body: () => (
        <If when={sizesBy() === "speed" && sig().sizes.throughput.length > 0} else={<Sizes signals={sig()} />}>
          <Throughput signals={sig()} />
        </If>
      ),
    },
    // --- latency: how long, of which kind, where the time went
    {
      id: "latency",
      group: "latency",
      when: () => graphic() && overTime(),
      title: () => t("latencyTime.title"),
      text: () => t("latencyTime.hint"),
      legend: () => [{ label: t("latencyTime.p50"), swatch: "bg-chart-1" }, { label: t("latencyTime.p95"), swatch: "bg-chart-1/25" }, { label: t("latencyTime.p99"), swatch: "bg-muted-foreground/15" }],
      body: () => <LatencyBand profile={p()} domainMs={zoomMs()} focus={focus} class={`mb-6 h-44 w-full`} testID="latency-over-time" axis />,
    },
    {
      // every request, when it ran or how large it was (a press opens it), or the busiest kinds each over time
      id: "scatter",
      group: "latency",
      when: () => graphic() && timed(p()) && p().samples.length > 1,
      title: () => t("scatter.title"),
      // above what a session keeps of its requests, the dots are of a sample: said
      text: () => (scatterBy() === "kinds" ? t("kinds.title") : `${t("scatter.text", { n: shown(), p95: duration(p().p95Ms ?? 0) })}${p().samples.length < requestsOf(p()) ? ` · ${t("plots.sampled", { n: count(p().samples.length), of: count(requestsOf(p())) })}` : ""}`),
      action: () => <Options testID="scatter-by" value={scatterBy()} items={[{ value: "time", title: t("scatter.byTime") }, { value: "size", title: t("scatter.bySize") }, ...((p().groupRate?.length ?? 0) > 0 ? [{ value: "kinds", title: t("scatter.byKind") }] : [])]} onChange={(v) => scatterBy.set(v as "time" | "size" | "kinds")} />,
      body: () => (
        <If when={scatterBy() === "kinds"} else={<RequestScatter profile={p()} by={scatterBy() === "size" ? "size" : "time"} domainMs={zoomMs()} onRequest={props.onRequest} injected={props.view.injected} />}>
          <KindsOverTime profile={p()} domainMs={zoomMs()} />
        </If>
      ),
    },
    {
      id: "spent",
      group: "latency",
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
    { id: "distribution", group: "latency", when: () => !!p().latency?.length, title: () => t("dist.title"), text: () => t("dist.hint"), body: () => <Distribution profile={p()} /> },
    {
      // the wait for the first byte against the rest: only where a first byte was timed
      id: "firstByte",
      group: "latency",
      when: () => !!sig().firstByte?.kinds.length,
      title: () => t("firstByte.title"),
      text: () => t("firstByte.hint"),
      legend: () => [{ label: t("firstByte.first"), swatch: "bg-chart-1" }, { label: t("firstByte.rest"), swatch: "bg-chart-4" }],
      body: () => <FirstByte signals={sig()} />,
    },
    { id: "chains", group: "latency", when: () => !!p().chains?.length, title: () => t("chains.title"), text: () => t("chains.hint"), body: () => <Chains profile={p()} /> },
    // --- errors: what failed, and every answer the store gave
    {
      id: "errors",
      group: "errors",
      when: () => failed() && (shaped() || terminal()),
      title: () => t("failures.title"),
      text: () => t("failures.hint"),
      legend: () => failureLegend(sig()),
      body: () => <ErrorsOverTime signals={sig()} range={zoom()} focus={focus} />,
    },
    { id: "statuses", group: "errors", when: () => sig().statuses.length > 0, title: () => t("statuses.title"), text: () => t("statuses.hint"), body: () => <Statuses signals={sig()} /> },
    // --- where: which prefixes, at what cost, on which path
    {
      // where by prefix: how many, what they cost, how near the limit
      id: "prefixes",
      group: "where",
      when: () => p().prefixes.some((x) => x.prefix !== "other"),
      title: () => t("prefixUse.title"),
      text: () => t("prefixUse.hint"),
      action: () => <PrefixMeasure profile={p()} value={measure()} onChange={(m) => measure.set(m)} />,
      body: () => <PrefixUse profile={p()} measure={measure()} rates={sig().prefixes} />,
    },
    { id: "cost", group: "where", when: () => sig().cost.operations.some((x) => x.cost > 0), title: () => t("costBy.title"), text: () => t("costBy.hint"), body: () => <CostBy signals={sig()} /> },
    { id: "heat", group: "where", when: () => !!p().heat?.length, title: () => t("heat.title"), text: () => t("heat.hint"), body: () => <Heat profile={p()} range={zoom()} /> },
    { id: "flows", group: "where", wide: true, testID: "flows-chart", when: () => graphic() && !!p().flows?.length, title: () => t("flows.title"), text: () => t("flows.hint"), body: () => <Flows profile={p()} /> },
  ];
  const of = (g: Group) => charts.filter((c) => c.group === g && c.when());
  // where each group and each chart stands on the board: a signal pressed scrolls its chart into view
  let scroller: ScrollRef | undefined;
  const groupY: Partial<Record<Group, number>> = {};
  const rowY: Partial<Record<Group, number>> = {};
  const chartY: Record<string, number> = {};
  let revealed = -1;
  effect(() => {
    const r = props.reveal;
    if (!r || r.n === revealed) return;
    revealed = r.n;
    const chart = charts.find((c) => c.id === SIGNAL_CHART[r.id]);
    // an error-free session has no errors chart: its answers, then
    const target = chart && untracked(chart.when) ? chart : charts.find((c) => c.group === (chart?.group ?? "traffic") && untracked(c.when));
    if (!target) return;
    // once the board is laid out
    setTimeout(() => scroller?.scrollTo({ y: Math.max(0, (groupY[target.group] ?? 0) + ((chartY[target.id] ?? 0) > 0 ? (rowY[target.group] ?? 0) + chartY[target.id] - 8 : 0)), animated: true }), 80);
  });
  return (
    <Scroll ref={(r) => (scroller = r)} testID="time-pane" class="min-h-0 flex-1" contentClass={terminal() ? "gap-2" : "gap-6 pb-2"}>
      {/* the figures are the session header's; here, a terminal's sentence */}
      <If when={terminal() && timed(p())}>
        <Text class="text-foreground">{shapeSentence(p())}</Text>
      </If>
      {/* a log without times (CloudTrail): when and what, never how long */}
      <If when={!timed(p())}>
        <Notice testID="untimed" text={t("profile.untimed")} />
      </If>
      {/* the board: four groups at most, every chart in the one frame, by one loop */}
      <Each of={GROUPS.filter((g) => of(g).length > 0)} key={(g) => g}>
        {(g) => (
          <View testID={`board-${g()}`} class={terminal() ? "gap-2" : "gap-3"} onLayout={({ bounds }) => (groupY[g()] = bounds.origin.y)}>
            <Heading>{t(`board.${g()}`)}</Heading>
            <View class={terminal() ? "gap-2" : "flex-row flex-wrap items-stretch gap-4"} onLayout={({ bounds }) => (rowY[g()] = bounds.origin.y)}>
              <Each of={of(g())} key={(c) => c.id}>
                {(c) => (
                  <View class={terminal() ? "" : c().wide ? "w-full" : HALF} onLayout={({ bounds }) => (chartY[c().id] = bounds.origin.y)}>
                    <Chart class="grow" testID={c().testID ?? `chart-${c().id}`} title={c().title()} text={c().text?.()} action={c().action?.()} legend={c().legend?.()}>
                      {c().body()}
                    </Chart>
                  </View>
                )}
              </Each>
            </View>
          </View>
        )}
      </Each>
    </Scroll>
  );
}
