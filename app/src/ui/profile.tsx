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
import { Await, Button, DOT, Heading, Notice, Options, terminal, EMPHASIS, SimpleTable, TEXT, TONE } from "./kit";
import { findingMarks, StatusMark } from "./sessionview";
import { BytesOverTime, Chains, Chart, REASON, began, sizesSaid, throughputSaid, chainsSaid, prefixSaid, timelineSaid, lanesSaid, bytesSaid, groupsSaid, latencySaid, heatLatencySaid, scatterSaid, sizeSaid, spentSaid, distSaid, failuresSaid, errorRateSaid, statusesSaid, costSaid, heatSaid, flowsSaid, CostBy, Distribution, ErrorsOverTime, failureLegend, FirstByte, timed, Flows, focusOf, HALF, hasTail, Heat, Lanes, LatencyBand, type LegendItem, PrefixUse, requestsOf, RequestScatter, SessionShape, shapeSentence, Sizes, Throughput, TimeSpent, ChartNote, LatencyHeat, PLOT, SHAPE, TICK_ROOM, faultMoments, latencyLegend, shapeLegend, ErrorRate, TrialOverClean, trialLegend, bytesLegend, sizesLegend, sliceAt, slotMs, heatLegend, trialSaid } from "./plots";

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
  /** what it shows, as an answer to its title: computed from what it draws */
  text?: () => string;
  /** how to read it: the frame's info mark */
  how?: () => string;
  action?: () => JSX.Element;
  legend?: () => LegendItem[];
  testID?: string;
  /** not drawn over time: reckoned again for the stretch chosen (and its sentence says which stretch) */
  stretched?: boolean;
  /** drawn over time: zoomed to a stretch of two slices or more (and its sentence, of the zoom, says which) */
  zooms?: boolean;
  body: () => JSX.Element;
  /** what it draws, as rows: a CSV of it (each figure in one unit, named in its column) */
  rows?: () => ChartRows;
}

/** The chart a signal's tile opens. */
const SIGNAL_CHART: Record<SignalId, string> = { rate: "timeline", errors: "errors", latency: "latency", bytes: "bytes", inFlight: "lanes" };


/** Every answer the session got, as a table: its status, what S3 (or HTTP)
 * calls it, how many, how many of them a fault scenario's (in the injected
 * tone), and when the first and the last came. Every row is there: past a
 * few the table scrolls, none is left out. */
function Statuses(props: { signals: Signals }) {
  const all = () => props.signals.statuses;
  const at = (ms: number) => `+${duration(ms)}`;
  const span = (x: Signals["statuses"][number]) => (!props.signals.timed ? "—" : x.count > 1 ? `${at(x.firstMs)} – ${at(x.lastMs)}` : at(x.firstMs));
  const called = (x: Signals["statuses"][number]) => x.code ?? (x.status === null ? t("statuses.noAnswer") : (REASON[x.status] ?? t("statuses.unnamed")));
  const injected = () => all().some((x) => x.injected > 0);
  const columns = () => [t("statuses.status"), t("statuses.code"), t("column.requests"), ...(injected() ? [t("statuses.injectedColumn")] : []), t("statuses.when")];
  const rows = () => all().map((x) => [x.status === null ? "—" : `${x.status}`, called(x), count(x.count), ...(injected() ? [count(x.injected)] : []), span(x)]);
  return (
    <View testID="statuses">
      <SimpleTable
        bare
        columns={columns()}
        rows={rows()}
        max={8}
        end={[2, ...(injected() ? [3] : [])]}
        fill={1}
        cell={(c) =>
          c.column === 0 ? (
            <StatusMark status={c.text === "—" ? null : Number(c.text)} code={c.cells[1]} />
          ) : injected() && c.column === 3 && c.text !== count(0) ? (
            // answers that were sonde's own, not the store's: marked by their tone's dot
            <View class="flex-1 flex-row items-center justify-end gap-1">
              <View class={`${DOT} ${TONE.injected.dot}`} />
              <Text class={`tabular-nums ${TEXT.small}`}>{c.text}</Text>
            </View>
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
        bare
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
        // a share that failed: its dot in the failure's tone, the figure read as every other
        cell={(c) =>
          c.column === 2 && c.text !== "0%" && c.text !== "—" ? (
            <View class="flex-1 flex-row items-center justify-end gap-1">
              <View class={`${DOT} ${TONE.fail.dot}`} />
              <Text class={`tabular-nums ${TEXT.small}`}>{c.text}</Text>
            </View>
          ) : undefined
        }
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
export function TimePane(props: { /** whether the board is scrolled from its top (the session's header folds while it is) */ onScrolled?: (away: boolean) => void; /** what Group by splits the operations by, and the split read (the app's bridge): the table of groups follows it */ groupBy?: SplitBy | null; splitOf?: (by: SplitBy) => Promise<Split>; /** the kind of request chosen (the operations narrowed to it), and a press on a kind's row: the operations of that kind */ kind?: string | null; onKind?: (kind: string | null) => void; /** a chart's rows out as CSV, where the page can write a file (the app; a shared page cannot) */ onExport?: (name: string, csv: string) => void; view: SessionView; onRequest?: (id: number) => void; /** a failure bar pressed: the operations that failed in its slice (from, to: ms from the session's start) */ onFailures?: (ms: [number, number]) => void; /** a stretch to open over (a link's), in ms from the session's start */ range?: Range | null; /** the stretch shown changed (null: the whole session): for a link to keep it */ onRange?: (r: Range | null) => void; /** the stretch chosen, as slices, when it is the session's one selection of time (the Operations strip's too); with `onStretch` the pane follows it and says its picks there */ stretch?: [number, number] | null; onStretch?: (s: [number, number] | null) => void; /** the program's budget in its project's sonde.toml: drawn as a line on the charts it is about */ budget?: ProjectBudget | null; /** a signal pressed in the header: its chart comes into view (`n` tells one press from the next) */ reveal?: { id: SignalId; n: number } | null; /** a trial's session: the clean run it is judged against, when that run is still there */ clean?: SessionView | null; /** the requests that started in a stretch, reckoned as a session (the app's bridge; a shared page has none, and says its charts not over time are of the whole session) */ stretchOf?: (fromMs: number, toMs: number) => Promise<Stretch> }) {
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
    const [a, b] = [Math.min(from, i), Math.max(from, i)];
    batch(() => {
      // a stretch where no request began would dim every chart for nothing: it does not become the stretch
      if (began(p(), a, b)) setStretch([a, b]);
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
  // the charts over time, zoomed: the stretch's own profile and signals, its slices its own (finer: real detail, not
  // the session's few wide slices), once reckoned; until then, and on a shared page (which reckons none), the
  // session's slices over the stretch
  const fine = () => (zoom() && sel() && sel()!.summary.requests > 0 ? sel() : null);
  const zp = () => fine()?.profile ?? p();
  const zs = () => fine()?.signals ?? sig();
  /** the zoom, as slices of what the charts over time draw: none once they draw the stretch's own */
  const zr = () => (fine() ? null : zoom());
  /** a slice of what the timeline draws, as the session's (a stretch is picked in the session's slices) */
  const toSession = (i: number) => (fine() ? Math.min(p().rate.length - 1, sliceAt(p(), zp().rate[i]?.tMs ?? 0)) : i);
  const fromSession = (i: number | null) => (i === null || !fine() ? i : sliceAt(zp(), slotMs(p(), i)));
  // Group by, as the Operations pane has it: the table of groups follows it (by kind of request, the default)
  const grouped = () => (props.groupBy && props.groupBy !== "kind" && props.splitOf ? props.groupBy : null);
  const split = resource(
    () => grouped(),
    (by) => props.splitOf!(by),
  );
  // when sonde answered in the store's place (a trial's session), on the timeline itself
  const faults = () => faultMoments(p(), props.view.injected);
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
  const sampled = () => (zp().samples.length < requestsOf(zp()) ? ` · ${t("plots.sampled", { n: count(zp().samples.length), of: count(requestsOf(zp())) })}` : "");
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
      zooms: true,
      when: () => graphic() && overTime() && !!props.clean && timed(props.clean.profile) && props.clean.profile.rate.length > 2,
      title: () => t("trialOver.title"),
      text: () => trialSaid(sig(), faults().length, p().wallMs, props.clean!.profile.wallMs),
      how: () => t("trialOver.hint"),
      legend: () => trialLegend(faults().length > 0),
      body: () => <TrialOverClean trial={p()} clean={props.clean!.profile} faults={faults()} domainMs={zoomMs()} />,
    },
    {
      // the whole session, its findings marked; the stretch picked here is what every chart follows
      id: "timeline",
      group: "traffic",
      wide: true,
      zooms: true,
      when: () => graphic() && shaped(),
      title: () => t("timeline.title"),
      // what the slices shown hold; a first slice pressed asks where to end
      text: () => [timelineSaid(zp(), zr()), zoomFrom() !== null ? t("timeline.zoomFrom", { t: duration((zoomFrom() ?? 0) * p().binMs) }) : ""].filter(Boolean).join(" · "),
      how: () => t("timeline.hint"),
      action: () => (
        <If when={zoomFrom() !== null || (stretch() && !props.onStretch)}>
          {/* a first slice pressed, let go (the stretch chosen is cleared from its chip, above the pane) */}
          <Button small variant="outline" icon="x" label={t("timeline.unzoom")} onPress={() => batch(() => (zoomFrom.set(null), !props.onStretch && own.set(null)))} testID="zoom-reset" />
        </If>
      ),
      // every colour of the stack, the line, and what the numbers and rules over the slices are
      legend: () => [...shapeLegend(p(), findingMarks(props.view).length > 0, props.view.injected), ...(faults().length ? [{ label: t("trialOver.faults"), swatch: "bg-status-injected" }] : [])],
      rows: () => ({ columns: ["ms", "ok", "not_found", "client_error", "server_error", "p50_ms", "p95_ms"], rows: zp().rate.map((b) => [b.tMs, b.ok, b.miss, b.client, b.server, b.p50Ms, b.p95Ms]) }),
      body: () => <SessionShape profile={zp()} annotate axis marks={findingMarks(props.view)} faults={faults()} injected={props.view.injected} range={zr()} slice={fromSession(zoomFrom() ?? one())} onSlice={(i) => pick(i === null ? null : toSession(i))} focus={focus} peak={fine() ? zp().peakInFlight : sig().inFlight.peak} class={`${TICK_ROOM} mt-4 ${SHAPE} w-full`} testID="time-shape" />,
    },
    {
      // what ran at once, what waited: each request on its lane
      id: "lanes",
      group: "traffic",
      zooms: true,
      testID: "lanes-chart",
      when: () => graphic() && timed(p()) && p().samples.length > 1,
      title: () => t("lanes.title"),
      text: () => lanesSaid(zp(), zoomMs()),
      how: () => t("lanes.hint"),
      body: () => <Lanes profile={zp()} domainMs={zoomMs()} onRequest={props.onRequest} focus={focus} injected={props.view.injected} />,
    },
    {
      // bodies a second, each way
      id: "bytes",
      group: "traffic",
      zooms: true,
      when: () => overTime() && sig().bytes.upBytes + sig().bytes.downBytes > 0,
      title: () => t("bytesTime.title"),
      text: () => bytesSaid(zs(), zr()),
      how: () => t("bytesTime.hint"),
      legend: () => bytesLegend(zs()),
      rows: () => ({ columns: ["ms", "sent_bytes_per_s", "received_bytes_per_s"], rows: zs().bytes.up.map((u, i) => [slotMs(zs(), i), u, zs().bytes.down[i] ?? 0]) }),
      body: () => <BytesOverTime signals={zs()} range={zr()} focus={focus} />,
    },
    {
      // how large the bodies were
      id: "sizes",
      group: "traffic",
      stretched: true,
      when: () => sig().sizes.request.length + sig().sizes.response.length > 1,
      title: () => t("sizes.title"),
      text: () => sizesSaid(sa()),
      how: () => t("sizes.hint"),
      legend: () => sizesLegend(sa()),
      rows: () => ({ columns: ["up_to_bytes", "sent", "received"], rows: [...new Set([...sa().sizes.request, ...sa().sizes.response].map((b) => b.leBytes))].sort((a, b) => a - b).map((le) => [le, sa().sizes.request.find((b) => b.leBytes === le)?.count ?? 0, sa().sizes.response.find((b) => b.leBytes === le)?.count ?? 0]) }),
      body: () => <Sizes signals={sa()} />,
    },
    {
      // how fast bodies of each size moved: the median, by kind of request
      id: "throughput",
      group: "traffic",
      stretched: true,
      when: () => sig().sizes.throughput.length > 0,
      title: () => t("throughput.title"),
      text: () => throughputSaid(sa()),
      how: () => t("sizes.hintSpeed"),
      body: () => <Throughput signals={sa()} />,
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
      text: () => groupsSaid(kindStats(va()).map((o) => ({ name: o.kind, requests: o.count, errorShare: o.errorShare, p95Ms: o.p95Ms, few: o.few }))),
      how: () => t("byOp.hint"),
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
      text: () => [split() ? groupsSaid(groupRows(split()!)) : "", stretch() ? t("time.ofWhole") : ""].filter(Boolean).join(" · "),
      how: () => t("byGroup.hint"),
      body: () => (
        <Await loading={split.loading() && !split()} error={split.error()} retry={() => split.refetch()}>
          <If when={split()}>{(x) => <GroupTable testID="by-group" rows={groupRows(x())} title={t(`split.by.${x().by}`)} currency={x().currency} />}</If>
        </Await>
      ),
    },
    {
      id: "latency",
      group: "latency",
      zooms: true,
      when: () => graphic() && overTime(),
      title: () => t("latencyTime.title"),
      text: () => latencySaid(zp(), zr()) + (p95Budget() != null && sig().latency.p95Ms != null ? budgetSaid("p95", sig().latency.p95Ms! > p95Budget()!, { v: duration(sig().latency.p95Ms!), budget: duration(p95Budget()!) }) : ""),
      // (no slice with a tail: said, rather than a band named and not drawn)
      how: () => (hasTail(zp()) ? t("latencyTime.hint") : t("latencyTime.hintNoTail")),
      legend: () => latencyLegend(zp()),
      rows: () => ({ columns: ["ms", "requests", "p50_ms", "p95_ms", "p99_ms"], rows: zp().rate.map((b) => [b.tMs, b.ok + b.miss + b.client + b.server, b.p50Ms, b.p95Ms, b.p99Ms]) }),
      body: () => <LatencyBand profile={zp()} domainMs={zoomMs()} focus={focus} class={`${TICK_ROOM} ${PLOT}`} testID="latency-over-time" axis budgetMs={p95Budget()} />,
    },
    {
      // the same latencies as a grid: what a band averages away (two modes, a slow few)
      id: "latencyHeat",
      group: "latency",
      zooms: true,
      testID: "latency-heat-chart",
      when: () => graphic() && overTime(),
      title: () => t("latencyHeat.title"),
      text: () => heatLatencySaid(zp(), zr()),
      how: () => t("latencyHeat.hint"),
      legend: () => heatLegend(zp().latencyHeat.filter((c) => !zr() || (c.bin >= zr()![0] && c.bin <= zr()![1])).map((c) => c.n)),
      body: () => <LatencyHeat profile={zp()} range={zr()} focus={focus} />,
    },
    {
      // every request, when it ran (a press opens it)
      id: "scatter",
      group: "latency",
      zooms: true,
      when: () => graphic() && timed(p()) && p().samples.length > 1,
      title: () => t("scatter.title"),
      // above what a session keeps of its requests, the dots are of a sample: said
      text: () => `${scatterSaid(zp(), zoomMs())}${sampled()}`,
      how: () => t("scatter.hint"),
      body: () => <RequestScatter profile={zp()} by="time" domainMs={zoomMs()} onRequest={props.onRequest} injected={props.view.injected} />,
    },
    {
      // every request, by how large it was (a press opens it)
      id: "scatterSize",
      group: "latency",
      zooms: true,
      testID: "scatter-size-chart",
      when: () => graphic() && timed(p()) && p().samples.some((x) => (x.bytes ?? 0) > 0),
      title: () => t("scatterSize.title"),
      text: () => `${sizeSaid(zp(), zoomMs())}${sampled()}`,
      how: () => t("scatterSize.hint"),
      body: () => <RequestScatter profile={zp()} by="size" domainMs={zoomMs()} onRequest={props.onRequest} injected={props.view.injected} />,
    },
    {
      id: "spent",
      group: "latency",
      stretched: true,
      when: () => timed(p()),
      title: () => t("profile.spentTitle"),
      text: () => spentSaid(pa()),
      how: () => t("profile.spentHint"),
      body: () => <TimeSpent profile={pa()} />,
    },
    { id: "distribution", group: "latency", stretched: true, when: () => !!p().latency?.length, title: () => t("dist.title"), text: () => distSaid(pa()), how: () => t("dist.hint"), body: () => <Distribution profile={pa()} /> },
    {
      // the wait for the first byte against the rest: only where a first byte was timed
      id: "firstByte",
      group: "latency",
      stretched: true,
      when: () => !!sig().firstByte?.kinds.length,
      title: () => t("firstByte.title"),
      text: () => (sa().firstByte ? t("firstByte.text", { pct: percent(sa().firstByte!.share), n: count(sa().firstByte!.count) }) : ""),
      how: () => t("firstByte.hint"),
      legend: () => [{ label: t("firstByte.first"), swatch: "bg-chart-1" }, { label: t("firstByte.rest"), swatch: "bg-chart-4" }],
      body: () => <FirstByte signals={sa()} />,
    },
    { id: "chains", group: "latency", when: () => !!p().chains?.length, title: () => t("chains.title"), text: () => chainsSaid(p()), how: () => t("chains.hint"), body: () => <Chains profile={p()} domainMs={zoomMs()} /> },
    // --- errors: when they failed, what share, and every answer the store gave
    {
      id: "errors",
      group: "errors",
      zooms: true,
      when: () => failed() && (shaped() || terminal()),
      title: () => t("failures.title"),
      text: () => failuresSaid(zs(), zr(), sa()),
      how: () => t("failures.hint"),
      legend: () => failureLegend(zs()),
      rows: () => ({ columns: ["ms", "client", "server", "throttled", "transport", "injected", "retries"], rows: zs().errors.series.map((_, i) => [slotMs(zs(), i), zs().errors.client[i] ?? 0, zs().errors.server[i] ?? 0, zs().errors.throttled[i] ?? 0, zs().errors.transport[i] ?? 0, zs().errors.injected[i] ?? 0, zs().errors.retries[i] ?? 0]) }),
      body: () => <ErrorsOverTime signals={zs()} range={zr()} focus={focus} onFailures={props.onFailures ? (ms) => props.onFailures!(ms) : undefined} />,
    },
    {
      // the rate beside the counts: the share of each slice's requests that failed
      id: "errorRate",
      group: "errors",
      zooms: true,
      when: () => graphic() && sig().errors.count > 0 && shaped() && timed(p()),
      title: () => t("errorRate.title"),
      text: () => errorRateSaid(zs(), zr()) + (props.budget?.maxErrors != null ? budgetSaid("errors", sig().errors.count > props.budget.maxErrors, { n: count(sig().errors.count), budget: count(props.budget.maxErrors) }) : ""),
      how: () => t("errorRate.hint"),
      rows: () => ({ columns: ["ms", "requests", "failed"], rows: zs().errors.series.map((x, i) => [slotMs(zs(), i), Math.round(((zs().rate.series[i] ?? 0) * zs().binMs) / 1000), x]) }),
      body: () => <ErrorRate signals={zs()} range={zr()} focus={focus} budgetShare={errorBudget()} />,
    },
    { id: "statuses", group: "errors", stretched: true, when: () => sig().statuses.length > 0, title: () => t("statuses.title"), text: () => statusesSaid(sa()), how: () => t("statuses.hint"), body: () => <Statuses signals={sa()} /> },
    // --- where: which prefixes, at what cost, on which path
    {
      // where by prefix: how many, and how near S3's per-prefix limit each came in its busiest second
      id: "prefixes",
      group: "where",
      stretched: true,
      when: () => p().prefixes.some((x) => x.prefix !== "other"),
      title: () => t("prefixUse.title"),
      text: () => prefixSaid(pa()),
      how: () => t("prefixUse.hint"),
      body: () => <PrefixUse profile={pa()} rates={sa().prefixes} />,
    },
    { id: "cost", group: "where", stretched: true, when: () => sig().cost.kinds.some((x) => x.cost > 0), title: () => t("costBy.title"), text: () => costSaid(sa()), how: () => t("costBy.hint"), body: () => <CostBy signals={sa()} /> },
    { id: "heat", group: "where", zooms: true, when: () => !!p().heat?.length, title: () => t("heat.title"), text: () => heatSaid(zp(), zr()), how: () => t("heat.hint"), legend: () => heatLegend(zp().heat.filter((c) => !zr() || (c.bin >= zr()![0] && c.bin <= zr()![1])).map((c) => c.n)), body: () => <Heat profile={zp()} range={zr()} /> },
    { id: "flows", group: "where", wide: true, stretched: true, testID: "flows-chart", when: () => graphic() && !!p().flows?.length, title: () => t("flows.title"), text: () => flowsSaid(pa()), how: () => t("flows.hint"), body: () => <Flows profile={pa()} /> },
  ];
  const of = (g: Group) => charts.filter((c) => c.group === g && c.when());
  /** a chart's sentence, and beside it the stretch it is of: one reckoned again for the stretch chosen, or one zoomed to it */
  const said = (c: ChartSpec) => {
    const text = c.text?.();
    const z = zoomMs();
    const stretch = c.stretched ? ofStretch() : c.zooms && z ? t("time.ofStretch", { a: duration(z[0]), b: duration(z[1]) }) : "";
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
    <Scroll testID="time-pane" class="min-h-0 flex-1" onScroll={(e) => props.onScrolled?.(e.offset.y > 8)} contentClass="gap-6 pb-4 terminal:gap-2 terminal:pb-0">
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
                    <Chart testID={c().testID ?? `chart-${c().id}`} title={c().title()} text={said(c())} how={c().how?.()} action={c().action?.()} legend={c().legend?.()} empty={c().stretched ? noneHeld() : null} onExport={props.onExport && c().rows ? () => props.onExport!(c().id, toCsv(c().rows!())) : undefined}>
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
