// Errors: what failed over the run, by class.

import { signal, Text } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { Bar, Dot, Plot, Rule } from "nsh/base/plot";
import { count, duration, percent } from "../../format";
import { t } from "../../i18n";
import type { Signals } from "../../types";
import { CROSS_BAND, ChartTip, type Focus, FOCUS_RULE, fitted, HEADROOM, INACTIVE, type LegendItem, MOTION, NoneInStretch, PLOT, TICK_ROOM, nearest, stretch, tipValue, TimeAxis, ValueAxis, BARS, TIP, sinceTick, Budget, budgetTop, slotMs } from "./system";
import { terminal } from "../kit";
import { errorRateSaid, failuresSaid } from "./answers";

/** The classes a failure is of, in the order they stack: the store's own first (a throttle, the store asking the
 * program to slow down, in the warn tone, apart from its other 5xx), a scenario's last. */
const FAILURES = ["throttled", "server", "client", "transport", "injected"] as const;
export const FAILURE_FILL: Record<(typeof FAILURES)[number], string> = { throttled: "fill-warn/55", server: "fill-status-server", client: "fill-status-client", transport: "fill-status-refused", injected: "fill-status-injected" };
const FAILURE_SWATCH: Record<(typeof FAILURES)[number], string> = { throttled: "bg-warn/55", server: "bg-status-server", client: "bg-status-client", transport: "bg-status-refused", injected: "bg-status-injected" };

/** The slices the failures charts show of a whole run: those around its failures and retries (`fitted`), or all. */
export const failedSlices = (s: Signals) => fitted(s.errors.series.length, (i) => (s.errors.series[i] ?? 0) + (s.errors.retries[i] ?? 0) > 0);

/** What the failures chart's colors stand for: the classes the run had, each with its count, and its retries. */
export function failureLegend(s: Signals): LegendItem[] {
  const c = s.errors.counts;
  return [
    ...FAILURES.filter((k) => c[k] > 0).map((k) => ({ label: t(`failures.${k}`, { n: count(c[k]) }), swatch: FAILURE_SWATCH[k] })),
    ...(s.errors.retriesTotal > 0 ? [{ label: t("failures.retries", { n: count(s.errors.retriesTotal) }), swatch: "bg-info" }] : []),
  ];
}

/** The share of each slice's requests that failed: the rate beside the
 * counts — two failures of two requests and two of two hundred are not one
 * height here. */
export function ErrorRate(props: { signals: Signals; range?: [number, number] | null; focus?: Focus; /** the share of the run's requests the project's `max-errors` allows: its line */ budgetShare?: number | null }) {
  const s = () => props.signals;
  const e = () => s().errors;
  const rows = () =>
    stretch(
      e().series.map((_, i) => {
        // the slice's requests, from its rate (a second's worth) and its length
        const n = Math.round(((s().rate.series[i] ?? 0) * s().binMs) / 1000);
        const failed = (e().series[i] ?? 0);
        return { tMs: slotMs(s(), i), n: Math.max(n, failed), failed, share: failed > 0 ? failed / Math.max(n, failed) : 0 };
      }),
      // the stretch chosen, or the slices around the failures (`fitted`): a few failures fill the chart
      props.range ?? failedSlices(s()),
    );
  const peak = () => Math.max(0.01, ...rows().map((r) => r.share));
  // one failure, one colour: in the client's tone where every failure was the program's own (4xx), as the counts beside draw it
  const tone = () => {
    const c = e().counts;
    const others = (k: keyof typeof c) => (Object.keys(c) as (keyof typeof c)[]).filter((x) => x !== k).every((x) => c[x] === 0);
    // every failure of one class: in its tone, as the counts beside draw it; else a failure's
    const one = FAILURES.find((k) => c[k] > 0 && others(k));
    return one ? FAILURE_FILL[one] : FAILURE_FILL.server;
  };
  const hovered = signal<number | null>(null);
  const label = () => errorRateSaid(s(), props.range);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    return r ? { title: sinceTick(r.tMs), lines: [t("errorRate.line", { pct: percent(r.share), failed: count(r.failed), n: count(r.n) })] } : null;
  };
  const focused = () => (hovered() === null ? nearest(rows(), props.focus?.() ?? null) : null);
  return (
    <If when={!terminal()} else={<Text class="text-foreground">{label()}</Text>}>
      <If when={rows().some((r) => r.failed > 0)} else={<NoneInStretch />}>
        <Plot
          testID="error-rate"
          data={rows()}
          xScale={{ type: "band", ...BARS.slices }}
          yScale={{ type: "linear", domain: [0, budgetTop(peak() * HEADROOM.plot, props.budgetShare)] }}
          accessibilityLabel={label()}
          accessibilityValue={tipValue(tip())}
          appearGeneration={1}
          motion={MOTION}
          crossLineClass={CROSS_BAND}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => {
            hovered.set(index);
            props.focus?.set(index === null ? null : rows()[index].tMs + s().binMs / 2);
          }}
          class={`${TICK_ROOM} ${PLOT} w-full`}
        >
          <Bar value="share" class={tone()} cornerRadii={TIP.up} inactiveOpacity={INACTIVE} />
          <Rule value={(_, i) => (i === focused() ? budgetTop(peak() * HEADROOM.plot, props.budgetShare) : (undefined as unknown as number))} class={FOCUS_RULE} />
          <TimeAxis domain={[rows()[0]?.tMs ?? 0, (rows()[rows().length - 1]?.tMs ?? 0) + s().binMs]} slices={rows().length} />
          <ValueAxis peak={peak()} say={(v) => t("errorRate.peak", { pct: percent(v) })} />
          <Budget value={props.budgetShare} say={percent} />
        </Plot>
      </If>
    </If>
  );
}

/** Failures over the run by class — the store's throttles and other 5xx,
 * the program's 4xx, no answer, a fault scenario's own — stacked on the
 * timeline's slices, with the times a request was sent again as dots over
 * them. A press on a slice's bar: the operations that failed in it. */
export function ErrorsOverTime(props: { signals: Signals; range?: [number, number] | null; focus?: Focus; onFailures?: (ms: [number, number]) => void }) {
  const s = () => props.signals;
  const e = () => s().errors;
  // the stretch chosen, or the slices around the failures (`fitted`), as the share's chart beside it
  const rows = () => stretch(e().series.map((_, i) => ({ tMs: slotMs(s(), i), throttled: e().throttled[i] ?? 0, server: e().server[i] ?? 0, client: e().client[i] ?? 0, transport: e().transport[i] ?? 0, injected: e().injected[i] ?? 0, retries: e().retries[i] ?? 0 })), props.range ?? failedSlices(s()));
  const total = (r: ReturnType<typeof rows>[number]) => r.throttled + r.server + r.client + r.transport + r.injected;
  const top = () => Math.max(1, ...rows().map((r) => Math.max(total(r), r.retries))) * HEADROOM.plot;
  const hovered = signal<number | null>(null);
  const label = () => failuresSaid(s(), props.range);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    if (!r) return null;
    const parts = FAILURES.filter((k) => r[k] > 0).map((k) => t(`failures.${k}`, { n: count(r[k]) }));
    return { title: `+${duration(r.tMs)}`, lines: [parts.length ? parts.join(" · ") : t("signal.noErrors"), ...(r.retries > 0 ? [t("failures.retries", { n: count(r.retries) })] : []), ...(props.onFailures && total(r) > 0 ? [t("failures.press")] : [])] };
  };
  const focused = () => (hovered() === null ? nearest(rows(), props.focus?.() ?? null) : null);
  return (
    <If when={!terminal()} else={<Text class="text-foreground">{label()}</Text>}>
      <If when={rows().some((r) => total(r) + r.retries > 0)} else={<NoneInStretch />}>
        <BaseButton
          testID="errors-press"
          accessibilityLabel={label()}
          disabled={!props.onFailures}
          onPress={() => {
            const r = hovered() === null ? null : rows()[hovered()!];
            if (r && total(r) > 0) props.onFailures?.([r.tMs, r.tMs + s().binMs]);
          }}
          class="rounded-(--radius)"
        >
        <Plot
          testID="errors-over-time"
          data={rows()}
          xScale={{ type: "band", ...BARS.slices }}
          yScale={{ type: "linear", domain: [0, top()] }}
          accessibilityLabel={label()}
          accessibilityValue={tipValue(tip())}
          appearGeneration={1}
          motion={MOTION}
          crossLineClass={CROSS_BAND}
          tooltipPlacement="pointer"
          tooltipOffset={10}
          tooltipContent={<ChartTip tip={tip()} />}
          onHoverChange={({ index }) => {
            hovered.set(index);
            props.focus?.set(index === null ? null : rows()[index].tMs + s().binMs / 2);
          }}
          class={`${TICK_ROOM} ${PLOT} w-full`}
        >
          <Bar value="throttled" stack="f" class={FAILURE_FILL.throttled} inactiveOpacity={INACTIVE} />
          <Bar value="server" stack="f" class={FAILURE_FILL.server} inactiveOpacity={INACTIVE} />
          <Bar value="client" stack="f" class={FAILURE_FILL.client} inactiveOpacity={INACTIVE} />
          <Bar value="transport" stack="f" class={FAILURE_FILL.transport} inactiveOpacity={INACTIVE} />
          <Bar value="injected" stack="f" class={FAILURE_FILL.injected} cornerRadii={TIP.up} inactiveOpacity={INACTIVE} />
          {/* a retry is a mark, not a failure of its own: a dot at how many in the slice */}
          <Dot value={(r) => (r.retries > 0 ? r.retries : (undefined as unknown as number))} r={3} class="fill-info" hoverDotClass="hidden" />
          <Rule value={(_, i) => (i === focused() ? top() : (undefined as unknown as number))} class={FOCUS_RULE} />
          <TimeAxis domain={[rows()[0]?.tMs ?? 0, (rows()[rows().length - 1]?.tMs ?? 0) + s().binMs]} slices={rows().length} />
          <ValueAxis peak={top() / HEADROOM.plot} say={(v) => t("failures.peak", { n: count(v) })} />
        </Plot>
        </BaseButton>
      </If>
    </If>
  );
}
