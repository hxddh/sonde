// Capacity's charts, on Native Shell's Plot in windows; in a terminal, a
// table and a sentence.

import { AXIS_LABEL, GUTTER } from "./plots";
import { signal, Text, View } from "nsh";
import { Bar, Plot, PlotAxis, Treemap } from "nsh/base/plot";
import { bytes, pct } from "../format";
import { t } from "../i18n";
import { Legend, SimpleTable, terminal, textual } from "./kit";

const PALETTE = ["chart-1", "chart-2", "chart-3", "chart-4", "chart-5"] as const;
const MOTION = { appear: { duration: 420, easing: "easeOut" as const }, enter: { duration: 100 }, exit: { duration: 160 } };

/** Bars across of `items`, the value and its share at each bar's end. */
export function BarChart(props: { items: { label: string; value: number; sub?: string }[]; height?: number; testID?: string; unit?: "bytes" | "count" }) {
  const fmt = (v: number) => (props.unit === "count" ? String(v) : bytes(v));
  const total = () => props.items.reduce((s, i) => s + i.value, 0);
  const sentence = () => (props.items.length > 0 ? t("chart.shareOf", { what: props.items[0].label, pct: pct(props.items[0].value, total()), of: fmt(total()) }) : "");
  return (
    <If
      when={!terminal()}
      else={
        <View class="gap-1">
          <SimpleTable columns={["", "value", "share"]} rows={props.items.map((i) => [i.label, fmt(i.value), pct(i.value, total())])} />
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
        appearGeneration={total()}
        motion={MOTION}
        crossLineClass="w-full h-full rounded-sm bg-foreground/5"
        class={GUTTER}
        style={{ height: props.height ?? Math.max(48, props.items.length * 28 + 8) }}
      >
        <Bar
          value="value"
          class="fill-chart-1"
          cornerRadii={{ topRight: 3, bottomRight: 3 }}
          inactiveOpacity={0.45}
          label={(_, i) => `${fmt(props.items[i].value)} · ${pct(props.items[i].value, total())}`}
          labelClass="pl-2 tabular-nums text-caption text-muted-foreground"
        />
        <PlotAxis placement="left" format={(i) => props.items[i]?.label ?? ""} labelClass={AXIS_LABEL} />
      </Plot>
    </If>
  );
}

/** One bar of the parts of a whole, with a legend. */
export function StackBar(props: { parts: { label: string; value: number }[]; testID?: string; format?: (v: number) => string }) {
  const fmt = (v: number) => (props.format ?? bytes)(v);
  const total = () => Math.max(1, props.parts.reduce((s, p) => s + p.value, 0));
  const row = () => [Object.fromEntries(props.parts.map((p, i) => [`p${i}`, p.value]))];
  return (
    <View class="gap-2">
      <If when={!terminal()}>
        <Plot
          testID={props.testID}
          data={row()}
          yScale={{ type: "band" }}
          xScale={{ type: "linear", domain: [0, total()] }}
          interactive={false}
          accessibilityLabel={props.parts.map((p) => `${p.label} ${fmt(p.value)}`).join(", ")}
          appearGeneration={total()}
          motion={MOTION}
          class="h-3.5 overflow-hidden rounded-full bg-muted"
        >
          <Each of={props.parts}>{(_, i) => <Bar value={`p${i()}`} stack="parts" class={`fill-${PALETTE[i() % PALETTE.length]}`} />}</Each>
        </Plot>
      </If>
      <Legend square items={props.parts.map((p, i) => ({ label: `${p.label} ${fmt(p.value)} (${pct(p.value, total())})`, swatch: `bg-${PALETTE[i % PALETTE.length]}` }))} />
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
  return (
    <If when={!textual()} else={<BarChart items={props.items} />}>
      <Plot
        testID={props.testID}
        data={rows()}
        accessibilityLabel={sentence()}
        accessibilityValue={{ text: hovered() === null ? "" : `${rows()[hovered()!]?.id}, ${bytes(rows()[hovered()!]?.value ?? 0)}` }}
        appearGeneration={total()}
        motion={MOTION}
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
          hoverClass="rounded-[4px] outline-2 outline-foreground"
        />
      </Plot>
    </If>
  );
}
