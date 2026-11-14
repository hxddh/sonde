// The chart system: every constant a chart is drawn with, the one frame,
// legend, tooltip and note, the tones of an outcome, and the moment shared
// by a pane's charts. Here and nowhere else (scripts/lint-charts.py counts them).

import { type JSX, signal, Text, token, View } from "nsh";
import { Grid, PlotAxis } from "nsh/base/plot";
import { duration, middle, percent } from "../../format";
import { t } from "../../i18n";
import type { ProfileGroup } from "../../types";
import { Card, EMPHASIS, Empty, Heading, IconButton, LEADING, POPUP, terminal, TEXT, TIMING, TONE, TRANSITION, TYPE } from "../kit";

/** A kind of request as people say it: "HeadObject" or "HeadObject, not found". */
export const groupName = (g: Pick<ProfileGroup, "kind" | "missed">) => (g.missed ? t("profile.notFound", { op: g.kind }) : g.kind);

export const pct = percent;

// --- one chart system: every constant a chart is drawn with, here and nowhere else ----
// (scripts/lint-charts.py counts them: one frame, one left edge, one row
// pitch, one treemap height, one legend, one tooltip, one fade, one outline)

/** The chart components: each draws its `Plot`s as the body of a `Chart`
 * frame. lint-charts fails a `Plot` outside these, and one of these used
 * outside a frame. */
export const CHARTS = ["Lanes", "SessionShape", "LatencyBand", "Chains", "StoreLatency", "TimeSpent", "Waterfall", "RunBars", "Flows", "Heat", "Distribution", "PrefixUse", "PrefixChangeMap", "RunTrend", "RequestScatter", "KindsOverTime", "DistributionCompare", "BarChart", "StackBar", "PrefixMap", "BytesOverTime", "ErrorsOverTime", "ErrorRate", "LatencyHeat", "TrialOverClean", "FirstByte", "Sizes", "Throughput", "CostBy"] as const;
/** The charts drawn over time: each has the one time axis (`TimeAxis`), so
 * they name the same moments at the same places. lint-charts fails one
 * without it, and a time axis made any other way. */
export const OVER_TIME = ["Lanes", "SessionShape", "LatencyBand", "LatencyHeat", "Chains", "Waterfall", "Heat", "RequestScatter", "KindsOverTime", "BytesOverTime", "ErrorsOverTime", "ErrorRate", "TrialOverClean"] as const;
/** The charts of counts: each has the one value scale (`ValueAxis`: zero and the peak, with its unit). */
export const COUNTS = ["SessionShape", "LatencyBand", "Distribution", "BytesOverTime", "ErrorsOverTime", "ErrorRate", "Sizes", "TrialOverClean"] as const;
/** Marks that are not charts and stand in no frame: a session's requests a
 * few points high in its row, and the shape of a session as the strip under
 * its header (`SessionShape strip`). */
export const MARKS = ["Sparkline", "SessionShape strip"] as const;
/** Charts that draw their own frame around their plot (their figure stands
 * where a frame's control does): used as they are, never inside another `Chart`. */
export const FRAMED = ["RunTrend"] as const;

/** The room left of a chart across, for the names of its rows: one width
 * for every chart, so their bars start at one edge. Wide enough for
 * "GetObject, not found" and "Chain 2 · 18 requests" whole. */
export const GUTTER = "ml-44";
/** The same room before a legend or a note under such a chart. */
export const GUTTER_PAD = "pl-44";
/** The same room either side: a chart with names at both edges (flows). */
export const GUTTER_BOTH = "mx-44";
/** The room left of a plot whose scale has several ticks (a log scale of
 * latency, a band of them): its labels stand there, outside the plot, never
 * on a dot or a cell. One width, for "≤ 100 ms" whole. */
export const GUTTER_SCALE = "ml-14";
/** The same at the far edge: a second scale's ticks (the share done by then). */
export const GUTTER_SCALE_END = "mr-10";
/** A row's name in that room: right-aligned; `axisName` cuts what does not fit, in its middle. */
export const AXIS_LABEL = `w-44 pr-3 text-right line-clamp-1 ${TEXT.small}`;
/** The same name beside a plot that has no axis of names (small charts in rows). */
export const ROW_LABEL = `w-44 shrink-0 pr-3 text-right line-clamp-1 ${TEXT.small}`;
/** What the gutter holds of a name, in characters: a longer one is cut in its
 * middle (both ends tell kinds apart), never at its end by the cell. */
export const GUTTER_CHARS = 25;
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
export const PLOT = "h-44";
/** The session's shape on the Time pane, its findings' numbers over it. */
export const SHAPE = "h-28";
/** A thin strip: a figure over a program's runs, latency in Run's live panel. */
export const STRIP = "h-16";
/** One bar of parts, a stack (Capacity's classes). */
export const STACK = "h-3";
/** The shape of a session under its header (`SessionShape strip`). */
export const SHAPE_STRIP = "h-11";
/** A mark a few points high, in a row or beside a figure (`Sparkline`). */
export const MARK = { row: "h-4 w-16", tile: "h-5 w-16 shrink-0", pair: "h-5 w-12 shrink-0" } as const;
/** How thick a bar is, by what it is a bar of — the band's padding, and no
 * other anywhere: a row of a chart of rows, two marks on a row (a dumbbell,
 * a chain as it ran and at once), a slice of time, a bucket (a size, a
 * latency), a mark's bars. */
export const BARS = {
  rows: { paddingInner: 0.32, paddingOuter: 0.16 },
  pairs: { paddingInner: 0.3, paddingOuter: 0.14 },
  slices: { paddingInner: 0.18 },
  buckets: { paddingInner: 0.26 },
} as const;
/** The cells of a grid: lanes touch along time and part between lanes; a heat map's cells part both ways. */
export const CELLS = { run: 0, lane: 0.3, heat: 0.14 } as const;
/** A bar's tip, by where it points — its base is square: across (a chart of
 * rows), up or down (slices, buckets), a bar between two moments (`span`),
 * a cell, an area of a map (and the gap between two areas), a mark's bars. */
export const TIP = {
  across: { topRight: 3, bottomRight: 3 },
  up: { topLeft: 2, topRight: 2 },
  down: { bottomLeft: 2, bottomRight: 2 },
  span: 2,
  cell: 2,
  area: 4,
  areaGap: 2,
  mark: { topLeft: 1, topRight: 1 },
} as const;
/** A chart's grid lines: hairlines. */
export const GRID = "stroke-border";
/** The outcomes a request may have, each with its name and swatch (its tone's `fill`): for a legend of dots or cells. */
export const OUTCOMES = [
  { tone: "ok", key: "flows.ok", swatch: TONE.ok.fill },
  { tone: "miss", key: "flows.notfound", swatch: TONE.miss.fill },
  { tone: "client", key: "flows.clienterror", swatch: TONE.client.fill },
  { tone: "fail", key: "lanes.failed", swatch: TONE.fail.fill },
] as const;
/** The flows' height: three columns of nodes need the room. */
export const FLOWS = "h-80";
/** Under a chart whose bottom axis has ticks: the room they take. */
export const TICK_ROOM = "mb-6";
/** What the pointer rests on: a band over a row or a slice, a line at a moment. */
export const CROSS_BAND = "w-full h-full rounded-(--radius-sm) bg-foreground/5";
export const CROSS_LINE = "w-px h-full bg-foreground/25";
/** The moment another chart's pointer rests on, marked in this one. */
export const FOCUS_RULE = "stroke-foreground/40 stroke-1";
/** What the pointer does not rest on fades to this, in every chart. */
export const INACTIVE = 0.4;
/** The one outline of the cell, the area or the node under the pointer. */
export const HOVER = "rounded-(--radius-sm) outline-2 outline-foreground";
/** A budget's line (sonde.toml's limit): in the warning's tone, dashed, its words at the far edge. */
export const BUDGET_LINE = "stroke-warn/70";
/** The words of a chart, each a text role of the kit's (`TEXT`): read where
 * they are drawn (getters), so Chinese takes its 12 pt caption and a change
 * of language reaches them. A label's place (its padding, a shift off its
 * line) stands beside its role. */
export const LABEL = {
  /** a time axis's ticks: TimeAxis's alone (lint-charts fails it anywhere else) */
  get time() {
    return `pt-1 tabular-nums ${TEXT.meta}`;
  },
  /** a bottom axis of buckets or measures (sizes, latencies, a count's two ends): time's look, by its own name */
  get bucket() {
    return `pt-1 tabular-nums ${TEXT.meta}`;
  },
  /** a value scale's peak, inside the plot at its edge and over its own line, in the headroom the
   * plot leaves above its marks: never on one. On the card's colour, so a rule that crosses it (a
   * finding's, the pointer's) passes under it. */
  get peak() {
    return `ml-1 -translate-y-1/2 rounded-(--radius-sm) bg-card px-1 tabular-nums ${TEXT.meta}`;
  },
  /** the same at the far edge (a second scale's peak) */
  get peakEnd() {
    return `mr-1 -translate-y-1/2 rounded-(--radius-sm) bg-card px-1 tabular-nums ${TEXT.meta}`;
  },
  /** the peak of what hangs under the baseline (bytes received): under its own line */
  get low() {
    return `ml-1 translate-y-1/2 rounded-(--radius-sm) bg-card px-1 tabular-nums ${TEXT.meta}`;
  },
  /** zero, outside the plot beside its baseline (inside it stood on the first bar) */
  get zero() {
    return `pr-1 tabular-nums ${TEXT.meta}`;
  },
  get zeroEnd() {
    return `pl-1 tabular-nums ${TEXT.meta}`;
  },
  /** a scale of several ticks, outside the plot in its room (`GUTTER_SCALE`, `GUTTER_SCALE_END`) */
  get scale() {
    return `pr-2 tabular-nums ${TEXT.meta}`;
  },
  get scaleEnd() {
    return `pl-2 tabular-nums ${TEXT.meta}`;
  },
  /** a budget's words, over its line at the far edge */
  get budget() {
    return `mr-1 -translate-y-1/2 rounded-(--radius-sm) bg-card px-1 tabular-nums ${TEXT.captionSize} text-warn`;
  },
  /** a figure at a bar's end */
  get bar() {
    return `pl-2 tabular-nums ${TEXT.meta}`;
  },
  /** a figure over an upright bar */
  get top() {
    return `pb-1 tabular-nums ${TEXT.meta}`;
  },
  /** a row's figures past its last mark, a line's own figure (a run's, a p95's) */
  get figure() {
    return `tabular-nums ${TEXT.meta}`;
  },
  /** the same, in the text's colour: what the row is about (a dumbbell's two figures) */
  get figureStrong() {
    return `tabular-nums ${TEXT.caption}`;
  },
  /** emphasis: a finding's number over its slice */
  get mark() {
    return `tabular-nums ${TEXT.captionSize} ${EMPHASIS} text-warn`;
  },
  /** a legend's names, a note under a chart, a tooltip's figures */
  get note() {
    return `tabular-nums ${TEXT.meta}`;
  },
  /** emphasis: a group's name over the areas in it (a map of areas) */
  get group() {
    return `px-2 ${TEXT.captionSize} ${EMPHASIS} text-foreground`;
  },
  /** an area's name, and its figure under it on the same edge */
  get area() {
    return `p-2 font-mono ${TEXT.caption}`;
  },
  get areaValue() {
    return `self-start font-sans tabular-nums ${TEXT.caption}`;
  },
  /** a flow's node: its name and count beside it (on the card's colour where it stands over ribbons) */
  get node() {
    return `m-2 max-w-44 rounded-(--radius-sm) bg-card px-1 tabular-nums ${TEXT.small}`;
  },
};
/** A row's figures on the row's own line, past its last mark: the label's middle on the line. */
export const ROW_FIGURE = "-translate-y-1/2 pl-2";
/** The same before its first mark, where the row's marks reach the plot's far edge. */
export const ROW_FIGURE_BEFORE = "-translate-y-1/2 pr-2";
/** Two charts abreast where the board is wide, one under another where it is not. */
export const HALF = "min-w-[420px] flex-1";

/** A moment on a time axis, read from the session's start: `+230 ms`. The one
 * form of every moment (a length of time — a latency, how long a chain took
 * — is `duration` alone). */
export const sinceTick = (ms: number) => `+${duration(ms)}`;

/** The whole of a session on a time axis, in ms: its slices' extent (never
 * less than its wall time), the one stretch every chart over a session shows
 * until a zoom picks another — so their ticks are the same ticks. */
export const sessionSpan = (x: { binMs: number; wallMs: number }, slices: number): [number, number] => [0, Math.max(slices * x.binMs, x.wallMs, 1)];

/** The moments a time axis names over `[a, b]` ms: round ones (1, 2 or 5 of a
 * power of ten apart), three or four of them, the same for every chart over
 * the same stretch — whatever its scale is made of. */
export function timeTicks(domain: [number, number]): number[] {
  const [a, b] = domain;
  const span = b - a;
  if (!(span > 0)) return [a];
  const raw = span / 4;
  const pow = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 5, 10].map((m) => m * pow).find((x) => x >= raw - 1e-9) ?? 10 * pow;
  const out: number[] = [];
  // (`|| 0`: the first of them is 0, never −0)
  for (let k = Math.ceil(a / step - 1e-9); k * step <= b + 1e-9; k++) out.push(k * step || 0);
  return out;
}

/** The one time axis, under every chart over time: `timeTicks` of the stretch
 * shown, each said from the session's start (`+200 ms`). A chart of slices
 * (a band scale) gives how many it shows (`slices`): a tick stands on the
 * slice its moment falls in. `lengths` for an axis of time within one
 * operation: the same ticks, said as lengths. lint-charts fails a `PlotAxis`
 * over time anywhere else. */
export function TimeAxis(props: { domain: [number, number]; slices?: number; lengths?: boolean }) {
  const ticks = () => timeTicks(props.domain);
  const say = (ms: number) => (props.lengths ? (ms === 0 ? "0" : duration(ms)) : sinceTick(ms));
  const width = () => (props.domain[1] - props.domain[0]) / Math.max(1, props.slices ?? 1);
  // a slice's index for each moment (the last slice for the stretch's end)
  const places = () => ticks().map((ms) => Math.min((props.slices ?? 1) - 1, Math.max(0, Math.floor((ms - props.domain[0]) / width() + 1e-9))));
  return (
    <If when={props.slices !== undefined} else={<PlotAxis tickValues={ticks()} format={(v) => (v < props.domain[0] - 1e-9 ? "" : say(v))} labelClass={LABEL.time} />}>
      <PlotAxis tickValues={[...new Set(places())]} format={(i) => say(ticks()[places().indexOf(i)] ?? 0)} labelClass={LABEL.time} />
    </If>
  );
}

/** The one value scale of a chart of counts: zero and the peak with its unit
 * (`say`). The peak stands over its own line, inside the plot at its edge,
 * in the headroom a chart leaves above its marks (`HEADROOM`): no mark
 * reaches it. Zero is outside the plot, in the frame's own padding beside the
 * baseline: inside it stood on the first bar. `end` for a second scale at the
 * plot's far edge; `outside` for a plot with the room for a scale
 * (`GUTTER_SCALE`): both there, each on its line. */
export function ValueAxis(props: { peak: number; say: (v: number) => string; /** a second quantity under the line (bytes received) */ low?: number; scale?: "y" | "y2"; end?: boolean; /** in the plot's room for a scale (`GUTTER_SCALE`), outside it */ outside?: boolean; /** every mark says its own value over itself (the histogram's bars): the scale writes zero alone, its peak would stand on the tallest one's */ marked?: boolean }) {
  const side = () => (props.end ? "right" : "left") as "right" | "left";
  return (
    <>
      <If when={!props.marked}>
        {/* the peak's own line, under its words (a second scale's peak has its words alone) */}
        <If when={props.scale !== "y2"}>
          <Grid y={[...(props.low !== undefined ? [props.low] : []), props.peak]} class={GRID} />
        </If>
        <If when={!props.outside} else={<PlotAxis placement={side()} scale={props.scale} tickValues={[...(props.low !== undefined ? [props.low] : []), props.peak]} format={(v) => props.say(Math.abs(v))} labelClass={props.end ? LABEL.scaleEnd : LABEL.scale} />}>
          <PlotAxis placement={side()} scale={props.scale} labelPlacement="inside" tickValues={[props.peak]} format={(v) => props.say(Math.abs(v))} labelClass={props.end ? LABEL.peakEnd : LABEL.peak} />
          <If when={props.low !== undefined}>
            <PlotAxis placement={side()} scale={props.scale} labelPlacement="inside" tickValues={[props.low!]} format={(v) => props.say(Math.abs(v))} labelClass={LABEL.low} />
          </If>
        </If>
      </If>
      <PlotAxis placement={side()} scale={props.scale} tickValues={[0]} format={() => "0"} labelClass={props.outside ? (props.end ? LABEL.scaleEnd : LABEL.scale) : props.end ? LABEL.zeroEnd : LABEL.zero} />
    </>
  );
}

/** How far a chart's scale reaches over its peak: room for the peak's words
 * over its line, clear of every mark. A plot of `PLOT`'s height; a strip
 * (`STRIP`) needs more of its height for the same words. */
export const HEADROOM = { plot: 1.25, strip: 1.45 } as const;

/** A budget drawn on its chart: sonde.toml's limit (`max-p95`, `max-errors`,
 * `max-waiting`) as a dashed line in the warning's tone, its words over it at
 * the far edge ("budget 50 ms"). Nothing where the session's program has no
 * budget for it. The chart's domain must reach it (`budgetTop`). */
export function Budget(props: { value: number | null | undefined; say: (v: number) => string }) {
  return (
    <If when={props.value != null && props.value > 0}>
      <Grid y={[props.value!]} class={BUDGET_LINE} style={{ strokeDasharray: DASH.reference }} />
      <PlotAxis placement="right" labelPlacement="inside" tickValues={[props.value!]} format={(v) => t("budget.line", { v: props.say(v) })} labelClass={LABEL.budget} />
    </If>
  );
}
/** A domain's top that leaves the budget's line (and its chip) in view: a fifth above the higher of the two. */
export const budgetTop = (top: number, budget: number | null | undefined) => (budget != null && budget > 0 ? Math.max(top, budget * HEADROOM.plot) : top);

/** The one ease of the kit's motion (nsh.json's `ease-standard`), for Plot's own timings. */
const STANDARD: [number, number, number, number] = [0.22, 1, 0.36, 1];
/** Every chart's motion, on the kit's lengths (`TIMING`) and its one ease: the marks draw in as a
 * reveal (`slow`) and glide to new values as a thing moving into place (`base`); the hover's wash
 * comes in a step slower than it goes. The keys and screen readers step the hover with a spring; the
 * pointer's own moves follow at once. Nothing moves under reduced motion (Native Shell's own rule). */
export const MOTION = {
  appear: { duration: TIMING.slow, easing: STANDARD },
  update: { duration: TIMING.base, easing: STANDARD },
  enter: { duration: TIMING.fast, easing: STANDARD },
  exit: { duration: TIMING.instant, easing: STANDARD },
  pointer: { stiffness: 420, damping: 36 },
};

/** How thick a chart's lines are, by what they draw, and no other: a rule or
 * a grid (the moment the pointer rests on, a finding's place), a line of
 * data, a run's trend (the one line of its chart), a dumbbell's bar between
 * two dots. */
export const STROKE = { hair: "stroke-1", line: "stroke-[1.5]", trend: "stroke-2", link: "stroke-[2.5]" } as const;
/** The dashes of a line that is not data: a reference (a p95, a baseline, a budget, a share done), a mark (a finding's place). */
export const DASH = { reference: "3 3", mark: "2 2" } as const;

/** The outcomes a request may have in a chart: the kit's tones (`TONE`) of a request. */
export const OUTCOME_TONES = ["ok", "miss", "client", "fail", "injected"] as const;
export type Outcome = (typeof OUTCOME_TONES)[number];
/** Each outcome's colour token (its `TONE[t].fill` without the `bg-`): for a colour scale. */
export const OUTCOME_TOKEN: Record<Outcome, string> = { ok: "bar", miss: "status-miss", client: "status-client", fail: "status-server", injected: "status-injected" };
/** A request's outcome in a chart: its status's, or (a fault scenario answered it, not the store) the injected one. */
export const toneOf = (x: { id?: number; status?: number | null }, injected?: Set<number>): Outcome => (x.id !== undefined && injected?.has(x.id) ? "injected" : statusTone(x.status ?? null));
/** The legend's entry for requests a scenario answered, when a session has any. */
export const injectedLegend = (ids?: number[]): LegendItem[] => (ids?.length ? [{ label: t("plots.injected"), swatch: TONE.injected.fill }] : []);
export const tones = () => ({ type: "ordinal" as const, domain: [...OUTCOME_TONES, "dim"], range: [...OUTCOME_TONES.map((x) => token(OUTCOME_TOKEN[x] as never) as unknown as string), token("bar-track" as never) as unknown as string] });

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
          <Text class="text-small text-popover-foreground line-clamp-2">{x().title}</Text>
          <Each of={x().lines}>{(l) => <Text class={LABEL.note}>{l()}</Text>}</Each>
        </View>
      )}
    </If>
  );
}
/** What a screen reader hears of the row under the pointer: the tooltip's words. */
export const tipValue = (tip: ChartTipText | null | undefined) => ({ text: tip ? `${tip.title}: ${tip.lines.join(", ")}` : "" });

/** `line`: a line the chart draws, its swatch a short stroke in that colour (`border-*`), never a
 * dot: `"dashed"` for a reference (a p95, a share done), `"solid"` for a line of data. */
export type LegendItem = { label: string; swatch: string; line?: "solid" | "dashed" };

/** What a chart's colors stand for: a round swatch (its `bg-*` class) and a
 * name each, in a row that wraps, and after them what else is to be said
 * under the chart (`children`). `gutter` starts it at the plot's left edge
 * under a chart of rows. The one legend: a short name for each mark drawn,
 * and none for a mark that is not. */
export function Legend(props: { items: LegendItem[]; gutter?: boolean; children?: JSX.Element }) {
  return (
    <View testID="chart-legend" class={`flex-row flex-wrap items-center gap-x-4 gap-y-1 ${props.gutter ? GUTTER_PAD : ""}`}>
      <Each of={props.items} key={(it) => it.label}>
        {(it) => (
          <View class="flex-row items-center gap-1">
            <View class={it().line ? `w-3 border-t-2 ${it().line === "dashed" ? "border-dashed" : ""} ${it().swatch}` : `size-2 rounded-full ${it().swatch}`} />
            <Text class={LABEL.note}>{it().label}</Text>
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
    <Text testID={props.testID} class={`${LABEL.note} ${props.gutter ? GUTTER_PAD : ""}`}>
      {props.children}
    </Text>
  );
}

/** A frame's own controls (how to read it, its rows out as CSV): quiet until
 * the pointer is over the chart or the keys are on one of them, the same on
 * every chart. */
const CHROME = `flex-row opacity-0 group-hover:opacity-100 focus-within:opacity-100 ${TRANSITION}`;

/** A chart in its frame, the same for every chart: what it asks (`title`,
 * the card's whole width), a sentence that answers it (`text`, computed from
 * what it draws; it wraps: never cut), how it is cut (`action`, a row of its
 * own under the sentence), how to read it (`how`: an info mark at the
 * title's end, the words its tooltip) and its rows out (`onExport`), both
 * shown under the pointer; the plot, its `legend` at the plot's left edge
 * (`gutter` for a chart of rows), and `empty` said in the plot's room when
 * there is nothing to draw. A terminal gets the title and what the chart
 * says in words. */
export function Chart(props: { title: string; text?: string; how?: string; action?: JSX.Element; children?: JSX.Element; legend?: LegendItem[]; gutter?: boolean; empty?: string | null | false; testID?: string; class?: string; /** its rows out as CSV (where the page can write a file): a button at the title's end */ onExport?: () => void }) {
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
    <Card testID={props.testID} class={`group ${props.class ?? ""}`}>
      <View class="gap-1">
        <View class="min-h-7 flex-row items-center gap-2">
          <View class="min-w-0 flex-1">
            <Heading>{props.title}</Heading>
          </View>
          <If when={props.how || (props.onExport && !props.empty)}>
            <View class={CHROME}>
              {/* how to read it, in its tooltip: the sentence under the title says what it shows */}
              <If when={props.how}>
                <IconButton icon="info" label={props.how!} testID={props.testID ? `${props.testID}-how` : undefined} />
              </If>
              <If when={props.onExport && !props.empty}>
                <IconButton icon="download" label={t("chart.csv")} onPress={() => props.onExport?.()} testID={props.testID ? `${props.testID}-csv` : undefined} />
              </If>
            </View>
          </If>
        </View>
        <If when={props.text}>
          <Text testID="chart-text" class={`tabular-nums ${TEXT.note}`}>{props.text}</Text>
        </If>
      </View>
      <If when={props.action}>
        <View class="flex-row items-center">{props.action}</View>
      </If>
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
export function NoneInStretch() {
  return <Empty compact testID="none-in-stretch" class={`${TICK_ROOM} ${PLOT} w-full`} text={t("plots.noneInStretch")} />;
}

// --- one moment across the charts ------------------------------------------------

/** The moment the pointer rests on in one time chart (ms from the session's
 * start), marked in the others: the charts of a pane read as one. Each pane
 * (and each window) makes its own with `focusOf()` and hands it to its
 * charts; two sessions open at once do not mark each other's moments. */
export type Focus = { (): number | null; set: (at: number | null) => void };
export const focusOf = (): Focus => signal<number | null>(null);

/** The row of `rows` nearest the focused moment, if one is focused. */
export function nearest(rows: { tMs: number }[], at: number | null): number | null {
  if (at === null || rows.length === 0) return null;
  let best = 0;
  for (let i = 1; i < rows.length; i++) if (Math.abs(rows[i].tMs - at) < Math.abs(rows[best].tMs - at)) best = i;
  return best;
}

export const statusTone = (s: number | null): Outcome => (s === null || s >= 500 ? "fail" : s === 404 ? "miss" : s >= 400 ? "client" : "ok");

// --- a map of areas: which areas have room for their words ---------------------------

type Box = { x0: number; y0: number; x1: number; y1: number };
type Area = Box & { i: number; value: number; children: Area[] };
const GOLDEN = (1 + Math.sqrt(5)) / 2;

/** d3's squarified tiling (`treemapSquarify`, at the golden ratio, Native Shell's default) of `node`'s children in a box. */
function squarify(node: Area, x0: number, y0: number, x1: number, y1: number) {
  const nodes = node.children;
  let value = node.value;
  let i0 = 0;
  let i1 = 0;
  while (i0 < nodes.length) {
    const dx = x1 - x0;
    const dy = y1 - y0;
    let sum = nodes[i1++].value;
    let min = sum;
    let max = sum;
    const alpha = Math.max(dy / dx, dx / dy) / (value * GOLDEN);
    let beta = sum * sum * alpha;
    let best = Math.max(max / beta, beta / min);
    for (; i1 < nodes.length; ++i1) {
      const v = nodes[i1].value;
      sum += v;
      min = Math.min(min, v);
      max = Math.max(max, v);
      beta = sum * sum * alpha;
      const ratio = Math.max(max / beta, beta / min);
      if (ratio > best) {
        sum -= v;
        break;
      }
      best = ratio;
    }
    const row = nodes.slice(i0, i1);
    if (dx < dy) {
      // a row across the top, its nodes side by side
      const y = dy ? y0 + (dy * sum) / value : y1;
      let x = x0;
      for (const n of row) Object.assign(n, { x0: x, x1: (x += (n.value * (x1 - x0)) / sum), y0, y1: y });
      y0 = y;
    } else {
      // a column down the left, its nodes one under another
      const x = dx ? x0 + (dx * sum) / value : x1;
      let y = y0;
      for (const n of row) Object.assign(n, { y0: y, y1: (y += (n.value * (y1 - y0)) / sum), x0, x1: x });
      x0 = x;
    }
    value -= sum;
    i0 = i1;
  }
}

/** Which leaves of a map of areas (`rows` in a Treemap's `id`/`parent`/`value`
 * form) have room for their name whole in a plot `width` by `height` (Native
 * Shell writes the figure under it where that fits too). Laid out as Native
 * Shell lays a treemap out (d3's squarified tiling, siblings largest first,
 * `gap` between them and a band of `top` over a group's children). An area
 * without room says nothing: a name cut to "slat…" names nothing, and the
 * tooltip has it whole. `chars` is how wide a leaf's name is, in characters. */
export function areasWithRoom(rows: { id: string; parent: string; value: number }[], width: number, height: number, gap: number, top: number, chars: (i: number) => number): boolean[] {
  const out = rows.map(() => false);
  if (!(width > 0 && height > 0)) return out;
  const ids = new Map(rows.map((r, i) => [r.id, i]));
  const made = rows.map((r, i): Area => ({ i, value: 0, children: [], x0: 0, y0: 0, x1: 0, y1: 0 }));
  const root: Area = { i: -1, value: 0, children: [], x0: 0, y0: 0, x1: width, y1: height };
  rows.forEach((r, i) => (ids.has(r.parent) && r.parent !== r.id ? made[ids.get(r.parent)!] : root).children.push(made[i]));
  const sum = (n: Area): number => (n.value = n.children.length ? n.children.reduce((s, c) => s + sum(c), 0) : Math.max(0, rows[n.i]?.value ?? 0));
  sum(root);
  const place = (n: Area, depth: number) => {
    n.children = n.children.filter((c) => c.value > 0).sort((a, b) => b.value - a.value);
    if (!n.children.length) return;
    // d3's padding: half the gap round each child, the parent's box widened by as much (no gap on its outer edge)
    const p = gap / 2;
    const band = depth > 0 ? top : 0;
    squarify(n, n.x0 - p, n.y0 + band - p, n.x1 + p, n.y1 + p);
    for (const c of n.children) {
      Object.assign(c, { x0: c.x0 + p, y0: c.y0 + p, x1: c.x1 - p, y1: c.y1 - p });
      place(c, depth + 1);
    }
  };
  place(root, 0);
  // a name's line, a caption's, in the label's padding (p-2)
  const em = TYPE.caption * 0.62;
  const line = LEADING.caption;
  const pad = 8;
  const walk = (n: Area) => {
    if (n.i >= 0 && !n.children.length) {
      const w = n.x1 - n.x0 - 2 * pad;
      const h = n.y1 - n.y0 - 2 * pad;
      out[n.i] = w >= chars(n.i) * em && h >= line;
    }
    n.children.forEach(walk);
  };
  walk(root);
  return out;
}

export const log10 = (v: number) => Math.log10(Math.max(v, 1e-3));
/** Powers of ten across `[lo, hi]` (as logs): the ticks of a log scale. */
export const decades = (lo: number, hi: number) => {
  const out: number[] = [];
  for (let d = Math.floor(lo); d <= Math.ceil(hi); d++) if (d >= lo - 1e-9 && d <= hi + 1e-9) out.push(d);
  return out.length >= 2 ? out : [lo, hi];
};

/** A bar of none that is said ("0" beside a bar with a count): drawn this share of the scale's top, a sliver on
 * the baseline for its label to stand on (Native Shell draws no bar, and no label, for a row of no value). */
export const ZERO_SLIVER = 0.006;

/** A heat map's ramp, from a cell of one request to the busiest: its own two tones, so a few
 * requests read on the card in dark too (bar-track to chart-1 was navy on near-black there). */
export const HEAT_RAMP = (): [string, string] => [token("heat-low" as never) as unknown as string, token("heat-high" as never) as unknown as string];

/** The fewest slices a chart fitted to its marks shows (`fitted`). */
const FIT_LEAST = 12;

/** The slices a chart of a few marks over time shows when no stretch is
 * chosen: from a slice before its first mark to one after its last, never
 * fewer than `FIT_LEAST` (nor a fifth of the session). Two failures in a
 * run's last moment are two bars of a dozen slices, not two slivers at the
 * edge of a chart nine tenths empty. Null where that is (nearly) the whole
 * session: it shows the whole session, as every other chart does. */
export function fitted(n: number, has: (i: number) => boolean): [number, number] | null {
  let a = -1;
  let b = -1;
  for (let i = 0; i < n; i++) if (has(i)) (a = a < 0 ? i : a), (b = i);
  const least = Math.max(FIT_LEAST, Math.ceil(n / 5));
  if (a < 0 || n <= least) return null;
  let lo = a - 1;
  let hi = b + 1;
  const short = least - (hi - lo + 1);
  if (short > 0) (lo -= Math.ceil(short / 2)), (hi += Math.floor(short / 2));
  if (lo < 0) (hi -= lo), (lo = 0);
  if (hi > n - 1) (lo = Math.max(0, lo - (hi - (n - 1)))), (hi = n - 1);
  return hi - lo + 1 >= n * 0.8 ? null : [lo, hi];
}

/** A stretch of the session's slices (a zoom), or all of them. */
export const stretch = <T,>(rows: T[], range?: [number, number] | null): T[] => (range ? rows.slice(range[0], range[1] + 1) : rows);

/** A path too long for its place, cut at its start on a folder's edge: the
 * end is what tells two prefixes apart (`…/compactions/`, `…/compacted/`). */
export function tail(path: string, max: number): string {
  if (path.length <= max) return path;
  const keep = path.slice(-(max - 1));
  const edge = keep.indexOf("/");
  // from a folder's edge where one is near the cut, else mid-name
  return `…${edge >= 0 && edge < keep.length - 1 && edge <= 8 ? keep.slice(edge) : keep}`;
}
