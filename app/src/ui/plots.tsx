// sonde's charts, on Native Shell's Plot: drawn natively from the rows, the
// pointer followed in Rust, colors from the theme's tokens. What a session's
// time looked like (requests by outcome and requests in flight), where it
// went (by kind of request), how long each request took, an operation's
// requests over time, and two runs side by side. Each says in words what it
// shows; a terminal gets the words and a table instead.

import { computed, type JSX, signal, Text, token, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { Area, Bar, Cell, Dot, Grid, Line, Plot, PlotAppearScope, PlotAxis, PlotLabel, Rule, Sankey, Treemap } from "nsh/base/plot";
import { bytes, cost, count, day, duration, factor, middle, percent } from "../format";
import { t } from "../i18n";
import type { LatencyChange, Observation, Profile, ProfileGroup, ProgramHistory, RecordSummary, RunChange } from "../types";
import { observationSaving, observationText } from "../phrases";
import { Card, EMPHASIS, Empty, Figure, Heading, Options, POPUP, terminal, textual, SimpleTable } from "./kit";

export { cost };

/** A kind of request as people say it: "HeadObject" or "HeadObject, not found". */
export const groupName = (g: Pick<ProfileGroup, "operation" | "missed">) => (g.missed ? t("profile.notFound", { op: g.operation }) : g.operation);

const pct = percent;

// --- one chart system: every constant a chart is drawn with, here and nowhere else ----
// (scripts/lint-charts.py counts them: one frame, one left edge, one row
// pitch, one treemap height, one legend, one tooltip, one fade, one outline)

/** The chart components: each draws its `Plot`s as the body of a `Chart`
 * frame. lint-charts fails a `Plot` outside these, and one of these used
 * outside a frame. */
export const CHARTS = ["Lanes", "SessionShape", "LatencyBand", "Chains", "StoreLatency", "TimeSpent", "Waterfall", "RunBars", "Flows", "Heat", "Pressure", "Distribution", "PrefixUse", "PrefixChangeMap", "RunTrend", "RequestScatter", "KindsOverTime", "DistributionCompare", "BarChart", "StackBar", "PrefixMap"] as const;
/** Marks that are not charts and stand in no frame: a session's requests a
 * few points high in its row, and the shape of a session as the strip under
 * its header (`SessionShape strip`). */
export const MARKS = ["Sparkline", "SessionShape strip"] as const;

/** The room left of a chart across, for the names of its rows: one width
 * for every chart, so their bars start at one edge. Wide enough for
 * "GetObject, not found" and "Chain 2 · 18 requests" whole. */
export const GUTTER = "ml-44";
/** The same room before a legend or a note under such a chart. */
export const GUTTER_PAD = "pl-44";
/** The same room either side: a chart with names at both edges (flows). */
const GUTTER_BOTH = "mx-44";
/** A row's name in that room: right-aligned; `axisName` cuts what does not fit, in its middle. */
export const AXIS_LABEL = "w-44 pr-3 text-right text-small text-foreground line-clamp-1";
/** The same name beside a plot that has no axis of names (small charts in rows). */
const ROW_LABEL = "w-44 shrink-0 pr-3 text-right text-small text-foreground line-clamp-1";
/** What the gutter holds of a name, in characters: a longer one is cut in its
 * middle (both ends tell kinds apart), never at its end by the cell. */
const GUTTER_CHARS = 25;
export const axisName = (name: string) => middle(name, GUTTER_CHARS);
/** A row of a chart of rows: one pitch for every chart. Charts that are not
 * rows have a height of their own: a map of areas (`TREEMAP`), requests
 * against time (`PLOT`), lanes (as many as ran at once), flows (`FLOWS`). */
export const PITCH = 28;
/** A chart of rows: its height for `n` of them. */
export const rowsHeight = (n: number) => Math.max(1, n) * PITCH + 8;
/** Every map of areas: one height. */
export const TREEMAP = "h-56";
/** Every plot against time or size that is not rows. */
const PLOT = "h-44";
/** The flows' height: three columns of nodes need the room. */
const FLOWS = "h-80";
/** Under a chart whose bottom axis has ticks: the room they take. */
const TICK_ROOM = "mb-6";
/** What the pointer rests on: a band over a row or a slice, a line at a moment. */
export const CROSS_BAND = "w-full h-full rounded-(--radius-sm) bg-foreground/5";
const CROSS_LINE = "w-px h-full bg-foreground/25";
/** What the pointer does not rest on fades to this, in every chart. */
export const INACTIVE = 0.4;
/** The one outline of the cell, the area or the node under the pointer. */
export const HOVER = "rounded-(--radius-sm) outline-2 outline-foreground";
/** A time axis's ticks: three, tabular, quiet. */
export const TIME_TICK = "pt-1 tabular-nums text-caption text-muted-foreground";
/** A value axis's ticks, inside the plot on a chip. */
export const VALUE_TICK = "ml-1 rounded-(--radius-sm) bg-card/85 px-1 tabular-nums text-caption text-muted-foreground";
/** The same at the plot's far edge (a second scale). */
const VALUE_TICK_END = "mr-1 rounded-(--radius-sm) bg-card/85 px-1 tabular-nums text-caption text-muted-foreground";
/** A figure at a bar's end, a note in the plot. */
const BAR_LABEL = "pl-2 tabular-nums text-caption text-muted-foreground";
/** Two charts abreast where the board is wide, one under another where it is not. */
export const HALF = "min-w-[420px] flex-1";

/** The first, middle and last of `n` places along an axis, each once: two
 * places have two ticks, not the first one twice. */
export const ticks3 = (n: number): number[] => [...new Set([0, Math.floor((n - 1) / 2), n - 1].filter((i) => i >= 0))];

// the keys and screen readers step the hover with a spring; the pointer's own moves follow at once
export const MOTION = { appear: { duration: 420, easing: "easeOut" as const }, update: { duration: 260, easing: "easeOut" as const }, enter: { duration: 100 }, exit: { duration: 160 }, pointer: { stiffness: 420, damping: 36 } };

const TONES = ["ok", "miss", "client", "server"] as const;
type Tone = (typeof TONES)[number];
const TONE_TOKEN: Record<Tone, string> = { ok: "bar", miss: "status-miss", client: "status-client", server: "status-server" };
const tones = () => ({ type: "ordinal" as const, domain: [...TONES, "dim"], range: [...TONES.map((x) => token(TONE_TOKEN[x] as never) as unknown as string), token("bar-track" as never) as unknown as string] });

export type ChartTipText = { title: string; lines: string[] };

/** The one tooltip of every chart: the popup surface, a title and a line or
 * two of figures, beside the pointer (a Plot's `tooltipContent`, with
 * `tooltipPlacement="pointer"` and `tooltipOffset={10}`). Nothing while the
 * pointer is on nothing. */
export function ChartTip(props: { tip: ChartTipText | null | undefined }) {
  return (
    <If when={props.tip}>
      {(x) => (
        <View class={`max-w-[280px] gap-1 ${POPUP} px-2 py-1`}>
          <Text class="text-small font-medium text-popover-foreground line-clamp-2">{x().title}</Text>
          <Each of={x().lines}>{(l) => <Text class="tabular-nums text-caption text-muted-foreground">{l()}</Text>}</Each>
        </View>
      )}
    </If>
  );
}
/** What a screen reader hears of the row under the pointer: the tooltip's words. */
const tipValue = (tip: ChartTipText | null | undefined) => ({ text: tip ? `${tip.title}: ${tip.lines.join(", ")}` : "" });

export type LegendItem = { label: string; swatch: string };

/** What a chart's colors stand for: a round swatch (its `bg-*` class) and a
 * name each, in a row that wraps, and after them what else is to be said
 * under the chart (`children`). `gutter` starts it at the plot's left edge
 * under a chart of rows. The one legend. */
export function Legend(props: { items: LegendItem[]; gutter?: boolean; children?: JSX.Element }) {
  return (
    <View testID="chart-legend" class={`flex-row flex-wrap items-center gap-x-4 gap-y-1 ${props.gutter ? GUTTER_PAD : ""}`}>
      <Each of={props.items} key={(it) => it.label}>
        {(it) => (
          <View class="flex-row items-center gap-1">
            <View class={`size-2 rounded-full ${it().swatch}`} />
            <Text class="text-caption text-muted-foreground">{it().label}</Text>
          </View>
        )}
      </Each>
      {props.children}
    </View>
  );
}

/** A line under a chart or beside its legend: what the picture leaves out or stands for. */
export function ChartNote(props: { children: JSX.Element; gutter?: boolean; testID?: string }) {
  return (
    <Text testID={props.testID} class={`tabular-nums text-caption text-muted-foreground ${props.gutter ? GUTTER_PAD : ""}`}>
      {props.children}
    </Text>
  );
}

/** A chart in its frame, the same for every chart: what it asks (`title`), a
 * sentence that answers it (`text`, which wraps: never cut), how it is cut
 * (`action`, at the title's end), the plot, its `legend` at the plot's left
 * edge (`gutter` for a chart of rows), and `empty` said in the plot's room
 * when there is nothing to draw. A terminal gets the title and what the
 * chart says in words. */
export function Chart(props: { title: string; text?: string; action?: JSX.Element; children?: JSX.Element; legend?: LegendItem[]; gutter?: boolean; empty?: string | null | false; testID?: string; class?: string }) {
  if (terminal()) {
    return (
      <View testID={props.testID} class="gap-0">
        <Heading>{props.title}</Heading>
        <If when={!props.empty} else={<Text class="text-muted-foreground">{props.empty}</Text>}>
          {props.children}
        </If>
      </View>
    );
  }
  return (
    <Card testID={props.testID} class={props.class}>
      <View class="min-h-7 flex-row items-start gap-3">
        <View class="min-w-0 flex-1 gap-1">
          <Heading>{props.title}</Heading>
          <If when={props.text}>
            <Text testID="chart-text" class="tabular-nums text-small text-muted-foreground">{props.text}</Text>
          </If>
        </View>
        {props.action}
      </View>
      <If when={!props.empty} else={<Empty compact class={PLOT} text={props.empty || ""} />}>
        {props.children}
        <If when={props.legend?.length}>
          <Legend items={props.legend ?? []} gutter={props.gutter} />
        </If>
      </If>
    </Card>
  );
}

/** A chart over time whose stretch (chosen on the timeline) holds nothing of
 * its kind: said inside the chart's frame, at a plot's height, so the board keeps its shape. */
function NoneInStretch() {
  return <Empty compact testID="none-in-stretch" class={`${TICK_ROOM} ${PLOT} w-full`} text={t("plots.noneInStretch")} />;
}

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
const WORST: Record<Tone, number> = { ok: 0, miss: 1, client: 2, server: 3 };

/** The lanes' cells over `domain`, cut in `bins` slices: one cell per slice
 * and lane, with how many requests it holds (`n`), the worst outcome among
 * them (`tone` — a failure is never painted over), and the request it stands
 * for (`sample`, an index into `samples`: the worst, the earliest of those).
 * Requests of a lane alternate two shades (`alt`) so two in a row read as
 * two; cells that hold several alternate by slice. In order of lane, then slice. */
export function laneCells(samples: { tMs: number; ms: number; lane: number; status?: number | null }[], domain: [number, number], bins: number, lanes: number): { bin: number; lane: number; n: number; tone: Tone; alt: boolean; sample: number }[] {
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
    const tone = statusTone(x.status ?? null);
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
export function Lanes(props: { profile: Profile; domainMs?: [number, number] | null; onRequest?: (id: number) => void; /** the moment the pointer rests on, shared by the pane's charts */ focus?: Focus }) {
  const domain = (): [number, number] => props.domainMs ?? [0, Math.max(props.profile.wallMs, 1)];
  const all = computed(() => {
    const lane = lanesOf(props.profile.samples);
    return props.profile.samples.map((x, i) => ({ ...x, lane: lane[i] }));
  });
  const lanes = () => Math.min(LANES, all().reduce((m, x) => Math.max(m, x.lane + 1), 1));
  const hidden = () => all().filter((x) => x.lane >= LANES).length;
  const width = () => (domain()[1] - domain()[0]) / LANE_BINS;
  // a cell per slice of time and lane, holding the requests that span it: drawn once, in the worst outcome of what it holds
  const rows = computed(() => laneCells(all(), domain(), LANE_BINS, LANES).map((c) => ({ ...c, x: String(c.bin), y: String(c.lane), id: all()[c.sample].id })));
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
  const label = () => t("lanes.text", { n: props.profile.peakInFlight, at: duration(props.profile.peakAtMs) });
  const bins = Array.from({ length: LANE_BINS }, (_, i) => String(i));
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
            xScale={{ type: "band", domain: bins, paddingInner: 0 }}
            yScale={{ type: "band", domain: Array.from({ length: lanes() }, (_, i) => String(i)), paddingInner: 0.3 }}
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
            <PlotAxis tickValues={ticks3(LANE_BINS)} format={(i) => duration(domain()[0] + i * width())} labelClass={TIME_TICK} />
          </Plot>
        </BaseButton>
        </If>
        <Legend items={[{ label: t("flows.ok"), swatch: "bg-bar" }, { label: t("flows.notfound"), swatch: "bg-status-miss" }, { label: t("flows.clienterror"), swatch: "bg-status-client" }, { label: t("lanes.failed"), swatch: "bg-status-server" }]}>
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

// --- one moment across the charts ------------------------------------------------

/** The moment the pointer rests on in one time chart (ms from the session's
 * start), marked in the others: the charts of a pane read as one. Each pane
 * (and each window) makes its own with `focusOf()` and hands it to its
 * charts; two sessions open at once do not mark each other's moments. */
export type Focus = { (): number | null; set: (at: number | null) => void };
export const focusOf = (): Focus => signal<number | null>(null);

/** The row of `rows` nearest the focused moment, if one is focused. */
function nearest(rows: { tMs: number }[], at: number | null): number | null {
  if (at === null || rows.length === 0) return null;
  let best = 0;
  for (let i = 1; i < rows.length; i++) if (Math.abs(rows[i].tMs - at) < Math.abs(rows[best].tMs - at)) best = i;
  return best;
}

// --- the shape of a session ---------------------------------------------------

type Slice = { tMs: number; ok: number; miss: number; client: number; server: number; inFlight: number };

/** The session's slices with the most requests in flight in each. */
export function slices(p: Profile): Slice[] {
  let k = 0;
  let last = 0;
  return p.rate.map((b) => {
    const end = b.tMs + p.binMs;
    let peak = last;
    while (k < p.inFlight.length && p.inFlight[k].tMs < end) {
      peak = Math.max(peak, p.inFlight[k].n);
      last = p.inFlight[k].n;
      k++;
    }
    return { ...b, inFlight: peak };
  });
}

/** In words: what the shape shows and what stands out. */
export function shapeSentence(p: Profile): string {
  return t("profile.shapeText", { busy: duration(p.busyMs), wall: duration(p.wallMs), pct: pct(p.busyMs / Math.max(p.wallMs, 1e-9)), n: p.peakInFlight, at: duration(p.peakAtMs) });
}

/**
 * Requests over a session: a bar per slice of time, stacked by outcome, and
 * a step line of the requests in flight. A press picks the slice under the
 * pointer (again: lets it go), for the operations running then.
 */
export function SessionShape(props: { profile: Profile; slice?: number | null; onSlice?: (i: number | null) => void; class?: string; testID?: string; annotate?: boolean; marks?: { tMs: number; n: number }[]; axis?: boolean; range?: [number, number] | null; focus?: Focus; /** the strip under a session's header: a mark, in no frame */ strip?: boolean }) {
  const hovered = signal<number | null>(null);
  // zoomed: the slices from `range[0]` to `range[1]`, wider each
  const from = () => props.range?.[0] ?? 0;
  const rows = () => {
    const all = slices(props.profile);
    return props.range ? all.slice(props.range[0], props.range[1] + 1) : all;
  };
  const top = () => Math.max(1, ...rows().map((r) => r.ok + r.miss + r.client + r.server));
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
      m.set(i, [...(m.get(i) ?? []), k.n]);
      last = i;
    }
    return m;
  };
  const focused = () => (hovered() === null ? nearest(rows(), props.focus?.() ?? null) : null);
  const peakBin = () => {
    const at = props.profile.peakAtMs;
    const i = Math.floor(at / props.profile.binMs) - from();
    return props.profile.peakInFlight > 1 && i >= 0 && i < rows().length ? i : null;
  };
  const tone = (x: Tone) => (d: Slice, i: number) => (props.slice == null || props.slice === i + from() ? x : "dim");
  const tip = () => {
    const i = hovered();
    const r = i === null ? null : rows()[i];
    if (!r) return null;
    const n = r.ok + r.miss + r.client + r.server;
    const parts = [t("sessions.requestsN", { n }), ...(r.miss ? [t("profile.missN", { n: r.miss })] : []), ...(r.client + r.server ? [t("profile.failedN", { n: r.client + r.server })] : [])];
    // the findings that begin in this slice: what the numbers over it are
    const marks = marked().get(i!) ?? [];
    return { title: `+${duration(r.tMs)} – +${duration(r.tMs + props.profile.binMs)}`, lines: [parts.join(" · "), t("profile.inFlightN", { n: r.inFlight }), ...(props.annotate && marks.length ? [t("shape.findingsHere", { list: marks.join(", ") })] : [])] };
  };
  return (
    // a handful of requests has no shape worth a chart
    <If when={!terminal() && rows().length > 1 && props.profile.samples.length >= 12} else={<Text class={terminal() ? "text-muted-foreground" : "hidden"}>{shapeSentence(props.profile)}</Text>}>
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
          xScale={{ type: "band", paddingInner: 0.18 }}
          y2Scale={{ type: "linear" }}
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
          <Bar value="server" stack="r" color={tone("server")} cornerRadii={{ topLeft: 1.5, topRight: 1.5 }} />
          <Line y="inFlight" scale="y2" curve="stepAfter" class="stroke-foreground/30 stroke-[1.25]" hoverDotClass="hidden" />
          {/* full width: when (from the session's start) and how many at once */}
          <If when={props.axis}>
            <PlotAxis tickValues={ticks3(rows().length)} format={(i) => `+${duration(rows()[i]?.tMs ?? 0)}`} labelClass={TIME_TICK} />
            <PlotAxis placement="right" scale="y2" labelPlacement="inside" ticks={2} format={(v) => (Math.round(v) > 0 ? t("profile.inFlightN", { n: Math.round(v) }) : "")} labelClass={VALUE_TICK_END} />
          </If>
          {/* the moment another chart rests on */}
          <Rule value={(_, i) => (i === focused() ? top() : (undefined as unknown as number))} class="stroke-foreground/40 stroke-1" />
          {/* where each finding begins */}
          <If when={props.annotate && marked().size > 0}>
            <Rule value={(_, i) => (marked().has(i) ? top() : (undefined as unknown as number))} class="stroke-warn/70 stroke-1" style={{ strokeDasharray: "2 2" }} />
            <PlotLabel /* emphasis: a finding's number over its slice */ class={`tabular-nums text-caption ${EMPHASIS} text-warn`} texts={[...marked()].map(([i, ns]) => ({ text: ns.join(" "), x: i, y: top(), align: "center" as const, class: "-translate-y-full pb-1" }))} />
          </If>
          {/* the busiest moment, named where it happened */}
          {/* (left out where a finding's number stands near it: the two would cross; Time says it too) */}
          <If when={props.annotate && peakBin() !== null && ![...marked().keys()].some((i) => Math.abs(i - peakBin()!) <= Math.max(3, rows().length / 12))}>
            <PlotLabel
              scale="y2"
              class="tabular-nums text-caption font-medium text-foreground"
              texts={[{ text: t("profile.peakLabel", { n: props.profile.peakInFlight }), x: peakBin()!, y: props.profile.peakInFlight, align: "center", class: "-translate-y-full pb-1" }]}
            />
          </If>
        </Plot>
      </BaseButton>
    </If>
  );
}

// --- a session in a row --------------------------------------------------------------

/** A session's requests over its time, a few points high: drawn once per
 * session, kept as it is when its row scrolls away and back. */
export function Sparkline(props: { shape: number[]; id: string; class?: string }) {
  const rows = () => props.shape.map((n, i) => ({ i, n }));
  return (
    <If when={!terminal() && props.shape.some((n) => n > 0)}>
      <PlotAppearScope id={props.id}>
        <Plot data={rows()} xScale={{ type: "band", paddingInner: 0.25 }} yScale={{ type: "linear", domain: [0, Math.max(1, ...props.shape)] }} interactive={false} accessibilityLabel="" appearGeneration={1} motion={MOTION} class={props.class ?? "h-4 w-16"}>
          <Bar value="n" class="fill-chart-1/70" cornerRadii={{ topLeft: 1, topRight: 1 }} />
        </Plot>
      </PlotAppearScope>
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

/** How long requests took over the session: the band from median to p95,
 * the p99 a line above it; slices without requests leave a gap. */
export function LatencyBand(props: { profile: Profile; class?: string; testID?: string; domainMs?: [number, number] | null; focus?: Focus }) {
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
  const top = () => Math.max(...inView().map((b) => b.p99Ms), 1) * 1.15;
  const hovered = signal<number | null>(null);
  const label = () => {
    const last = answered()[answered().length - 1];
    return last ? t("band.text", { p50: duration(last.p50Ms), p95: duration(last.p95Ms), p99: duration(last.p99Ms) }) : "";
  };
  const tip = () => {
    const b = hovered() === null ? null : rows()[hovered()!];
    return b && has(b) ? { title: `+${duration(b.tMs)}`, lines: [t("band.line", { p50: duration(b.p50Ms), p95: duration(b.p95Ms), p99: duration(b.p99Ms) })] } : null;
  };
  return (
    <If when={!terminal() && answered().length > 1} else={<Text class={terminal() ? "text-muted-foreground" : "hidden"}>{label()}</Text>}>
      <Plot
        testID={props.testID ?? "latency-band"}
        data={rows()}
        x="tMs"
        xScale={{ type: "linear", domain: props.domainMs ?? [0, Math.max(props.profile.wallMs, 1)] }}
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
        class={props.class ?? "h-24 w-full"}
      >
        {/* straight between slices: a curve would show values no request had */}
        <Area y0={gap((b) => b.p50Ms)} y1={gap((b) => b.p95Ms)} curve="linear" class="fill-chart-1/25" />
        <Line y={gap((b) => b.p50Ms)} curve="linear" class="stroke-chart-1 stroke-[1.5]" hoverDotClass="size-1.5 rounded-full bg-chart-1" />
        <Line y={gap((b) => b.p99Ms)} curve="linear" class="stroke-muted-foreground/70 stroke-[1]" style={{ strokeDasharray: "3 3" }} hoverDotClass="hidden" />
        <Rule value={(_, i) => (hovered() === null && i === nearest(rows(), props.focus?.() ?? null) ? top() : (undefined as unknown as number))} class="stroke-foreground/40 stroke-1" />
        <PlotAxis placement="left" labelPlacement="inside" ticks={3} format={(v) => duration(v)} labelClass={VALUE_TICK} />
      </Plot>
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
          yScale={{ type: "band", paddingInner: 0.28, paddingOuter: 0.12 }}
          xScale={{ type: "linear", domain: [0, Math.max(props.profile.wallMs, 1)] }}
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
          <Bar base="startMs" value="endMs" class="fill-muted-foreground/45" cornerRadii={3} inactiveOpacity={INACTIVE} label={(_, i) => duration(rows()[i].endMs - rows()[i].startMs)} labelClass={BAR_LABEL} />
          {/* the same requests eight at once */}
          <Bar base="startMs" value="soon" class="fill-chart-1" cornerRadii={3} inactiveOpacity={INACTIVE} label={(_, i) => t("chains.bar", { fast: duration(rows()[i].atOnceMs) })} labelClass={BAR_LABEL} />
          <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.name ?? "")} labelClass={AXIS_LABEL} />
          <PlotAxis placement="bottom" ticks={4} format={(v) => duration(v)} labelClass={TIME_TICK} />
        </Plot>
        <Legend gutter items={[{ label: t("chains.asRan"), swatch: "bg-muted-foreground/45" }, { label: t("chains.atOnce"), swatch: "bg-chart-1" }]} />
      </View>
    </If>
  );
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
          yScale={{ type: "band", paddingInner: 0.3, paddingOuter: 0.12 }}
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
          <Rule base="a95" value="b95" class="stroke-chart-1/60 stroke-[2.5]" inactiveOpacity={INACTIVE} />
          <Dot value="a95" r={4} class="fill-card stroke-muted-foreground stroke-[1.5]" inactiveOpacity={INACTIVE} />
          <Dot value="b95" r={4.5} class="fill-chart-1 stroke-card stroke-[1.5]" inactiveOpacity={INACTIVE} />
          <PlotLabel class="tabular-nums text-caption text-foreground" texts={rows().map((r, i) => ({ text: `${duration(r.a95)} → ${duration(r.b95)} · ${factor(ratio(r))}×`, x: Math.max(r.a95, r.b95), y: i, align: "left" as const, class: "pl-2" }))} />
          <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.name ?? "")} labelClass={AXIS_LABEL} />
        </Plot>
        <Legend gutter items={[{ label: `p95 · ${props.names[0]}`, swatch: "bg-muted-foreground/45" }, { label: `p95 · ${props.names[1]}`, swatch: "bg-chart-1" }]} />
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
    return g.code ? { title: g.name, lines: [spent, cost(g.cost, props.profile.cost.currency)] } : { title: g.name, lines: [spent, t("profile.latencyLine", { p50: duration(g.p50Ms), p95: duration(g.p95Ms), max: duration(g.maxMs) }), `↑${bytes(g.bytesUp)} ↓${bytes(g.bytesDown)} · ${cost(g.cost, props.profile.cost.currency)}`] };
  };
  return (
    <If
      when={!textual()}
      else={<SimpleTable columns={[t("profile.kind"), t("profile.share"), t("profile.time"), t("figure.requests")]} rows={rows().map((g) => [g.name, pct(g.share), duration(g.waitedMs), count(g.requests)])} />}
    >
      <Plot
        testID={props.by === "code" ? "time-spent-code" : "time-spent"}
        data={rows()}
        yScale={{ type: "band", paddingInner: 0.32, paddingOuter: 0.16 }}
        xScale={{ type: "linear", domain: [0, Math.max(...rows().map((g) => g.waitedMs), 1) * 1.18] }}
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
          cornerRadii={{ topRight: 3, bottomRight: 3 }}
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

// --- an operation's requests ------------------------------------------------------

const statusTone = (s: number | null): Tone => (s === null || s >= 500 ? "server" : s === 404 ? "miss" : s >= 400 ? "client" : "ok");

/** An operation's requests in time: a bar from each one's start to its end, so waits and overlaps show. */
/** A key as a row's name: its last two parts (`…/manifest/00012.json`). */
const short = (key: string) => {
  const parts = key.split("/").filter(Boolean);
  return parts.length > 2 ? `…/${parts.slice(-2).join("/")}` : key;
};

/** How many requests the waterfall draws, a row each at the one pitch. */
const WATERFALL_ROWS = 12;

export function Waterfall(props: { records: RecordSummary[]; onRecord?: (id: number) => void }) {
  // a loop, not a spread: an operation may hold more requests than a call takes arguments
  const t0 = () => props.records.reduce((m, r) => Math.min(m, new Date(r.time).getTime()), Infinity);
  const rows = () =>
    // the first few, in the order the list below has them: a bar is its row
    props.records.slice(0, WATERFALL_ROWS).map((r) => {
      const s = new Date(r.time).getTime() - t0();
      // the wait for the first byte, then the body: one bar in two shades
      const first = r.firstByteMs != null && r.firstByteMs < r.latencyMs ? r.firstByteMs : null;
      return { id: r.id, op: r.operation, target: r.target, start: s, firstEnd: s + (first ?? Math.max(r.latencyMs, 0.3)), end: s + Math.max(r.latencyMs, 0.3), tone: statusTone(r.status), status: r.status, ms: r.latencyMs, first };
    });
  const hovered = signal<number | null>(null);
  const end = () => Math.max(...rows().map((r) => r.end), 1);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: `${r.status ?? "—"} ${r.op}`, lines: [r.target, `+${duration(r.start)} · ${duration(r.ms)}`, ...(r.first != null ? [t("waterfall.firstByte", { ms: duration(r.first) })] : [])] } : null;
  };
  const anyFirst = () => rows().some((r) => r.first != null);
  return (
    <If when={!terminal() && props.records.length > 1}>
      <BaseButton
        testID="waterfall"
        accessibilityLabel={t("profile.waterfallText", { n: props.records.length, time: duration(end()) })}
        disabled={!props.onRecord}
        onPress={() => {
          const r = hovered() === null ? null : rows()[hovered()!];
          if (r) props.onRecord?.(r.id);
        }}
      >
        <Plot
          data={rows()}
          yScale={{ type: "band", paddingInner: 0.35 }}
          xScale={{ type: "linear", domain: [0, end()] }}
          colorScale={tones()}
          accessibilityLabel={t("profile.waterfallText", { n: props.records.length, time: duration(end()) })}
          accessibilityValue={tipValue(tip())}
          appearGeneration={props.records[0]?.id}
          motion={MOTION}
          crossLineClass={CROSS_BAND}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => hovered.set(index)}
          class={`${TICK_ROOM} ${GUTTER}`}
          style={{ height: rowsHeight(rows().length) }}
        >
          {/* the body's part a lighter shade of the bar, the wait for its first byte the full one */}
          <Bar base="start" value="end" color="tone" cornerRadii={2} inactiveOpacity={INACTIVE} stagger={0.4} style={anyFirst() ? { opacity: 0.45 } : undefined} />
          <If when={anyFirst()}>
            <Bar base="start" value="firstEnd" color="tone" cornerRadii={2} inactiveOpacity={INACTIVE} stagger={0.4} />
          </If>
          <PlotAxis placement="left" format={(i) => tail(short(rows()[i]?.target ?? ""), GUTTER_CHARS - 2)} labelClass={`${AXIS_LABEL} font-mono text-caption`} />
          <PlotAxis ticks={4} format={(v) => (v === 0 ? "0" : duration(v))} labelClass={TIME_TICK} />
        </Plot>
        <If when={anyFirst() || props.records.length > WATERFALL_ROWS}>
          <Legend gutter items={anyFirst() ? [{ label: t("waterfall.firstByteKey"), swatch: "bg-bar" }, { label: t("waterfall.bodyKey"), swatch: "bg-bar/45" }] : []}>
            <If when={props.records.length > WATERFALL_ROWS}>
              <ChartNote>{t("waterfall.more", { n: props.records.length - WATERFALL_ROWS })}</ChartNote>
            </If>
          </Legend>
        </If>
      </BaseButton>
    </If>
  );
}



// --- two runs ---------------------------------------------------------------------

/** Two runs of a program by kind of request: the requests before (muted) and after (in color). */
export function RunBars(props: { change: RunChange; max?: number }) {
  const rows = () =>
    props.change.groups
      .filter((g) => g.requests[0] !== g.requests[1] || Math.abs(g.busyMs[0] - g.busyMs[1]) >= 1)
      .slice(0, props.max ?? 8)
      .map((g) => ({ name: groupName(g), before: g.requests[0], after: g.requests[1], tone: g.requests[1] < g.requests[0] ? "fewer" : g.requests[1] > g.requests[0] ? "more" : "same" }));
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
          yScale={{ type: "band", paddingInner: 0.3, paddingOuter: 0.15, groupPadding: 0.12 }}
          // room for the labels at the bars' ends
          xScale={{ type: "linear", domain: [0, Math.max(1, ...rows().map((r) => Math.max(r.before, r.after))) * 1.35] }}
          colorScale={{ type: "ordinal", domain: ["fewer", "more", "same"], range: [token("status-ok" as never) as unknown as string, token("status-server" as never) as unknown as string, token("muted-foreground" as never) as unknown as string] }}
          accessibilityLabel={rows().map((r) => `${r.name} ${r.before} → ${r.after}`).join(", ")}
          accessibilityValue={tipValue(tip())}
          motion={MOTION}
          appearGeneration={props.change.requests.join("-")}
          crossLineClass={CROSS_BAND}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => hovered.set(index)}
          class={GUTTER}
          style={{ height: rowsHeight(rows().length) }}
        >
          {/* a dumbbell: before (hollow) joined to after (in color); a drop to none still has its dot */}
          <Rule base="before" value="after" color="tone" class="stroke-[2.5]" inactiveOpacity={INACTIVE} />
          <Dot value="before" r={4} class="fill-card stroke-muted-foreground stroke-[1.5]" inactiveOpacity={INACTIVE} />
          <Dot value="after" color="tone" r={4.5} class="stroke-card stroke-[1.5]" inactiveOpacity={INACTIVE} />
          <PlotLabel class="tabular-nums text-caption text-foreground" texts={rows().map((r, i) => ({ text: `${count(r.before)} → ${count(r.after)}`, x: Math.max(r.before, r.after), y: i, align: "left" as const, class: "pl-2" }))} />
          <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.name ?? "")} labelClass={AXIS_LABEL} />
        </Plot>
      </If>
    </If>
  );
}

// --- what a session comes to, in a sentence ------------------------------------------

/** Whether a session's requests have times (a CloudTrail file's have none). */
export const timed = (p: Profile | null | undefined) => p?.timed !== false;

/** A session in a sentence: where its time went and what the first finding would save. */
export function sessionStory(p: Profile, findings: Observation[]): string {
  const top = p.groups[0];
  const busiest = [...p.groups].sort((a, b) => b.requests - a.requests)[0];
  const total = p.groups.reduce((n, g) => n + g.requests, 0) || 1;
  const parts = !timed(p)
    ? busiest
      ? [t("story.untimed", { what: groupName(busiest), pct: pct(busiest.requests / total) })]
      : []
    : top
      ? [t("story.time", { busy: duration(p.busyMs), wall: duration(p.wallMs), what: groupName(top), pct: pct(top.share) })]
      : [];
  const first = findings.find((o) => o.saving) ?? findings[0];
  if (first) {
    const saving = observationSaving(first);
    parts.push(saving ? t("story.saving", { saving }) : t("story.finding", { finding: observationText(first) }));
  }
  return parts.join(" ");
}

// --- prefixes, named ---------------------------------------------------------------

/** A path too long for its place, cut at its start on a folder's edge: the
 * end is what tells two prefixes apart (`…/compactions/`, `…/compacted/`). */
export function tail(path: string, max: number): string {
  if (path.length <= max) return path;
  const keep = path.slice(-(max - 1));
  const edge = keep.indexOf("/");
  // from a folder's edge where one is near the cut, else mid-name
  return `…${edge >= 0 && edge < keep.length - 1 && edge <= 8 ? keep.slice(edge) : keep}`;
}

/** How a chart names a set of prefixes: what they all start with is said
 * once (`common`), and each is named by the rest — whole where it fits, cut
 * at its start where it does not, with `…` after one that stands for the
 * folders under it. No two get the same name. */
export function prefixNames(prefixes: { prefix: string; folders?: number }[], max = 20): { common: string; name: (prefix: string) => string } {
  const real = prefixes.filter((x) => x.prefix !== "other" && x.prefix !== "/");
  // what they all start with, as written (`a//b/` and `/a/b/` keep their slashes), back to a folder's edge
  let common = "";
  if (real.length > 1) {
    common = real[0].prefix;
    for (const x of real) while (!x.prefix.startsWith(common)) common = common.slice(0, -1);
    common = common.slice(0, common.lastIndexOf("/") + 1);
    // each keeps a name of its own after it
    const bare = (rest: string) => rest.replace(/\/+/g, "") === "";
    while (common && real.some((x) => bare(x.prefix.slice(common.length)))) common = common.slice(0, common.replace(/\/+$/, "").lastIndexOf("/") + 1);
    // slashes alone are nothing shared
    if (bare(common)) common = "";
  }
  const names = new Map<string, string>();
  const taken = new Set<string>();
  for (const x of real) {
    const own = `${x.prefix.slice(common.length)}${x.folders ? "…" : ""}`;
    let shown = tail(own, max);
    // two cut to the same name: the whole of it rather than a name that lies
    if (taken.has(shown)) shown = own;
    taken.add(shown);
    names.set(x.prefix, shown);
  }
  // the rest of them, in words: never the raw "other"
  return { common, name: (prefix) => names.get(prefix) ?? (prefix === "other" ? t("flows.otherPrefixes") : prefix) };
}

/** Under a chart of prefixes: what they all start with, and that `…` marks one shown with the folders under it. */
function PrefixNote(props: { prefixes: { prefix: string; folders?: number }[]; common: string; gutter?: boolean }) {
  const folded = () => props.prefixes.some((x) => (x.folders ?? 0) > 0);
  return (
    <If when={props.common || folded()}>
      <ChartNote testID="prefix-note" gutter={props.gutter}>
        {[props.common ? t("prefix.under", { path: props.common }) : "", folded() ? t("prefix.foldedNote") : ""].filter(Boolean).join(" · ")}
      </ChartNote>
    </If>
  );
}

/** A prefix in full, for a tooltip: its whole path, and the folders it stands for. */
const prefixTitle = (x: { prefix: string; folders?: number }) => (x.folders ? `${x.prefix} ${t("prefix.folders", { n: x.folders })}` : x.prefix);

// --- where requests went -----------------------------------------------------------

const OUTCOME_FILL: Record<string, string> = { ok: "fill-bar", "not found": "fill-status-miss", "client error": "fill-status-client", "server error": "fill-status-server" };
const OUTCOME_FROM: Record<string, string> = { ok: "to-bar/45", "not found": "to-status-miss/45", "client error": "to-status-client/45", "server error": "to-status-server/45" };

/**
 * Where a session's requests went: from each kind of operation to the
 * prefixes it touched, and from each prefix to how it was answered. The
 * widths are requests; the pointer over a node keeps its flows and fades the rest.
 */
export function Flows(props: { profile: Profile }) {
  const nodes = () => {
    const names: string[] = [];
    for (const f of props.profile.flows) for (const n of [f.source, f.target]) if (!names.includes(n)) names.push(n);
    return names.map((name) => ({ name, stage: OUTCOME_FILL[name] ? 2 : props.profile.groups.some((g) => g.operation === name) || name === "other operations" ? 0 : 1 }));
  };
  const links = () => {
    const at = new Map(nodes().map((n, i) => [n.name, i]));
    return props.profile.flows.map((f) => ({ source: at.get(f.source)!, target: at.get(f.target)!, value: f.value }));
  };
  // prefixes by what tells them apart; what they share is said once under the chart
  const named = () => prefixNames(props.profile.prefixes, GUTTER_CHARS - 6);
  const short = (n: string) => named().name(n);
  const name = (n: { name: string }) => (n.name === "other operations" ? t("flows.otherOps") : n.name === "other prefixes" ? t("flows.otherPrefixes") : OUTCOME_FILL[n.name] ? t(`flows.${n.name.replace(" ", "")}` as never) : short(n.name));
  const fill = (n: { name: string; stage: number }) => (n.stage === 2 ? OUTCOME_FILL[n.name] : n.stage === 0 ? "fill-chart-1" : "fill-chart-4");
  const from = (n: { stage: number }) => (n.stage === 0 ? "from-chart-1/35" : "from-chart-4/35");
  const to = (n: { name: string; stage: number }) => (n.stage === 2 ? OUTCOME_FROM[n.name] : "to-chart-4/35");
  // a node's requests: what came in, or (the first column) what went out
  const through = (n: { name: string }) => {
    const sum = (side: "source" | "target") => props.profile.flows.reduce((s, f) => (f[side] === n.name ? s + f.value : s), 0);
    return Math.max(sum("source"), sum("target"));
  };
  const hovered = signal<number | null>(null);
  // the node under the pointer: its name whole, what went through it and its share
  const tip = () => {
    const n = hovered() === null ? null : nodes()[hovered()!];
    if (!n) return null;
    const all = props.profile.flows.filter((f) => OUTCOME_FILL[f.target]).reduce((s, f) => s + f.value, 0);
    const whole = n.stage === 1 && n.name !== "other prefixes" ? prefixTitle(props.profile.prefixes.find((x) => x.prefix === n.name) ?? { prefix: n.name }) : name(n);
    return { title: whole, lines: [`${t("sessions.requestsN", { n: through(n) })} · ${pct(through(n) / Math.max(all, 1))}`] };
  };
  const label = () => {
    const ok = props.profile.flows.filter((f) => f.target === "ok").reduce((s, f) => s + f.value, 0);
    const all = props.profile.flows.filter((f) => OUTCOME_FILL[f.target]).reduce((s, f) => s + f.value, 0);
    const top = props.profile.prefixes[0];
    return t("flows.text", { n: all, ok: pct(ok / Math.max(all, 1)), prefix: top?.prefix ?? "", share: pct((top?.requests ?? 0) / Math.max(all, 1)) });
  };
  return (
    <If when={!terminal() && props.profile.flows.length > 0} else={<Text class={terminal() ? "text-muted-foreground" : "hidden"}>{label()}</Text>}>
      <View class="gap-1">
      <Plot
        testID="flows"
        data={nodes()}
        accessibilityLabel={label()}
        accessibilityValue={tipValue(tip())}
        tooltipPlacement="pointer"
        tooltipOffset={10}
        tooltipContent={<ChartTip tip={tip()} />}
        onHoverChange={({ index }) => hovered.set(index)}
        appearGeneration={1}
        motion={{ ...MOTION, appear: { duration: 700, easing: "easeOut" } }}
        // not rows: names stand at both edges, in the one gutter either side
        class={`${GUTTER_BOTH} mt-2 ${FLOWS}`}
      >
        <Sankey
          links={links()}
          nodeWidth={7}
          nodePadding={26}
          nodeCornerRadius={2}
          valueScale="linear"
          minLinkWidth={1.5}
          nodeClass={(n) => fill(n)}
          linkBackgroundClass={(l) => `bg-linear-to-r ${from(nodes()[l.source])} ${to(nodes()[l.target])}`}
          hoverDim={1 - INACTIVE}
          // one line, name then count: two lines over a middle node reached the bar of the node above it
          nodeLabel={(n) => `${axisName(name(n))} · ${count(through(n))}`}
          nodeLabelClass="max-w-44 px-2 py-1 tabular-nums text-small text-foreground"
        />
      </Plot>
      <PrefixNote prefixes={props.profile.prefixes} common={named().common} gutter />
      </View>
    </If>
  );
}

// --- when, and where ---------------------------------------------------------------

/** Requests by prefix and slice of the session: when each part of the key space was busy. */
export function Heat(props: { profile: Profile; range?: [number, number] | null }) {
  const p = () => props.profile;
  // a stretch chosen on the timeline: its slices only, as every chart over time
  const from = () => props.range?.[0] ?? 0;
  const to = () => props.range?.[1] ?? p().rate.length - 1;
  const bins = () => p().rate.map((_, i) => String(i)).slice(from(), to() + 1);
  // a row's name keeps its end (what tells two prefixes apart); the tooltip has it whole
  const named = () => prefixNames(p().prefixes, GUTTER_CHARS);
  const names = () => p().prefixes.map((x) => (x.prefix === "other" ? t("flows.otherPrefixes") : named().name(x.prefix)));
  const whole = (i: number) => {
    const x = p().prefixes[i];
    return !x ? "" : x.prefix === "other" ? t("flows.otherPrefixes") : prefixTitle(x);
  };
  const rows = () => p().heat.filter((c) => c.bin >= from() && c.bin <= to()).map((c) => ({ x: String(c.bin), y: names()[c.prefix] ?? "", n: c.n, bin: c.bin, prefix: c.prefix }));
  const hovered = signal<number | null>(null);
  // of the whole session, so a stretch keeps the colors it had
  const max = () => p().heat.reduce((m, r) => Math.max(m, r.n), 1);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: whole(r.prefix), lines: [`+${duration(r.bin * p().binMs)} – +${duration((r.bin + 1) * p().binMs)}`, t("sessions.requestsN", { n: r.n })] } : null;
  };
  const last = () => Math.max(0, bins().length - 1);
  const label = () => {
    const top = p().prefixes[0];
    return top ? t("heat.text", { prefix: top.prefix, n: top.peakPerSecond }) : "";
  };
  return (
    <If
      when={!terminal() && p().heat.length > 0 && bins().length > 1}
      // a terminal: each prefix's requests and its busiest second, as figures
      else={<If when={p().prefixes.length > 0}><SimpleTable columns={[t("common.prefix"), t("heat.requests"), t("heat.peak"), t("heat.errors")]} rows={p().prefixes.map((x, i) => [whole(i), count(x.requests), count(x.peakPerSecond), count(x.errors)])} /></If>}
    >
      <View class="gap-1">
      <If when={rows().length > 0} else={<NoneInStretch />}>
      <Plot
        testID="heat"
        data={rows()}
        xScale={{ type: "band", domain: bins(), paddingInner: 0.12 }}
        yScale={{ type: "band", domain: names(), paddingInner: 0.18 }}
        // by the square root: one hot cell does not wash out the rest
        colorScale={{ type: "sequential", domain: [0, Math.sqrt(max())], range: [token("bar-track" as never) as unknown as string, token("chart-1" as never) as unknown as string] }}
        accessibilityLabel={label()}
        accessibilityValue={tipValue(tip())}
        appearGeneration={1}
        motion={MOTION}
        tooltipPlacement="pointer"
        tooltipOffset={10}
        tooltipContent={<ChartTip tip={tip()} />}
        onHoverChange={({ index }) => hovered.set(index)}
        class={`${TICK_ROOM} ${GUTTER}`}
        style={{ height: rowsHeight(names().length) }}
      >
        <Cell x="x" y="y" value={(_, i) => Math.sqrt(rows()[i]?.n ?? 0)} cornerRadii={2} inactiveOpacity={INACTIVE} stagger={0.2} hoverCellClass={HOVER} />
        <PlotAxis placement="left" labelClass={AXIS_LABEL} />
        <PlotAxis tickValues={ticks3(last() + 1)} format={(i) => `+${duration((from() + i) * p().binMs)}`} labelClass={TIME_TICK} />
      </Plot>
      </If>
      <PrefixNote prefixes={p().prefixes} common={named().common} gutter />
      </View>
    </If>
  );
}

// --- what the store pushed back -------------------------------------------------

/** Over the session: requests the store asked to slow down (503 SlowDown,
 * 429), others that failed, and those the SDK sent again; a thin lane on the
 * timeline's slices. */
export function Pressure(props: { profile: Profile; range?: [number, number] | null }) {
  const all = () =>
    props.profile.rate.map((b) => {
      const throttled = b.throttled ?? 0;
      return { tMs: b.tMs, throttled, failed: Math.max(0, b.client + b.server - throttled), retried: b.retried ?? 0 };
    });
  const rows = () => (props.range ? all().slice(props.range[0], props.range[1] + 1) : all());
  // the legend counts what the chart shows: a zoom's slices, or all of them
  const totals = () => rows().reduce((s, r) => ({ throttled: s.throttled + r.throttled, failed: s.failed + r.failed, retried: s.retried + r.retried }), { throttled: 0, failed: 0, retried: 0 });
  const label = () => t("pressure.text", { throttled: totals().throttled, failed: totals().failed, retried: totals().retried });
  const hovered = signal<number | null>(null);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: `+${duration(r.tMs)}`, lines: [t("pressure.line", { throttled: r.throttled, failed: r.failed, retried: r.retried })] } : null;
  };
  const any = () => all().some((r) => r.throttled + r.failed + r.retried > 0);
  return (
    <If when={any()} else={<Empty compact testID="pressure-none" text={t("pressure.none")} />}>
      <If when={!terminal()} else={<Text class="text-foreground">{label()}</Text>}>
        <View class="gap-2">
          <Plot
            testID="pressure"
            data={rows()}
            xScale={{ type: "band", paddingInner: 0.18 }}
            accessibilityLabel={label()}
            accessibilityValue={tipValue(tip())}
            motion={MOTION}
            crossLineClass={CROSS_BAND}
            tooltipPlacement="pointer"
            tooltipOffset={10}
            tooltipContent={<ChartTip tip={tip()} />}
            onHoverChange={({ index }) => hovered.set(index)}
            // a thin lane under the timeline's slices, not rows
            class="h-14 w-full"
          >
            <Bar value="throttled" stack="p" class="fill-warn" />
            <Bar value="failed" stack="p" class="fill-status-server" />
            <Bar value="retried" stack="p" class="fill-info" cornerRadii={{ topLeft: 1.5, topRight: 1.5 }} />
          </Plot>
          <Legend items={[{ label: t("pressure.throttled", { n: totals().throttled }), swatch: "bg-warn" }, { label: t("pressure.failed", { n: totals().failed }), swatch: "bg-status-server" }, { label: t("pressure.retried", { n: totals().retried }), swatch: "bg-info" }]} />
        </View>
      </If>
    </If>
  );
}

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
  return (
    <If when={buckets().length > 1} else={<Text class="text-small text-muted-foreground">{label()}</Text>}>
      <View class="gap-2">
        <Text testID="dist-figures" class="tabular-nums text-small text-foreground">{label()}</Text>
        <If when={!textual()} else={<SimpleTable columns={[t("dist.took"), t("figure.requests"), "%"]} rows={rows().map((r) => [r.name, count(r.n), percent(r.cum / 100)])} />}>
          <Plot
            testID="distribution"
            data={rows()}
            xScale={{ type: "band", paddingInner: 0.22, maxBandWidth: 64 }}
            yScale={{ type: "linear", domain: [0, Math.max(1, ...rows().map((r) => r.n)) * 1.18] }}
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
            <Bar value="n" color={(_, i) => (first95() >= 0 && i >= first95() ? "tail" : "body")} cornerRadii={{ topLeft: 2, topRight: 2 }} inactiveOpacity={INACTIVE} stagger={0.2} label={(_, i) => percent((rows()[i]?.cum ?? 0) / 100)} labelClass="pb-1 tabular-nums text-caption text-muted-foreground" />
            <PlotAxis format={(i) => rows()[i]?.name ?? ""} labelClass={TIME_TICK} />
          </Plot>
          <Legend items={[{ label: t("dist.requests"), swatch: "bg-chart-1" }, { label: t("dist.tail"), swatch: "bg-chart-tail" }]} />
        </If>
      </View>
    </If>
  );
}

/** A map of areas' words, the same in every one: a group's name, an area's, its figure. */
// emphasis: a group's name over the areas in it
export const GROUP_LABEL = `px-2 text-caption ${EMPHASIS} text-foreground`;
export const AREA_LABEL = "p-2 font-mono text-caption text-foreground";
export const AREA_VALUE = "font-sans tabular-nums text-caption text-muted-foreground";

/** S3's documented rates per prefix: reads and writes a second. */
const READS_PER_S = 5500;
const WRITES_PER_S = 3500;


/** Where a session's requests went by prefix, one map for three questions:
 * how many, what they cost, and how near the busiest second came to S3's
 * per-prefix limit (5,500 reads, 3,500 writes a second). A rectangle per
 * prefix under its bucket, its area the measure chosen; a prefix past half
 * its limit in the warning color. */
export type PrefixMeasureKind = "requests" | "cost" | "peak";

/** What the prefix map's areas stand for: requests, cost, or the busiest second. */
export function PrefixMeasure(props: { profile: Profile; value: PrefixMeasureKind; onChange: (m: PrefixMeasureKind) => void }) {
  const hasCost = () => props.profile.prefixes.some((x) => (x.cost ?? 0) > 0);
  return (
    <If when={!textual()}>
      <Options
        testID="prefix-measure"
        value={props.value}
        items={[{ value: "requests", title: t("prefixUse.byRequests") }, ...(hasCost() ? [{ value: "cost", title: t("prefixUse.byCost") }] : []), { value: "peak", title: t("prefixUse.byPeak") }]}
        onChange={(v) => props.onChange(v as PrefixMeasureKind)}
      />
    </If>
  );
}

export function PrefixUse(props: { profile: Profile; measure?: PrefixMeasureKind }) {
  const measure = () => props.measure ?? "requests";
  const items = () => props.profile.prefixes.filter((x) => x.prefix !== "other");
  const share = (x: Profile["prefixes"][number]) => Math.max(x.peakReads / READS_PER_S, x.peakWrites / WRITES_PER_S);
  const value = (x: Profile["prefixes"][number]) => (measure() === "cost" ? (x.cost ?? 0) : measure() === "peak" ? x.peakPerSecond : x.requests);
  const shown = (v: number) => (measure() === "cost" ? cost(v, props.profile.cost.currency) : measure() === "peak" ? t("prefixUse.perSecond", { n: Math.round(v) }) : t("prefixUse.requests", { n: v }));
  const bucketOf = (p: string) => p.split("/")[0] || p;
  const buckets = () => [...new Set(items().map((x) => bucketOf(x.prefix)))];
  const nested = () => buckets().length > 1;
  // under several buckets each is named within its bucket's box; under one, by what tells them apart
  const named = () => (nested() ? { common: "", name: (p: string) => tail(`${p.slice(bucketOf(p).length + 1) || p}${items().find((x) => x.prefix === p)?.folders ? "…" : ""}`, 28) } : prefixNames(items(), 28));
  const rows = () => {
    const leaves = items()
      .filter((x) => value(x) > 0)
      .map((x) => ({ id: x.prefix, parent: nested() ? `b:${bucketOf(x.prefix)}` : "", label: named().name(x.prefix), value: value(x), hot: share(x) >= 0.5, x }));
    const groups = nested() ? buckets().map((b) => ({ id: `b:${b}`, parent: "", label: `${b}/`, value: 0, hot: false, x: null })) : [];
    return [...groups, ...leaves];
  };
  const total = () => rows().reduce((n, r) => n + r.value, 0);
  const hovered = signal<number | null>(null);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    if (!r?.x) return null;
    const x = r.x;
    return {
      title: prefixTitle(x),
      lines: [
        t("prefixUse.requests", { n: x.requests }) + (x.errors ? ` · ${t("timeline.errors", { n: x.errors })}` : ""),
        t("limits.line", { reads: x.peakReads, rpct: Math.round((x.peakReads / READS_PER_S) * 100), writes: x.peakWrites, wpct: Math.round((x.peakWrites / WRITES_PER_S) * 100) }),
        ...(x.cost ? [cost(x.cost, props.profile.cost.currency)] : []),
      ],
    };
  };
  const top = () => [...items()].sort((a, b) => value(b) - value(a))[0];
  const label = () => (top() ? t("prefixUse.text", { prefix: top()!.prefix, value: shown(value(top()!)) }) : "");
  return (
    <If when={items().length > 0}>
      <If
        when={!textual()}
        else={<SimpleTable columns={[t("common.prefix"), t("figure.requests"), t("limits.reads"), t("limits.writes"), t("profile.cost")]} rows={items().map((x) => [x.prefix, count(x.requests), t("prefixUse.perSecond", { n: count(x.peakReads) }), t("prefixUse.perSecond", { n: count(x.peakWrites) }), cost(x.cost ?? 0, props.profile.cost.currency)])} />}
      >
        <View class="gap-2">
          <Plot
            testID="prefix-use"
            data={rows()}
            accessibilityLabel={label()}
            accessibilityValue={tipValue(tip())}
            appearGeneration={total()}
            motion={MOTION}
            tooltipPlacement="pointer"
            tooltipOffset={10}
            tooltipContent={<ChartTip tip={tip()} />}
            onHoverChange={({ index }) => hovered.set(index)}
            class={TREEMAP}
          >
            <Treemap
              id="id"
              parent="parent"
              value="value"
              paddingInner={2}
              paddingTop={nested() ? 18 : 0}
              cornerRadii={4}
              leafClass={(_, i) => (rows()[i]?.hot ? "fill-warn/60" : "fill-bar/55")}
              groupClass="fill-muted"
              groupLabel="label"
              // emphasis: a bucket's name over the prefixes in it
              groupLabelClass={GROUP_LABEL}
              inactiveOpacity={INACTIVE}
              label="label"
              valueLabel={(_, v) => shown(v)}
              labelClass={AREA_LABEL}
              valueLabelClass={AREA_VALUE}
              hoverClass={HOVER}
            />
          </Plot>
          <If when={items().some((x) => share(x) >= 0.5)}>
            <Legend items={[{ label: t("prefixUse.hot"), swatch: "bg-warn/60" }]} />
          </If>
          <PrefixNote prefixes={items()} common={named().common} />
        </View>
      </If>
    </If>
  );
}

/** Two runs by prefix: a rectangle per prefix, its area the later run's
 * requests (or the earlier's, for one it left), its color how it changed —
 * fewer cool, more warm, the same neutral — on one scale either side of none. */
export function PrefixChangeMap(props: { prefixes: { prefix: string; requests: [number, number] }[] }) {
  // a change as a share of the larger run, from −1 (all gone) to +1 (all new)
  const change = (a: number, b: number) => (a === b ? 0 : (b - a) / Math.max(a, b));
  const named = () => prefixNames(props.prefixes, 28);
  const rows = () =>
    props.prefixes
      .filter((x) => x.requests[0] + x.requests[1] > 0)
      .map((x) => ({ id: x.prefix, parent: "", label: named().name(x.prefix), value: Math.max(x.requests[1], x.requests[0] > 0 && x.requests[1] === 0 ? x.requests[0] : 0), change: change(x.requests[0], x.requests[1]), x }));
  const hovered = signal<number | null>(null);
  const line = (x: { requests: [number, number] }) => t("prefixChange.line", { a: x.requests[0], b: x.requests[1] });
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
            colorScale={{ type: "diverging", domain: [-1, 1], pivot: 0, range: [token("status-ok"), token("muted"), token("status-server")] }}
            accessibilityLabel={label()}
            accessibilityValue={tipValue(tip())}
            appearGeneration={rows().length}
            motion={MOTION}
            tooltipPlacement="pointer"
            tooltipOffset={10}
            tooltipContent={<ChartTip tip={tip()} />}
            onHoverChange={({ index }) => hovered.set(index)}
            class={TREEMAP}
          >
            <Treemap
              id="id"
              parent="parent"
              value="value"
              colorValue="change"
              paddingInner={2}
              cornerRadii={4}
              inactiveOpacity={INACTIVE}
              label="label"
              valueLabel={(_, __, i) => line(rows()[i]!.x)}
              labelClass={AREA_LABEL}
              valueLabelClass={AREA_VALUE}
              hoverClass={HOVER}
            />
          </Plot>
          <Legend items={[{ label: t("prefixChange.fewer"), swatch: "bg-status-ok" }, { label: t("prefixChange.same"), swatch: "bg-muted" }, { label: t("prefixChange.more"), swatch: "bg-status-server" }]} />
        </View>
      </If>
    </If>
  );
}

// --- how long, and how spread ----------------------------------------------------------


// --- a program over its runs ------------------------------------------------------------

type Measure = "requests" | "busyMs" | "cost" | "warnings";

/** One figure of a program over its runs: a line through them, the baseline dashed, the latest run labelled. */
export function RunTrend(props: { history: ProgramHistory; measure: Measure; title: string; format: (v: number) => string; onOpen?: (path: string) => void }) {
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
    return props.measure === "requests" ? l.requests : props.measure === "busyMs" ? l.waitingMs : props.measure === "cost" ? l.cost : null;
  };
  const hi = () => Math.max(...values(), base() ?? 0, limit() ?? 0, 1e-9) * 1.25;
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
    <Chart testID={`trend-${props.measure}`} class="min-w-[220px] flex-1" title={props.title} action={<Figure value={props.format(shown()[props.measure] as number)} label={day(shown().created)} />}>
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
          class="mr-1 h-16"
        >
          <If when={base() !== null}>
            {/* the baseline, and up to what sonde check allows */}
            <If when={limit() !== null}>
              <Area y0={() => base()!} y1={() => limit()!} curve="linear" class="fill-muted-foreground/12" />
            </If>
            <Grid y={[base()!]} class="stroke-muted-foreground/60" style={{ strokeDasharray: "3 3" }} />
          </If>
          <Line curve="linear" class="stroke-chart-1 stroke-2" dot dotClass="size-1.5 rounded-full bg-chart-1" hoverDotClass="size-2.5 rounded-full border-2 border-card bg-chart-1" />
          <PlotLabel class="tabular-nums text-caption text-muted-foreground" texts={last() > 0 ? [{ text: props.format(values()[last()]), x: last(), y: values()[last()], align: "right", class: "-translate-y-full pb-1" }] : []} />
        </Plot>
      </BaseButton>
    </Chart>
  );
}

// --- every request: when, how long, how large -----------------------------------

const log10 = (v: number) => Math.log10(Math.max(v, 1e-3));
/** Powers of ten across `[lo, hi]` (as logs): the ticks of a log scale. */
const decades = (lo: number, hi: number) => {
  const out: number[] = [];
  for (let d = Math.floor(lo); d <= Math.ceil(hi); d++) if (d >= lo - 1e-9 && d <= hi + 1e-9) out.push(d);
  return out.length >= 2 ? out : [lo, hi];
};

/** Each request as a dot: when it started (or how large it was) across, how
 * long it took up, on a log scale, in its outcome's color; the p95 dashed.
 * A press opens the request. The shape a summary hides: a slow tail, a
 * cluster of 404s, large objects that take long. */
export function RequestScatter(props: { profile: Profile; by: "time" | "size"; domainMs?: [number, number] | null; onRequest?: (id: number) => void }) {
  const rows = () => {
    const d = props.domainMs;
    return props.profile.samples
      .filter((x) => !d || (x.tMs >= d[0] && x.tMs < d[1]))
      .map((x) => ({ ...x, y: log10(Math.max(x.ms, 0.1)), x: props.by === "time" ? x.tMs : log10(Math.max(x.bytes ?? 0, 1)), tone: statusTone(x.status ?? null) }));
  };
  const yLo = () => Math.min(...rows().map((r) => r.y), log10(1)) - 0.12;
  const yHi = () => Math.max(...rows().map((r) => r.y), log10(10)) + 0.15;
  // a little room either side: a dot at the first or last instant is drawn whole
  const xDomain = (): [number, number] => {
    if (props.by !== "time") return [-0.1, Math.max(...rows().map((r) => r.x), 3) + 0.2];
    const [a, b] = props.domainMs ?? [0, Math.max(props.profile.wallMs, 1)];
    const pad = (b - a) * 0.015;
    return [a - pad, b + pad];
  };
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
          <Grid y={decades(Math.ceil(yLo()), yHi())} class="stroke-border" />
          <Grid y={[p95()]} class="stroke-muted-foreground/60" style={{ strokeDasharray: "3 3" }} />
          <Dot value="y" r={2.5} hitRadius={6} color="tone" inactiveOpacity={INACTIVE} hoverDotClass="size-2 rounded-full border border-card bg-foreground" />
          <PlotAxis placement="left" labelPlacement="inside" tickValues={decades(Math.ceil(yLo()), yHi())} format={(v) => duration(10 ** v)} labelClass={VALUE_TICK} />
          <PlotAxis ticks={3} format={(v) => (v < 0 ? "" : props.by === "time" ? duration(v) : bytes(10 ** v))} labelClass={TIME_TICK} />
          <PlotLabel class="tabular-nums text-caption text-muted-foreground" texts={[{ text: `p95 ${duration(props.profile.p95Ms ?? 0)}`, x: xDomain()[1], y: p95(), align: "right", class: "-translate-y-full pb-1" }]} />
        </Plot>
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
  const domain = (): [number, number] => props.domainMs ?? [0, Math.max(props.profile.wallMs, 1)];
  const hovered = signal<{ group: number; index: number } | null>(null);
  const tipOf = (k: { group: number; name: string; rows: { tMs: number; p50: number; p95: number }[] }) => {
    const r = hovered()?.group === k.group ? k.rows[hovered()!.index] : null;
    return r ? { title: k.name, lines: [`+${duration(r.tMs)}`, `p50 ${duration(r.p50)} · p95 ${duration(r.p95)}`] } : null;
  };
  return (
    <If when={!terminal() && kinds().length > 0}>
      <View testID="kinds-over-time" class="gap-1">
        <Each of={kinds()} key={(k) => String(k.group)}>
          {(k, i) => (
            <View class="flex-row items-center">
              <Text class={ROW_LABEL}>{axisName(k().name)}</Text>
              <Plot data={k().rows} x="tMs" xScale={{ type: "linear", domain: domain() }} yScale={{ type: "linear", domain: [0, top()] }} accessibilityLabel={`${k().name}: p95 ${duration(Math.max(...k().rows.map((r) => r.p95)))}`} motion={MOTION} crossLineClass={CROSS_LINE} tooltipPlacement="pointer" tooltipOffset={10} tooltipContent={<ChartTip tip={tipOf(k())} />} onHoverChange={({ index }) => hovered.set(index === null ? null : { group: k().group, index })} style={{ height: PITCH }} class={`flex-1 ${i() === kinds().length - 1 ? TICK_ROOM : ""}`}>
                <Area y0="p50" y1="p95" curve="linear" class="fill-chart-1/20" />
                <Line y="p95" curve="linear" class="stroke-chart-1 stroke-[1.25]" hoverDotClass="size-1.5 rounded-full bg-chart-1" />
                <Line y="p50" curve="linear" class="stroke-chart-1/50 stroke-1" hoverDotClass="hidden" />
                <If when={i() === kinds().length - 1}>
                  <PlotAxis ticks={3} format={(v) => duration(v)} labelClass={TIME_TICK} />
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
        class={`${TICK_ROOM} ${PLOT} w-full`}
      >
        {/* the median and p95: 99 sits on the top edge, its label over 95's */}
        <Grid y={[50, 95]} class="stroke-border" />
        <Line y="a" curve="stepAfter" class="stroke-muted-foreground/70 stroke-[1.25]" style={{ strokeDasharray: "3 3" }} hoverDotClass="size-1.5 rounded-full bg-muted-foreground" />
        <Line y="b" curve="stepAfter" class="stroke-chart-1 stroke-[1.5]" hoverDotClass="size-1.5 rounded-full bg-chart-1" />
        <PlotAxis placement="left" labelPlacement="inside" tickValues={[50, 95]} format={(v) => percent(v / 100)} labelClass={VALUE_TICK} />
        {/* a log scale ticks at its decades: 1 ms, 10 ms, 100 ms, never 3.16 ms */}
        <PlotAxis tickValues={decades(rows()[0].x, rows()[rows().length - 1].x)} format={(v) => duration(10 ** v)} labelClass={TIME_TICK} />
      </Plot>
    </If>
  );
}
