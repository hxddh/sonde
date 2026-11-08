// sonde's charts, on Native Shell's Plot: drawn natively from the rows, the
// pointer followed in Rust, colors from the theme's tokens. What a session's
// time looked like (requests by outcome and requests in flight), where it
// went (by kind of request), how long each request took, an operation's
// requests over time, and two runs side by side. Each says in words what it
// shows; a terminal gets the words and a table instead.

import { signal, Text, token, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { Bar, Dot, Grid, Line, Plot, PlotAxis } from "nsh/base/plot";
import { bytes, ms } from "../format";
import { t } from "../i18n";
import type { Profile, ProfileGroup, RecordSummary, RunChange } from "../types";
import { SimpleTable, terminal } from "./kit";

/** Time from a session's start: 840 ms, 2.35 s, 41.2 s. */
export const offset = (v: number) => (v < 1000 ? `${Math.round(v)} ms` : `${(v / 1000).toFixed(v < 10000 ? 2 : 1)} s`);

/** Small amounts to two significant digits: a session's requests often cost thousandths of a cent. */
export function cost(v: number, currency: string): string {
  if (!Number.isFinite(v) || v < 0) return `— ${currency}`;
  if (v > 0 && v < 0.01) {
    const s = Number(v.toPrecision(2));
    return `${s < 1e-6 ? "<0.000001" : s.toFixed(Math.max(2, Math.ceil(-Math.log10(s)) + 1))} ${currency}`;
  }
  return `${v.toFixed(2)} ${currency}`;
}

/** A kind of request as people say it: "HeadObject" or "HeadObject, not found". */
export const groupName = (g: Pick<ProfileGroup, "operation" | "missed">) => (g.missed ? t("profile.notFound", { op: g.operation }) : g.operation);

const pct = (v: number) => `${Math.round(v * 100)}%`;

const MOTION = { appear: { duration: 420, easing: "easeOut" as const }, update: { duration: 260, easing: "easeOut" as const }, enter: { duration: 100 }, exit: { duration: 160 } };

const TONES = ["ok", "miss", "client", "server"] as const;
type Tone = (typeof TONES)[number];
const TONE_TOKEN: Record<Tone, string> = { ok: "bar", miss: "status-miss", client: "status-client", server: "status-server" };
const tones = () => ({ type: "ordinal" as const, domain: [...TONES, "dim"], range: [...TONES.map((x) => token(TONE_TOKEN[x] as never) as unknown as string), token("muted" as never) as unknown as string] });

/** A tooltip's card: the plot's popup surface. */
function Tip(props: { title: string; lines: string[] }) {
  return (
    <View class="max-w-[280px] gap-0.5 rounded-(--radius) border border-border bg-popover px-2.5 py-1.5 shadow-lg">
      <Text class="text-small font-medium text-popover-foreground line-clamp-2">{props.title}</Text>
      <Each of={props.lines}>{(l) => <Text class="tabular-nums text-caption text-muted-foreground">{l()}</Text>}</Each>
    </View>
  );
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
  return t("profile.shapeText", { busy: offset(p.busyMs), wall: offset(p.wallMs), pct: pct(p.busyMs / Math.max(p.wallMs, 1e-9)), peak: p.peakInFlight, at: offset(p.peakAtMs) });
}

/**
 * Requests over a session: a bar per slice of time, stacked by outcome, and
 * a step line of the requests in flight. A press picks the slice under the
 * pointer (again: lets it go), for the operations running then.
 */
export function SessionShape(props: { profile: Profile; slice?: number | null; onSlice?: (i: number | null) => void; class?: string; testID?: string }) {
  const hovered = signal<number | null>(null);
  const rows = () => slices(props.profile);
  const tone = (x: Tone) => (d: Slice, i: number) => (props.slice == null || props.slice === i ? x : "dim");
  const tip = () => {
    const i = hovered();
    const r = i === null ? null : rows()[i];
    if (!r) return null;
    const n = r.ok + r.miss + r.client + r.server;
    const parts = [t("sessions.requestsN", { n }), ...(r.miss ? [t("profile.missN", { n: r.miss })] : []), ...(r.client + r.server ? [t("profile.failedN", { n: r.client + r.server })] : [])];
    return { title: `+${offset(r.tMs)} – +${offset(r.tMs + props.profile.binMs)}`, lines: [parts.join(" · "), t("profile.inFlightN", { n: r.inFlight })] };
  };
  return (
    // a handful of requests has no shape worth a chart
    <If when={!terminal() && rows().length > 1 && props.profile.samples.length >= 12} else={<Text class={terminal() ? "text-muted-foreground" : "hidden"}>{shapeSentence(props.profile)}</Text>}>
      <BaseButton
        testID={props.testID ?? "session-shape"}
        accessibilityLabel={shapeSentence(props.profile)}
        disabled={!props.onSlice}
        onPress={() => {
          const i = hovered();
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
          accessibilityValue={{ text: tip() ? `${tip()!.title}: ${tip()!.lines.join(", ")}` : "" }}
          appearGeneration={1}
          motion={MOTION}
          crossLineClass="w-full h-full rounded-sm bg-foreground/6"
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<If when={tip()}>{(x) => <Tip title={x().title} lines={x().lines} />}</If>}
          onHoverChange={({ index }) => hovered.set(index)}
          class="h-full w-full"
        >
          <Bar value="ok" stack="r" color={tone("ok")} />
          <Bar value="miss" stack="r" color={tone("miss")} />
          <Bar value="client" stack="r" color={tone("client")} />
          <Bar value="server" stack="r" color={tone("server")} cornerRadii={{ topLeft: 1.5, topRight: 1.5 }} />
          <Line y="inFlight" scale="y2" curve="stepAfter" class="stroke-foreground/30 stroke-[1.25]" hoverDotClass="hidden" />
        </Plot>
      </BaseButton>
    </If>
  );
}

// --- where the time went --------------------------------------------------------

/** Where a session's time went: a bar per kind of request, its share of the time the program waited. */
export function TimeSpent(props: { profile: Profile; max?: number }) {
  const rows = () => props.profile.groups.slice(0, props.max ?? 8).map((g) => ({ ...g, name: groupName(g) }));
  const hovered = signal<number | null>(null);
  const label = () => {
    const top = rows()[0];
    return top ? t("profile.spentText", { what: top.name, pct: pct(top.share), time: offset(top.busyMs) }) : "";
  };
  const tip = () => {
    const g = hovered() === null ? null : rows()[hovered()!];
    return g ? { title: g.name, lines: [t("profile.spentLine", { time: offset(g.busyMs), pct: pct(g.share), n: g.requests }), t("profile.latencyLine", { p50: ms(g.p50Ms), p95: ms(g.p95Ms), max: ms(g.maxMs) }), `↑${bytes(g.bytesUp)} ↓${bytes(g.bytesDown)} · ${cost(g.cost, props.profile.cost.currency)}`] } : null;
  };
  return (
    <If
      when={!terminal()}
      else={<SimpleTable columns={[t("profile.kind"), t("profile.share"), t("profile.time"), t("figure.requests")]} rows={rows().map((g) => [g.name, pct(g.share), offset(g.busyMs), String(g.requests)])} />}
    >
      <Plot
        testID="time-spent"
        data={rows()}
        yScale={{ type: "band", paddingInner: 0.32, paddingOuter: 0.16 }}
        xScale={{ type: "linear", domain: [0, Math.max(...rows().map((g) => g.busyMs), 1) * 1.18] }}
        accessibilityLabel={label()}
        accessibilityValue={{ text: tip() ? `${tip()!.title}: ${tip()!.lines[0]}` : "" }}
        appearGeneration={1}
        motion={MOTION}
        crossLineClass="w-full h-full rounded-sm bg-foreground/5"
        tooltipPlacement="pointer"
        tooltipOffset={10}
        tooltipContent={<If when={tip()}>{(x) => <Tip title={x().title} lines={x().lines} />}</If>}
        onHoverChange={({ index }) => hovered.set(index)}
        class="ml-52"
        style={{ height: rows().length * 30 + 8 }}
      >
        <Bar
          value="busyMs"
          class="fill-chart-1"
          cornerRadii={{ topRight: 3, bottomRight: 3 }}
          inactiveOpacity={0.45}
          stagger={0.3}
          label={(_, i) => `${pct(rows()[i].share)} · ${offset(rows()[i].busyMs)}`}
          labelClass="pl-2 tabular-nums text-caption text-muted-foreground"
        />
        <PlotAxis placement="left" format={(i) => rows()[i]?.name ?? ""} labelClass="pr-3 text-small text-foreground" />
      </Plot>
    </If>
  );
}

/** How long each request took, by kind: a dot per request (the slowest few hundred), the slow ones far right. */
export function Latencies(props: { profile: Profile; max?: number; onRequest?: (id: number) => void }) {
  const groups = () => props.profile.groups.slice(0, props.max ?? 8);
  const rows = () => props.profile.samples.filter((s) => s.group < groups().length);
  const hovered = signal<number | null>(null);
  const top = () => Math.max(...rows().map((s) => s.ms), 1);
  const ticks = () => {
    const step = niceStep(top() / 4);
    return [0, step, step * 2, step * 3, step * 4].filter((v) => v <= top() * 1.05);
  };
  const tip = () => {
    const s = hovered() === null ? null : rows()[hovered()!];
    return s ? { title: groupName(groups()[s.group]), lines: [`${ms(s.ms)} · +${offset(s.tMs)}`] } : null;
  };
  const slowest = () => rows()[0];
  return (
    <If when={!terminal() && rows().length > 0}>
      <BaseButton
        testID="latencies"
        accessibilityLabel={slowest() ? t("profile.latencyText", { what: groupName(groups()[slowest()!.group]), ms: ms(slowest()!.ms) }) : ""}
        disabled={!props.onRequest}
        onPress={() => {
          const s = hovered() === null ? null : rows()[hovered()!];
          if (s) props.onRequest?.(s.id);
        }}
      >
        <Plot
          data={rows()}
          x="ms"
          xScale={{ type: "linear", domain: [0, top() * 1.05] }}
          yScale={{ type: "linear", domain: [-0.6, groups().length - 0.4], reverse: true }}
          accessibilityLabel={slowest() ? t("profile.latencyText", { what: groupName(groups()[slowest()!.group]), ms: ms(slowest()!.ms) }) : ""}
          appearGeneration={1}
          motion={MOTION}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<If when={tip()}>{(x) => <Tip title={x().title} lines={x().lines} />}</If>}
          onHoverChange={({ index }) => hovered.set(index)}
          class="mb-6 ml-52"
          style={{ height: groups().length * 30 + 8 }}
        >
          <Grid x={ticks()} class="stroke-border" />
          <Dot value={(_, i) => rows()[i].group} r={3.5} class="fill-chart-1/55 stroke-card" inactiveOpacity={0.35} hoverDotClass="rounded-full outline-2 outline-offset-1 outline-foreground" />
          <PlotAxis placement="left" tickValues={groups().map((_, i) => i)} format={(i) => groupName(groups()[i] ?? { operation: "", missed: false })} labelClass="pr-3 text-small text-foreground" />
          <PlotAxis tickValues={ticks()} format={(v) => ms(v)} labelClass="pt-2 tabular-nums text-caption text-muted-foreground" />
        </Plot>
      </BaseButton>
    </If>
  );
}

function niceStep(v: number): number {
  const p = 10 ** Math.floor(Math.log10(Math.max(v, 1e-6)));
  for (const m of [1, 2, 5, 10]) if (m * p >= v) return m * p;
  return 10 * p;
}

// --- an operation's requests ------------------------------------------------------

const statusTone = (s: number | null): Tone => (s === null || s >= 500 ? "server" : s === 404 ? "miss" : s >= 400 ? "client" : "ok");

/** An operation's requests in time: a bar from each one's start to its end, so waits and overlaps show. */
export function Waterfall(props: { records: RecordSummary[]; onRecord?: (id: number) => void }) {
  // a loop, not a spread: an operation may hold more requests than a call takes arguments
  const t0 = () => props.records.reduce((m, r) => Math.min(m, new Date(r.time).getTime()), Infinity);
  const rows = () =>
    props.records.slice(0, 60).map((r) => {
      const s = new Date(r.time).getTime() - t0();
      return { id: r.id, op: r.operation, target: r.target, start: s, end: s + Math.max(r.latencyMs, 0.3), tone: statusTone(r.status), status: r.status, ms: r.latencyMs };
    });
  const hovered = signal<number | null>(null);
  const end = () => Math.max(...rows().map((r) => r.end), 1);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: `${r.status ?? "—"} ${r.op}`, lines: [r.target, `+${offset(r.start)} · ${ms(r.ms)}`] } : null;
  };
  return (
    <If when={!terminal() && props.records.length > 1}>
      <BaseButton
        testID="waterfall"
        accessibilityLabel={t("profile.waterfallText", { n: props.records.length, time: offset(end()) })}
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
          accessibilityLabel={t("profile.waterfallText", { n: props.records.length, time: offset(end()) })}
          appearGeneration={props.records[0]?.id}
          motion={MOTION}
          crossLineClass="w-full h-full rounded-sm bg-foreground/5"
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<If when={tip()}>{(x) => <Tip title={x().title} lines={x().lines} />}</If>}
          onHoverChange={({ index }) => hovered.set(index)}
          class="mb-5"
          style={{ height: Math.min(rows().length, 24) * 12 + 6 }}
        >
          <Bar base="start" value="end" color="tone" cornerRadii={2} inactiveOpacity={0.4} stagger={0.4} />
          <PlotAxis tickValues={[0, end()]} format={(v) => (v === 0 ? "0" : offset(v))} labelClass="pt-1.5 tabular-nums text-caption text-muted-foreground" />
        </Plot>
      </BaseButton>
    </If>
  );
}

// --- operations over time (the timeline) -------------------------------------------

export type GanttRow = { i: number; title: string; start: number; end: number; tone: Tone; detail: string };

/**
 * Operations in time: a bar per operation from its start to its end, in its
 * outcome's color, against the time from the session's start. `domain`
 * shows a part of the session (a slice picked on the shape).
 */
export function Gantt(props: { rows: GanttRow[]; domain: [number, number]; selected: number | null; onSelect: (i: number) => void; label: string }) {
  const hovered = signal<number | null>(null);
  const ticks = () => {
    const [a, b] = props.domain;
    const step = niceStep((b - a) / 6);
    const first = Math.ceil(a / step) * step;
    return Array.from({ length: Math.floor((b - first) / step) + 1 }, (_, k) => first + k * step);
  };
  const tip = () => {
    const r = hovered() === null ? null : props.rows[hovered()!];
    return r ? { title: r.title, lines: [`+${offset(r.start)} · ${offset(r.end - r.start)}`, r.detail] } : null;
  };
  const color = (r: GanttRow) => (props.selected === null || props.selected === r.i ? r.tone : "dim");
  return (
    <BaseButton
      testID="gantt"
      accessibilityLabel={props.label}
      onPress={() => {
        const r = hovered() === null ? null : props.rows[hovered()!];
        if (r) props.onSelect(r.i);
      }}
    >
      <Plot
        data={props.rows}
        yScale={{ type: "band", paddingInner: 0.38, paddingOuter: 0.2 }}
        xScale={{ type: "linear", domain: props.domain }}
        colorScale={tones()}
        accessibilityLabel={props.label}
        accessibilityValue={{ text: tip() ? `${tip()!.title}, ${tip()!.lines[0]}` : "" }}
        motion={MOTION}
        crossLineClass="w-full h-full bg-foreground/5"
        tooltipPlacement="pointer"
        tooltipOffset={12}
        tooltipContent={<If when={tip()}>{(x) => <Tip title={x().title} lines={x().lines} />}</If>}
        onHoverChange={({ index }) => hovered.set(index)}
        class="ml-60 mr-4 mt-7"
        style={{ height: props.rows.length * 22 + 8 }}
      >
        <Grid x={ticks()} class="stroke-border" />
        <Bar base="start" value="end" color={color} cornerRadii={3} inactiveOpacity={0.5} />
        <PlotAxis placement="top" tickValues={ticks()} format={(v) => offset(v)} labelClass="pb-1.5 tabular-nums text-caption text-muted-foreground" />
        <PlotAxis placement="left" format={(i) => props.rows[i]?.title ?? ""} labelClass="w-56 pr-3 text-right text-small text-foreground line-clamp-1" />
      </Plot>
    </BaseButton>
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
      when={!terminal()}
      else={<SimpleTable columns={[t("profile.kind"), t("profile.before"), t("profile.after")]} rows={rows().map((r) => [r.name, String(r.before), String(r.after)])} />}
    >
      <If when={rows().length > 0}>
        <Plot
          testID="run-bars"
          data={rows()}
          yScale={{ type: "band", paddingInner: 0.3, paddingOuter: 0.15, groupPadding: 0.12 }}
          // room for the labels at the bars' ends
          xScale={{ type: "linear", domain: [0, Math.max(1, ...rows().map((r) => Math.max(r.before, r.after))) * 1.15] }}
          colorScale={{ type: "ordinal", domain: ["fewer", "more", "same"], range: [token("status-ok" as never) as unknown as string, token("status-server" as never) as unknown as string, token("muted-foreground" as never) as unknown as string] }}
          accessibilityLabel={rows().map((r) => `${r.name} ${r.before} → ${r.after}`).join(", ")}
          motion={MOTION}
          appearGeneration={props.change.requests.join("-")}
          crossLineClass="w-full h-full rounded-sm bg-foreground/5"
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<If when={tip()}>{(x) => <Tip title={x().title} lines={x().lines} />}</If>}
          onHoverChange={({ index }) => hovered.set(index)}
          class="ml-52"
          style={{ height: rows().length * 34 + 8 }}
        >
          <Bar value="before" class="fill-muted-foreground/30" cornerRadii={{ topRight: 2, bottomRight: 2 }} label={(_, i) => String(rows()[i].before)} labelClass="pl-1.5 tabular-nums text-caption text-muted-foreground" />
          <Bar value="after" color="tone" cornerRadii={{ topRight: 2, bottomRight: 2 }} label={(_, i) => String(rows()[i].after)} labelClass="pl-1.5 tabular-nums text-caption text-foreground" />
          <PlotAxis placement="left" format={(i) => rows()[i]?.name ?? ""} labelClass="pr-3 text-small text-foreground" />
        </Plot>
      </If>
    </If>
  );
}
