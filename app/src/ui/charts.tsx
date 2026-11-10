// Capacity's charts, on Native Shell's Plot in windows; in a terminal, a
// table and a sentence. Their frame, motion and tooltip are the session
// charts' (`plots.tsx`): one chart language in the app.

import { AXIS_LABEL, GUTTER, MOTION } from "./plots";
import { signal, Text, View } from "nsh";
import { Bar, Plot, PlotAxis, Treemap } from "nsh/base/plot";
import { bytes, count, pct } from "../format";
import { t } from "../i18n";
import { ChartTip, Legend, SimpleTable, terminal, textual } from "./kit";

const PALETTE = ["chart-1", "chart-2", "chart-3", "chart-4", "chart-5"] as const;
// plots.tsx's own (not exported there): a row's pitch, the band under the pointer, how far the rest fades
const PITCH = 28;
const CROSS_BAND = "w-full h-full rounded-(--radius-sm) bg-foreground/5";
const INACTIVE = 0.4;

/** Bars across of `items`, the value and its share at each bar's end. */
export function BarChart(props: { items: { label: string; value: number; sub?: string }[]; height?: number; testID?: string; unit?: "bytes" | "count" }) {
  const fmt = (v: number) => (props.unit === "count" ? count(v) : bytes(v));
  const total = () => props.items.reduce((s, i) => s + i.value, 0);
  const hovered = signal<number | null>(null);
  const figures = (v: number) => `${fmt(v)} · ${pct(v, total())}`;
  const sentence = () => (props.items.length > 0 ? t("chart.shareOf", { what: props.items[0].label, pct: pct(props.items[0].value, total()), of: fmt(total()) }) : "");
  const tip = () => {
    const i = hovered() === null ? null : props.items[hovered()!];
    return i ? { title: i.label, lines: [figures(i.value)] } : null;
  };
  return (
    <If
      when={!terminal()}
      else={
        <View class="gap-1">
          <SimpleTable columns={["", t("chart.value"), t("profile.share")]} rows={props.items.map((i) => [i.label, fmt(i.value), pct(i.value, total())])} />
          <Text class="text-small text-muted-foreground">{sentence()}</Text>
        </View>
      }
    >
      <Plot
        testID={props.testID}
        data={props.items}
        yScale={{ type: "band", paddingInner: 0.3, paddingOuter: 0.15 }}
        xScale={{ type: "linear", domain: [0, Math.max(1, ...props.items.map((i) => i.value)) * 1.25] }}
        accessibilityLabel={sentence()}
        accessibilityValue={{ text: tip() ? `${tip()!.title}: ${tip()!.lines[0]}` : "" }}
        appearGeneration={total()}
        motion={MOTION}
        crossLineClass={CROSS_BAND}
        tooltipPlacement="pointer"
        tooltipOffset={10}
        tooltipContent={<If when={tip()}>{(x) => <ChartTip title={x().title} lines={x().lines} />}</If>}
        onHoverChange={({ index }) => hovered.set(index)}
        class={GUTTER}
        style={{ height: props.height ?? Math.max(48, props.items.length * PITCH + 8) }}
      >
        <Bar value="value" class="fill-chart-1" cornerRadii={{ topRight: 3, bottomRight: 3 }} inactiveOpacity={INACTIVE} label={(_, i) => figures(props.items[i].value)} labelClass="pl-2 tabular-nums text-caption text-muted-foreground" />
        <PlotAxis placement="left" format={(i) => props.items[i]?.label ?? ""} labelClass={AXIS_LABEL} />
      </Plot>
    </If>
  );
}

/** One bar of the parts of a whole, with a legend. */
export function StackBar(props: { parts: { label: string; value: number }[]; testID?: string; format?: (v: number) => string }) {
  const fmt = (v: number) => (props.format ?? bytes)(v);
  const sum = () => props.parts.reduce((s, p) => s + p.value, 0);
  const total = () => Math.max(1, sum());
  const row = () => [Object.fromEntries(props.parts.map((p, i) => [`p${i}`, p.value]))];
  const part = (p: { label: string; value: number }) => `${p.label} ${fmt(p.value)} (${pct(p.value, total())})`;
  const hovered = signal<number | null>(null);
  return (
    <View class="gap-2">
      <If when={!terminal()}>
        <Plot
          testID={props.testID}
          data={row()}
          yScale={{ type: "band" }}
          xScale={{ type: "linear", domain: [0, total()] }}
          accessibilityLabel={props.parts.map((p) => `${p.label} ${fmt(p.value)}`).join(", ")}
          appearGeneration={total()}
          motion={MOTION}
          // the whole, and each part of it: one row, so the pointer is over all of them
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={
            <If when={hovered() !== null}>
              <ChartTip title={fmt(sum())} lines={props.parts.map(part)} />
            </If>
          }
          onHoverChange={({ index }) => hovered.set(index)}
          class="h-3.5 overflow-hidden rounded-full bg-muted"
        >
          <Each of={props.parts}>{(_, i) => <Bar value={`p${i()}`} stack="parts" class={`fill-${PALETTE[i() % PALETTE.length]}`} />}</Each>
        </Plot>
      </If>
      <Legend square items={props.parts.map((p, i) => ({ label: part(p), swatch: `bg-${PALETTE[i % PALETTE.length]}` }))} />
    </View>
  );
}

/** Where a bucket's bytes are: a rectangle per prefix, its area its size. */
export function PrefixMap(props: { items: { label: string; value: number }[]; testID?: string }) {
  const total = () => props.items.reduce((s, i) => s + i.value, 0);
  const hovered = signal<number | null>(null);
  const sentence = () => (props.items.length > 0 ? t("chart.shareOf", { what: props.items[0].label, pct: pct(props.items[0].value, total()), of: bytes(total()) }) : "");
  // nested: each prefix under its top folder, so a folder reads as the sum of its parts
  const top = (label: string) => label.split("/")[0] || label;
  const groups = () => [...new Set(props.items.map((i) => top(i.label)))];
  const nested = () => groups().some((g) => props.items.filter((i) => top(i.label) === g).length > 1);
  const rows = () =>
    nested()
      ? [
          ...groups().map((g) => ({ id: `g:${g}`, parent: "", label: `${g}/`, value: 0, group: g })),
          ...props.items.map((i) => ({ id: i.label, parent: `g:${top(i.label)}`, label: i.label.slice(top(i.label).length + 1) || i.label, value: i.value, group: top(i.label) })),
        ]
      : props.items.map((i) => ({ id: i.label, parent: "", label: i.label, value: i.value, group: i.label }));
  const color = (g: string) => PALETTE[groups().indexOf(g) % PALETTE.length];
  // a folder's size is the sum of what is under it
  const sizeOf = (r: { id: string; value: number; group: string }) => (r.id.startsWith("g:") && nested() ? props.items.filter((i) => top(i.label) === r.group).reduce((s, i) => s + i.value, 0) : r.value);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: r.id.startsWith("g:") && nested() ? r.label : r.id, lines: [`${bytes(sizeOf(r))} · ${pct(sizeOf(r), total())}`] } : null;
  };
  return (
    <If when={!textual()} else={<BarChart items={props.items} />}>
      <Plot
        testID={props.testID}
        data={rows()}
        accessibilityLabel={sentence()}
        accessibilityValue={{ text: tip() ? `${tip()!.title}, ${tip()!.lines[0]}` : "" }}
        appearGeneration={total()}
        motion={MOTION}
        tooltipPlacement="pointer"
        tooltipOffset={10}
        tooltipContent={<If when={tip()}>{(x) => <ChartTip title={x().title} lines={x().lines} />}</If>}
        onHoverChange={({ index }) => hovered.set(index)}
        class="h-64"
      >
        <Treemap
          id="id"
          parent="parent"
          value="value"
          paddingInner={2}
          paddingTop={nested() ? 18 : 0}
          cornerRadii={4}
          leafClass={(_, i) => `fill-${color(rows()[i].group)}/70`}
          groupClass={(_, i) => `fill-${color(rows()[i].group)}/15`}
          groupLabel="label"
          groupLabelClass="px-1.5 text-caption font-semibold text-foreground"
          inactiveOpacity={0.55}
          label="label"
          valueLabel={(_, v) => `${bytes(v)} · ${pct(v, total())}`}
          labelClass="p-1.5 text-small font-medium text-foreground"
          valueLabelClass="text-caption font-normal text-muted-foreground"
          hoverClass="rounded-(--radius-sm) outline-2 outline-foreground"
        />
      </Plot>
    </If>
  );
}
