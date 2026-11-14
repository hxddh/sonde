// The chart system: every constant a chart is drawn with, the one frame,
// legend, tooltip and note, the tones of an outcome, and the moment shared
// by a pane's charts. Here and nowhere else (scripts/lint-charts.py counts them).

import { type JSX, signal, Text, token, View } from "nsh";
import { Grid, PlotAxis } from "nsh/base/plot";
import { duration, middle, percent } from "../../format";
import { t } from "../../i18n";
import type { ProfileGroup } from "../../types";
import { Card, EMPHASIS, Empty, Heading, POPUP, terminal, IconButton } from "../kit";

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
/** A row's name in that room: right-aligned; `axisName` cuts what does not fit, in its middle. */
export const AXIS_LABEL = "w-44 pr-3 text-right text-small text-foreground line-clamp-1";
/** The same name beside a plot that has no axis of names (small charts in rows). */
export const ROW_LABEL = "w-44 shrink-0 pr-3 text-right text-small text-foreground line-clamp-1";
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
/** The outcomes a request may have, each with its name and swatch: for a legend of dots or cells. */
export const OUTCOMES = [
  { tone: "ok", key: "flows.ok", swatch: "bg-bar" },
  { tone: "miss", key: "flows.notfound", swatch: "bg-status-miss" },
  { tone: "client", key: "flows.clienterror", swatch: "bg-status-client" },
  { tone: "server", key: "lanes.failed", swatch: "bg-status-server" },
] as const;
/** The flows' height: three columns of nodes need the room. */
export const FLOWS = "h-80";
/** Under a chart whose bottom axis has ticks: the room they take. */
export const TICK_ROOM = "mb-6";
/** What the pointer rests on: a band over a row or a slice, a line at a moment. */
export const CROSS_BAND = "w-full h-full rounded-(--radius-sm) bg-foreground/5";
export const CROSS_LINE = "w-px h-full bg-foreground/25";
/** What the pointer does not rest on fades to this, in every chart. */
export const INACTIVE = 0.4;
/** The one outline of the cell, the area or the node under the pointer. */
export const HOVER = "rounded-(--radius-sm) outline-2 outline-foreground";
/** A time axis's ticks: three, tabular, quiet (TimeAxis's alone: lint-charts fails it anywhere else). */
export const TIME_TICK = "pt-1 tabular-nums text-caption text-muted-foreground";
/** A bottom axis of buckets or measures (sizes, latencies, a count's two ends): time's look, by its own name. */
export const BUCKET_TICK = "pt-1 tabular-nums text-caption text-muted-foreground";
/** A value axis's ticks, inside the plot on a chip (an edge of its own: it reads as one on the card). */
export const VALUE_TICK = "ml-1 rounded-(--radius-sm) border border-border bg-card px-1 tabular-nums text-caption text-muted-foreground";
/** Zero of a value scale, outside the plot beside its baseline. */
export const ZERO_TICK = "pr-1 tabular-nums text-caption text-muted-foreground";
export const ZERO_TICK_END = "pl-1 tabular-nums text-caption text-muted-foreground";
/** The same at the plot's far edge (a second scale). */
export const VALUE_TICK_END = "mr-1 rounded-(--radius-sm) border border-border bg-card px-1 tabular-nums text-caption text-muted-foreground";
/** A budget's line (sonde.toml's limit) and its chip: in the warning's tone, dashed, at the far edge. */
export const BUDGET_LINE = "stroke-warn/70";
export const BUDGET_TICK = "mr-1 rounded-(--radius-sm) border border-warn/60 bg-card px-1 tabular-nums text-caption text-warn";
/** A figure at a bar's end, a note in the plot. */
export const BAR_LABEL = "pl-2 tabular-nums text-caption text-muted-foreground";
/** A row's figures on the row's own line, past its last mark: the label's middle on the line. */
export const ROW_FIGURE = "-translate-y-1/2 pl-2";
/** A figure over an upright bar. */
export const TOP_LABEL = "pb-1 tabular-nums text-caption text-muted-foreground";
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
    <If when={props.slices !== undefined} else={<PlotAxis tickValues={ticks()} format={(v) => (v < props.domain[0] - 1e-9 ? "" : say(v))} labelClass={TIME_TICK} />}>
      <PlotAxis tickValues={[...new Set(places())]} format={(i) => say(ticks()[places().indexOf(i)] ?? 0)} labelClass={TIME_TICK} />
    </If>
  );
}

/** The one value scale of a chart of counts: zero and the peak with its unit
 * (`say`). The peak is on a chip inside the plot, over its own line: a chart
 * leaves headroom above its peak (its domain a fifth higher), so the chip
 * stands clear of every mark. Zero is outside the plot, in the frame's own
 * padding beside the baseline: inside it stood on the first bar. `end` for a
 * second scale at the plot's far edge. */
export function ValueAxis(props: { peak: number; say: (v: number) => string; /** a second quantity under the line (bytes received) */ low?: number; scale?: "y" | "y2"; end?: boolean; /** every mark says its own value over itself (the histogram's bars): the scale writes zero alone, its chip would stand on the tallest one's */ marked?: boolean }) {
  return (
    <>
      <If when={!props.marked}>
      {/* the peak's own line, under its chip (a second scale's peak has the chip alone) */}
      <If when={props.scale !== "y2"}>
        <Grid y={[...(props.low !== undefined ? [props.low] : []), props.peak]} class={GRID} />
      </If>
      <PlotAxis
        placement={props.end ? "right" : "left"}
        scale={props.scale}
        labelPlacement="inside"
        tickValues={[...(props.low !== undefined ? [props.low] : []), props.peak]}
        format={(v) => props.say(Math.abs(v))}
        labelClass={props.end ? VALUE_TICK_END : VALUE_TICK}
      />
      </If>
      <PlotAxis placement={props.end ? "right" : "left"} scale={props.scale} tickValues={[0]} format={() => "0"} labelClass={props.end ? ZERO_TICK_END : ZERO_TICK} />
    </>
  );
}

/** A budget drawn on its chart: sonde.toml's limit (`max-p95`, `max-errors`,
 * `max-waiting`) as a dashed line in the warning's tone, its chip at the far
 * edge ("budget 50 ms"). Nothing where the session's program has no budget
 * for it. The chart's domain must reach it (`budgetTop`). */
export function Budget(props: { value: number | null | undefined; say: (v: number) => string }) {
  return (
    <If when={props.value != null && props.value > 0}>
      <Grid y={[props.value!]} class={BUDGET_LINE} style={{ strokeDasharray: "4 3" }} />
      <PlotAxis placement="right" labelPlacement="inside" tickValues={[props.value!]} format={(v) => t("budget.line", { v: props.say(v) })} labelClass={BUDGET_TICK} />
    </If>
  );
}
/** A domain's top that leaves the budget's line (and its chip) in view: a fifth above the higher of the two. */
export const budgetTop = (top: number, budget: number | null | undefined) => (budget != null && budget > 0 ? Math.max(top, budget * 1.2) : top);

// the keys and screen readers step the hover with a spring; the pointer's own moves follow at once
export const MOTION = { appear: { duration: 420, easing: "easeOut" as const }, update: { duration: 260, easing: "easeOut" as const }, enter: { duration: 100 }, exit: { duration: 160 }, pointer: { stiffness: 420, damping: 36 } };

export const TONES = ["ok", "miss", "client", "server", "injected"] as const;
export type Tone = (typeof TONES)[number];
export const TONE_TOKEN: Record<Tone, string> = { ok: "bar", miss: "status-miss", client: "status-client", server: "status-server", injected: "status-injected" };
/** A request's tone in a chart: its status's, or (a fault scenario answered it, not the store) the injected one. */
export const toneOf = (x: { id?: number; status?: number | null }, injected?: Set<number>): Tone => (x.id !== undefined && injected?.has(x.id) ? "injected" : statusTone(x.status ?? null));
/** The legend's entry for requests a scenario answered, when a session has any. */
export const injectedLegend = (ids?: number[]): LegendItem[] => (ids?.length ? [{ label: t("plots.injected"), swatch: "bg-status-injected" }] : []);
export const tones = () => ({ type: "ordinal" as const, domain: [...TONES, "dim"], range: [...TONES.map((x) => token(TONE_TOKEN[x] as never) as unknown as string), token("bar-track" as never) as unknown as string] });

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
          <Each of={x().lines}>{(l) => <Text class="tabular-nums text-caption text-muted-foreground">{l()}</Text>}</Each>
        </View>
      )}
    </If>
  );
}
/** What a screen reader hears of the row under the pointer: the tooltip's words. */
export const tipValue = (tip: ChartTipText | null | undefined) => ({ text: tip ? `${tip.title}: ${tip.lines.join(", ")}` : "" });

/** `line`: a line the chart draws (dashed: a p95, a share done), its swatch a short dash in that colour (`border-*`), never a dot. */
export type LegendItem = { label: string; swatch: string; line?: boolean };

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
            <View class={it().line ? `w-3 border-t-2 border-dashed ${it().swatch}` : `size-2 rounded-full ${it().swatch}`} />
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
 * sentence that answers it (`text`, computed from what it draws; it wraps:
 * never cut), how it is cut (`action`, at the title's end), how to read it
 * (`how`: an info mark there, the words its tooltip), the plot, its `legend`
 * at the plot's left edge (`gutter` for a chart of rows), and `empty` said in
 * the plot's room when there is nothing to draw. A terminal gets the title
 * and what the chart says in words. */
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
    <Card testID={props.testID} class={props.class}>
      <View class="min-h-7 flex-row items-start gap-3">
        <View class="min-w-0 flex-1 gap-1">
          <Heading>{props.title}</Heading>
          <If when={props.text}>
            <Text testID="chart-text" class="tabular-nums text-small text-muted-foreground">{props.text}</Text>
          </If>
        </View>
        {props.action}
        {/* how to read it, in its tooltip: the sentence under the title says what it shows */}
        <If when={props.how}>
          <IconButton icon="info" label={props.how!} testID={props.testID ? `${props.testID}-how` : undefined} />
        </If>
        <If when={props.onExport && !props.empty}>
          <IconButton icon="download" label={t("chart.csv")} onPress={() => props.onExport?.()} testID={props.testID ? `${props.testID}-csv` : undefined} />
        </If>
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

export const statusTone = (s: number | null): Tone => (s === null || s >= 500 ? "server" : s === 404 ? "miss" : s >= 400 ? "client" : "ok");

/** A map of areas' words, the same in every one: a group's name, an area's, its figure. */
// emphasis: a group's name over the areas in it
export const GROUP_LABEL = `px-2 text-caption ${EMPHASIS} text-foreground`;
export const AREA_LABEL = "p-2 font-mono text-caption text-foreground";
/** (on a chip of the card's colour: a figure reads on a saturated tile, light or dark) */
export const AREA_VALUE = "ml-2 self-start rounded-(--radius-sm) bg-card/85 px-1 font-sans tabular-nums text-caption text-foreground";

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
