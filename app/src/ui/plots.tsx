// sonde's charts, on Native Shell's Plot: drawn natively from the rows, the
// pointer followed in Rust, colors from the theme's tokens. What a session's
// time looked like (requests by outcome and requests in flight), where it
// went (by kind of request), how long each request took, an operation's
// requests over time, and two runs side by side. Each says in words what it
// shows; a terminal gets the words and a table instead.

import { signal, Text, token, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { Arc, Area, Bar, Cell, Dot, Grid, Line, Pie, Plot, PlotAppearScope, PlotAxis, PlotLabel, Rule, Sankey } from "nsh/base/plot";
import { bytes, ms } from "../format";
import { t } from "../i18n";
import type { LatencyChange, Observation, Profile, ProfileGroup, ProgramHistory, RecordSummary, RunChange } from "../types";
import { observationSaving, observationText } from "../phrases";
import { Legend, POPUP, SimpleTable, terminal } from "./kit";

/** Time from a session's start: 840 ms, 2.35 s, 41.2 s. */
export const offset = (v: number) => (v < 1000 ? `${Math.round(v)} ms` : `${(v / 1000).toFixed(v < 10000 ? 2 : 1)} s`);

/** An amount as people read it: a session's requests often cost thousandths
 * of a cent, which read as a thousand runs' worth ("0.28 USD per 1,000 runs"). */
export function cost(v: number, currency: string): string {
  if (!Number.isFinite(v) || v < 0) return `— ${currency}`;
  if (v > 0 && v < 0.01) {
    const k = v * 1000;
    const s = k < 0.01 ? Number(k.toPrecision(2)).toString() : k.toFixed(2);
    return t("cost.perThousand", { v: s, cur: currency });
  }
  return `${v.toFixed(2)} ${currency}`;
}

/** A kind of request as people say it: "HeadObject" or "HeadObject, not found". */
export const groupName = (g: Pick<ProfileGroup, "operation" | "missed">) => (g.missed ? t("profile.notFound", { op: g.operation }) : g.operation);

const pct = (v: number) => `${Math.round(v * 100)}%`;

// the keys and screen readers step the hover with a spring; the pointer's own moves follow at once
const MOTION = { appear: { duration: 420, easing: "easeOut" as const }, update: { duration: 260, easing: "easeOut" as const }, enter: { duration: 100 }, exit: { duration: 160 }, pointer: { stiffness: 420, damping: 36 } };

const TONES = ["ok", "miss", "client", "server"] as const;
type Tone = (typeof TONES)[number];
const TONE_TOKEN: Record<Tone, string> = { ok: "bar", miss: "status-miss", client: "status-client", server: "status-server" };
const tones = () => ({ type: "ordinal" as const, domain: [...TONES, "dim"], range: [...TONES.map((x) => token(TONE_TOKEN[x] as never) as unknown as string), token("muted" as never) as unknown as string] });

/** A tooltip's card: the app's one popup surface. */
function Tip(props: { title: string; lines: string[] }) {
  return (
    <View class={`max-w-[280px] gap-0.5 ${POPUP} px-2.5 py-1.5`}>
      <Text class="text-small font-medium text-popover-foreground line-clamp-2">{props.title}</Text>
      <Each of={props.lines}>{(l) => <Text class="tabular-nums text-caption text-muted-foreground">{l()}</Text>}</Each>
    </View>
  );
}

// --- one moment across the charts ------------------------------------------------

/** The moment the pointer rests on in one time chart (ms from the session's
 * start), marked in the others: the charts of a session read as one. */
export const focusMs = signal<number | null>(null);

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
  return t("profile.shapeText", { busy: offset(p.busyMs), wall: offset(p.wallMs), pct: pct(p.busyMs / Math.max(p.wallMs, 1e-9)), peak: p.peakInFlight, at: offset(p.peakAtMs) });
}

/**
 * Requests over a session: a bar per slice of time, stacked by outcome, and
 * a step line of the requests in flight. A press picks the slice under the
 * pointer (again: lets it go), for the operations running then.
 */
export function SessionShape(props: { profile: Profile; slice?: number | null; onSlice?: (i: number | null) => void; class?: string; testID?: string; annotate?: boolean; marks?: { tMs: number; n: number }[] }) {
  const hovered = signal<number | null>(null);
  const rows = () => slices(props.profile);
  const top = () => Math.max(1, ...rows().map((r) => r.ok + r.miss + r.client + r.server));
  // where the findings happened: their numbers over the slices they start in
  const marked = () => {
    const m = new Map<number, number[]>();
    // numbers a few slices apart would print over each other: they share the first one's place
    const near = Math.max(2, Math.round(rows().length / 40));
    const at = (props.marks ?? []).map((k) => ({ n: k.n, i: Math.min(rows().length - 1, Math.max(0, Math.floor(k.tMs / props.profile.binMs))) })).sort((a, b) => a.i - b.i || a.n - b.n);
    let last = -Infinity;
    for (const k of at) {
      const i = k.i - last <= near ? last : k.i;
      m.set(i, [...(m.get(i) ?? []), k.n]);
      last = i;
    }
    return m;
  };
  const focused = () => (hovered() === null ? nearest(rows(), focusMs()) : null);
  const peakBin = () => {
    const at = props.profile.peakAtMs;
    const i = Math.floor(at / props.profile.binMs);
    return props.profile.peakInFlight > 1 && i < rows().length ? i : null;
  };
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
          onHoverChange={({ index }) => {
            hovered.set(index);
            focusMs.set(index === null ? null : rows()[index].tMs + props.profile.binMs / 2);
          }}
          class="h-full w-full"
        >
          <Bar value="ok" stack="r" color={tone("ok")} />
          <Bar value="miss" stack="r" color={tone("miss")} />
          <Bar value="client" stack="r" color={tone("client")} />
          <Bar value="server" stack="r" color={tone("server")} cornerRadii={{ topLeft: 1.5, topRight: 1.5 }} />
          <Line y="inFlight" scale="y2" curve="stepAfter" class="stroke-foreground/30 stroke-[1.25]" hoverDotClass="hidden" />
          {/* the moment another chart rests on */}
          <Rule value={(_, i) => (i === focused() ? top() : (undefined as unknown as number))} class="stroke-foreground/40 stroke-1" />
          {/* where each finding begins */}
          <If when={props.annotate && marked().size > 0}>
            <Rule value={(_, i) => (marked().has(i) ? top() : (undefined as unknown as number))} class="stroke-warn/70 stroke-1" style={{ strokeDasharray: "2 2" }} />
            <PlotLabel class="tabular-nums text-caption font-semibold text-warn" texts={[...marked()].map(([i, ns]) => ({ text: ns.join(" "), x: i, y: top(), align: "center" as const, class: "-translate-y-full pb-0.5" }))} />
          </If>
          {/* the busiest moment, named where it happened */}
          {/* (left out where a finding's number stands near it: the two would cross; Time says it too) */}
          <If when={props.annotate && peakBin() !== null && ![...marked().keys()].some((i) => Math.abs(i - peakBin()!) <= Math.max(3, rows().length / 12))}>
            <PlotLabel
              scale="y2"
              class="tabular-nums text-caption font-medium text-foreground"
              texts={[{ text: t("profile.peakLabel", { n: props.profile.peakInFlight }), x: peakBin()!, y: props.profile.peakInFlight, align: "center", class: "-translate-y-full pb-0.5" }]}
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
export function LatencyBand(props: { profile: Profile; class?: string; testID?: string }) {
  // every slice, so one without requests leaves a gap rather than a line across it
  const rows = () => props.profile.rate;
  const has = (b: Profile["rate"][number]) => b.ok + b.miss + b.client + b.server > 0;
  const answered = () => rows().filter(has);
  const gap = (f: (b: Profile["rate"][number]) => number) => (b: Profile["rate"][number]) => (has(b) ? f(b) : (undefined as unknown as number));
  const top = () => Math.max(...answered().map((b) => b.p99Ms), 1) * 1.15;
  const hovered = signal<number | null>(null);
  const label = () => {
    const last = answered()[answered().length - 1];
    return last ? t("band.text", { p50: ms(last.p50Ms), p95: ms(last.p95Ms), p99: ms(last.p99Ms) }) : "";
  };
  const tip = () => {
    const b = hovered() === null ? null : rows()[hovered()!];
    return b && has(b) ? { title: `+${offset(b.tMs)}`, lines: [t("band.line", { p50: ms(b.p50Ms), p95: ms(b.p95Ms), p99: ms(b.p99Ms) })] } : null;
  };
  return (
    <If when={!terminal() && answered().length > 1} else={<Text class={terminal() ? "text-muted-foreground" : "hidden"}>{label()}</Text>}>
      <Plot
        testID={props.testID ?? "latency-band"}
        data={rows()}
        x="tMs"
        xScale={{ type: "linear", domain: [0, Math.max(props.profile.wallMs, 1)] }}
        yScale={{ type: "linear", domain: [0, top()] }}
        accessibilityLabel={label()}
        accessibilityValue={{ text: tip() ? `${tip()!.title}: ${tip()!.lines[0]}` : "" }}
        motion={MOTION}
        crossLineClass="w-px h-full bg-foreground/20"
        tooltipPlacement="pointer"
        tooltipOffset={10}
        tooltipContent={<If when={tip()}>{(x) => <Tip title={x().title} lines={x().lines} />}</If>}
        onHoverChange={({ index }) => {
          hovered.set(index);
          focusMs.set(index === null ? null : rows()[index].tMs);
        }}
        class={props.class ?? "h-24 w-full"}
      >
        {/* straight between slices: a curve would show values no request had */}
        <Area y0={gap((b) => b.p50Ms)} y1={gap((b) => b.p95Ms)} curve="linear" class="fill-chart-1/25" />
        <Line y={gap((b) => b.p50Ms)} curve="linear" class="stroke-chart-1 stroke-[1.5]" hoverDotClass="size-1.5 rounded-full bg-chart-1" />
        <Line y={gap((b) => b.p99Ms)} curve="linear" class="stroke-muted-foreground/70 stroke-[1]" style={{ strokeDasharray: "3 3" }} hoverDotClass="hidden" />
        <Rule value={(_, i) => (hovered() === null && i === nearest(rows(), focusMs()) ? top() : (undefined as unknown as number))} class="stroke-foreground/40 stroke-1" />
        <PlotAxis placement="left" labelPlacement="inside" ticks={3} format={(v) => ms(v)} labelClass="ml-1 rounded-sm bg-card/85 px-1 tabular-nums text-caption text-muted-foreground" />
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
    return c ? t("chains.text", { n: c.requests, time: offset(c.endMs - c.startMs), fast: offset(c.atOnceMs) }) : "";
  };
  const tip = () => {
    const c = hovered() === null ? null : rows()[hovered()!];
    return c ? { title: c.name, lines: [t("chains.line", { from: offset(c.startMs), to: offset(c.endMs) }), t("chains.fast", { fast: offset(c.atOnceMs), saved: offset(c.endMs - c.startMs - c.atOnceMs) })] } : null;
  };
  return (
    <If
      when={!terminal()}
      else={<SimpleTable columns={[t("chains.chain"), t("chains.from"), t("profile.time"), t("chains.atOnce")]} rows={rows().map((c) => [c.name, `+${offset(c.startMs)}`, offset(c.endMs - c.startMs), offset(c.atOnceMs)])} />}
    >
      <View class="gap-2">
        <Plot
          testID="chains"
          data={rows()}
          yScale={{ type: "band", paddingInner: 0.28, paddingOuter: 0.12 }}
          xScale={{ type: "linear", domain: [0, Math.max(props.profile.wallMs, 1)] }}
          accessibilityLabel={label()}
          accessibilityValue={{ text: tip() ? `${tip()!.title}: ${tip()!.lines.join(", ")}` : "" }}
          appearGeneration={1}
          motion={MOTION}
          crossLineClass="w-full h-full rounded-sm bg-foreground/5"
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<If when={tip()}>{(x) => <Tip title={x().title} lines={x().lines} />}</If>}
          onHoverChange={({ index }) => hovered.set(index)}
          class="mb-6 ml-32"
          style={{ height: rows().length * 34 + 30 }}
        >
          {/* as it ran: one after another */}
          <Bar base="startMs" value="endMs" class="fill-muted-foreground/45" cornerRadii={3} inactiveOpacity={0.45} label={(_, i) => offset(rows()[i].endMs - rows()[i].startMs)} labelClass="pl-2 tabular-nums text-caption text-muted-foreground" />
          {/* the same requests eight at once */}
          <Bar base="startMs" value="soon" class="fill-chart-1" cornerRadii={3} inactiveOpacity={0.45} label={(_, i) => t("chains.bar", { fast: offset(rows()[i].atOnceMs) })} labelClass="pl-2 tabular-nums text-caption text-muted-foreground" />
          <PlotAxis placement="left" format={(i) => rows()[i]?.name ?? ""} labelClass="pr-3 text-small text-foreground" />
          <PlotAxis placement="bottom" ticks={4} format={(v) => offset(v)} labelClass="pt-1 tabular-nums text-caption text-muted-foreground" />
        </Plot>
        <Legend class="pl-32" items={[{ label: t("chains.asRan"), swatch: "bg-muted-foreground/45" }, { label: t("chains.atOnce"), swatch: "bg-chart-1" }]} />
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
    return c ? t("latency.text", { what: c.name, a: ms(c.a95), b: ms(c.b95), x: ratio(c).toFixed(1) }) : "";
  };
  const tip = () => {
    const c = hovered() === null ? null : rows()[hovered()!];
    return c ? { title: c.name, lines: [`${props.names[0]}: p50 ${ms(c.a50)} · p95 ${ms(c.a95)} · ${c.requests[0]}`, `${props.names[1]}: p50 ${ms(c.b50)} · p95 ${ms(c.b95)} · ${c.requests[1]}`] } : null;
  };
  const top = () => Math.max(...rows().flatMap((c) => [c.a95, c.b95]), 1) * 1.25;
  return (
    <If
      when={!terminal()}
      else={<SimpleTable columns={[t("profile.kind"), `p50 ${props.names[0]} → ${props.names[1]}`, `p95 ${props.names[0]} → ${props.names[1]}`, "×"]} rows={rows().map((c) => [c.name, `${ms(c.a50)} → ${ms(c.b50)}`, `${ms(c.a95)} → ${ms(c.b95)}`, `${ratio(c).toFixed(1)}×`])} />}
    >
      <View class="gap-2">
        <Plot
          testID="store-latency"
          data={rows()}
          yScale={{ type: "band", paddingInner: 0.3, paddingOuter: 0.12 }}
          xScale={{ type: "linear", domain: [0, top()] }}
          accessibilityLabel={label()}
          accessibilityValue={{ text: tip() ? `${tip()!.title}: ${tip()!.lines.join(", ")}` : "" }}
          appearGeneration={1}
          motion={MOTION}
          crossLineClass="w-full h-full rounded-sm bg-foreground/5"
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<If when={tip()}>{(x) => <Tip title={x().title} lines={x().lines} />}</If>}
          onHoverChange={({ index }) => hovered.set(index)}
          class="ml-52"
          style={{ height: rows().length * 40 + 8 }}
        >
          <Bar value="a95" class="fill-muted-foreground/45" cornerRadii={{ topRight: 3, bottomRight: 3 }} inactiveOpacity={0.45} label={(_, i) => ms(rows()[i].a95)} labelClass="pl-2 tabular-nums text-caption text-muted-foreground" />
          <Bar value="b95" class="fill-chart-1" cornerRadii={{ topRight: 3, bottomRight: 3 }} inactiveOpacity={0.45} label={(_, i) => `${ms(rows()[i].b95)} · ${ratio(rows()[i]).toFixed(1)}×`} labelClass="pl-2 tabular-nums text-caption font-medium text-foreground" />
          <PlotAxis placement="left" format={(i) => rows()[i]?.name ?? ""} labelClass="pr-3 text-small text-foreground" />
        </Plot>
        <Legend class="pl-52" items={[{ label: `p95 · ${props.names[0]}`, swatch: "bg-muted-foreground/45" }, { label: `p95 · ${props.names[1]}`, swatch: "bg-chart-1" }]} />
      </View>
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
    return top ? t("profile.spentText", { what: top.name, pct: pct(top.share), time: offset(top.waitedMs) }) : "";
  };
  const tip = () => {
    const g = hovered() === null ? null : rows()[hovered()!];
    return g ? { title: g.name, lines: [t("profile.spentLine", { time: offset(g.waitedMs), pct: pct(g.share), n: g.requests }), t("profile.latencyLine", { p50: ms(g.p50Ms), p95: ms(g.p95Ms), max: ms(g.maxMs) }), `↑${bytes(g.bytesUp)} ↓${bytes(g.bytesDown)} · ${cost(g.cost, props.profile.cost.currency)}`] } : null;
  };
  return (
    <If
      when={!terminal()}
      else={<SimpleTable columns={[t("profile.kind"), t("profile.share"), t("profile.time"), t("figure.requests")]} rows={rows().map((g) => [g.name, pct(g.share), offset(g.waitedMs), String(g.requests)])} />}
    >
      <Plot
        testID="time-spent"
        data={rows()}
        yScale={{ type: "band", paddingInner: 0.32, paddingOuter: 0.16 }}
        xScale={{ type: "linear", domain: [0, Math.max(...rows().map((g) => g.waitedMs), 1) * 1.18] }}
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
          value="waitedMs"
          class="fill-chart-1"
          cornerRadii={{ topRight: 3, bottomRight: 3 }}
          inactiveOpacity={0.45}
          stagger={0.3}
          label={(_, i) => `${pct(rows()[i].share)} · ${offset(rows()[i].waitedMs)}`}
          labelClass="pl-2 tabular-nums text-caption text-muted-foreground"
        />
        <PlotAxis placement="left" format={(i) => rows()[i]?.name ?? ""} labelClass="pr-3 text-small text-foreground" />
      </Plot>
    </If>
  );
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
          accessibilityValue={{ text: tip() ? `${tip()!.title}: ${tip()!.lines.join(", ")}` : "" }}
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
          accessibilityValue={{ text: tip() ? `${tip()!.title}: ${tip()!.lines[0]}` : "" }}
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

// --- what a session comes to, in a sentence ------------------------------------------

/** A session in a sentence: where its time went and what the first finding would save. */
export function sessionStory(p: Profile, findings: Observation[]): string {
  const top = p.groups[0];
  const parts = top ? [t("story.time", { busy: offset(p.busyMs), wall: offset(p.wallMs), what: groupName(top), pct: pct(top.share) })] : [];
  const first = findings.find((o) => o.saving) ?? findings[0];
  if (first) {
    const saving = observationSaving(first);
    parts.push(saving ? t("story.saving", { saving }) : t("story.finding", { finding: observationText(first) }));
  }
  return parts.join(" ");
}

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
  const bucketOf = (p: string) => p.split("/")[0];
  const oneBucket = () => new Set(props.profile.prefixes.filter((x) => x.prefix !== "other").map((x) => bucketOf(x.prefix))).size === 1;
  const short = (n: string) => (oneBucket() && n.includes("/") && n.indexOf("/") < n.length - 1 ? n.slice(n.indexOf("/") + 1) : n);
  const name = (n: { name: string }) => (n.name === "other operations" ? t("flows.otherOps") : n.name === "other prefixes" ? t("flows.otherPrefixes") : OUTCOME_FILL[n.name] ? t(`flows.${n.name.replace(" ", "")}` as never) : short(n.name));
  const fill = (n: { name: string; stage: number }) => (n.stage === 2 ? OUTCOME_FILL[n.name] : n.stage === 0 ? "fill-chart-1" : "fill-chart-4");
  const from = (n: { stage: number }) => (n.stage === 0 ? "from-chart-1/35" : "from-chart-4/35");
  const to = (n: { name: string; stage: number }) => (n.stage === 2 ? OUTCOME_FROM[n.name] : "to-chart-4/35");
  const hovered = signal<number | null>(null);
  const label = () => {
    const ok = props.profile.flows.filter((f) => f.target === "ok").reduce((s, f) => s + f.value, 0);
    const all = props.profile.flows.filter((f) => OUTCOME_FILL[f.target]).reduce((s, f) => s + f.value, 0);
    const top = props.profile.prefixes[0];
    return t("flows.text", { n: all, ok: pct(ok / Math.max(all, 1)), prefix: top?.prefix ?? "", share: pct((top?.requests ?? 0) / Math.max(all, 1)) });
  };
  return (
    <If when={!terminal() && props.profile.flows.length > 0} else={<Text class={terminal() ? "text-muted-foreground" : "hidden"}>{label()}</Text>}>
      <Plot
        testID="flows"
        data={nodes()}
        accessibilityLabel={label()}
        accessibilityValue={{ text: hovered() === null ? "" : name(nodes()[hovered()!]) }}
        onHoverChange={({ index }) => hovered.set(index)}
        appearGeneration={1}
        motion={{ ...MOTION, appear: { duration: 700, easing: "easeOut" } }}
        class="mx-36 mt-4 h-96"
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
          hoverDim={0.75}
          nodeLabel={(n) => name(n)}
          valueLabel={(_, v) => String(v)}
          nodeLabelClass="max-w-36 truncate px-2 py-0.5 text-small text-foreground"
          valueLabelClass="tabular-nums text-caption text-muted-foreground"
        />
      </Plot>
    </If>
  );
}

// --- when, and where ---------------------------------------------------------------

/** Requests by prefix and slice of the session: when each part of the key space was busy. */
export function Heat(props: { profile: Profile }) {
  const p = () => props.profile;
  const bins = () => p().rate.map((_, i) => String(i));
  const names = () => p().prefixes.map((x) => (x.prefix === "other" ? t("flows.otherPrefixes") : x.prefix));
  const rows = () => p().heat.map((c) => ({ x: String(c.bin), y: names()[c.prefix] ?? "", n: c.n, bin: c.bin }));
  const hovered = signal<number | null>(null);
  const max = () => rows().reduce((m, r) => Math.max(m, r.n), 1);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: r.y, lines: [`+${offset(r.bin * p().binMs)} – +${offset((r.bin + 1) * p().binMs)}`, t("sessions.requestsN", { n: r.n })] } : null;
  };
  const last = () => Math.max(0, bins().length - 1);
  const label = () => {
    const top = p().prefixes[0];
    return top ? t("heat.text", { prefix: top.prefix, n: top.peakPerSecond }) : "";
  };
  return (
    <If
      when={!terminal() && rows().length > 0 && bins().length > 1}
      // a terminal: each prefix's requests and its busiest second, as figures
      else={<If when={p().prefixes.length > 0}><SimpleTable columns={[t("common.prefix"), t("heat.requests"), t("heat.peak"), t("heat.errors")]} rows={p().prefixes.map((x, i) => [names()[i], String(x.requests), String(x.peakPerSecond), String(x.errors)])} /></If>}
    >
      <Plot
        testID="heat"
        data={rows()}
        xScale={{ type: "band", domain: bins(), paddingInner: 0.12 }}
        yScale={{ type: "band", domain: names(), paddingInner: 0.18 }}
        colorScale={{ type: "sequential", domain: [0, max()], range: [token("muted" as never) as unknown as string, token("chart-1" as never) as unknown as string] }}
        accessibilityLabel={label()}
        accessibilityValue={{ text: tip() ? `${tip()!.title}, ${tip()!.lines.join(", ")}` : "" }}
        appearGeneration={1}
        motion={MOTION}
        tooltipPlacement="pointer"
        tooltipOffset={10}
        tooltipContent={<If when={tip()}>{(x) => <Tip title={x().title} lines={x().lines} />}</If>}
        onHoverChange={({ index }) => hovered.set(index)}
        class="mb-6 ml-52"
        style={{ height: names().length * 22 + 8 }}
      >
        <Cell x="x" y="y" value="n" cornerRadii={2} inactiveOpacity={0.45} stagger={0.2} hoverCellClass="rounded-sm outline-2 outline-offset-1 outline-foreground" />
        <PlotAxis placement="left" labelClass="w-48 pr-3 text-right text-small text-foreground line-clamp-1" />
        <PlotAxis tickValues={[0, Math.round(last() / 2), last()]} format={(i) => `+${offset(i * p().binMs)}`} labelClass="pt-2 tabular-nums text-caption text-muted-foreground" />
      </Plot>
    </If>
  );
}

// --- how long, and how spread ----------------------------------------------------------

/** Each kind's latency: its middle half (median to p95) as a line, the median a dot, the slowest a ring. */
export function Spread(props: { profile: Profile; max?: number }) {
  const rows = () => props.profile.groups.slice(0, props.max ?? 8).map((g) => ({ ...g, name: groupName(g) }));
  const top = () => rows().reduce((m, g) => Math.max(m, g.maxMs), 1);
  const hovered = signal<number | null>(null);
  const tip = () => {
    const g = hovered() === null ? null : rows()[hovered()!];
    return g ? { title: g.name, lines: [t("profile.latencyLine", { p50: ms(g.p50Ms), p95: ms(g.p95Ms), max: ms(g.maxMs) }), t("sessions.requestsN", { n: g.requests })] } : null;
  };
  const slowest = () => rows().reduce<(typeof rows extends () => (infer R)[] ? R : never) | null>((m, g) => (!m || g.maxMs > m.maxMs ? g : m), null);
  return (
    <If when={!terminal() && rows().length > 0} else={<SimpleTable columns={[t("profile.kind"), "p50", "p95", "max"]} rows={rows().map((g) => [g.name, ms(g.p50Ms), ms(g.p95Ms), ms(g.maxMs)])} />}>
      <Plot
        testID="spread"
        data={rows()}
        yScale={{ type: "band", paddingInner: 0.4, paddingOuter: 0.2 }}
        xScale={{ type: "linear", domain: [0, top() * 1.05] }}
        accessibilityLabel={slowest() ? t("profile.latencyText", { what: slowest()!.name, ms: ms(slowest()!.maxMs) }) : ""}
        accessibilityValue={{ text: tip() ? `${tip()!.title}: ${tip()!.lines[0]}` : "" }}
        appearGeneration={1}
        motion={MOTION}
        crossLineClass="w-full h-full rounded-sm bg-foreground/5"
        tooltipPlacement="pointer"
        tooltipOffset={10}
        tooltipContent={<If when={tip()}>{(x) => <Tip title={x().title} lines={x().lines} />}</If>}
        onHoverChange={({ index }) => hovered.set(index)}
        class="mb-6 ml-52"
        style={{ height: rows().length * 30 + 8 }}
      >
        <Grid ticks={4} class="stroke-border" />
        <Rule base="p50Ms" value="maxMs" class="stroke-muted-foreground/35 stroke-1" />
        <Rule base="p50Ms" value="p95Ms" class="stroke-chart-1 stroke-[3]" inactiveOpacity={0.4} />
        <Dot value="maxMs" r={3} class="fill-card stroke-muted-foreground stroke-[1.5]" />
        <Dot value="p50Ms" r={4} class="fill-chart-1 stroke-card stroke-2" inactiveOpacity={0.4} />
        <PlotAxis placement="left" format={(i) => rows()[i]?.name ?? ""} labelClass="pr-3 text-small text-foreground" />
        <PlotAxis ticks={4} format={(v) => ms(v)} labelClass="pt-2 tabular-nums text-caption text-muted-foreground" />
      </Plot>
    </If>
  );
}

// --- a program over its runs ------------------------------------------------------------

type Measure = "requests" | "busyMs" | "cost" | "warnings";

/** One figure of a program over its runs: a line through them, the baseline dashed, the latest run labelled. */
export function RunTrend(props: { history: ProgramHistory; measure: Measure; title: string; format: (v: number) => string; onOpen?: (path: string) => void }) {
  const runs = () => props.history.runs;
  const values = () => runs().map((r) => r[props.measure] as number);
  const base = () => {
    const b = runs().find((r) => r.path === props.history.baseline);
    return b ? (b[props.measure] as number) : null;
  };
  const hi = () => Math.max(...values(), base() ?? 0, 1e-9) * 1.25;
  const hovered = signal<number | null>(null);
  const shown = () => runs()[hovered() ?? runs().length - 1];
  const last = () => runs().length - 1;
  return (
    <View testID={`trend-${props.measure}`} class="min-w-[220px] flex-1 gap-1.5 rounded-(--radius-card) border border-border bg-card px-3.5 py-3">
      <View class="flex-row items-baseline gap-2">
        <Text class="flex-1 text-caption font-medium text-muted-foreground">{props.title}</Text>
        <Text class="tabular-nums text-heading font-semibold text-foreground">{props.format(shown()[props.measure] as number)}</Text>
      </View>
      <BaseButton
        accessibilityLabel={`${props.title}: ${values().map(props.format).join(", ")}`}
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
          accessibilityLabel={`${props.title}: ${values().map(props.format).join(", ")}`}
          appearGeneration={props.history.program}
          motion={MOTION}
          crossLineClass="w-px h-full bg-muted-foreground/40"
          onHoverChange={({ index }) => hovered.set(index)}
          class="mr-1 h-16"
        >
          <If when={base() !== null}>
            <Grid y={[base()!]} class="stroke-muted-foreground/60" style={{ strokeDasharray: "3 3" }} />
          </If>
          <Line curve="linear" class="stroke-chart-1 stroke-2" dot dotClass="size-1.5 rounded-full bg-chart-1" hoverDotClass="size-2.5 rounded-full border-2 border-card bg-chart-1" />
          <PlotLabel class="tabular-nums text-caption text-muted-foreground" texts={last() > 0 ? [{ text: props.format(values()[last()]), x: last(), y: values()[last()], align: "right", class: "-translate-y-full pb-1" }] : []} />
        </Plot>
      </BaseButton>
    </View>
  );
}
