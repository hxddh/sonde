// Charts: drawn on a Canvas in windows; in a terminal, a table and a sentence.

import { Canvas, Text, token, View } from "nsh";
import { bytes, pct } from "../format";
import { SimpleTable, terminal } from "./kit";

const PALETTE = ["chart-1", "chart-2", "chart-3", "chart-4", "chart-5"] as const;

/** Horizontal bars of `items`, with their share of the largest. */
export function BarChart(props: { items: { label: string; value: number; sub?: string }[]; height?: number; testID?: string; unit?: "bytes" | "count" }) {
  const fmt = (v: number) => (props.unit === "count" ? String(v) : bytes(v));
  const total = () => props.items.reduce((s, i) => s + i.value, 0);
  return (
    <If
      when={!terminal()}
      else={
        <View class="gap-1">
          <SimpleTable columns={["", "value", "share"]} rows={props.items.map((i) => [i.label, fmt(i.value), pct(i.value, total())])} />
          <Text class="text-small text-muted-foreground">
            {props.items.length > 0 ? `${props.items[0].label}: ${pct(props.items[0].value, total())} of ${fmt(total())}` : ""}
          </Text>
        </View>
      }
    >
      <Canvas
        testID={props.testID}
        accessibilityLabel={props.items.map((i) => `${i.label} ${fmt(i.value)}`).join(", ")}
        style={{ height: props.height ?? Math.max(60, props.items.length * 26 + 8), width: "100%" }}
        draw={(cx, size) => {
          const max = Math.max(1, ...props.items.map((i) => i.value));
          const labelW = Math.min(220, size.width * 0.38);
          const barW = size.width - labelW - 90;
          cx.font = "12px sans-serif";
          cx.textBaseline = "middle";
          props.items.forEach((item, i) => {
            const y = 4 + i * 26;
            cx.fillStyle = token("muted-foreground");
            cx.fillText(item.label.length > 32 ? `${item.label.slice(0, 31)}…` : item.label, 0, y + 10);
            cx.fillStyle = token("muted");
            cx.fillRect(labelW, y + 2, barW, 16);
            cx.fillStyle = token(PALETTE[i % PALETTE.length]);
            cx.fillRect(labelW, y + 2, Math.max(2, (item.value / max) * barW), 16);
            cx.fillStyle = token("foreground");
            cx.fillText(fmt(item.value), labelW + barW + 8, y + 10);
          });
        }}
      />
    </If>
  );
}

/** One stacked bar of parts of a whole, with a legend. */
export function StackBar(props: { parts: { label: string; value: number }[]; testID?: string }) {
  const total = () => Math.max(1, props.parts.reduce((s, p) => s + p.value, 0));
  return (
    <View class="gap-2">
      <If when={!terminal()}>
        <Canvas
          testID={props.testID}
          accessibilityLabel={props.parts.map((p) => `${p.label} ${bytes(p.value)}`).join(", ")}
          style={{ height: 18, width: "100%" }}
          draw={(cx, size) => {
            let x = 0;
            props.parts.forEach((p, i) => {
              const w = (p.value / total()) * size.width;
              cx.fillStyle = token(PALETTE[i % PALETTE.length]);
              cx.fillRect(x, 0, w, size.height);
              x += w;
            });
          }}
        />
      </If>
      <View class="flex-row flex-wrap gap-x-4 gap-y-1">
        <Each of={props.parts}>
          {(p, i) => (
            <View class="flex-row items-center gap-1.5">
              <View class={`h-2.5 w-2.5 rounded-sm bg-${PALETTE[i() % PALETTE.length]}`} />
              <Text class="text-small text-muted-foreground">{`${p().label} ${bytes(p().value)} (${pct(p().value, total())})`}</Text>
            </View>
          )}
        </Each>
      </View>
    </View>
  );
}
