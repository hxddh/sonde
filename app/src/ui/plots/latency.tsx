// Latency: how long requests took — over the session, every one of them,
// by kind, as a distribution, the first byte against the rest, chains, and
// where the time went.

import { computed, signal, Text, token, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { Area, Bar, Dot, Grid, Line, Plot, PlotAxis, PlotLabel, Rule, Cell } from "nsh/base/plot";
import { bytes, cost, count, duration, percent, transfer } from "../../format";
import { t } from "../../i18n";
import type { Profile, Signals } from "../../types";
import { AXIS_LABEL, BUCKET_TICK, BAR_LABEL, CROSS_BAND, CROSS_LINE, ChartTip, type Focus, GUTTER, GUTTER_CHARS, INACTIVE, Legend, MOTION, NoneInStretch, PITCH, PLOT, ROW_LABEL, TICK_ROOM, TIME_TICK, TOP_LABEL, VALUE_TICK, axisName, decades, groupName, injectedLegend, log10, nearest, pct, rowsHeight, tipValue, toneOf, tones, tail, LegendItem, OUTCOMES, TimeAxis, ValueAxis, sessionSpan, BARS, GRID, STRIP, TIP, VALUE_TICK_END, CELLS, HOVER, sinceTick, Budget, budgetTop } from "./system";
import { terminal, textual, SimpleTable } from "../kit";

/** The two tones of latency's bands: the body of the requests (median to
 * p95) and their tail (p95 to p99). */
export const LATENCY_FILL = { body: "fill-latency-body", tail: "fill-latency-tail" } as const;
/** What the bands stand for, for a frame's legend. */
/** Whether any slice has a tail to draw: with few requests in each, a slice's p95 is its p99 and the band between them is nothing. */
export const hasTail = (profile: Profile): boolean => profile.rate.some((b) => b.p99Ms > b.p95Ms + 1e-9);
/** The bands' legend: the tail is named where one is drawn; where no slice has one, that is said in its place. */
export const latencyLegend = (profile?: Profile): LegendItem[] => [
  { label: t("latencyTime.p50"), swatch: "bg-foreground" },
  { label: t("latencyTime.p95"), swatch: "bg-latency-body" },
  ...(!profile || hasTail(profile) ? [{ label: t("latencyTime.p99"), swatch: "bg-latency-tail" }] : []),
];

/** How long requests took over the session: the band from median to p95,
 * the p99 a line above it; slices without requests leave a gap. */
export function LatencyBand(props: { profile: Profile; class?: string; testID?: string; domainMs?: [number, number] | null; focus?: Focus; /** on the board: its time axis under it */ axis?: boolean; /** the project's `max-p95`, in ms: its line */ budgetMs?: number | null }) {
  // every slice, so one without requests leaves a gap rather than a line across it
  const rows = () => props.profile.rate;
  const has = (b: Profile["rate"][number]) => b.ok + b.miss + b.client + b.server > 0;
  const answered = () => rows().filter(has);
  const gap = (f: (b: Profile["rate"][number]) => number) => (b: Profile["rate"][number]) => (has(b) ? f(b) : (undefined as unknown as number));
  // the scale of what is shown: the zoomed stretch's slices only
  const inView = () => {
    const d = props.domainMs;
    return d ? answered().filter((b) => b.tMs >= d[0] && b.tMs < d[1]) : answered();
  };
  const top = () => budgetTop(Math.max(...inView().map((b) => b.p99Ms), 1) * 1.15, props.budgetMs);
  const hovered = signal<number | null>(null);
  const domain = (): [number, number] => props.domainMs ?? sessionSpan(props.profile, props.profile.rate.length);
  const label = () => {
    const last = answered()[answered().length - 1];
    return last ? t("band.text", { p50: duration(last.p50Ms), p95: duration(last.p95Ms), p99: duration(last.p99Ms) }) : "";
  };
  const tip = () => {
    const b = hovered() === null ? null : rows()[hovered()!];
    return b && has(b) ? { title: `+${duration(b.tMs)}`, lines: [t("band.line", { p50: duration(b.p50Ms), p95: duration(b.p95Ms), p99: duration(b.p99Ms) })] } : null;
  };
  return (
    <If when={!terminal() && answered().length > 1} else={<Text class="hidden text-muted-foreground terminal:flex">{label()}</Text>}>
      <Plot
        testID={props.testID ?? "latency-band"}
        data={rows()}
        x="tMs"
        xScale={{ type: "linear", domain: domain() }}
        yScale={{ type: "linear", domain: [0, top()] }}
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
        class={props.class ?? `${STRIP} w-full`}
      >
        {/* straight between slices: a curve would show values no request had */}
        {/* two tones that tell apart from each other and from the card, in light and in dark
            (lint-charts checks their contrast): the body of the requests, and their tail */}
        <Area y0={gap((b) => b.p95Ms)} y1={gap((b) => b.p99Ms)} curve="linear" class={LATENCY_FILL.tail} />
        <Area y0={gap((b) => b.p50Ms)} y1={gap((b) => b.p95Ms)} curve="linear" class={LATENCY_FILL.body} />
        <Line y={gap((b) => b.p50Ms)} curve="linear" class="stroke-foreground stroke-[1.5]" hoverDotClass="size-1.5 rounded-full bg-foreground" />
        <Rule value={(_, i) => (hovered() === null && i === nearest(rows(), props.focus?.() ?? null) ? top() : (undefined as unknown as number))} class="stroke-foreground/40 stroke-1" />
        <ValueAxis peak={Math.max(...inView().map((b) => b.p99Ms), 1)} say={duration} />
        <Budget value={props.budgetMs} say={duration} />
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
 * Counted from the requests the session keeps (`samples`). */
export function LatencyHeat(props: { profile: Profile; range?: [number, number] | null; focus?: Focus }) {
  const p = () => props.profile;
  const from = () => props.range?.[0] ?? 0;
  const to = () => props.range?.[1] ?? p().rate.length - 1;
  const bins = () => p().rate.map((_, i) => `${i}`).slice(from(), to() + 1);
  const bucketOf = (ms: number) => Math.ceil(log10(Math.max(ms, 0.1)) * PER_DECADE - 1e-9);
  const cells = computed(() => {
    const m = new Map<string, { bin: number; bucket: number; n: number }>();
    for (const x of p().samples) {
      const bin = Math.min(p().rate.length - 1, Math.floor(x.tMs / Math.max(p().binMs, 1e-9)));
      if (bin < from() || bin > to()) continue;
      const bucket = bucketOf(x.ms);
      const key = `${bin}:${bucket}`;
      const c = m.get(key);
      if (c) c.n += 1;
      else m.set(key, { bin, bucket, n: 1 });
    }
    return [...m.values()];
  });
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
    return r ? { title: `${sinceTick(r.bin * p().binMs)} – ${sinceTick((r.bin + 1) * p().binMs)}`, lines: [`${duration(edge(r.bucket - 1))} – ${duration(edge(r.bucket))}`, t("sessions.requestsN", { n: r.n })] } : null;
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
          colorScale={{ type: "sequential", domain: [0, Math.sqrt(max())], range: [token("bar-track" as never) as unknown as string, token("chart-1" as never) as unknown as string] }}
          accessibilityLabel={label()}
          accessibilityValue={tipValue(tip())}
          appearGeneration={1}
          motion={MOTION}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => {
            hovered.set(index);
            props.focus?.set(index === null ? null : (rows()[index].bin + 0.5) * p().binMs);
          }}
          class={`${TICK_ROOM} ${PLOT} w-full`}
        >
          <Cell x="x" y="y" value={(_, i) => Math.sqrt(rows()[i]?.n ?? 0)} cornerRadii={TIP.cell} inactiveOpacity={INACTIVE} hoverCellClass={HOVER} />
          <PlotAxis placement="left" labelPlacement="inside" format={decade} labelClass={VALUE_TICK} />
          <TimeAxis domain={[from() * p().binMs, (to() + 1) * p().binMs]} slices={bins().length} />
        </Plot>
      </If>
    </If>
  );
}

// --- requests that waited on each other ------------------------------------------

/** Chains of requests that each waited for the one before: how long each took
 * in turn, beside how long the same requests would take eight at once. */
export function Chains(props: { profile: Profile; max?: number }) {
  const rows = () => (props.profile.chains ?? []).slice(0, props.max ?? 5).map((c, i) => ({ ...c, name: t("chains.row", { i: i + 1, n: c.requests }), soon: c.startMs + c.atOnceMs }));
  const hovered = signal<number | null>(null);
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
      <View class="gap-2">
        <Plot
          testID="chains"
          data={rows()}
          yScale={{ type: "band", ...BARS.pairs }}
          xScale={{ type: "linear", domain: sessionSpan(props.profile, props.profile.rate.length) }}
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
          <Bar base="startMs" value="endMs" class="fill-muted-foreground/45" cornerRadii={TIP.span} inactiveOpacity={INACTIVE} label={(_, i) => duration(rows()[i].endMs - rows()[i].startMs)} labelClass={BAR_LABEL} />
          {/* the same requests eight at once */}
          <Bar base="startMs" value="soon" class="fill-chart-1" cornerRadii={TIP.span} inactiveOpacity={INACTIVE} label={(_, i) => t("chains.bar", { fast: duration(rows()[i].atOnceMs) })} labelClass={BAR_LABEL} />
          <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.name ?? "")} labelClass={AXIS_LABEL} />
          <TimeAxis domain={sessionSpan(props.profile, props.profile.rate.length)} />
        </Plot>
        <Legend gutter items={[{ label: t("chains.asRan"), swatch: "bg-muted-foreground/45" }, { label: t("chains.atOnce"), swatch: "bg-chart-1" }]} />
      </View>
    </If>
  );
}

// --- where the time went --------------------------------------------------------

/** Where a session's time went: a bar per kind of request, its share of the time the program waited. */
export function TimeSpent(props: { profile: Profile; max?: number; by?: "kind" | "code" }) {
  // by kind of request, or by the place in the program's code that sent them
  const rows = () =>
    props.by === "code"
      ? (props.profile.callers ?? []).slice(0, props.max ?? 8).map((c) => ({
          name: c.place,
          waitedMs: c.waitedMs,
          share: c.waitedMs / Math.max(props.profile.busyMs, 1e-9),
          requests: c.requests,
          p50Ms: 0,
          p95Ms: 0,
          maxMs: 0,
          bytesUp: 0,
          bytesDown: 0,
          cost: c.cost,
          code: true,
        }))
      : props.profile.groups.slice(0, props.max ?? 8).map((g) => ({ ...g, name: groupName(g), code: false }));
  const hovered = signal<number | null>(null);
  const label = () => {
    const top = rows()[0];
    return top ? t("profile.spentText", { what: top.name, pct: pct(top.share), time: duration(top.waitedMs) }) : "";
  };
  const tip = () => {
    const g = hovered() === null ? null : rows()[hovered()!];
    if (!g) return null;
    const spent = t("profile.spentLine", { time: duration(g.waitedMs), pct: pct(g.share), n: g.requests });
    return g.code ? { title: g.name, lines: [spent, cost(g.cost, props.profile.cost.currency)] } : { title: g.name, lines: [spent, t("profile.latencyLine", { p50: duration(g.p50Ms), p95: duration(g.p95Ms), max: duration(g.maxMs) }), `${transfer(g.bytesUp, g.bytesDown)} · ${cost(g.cost, props.profile.cost.currency)}`] };
  };
  return (
    <If
      when={!textual()}
      else={<SimpleTable columns={[t("profile.kind"), t("profile.share"), t("profile.time"), t("figure.requests")]} rows={rows().map((g) => [g.name, pct(g.share), duration(g.waitedMs), count(g.requests)])} />}
    >
      <Plot
        testID={props.by === "code" ? "time-spent-code" : "time-spent"}
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
          labelClass={BAR_LABEL}
        />
        {/* a place in the code keeps its end (the file and line), a kind both its ends */}
        <PlotAxis placement="left" format={(i) => (rows()[i]?.code ? tail(rows()[i].name, GUTTER_CHARS) : axisName(rows()[i]?.name ?? ""))} labelClass={AXIS_LABEL} />
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
  return (
    <If when={buckets().length > 1} else={<Text class="text-small text-muted-foreground">{label()}</Text>}>
      <View class="gap-2">
        <Text testID="dist-figures" class="tabular-nums text-small text-foreground">{label()}</Text>
        <If when={!textual()} else={<SimpleTable columns={[t("dist.took"), t("figure.requests"), "%"]} rows={rows().map((r) => [r.name, count(r.n), percent(r.cum / 100)])} />}>
          <Plot
            testID="distribution"
            data={rows()}
            xScale={{ type: "band", ...BARS.buckets, maxBandWidth: 64 }}
            yScale={{ type: "linear", domain: [0, peak() * 1.18] }}
            // the share done by each time, on a scale of its own: a line apart from the bars
            y2Scale={{ type: "linear", domain: [0, 118] }}
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
            class={`${TICK_ROOM} ${PLOT} w-full`}
          >
            {/* each bar says how many it holds; the share done by then is the line, never a bar's label */}
            <Bar value="n" color={(_, i) => (first95() >= 0 && i >= first95() ? "tail" : "body")} cornerRadii={TIP.up} inactiveOpacity={INACTIVE} stagger={0.2} label={(_, i) => ((rows()[i]?.n ?? 0) > 0 ? count(rows()[i].n) : "")} labelClass={TOP_LABEL} />
            <Line y="cum" scale="y2" curve="linear" class="stroke-muted-foreground stroke-1" style={{ strokeDasharray: "3 3" }} hoverDotClass="size-1.5 rounded-full bg-muted-foreground" />
            <PlotAxis format={(i) => rows()[i]?.name ?? ""} labelClass={BUCKET_TICK} />
            <ValueAxis marked peak={peak()} say={(v) => t("run.bandRequests", { n: v })} />
            <PlotAxis placement="right" scale="y2" labelPlacement="inside" tickValues={[50, 100]} format={(v) => percent(v / 100)} labelClass={VALUE_TICK_END} />
          </Plot>
          <Legend items={[{ label: t("dist.requests"), swatch: "bg-chart-1" }, ...(tailShare() > 0 ? [{ label: t("dist.tail", { pct: percent(tailShare()) }), swatch: "bg-chart-tail" }] : []), { label: t("dist.cumulative"), swatch: "border-muted-foreground", line: true }]} />
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
  const rows = () => {
    const d = props.domainMs;
    return props.profile.samples
      .filter((x) => !d || (x.tMs >= d[0] && x.tMs < d[1]))
      .map((x) => ({ ...x, y: log10(Math.max(x.ms, 0.1)), x: props.by === "time" ? x.tMs : log10(Math.max(x.bytes ?? 0, 1)), tone: toneOf(x, marked()) }));
  };
  const yLo = () => Math.min(...rows().map((r) => r.y), log10(1)) - 0.12;
  const yHi = () => Math.max(...rows().map((r) => r.y), log10(10)) + 0.15;
  // a little room either side: a dot at the first or last instant is drawn whole
  const xDomain = (): [number, number] => {
    if (props.by !== "time") return [-0.1, Math.max(...rows().map((r) => r.x), 3) + 0.2];
    const [a, b] = span();
    const pad = (b - a) * 0.015;
    return [a - pad, b + pad];
  };
  const span = (): [number, number] => props.domainMs ?? sessionSpan(props.profile, props.profile.rate.length);
  const p95 = () => log10(Math.max(props.profile.p95Ms ?? 0, 0.1));
  const hovered = signal<number | null>(null);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    if (!r) return null;
    const g = props.profile.groups[r.group];
    return { title: g ? groupName(g) : "", lines: [`${duration(r.ms)} · ${r.status ?? "—"} · ${bytes(r.bytes ?? 0)}`, `+${duration(r.tMs)}`] };
  };
  const label = () => t("scatter.text", { n: rows().length, p95: duration(props.profile.p95Ms ?? 0) });
  return (
    // a stretch with one request draws its one dot; with none, says so in the chart's room
    <If when={!terminal() && rows().length > 0} else={terminal() ? <Text class="text-small text-muted-foreground">{label()}</Text> : <NoneInStretch />}>
      <BaseButton
        accessibilityLabel={label()}
        disabled={!props.onRequest}
        onPress={() => {
          const r = hovered() === null ? null : rows()[hovered()!];
          if (r) props.onRequest?.(r.id);
        }}
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
          class={`${TICK_ROOM} ${PLOT} w-full`}
        >
          <Grid y={decades(Math.ceil(yLo()), yHi())} class={GRID} />
          <Grid y={[p95()]} class="stroke-muted-foreground/60" style={{ strokeDasharray: "3 3" }} />
          <Dot value="y" r={2.5} hitRadius={6} color="tone" inactiveOpacity={INACTIVE} hoverDotClass="size-2 rounded-full border border-card bg-foreground" />
          <PlotAxis placement="left" labelPlacement="inside" tickValues={decades(Math.ceil(yLo()), yHi())} format={(v) => duration(10 ** v)} labelClass={VALUE_TICK} />
          <If when={props.by === "time"} else={<PlotAxis ticks={3} format={(v) => (v < 0 ? "" : bytes(10 ** v))} labelClass={BUCKET_TICK} />}>
            <TimeAxis domain={span()} />
          </If>
          <PlotLabel class="tabular-nums text-caption text-muted-foreground" texts={[{ text: `p95 ${duration(props.profile.p95Ms ?? 0)}`, x: xDomain()[1], y: p95(), align: "right", class: "-translate-y-full pb-1" }]} />
        </Plot>
        {/* every colour a dot may have, named: the outcomes this stretch holds, and a scenario's answers */}
        <Legend items={[...OUTCOMES.filter((o) => rows().some((r) => r.tone === o.tone)).map((o) => ({ label: t(o.key), swatch: o.swatch })), ...injectedLegend(props.injected), { label: t("scatter.p95Line"), swatch: "border-muted-foreground", line: true }]} />
      </BaseButton>
    </If>
  );
}

/** The busiest kinds of request, each its own small chart on one time axis:
 * its median to p95 as a band, slice by slice. Which kind slowed, and when. */
export function KindsOverTime(props: { profile: Profile; domainMs?: [number, number] | null }) {
  const kinds = () => {
    const by = new Map<number, { tMs: number; p50: number; p95: number }[]>();
    for (const g of props.profile.groupRate ?? []) {
      const tMs = g.bin * props.profile.binMs;
      if (props.domainMs && (tMs < props.domainMs[0] || tMs >= props.domainMs[1])) continue;
      by.set(g.group, [...(by.get(g.group) ?? []), { tMs, p50: g.p50Ms, p95: g.p95Ms }]);
    }
    return [...by.entries()].filter(([, rows]) => rows.length > 1).map(([group, rows]) => ({ group, name: groupName(props.profile.groups[group]), rows }));
  };
  const top = () => Math.max(...kinds().flatMap((k) => k.rows.map((r) => r.p95)), 1) * 1.15;
  const domain = (): [number, number] => props.domainMs ?? sessionSpan(props.profile, props.profile.rate.length);
  const hovered = signal<{ group: number; index: number } | null>(null);
  const tipOf = (k: { group: number; name: string; rows: { tMs: number; p50: number; p95: number }[] }) => {
    const r = hovered()?.group === k.group ? k.rows[hovered()!.index] : null;
    return r ? { title: k.name, lines: [`+${duration(r.tMs)}`, `p50 ${duration(r.p50)} · p95 ${duration(r.p95)}`] } : null;
  };
  return (
    <If when={!terminal() && kinds().length > 0}>
      <View testID="kinds-over-time" class="gap-1">
        <Each of={kinds()} key={(k) => `${k.group}`}>
          {(k, i) => (
            <View class="flex-row items-center">
              <Text class={ROW_LABEL}>{axisName(k().name)}</Text>
              <Plot data={k().rows} x="tMs" xScale={{ type: "linear", domain: domain() }} yScale={{ type: "linear", domain: [0, top()] }} accessibilityLabel={`${k().name}: p95 ${duration(Math.max(...k().rows.map((r) => r.p95)))}`} motion={MOTION} crossLineClass={CROSS_LINE} tooltipPlacement="pointer" tooltipOffset={10} tooltipContent={<ChartTip tip={tipOf(k())} />} onHoverChange={({ index }) => hovered.set(index === null ? null : { group: k().group, index })} style={{ height: PITCH }} class={`flex-1 ${i() === kinds().length - 1 ? TICK_ROOM : ""}`}>
                <Area y0="p50" y1="p95" curve="linear" class="fill-chart-1/20" />
                <Line y="p95" curve="linear" class="stroke-chart-1 stroke-[1.25]" hoverDotClass="size-1.5 rounded-full bg-chart-1" />
                <Line y="p50" curve="linear" class="stroke-chart-1/50 stroke-1" hoverDotClass="hidden" />
                <If when={i() === kinds().length - 1}>
                  <TimeAxis domain={domain()} />
                </If>
              </Plot>
            </View>
          )}
        </Each>
        <Legend gutter items={[{ label: "p95", swatch: "bg-chart-1" }, { label: t("kinds.band"), swatch: "bg-chart-1/30" }]} />
      </View>
    </If>
  );
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
    return r ? { title: r.operation, lines: [line(r), t("run.bandRequests", { n: r.count })] } : null;
  };
  return (
    <If when={!textual()} else={<SimpleTable columns={[t("profile.kind"), t("firstByte.first"), t("firstByte.rest"), t("figure.requests")]} rows={rows().map((r) => [r.operation, duration(r.firstByteMs), duration(r.restMs), count(r.count)])} />}>
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
        <Bar value="restMs" stack="t" class="fill-chart-4" cornerRadii={TIP.across} inactiveOpacity={INACTIVE} label={(_, i) => duration(rows()[i].firstByteMs + rows()[i].restMs)} labelClass={BAR_LABEL} />
        <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.operation ?? "")} labelClass={AXIS_LABEL} />
      </Plot>
    </If>
  );
}
