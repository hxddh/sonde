// Where: prefixes named, where requests went (flows), when each prefix was
// busy, the map of prefixes, and what the requests cost.

import { signal, Text, View } from "nsh";
import { Bar, Cell, Plot, PlotAxis, Sankey, Treemap } from "nsh/base/plot";
import { cost, count, duration, percent, costParts } from "../../format";
import { t } from "../../i18n";
import { nearestLimit } from "../../signals";
import type { Profile, Signals } from "../../types";
import { areasWithRoom, AXIS_LABEL, CROSS_BAND, ChartNote, ChartTip, FLOWS, GUTTER, GUTTER_BOTH, GUTTER_CHARS, HOVER, INACTIVE, LABEL, Legend, MOTION, NoneInStretch, TICK_ROOM, TREEMAP, axisName, pct, rowsHeight, tipValue, tail, TimeAxis, BARS, CELLS, TIP, HEAT_RAMP } from "./system";
import { Options, terminal, textual, SimpleTable, TONE } from "../kit";

// --- prefixes, named ---------------------------------------------------------------


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

/** The flows' colours are roles, as every chart's: the requests in the bar's colour from the kind of
 * request through its prefix, and each answer in its outcome's tone (`TONE`), the ribbons into it
 * turning to that tone. */
const OUTCOME_FILL: Record<string, string> = { ok: "fill-bar", "not found": "fill-status-miss", "client error": "fill-status-client", "server error": "fill-status-server" };
const OUTCOME_TO: Record<string, string> = { ok: "to-bar/40", "not found": "to-status-miss/40", "client error": "to-status-client/40", "server error": "to-status-server/40" };

/**
 * Where a session's requests went: from each kind of request to the
 * prefixes it touched, and from each prefix to how it was answered. The
 * widths are requests; the pointer over a node keeps its flows and fades the rest.
 */
export function Flows(props: { profile: Profile }) {
  const nodes = () => {
    const names: string[] = [];
    for (const f of props.profile.flows) for (const n of [f.source, f.target]) if (!names.includes(n)) names.push(n);
    return names.map((name) => ({ name, stage: OUTCOME_FILL[name] ? 2 : props.profile.groups.some((g) => g.kind === name) || name === "other operations" ? 0 : 1 }));
  };
  const links = () => {
    const at = new Map(nodes().map((n, i) => [n.name, i]));
    return props.profile.flows.map((f) => ({ source: at.get(f.source)!, target: at.get(f.target)!, value: f.value }));
  };
  // prefixes by what tells them apart; what they share is said once under the chart
  const named = () => prefixNames(props.profile.prefixes, GUTTER_CHARS - 6);
  const short = (n: string) => named().name(n);
  const name = (n: { name: string }) => (n.name === "other operations" ? t("flows.otherOps") : n.name === "other prefixes" ? t("flows.otherPrefixes") : OUTCOME_FILL[n.name] ? t(`flows.${n.name.replace(" ", "")}` as never) : short(n.name));
  const fill = (n: { name: string; stage: number }) => (n.stage === 2 ? OUTCOME_FILL[n.name] : "fill-bar");
  const to = (n: { name: string; stage: number }) => (n.stage === 2 ? OUTCOME_TO[n.name] : "to-bar/25");
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
    <If when={!terminal() && props.profile.flows.length > 0} else={<Text class="hidden text-muted-foreground terminal:flex">{label()}</Text>}>
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
        motion={MOTION}
        // not rows: names stand at both edges, in the one gutter either side
        class={`${GUTTER_BOTH} mt-2 ${FLOWS}`}
      >
        <Sankey
          links={links()}
          nodeWidth={7}
          nodePadding={30}
          nodeCornerRadius={TIP.cell}
          valueScale="linear"
          minLinkWidth={1.5}
          nodeClass={(n) => fill(n)}
          linkBackgroundClass={(l) => `bg-linear-to-r from-bar/25 ${to(nodes()[l.target])}`}
          hoverDim={1 - INACTIVE}
          // one line, name then count: two lines over a middle node reached the bar of the node above it
          nodeLabel={(n) => `${axisName(name(n))} · ${count(through(n))}`}
          // beside the first and last columns' nodes, in the gutters; over a middle node, in the gap
          // above it, on the card's colour (the margin, not the padding, is the gap to its node)
          nodeLabelClass={LABEL.node}
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
  const bins = () => p().rate.map((_, i) => `${i}`).slice(from(), to() + 1);
  // a row's name keeps its end (what tells two prefixes apart); the tooltip has it whole
  const named = () => prefixNames(p().prefixes, GUTTER_CHARS);
  const names = () => p().prefixes.map((x) => (x.prefix === "other" ? t("flows.otherPrefixes") : named().name(x.prefix)));
  const whole = (i: number) => {
    const x = p().prefixes[i];
    return !x ? "" : x.prefix === "other" ? t("flows.otherPrefixes") : prefixTitle(x);
  };
  const rows = () => p().heat.filter((c) => c.bin >= from() && c.bin <= to()).map((c) => ({ x: `${c.bin}`, y: names()[c.prefix] ?? "", n: c.n, bin: c.bin, prefix: c.prefix }));
  const hovered = signal<number | null>(null);
  // of the whole session, so a stretch keeps the colors it had
  const max = () => p().heat.reduce((m, r) => Math.max(m, r.n), 1);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: whole(r.prefix), lines: [`+${duration(r.bin * p().binMs)} – +${duration((r.bin + 1) * p().binMs)}`, t("sessions.requestsN", { n: r.n })] } : null;
  };
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
        xScale={{ type: "band", domain: bins(), paddingInner: CELLS.heat }}
        yScale={{ type: "band", domain: names(), paddingInner: CELLS.heat }}
        // by the square root: one hot cell does not wash out the rest
        colorScale={{ type: "sequential", domain: [0, Math.sqrt(max())], range: HEAT_RAMP() }}
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
        <Cell x="x" y="y" value={(_, i) => Math.sqrt(rows()[i]?.n ?? 0)} cornerRadii={TIP.cell} inactiveOpacity={INACTIVE} stagger={0.2} hoverCellClass={HOVER} />
        <PlotAxis placement="left" labelClass={AXIS_LABEL} />
        <TimeAxis domain={[from() * p().binMs, (to() + 1) * p().binMs]} slices={bins().length} />
      </Plot>
      </If>
      <PrefixNote prefixes={p().prefixes} common={named().common} gutter />
      </View>
    </If>
  );
}

/** The band over a group's areas, its name in it (a map of several buckets' prefixes). */
const GROUP_BAND = 18;

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

export function PrefixUse(props: { profile: Profile; measure?: PrefixMeasureKind; /** each prefix's busiest second against S3's limit, as the signals count it */ rates?: Signals["prefixes"] }) {
  const measure = () => props.measure ?? "requests";
  const items = () => props.profile.prefixes.filter((x) => x.prefix !== "other");
  // of S3's limit for a prefix (5,500 reads, 3,500 writes a second): the signals' count, or (an older share) the profile's peaks
  const limit = (x: Profile["prefixes"][number]) => nearestLimit(props.rates?.find((r) => r.prefix === x.prefix) ?? { readShare: x.peakReads / READS_PER_S, writeShare: x.peakWrites / WRITES_PER_S });
  const share = (x: Profile["prefixes"][number]) => limit(x).share;
  const nearest = () => [...items()].sort((a, b) => share(b) - share(a))[0];
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
  // the areas with room for their name whole (and their figure under it): the others say nothing, their tooltip does
  const size = signal<[number, number]>([0, 0]);
  const room = () => areasWithRoom(rows(), size()[0], size()[1], TIP.areaGap, nested() ? GROUP_BAND : 0, (i) => rows()[i].label.length);
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
            onLayout={({ bounds }) => size.set([bounds.size.width, bounds.size.height])}
            class={TREEMAP}
          >
            <Treemap
              id="id"
              parent="parent"
              value="value"
              paddingInner={TIP.areaGap}
              paddingTop={nested() ? GROUP_BAND : 0}
              cornerRadii={TIP.area}
              leafClass={(_, i) => (rows()[i]?.hot ? "fill-warn/60" : "fill-bar/55")}
              groupClass="fill-muted"
              groupLabel="label"
              // emphasis: a bucket's name over the prefixes in it
              groupLabelClass={LABEL.group}
              inactiveOpacity={INACTIVE}
              label={(_, i) => (room()[i] === "none" ? "" : rows()[i].label)}
              valueLabel={(_, v, i) => (room()[i] === "both" ? shown(v) : "")}
              labelClass={LABEL.area}
              valueLabelClass={LABEL.areaValue}
              hoverClass={HOVER}
            />
          </Plot>
          <If when={items().some((x) => share(x) >= 0.5)}>
            <Legend items={[{ label: t("prefixUse.hot"), swatch: `${TONE.warn.fill}/60` }]} />
          </If>
          {/* how near the busiest second came to what S3 serves one prefix */}
          <If when={nearest() && share(nearest()!) > 0}>
            <ChartNote testID="prefix-limit">{t(`prefixUse.limit.${limit(nearest()!).kind}`, { prefix: named().name(nearest()!.prefix), pct: percent(share(nearest()!)) })}</ChartNote>
          </If>
          <PrefixNote prefixes={items()} common={named().common} />
        </View>
      </If>
    </If>
  );
}

/** What the session's requests cost, by kind: a bar each, dearest
 * first, with its share of the whole. */
export function CostBy(props: { signals: Signals; max?: number }) {
  const c = () => props.signals.cost;
  const rows = () => c().kinds.filter((x) => x.cost > 0).slice(0, props.max ?? 8);
  const hovered = signal<number | null>(null);
  const label = () => (rows()[0] ? t("costBy.text", { op: rows()[0].kind, pct: percent(rows()[0].share), total: cost(c().amount, c().currency) }) : "");
  // every row's cost in one unit (all of them thousandths of a cent, or none): the figure at the bar, the unit once under the chart
  const parts = () => rows().map((r) => costParts(r.cost, c().currency));
  const unit = () => (parts().length && parts().every((x) => x.per1000 === parts()[0].per1000) ? parts()[0].unit : null);
  const money = (v: number) => (unit() ? costParts(v, c().currency).figure : cost(v, c().currency));
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: r.kind, lines: [`${cost(r.cost, c().currency)} · ${percent(r.share)}`, t("costBy.line", { n: count(r.billed), class: t(`costBy.${r.class}`) })] } : null;
  };
  return (
    <If when={!textual()} else={<SimpleTable columns={[t("column.kind"), t("figure.requests"), t("profile.cost"), t("profile.share")]} rows={rows().map((r) => [r.kind, count(r.requests), cost(r.cost, c().currency), percent(r.share)])} />}>
      <View class="gap-1">
      <Plot
        testID="profile-cost"
        data={rows()}
        yScale={{ type: "band", ...BARS.rows }}
        xScale={{ type: "linear", domain: [0, Math.max(...rows().map((r) => r.cost), 1e-12) * 1.6] }}
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
        {/* what each cost, in money, then its share: the bar is of the money (the unit is said once, under the chart) */}
        <Bar value="cost" class="fill-chart-1" cornerRadii={TIP.across} inactiveOpacity={INACTIVE} stagger={0.3} label={(_, i) => `${money(rows()[i].cost)} · ${percent(rows()[i].share)}`} labelClass={LABEL.bar} />
        <PlotAxis placement="left" format={(i) => axisName(rows()[i]?.kind ?? "")} labelClass={AXIS_LABEL} />
      </Plot>
      <If when={unit()}>{(u) => <ChartNote gutter testID="cost-unit">{t("costBy.unit", { unit: u() })}</ChartNote>}</If>
      </View>
    </If>
  );
}
