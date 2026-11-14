// Runs compared: one workload on two stores, two runs of a program, and a
// program over its runs.

import { signal, token, View, Text } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { Area, Dot, Grid, Line, Plot, PlotAxis, PlotLabel, Rule, Treemap } from "nsh/base/plot";
import { count, day, duration, factor, percent } from "../../format";
import { t } from "../../i18n";
import type { LatencyChange, ProgramHistory, RunChange, Profile } from "../../types";
import { areasWithRoom, AXIS_LABEL, CROSS_BAND, CROSS_LINE, Chart, ChartTip, DASH, GUTTER, GUTTER_SCALE, HEADROOM, HOVER, INACTIVE, LABEL, Legend, type LegendItem, MOTION, PLOT, TICK_ROOM, TREEMAP, axisName, decades, groupName, log10, rowsHeight, tipValue, BARS, GRID, STRIP, STROKE, TIP, TimeAxis, ValueAxis, sessionSpan, sinceTick, ROW_FIGURE, Budget, budgetTop } from "./system";
import { prefixNames } from "./where";
import { Figure, terminal, textual, SimpleTable, SURFACE, TONE } from "../kit";

// --- a trial over its clean run ------------------------------------------------------

/** A trial drawn over the clean run it is judged against, on one time axis:
 * the requests sent so far (a count that climbs, so a run of a few requests
 * is not drawn as a rate that leaps between nothing and hundreds a second)
 * and the median latency of the slices that sent any, and a rule at every
 * moment sonde answered in the store's place. What the faults cost shows as
 * the gap between the two lines after a rule. */
export function TrialOverClean(props: { trial: Profile; clean: Profile; faults: number[]; /** the stretch chosen on the timeline (ms from the start): both runs over it */ domainMs?: [number, number] | null }) {
  // (inside the stretch, with the slice that starts before it: the line enters the plot at its edge)
  const within = (tMs: number, binMs: number) => !props.domainMs || (tMs + binMs > props.domainMs[0] && tMs < props.domainMs[1]);
  const lineOf = (p: Profile) => {
    let sent = 0;
    // the count so far at each slice's end (counted from the run's start, whatever stretch is shown)
    return p.rate
      .map((b) => ({ tMs: b.tMs, n: b.ok + b.miss + b.client + b.server, p50: b.p50Ms }))
      .map((b) => ({ tMs: b.tMs, sent: (sent += b.n), p50: b.p50, has: b.n > 0 }))
      .filter((b) => within(b.tMs, p.binMs));
  };
  const trial = () => lineOf(props.trial);
  const clean = () => lineOf(props.clean);
  // one axis for both: the longer of the two runs, or the stretch chosen
  const span = (): [number, number] => {
    if (props.domainMs) return props.domainMs;
    const a = sessionSpan(props.trial, props.trial.rate.length);
    const b = sessionSpan(props.clean, props.clean.rate.length);
    return [0, Math.max(a[1], b[1])];
  };
  const sentTop = () => Math.max(1, ...trial().map((r) => r.sent), ...clean().map((r) => r.sent));
  const p50Top = () => Math.max(0.1, ...trial().map((r) => r.p50), ...clean().map((r) => r.p50));
  // the plot's own rows are the faults: the pointer rests on one, and the tooltip says when
  const faults = () => [...props.faults].filter((tMs) => within(tMs, 0)).sort((a, b) => a - b).map((tMs) => ({ tMs }));
  const hovered = signal<number | null>(null);
  const tip = () => {
    const f = hovered() === null ? null : faults()[hovered()!];
    return f ? { title: sinceTick(f.tMs), lines: [t("trialOver.fault")] } : null;
  };
  const label = () => t("trialOver.text", { trial: duration(props.trial.wallMs), clean: duration(props.clean.wallMs), n: props.faults.length });
  // the count steps up at each slice's end; the median is of the slices that sent a request, joined across the rest
  const of = (rows: ReturnType<typeof lineOf>, key: "sent" | "p50") => (key === "sent" ? rows : rows.filter((r) => r.has));
  const plot = (key: "sent" | "p50", top: () => number, say: (v: number) => string, axis: boolean, testID: string) => (
    <Plot
      testID={testID}
      data={faults()}
      x="tMs"
      xScale={{ type: "linear", domain: span() }}
      yScale={{ type: "linear", domain: [0, top() * HEADROOM.strip] }}
      accessibilityLabel={label()}
      accessibilityValue={tipValue(tip())}
      motion={MOTION}
      crossLineClass={CROSS_LINE}
      tooltipPlacement="pointer"
      tooltipOffset={10}
      tooltipContent={<ChartTip tip={tip()} />}
      onHoverChange={({ index }) => hovered.set(index)}
      class={`${axis ? TICK_ROOM : ""} ${STRIP} w-full`}
    >
      {/* where sonde answered in the store's place */}
      <Rule value={() => top() * HEADROOM.strip} class={`stroke-status-injected ${STROKE.line}`} />
      {/* the clean run under, dashed and quiet; the trial over it */}
      <Line data={of(clean(), key)} x="tMs" y={key} curve={key === "sent" ? "stepAfter" : "linear"} class={`stroke-muted-foreground ${STROKE.line}`} style={{ strokeDasharray: DASH.reference }} hoverDotClass="hidden" />
      <Line data={of(trial(), key)} x="tMs" y={key} curve={key === "sent" ? "stepAfter" : "linear"} class={`stroke-chart-1 ${STROKE.line}`} hoverDotClass="hidden" />
      <ValueAxis peak={top()} say={say} />
      <If when={axis}>
        <TimeAxis domain={span()} />
      </If>
    </Plot>
  );
  return (
    <If when={!terminal()} else={<Text class="text-foreground">{label()}</Text>}>
      <View testID="trial-over-clean" class="gap-2">
        {plot("sent", sentTop, (v) => t("shape.peak", { n: count(Math.round(v)) }), false, "trial-over-rate")}
        {plot("p50", p50Top, (v) => t("trialOver.median", { t: duration(v) }), true, "trial-over-latency")}
      </View>
    </If>
  );
}

/** What the trial's chart draws: its line, the clean run's dashed under it, and a rule where sonde answered. */
export function trialLegend(faults: boolean): LegendItem[] {
  return [
    { label: t("trialOver.trial"), swatch: "border-chart-1", line: "solid" },
    { label: t("trialOver.clean"), swatch: "border-muted-foreground", line: "dashed" },
    ...(faults ? [{ label: t("trialOver.faults"), swatch: TONE.injected.fill }] : []),
  ];
}

// --- one workload on two stores ---------------------------------------------------

/** The same requests on two stores: each kind's p95 on one beside the other,
 * the kind that slowed most first; the median as a dot on each bar. */
export function StoreLatency(props: { changes: LatencyChange[]; names: [string, string]; max?: number }) {
  const rows = () => props.changes.slice(0, props.max ?? 8).map((c) => ({ ...c, name: groupName(c), a95: c.p95Ms[0], b95: c.p95Ms[1], a50: c.p50Ms[0], b50: c.p50Ms[1] }));
  const hovered = signal<number | null>(null);
  const ratio = (c: LatencyChange) => c.p95Ms[1] / Math.max(c.p95Ms[0], 0.01);
  const label = () => {
    const c = rows()[0];
    return c ? t("latency.text", { what: c.name, a: duration(c.a95), b: duration(c.b95), x: factor(ratio(c)) }) : "";
  };
  const tip = () => {
    const c = hovered() === null ? null : rows()[hovered()!];
    return c ? { title: c.name, lines: [`${props.names[0]}: p50 ${duration(c.a50)} · p95 ${duration(c.a95)} · ${count(c.requests[0])}`, `${props.names[1]}: p50 ${duration(c.b50)} · p95 ${duration(c.b95)} · ${count(c.requests[1])}`] } : null;
  };
  const top = () => Math.max(...rows().flatMap((c) => [c.a95, c.b95]), 1) * 1.6;
  return (
    <If
      when={!textual()}
      else={<SimpleTable columns={[t("profile.kind"), `p50 ${props.names[0]} → ${props.names[1]}`, `p95 ${props.names[0]} → ${props.names[1]}`, "×"]} rows={rows().map((c) => [c.name, `${duration(c.a50)} → ${duration(c.b50)}`, `${duration(c.a95)} → ${duration(c.b95)}`, `${factor(ratio(c))}×`])} />}
    >
      <View class="gap-2">
        <Plot
          testID="store-latency"
          data={rows()}
          yScale={{ type: "band", ...BARS.pairs }}
          xScale={{ type: "linear", domain: [0, top()] }}
          accessibilityLabel={label()}
          accessibilityValue={tipValue(tip())}
          appearGeneration={1}
          motion={MOTION}
          crossLineClass={CROSS_BAND}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => hovered.set(index)}
          class={GUTTER}
          style={{ height: rowsHeight(rows().length) }}
        >
          {/* each kind's p95 on the first store (hollow) joined to the second's */}
          <Rule base="a95" value="b95" class={`stroke-chart-1/60 ${STROKE.link}`} inactiveOpacity={INACTIVE} />
          <Dot value="a95" r={4} class={`fill-card stroke-muted-foreground ${STROKE.line}`} inactiveOpacity={INACTIVE} />
          <Dot value="b95" r={4.5} class={`fill-chart-1 stroke-card ${STROKE.line}`} inactiveOpacity={INACTIVE} />
          <PlotLabel class={LABEL.figureStrong} texts={rows().map((r, i) => ({ text: `${duration(r.a95)} → ${duration(r.b95)} · ${factor(ratio(r))}×`, x: Math.max(r.a95, r.b95), y: i, align: "left" as const, class: ROW_FIGURE }))} />
          <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.name ?? "")} labelClass={AXIS_LABEL} />
        </Plot>
        <Legend gutter items={[{ label: `p95 · ${props.names[0]}`, swatch: "border border-muted-foreground bg-card" }, { label: `p95 · ${props.names[1]}`, swatch: "bg-chart-1" }]} />
      </View>
    </If>
  );
}

// --- two runs ---------------------------------------------------------------------

/** Two runs of a program by kind of request: the requests before (muted) and after (in color). */
export function RunBars(props: { change: RunChange; max?: number; /** two different programs: fewer is not better, more is not worse */ neutral?: boolean }) {
  const rows = () =>
    props.change.groups
      .filter((g) => g.requests[0] !== g.requests[1] || Math.abs(g.waitingMs[0] - g.waitingMs[1]) >= 1)
      .slice(0, props.max ?? 8)
      .map((g) => ({ name: groupName(g), before: g.requests[0], after: g.requests[1], tone: props.neutral ? "plain" : g.requests[1] < g.requests[0] ? "fewer" : g.requests[1] > g.requests[0] ? "more" : "same" }));
  const peak = () => Math.max(1, ...rows().map((r) => Math.max(r.before, r.after)));
  const hovered = signal<number | null>(null);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: r.name, lines: [t("profile.beforeAfter", { a: r.before, b: r.after })] } : null;
  };
  return (
    <If
      when={!textual()}
      else={<SimpleTable columns={[t("profile.kind"), t("profile.before"), t("profile.after")]} rows={rows().map((r) => [r.name, count(r.before), count(r.after)])} />}
    >
      <If when={rows().length > 0}>
        <Plot
          testID="run-bars"
          data={rows()}
          yScale={{ type: "band", ...BARS.pairs }}
          // room for the labels at the bars' ends
          xScale={{ type: "linear", domain: [0, peak() * 1.35] }}
          colorScale={{ type: "ordinal", domain: ["fewer", "more", "same", "plain"], range: [token("status-ok" as never) as unknown as string, token("status-server" as never) as unknown as string, token("muted-foreground" as never) as unknown as string, token("chart-1" as never) as unknown as string] }}
          accessibilityLabel={rows().map((r) => `${r.name} ${r.before} → ${r.after}`).join(", ")}
          accessibilityValue={tipValue(tip())}
          motion={MOTION}
          appearGeneration={props.change.requests.join("-")}
          crossLineClass={CROSS_BAND}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => hovered.set(index)}
          class={`${TICK_ROOM} ${GUTTER}`}
          style={{ height: rowsHeight(rows().length) }}
        >
          {/* a dumbbell: before (hollow) joined to after (in color); a drop to none still has its dot */}
          <Rule base="before" value="after" color="tone" class={STROKE.link} inactiveOpacity={INACTIVE} />
          <Dot value="before" r={4} class={`fill-card stroke-muted-foreground ${STROKE.line}`} inactiveOpacity={INACTIVE} />
          <Dot value="after" color="tone" r={4.5} class={`stroke-card ${STROKE.line}`} inactiveOpacity={INACTIVE} />
          {/* its figures on its own line (the label's middle on the row's), past the further dot */}
          <PlotLabel class={LABEL.figureStrong} texts={rows().map((r, i) => ({ text: r.before === r.after ? count(r.after) : `${count(r.before)} → ${count(r.after)}`, x: Math.max(r.before, r.after), y: i, align: "left" as const, class: ROW_FIGURE }))} />
          <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.name ?? "")} labelClass={AXIS_LABEL} />
          {/* how many: none, and the most of either run */}
          <PlotAxis placement="bottom" tickValues={[0, peak()]} format={(v) => (v === 0 ? "0" : t("run.bandRequests", { n: v }))} labelClass={LABEL.bucket} />
        </Plot>
        {/* which dot is which run: the earlier hollow, the later in the colour of its change */}
        <Legend
          gutter
          // the dots drawn, and no other: "more" under a run that only fell is a colour nothing has
          items={[
            { label: t("runBars.before"), swatch: "border border-muted-foreground bg-card" },
            ...(props.neutral
              ? [{ label: t("runBars.after"), swatch: "bg-chart-1" }]
              : [...(rows().some((r) => r.tone === "fewer") ? [{ label: t("runBars.fewer"), swatch: "bg-status-ok" }] : []), ...(rows().some((r) => r.tone === "more") ? [{ label: t("runBars.more"), swatch: "bg-status-server" }] : []), ...(rows().some((r) => r.tone === "same") ? [{ label: t("runBars.after"), swatch: "bg-muted-foreground" }] : [])]),
          ]}
        />
      </If>
    </If>
  );
}

/** Two runs by prefix: a rectangle per prefix, its area the later run's
 * requests (or the earlier's, for one it left), its color how it changed —
 * fewer cool, more warm, the same neutral — on one scale either side of none. */
export function PrefixChangeMap(props: { prefixes: { prefix: string; requests: [number, number] }[]; /** two different programs: fewer is not better, more is not worse */ neutral?: boolean }) {
  // a change as a share of the larger run, from −1 (all gone) to +1 (all new)
  const change = (a: number, b: number) => (a === b ? 0 : (b - a) / Math.max(a, b));
  const named = () => prefixNames(props.prefixes, 28);
  const rows = () =>
    props.prefixes
      .filter((x) => x.requests[0] + x.requests[1] > 0)
      .map((x) => ({ id: x.prefix, parent: "", label: named().name(x.prefix), value: Math.max(x.requests[1], x.requests[0] > 0 && x.requests[1] === 0 ? x.requests[0] : 0), change: change(x.requests[0], x.requests[1]), x }));
  // the areas with room for their name whole (and their figures under it): the others say nothing, their tooltip does
  const size = signal<[number, number]>([0, 0]);
  const room = () => areasWithRoom(rows(), size()[0], size()[1], TIP.areaGap, 0, (i) => rows()[i].label.length);
  const hovered = signal<number | null>(null);
  // (the same in both runs: one figure)
  const line = (x: { requests: [number, number] }) => (x.requests[0] === x.requests[1] ? count(x.requests[1]) : t("prefixChange.line", { a: count(x.requests[0]), b: count(x.requests[1]) }));
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: r.x.prefix === "other" ? r.label : r.x.prefix, lines: [line(r.x)] } : null;
  };
  const biggest = () => [...rows()].sort((p, q) => Math.abs(q.x.requests[1] - q.x.requests[0]) - Math.abs(p.x.requests[1] - p.x.requests[0]))[0];
  const label = () => (biggest() ? t("prefixChange.text", { prefix: biggest()!.label, line: line(biggest()!.x) }) : "");
  return (
    <If when={rows().length > 1}>
      <If when={!textual()} else={<SimpleTable columns={[t("common.prefix"), t("profile.before"), t("profile.after")]} rows={rows().map((r) => [r.label, count(r.x.requests[0]), count(r.x.requests[1])])} />}>
        <View class="gap-2">
          <Plot
            testID="prefix-change"
            data={rows()}
            colorScale={{ type: "diverging", domain: [-1, 1], pivot: 0, range: props.neutral ? [token("chart-4"), token("muted"), token("chart-1")] : [token("status-ok"), token("muted"), token("status-server")] }}
            accessibilityLabel={label()}
            accessibilityValue={tipValue(tip())}
            appearGeneration={rows().length}
            motion={MOTION}
            tooltipPlacement="pointer"
            tooltipOffset={10}
            tooltipContent={<ChartTip tip={tip()} />}
            onHoverChange={({ index }) => hovered.set(index)}
            onLayout={({ bounds }) => size.set([bounds.size.width, bounds.size.height])}
            class={TREEMAP}
          >
            <Treemap
              id="id"
              parent="parent"
              value="value"
              colorValue="change"
              paddingInner={TIP.areaGap}
              cornerRadii={TIP.area}
              inactiveOpacity={INACTIVE}
              label={(_, i) => (room()[i] === "none" ? "" : rows()[i].label)}
              valueLabel={(_, __, i) => (room()[i] === "both" ? line(rows()[i]!.x) : "")}
              // on a tile its change made dark, the words turn light (Native Shell marks it `data-dark`)
              labelClass={`${LABEL.area} data-dark:text-white`}
              valueLabelClass={`${LABEL.areaValue} data-dark:text-white/80`}
              hoverClass={HOVER}
            />
          </Plot>
          {/* the changes there are, and no other */}
          <Legend items={[...(rows().some((r) => r.change < 0) ? [{ label: t("prefixChange.fewer"), swatch: props.neutral ? "bg-chart-4" : "bg-status-ok" }] : []), ...(rows().some((r) => r.change === 0) ? [{ label: t("prefixChange.same"), swatch: "bg-muted" }] : []), ...(rows().some((r) => r.change > 0) ? [{ label: t("prefixChange.more"), swatch: props.neutral ? "bg-chart-1" : "bg-status-server" }] : [])]} />
        </View>
      </If>
    </If>
  );
}

// --- a program over its runs ------------------------------------------------------------

type Measure = "requests" | "waitingMs" | "cost" | "warnings";

/** One figure of a program over its runs: a line through them, the baseline dashed, the latest run labelled. */
export function RunTrend(props: { history: ProgramHistory; measure: Measure; title: string; format: (v: number) => string; onOpen?: (path: string) => void; /** the project's own limit on this measure (sonde.toml's `max-waiting`): its line */ budget?: number | null }) {
  const runs = () => props.history.runs;
  const values = () => runs().map((r) => r[props.measure] as number);
  const base = () => {
    const b = props.history.baselinePoint ?? runs().find((r) => r.path === props.history.baseline);
    return b ? (b[props.measure] as number) : null;
  };
  // what a run may grow to before sonde check fails it (no band for warnings)
  const limit = () => {
    const l = props.history.limits;
    if (!l) return null;
    return props.measure === "requests" ? l.requests : props.measure === "waitingMs" ? l.waitingMs : props.measure === "cost" ? l.cost : null;
  };
  const hi = () => budgetTop(Math.max(...values(), base() ?? 0, limit() ?? 0, 1e-9) * 1.25, props.budget);
  const hovered = signal<number | null>(null);
  const shown = () => runs()[hovered() ?? runs().length - 1];
  const last = () => runs().length - 1;
  // the run under the pointer: when it ran, its figure, and the baseline's beside it
  const tip = () => {
    const r = hovered() === null ? null : runs()[hovered()!];
    return r ? { title: day(r.created), lines: [props.format(r[props.measure] as number), ...(base() !== null ? [t("trend.baseline", { v: props.format(base()!) })] : [])] } : null;
  };
  const label = () => `${props.title}: ${values().map(props.format).join(", ")}`;
  return (
    // the latest run's figure (or the one under the pointer) where a chart's control is
    <Chart testID={`trend-${props.measure}`} class={`${SURFACE.trend} flex-1`} title={props.title} action={<Figure value={props.format(shown()[props.measure] as number)} label={day(shown().created)} />}>
      <BaseButton
        accessibilityLabel={label()}
        disabled={!props.onOpen}
        onPress={() => {
          const r = hovered() === null ? null : runs()[hovered()!];
          if (r) props.onOpen?.(r.path);
        }}
      >
        <Plot
          data={runs().map((r, i) => ({ v: r[props.measure] as number, i }))}
          y="v"
          yScale={{ type: "linear", domain: [0, hi()] }}
          accessibilityLabel={label()}
          accessibilityValue={tipValue(tip())}
          appearGeneration={props.history.program}
          motion={MOTION}
          crossLineClass={CROSS_LINE}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => hovered.set(index)}
          // a line through a few runs, not rows
          class={`mr-1 ${STRIP}`}
        >
          <If when={base() !== null}>
            {/* the baseline, and up to what sonde check allows */}
            <If when={limit() !== null}>
              <Area y0={() => base()!} y1={() => limit()!} curve="linear" class="fill-muted-foreground/12" />
            </If>
            <Grid y={[base()!]} class={`stroke-muted-foreground/60 ${STROKE.hair}`} style={{ strokeDasharray: DASH.reference }} />
          </If>
          <Budget value={props.budget} say={props.format} />
          <Line curve="linear" class={`stroke-chart-1 ${STROKE.trend}`} dot dotClass="size-1.5 rounded-full bg-chart-1" hoverDotClass="size-2.5 rounded-full border-2 border-card bg-chart-1" />
        </Plot>
      </BaseButton>
    </Chart>
  );
}

/** Two runs' latency as cumulative curves on the same edges: the share of
 * requests done within each time, before dashed and after solid. A curve to
 * the left is faster; where they part says for which requests. */
export function DistributionCompare(props: { latency: [{ leMs: number | null; n: number }[], { leMs: number | null; n: number }[]]; names: [string, string] }) {
  const rows = () => {
    const edges = [...new Set([...props.latency[0], ...props.latency[1]].map((b) => b.leMs).filter((e): e is number => e !== null))].sort((a, b) => a - b);
    const cum = (bs: { leMs: number | null; n: number }[]) => {
      const total = Math.max(1, bs.reduce((s, b) => s + b.n, 0));
      return edges.map((e) => (bs.filter((b) => b.leMs !== null && b.leMs <= e).reduce((s, b) => s + b.n, 0) * 100) / total);
    };
    const a = cum(props.latency[0]);
    const b = cum(props.latency[1]);
    return edges.map((e, i) => ({ e, x: log10(e), a: a[i], b: b[i] }));
  };
  const label = () => t("cdf.text", { a: props.names[0], b: props.names[1] });
  const hovered = signal<number | null>(null);
  // the share of each run's requests done within the time under the pointer
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: `≤ ${duration(r.e)}`, lines: [`${props.names[0]}: ${percent(r.a / 100)}`, `${props.names[1]}: ${percent(r.b / 100)}`] } : null;
  };
  return (
    <If when={!terminal() && rows().length > 1}>
      <Plot
        testID="distribution-compare"
        data={rows()}
        x="x"
        xScale={{ type: "linear", domain: [rows()[0].x, rows()[rows().length - 1].x] }}
        yScale={{ type: "linear", domain: [0, 100] }}
        accessibilityLabel={label()}
        accessibilityValue={tipValue(tip())}
        motion={MOTION}
        crossLineClass={CROSS_LINE}
        tooltipPlacement="pointer"
        tooltipOffset={10}
        tooltipContent={<ChartTip tip={tip()} />}
        onHoverChange={({ index }) => hovered.set(index)}
        // its scale outside, in the room for one: "95%" never on a curve
        class={`${TICK_ROOM} ${GUTTER_SCALE} ${PLOT}`}
      >
        {/* the median and p95 */}
        <Grid y={[50, 95]} class={GRID} />
        <Line y="a" curve="stepAfter" class={`stroke-muted-foreground/70 ${STROKE.line}`} style={{ strokeDasharray: DASH.reference }} hoverDotClass="size-1.5 rounded-full bg-muted-foreground" />
        <Line y="b" curve="stepAfter" class={`stroke-chart-1 ${STROKE.line}`} hoverDotClass="size-1.5 rounded-full bg-chart-1" />
        <PlotAxis placement="left" tickValues={[50, 95]} format={(v) => percent(v / 100)} labelClass={LABEL.scale} />
        {/* a log scale ticks at its decades: 1 ms, 10 ms, 100 ms, never 3.16 ms */}
        <PlotAxis tickValues={decades(rows()[0].x, rows()[rows().length - 1].x)} format={(v) => duration(10 ** v)} labelClass={LABEL.bucket} />
      </Plot>
    </If>
  );
}
