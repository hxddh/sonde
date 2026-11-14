// A session's time, read in one order — Traffic (when, how many at once, how
// much), Latency (which operations were slow, how long, where the time went),
// Errors (when, what share, what was answered), Where (which prefixes, at
// what cost, on which path) — each chart in the one frame, titled by the
// question it answers. A stretch picked on the timeline is the pane's
// selection: the charts over time zoom to it, and every other chart is
// reckoned again from the requests that started in it (the bridge's
// `stretch`), saying so. A signal pressed in the session's header brings its
// chart into view.

import { batch, effect, type JSX, resource, Scroll, signal, Text, untracked, View, type ViewRef } from "nsh";
import { t } from "../i18n";
import { costColumn, count, duration, percent } from "../format";
import { FEW, kindStats, type SignalId, signalsOf } from "../signals";
import { type Range, slicesOf } from "../range";
import { type ChartRows, toCsv } from "../csv";
import type { ProjectBudget, SessionView, Signals, Split, SplitBy, Stretch } from "../types";
import { Await, Button, Heading, Notice, Options, terminal, EMPHASIS, SimpleTable } from "./kit";
import { findingMarks, StatusMark } from "./sessionview";
import { BytesOverTime, Chains, Chart, CostBy, Distribution, ErrorsOverTime, failureLegend, FirstByte, timed, Flows, focusOf, HALF, hasTail, Heat, KindsOverTime, Lanes, LatencyBand, type LegendItem, PrefixMeasure, type PrefixMeasureKind, PrefixUse, requestsOf, RequestScatter, SessionShape, shapeSentence, Sizes, Throughput, TimeSpent, ChartNote, LatencyHeat, PLOT, SHAPE, TICK_ROOM, faultMoments, latencyLegend, shapeLegend, ErrorRate, TrialOverClean } from "./plots";

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
  /** not drawn over time: reckoned again for the stretch chosen (and its sentence says which stretch) */
  stretched?: boolean;
  body: () => JSX.Element;
  /** what it draws, as rows: a CSV of it (each figure in one unit, named in its column) */
  rows?: () => ChartRows;
}

/** The chart a signal's tile opens. */
const SIGNAL_CHART: Record<SignalId, string> = { rate: "timeline", errors: "errors", latency: "latency", bytes: "bytes", inFlight: "lanes" };

/** What a status is called when S3 gave no code of its own for it: its HTTP name. */
const REASON: Record<number, string> = { 200: "OK", 201: "Created", 204: "No Content", 206: "Partial Content", 301: "Moved Permanently", 304: "Not Modified", 307: "Temporary Redirect", 400: "Bad Request", 403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed", 409: "Conflict", 411: "Length Required", 412: "Precondition Failed", 416: "Range Not Satisfiable", 429: "Too Many Requests", 500: "Internal Server Error", 501: "Not Implemented", 502: "Bad Gateway", 503: "Service Unavailable", 504: "Gateway Timeout" };

/** Every answer the session got, as a table: its status, what S3 (or HTTP)
 * calls it, how many, how many of them a fault scenario's (in the injected
 * tone), and when the first and the last came. Every row is there: past a
 * few the table scrolls, none is left out. */
function Statuses(props: { signals: Signals; /** where the requests it counts began, from the session's start (a stretch's) */ offsetMs?: number }) {
  const all = () => props.signals.statuses;
  const at = (ms: number) => `+${duration(ms + (props.offsetMs ?? 0))}`;
  const span = (x: Signals["statuses"][number]) => (!props.signals.timed ? "—" : x.count > 1 ? `${at(x.firstMs)} – ${at(x.lastMs)}` : at(x.firstMs));
  const called = (x: Signals["statuses"][number]) => x.code ?? (x.status === null ? t("statuses.noAnswer") : (REASON[x.status] ?? t("statuses.unnamed")));
  const injected = () => all().some((x) => x.injected > 0);
  const columns = () => [t("statuses.status"), t("statuses.code"), t("column.requests"), ...(injected() ? [t("statuses.injectedColumn")] : []), t("statuses.when")];
  const rows = () => all().map((x) => [x.status === null ? "—" : `${x.status}`, called(x), count(x.count), ...(injected() ? [count(x.injected)] : []), span(x)]);
  return (
    <View testID="statuses">
      <SimpleTable
        columns={columns()}
        rows={rows()}
        max={8}
        end={[2, ...(injected() ? [3] : [])]}
        fill={1}
        cell={(c) =>
          c.column === 0 ? (
            <StatusMark status={c.text === "—" ? null : Number(c.text)} code={c.cells[1]} />
          ) : injected() && c.column === 3 && c.text !== count(0) ? (
            // emphasis: answers that were sonde's own, not the store's
            <Text class={`tabular-nums text-small ${EMPHASIS} text-status-injected`}>{c.text}</Text>
          ) : undefined
        }
      />
    </View>
  );
}

/** A group of requests as the by-kind table reads it: a kind of request (or
 * a prefix, a size, a line of the program… where Group by splits by those). */
export interface GroupRow {
  name: string;
  requests: number;
  errorShare: number;
  p50Ms: number | null;
  p95Ms: number | null;
  p99Ms: number | null;
  /** Fewer than `FEW` requests: its percentiles are one of a handful, not a tail (`~`). */
  few: boolean;
  /** Its share of the program's waiting, and what its requests cost. */
  waitingShare: number | null;
  cost: number | null;
}

/** The one table of groups of requests: by kind of request on the Time pane
 * and under Group by alike — how many, what share failed, the median, p95 and
 * p99 (a percentile of fewer than `FEW` requests marked `~`), the share of the
 * waiting and the cost (its unit once, in its title). Sortable by a press on
 * a title; the last columns go first where it is narrow; with `onSelect` a
 * row is chosen (and pressed again, let go). */
export function GroupTable(props: { rows: GroupRow[]; title: string; currency: string; max?: number; selected?: string | null; onSelect?: (name: string | null) => void; testID?: string }) {
  const pc = (o: GroupRow, v: number | null) => (v == null ? "—" : `${o.few ? "~" : ""}${duration(v)}`);
  const costs = () => costColumn(props.rows.map((o) => o.cost ?? Number.NaN), props.currency);
  const hasWaiting = () => props.rows.some((o) => o.waitingShare != null);
  const hasCost = () => props.rows.some((o) => o.cost != null);
  const columns = () => [props.title, t("column.requests"), t("byOp.errors"), t("byOp.p50"), "p95", "p99", ...(hasWaiting() ? [t("column.waiting")] : []), ...(hasCost() ? [t("column.costIn", { unit: costs().unit })] : [])];
  const rows = () =>
    props.rows.map((o, i) => [
      o.name,
      count(o.requests),
      o.requests ? percent(o.errorShare) : "—",
      pc(o, o.p50Ms),
      pc(o, o.p95Ms),
      pc(o, o.p99Ms),
      ...(hasWaiting() ? [o.waitingShare == null ? "—" : percent(o.waitingShare)] : []),
      ...(hasCost() ? [costs().figures[i]] : []),
    ]);
  const at = () => (props.selected == null ? null : props.rows.findIndex((o) => o.name === props.selected));
  return (
    <View class="gap-2">
      <SimpleTable
        testID={props.testID}
        label={props.title}
        columns={columns()}
        rows={rows()}
        max={props.max ?? 8}
        end={columns().map((_, c) => c).slice(1)}
        fill={0}
        ranked={2}
        selected={at()}
        onSelect={props.onSelect ? (i) => props.onSelect!(i === null ? null : (props.rows[i]?.name ?? null)) : undefined}
        cell={(c) => (c.column === 2 && c.text !== "0%" && c.text !== "—" ? <Text class="tabular-nums text-small text-status-server">{c.text}</Text> : undefined)}
      />
      <If when={props.rows.some((o) => o.few)}>
        <ChartNote testID={props.testID ? `${props.testID}-few` : undefined}>{t("byOp.few", { n: FEW })}</ChartNote>
      </If>
    </View>
  );
}

/** A split's groups as the table's rows. */
export const groupRows = (s: Split): GroupRow[] =>
  s.rows.map((r) => ({
    name: r.other ? t("split.other") : r.key || "—",
    requests: r.requests,
    errorShare: r.errorShare,
    p50Ms: r.p50Ms,
    p95Ms: r.p95Ms,
    p99Ms: r.p99Ms,
    few: r.requests < FEW,
    waitingShare: r.share,
    cost: r.cost,
  }));

/** The run by kind of request, as a reader of dashboards expects it first:
 * Rust's figures for each kind, and from where the time went, each kind's
 * share of the waiting and its cost. */
function ByKind(props: { view: Pick<SessionView, "summary" | "profile">; selected?: string | null; onSelect?: (kind: string | null) => void }) {
  const rows = (): GroupRow[] => {
    const groups = props.view.profile.groups;
    const timed = props.view.profile.timed !== false;
    return kindStats(props.view).map((o) => {
      const mine = groups.filter((g) => g.kind === o.kind);
      return {
        name: o.kind,
        requests: o.count,
        errorShare: o.errorShare,
        p50Ms: o.p50Ms,
        p95Ms: o.p95Ms,
        p99Ms: o.p99Ms,
        few: o.few,
        waitingShare: timed && mine.length ? mine.reduce((n, g) => n + g.share, 0) : null,
        cost: mine.length ? mine.reduce((n, g) => n + g.cost, 0) : null,
      };
    });
  };
  return <GroupTable testID="by-operation" rows={rows()} title={t("column.kind")} currency={props.view.profile.cost.currency} selected={props.selected} onSelect={props.onSelect} />;
}

/** The Time pane of a session. */
export function TimePane(props: { /** what Group by splits the operations by, and the split read (the app's bridge): the table of groups follows it */ groupBy?: SplitBy | null; splitOf?: (by: SplitBy) => Promise<Split>; /** the kind of request chosen (the operations narrowed to it), and a press on a kind's row: the operations of that kind */ kind?: string | null; onKind?: (kind: string | null) => void; /** a chart's rows out as CSV, where the page can write a file (the app; a shared page cannot) */ onExport?: (name: string, csv: string) => void; view: SessionView; onRequest?: (id: number) => void; /** a stretch to open over (a link's), in ms from the session's start */ range?: Range | null; /** the stretch shown changed (null: the whole session): for a link to keep it */ onRange?: (r: Range | null) => void; /** the stretch chosen, as slices, when it is the session's one selection of time (the Operations strip's too); with `onStretch` the pane follows it and says its picks there */ stretch?: [number, number] | null; onStretch?: (s: [number, number] | null) => void; /** the program's budget in its project's sonde.toml: drawn as a line on the charts it is about */ budget?: ProjectBudget | null; /** a signal pressed in the header: its chart comes into view (`n` tells one press from the next) */ reveal?: { id: SignalId; n: number } | null; /** a trial's session: the clean run it is judged against, when that run is still there */ clean?: SessionView | null; /** the requests that started in a stretch, reckoned as a session (the app's bridge; a shared page has none, and says its charts not over time are of the whole session) */ stretchOf?: (fromMs: number, toMs: number) => Promise<Stretch> }) {
  const p = () => props.view.profile;
  // the four signals, as Rust counted them (an older share: what its profile holds of them)
  const sig = () => signalsOf(props.view)!;
  // the stretch of the session chosen: the session's one selection of time (the Operations strip
  // picks one slice of it, the timeline here two presses), or this pane's own on a shared page
  const own = signal<[number, number] | null>(null);
  const stretch = () => (props.onStretch ? (props.stretch ?? null) : own());
  const setStretch = (z: [number, number] | null) => (props.onStretch ? props.onStretch(z) : own.set(z));
  // the charts over time zoom to a stretch of two slices or more (one slice is no shape over time:
  // they would have nothing to draw, and mark it instead); every other chart is reckoned over any
  const zoom = () => {
    const z = stretch();
    return z && z[1] > z[0] ? z : null;
  };
  const one = () => {
    const z = stretch();
    return z && z[1] === z[0] ? z[0] : null;
  };
  const zoomFrom = signal<number | null>(null);
  const pick = (i: number | null) => {
    // pressing the first slice again lets it go: a stretch picked here is two slices at least
    if (i === null) return zoomFrom.set(null);
    const from = zoomFrom();
    if (from === i) return zoomFrom.set(null);
    if (from === null) return zoomFrom.set(i);
    batch(() => {
      setStretch([Math.min(from, i), Math.max(from, i)]);
      zoomFrom.set(null);
    });
  };
  const msOf = (z: [number, number] | null): [number, number] | null => (z ? [z[0] * p().binMs, (z[1] + 1) * p().binMs] : null);
  const zoomMs = () => msOf(zoom());
  const stretchMs = () => msOf(stretch());
  // a link's stretch, as this session's slices; and the stretch shown, told as it changes
  untracked(() => {
    const r = props.range ? slicesOf(props.range, p().binMs, p().rate.length) : null;
    if (r) setStretch(r);
  });
  effect(() => {
    const r = stretchMs();
    untracked(() => props.onRange?.(r));
  });
  // the selection: the requests that started in the stretch chosen, reckoned again for the charts not over time
  const held = resource(
    () => (props.stretchOf ? stretchMs() : null),
    async (r) => props.stretchOf!(r[0], r[1]),
  );
  const sel = (): Stretch | null => (stretchMs() && !held.error() ? (held() ?? null) : null);
  /** what the charts not over time read: the stretch's requests, or the session's */
  const pa = () => sel()?.profile ?? p();
  const sa = () => sel()?.signals ?? sig();
  const va = () => (sel() ? { summary: sel()!.summary, profile: sel()!.profile } : props.view);
  /** a chart reckoned for the stretch says which; a shared page, which reckons none, says it is of the whole session */
  const ofStretch = () => {
    const z = stretchMs();
    if (!z) return "";
    return props.stretchOf ? t("time.ofStretch", { a: duration(z[0]), b: duration(z[1]) }) : t("time.ofWhole");
  };
  const noneHeld = () => (sel() && sel()!.summary.requests === 0 ? t("plots.noneInStretch") : null);
  // Group by, as the Operations pane has it: the table of groups follows it (by kind of request, the default)
  const grouped = () => (props.groupBy && props.groupBy !== "kind" && props.splitOf ? props.groupBy : null);
  const split = resource(
    () => grouped(),
    (by) => props.splitOf!(by),
  );
  const latencyAs = signal<"bands" | "heat">("bands");
  // when sonde answered in the store's place (a trial's session), on the timeline itself
  const faults = () => faultMoments(p(), props.view.injected);
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
    if (binMs !== undefined && b !== binMs) untracked(() => batch(() => (setStretch(null), zoomFrom.set(null))));
    binMs = b;
  });
  // the moment the pointer rests on in one chart, marked in the others: this pane's own
  const focus = focusOf();
  // above what a session keeps of its requests, a chart of single requests is of a sample: said
  const sampled = () => (p().samples.length < requestsOf(p()) ? ` · ${t("plots.sampled", { n: count(p().samples.length), of: count(requestsOf(p())) })}` : "");
  const graphic = () => !terminal();
  const overTime = () => timed(p()) && shaped();
  const failed = () => sig().errors.count + sig().errors.retriesTotal > 0;
  // the budget's lines, and how far the run is from each (a share of the run's requests for its errors)
  const p95Budget = () => props.budget?.maxP95Ms ?? null;
  const errorBudget = () => (props.budget?.maxErrors != null && sig().rate.requests > 0 ? props.budget.maxErrors / sig().rate.requests : null);
  const BUDGET_SAID = { p95: ["budget.p95", "budget.p95.over"], errors: ["budget.errors", "budget.errors.over"] } as const;
  const budgetSaid = (what: keyof typeof BUDGET_SAID, over: boolean, vars: Record<string, string>) => ` ${t(BUDGET_SAID[what][over ? 1 : 0], vars)}`;
  const charts: ChartSpec[] = [
    // --- traffic: when, how many at once, how much
    {
      // a trial's session: the run itself over the clean run it is judged against, the faults' moments marked
      id: "trial",
      group: "traffic",
      wide: true,
      when: () => graphic() && overTime() && !!props.clean && timed(props.clean.profile) && props.clean.profile.rate.length > 2,
      title: () => t("trialOver.title"),
      text: () => t("trialOver.hint"),
      legend: () => [{ label: t("trialOver.trial"), swatch: "bg-chart-1" }, { label: t("trialOver.clean"), swatch: "bg-muted-foreground" }, ...(faults().length ? [{ label: t("trialOver.faults"), swatch: "bg-status-injected" }] : [])],
      body: () => <TrialOverClean trial={p()} clean={props.clean!.profile} faults={faults()} domainMs={zoomMs()} />,
    },
    {
      // the whole session, its findings marked; the stretch picked here is what every chart follows
      id: "timeline",
      group: "traffic",
      wide: true,
      when: () => graphic() && shaped(),
      title: () => t("timeline.title"),
      text: () => (stretch() ? t("timeline.zoomed", { a: duration(stretchMs()![0]), b: duration(stretchMs()![1]) }) : zoomFrom() !== null ? t("timeline.zoomFrom", { t: duration((zoomFrom() ?? 0) * p().binMs) }) : t("timeline.hint")),
      action: () => (
        <If when={zoomFrom() !== null || (stretch() && !props.onStretch)}>
          {/* a first slice pressed, let go (the stretch chosen is cleared from its chip, above the pane) */}
          <Button small variant="outline" icon="x" label={t("timeline.unzoom")} onPress={() => batch(() => (zoomFrom.set(null), !props.onStretch && own.set(null)))} testID="zoom-reset" />
        </If>
      ),
      // every colour of the stack, the line, and what the numbers and rules over the slices are
      legend: () => [...shapeLegend(p(), findingMarks(props.view).length > 0), ...(faults().length ? [{ label: t("trialOver.faults"), swatch: "bg-status-injected" }] : [])],
      rows: () => ({ columns: ["ms", "ok", "not_found", "client_error", "server_error", "p50_ms", "p95_ms"], rows: p().rate.map((b) => [b.tMs, b.ok, b.miss, b.client, b.server, b.p50Ms, b.p95Ms]) }),
      body: () => <SessionShape profile={p()} annotate axis marks={findingMarks(props.view)} faults={faults()} range={zoom()} slice={zoomFrom() ?? one()} onSlice={pick} focus={focus} peak={sig().inFlight.peak} class={`${TICK_ROOM} mt-4 ${SHAPE} w-full`} testID="time-shape" />,
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
      rows: () => ({ columns: ["ms", "sent_bytes_per_s", "received_bytes_per_s"], rows: sig().bytes.up.map((u, i) => [i * sig().binMs, u, sig().bytes.down[i] ?? 0]) }),
      body: () => <BytesOverTime signals={sig()} range={zoom()} focus={focus} />,
    },
    {
      // how large the bodies were, or how fast each size moved
      id: "sizes",
      group: "traffic",
      stretched: true,
      when: () => sig().sizes.request.length + sig().sizes.response.length > 1,
      title: () => t("sizes.title"),
      text: () => (sizesBy() === "speed" ? t("sizes.hintSpeed") : t("sizes.hint")),
      action: () => (
        <If when={sig().sizes.throughput.length > 0}>
          <Options testID="sizes-by" value={sizesBy()} items={[{ value: "count", title: t("sizes.byCount") }, { value: "speed", title: t("sizes.bySpeed") }]} onChange={(v) => sizesBy.set(v as "count" | "speed")} />
        </If>
      ),
      legend: () => (sizesBy() === "speed" ? [] : [{ label: t("sizes.sent"), swatch: "bg-chart-1" }, { label: t("sizes.received"), swatch: "bg-chart-4" }]),
      rows: () => ({ columns: ["up_to_bytes", "sent", "received"], rows: [...new Set([...sa().sizes.request, ...sa().sizes.response].map((b) => b.leBytes))].sort((a, b) => a - b).map((le) => [le, sa().sizes.request.find((b) => b.leBytes === le)?.count ?? 0, sa().sizes.response.find((b) => b.leBytes === le)?.count ?? 0]) }),
      body: () => (
        <If when={sizesBy() === "speed" && sa().sizes.throughput.length > 0} else={<Sizes signals={sa()} />}>
          <Throughput signals={sa()} />
        </If>
      ),
    },
    // --- latency: which kinds of request were slow, how long, where the time went
    {
      // the run by kind of request: count, error share, median, p95, p99, waiting, cost — the table a reader of dashboards looks for
      id: "operations",
      group: "latency",
      wide: true,
      stretched: true,
      when: () => kindStats(props.view).length > 0,
      title: () => t("byOp.title"),
      text: () => t("byOp.hint"),
      body: () => <ByKind view={va()} selected={props.kind} onSelect={props.onKind} />,
    },
    {
      // the same table, by what Group by splits the operations by (a prefix, a size, a line of the program…):
      // the session's whole, read where the groups are reckoned (said so when a stretch is chosen)
      id: "groups",
      group: "latency",
      wide: true,
      when: () => !!grouped(),
      title: () => t(`byGroup.${grouped()!}` as never),
      text: () => [t("byGroup.hint"), stretch() ? t("time.ofWhole") : ""].filter(Boolean).join(" · "),
      body: () => (
        <Await loading={split.loading() && !split()} error={split.error()} retry={() => split.refetch()}>
          <If when={split()}>{(x) => <GroupTable testID="by-group" rows={groupRows(x())} title={t(`split.by.${x().by}`)} currency={x().currency} />}</If>
        </Await>
      ),
    },
    {
      id: "latency",
      group: "latency",
      when: () => graphic() && overTime(),
      title: () => t("latencyTime.title"),
      // (no slice with a tail: said, rather than a band named and not drawn)
      text: () =>
        (latencyAs() === "heat" ? `${t("latencyHeat.hint")}${sampled()}` : hasTail(p()) ? t("latencyTime.hint") : t("latencyTime.hintNoTail")) +
        (p95Budget() != null && sig().latency.p95Ms != null ? budgetSaid("p95", sig().latency.p95Ms! > p95Budget()!, { v: duration(sig().latency.p95Ms!), budget: duration(p95Budget()!) }) : ""),
      // the bands are of every request; the grid shows what a band averages away (two modes, a slow few)
      action: () => <Options testID="latency-as" value={latencyAs()} items={[{ value: "bands", title: t("latencyTime.bands") }, { value: "heat", title: t("latencyTime.heat") }]} onChange={(v) => latencyAs.set(v as "bands" | "heat")} />,
      legend: () => (latencyAs() === "heat" ? [{ label: t("latencyHeat.scale"), swatch: "bg-heat-high" }] : latencyLegend(p())),
      rows: () => ({ columns: ["ms", "requests", "p50_ms", "p95_ms", "p99_ms"], rows: p().rate.map((b) => [b.tMs, b.ok + b.miss + b.client + b.server, b.p50Ms, b.p95Ms, b.p99Ms]) }),
      body: () => (
        <If when={latencyAs() === "heat"} else={<LatencyBand profile={p()} domainMs={zoomMs()} focus={focus} class={`${TICK_ROOM} ${PLOT} w-full`} testID="latency-over-time" axis budgetMs={p95Budget()} />}>
          <LatencyHeat profile={p()} range={zoom()} focus={focus} />
        </If>
      ),
    },
    {
      // every request, when it ran or how large it was (a press opens it), or the busiest kinds each over time
      id: "scatter",
      group: "latency",
      when: () => graphic() && timed(p()) && p().samples.length > 1,
      title: () => t("scatter.title"),
      // above what a session keeps of its requests, the dots are of a sample: said
      text: () => (scatterBy() === "kinds" ? t("kinds.title") : `${t("scatter.text", { n: shown(), p95: duration(p().p95Ms ?? 0) })}${sampled()}`),
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
      stretched: true,
      when: () => timed(p()),
      title: () => t("profile.spentTitle"),
      text: () => t("profile.spentHint"),
      action: () => (
        <If when={(p().callers?.length ?? 0) > 0}>
          <Options testID="spent-by" value={spentBy()} items={[{ value: "kind", title: t("spent.byKind") }, { value: "code", title: t("spent.byCode") }]} onChange={(v) => spentBy.set(v as "kind" | "code")} />
        </If>
      ),
      body: () => <TimeSpent profile={pa()} by={spentBy()} />,
    },
    { id: "distribution", group: "latency", stretched: true, when: () => !!p().latency?.length, title: () => t("dist.title"), text: () => t("dist.hint"), body: () => <Distribution profile={pa()} /> },
    {
      // the wait for the first byte against the rest: only where a first byte was timed
      id: "firstByte",
      group: "latency",
      stretched: true,
      when: () => !!sig().firstByte?.kinds.length,
      title: () => t("firstByte.title"),
      text: () => t("firstByte.hint"),
      legend: () => [{ label: t("firstByte.first"), swatch: "bg-chart-1" }, { label: t("firstByte.rest"), swatch: "bg-chart-4" }],
      body: () => <FirstByte signals={sa()} />,
    },
    { id: "chains", group: "latency", when: () => !!p().chains?.length, title: () => t("chains.title"), text: () => t("chains.hint"), body: () => <Chains profile={p()} domainMs={zoomMs()} /> },
    // --- errors: when they failed, what share, and every answer the store gave
    {
      id: "errors",
      group: "errors",
      when: () => failed() && (shaped() || terminal()),
      title: () => t("failures.title"),
      text: () => t("failures.hint"),
      legend: () => failureLegend(sig()),
      rows: () => ({ columns: ["ms", "client", "server", "transport", "injected", "retries"], rows: sig().errors.series.map((_, i) => [i * sig().binMs, sig().errors.client[i] ?? 0, sig().errors.server[i] ?? 0, sig().errors.transport[i] ?? 0, sig().errors.injected[i] ?? 0, sig().errors.retries[i] ?? 0]) }),
      body: () => <ErrorsOverTime signals={sig()} range={zoom()} focus={focus} />,
    },
    {
      // the rate beside the counts: the share of each slice's requests that failed
      id: "errorRate",
      group: "errors",
      when: () => graphic() && sig().errors.count > 0 && shaped() && timed(p()),
      title: () => t("errorRate.title"),
      text: () => t("errorRate.hint") + (props.budget?.maxErrors != null ? budgetSaid("errors", sig().errors.count > props.budget.maxErrors, { n: count(sig().errors.count), budget: count(props.budget.maxErrors) }) : ""),
      rows: () => ({ columns: ["ms", "requests", "failed"], rows: sig().errors.series.map((x, i) => [i * sig().binMs, Math.round(((sig().rate.series[i] ?? 0) * sig().binMs) / 1000), x]) }),
      body: () => <ErrorRate signals={sig()} range={zoom()} focus={focus} budgetShare={errorBudget()} />,
    },
    { id: "statuses", group: "errors", stretched: true, when: () => sig().statuses.length > 0, title: () => t("statuses.title"), text: () => t("statuses.hint"), body: () => <Statuses signals={sa()} offsetMs={sel()?.offsetMs} /> },
    // --- where: which prefixes, at what cost, on which path
    {
      // where by prefix: how many, what they cost, how near the limit
      id: "prefixes",
      group: "where",
      stretched: true,
      when: () => p().prefixes.some((x) => x.prefix !== "other"),
      title: () => t("prefixUse.title"),
      text: () => t("prefixUse.hint"),
      action: () => <PrefixMeasure profile={pa()} value={measure()} onChange={(m) => measure.set(m)} />,
      body: () => <PrefixUse profile={pa()} measure={measure()} rates={sa().prefixes} />,
    },
    { id: "cost", group: "where", stretched: true, when: () => sig().cost.kinds.some((x) => x.cost > 0), title: () => t("costBy.title"), text: () => t("costBy.hint"), body: () => <CostBy signals={sa()} /> },
    { id: "heat", group: "where", when: () => !!p().heat?.length, title: () => t("heat.title"), text: () => t("heat.hint"), body: () => <Heat profile={p()} range={zoom()} /> },
    { id: "flows", group: "where", wide: true, stretched: true, testID: "flows-chart", when: () => graphic() && !!p().flows?.length, title: () => t("flows.title"), text: () => t("flows.hint"), body: () => <Flows profile={pa()} /> },
  ];
  const of = (g: Group) => charts.filter((c) => c.group === g && c.when());
  /** a chart's sentence, and for one reckoned again for the stretch, which stretch */
  const said = (c: ChartSpec) => {
    const text = c.text?.();
    const stretch = c.stretched ? ofStretch() : "";
    return stretch ? (text ? `${text} · ${stretch}` : stretch) : text;
  };
  // a signal pressed scrolls its chart into view (Native Shell's `reveal`, from the chart's own ref)
  const frames: Record<string, ViewRef> = {};
  // the chart asked for before the board was there: it shows itself as it comes
  let wanted: string | null = null;
  const show = (id: string) => {
    wanted = frames[id] ? null : id;
    frames[id]?.reveal({ vertical: "start" });
  };
  let revealed = -1;
  effect(() => {
    const r = props.reveal;
    if (!r || r.n === revealed) return;
    revealed = r.n;
    const chart = charts.find((c) => c.id === SIGNAL_CHART[r.id]);
    // an error-free session has no errors chart: its answers, then
    const target = chart && untracked(chart.when) ? chart : charts.find((c) => c.group === (chart?.group ?? "traffic") && untracked(c.when));
    if (target) show(target.id);
  });
  return (
    <Scroll testID="time-pane" class="min-h-0 flex-1" contentClass="gap-6 pb-4 terminal:gap-2 terminal:pb-0">
      {/* the figures are the session header's; here, a terminal's sentence */}
      <If when={terminal() && timed(p())}>
        <Text class="text-foreground">{shapeSentence(p())}</Text>
      </If>
      {/* a log without times (CloudTrail): when and what, never how long */}
      <If when={!timed(p())}>
        <Notice testID="untimed" text={t("profile.untimed")} />
      </If>
      {/* the board: the four groups in their reading order, every chart in the one frame, by one loop */}
      <Each of={GROUPS.filter((g) => of(g).length > 0)} key={(g) => g}>
        {(g) => (
          <View testID={`board-${g()}`} class="gap-3 terminal:gap-2">
            <Heading>{t(`board.${g()}`)}</Heading>
            {/* two abreast where there is room (a last one alone takes the row); a card is as high as its content */}
            <View class="flex-row flex-wrap items-start gap-4 terminal:flex-col terminal:flex-nowrap terminal:items-stretch terminal:gap-2">
              <Each of={of(g())} key={(c) => c.id}>
                {(c) => (
                  <View
                    ref={(r) => {
                      frames[c().id] = r;
                      if (wanted === c().id) show(wanted);
                    }}
                    class={terminal() ? "" : c().wide ? "w-full" : HALF}>
                    <Chart testID={c().testID ?? `chart-${c().id}`} title={c().title()} text={said(c())} action={c().action?.()} legend={c().legend?.()} empty={c().stretched ? noneHeld() : null} onExport={props.onExport && c().rows ? () => props.onExport!(c().id, toCsv(c().rows!())) : undefined}>
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
