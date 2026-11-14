// Errors: what failed over the session, by class.

import { signal, Text } from "nsh";
import { Bar, Dot, Plot, Rule } from "nsh/base/plot";
import { count, duration, percent } from "../../format";
import { t } from "../../i18n";
import type { Signals } from "../../types";
import { CROSS_BAND, ChartTip, type Focus, INACTIVE, type LegendItem, MOTION, NoneInStretch, PLOT, TICK_ROOM, nearest, stretch, tipValue, TimeAxis, ValueAxis, BARS, TIP, sinceTick, Budget, budgetTop } from "./system";
import { terminal } from "../kit";
import { errorRateSaid, failuresSaid } from "./answers";

/** The classes a failure is of, in the order they stack: the store's own first, a scenario's last. */
const FAILURES = ["server", "client", "transport", "injected"] as const;
export const FAILURE_FILL: Record<(typeof FAILURES)[number], string> = { server: "fill-status-server", client: "fill-status-client", transport: "fill-status-refused", injected: "fill-status-injected" };
const FAILURE_SWATCH: Record<(typeof FAILURES)[number], string> = { server: "bg-status-server", client: "bg-status-client", transport: "bg-status-refused", injected: "bg-status-injected" };

/** What the failures chart's colors stand for: the classes the session had, each with its count, and its retries. */
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
        const failed = (e().server[i] ?? 0) + (e().client[i] ?? 0) + (e().transport[i] ?? 0) + (e().injected[i] ?? 0);
        return { tMs: i * s().binMs, n: Math.max(n, failed), failed, share: failed > 0 ? failed / Math.max(n, failed) : 0 };
      }),
      props.range,
    );
  const peak = () => Math.max(0.01, ...rows().map((r) => r.share));
  // one failure, one colour: in the client's tone where every failure was the program's own (4xx), as the counts beside draw it
  const tone = () => { const c = e().counts; return c.server + c.client + c.transport === 0 && c.injected > 0 ? FAILURE_FILL.injected : c.client > 0 && c.server + c.transport === 0 ? FAILURE_FILL.client : FAILURE_FILL.server; };
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
          yScale={{ type: "linear", domain: [0, budgetTop(peak() * 1.25, props.budgetShare)] }}
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
          <Rule value={(_, i) => (i === focused() ? budgetTop(peak() * 1.25, props.budgetShare) : (undefined as unknown as number))} class="stroke-foreground/40 stroke-1" />
          <TimeAxis domain={[rows()[0]?.tMs ?? 0, (rows()[rows().length - 1]?.tMs ?? 0) + s().binMs]} slices={rows().length} />
          <ValueAxis peak={peak()} say={(v) => t("errorRate.peak", { pct: percent(v) })} />
          <Budget value={props.budgetShare} say={percent} />
        </Plot>
      </If>
    </If>
  );
}

/** Failures over the session by class — the store's 5xx, the program's 4xx,
 * no answer, a fault scenario's own — stacked on the timeline's slices,
 * with the times a request was sent again as dots over them. */
export function ErrorsOverTime(props: { signals: Signals; range?: [number, number] | null; focus?: Focus }) {
  const s = () => props.signals;
  const e = () => s().errors;
  const rows = () => stretch(e().series.map((_, i) => ({ tMs: i * s().binMs, server: e().server[i] ?? 0, client: e().client[i] ?? 0, transport: e().transport[i] ?? 0, injected: e().injected[i] ?? 0, retries: e().retries[i] ?? 0 })), props.range);
  const total = (r: ReturnType<typeof rows>[number]) => r.server + r.client + r.transport + r.injected;
  const top = () => Math.max(1, ...rows().map((r) => Math.max(total(r), r.retries))) * 1.25;
  const hovered = signal<number | null>(null);
  const label = () => failuresSaid(s(), props.range);
  const tip = () => {
    const r = hovered() === null ? null : rows()[hovered()!];
    if (!r) return null;
    const parts = FAILURES.filter((k) => r[k] > 0).map((k) => t(`failures.${k}`, { n: count(r[k]) }));
    return { title: `+${duration(r.tMs)}`, lines: [parts.length ? parts.join(" · ") : t("signal.noErrors"), ...(r.retries > 0 ? [t("failures.retries", { n: count(r.retries) })] : [])] };
  };
  const focused = () => (hovered() === null ? nearest(rows(), props.focus?.() ?? null) : null);
  return (
    <If when={!terminal()} else={<Text class="text-foreground">{label()}</Text>}>
      <If when={rows().some((r) => total(r) + r.retries > 0)} else={<NoneInStretch />}>
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
          <Bar value="server" stack="f" class={FAILURE_FILL.server} inactiveOpacity={INACTIVE} />
          <Bar value="client" stack="f" class={FAILURE_FILL.client} inactiveOpacity={INACTIVE} />
          <Bar value="transport" stack="f" class={FAILURE_FILL.transport} inactiveOpacity={INACTIVE} />
          <Bar value="injected" stack="f" class={FAILURE_FILL.injected} cornerRadii={TIP.up} inactiveOpacity={INACTIVE} />
          {/* a retry is a mark, not a failure of its own: a dot at how many in the slice */}
          <Dot value={(r) => (r.retries > 0 ? r.retries : (undefined as unknown as number))} r={3} class="fill-info" hoverDotClass="hidden" />
          <Rule value={(_, i) => (i === focused() ? top() : (undefined as unknown as number))} class="stroke-foreground/40 stroke-1" />
          <TimeAxis domain={[rows()[0]?.tMs ?? 0, (rows()[rows().length - 1]?.tMs ?? 0) + s().binMs]} slices={rows().length} />
          <ValueAxis peak={top() / 1.25} say={(v) => t("failures.peak", { n: count(v) })} />
        </Plot>
      </If>
    </If>
  );
}
