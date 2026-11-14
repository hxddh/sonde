// Latency: how long requests took — over the session, every one of them,
// by kind, as a distribution, the first byte against the rest, chains, and
// where the time went.

import { computed, signal, Text, token, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { Area, Bar, Dot, Grid, Line, Plot, PlotAxis, PlotLabel, Rule, Cell } from "nsh/base/plot";
import { bytes, cost, count, duration, percent, transfer } from "../../format";
import { t } from "../../i18n";
import type { Profile, Signals } from "../../types";
import { FEW } from "../../signals";
import { AXIS_LABEL, CROSS_BAND, CROSS_LINE, ChartTip, type Focus, GUTTER, GUTTER_CHARS, GUTTER_SCALE, GUTTER_SCALE_END, INACTIVE, LABEL, Legend, MOTION, NoneInStretch, PITCH, PLOT, ROW_FIGURE, ROW_FIGURE_BEFORE, ROW_LABEL, TICK_ROOM, axisName, decades, groupName, injectedLegend, log10, nearest, pct, rowsHeight, tipValue, toneOf, tones, tail, type LegendItem, OUTCOMES, TimeAxis, ValueAxis, sessionSpan, BARS, DASH, FOCUS_RULE, GRID, HEADROOM, STRIP, STROKE, TIP, CELLS, HOVER, sinceTick, slotMs, Budget, budgetTop, HEAT_RAMP, ZERO_SLIVER } from "./system";
import { terminal, textual, SimpleTable, TEXT } from "../kit";
import { scatterSaid } from "./answers";

/** The two tones of latency's bands: the body of the requests (median to
 * p95) and their tail (p95 to p99). */
export const LATENCY_FILL = { body: "fill-latency-body", tail: "fill-latency-tail" } as const;
/** What the bands stand for, for a frame's legend. */
/** Requests in a slice of the profile. */
const inSlice = (b: Profile["rate"][number]) => b.ok + b.miss + b.client + b.server;
/** Whether a slice's p95 and p99 are drawn: of `FEW` requests or more (in a slice of two, its "p95" is its slower
 * request, and a spike of it says nothing of a tail). */
const tells = (b: Profile["rate"][number]) => inSlice(b) >= FEW;
/** Whether any slice has a tail to draw: enough requests, and a p99 past its p95. */
export const hasTail = (profile: Profile): boolean => profile.rate.some((b) => tells(b) && b.p99Ms > b.p95Ms + 1e-9);
/** The bands' legend: the tail is named where one is drawn; where no slice has one, that is said in its place. */
export const latencyLegend = (profile?: Profile): LegendItem[] => [
  { label: t("latencyTime.p50"), swatch: "border-foreground", line: "solid" },
  ...(!profile || profile.rate.some(tells) ? [{ label: t("latencyTime.p95"), swatch: "bg-latency-body" }] : []),
  ...(!profile || hasTail(profile) ? [{ label: t("latencyTime.p99"), swatch: "bg-latency-tail" }] : []),
];

/** How long requests took over the session, on a log scale (a decade a step, as the scatter's): the median a line,
 * the band from it to p95 and the band on to p99 where a slice has `FEW` requests or more; slices without requests
 * leave a gap. */
export function LatencyBand(props: { profile: Profile; class?: string; testID?: string; domainMs?: [number, number] | null; focus?: Focus; /** on the board: its time axis under it */ axis?: boolean; /** the project's `max-p95`, in ms: its line */ budgetMs?: number | null }) {
  // every slice, so one without requests leaves a gap rather than a line across it
  const rows = () => props.profile.rate;
  const has = (b: Profile["rate"][number]) => inSlice(b) > 0;
  const answered = () => rows().filter(has);
  const lg = (ms: number) => log10(Math.max(ms, 0.01));
  const gap = (ok: (b: Profile["rate"][number]) => boolean, f: (b: Profile["rate"][number]) => number) => (b: Profile["rate"][number]) => (ok(b) ? lg(f(b)) : (undefined as unknown as number));
  // the scale of what is shown: the zoomed stretch's slices only
  const inView = () => {
    const d = props.domainMs;
    return d ? answered().filter((b) => b.tMs >= d[0] && b.tMs < d[1]) : answered();
  };
  // from under the fastest median to over the slowest drawn (and the budget's line): a decade at least
  const yRange = (): [number, number] => {
    const v = inView();
    const lo = lg(Math.min(...v.map((b) => b.p50Ms), props.budgetMs ?? Infinity, 1e9));
    const hi = lg(Math.max(...v.map((b) => (tells(b) ? b.p99Ms : b.p50Ms)), props.budgetMs ?? 0, 0.01));
    const spare = Math.max(0, 1 - (hi - lo)) / 2;
    return [lo - spare - 0.1, hi + spare + 0.15];
  };
  // a decade's line and its words (1 ms, 10 ms…); a scale under a decade wide names its two ends
  const ticks = () => {
    const [a, b] = yRange();
    return decades(Math.ceil(a - 1e-9), b).filter((d) => d >= a && d <= b);
  };
  const hovered = signal<number | null>(null);
  const domain = (): [number, number] => props.domainMs ?? sessionSpan(props.profile, props.profile.rate.length);
  const label = () => {
    const last = answered()[answered().length - 1];
    return last ? t("band.text", { p50: duration(last.p50Ms), p95: duration(last.p95Ms), p99: duration(last.p99Ms) }) : "";
  };
  const tip = () => {
    const b = hovered() === null ? null : rows()[hovered()!];
    if (!b || !has(b)) return null;
    // a slice of a few: its median, and how many (no tail to tell)
    return { title: `+${duration(b.tMs)}`, lines: [tells(b) ? t("band.line", { p50: duration(b.p50Ms), p95: duration(b.p95Ms), p99: duration(b.p99Ms) }) : t("band.lineFew", { p50: duration(b.p50Ms) }), t("sessions.requestsN", { n: inSlice(b) })] };
  };
  return (
    <If when={!terminal() && answered().length > 1} else={<Text class="hidden text-muted-foreground terminal:flex">{label()}</Text>}>
      <Plot
        testID={props.testID ?? "latency-band"}
        data={rows()}
        x="tMs"
        xScale={{ type: "linear", domain: domain() }}
        yScale={{ type: "linear", domain: yRange() }}
        accessibilityLabel={label()}
        accessibilityValue={tipValue(tip())}
        motion={MOTION}
        crossLineClass={CROSS_LINE}
        tooltipPlacement="pointer"
        tooltipOffset={10}
        tooltipContent={<ChartTip tip={tip()} />}
        onHoverChange={({ index }) => {
          hovered.set(index);
          props.focus?.set(index === null ? null : rows()[index].tMs);
        }}
        // the scale's room at its start: bands and the heat map beside it (one card) start at one edge
        class={`${GUTTER_SCALE} ${props.class ?? STRIP}`}
      >
        <Grid y={ticks()} class={GRID} />
        {/* straight between slices: a curve would show values no request had */}
        {/* two tones that tell apart from each other and from the card, in light and in dark
            (lint-charts checks their contrast): the body of the requests, and their tail */}
        <Area y0={gap(tells, (b) => b.p95Ms)} y1={gap(tells, (b) => b.p99Ms)} curve="linear" class={LATENCY_FILL.tail} />
        <Area y0={gap(tells, (b) => b.p50Ms)} y1={gap(tells, (b) => b.p95Ms)} curve="linear" class={LATENCY_FILL.body} />
        <Line y={gap(has, (b) => b.p50Ms)} curve="linear" class={`stroke-foreground ${STROKE.line}`} hoverDotClass="size-1.5 rounded-full bg-foreground" />
        <Rule value={(_, i) => (hovered() === null && i === nearest(rows(), props.focus?.() ?? null) ? yRange()[1] : (undefined as unknown as number))} class={FOCUS_RULE} />
        <PlotAxis placement="left" tickValues={ticks()} format={(v) => duration(10 ** v)} labelClass={LABEL.scale} />
        <Budget value={props.budgetMs ? lg(props.budgetMs) : null} say={(v) => duration(10 ** v)} />
        <If when={props.axis}>
          <TimeAxis domain={domain()} />
        </If>
      </Plot>
    </If>
  );
}

/** How many latency buckets a decade is cut in (each about 2.15× the last). */
const PER_DECADE = 3;

/** Latency over the session as a grid: a column a slice of time, a row a
 * bucket of latency on a log scale, a cell's tone how many requests fell in
 * it. What bands average away shows here: two modes, a slow few, a shift.
 * Counted by Rust over every request (`latencyHeat`), never from a sample. */
export function LatencyHeat(props: { profile: Profile; range?: [number, number] | null; focus?: Focus }) {
  const p = () => props.profile;
  const from = () => props.range?.[0] ?? 0;
  const to = () => props.range?.[1] ?? p().rate.length - 1;
  const bins = () => p().rate.map((_, i) => `${i}`).slice(from(), to() + 1);
  const cells = computed(() => p().latencyHeat.filter((c) => c.bin >= from() && c.bin <= to()));
  // every bucket from the slowest seen down to the fastest, slowest on top: a gap in latency is a gap in rows
  const buckets = () => {
    const all = cells().map((c) => c.bucket);
    if (!all.length) return [];
    const lo = Math.min(...all);
    const hi = Math.max(...all, lo + 1);
    return Array.from({ length: hi - lo + 1 }, (_, i) => hi - i);
  };
  const edge = (bucket: number) => 10 ** (bucket / PER_DECADE);
  const rows = () => cells().map((c) => ({ x: `${c.bin}`, y: `${c.bucket}`, n: c.n, bin: c.bin, bucket: c.bucket }));
  const max = () => cells().reduce((m, c) => Math.max(m, c.n), 1);
  const hovered = signal<number | null>(null);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: `${sinceTick(slotMs(p(), r.bin))} – ${sinceTick(slotMs(p(), r.bin + 1))}`, lines: [`${duration(edge(r.bucket - 1))} – ${duration(edge(r.bucket))}`, t("sessions.requestsN", { n: r.n })] } : null;
  };
  const label = () => t("latencyHeat.text", { p50: duration(p().p50Ms ?? 0), p99: duration(p().p99Ms ?? 0) });
  // a decade's edge is named on the scale; the buckets between are not
  const decade = (i: number) => {
    const b = buckets()[i];
    return b !== undefined && b % PER_DECADE === 0 ? `≤ ${duration(edge(b))}` : "";
  };
  return (
    <If when={!terminal() && bins().length > 1} else={<Text class="hidden text-muted-foreground terminal:flex">{label()}</Text>}>
      <If when={rows().length > 0} else={<NoneInStretch />}>
        <Plot
          testID="latency-heat"
          data={rows()}
          xScale={{ type: "band", domain: bins(), paddingInner: CELLS.heat }}
          yScale={{ type: "band", domain: buckets().map((b) => `${b}`), paddingInner: CELLS.heat }}
          colorScale={{ type: "sequential", domain: [0, Math.sqrt(max())], range: HEAT_RAMP() }}
          accessibilityLabel={label()}
          accessibilityValue={tipValue(tip())}
          appearGeneration={1}
          motion={MOTION}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => {
            hovered.set(index);
            props.focus?.set(index === null ? null : slotMs(p(), rows()[index].bin + 0.5));
          }}
          // its scale outside, in the room bands have too (the two share a card): never on a cell
          class={`${TICK_ROOM} ${GUTTER_SCALE} ${PLOT}`}
        >
          <Cell x="x" y="y" value={(_, i) => Math.sqrt(rows()[i]?.n ?? 0)} cornerRadii={TIP.cell} inactiveOpacity={INACTIVE} hoverCellClass={HOVER} />
          <PlotAxis placement="left" format={decade} labelClass={LABEL.scale} />
          <TimeAxis domain={[slotMs(p(), from()), slotMs(p(), to() + 1)]} slices={bins().length} />
        </Plot>
      </If>
    </If>
  );
}

// --- requests that waited on each other ------------------------------------------

/** Chains of requests that each waited for the one before: how long each took
 * in turn, beside how long the same requests would take eight at once. */
export function Chains(props: { profile: Profile; max?: number; /** the stretch chosen on the timeline (ms from the start): the chains in it, over it */ domainMs?: [number, number] | null }) {
  const span = (): [number, number] => props.domainMs ?? sessionSpan(props.profile, props.profile.rate.length);
  const inSpan = (c: { startMs: number; endMs: number }) => !props.domainMs || (c.endMs > props.domainMs[0] && c.startMs < props.domainMs[1]);
  const rows = () => (props.profile.chains ?? []).filter(inSpan).slice(0, props.max ?? 5).map((c, i) => ({ ...c, name: t("chains.row", { i: i + 1, n: c.requests }), soon: c.startMs + c.atOnceMs }));
  const hovered = signal<number | null>(null);
  // "59 ms · ≈ 10 ms at once": past the bars where there is more room after them than before
  const figures = (c: { startMs: number; endMs: number; atOnceMs: number }) => `${duration(c.endMs - c.startMs)} · ${t("chains.bar", { fast: duration(c.atOnceMs) })}`;
  const after = (c: { startMs: number; endMs: number }) => span()[1] - c.endMs >= c.startMs - span()[0];
  const label = () => {
    const c = rows()[0];
    return c ? t("chains.text", { n: c.requests, time: duration(c.endMs - c.startMs), fast: duration(c.atOnceMs) }) : "";
  };
  const tip = () => {
    const c = hovered() === null ? null : rows()[hovered()!];
    return c ? { title: c.name, lines: [t("chains.line", { from: duration(c.startMs), to: duration(c.endMs) }), t("chains.fast", { fast: duration(c.atOnceMs), saved: duration(c.endMs - c.startMs - c.atOnceMs) })] } : null;
  };
  return (
    <If
      when={!textual()}
      else={<SimpleTable columns={[t("chains.chain"), t("chains.from"), t("profile.time"), t("chains.atOnce")]} rows={rows().map((c) => [c.name, `+${duration(c.startMs)}`, duration(c.endMs - c.startMs), duration(c.atOnceMs)])} />}
    >
      <If when={rows().length > 0} else={<NoneInStretch />}>
      <View class="gap-2">
        <Plot
          testID="chains"
          data={rows()}
          yScale={{ type: "band", ...BARS.pairs }}
          xScale={{ type: "linear", domain: span() }}
          accessibilityLabel={label()}
          accessibilityValue={tipValue(tip())}
          appearGeneration={1}
          motion={MOTION}
          crossLineClass={CROSS_BAND}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => hovered.set(index)}
          class={`${TICK_ROOM} ${GUTTER}`}
          style={{ height: rowsHeight(rows().length) }}
        >
          {/* as it ran: one after another */}
          <Bar base="startMs" value="endMs" class="fill-muted-foreground/45" cornerRadii={TIP.span} inactiveOpacity={INACTIVE} />
          {/* the same requests eight at once */}
          <Bar base="startMs" value="soon" class="fill-chart-1" cornerRadii={TIP.span} inactiveOpacity={INACTIVE} />
          {/* the row's two figures on one line, the row's own middle: past its bars, or before them where they reach the far edge */}
          <PlotLabel class={LABEL.figure} texts={rows().map((c, i) => (after(c) ? { text: figures(c), x: c.endMs, y: i, align: "left" as const, class: ROW_FIGURE } : { text: figures(c), x: c.startMs, y: i, align: "right" as const, class: ROW_FIGURE_BEFORE }))} />
          <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.name ?? "")} labelClass={AXIS_LABEL} />
          <TimeAxis domain={span()} />
        </Plot>
        <Legend gutter items={[{ label: t("chains.asRan"), swatch: "bg-muted-foreground/45" }, { label: t("chains.atOnce"), swatch: "bg-chart-1" }]} />
      </View>
      </If>
    </If>
  );
}

// --- where the time went --------------------------------------------------------

/** Where a session's time went: a bar per kind of request, its share of the time the program waited. */
export function TimeSpent(props: { profile: Profile; max?: number }) {
  // (by line of the program, the waiting is Group by's table: "Which lines of the program were slow or failed?")
  const rows = () => props.profile.groups.slice(0, props.max ?? 8).map((g) => ({ ...g, name: groupName(g) }));
  const hovered = signal<number | null>(null);
  const label = () => {
    const top = rows()[0];
    return top ? t("profile.spentText", { what: top.name, pct: pct(top.share), time: duration(top.waitedMs) }) : "";
  };
  const tip = () => {
    const g = hovered() === null ? null : rows()[hovered()!];
    if (!g) return null;
    const spent = t("profile.spentLine", { time: duration(g.waitedMs), pct: pct(g.share), n: g.requests });
    return { title: g.name, lines: [spent, t("profile.latencyLine", { p50: duration(g.p50Ms), p95: duration(g.p95Ms), max: duration(g.maxMs) }), `${transfer(g.bytesUp, g.bytesDown)} · ${cost(g.cost, props.profile.cost.currency)}`] };
  };
  return (
    <If
      when={!textual()}
      else={<SimpleTable columns={[t("profile.kind"), t("profile.share"), t("profile.time"), t("figure.requests")]} rows={rows().map((g) => [g.name, pct(g.share), duration(g.waitedMs), count(g.requests)])} />}
    >
      <Plot
        testID="time-spent"
        data={rows()}
        yScale={{ type: "band", ...BARS.rows }}
        xScale={{ type: "linear", domain: [0, Math.max(...rows().map((g) => g.waitedMs), 1) * 1.45] }}
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
        <Bar
          value="waitedMs"
          class="fill-chart-1"
          cornerRadii={TIP.across}
          inactiveOpacity={INACTIVE}
          stagger={0.3}
          label={(_, i) => `${pct(rows()[i].share)} · ${duration(rows()[i].waitedMs)}`}
          labelClass={LABEL.bar}
        />
        {/* a kind keeps both its ends */}
        <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.name ?? "")} labelClass={AXIS_LABEL} />
      </Plot>
    </If>
  );
}

// --- how long, every request -----------------------------------------------------

/** How long requests took, every one of them: a bar per bucket (each about
 * 2.5× the one before), the share at or under each as a line; the median,
 * p95 and p99 named above it. */
export function Distribution(props: { profile: Profile }) {
  const buckets = () => props.profile.latency ?? [];
  const total = () => Math.max(1, buckets().reduce((s, b) => s + b.n, 0));
  const rows = () => {
    let run = 0;
    return buckets().map((b, i) => {
      run += b.n;
      const prev = i > 0 ? buckets()[i - 1].leMs : null;
      const name = b.leMs === null ? `> ${duration(prev ?? 0)}` : `≤ ${duration(b.leMs)}`;
      return { name, n: b.n, cum: (run * 100) / total(), p95: b.leMs === null || b.leMs >= (props.profile.p95Ms ?? 0) };
    });
  };
  const label = () => t("dist.text", { p50: duration(props.profile.p50Ms ?? 0), p95: duration(props.profile.p95Ms ?? 0), p99: duration(props.profile.p99Ms ?? 0) });
  const hovered = signal<number | null>(null);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: r.name, lines: [t("dist.line", { n: r.n, cum: Math.round(r.cum) })] } : null;
  };
  // the tail in its own color: the buckets at or past the p95
  const first95 = () => rows().findIndex((r) => r.p95);
  // what those buckets hold of the requests: said as it is, not as "5%" (a bucket is not cut at the p95)
  const tailShare = () => (first95() < 0 ? 0 : rows().slice(first95()).reduce((n, r) => n + r.n, 0) / total());
  const peak = () => Math.max(1, ...rows().map((r) => r.n));
  // between the first bucket that holds a request and the last
  const inside = (i: number) => rows().findIndex((r) => r.n > 0) < i && i < rows().length - 1 - [...rows()].reverse().findIndex((r) => r.n > 0);
  return (
    // the frame says the figures (its sentence); a terminal's table says them again as rows
    <If when={buckets().length > 1} else={<Text class={TEXT.note}>{label()}</Text>}>
      <View class="gap-2">
        <If when={!textual()} else={<SimpleTable columns={[t("dist.took"), t("figure.requests"), "%"]} rows={rows().map((r) => [r.name, count(r.n), percent(r.cum / 100)])} />}>
          <Plot
            testID="distribution"
            data={rows()}
            xScale={{ type: "band", ...BARS.buckets, maxBandWidth: 64 }}
            yScale={{ type: "linear", domain: [0, peak() * HEADROOM.plot] }}
            // the share done by each time, on a scale of its own: a line apart from the bars
            y2Scale={{ type: "linear", domain: [0, 100 * HEADROOM.plot] }}
            alignTicks={false}
            colorScale={{ type: "ordinal", domain: ["body", "tail"], range: [token("chart-1"), token("chart-tail" as never) as unknown as string] }}
            accessibilityLabel={label()}
            accessibilityValue={tipValue(tip())}
            appearGeneration={1}
            motion={MOTION}
            crossLineClass={CROSS_BAND}
            tooltipPlacement="pointer"
            tooltipOffset={10}
            tooltipContent={<ChartTip tip={tip()} />}
            onHoverChange={({ index }) => hovered.set(index)}
            // the share done's scale outside, at the far edge: its words never on the line or a bar
            class={`${TICK_ROOM} ${GUTTER_SCALE_END} ${PLOT}`}
          >
            {/* each bar says how many it holds; the share done by then is the line, never a bar's label */}
            {/* a bucket of none between two that hold some says "0" on a sliver of a bar (a bar of no value is not drawn, nor its label) */}
            <Bar value={(r, i) => (r.n > 0 ? r.n : inside(i) ? peak() * ZERO_SLIVER : 0)} color={(_, i) => (first95() >= 0 && i >= first95() ? "tail" : "body")} cornerRadii={TIP.up} inactiveOpacity={INACTIVE} stagger={0.2} label={(_, i) => ((rows()[i]?.n ?? 0) > 0 || inside(i) ? count(rows()[i]?.n ?? 0) : "")} labelClass={LABEL.top} />
            <Line y="cum" scale="y2" curve="linear" class={`stroke-muted-foreground ${STROKE.hair}`} style={{ strokeDasharray: DASH.reference }} hoverDotClass="size-1.5 rounded-full bg-muted-foreground" />
            <PlotAxis format={(i) => rows()[i]?.name ?? ""} labelClass={LABEL.bucket} />
            <ValueAxis marked peak={peak()} say={(v) => t("run.bandRequests", { n: v })} />
            <PlotAxis placement="right" scale="y2" tickValues={[50, 100]} format={(v) => percent(v / 100)} labelClass={LABEL.scaleEnd} />
          </Plot>
          <Legend items={[{ label: t("dist.requests"), swatch: "bg-chart-1" }, ...(tailShare() > 0 ? [{ label: t("dist.tail", { pct: percent(tailShare()) }), swatch: "bg-chart-tail" }] : []), { label: t("dist.cumulative"), swatch: "border-muted-foreground", line: "dashed" }]} />
        </If>
      </View>
    </If>
  );
}

/** Each request as a dot: when it started (or how large it was) across, how
 * long it took up, on a log scale, in its outcome's color; the p95 dashed.
 * A press opens the request. The shape a summary hides: a slow tail, a
 * cluster of 404s, large objects that take long. */
export function RequestScatter(props: { profile: Profile; by: "time" | "size"; domainMs?: [number, number] | null; onRequest?: (id: number) => void; /** the requests a fault scenario answered, by id */ injected?: number[] }) {
  const marked = computed(() => new Set(props.injected ?? []));
  const all = computed(() => {
    const d = props.domainMs;
    return props.profile.samples
      .filter((x) => !d || (x.tMs >= d[0] && x.tMs < d[1]))
      .map((x) => ({ ...x, y: log10(Math.max(x.ms, 0.1)), x: props.by === "time" ? x.tMs : log10(Math.max(x.bytes ?? 0, 1)), tone: toneOf(x, marked()) }));
  });
  // the scale is the dots' own range (a decade at least, the dots in its middle), a little over it each way
  const range = computed(() => {
    const ys = all().map((r) => r.y);
    const lo = Math.min(...ys);
    const hi = Math.max(...ys);
    const spare = Math.max(0, 1 - (hi - lo)) / 2;
    return [lo - spare - 0.12, hi + spare + 0.15] as const;
  });
  const yLo = () => range()[0];
  const yHi = () => range()[1];
  // every dot inside the plot: clamped to the scale, whatever a stray value is
  const rows = computed(() => {
    const [a, b] = range();
    return all().map((r) => ({ ...r, y: Math.min(Math.max(r.y, a), b) }));
  });
  // a little room either side: a dot at the first or last instant is drawn whole
  const xDomain = (): [number, number] => {
    if (props.by !== "time") return [-0.1, Math.max(...rows().map((r) => r.x), 3) + 0.2];
    const [a, b] = span();
    const pad = (b - a) * 0.015;
    return [a - pad, b + pad];
  };
  const span = (): [number, number] => props.domainMs ?? sessionSpan(props.profile, props.profile.rate.length);
  const p95 = () => log10(Math.max(props.profile.p95Ms ?? 0, 0.1));
  // a decade's line and its words (1 ms, 10 ms…); a scale under a decade wide names its two ends
  const ticks = () => decades(Math.ceil(yLo() - 1e-9), yHi()).filter((d) => d >= yLo() && d <= yHi());
  const hovered = signal<number | null>(null);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    if (!r) return null;
    const g = props.profile.groups[r.group];
    return { title: g ? groupName(g) : "", lines: [`${duration(r.ms)} · ${r.status ?? "—"} · ${bytes(r.bytes ?? 0)}`, `+${duration(r.tMs)}`] };
  };
  const label = () => scatterSaid(props.profile, props.domainMs);
  return (
    // a stretch with one request draws its one dot; with none, says so in the chart's room
    <If when={!terminal() && rows().length > 0} else={terminal() ? <Text class={TEXT.note}>{label()}</Text> : <NoneInStretch />}>
      <BaseButton
        accessibilityLabel={label()}
        disabled={!props.onRequest}
        onPress={() => {
          const r = hovered() === null ? null : rows()[hovered()!];
          if (r) props.onRequest?.(r.id);
        }}
        // the plot as wide as the card, less its scale's room
        class="items-stretch"
      >
        <Plot
          testID="request-scatter"
          data={rows()}
          x="x"
          xScale={{ type: "linear", domain: xDomain() }}
          yScale={{ type: "linear", domain: [yLo(), yHi()] }}
          colorScale={tones()}
          accessibilityLabel={label()}
          accessibilityValue={tipValue(tip())}
          motion={MOTION}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => hovered.set(index)}
          // its scale outside, in the room for one: a decade's words never on a dot
          class={`${TICK_ROOM} ${GUTTER_SCALE} ${PLOT}`}
        >
          <Grid y={ticks()} class={GRID} />
          <Grid y={[p95()]} class={`stroke-muted-foreground/60 ${STROKE.hair}`} style={{ strokeDasharray: DASH.reference }} />
          <Dot value="y" r={2.5} hitRadius={6} color="tone" inactiveOpacity={INACTIVE} hoverDotClass="size-2 rounded-full border border-card bg-foreground" />
          <PlotAxis placement="left" tickValues={ticks()} format={(v) => duration(10 ** v)} labelClass={LABEL.scale} />
          <If when={props.by === "time"} else={<PlotAxis tickValues={sizeTicks(xDomain())} format={(v) => bytes(Math.round(10 ** v))} labelClass={LABEL.bucket} />}>
            <TimeAxis domain={span()} />
          </If>
        </Plot>
        {/* every colour a dot may have, named: the outcomes this stretch holds, and a scenario's answers */}
        <Legend items={[...OUTCOMES.filter((o) => rows().some((r) => r.tone === o.tone)).map((o) => ({ label: t(o.key), swatch: o.swatch })), ...injectedLegend(props.injected), { label: `${t("scatter.p95Line")} ${duration(props.profile.p95Ms ?? 0)}`, swatch: "border-muted-foreground", line: "dashed" }]} />
      </BaseButton>
    </If>
  );
}

/** The ticks of a scale of sizes (as logs of bytes): at powers of 1024 (1 KiB, 1 MiB…), each a round figure in its
 * unit; a scale under two of them wide is ticked at powers of 32 (32 KiB, 1 MiB, 32 MiB). */
export function sizeTicks([a, b]: [number, number]): number[] {
  const at = (step: number) => {
    const out: number[] = [];
    for (let k = Math.ceil(Math.max(a, 0) / step - 1e-9); k * step <= b + 1e-9; k++) out.push(k * step);
    return out;
  };
  const kib = at(Math.log10(1024));
  return kib.length >= 2 ? kib : at(Math.log10(32));
}

/** The wait for an answer's first byte against the rest of the request, by
 * kind: a row each, the two stacked (medians). */
export function FirstByte(props: { signals: Signals; max?: number }) {
  const rows = () => (props.signals.firstByte?.kinds ?? []).slice(0, props.max ?? 8);
  const hovered = signal<number | null>(null);
  const label = () => t("firstByte.text", { pct: percent(props.signals.firstByte?.share ?? 0), n: count(props.signals.firstByte?.count ?? 0) });
  const line = (r: ReturnType<typeof rows>[number]) => t("firstByte.line", { first: duration(r.firstByteMs), rest: duration(r.restMs) });
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: r.kind, lines: [line(r), t("run.bandRequests", { n: r.count })] } : null;
  };
  return (
    <If when={!textual()} else={<SimpleTable columns={[t("profile.kind"), t("firstByte.first"), t("firstByte.rest"), t("figure.requests")]} rows={rows().map((r) => [r.kind, duration(r.firstByteMs), duration(r.restMs), count(r.count)])} />}>
      <Plot
        testID="first-byte"
        data={rows()}
        yScale={{ type: "band", ...BARS.rows }}
        xScale={{ type: "linear", domain: [0, Math.max(...rows().map((r) => r.firstByteMs + r.restMs), 1e-3) * 1.3] }}
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
        <Bar value="firstByteMs" stack="t" class="fill-chart-1" inactiveOpacity={INACTIVE} />
        <Bar value="restMs" stack="t" class="fill-chart-4" cornerRadii={TIP.across} inactiveOpacity={INACTIVE} label={(_, i) => duration(rows()[i].firstByteMs + rows()[i].restMs)} labelClass={LABEL.bar} />
        <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.kind ?? "")} labelClass={AXIS_LABEL} />
      </Plot>
    </If>
  );
}
