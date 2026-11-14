// An operation's requests in time, and a session in a sentence.

import { signal } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { Bar, Plot, PlotAxis } from "nsh/base/plot";
import { duration } from "../../format";
import { t } from "../../i18n";
import type { Profile, RecordSummary } from "../../types";
import { AXIS_LABEL, CROSS_BAND, ChartNote, ChartTip, GUTTER, GUTTER_CHARS, INACTIVE, Legend, MOTION, TICK_ROOM, groupName, pct, rowsHeight, statusTone, tipValue, tones, tail, TimeAxis, BARS, TIP } from "./system";
import { terminal } from "../kit";

/** An operation's requests in time: a bar from each one's start to its end, so waits and overlaps show. */
/** A key as a row's name: its last two parts (`…/manifest/00012.json`). */
const short = (key: string) => {
  const parts = key.split("/").filter(Boolean);
  return parts.length > 2 ? `…/${parts.slice(-2).join("/")}` : key;
};

/** How many requests the waterfall draws, a row each at the one pitch. */
const WATERFALL_ROWS = 12;

export function Waterfall(props: { records: RecordSummary[]; onRecord?: (id: number) => void }) {
  // a loop, not a spread: an operation may hold more requests than a call takes arguments
  const t0 = () => props.records.reduce((m, r) => Math.min(m, new Date(r.time).getTime()), Infinity);
  const rows = () =>
    // the first few, in the order the list below has them: a bar is its row
    props.records.slice(0, WATERFALL_ROWS).map((r) => {
      const s = new Date(r.time).getTime() - t0();
      // the wait for the first byte, then the body: one bar in two shades
      const first = r.firstByteMs != null && r.firstByteMs < r.latencyMs ? r.firstByteMs : null;
      return { id: r.id, op: r.kind, target: r.target, start: s, firstEnd: s + (first ?? Math.max(r.latencyMs, 0.3)), end: s + Math.max(r.latencyMs, 0.3), tone: statusTone(r.status), status: r.status, ms: r.latencyMs, first };
    });
  const hovered = signal<number | null>(null);
  const end = () => Math.max(...rows().map((r) => r.end), 1);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: `${r.status ?? "—"} ${r.op}`, lines: [r.target, `+${duration(r.start)} · ${duration(r.ms)}`, ...(r.first != null ? [t("waterfall.firstByte", { ms: duration(r.first) })] : [])] } : null;
  };
  const anyFirst = () => rows().some((r) => r.first != null);
  return (
    <If when={!terminal() && props.records.length > 1}>
      <BaseButton
        testID="waterfall"
        accessibilityLabel={t("profile.waterfallText", { n: props.records.length, time: duration(end()) })}
        disabled={!props.onRecord}
        onPress={() => {
          const r = hovered() === null ? null : rows()[hovered()!];
          if (r) props.onRecord?.(r.id);
        }}
      >
        <Plot
          data={rows()}
          yScale={{ type: "band", ...BARS.rows }}
          xScale={{ type: "linear", domain: [0, end()] }}
          colorScale={tones()}
          accessibilityLabel={t("profile.waterfallText", { n: props.records.length, time: duration(end()) })}
          accessibilityValue={tipValue(tip())}
          appearGeneration={props.records[0]?.id}
          motion={MOTION}
          crossLineClass={CROSS_BAND}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => hovered.set(index)}
          class={`${TICK_ROOM} ${GUTTER}`}
          style={{ height: rowsHeight(rows().length) }}
        >
          {/* the body's part a lighter shade of the bar, the wait for its first byte the full one */}
          <Bar base="start" value="end" color="tone" cornerRadii={TIP.span} inactiveOpacity={INACTIVE} stagger={0.4} style={anyFirst() ? { opacity: 0.45 } : undefined} />
          <If when={anyFirst()}>
            <Bar base="start" value="firstEnd" color="tone" cornerRadii={TIP.span} inactiveOpacity={INACTIVE} stagger={0.4} />
          </If>
          <PlotAxis placement="left" format={(i) => tail(short(rows()[i]?.target ?? ""), GUTTER_CHARS - 2)} labelClass={`${AXIS_LABEL} font-mono text-caption`} />
          {/* from the operation's first request: lengths of time, on the one axis's ticks */}
          <TimeAxis domain={[0, end()]} lengths />
        </Plot>
        <If when={anyFirst() || props.records.length > WATERFALL_ROWS}>
          <Legend gutter items={anyFirst() ? [{ label: t("waterfall.firstByteKey"), swatch: "bg-bar" }, { label: t("waterfall.bodyKey"), swatch: "bg-bar/45" }] : []}>
            <If when={props.records.length > WATERFALL_ROWS}>
              <ChartNote>{t("waterfall.more", { n: props.records.length - WATERFALL_ROWS })}</ChartNote>
            </If>
          </Legend>
        </If>
      </BaseButton>
    </If>
  );
}

// --- what a session comes to, in a sentence ------------------------------------------

/** Whether a session's requests have times (a CloudTrail file's have none). */
export const timed = (p: Profile | null | undefined) => p?.timed !== false;

/** A session in a sentence: what it did, where its time went. What acting on
 * its findings would save is said once, with the findings (`whatIfText`), never here too. */
export function sessionStory(p: Profile): string {
  const top = p.groups[0];
  const busiest = [...p.groups].sort((a, b) => b.requests - a.requests)[0];
  const total = p.groups.reduce((n, g) => n + g.requests, 0) || 1;
  if (!timed(p)) return busiest ? t("story.untimed", { what: groupName(busiest), pct: pct(busiest.requests / total) }) : "";
  return top ? t("story.time", { busy: duration(p.waitingMs), wall: duration(p.wallMs), what: groupName(top), pct: pct(top.share) }) : "";
}
