// Traffic: what ran when and at once — the lanes, the session's shape and
// its mark in a row, bytes each way, how large the bodies were.

import { computed, signal, Text, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { Bar, Cell, Grid, Line, Plot, PlotAppearScope, PlotAxis, PlotLabel, Rule } from "nsh/base/plot";
import { bytes, count, duration, rate, transfer } from "../../format";
import { t } from "../../i18n";
import { bytesRate, sizeLabel, sizeRange } from "../../signals";
import type { Profile, Signals } from "../../types";
import { AXIS_LABEL, BUCKET_TICK, BAR_LABEL, CROSS_BAND, ChartNote, ChartTip, type Focus, statusTone, GUTTER, HOVER, INACTIVE, Legend, MOTION, NoneInStretch, PITCH, PLOT, TICK_ROOM, TIME_TICK, TOP_LABEL, type Tone, VALUE_TICK_END, axisName, groupName, injectedLegend, nearest, pct, rowsHeight, stretch, tipValue, toneOf, tones, LegendItem, MARK, TIP, TimeAxis, ValueAxis, sessionSpan, BARS, CELLS, GRID, OUTCOMES, ZERO_SLIVER } from "./system";
import { EMPHASIS, terminal, textual, SimpleTable } from "../kit";
import { bytesSaid, lanesSaid } from "./answers";

// --- what ran at once ----------------------------------------------------------------

/** Each request its lane: a request takes the first lane free when it
 * starts, so the lanes in use at a moment are the requests in flight then. */
export function lanesOf(samples: { tMs: number; ms: number }[]): number[] {
  const order = samples.map((_, i) => i).sort((a, b) => samples[a].tMs - samples[b].tMs || a - b);
  const ends: number[] = [];
  const lane = new Array<number>(samples.length).fill(0);
  for (const i of order) {
    const start = samples[i].tMs;
    let k = ends.findIndex((e) => e <= start);
    if (k < 0) k = ends.push(0) - 1;
    ends[k] = start + Math.max(samples[i].ms, 1e-6);
    lane[i] = k;
  }
  return lane;
}

/** Every request of the session, as its slices count them: more than `samples` holds once a session is sampled. */
export const requestsOf = (p: Profile) => p.rate.reduce((n, b) => n + b.ok + b.miss + b.client + b.server, 0);

const LANES = 24;
const LANE_BINS = 96;
/** Outcomes from quiet to worst: a cell that holds several requests shows the worst. */
const WORST: Record<Tone, number> = { ok: 0, miss: 1, client: 2, server: 3, injected: 4 };

/** The lanes' cells over `domain`, cut in `bins` slices: one cell per slice
 * and lane, with how many requests it holds (`n`), the worst outcome among
 * them (`tone` — a failure is never painted over), and the request it stands
 * for (`sample`, an index into `samples`: the worst, the earliest of those).
 * Requests of a lane alternate two shades (`alt`) so two in a row read as
 * two; cells that hold several alternate by slice. In order of lane, then slice. */
export function laneCells(samples: { tMs: number; ms: number; lane: number; status?: number | null; id?: number }[], domain: [number, number], bins: number, lanes: number, injected?: Set<number>): { bin: number; lane: number; n: number; tone: Tone; alt: boolean; sample: number }[] {
  const [a, b] = domain;
  const w = (b - a) / bins;
  const order = samples.map((_, i) => i).sort((i, j) => samples[i].tMs - samples[j].tMs || i - j);
  const turn = new Map<number, number>();
  const cells = new Map<number, { bin: number; lane: number; n: number; tone: Tone; alt: boolean; sample: number }>();
  for (const i of order) {
    const x = samples[i];
    if (x.lane >= lanes || x.tMs >= b || x.tMs + x.ms < a) continue;
    const n = (turn.get(x.lane) ?? 0) + 1;
    turn.set(x.lane, n);
    const tone = toneOf(x, injected);
    const from = Math.max(0, Math.floor((x.tMs - a) / w));
    const to = Math.min(bins - 1, Math.floor((x.tMs + Math.max(x.ms, 0) - a) / w));
    for (let k = from; k <= to; k++) {
      const at = x.lane * bins + k;
      const c = cells.get(at);
      if (!c) cells.set(at, { bin: k, lane: x.lane, n: 1, tone, alt: n % 2 === 0, sample: i });
      else {
        c.n += 1;
        c.alt = k % 2 === 1;
        // the earliest of the worst: later ones of the same outcome do not take its place
        if (WORST[tone] > WORST[c.tone]) (c.tone = tone), (c.sample = i);
      }
    }
  }
  return [...cells.entries()].sort((p, q) => p[0] - q[0]).map(([, c]) => c);
}

/** The session's requests lane by lane: each a run of cells from its start
 * to its end, in its outcome's color, on the first lane free. What ran at
 * once is stacked; what waited for the one before follows it on its lane; a
 * long request is a long run. A press opens the request under the pointer. */
export function Lanes(props: { profile: Profile; domainMs?: [number, number] | null; onRequest?: (id: number) => void; /** the requests a fault scenario answered, by id */ injected?: number[]; /** the moment the pointer rests on, shared by the pane's charts */ focus?: Focus }) {
  const domain = (): [number, number] => props.domainMs ?? sessionSpan(props.profile, props.profile.rate.length);
  const all = computed(() => {
    const lane = lanesOf(props.profile.samples);
    return props.profile.samples.map((x, i) => ({ ...x, lane: lane[i] }));
  });
  const lanes = () => Math.min(LANES, all().reduce((m, x) => Math.max(m, x.lane + 1), 1));
  const hidden = () => all().filter((x) => x.lane >= LANES).length;
  // a cell per slice of time and lane, holding the requests that span it: drawn once, in the worst outcome of what it holds
  const marked = computed(() => new Set(props.injected ?? []));
  const rows = computed(() => laneCells(all(), domain(), LANE_BINS, LANES, marked()).map((c) => ({ ...c, x: `${c.bin}`, y: `${c.lane}`, id: all()[c.sample].id })));
  const hovered = signal<number | null>(null);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    const x = r ? all()[r.sample] : null;
    if (!r || !x) return null;
    const g = props.profile.groups[x.group];
    // several in one cell: how many, and that the figures are the worst one's (the one a press opens)
    return { title: g ? groupName(g) : "", lines: [`+${duration(x.tMs)} · ${duration(x.ms)}`, `${x.status ?? "—"} · ${bytes(x.bytes ?? 0)}`, ...(r.n > 1 ? [t("lanes.cellHolds", { n: r.n })] : [])] };
  };
  // the lanes are of the requests sampled: said when that is not all of them
  const total = () => requestsOf(props.profile);
  const label = () => lanesSaid(props.profile, props.domainMs);
  const bins = Array.from({ length: LANE_BINS }, (_, i) => `${i}`);
  return (
    <If when={!terminal() && all().length > 1} else={<Text class="text-small text-muted-foreground">{label()}</Text>}>
      <View class="gap-2">
        <If when={rows().length > 0} else={<NoneInStretch />}>
        <BaseButton
          accessibilityLabel={label()}
          disabled={!props.onRequest}
          onPress={() => {
            const r = hovered() === null ? null : rows()[hovered()!];
            if (r) props.onRequest?.(r.id);
          }}
        >
          <Plot
            testID="lanes"
            data={rows()}
            xScale={{ type: "band", domain: bins, paddingInner: CELLS.run }}
            yScale={{ type: "band", domain: Array.from({ length: lanes() }, (_, i) => `${i}`), paddingInner: CELLS.lane }}
            colorScale={tones()}
            accessibilityLabel={label()}
            accessibilityValue={tipValue(tip())}
            motion={MOTION}
            tooltipPlacement="pointer"
            tooltipOffset={10}
            tooltipContent={<ChartTip tip={tip()} />}
            onHoverChange={({ index }) => {
              hovered.set(index);
              const r = index === null ? null : rows()[index];
              props.focus?.set(r ? all()[r.sample].tMs : null);
            }}
            class={`${TICK_ROOM} w-full`}
            // not rows of names: a few lanes are tall, many are thin, about the height of the chart beside it either way
            style={{ height: lanes() * Math.max(8, Math.min(PITCH, Math.round(176 / lanes()))) + 6 }}
          >
            <Cell x={(_, i) => (rows()[i]?.alt ? (undefined as unknown as string) : (rows()[i]?.x ?? ""))} y="y" color="tone" inactiveOpacity={INACTIVE} hoverCellClass={HOVER} />
            <Cell x={(_, i) => (rows()[i]?.alt ? rows()[i].x : (undefined as unknown as string))} y="y" color="tone" inactiveOpacity={INACTIVE} style={{ opacity: 0.6 }} hoverCellClass={HOVER} />
            <TimeAxis domain={domain()} slices={LANE_BINS} />
          </Plot>
        </BaseButton>
        </If>
        {/* the outcomes the session had, and only those: no "Failed" under a run that never failed */}
        <Legend items={[...OUTCOMES.filter((o) => all().some((x) => toneOf(x, marked()) === o.tone)).map((o) => ({ label: t(o.key), swatch: o.swatch })), ...injectedLegend(props.injected)]}>
          {/* the second shade the cells are drawn in: named, as every colour a chart draws */}
          <ChartNote testID="lanes-shade">{t("lanes.shade")}</ChartNote>
          <If when={all().length < total()}>
            <ChartNote testID="lanes-sampled">{t("plots.sampled", { n: count(all().length), of: count(total()) })}</ChartNote>
          </If>
          <If when={hidden() > 0}>
            <ChartNote>{t("lanes.more", { n: hidden(), lanes: LANES })}</ChartNote>
          </If>
        </Legend>
      </View>
    </If>
  );
}

// --- the shape of a session ---------------------------------------------------

type Slice = { tMs: number; ok: number; miss: number; client: number; server: number; injected: number; inFlight: number };

/** The session's slices with the most requests in flight in each. A fault
 * scenario's answers (`injected`, by id) are counted apart, in their own
 * tone: taken from the outcome they looked like, never the store's failure. */
export function slices(p: Profile, injected?: number[]): Slice[] {
  let k = 0;
  let last = 0;
  // the requests sonde answered, slice by slice and by the outcome they looked like
  const marked = new Set(injected ?? []);
  const apart = new Map<number, Record<"ok" | "miss" | "client" | "server", number>>();
  if (marked.size)
    for (const x of p.samples) {
      if (!marked.has(x.id)) continue;
      const i = Math.min(Math.floor(x.tMs / p.binMs), p.rate.length - 1);
      const c = apart.get(i) ?? { ok: 0, miss: 0, client: 0, server: 0 };
      c[statusTone(x.status ?? null) as "ok" | "miss" | "client" | "server"] += 1;
      apart.set(i, c);
    }
  return p.rate.map((b, i) => {
    const end = b.tMs + p.binMs;
    let peak = last;
    while (k < p.inFlight.length && p.inFlight[k].tMs < end) {
      peak = Math.max(peak, p.inFlight[k].n);
      last = p.inFlight[k].n;
      k++;
    }
    const c = apart.get(i);
    if (!c) return { ...b, injected: 0, inFlight: peak };
    const less = (v: number, by: number) => Math.max(0, v - by);
    return { ...b, ok: less(b.ok, c.ok), miss: less(b.miss, c.miss), client: less(b.client, c.client), server: less(b.server, c.server), injected: Math.min(c.ok + c.miss + c.client + c.server, b.ok + b.miss + b.client + b.server), inFlight: peak };
  });
}

/** In words: what the shape shows and what stands out. */
export function shapeSentence(p: Profile): string {
  return t("profile.shapeText", { busy: duration(p.waitingMs), wall: duration(p.wallMs), pct: pct(p.waitingMs / Math.max(p.wallMs, 1e-9)), n: p.peakInFlight, at: duration(p.peakAtMs) });
}

/**
 * Requests over a session: a bar per slice of time, stacked by outcome, and
 * a step line of the requests in flight. A press picks the slice under the
 * pointer (again: lets it go), for the operations running then.
 */
export function SessionShape(props: { profile: Profile; slice?: number | null; onSlice?: (i: number | null) => void; class?: string; testID?: string; annotate?: boolean; marks?: { tMs: number; n: number }[]; /** the moments a fault scenario answered a request (a trial's session), in ms */ faults?: number[]; axis?: boolean; range?: [number, number] | null; /** the stretch chosen elsewhere (the Time pane's), as slices: marked, the rest quiet */ picked?: [number, number] | null; focus?: Focus; /** the strip under a session's header: a mark, in no frame */ strip?: boolean; /** the most in flight, as the session's signals count it */ peak?: number; /** the requests a fault scenario answered, by id: their own tone in the stack */ injected?: number[] }) {
  const hovered = signal<number | null>(null);
  // zoomed: the slices from `range[0]` to `range[1]`, wider each
  const from = () => props.range?.[0] ?? 0;
  const rows = () => {
    const all = slices(props.profile, props.injected);
    return props.range ? all.slice(props.range[0], props.range[1] + 1) : all;
  };
  const top = () => Math.max(1, ...rows().map((r) => r.ok + r.miss + r.client + r.server + r.injected));
  // the plot's own top: a fifth over the busiest slice where the scale is written
  const room = () => (props.axis && !props.strip ? 1.2 : 1);
  const ceiling = () => top() * room();
  // the most in flight: the session's own figure (`peak`, what the In flight tile says) while
  // the whole session shows, the stretch's own when zoomed
  const flightTop = () => (props.range ? Math.max(0, ...rows().map((r) => r.inFlight)) : (props.peak ?? props.profile.peakInFlight));
  // where the findings happened: their numbers over the slices they start in
  const marked = () => {
    const m = new Map<number, number[]>();
    // numbers a few slices apart would print over each other: they share the first one's place
    const near = Math.max(2, Math.round(rows().length / 40));
    const at = (props.marks ?? [])
      // a finding at the session's last instant is in its last slice
      .map((k) => ({ n: k.n, i: Math.min(Math.floor(k.tMs / props.profile.binMs), slices(props.profile).length - 1) - from() }))
      .filter((k) => k.i >= 0 && k.i < rows().length)
      .sort((a, b) => a.i - b.i || a.n - b.n);
    let last = -Infinity;
    for (const k of at) {
      const i = k.i - last <= near ? last : k.i;
      // in the order of their numbers, whichever began first: "1, 2, 3" reads, "2 3 1" does not
      m.set(i, [...(m.get(i) ?? []), k.n].sort((x, y) => x - y));
      last = i;
    }
    return m;
  };
  // the stretch shown, in ms: what the one time axis names
  const shownMs = (): [number, number] => [rows()[0]?.tMs ?? 0, (rows()[rows().length - 1]?.tMs ?? 0) + props.profile.binMs];
  // the slices a fault scenario answered a request in
  const faulted = () => new Set((props.faults ?? []).map((ms) => Math.min(Math.floor(ms / props.profile.binMs), slices(props.profile).length - 1) - from()).filter((i) => i >= 0 && i < rows().length));
  const focused = () => (hovered() === null ? nearest(rows(), props.focus?.() ?? null) : null);
  const peakBin = () => {
    const at = props.profile.peakAtMs;
    const i = Math.floor(at / props.profile.binMs) - from();
    return props.profile.peakInFlight > 1 && i >= 0 && i < rows().length ? i : null;
  };
  // a slice pressed, or the stretch chosen (`picked`): what it leaves out turns quiet, it does not go
  const on = (i: number) => (props.slice != null ? props.slice === i + from() : !props.picked || (i + from() >= props.picked[0] && i + from() <= props.picked[1]));
  const tone = (x: Tone) => (d: Slice, i: number) => (on(i) ? x : "dim");
  const tip = () => {
    const i = hovered();
    const r = i === null ? null : rows()[i];
    if (!r) return null;
    const n = r.ok + r.miss + r.client + r.server + r.injected;
    // what began in the slice (an operation that began before and still runs is no request of it: a press
    // shows the operations that span it, so a slice of none begun is not said to hold nothing)
    const parts = [n === 0 ? t("shape.noneBegan") : t("shape.began", { n }), ...(r.miss ? [t("profile.missN", { n: r.miss })] : []), ...(r.client + r.server ? [t("profile.failedN", { n: r.client + r.server })] : []), ...(r.injected ? [t("failures.injected", { n: count(r.injected) })] : [])];
    // the findings that begin in this slice: what the numbers over it are
    const marks = marked().get(i!) ?? [];
    return { title: `+${duration(r.tMs)} – +${duration(r.tMs + props.profile.binMs)}`, lines: [parts.join(" · "), ...(r.inFlight > 0 ? [t("profile.inFlightN", { n: r.inFlight })] : []), ...(props.strip && props.onSlice && n > 0 ? [t("shape.pressSpans")] : []), ...(props.annotate && marks.length ? [t("shape.findingsHere", { list: marks.join(", ") })] : [])] };
  };
  return (
    // a handful of requests has no shape worth a strip; a chart of its own (the Time pane's, with its axes) draws them
    <If when={!terminal() && rows().length > 1 && ((props.axis && !props.strip) || props.profile.samples.length >= 12)} else={<Text class="hidden text-muted-foreground terminal:flex">{shapeSentence(props.profile)}</Text>}>
      <BaseButton
        testID={props.testID ?? "session-shape"}
        accessibilityLabel={shapeSentence(props.profile)}
        disabled={!props.onSlice}
        onPress={() => {
          const h = hovered();
          const i = h === null ? null : h + from();
          if (i !== null) props.onSlice?.(props.slice === i ? null : i);
        }}
        class={`rounded-(--radius) ${props.class ?? ""}`}
      >
        <Plot
          data={rows()}
          xScale={{ type: "band", ...BARS.slices }}
          // headroom over the busiest slice where the scale is written: its chip stands over the peak's line, clear of the bars
          yScale={{ type: "linear", domain: [0, ceiling()] }}
          y2Scale={{ type: "linear", domain: [0, Math.max(1, flightTop()) * room()] }}
          alignTicks={false}
          colorScale={tones()}
          accessibilityLabel={shapeSentence(props.profile)}
          accessibilityValue={tipValue(tip())}
          appearGeneration={1}
          motion={MOTION}
          crossLineClass={CROSS_BAND}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => {
            hovered.set(index);
            props.focus?.set(index === null ? null : rows()[index].tMs + props.profile.binMs / 2);
          }}
          class="h-full w-full"
        >
          <Bar value="ok" stack="r" color={tone("ok")} />
          <Bar value="miss" stack="r" color={tone("miss")} />
          <Bar value="client" stack="r" color={tone("client")} />
          <Bar value="server" stack="r" color={tone("server")} />
          {/* what sonde answered in the store's place: its own tone, never a failure's */}
          <Bar value="injected" stack="r" color={tone("injected")} cornerRadii={TIP.up} />
          <Line y="inFlight" scale="y2" curve="stepAfter" class="stroke-foreground/30 stroke-[1.25]" hoverDotClass="hidden" />
          {/* full width: when (from the session's start) and how many at once */}
          <If when={props.axis}>
            <TimeAxis domain={shownMs()} slices={rows().length} />
          </If>
          {/* how many: zero and the busiest slice's requests; at the far edge, the most in flight in
              what is shown (the whole session's is the In flight tile's number) */}
          <If when={props.axis && !props.strip}>
            <ValueAxis peak={top()} say={(v) => t("shape.peak", { n: count(v) })} />
            <PlotAxis placement="right" scale="y2" labelPlacement="inside" tickValues={[flightTop()]} format={(v) => (Math.round(v) > 0 ? t("profile.inFlightN", { n: Math.round(v) }) : "")} labelClass={VALUE_TICK_END} />
          </If>
          {/* the moment another chart rests on */}
          <Rule value={(_, i) => (i === focused() ? ceiling() : (undefined as unknown as number))} class="stroke-foreground/40 stroke-1" />
          {/* where sonde answered in the store's place */}
          <If when={faulted().size > 0}>
            <Rule value={(_, i) => (faulted().has(i) ? ceiling() : (undefined as unknown as number))} class="stroke-status-injected stroke-[1.5]" />
          </If>
          {/* where each finding begins */}
          <If when={props.annotate && marked().size > 0}>
            <Rule value={(_, i) => (marked().has(i) ? ceiling() : (undefined as unknown as number))} class="stroke-warn/70 stroke-1" style={{ strokeDasharray: "2 2" }} />
            {/* (a mark near the plot's edge is set from that edge, so its numbers are not cut) */}
            <PlotLabel /* emphasis: a finding's number over its slice */ class={`tabular-nums text-caption ${EMPHASIS} text-warn`} texts={[...marked()].map(([i, ns]) => ({ text: ns.join(", "), x: i, y: ceiling(), align: i < 2 ? ("left" as const) : i > rows().length - 3 ? ("right" as const) : ("center" as const), class: "-translate-y-full pb-1" }))} />
          </If>
          {/* the busiest moment, named where it happened */}
          {/* (left out where a finding's number stands near it: the two would cross; Time says it too) */}
          <If when={props.annotate && peakBin() !== null && ![...marked().keys()].some((i) => Math.abs(i - peakBin()!) <= Math.max(3, rows().length / 12))}>
            <PlotLabel
              scale="y2"
              class="tabular-nums text-caption text-foreground"
              texts={[{ text: t("profile.peakLabel", { n: props.profile.peakInFlight }), x: peakBin()!, y: props.profile.peakInFlight, align: "center", class: "-translate-y-full pb-1" }]}
            />
          </If>
        </Plot>
      </BaseButton>
    </If>
  );
}

/** What the shape's colours and marks stand for: the outcomes the session
 * had, the line of requests in flight, and (with `marks`) the findings'
 * numbers. The whole legend of the timeline. */
export function shapeLegend(p: Profile, marks: boolean, injected?: number[]): LegendItem[] {
  const all = slices(p, injected);
  const had = (k: "miss" | "client" | "server") => all.some((b) => b[k] > 0);
  return [
    { label: t("flows.ok"), swatch: "bg-bar" },
    ...(had("miss") ? [{ label: t("flows.notfound"), swatch: "bg-status-miss" }] : []),
    ...(had("client") ? [{ label: t("flows.clienterror"), swatch: "bg-status-client" }] : []),
    ...(had("server") ? [{ label: t("lanes.failed"), swatch: "bg-status-server" }] : []),
    { label: t("shape.inFlightLine"), swatch: "bg-foreground/30" },
    ...(marks ? [{ label: t("shape.findingMarks"), swatch: "bg-warn" }] : []),
  ];
}

/** When a fault scenario answered a request, in ms from the session's start:
 * the injected requests among those the session keeps (`samples`). */
export function faultMoments(p: Profile, injected: number[] | undefined): number[] {
  if (!injected?.length) return [];
  const ids = new Set(injected);
  return p.samples.filter((x) => ids.has(x.id)).map((x) => x.tMs);
}

// --- a session in a row --------------------------------------------------------------

/** A session's requests over its time, a few points high: drawn once per
 * session, kept as it is when its row scrolls away and back. */
export function Sparkline(props: { shape: number[]; id: string; class?: string; /** its tone: a signal's own (failures in the server's), or quiet (the run before, beside the run after) */ tone?: "plain" | "bad" | "client" | "injected" | "quiet"; /** what it draws: counts as bars (the default), a level as steps (requests in flight), a measure as a line (latency) */ kind?: "bars" | "steps" | "line"; /** shown whole at once: a mark beside a figure does not draw itself in */ still?: boolean }) {
  const rows = () => props.shape.map((n, i) => ({ i, n }));
  const stroke = () => (props.tone === "bad" ? "stroke-status-server" : props.tone === "client" ? "stroke-status-client" : props.tone === "injected" ? "stroke-status-injected" : props.tone === "quiet" ? "stroke-muted-foreground" : "stroke-chart-1");
  const fill = () => (props.tone === "bad" ? "fill-status-server" : props.tone === "client" ? "fill-status-client" : props.tone === "injected" ? "fill-status-injected" : props.tone === "quiet" ? "fill-muted-foreground" : "fill-chart-1");
  const plot = () => (
    <Plot data={rows()} xScale={{ type: "band", ...BARS.buckets }} yScale={{ type: "linear", domain: [0, Math.max(1, ...props.shape)] }} interactive={false} accessibilityLabel="" appearGeneration={props.still ? undefined : 1} motion={props.still ? undefined : MOTION} class={props.class ?? MARK.row}>
      {/* a baseline, so a few bars read as bars on a line and not as stray ticks */}
      <Grid y={[0]} class="stroke-muted-foreground/50" />
      <If when={(props.kind ?? "bars") === "bars"} else={<Line y="n" curve={props.kind === "steps" ? "stepAfter" : "linear"} class={`${stroke()} stroke-[1.5]`} hoverDotClass="hidden" />}>
        <Bar value="n" class={fill()} cornerRadii={TIP.mark} />
      </If>
    </Plot>
  );
  return (
    <If when={!terminal() && props.shape.some((n) => n > 0)}>
      <If when={!props.still} else={plot()}>
        <PlotAppearScope id={props.id}>{plot()}</PlotAppearScope>
      </If>
    </If>
  );
}

// --- how requests were answered ---------------------------------------------------


// --- the live view ----------------------------------------------------------------


/** What the last few seconds of a session looked like: requests a second,
 * the share that failed, in flight now and the p95, for the live view. */
export function liveFigures(p: Profile, windowMs = 1000) {
  // the last second (or more, when a slice is longer): a few slices of a
  // short session are milliseconds, and 4 requests in them are not "200 a second"
  const slices = Math.max(1, Math.ceil(windowMs / Math.max(p.binMs, 1e-3)));
  const recent = p.rate.slice(-slices);
  const n = recent.reduce((s, b) => s + b.ok + b.miss + b.client + b.server, 0);
  const failed = recent.reduce((s, b) => s + b.client + b.server, 0);
  const secs = Math.max(slices * p.binMs, windowMs) / 1000;
  const p95 = Math.max(0, ...recent.map((b) => b.p95Ms));
  const inFlight = p.inFlight.length ? p.inFlight[p.inFlight.length - 1].n : 0;
  return { perSecond: n / secs, failedShare: n ? failed / n : 0, inFlight, p95 };
}

/** Bytes of bodies a second over the session: sent above the line, received
 * below it, on the timeline's slices. */
export function BytesOverTime(props: { signals: Signals; range?: [number, number] | null; focus?: Focus }) {
  const s = () => props.signals;
  const rows = () => stretch(s().bytes.up.map((up, i) => ({ tMs: i * s().binMs, up, down: -(s().bytes.down[i] ?? 0) })), props.range);
  const top = () => Math.max(1, ...rows().map((r) => r.up)) * 1.2;
  const bottom = () => Math.min(-1, ...rows().map((r) => r.down)) * 1.2;
  const hovered = signal<number | null>(null);
  const label = () => bytesSaid(s(), props.range);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: `+${duration(r.tMs)}`, lines: [transfer(r.up, -r.down, bytesRate)] } : null;
  };
  const focused = () => (hovered() === null ? nearest(rows(), props.focus?.() ?? null) : null);
  return (
    <If when={!terminal()} else={<Text class="text-foreground">{label()}</Text>}>
      <If when={rows().some((r) => r.up > 0 || r.down < 0)} else={<NoneInStretch />}>
        <Plot
          testID="bytes-over-time"
          data={rows()}
          xScale={{ type: "band", ...BARS.slices }}
          yScale={{ type: "linear", domain: [bottom(), top()] }}
          accessibilityLabel={label()}
          accessibilityValue={tipValue(tip())}
          appearGeneration={1}
          motion={MOTION}
          crossLineClass={CROSS_BAND}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => {
            hovered.set(index);
            props.focus?.set(index === null ? null : rows()[index].tMs + s().binMs / 2);
          }}
          class={`${TICK_ROOM} ${PLOT} w-full`}
        >
          <Grid y={[0]} class={GRID} />
          {/* one stem per slice: received hangs under the axis (from its depth up to zero), sent stands on it */}
          <Bar value={(d) => -d.down} base="down" stack="bytes" class="fill-chart-4" cornerRadii={TIP.down} inactiveOpacity={INACTIVE} />
          <Bar value="up" stack="bytes" class="fill-chart-1" cornerRadii={TIP.up} inactiveOpacity={INACTIVE} />
          <Rule value={(_, i) => (i === focused() ? top() : (undefined as unknown as number))} base={bottom()} class="stroke-foreground/40 stroke-1" />
          <TimeAxis domain={[rows()[0]?.tMs ?? 0, (rows()[rows().length - 1]?.tMs ?? 0) + s().binMs]} slices={rows().length} />
          {/* zero, and the most a second each way */}
          <ValueAxis peak={top() / 1.2} low={bottom() < -1.2 ? bottom() / 1.2 : undefined} say={bytesRate} />
        </Plot>
      </If>
    </If>
  );
}

/** How large the bodies were: requests sent and answers received, a pair of
 * bars per size (each twice the one before), by how many. */
export function Sizes(props: { signals: Signals }) {
  const z = () => props.signals.sizes;
  const rows = () => {
    const les = [...new Set([...z().request, ...z().response].map((b) => b.leBytes))].sort((a, b) => a - b);
    const of = (list: Signals["sizes"]["request"], le: number) => list.find((b) => b.leBytes === le);
    return les.map((le) => ({ le, name: sizeLabel(le), up: of(z().request, le)?.count ?? 0, down: of(z().response, le)?.count ?? 0, upBytes: of(z().request, le)?.bytes ?? 0, downBytes: of(z().response, le)?.bytes ?? 0 }));
  };
  const hovered = signal<number | null>(null);
  const most = () => [...rows()].filter((r) => r.le > 0).sort((a, b) => b.up + b.down - (a.up + a.down))[0] ?? rows()[0];
  const label = () => (most() ? t("sizes.text", { size: most().name, n: count(most().up + most().down) }) : "");
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: sizeRange(r.le), lines: [t("sizes.sentLine", { n: count(r.up), size: bytes(r.upBytes) }), t("sizes.receivedLine", { n: count(r.down), size: bytes(r.downBytes) })] } : null;
  };
  // every bucket named where there is room for it, else every other one
  const ticks = () => rows().map((_, i) => i).filter((i) => rows().length <= 12 || i % 2 === 0);
  // how many, at each bar's end (so the scale writes zero alone): "0" beside a bar that has a count, none where neither has
  const n = (r: { up: number; down: number }, v: number) => (v > 0 || r.up + r.down > 0 ? count(v) : "");
  // a bar of none beside one with a count is drawn as a sliver on the baseline, so its "0" has a bar to stand on
  // (a bar of no value is not drawn, and its label with it)
  const top = () => Math.max(1, ...rows().map((r) => Math.max(r.up, r.down)));
  const shown = (r: { up: number; down: number }, v: number) => (v > 0 ? v : r.up + r.down > 0 ? top() * ZERO_SLIVER : 0);
  return (
    <If when={!textual()} else={<SimpleTable columns={[t("requests.size"), t("sizes.sent"), t("sizes.received")]} rows={rows().map((r) => [r.name, count(r.up), count(r.down)])} />}>
      <Plot
        testID="sizes"
        data={rows()}
        xScale={{ type: "band", ...BARS.buckets }}
        yScale={{ type: "linear", domain: [0, Math.max(1, ...rows().map((r) => Math.max(r.up, r.down))) * 1.22] }}
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
        <Bar value={(r) => shown(r, r.up)} class="fill-chart-1" cornerRadii={TIP.up} inactiveOpacity={INACTIVE} label={(_, i) => n(rows()[i], rows()[i].up)} labelClass={TOP_LABEL} />
        <Bar value={(r) => shown(r, r.down)} class="fill-chart-4" cornerRadii={TIP.up} inactiveOpacity={INACTIVE} label={(_, i) => n(rows()[i], rows()[i].down)} labelClass={TOP_LABEL} />
        <PlotAxis tickValues={ticks()} format={(i) => rows()[i]?.name ?? ""} labelClass={BUCKET_TICK} />
        {/* every bar says its own count over itself: the scale writes zero alone (no peak chip) */}
        <ValueAxis marked peak={Math.max(1, ...rows().map((r) => Math.max(r.up, r.down)))} say={(v) => t("sizes.peak", { n: count(v) })} />
      </Plot>
    </If>
  );
}

/** Time against size: how fast bodies of each size moved, by kind (the
 * median of each, in MiB a second) — a row an operation and a size. */
export function Throughput(props: { signals: Signals; max?: number }) {
  const rows = () => props.signals.sizes.throughput.slice(0, props.max ?? 10).map((x) => ({ ...x, name: `${x.kind} · ${sizeLabel(x.bucket)}` }));
  const hovered = signal<number | null>(null);
  const speed = (v: number) => t("throughput.speed", { n: rate(v) });
  const label = () => (props.signals.sizes.mibPerSec == null ? "" : t("throughput.text", { speed: speed(props.signals.sizes.mibPerSec) }));
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: r.name, lines: [speed(r.mibPerSec), t("run.bandRequests", { n: r.count })] } : null;
  };
  return (
    <If when={!textual()} else={<SimpleTable columns={[t("column.kind"), t("requests.size"), t("throughput.column"), t("figure.requests")]} rows={rows().map((r) => [r.kind, sizeLabel(r.bucket), speed(r.mibPerSec), count(r.count)])} />}>
      <Plot
        testID="throughput"
        data={rows()}
        yScale={{ type: "band", ...BARS.rows }}
        xScale={{ type: "linear", domain: [0, Math.max(...rows().map((r) => r.mibPerSec), 1e-3) * 1.3] }}
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
        <Bar value="mibPerSec" class="fill-chart-1" cornerRadii={TIP.across} inactiveOpacity={INACTIVE} stagger={0.3} label={(_, i) => speed(rows()[i].mibPerSec)} labelClass={BAR_LABEL} />
        <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.name ?? "")} labelClass={AXIS_LABEL} />
      </Plot>
    </If>
  );
}
