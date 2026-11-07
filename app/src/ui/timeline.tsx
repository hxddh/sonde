// A session in time, drawn on a Canvas: one row per operation, its bar from
// start to end in its outcome's color, a time axis from the session's
// start. The pointer resting on a row shows what it is; a drag zooms to the
// time it covers, a double click zooms back out; a press chooses the
// operation, as the table's rows do.

import { Canvas, type CanvasRenderingContext2D, Scroll, signal, Text, token, View } from "nsh";
import { bytes, ms } from "../format";
import { episodeTitle } from "../phrases";
import { t } from "../i18n";
import type { Episode, SessionView } from "../types";
import { Button } from "./kit";
import { clock, duration, episodeTone, span, type Tone } from "./sessionview";

const ROW = 22;
const AXIS = 26;
const LABEL = 260;
const PAD = 16;

const TONE_TOKEN: Record<Tone, string> = { ok: "bar", miss: "status-miss", client: "status-client", server: "status-server", refused: "status-refused" };
const time = (iso: string) => new Date(iso).getTime();

/** A step for ticks about `px` points apart: 1, 2 or 5 times a power of ten (ms). */
function step(rangeMs: number, plotPx: number): number {
  const target = (rangeMs / plotPx) * 90;
  const p = 10 ** Math.floor(Math.log10(Math.max(target, 0.001)));
  for (const m of [1, 2, 5, 10]) if (m * p >= target) return m * p;
  return 10 * p;
}

const offset = (msFromStart: number) => (msFromStart < 1000 ? `${Math.round(msFromStart)} ms` : `${(msFromStart / 1000).toFixed(msFromStart < 10000 ? 2 : 1)} s`);

function fit(cx: CanvasRenderingContext2D, text: string, max: number): string {
  if (cx.measureText(text).width <= max) return text;
  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (cx.measureText(`${text.slice(0, mid)}…`).width <= max) lo = mid;
    else hi = mid - 1;
  }
  return `${text.slice(0, lo)}…`;
}

export function Timeline(props: { view: SessionView; rows?: number[]; selected: number | null; onSelect: (i: number) => void }) {
  const width = signal(800);
  const zoom = signal<[number, number] | null>(null);
  const hover = signal<{ row: number; x: number; y: number } | null>(null);
  const brush = signal<[number, number] | null>(null);
  const shown = () => props.rows ?? props.view.episodes.map((_, i) => i);
  const full = () => span(props.view);
  const range = () => zoom() ?? full();
  const plot = () => Math.max(60, width() - LABEL - PAD);
  const x = (ms: number) => LABEL + ((ms - range()[0]) / (range()[1] - range()[0])) * plot();
  const at = (px: number) => range()[0] + ((px - LABEL) / plot()) * (range()[1] - range()[0]);
  const height = () => AXIS + shown().length * ROW + 8;
  const rowAt = (y: number) => {
    const r = Math.floor((y - AXIS) / ROW);
    return r >= 0 && r < shown().length ? r : null;
  };
  const episode = (row: number): Episode | undefined => props.view.episodes[shown()[row]];

  const draw = (cx: CanvasRenderingContext2D, size: { width: number; height: number }) => {
    const w = size.width;
    const [r0, r1] = range();
    const color = (name: string) => token(name as never) as unknown as string;
    // rows
    shown().forEach((i, row) => {
      const y = AXIS + row * ROW;
      if (props.selected === i) {
        cx.fillStyle = color("selection");
        cx.fillRect(0, y, w, ROW);
      } else if (hover()?.row === row) {
        cx.fillStyle = color("row-hover");
        cx.fillRect(0, y, w, ROW);
      }
    });
    // ticks under the bars
    const s = step(r1 - r0, plot());
    const first = Math.ceil((r0 - full()[0]) / s) * s + full()[0];
    cx.font = "11px system-ui";
    cx.textBaseline = "middle";
    cx.textAlign = "center";
    for (let tick = first; tick <= r1; tick += s) {
      const tx = x(tick);
      cx.fillStyle = color("border");
      cx.fillRect(tx, AXIS - 4, 1, size.height - AXIS + 4);
      cx.fillStyle = color("muted-foreground");
      cx.fillText(offset(tick - full()[0]), tx, AXIS / 2);
    }
    cx.fillStyle = color("border");
    cx.fillRect(0, AXIS - 1, w, 1);
    // labels and bars
    cx.textAlign = "left";
    shown().forEach((i, row) => {
      const e = props.view.episodes[i];
      const y = AXIS + row * ROW;
      const tone = episodeTone(e);
      cx.font = "12px system-ui";
      cx.fillStyle = e.outcome.kind === "ok" ? color("foreground") : color(TONE_TOKEN[tone]);
      cx.fillText(fit(cx, episodeTitle(e), LABEL - 20), 10, y + ROW / 2);
      const a = Math.max(LABEL, x(time(e.start)));
      const b = Math.min(w - 4, x(time(e.end)));
      if (b < LABEL || a > w - 4) return;
      cx.fillStyle = color(TONE_TOKEN[tone]);
      cx.beginPath();
      cx.roundRect(a, y + 6, Math.max(3, b - a), ROW - 12, 3);
      cx.fill();
    });
    // the label column's edge
    cx.fillStyle = color("border");
    cx.fillRect(LABEL - 8, 0, 1, size.height);
    // a drag in progress
    const br = brush();
    if (br) {
      const [p, q] = br[0] < br[1] ? br : [br[1], br[0]];
      cx.fillStyle = color("primary");
      cx.globalAlpha = 0.14;
      cx.fillRect(p, AXIS, q - p, size.height - AXIS);
      cx.globalAlpha = 1;
    }
  };

  /** Ends a drag: zooms to what it covered; true when it was a drag, not a press. */
  const finish = () => {
    const br = brush();
    brush.set(null);
    if (!br || Math.abs(br[1] - br[0]) <= 6) return false;
    const [p, q] = br[0] < br[1] ? br : [br[1], br[0]];
    zoom.set([at(p), at(q)]);
    return true;
  };

  const card = () => {
    const h = hover();
    if (!h || brush()) return null;
    const e = episode(h.row);
    return e ? { e, h } : null;
  };

  return (
    <View testID="timeline" class="min-h-0 flex-1 overflow-hidden rounded-(--radius-card) border border-border bg-card">
      <View class="h-9 flex-row items-center gap-3 border-b border-border px-3">
        <Text class="flex-1 text-xs text-muted-foreground">
          {zoom() ? t("timeline.zoomed", { a: offset(range()[0] - full()[0]), b: offset(range()[1] - full()[0]), total: offset(full()[1] - full()[0]) }) : t("timeline.dragToZoom")}
        </Text>
        <If when={zoom()}>
          <Button small variant="ghost" icon="zoom-out" label={t("timeline.resetZoom")} onPress={() => zoom.set(null)} testID="timeline-reset" />
        </If>
      </View>
      <Scroll class="min-h-0 flex-1">
        <View onLayout={(e) => width.set(e.bounds.size.width)}>
          <Canvas
            accessibilityLabel={t("sessions.timeline")}
            style={{ height: height() }}
            draw={draw}
            onMouseMove={(e) => {
              const row = rowAt(e.position.y);
              // the button let go somewhere a press does not reach
              if (!e.pressedButton && brush()) finish();
              if (e.pressedButton === "left" && e.position.x >= LABEL) {
                const start = brush()?.[0] ?? e.position.x;
                brush.set([start, Math.max(LABEL, e.position.x)]);
              }
              hover.set(row === null ? null : { row, x: e.position.x, y: e.position.y });
            }}
            onMouseLeave={() => hover.set(null)}
            onPress={(e) => {
              if (finish()) return;
              if (e.clickCount === 2) {
                zoom.set(null);
                return;
              }
              const row = rowAt(e.position.y);
              if (row !== null) props.onSelect(shown()[row]);
            }}
          />
          <If when={card()}>
            {(c) => (
              <View
                testID="timeline-card"
                style={{ position: "absolute", left: Math.min(Math.max(LABEL, c().h.x + 14), width() - 280), top: AXIS + (c().h.row + 1) * ROW + 4, pointerEvents: "none" }}
                class="w-[264px] gap-1 rounded-(--radius) border border-border bg-popover px-3 py-2 shadow-lg"
              >
                <Text class="text-xs font-semibold text-popover-foreground line-clamp-2">{episodeTitle(c().e)}</Text>
                <Text class="font-mono text-[11px] text-muted-foreground">{`${clock(c().e.start)} · +${offset(time(c().e.start) - full()[0])}`}</Text>
                <Text class="font-mono text-[11px] text-muted-foreground">{`${ms(duration(c().e))} · ${t("sessions.requestsN", { n: c().e.records.length })} · ↑${bytes(c().e.bytesUp)} ↓${bytes(c().e.bytesDown)}`}</Text>
              </View>
            )}
          </If>
        </View>
      </Scroll>
    </View>
  );
}
