// A session's time: what the program waited on, by kind of request, and how
// long its requests took. The figures say it in words; the charts show it, in
// the order a question about S3 runs: when, how long, what pushed back, where
// the time and the money went, and whether a prefix ran into its limits.

import { batch, Scroll, signal, Text, View } from "nsh";
import { t } from "../i18n";
import type { SessionView } from "../types";
import { StackBar } from "./charts";
import { Button, CARD, terminal, Section, SimpleTable } from "./kit";
import { findingMarks } from "./sessionview";
import { Chains, cost, Distribution, Flows, Heat, LatencyBand, offset, PrefixLimits, Pressure, SessionShape, shapeSentence, TimeSpent } from "./plots";

/** The Time pane of a session. */
export function TimePane(props: { view: SessionView; onRequest?: (id: number) => void }) {
  const p = () => props.view.profile;
  // a stretch of the session, picked on the timeline by two presses
  const zoomFrom = signal<number | null>(null);
  const zoom = signal<[number, number] | null>(null);
  const pick = (i: number | null) => {
    // pressing the first slice again lets it go
    if (i === null) return zoomFrom.set(null);
    const from = zoomFrom();
    if (from === i && !zoom()) return zoomFrom.set(null);
    if (from === null) return zoomFrom.set(i);
    batch(() => {
      zoom.set([Math.min(from, i), Math.max(from, i)]);
      zoomFrom.set(null);
    });
  };
  const zoomMs = (): [number, number] | null => {
    const z = zoom();
    return z ? [z[0] * p().binMs, (z[1] + 1) * p().binMs] : null;
  };
  const costs = () => {
    const c = p().cost;
    return [
      { label: t("costs.writes"), value: (c.writes * p().prices.per1000Writes) / 1000 },
      { label: t("costs.reads"), value: (c.reads * p().prices.per1000Reads) / 1000 },
    ].filter((x) => x.value > 0);
  };
  const showFlows = signal(false);
  return (
    <Scroll testID="time-pane" class="min-h-0 flex-1" contentClass={terminal() ? "gap-2" : "gap-6 pb-4 pr-2"}>
      {/* the figures are the session header's; here, a terminal's sentence */}
      <If when={terminal()}>
        <Text class="text-foreground">{shapeSentence(p())}</Text>
      </If>
      {/* when: the whole session, its findings marked; two presses zoom the charts below */}
      <If when={!terminal()}>
        <Section title={t("timeline.title")} plain description={t("timeline.hint")}>
          <If when={zoom() || zoomFrom() !== null}>
            <View class="flex-row items-center gap-2">
              <Text testID="zoom-note" class="tabular-nums text-small text-foreground">
                {zoom() ? t("timeline.zoomed", { a: offset(zoomMs()![0]), b: offset(zoomMs()![1]) }) : t("timeline.zoomFrom", { t: offset((zoomFrom() ?? 0) * p().binMs) })}
              </Text>
              <Button small variant="ghost" icon="x" label={t("timeline.unzoom")} onPress={() => batch(() => (zoom.set(null), zoomFrom.set(null)))} testID="zoom-reset" />
            </View>
          </If>
          <SessionShape profile={p()} annotate axis marks={findingMarks(props.view)} range={zoom()} slice={zoomFrom()} onSlice={pick} class="h-32 w-full" testID="time-shape" />
        </Section>
      </If>
      <If when={!terminal() && p().rate.filter((b) => b.ok + b.miss + b.client + b.server > 0).length > 2}>
        <Section title={t("band.title")} plain description={t("live.latency")}>
          <LatencyBand profile={p()} class="h-28 w-full" testID="time-band" domainMs={zoomMs()} />
        </Section>
      </If>
      <Section title={t("pressure.title")} plain description={t("pressure.hint")}>
        <Pressure profile={p()} range={zoom()} />
      </Section>
      <If when={p().latency?.length}>
        <Section title={t("dist.title")} plain description={t("dist.hint")}>
          <Distribution profile={p()} />
        </Section>
      </If>
      <Section title={t("profile.spentTitle")} plain description={t("profile.spentHint")}>
        <TimeSpent profile={p()} />
      </Section>
      <If when={p().chains?.length}>
        <Section title={t("chains.title")} plain description={t("chains.hint")}>
          <Chains profile={p()} />
        </Section>
      </If>
      <If when={p().prefixes.some((x) => x.prefix !== "other" && x.peakPerSecond > 0)}>
        <Section title={t("limits.title")} plain description={t("limits.hint")}>
          <PrefixLimits profile={p()} />
        </Section>
      </If>
      <If when={p().heat?.length}>
        <Section title={t("heat.title")} plain description={t("heat.hint")}>
          <Heat profile={p()} />
        </Section>
      </If>
      <If when={costs().length > 0}>
        <Section title={t("costs.title")} plain description={t("costs.hint")}>
          <StackBar testID="cost-parts" parts={costs()} format={(v) => cost(v, p().cost.currency)} />
          <If when={p().prefixes.some((x) => (x.cost ?? 0) > 0)}>
            <SimpleTable columns={[t("costs.byPrefix"), t("figure.requests"), t("profile.cost")]} rows={p().prefixes.filter((x) => (x.cost ?? 0) > 0).map((x) => [x.prefix === "other" ? t("flows.otherPrefixes") : x.prefix, String(x.requests), cost(x.cost ?? 0, p().cost.currency)])} max={8} />
          </If>
        </Section>
      </If>
      {/* where requests went, operation → prefix → outcome: on demand */}
      <If when={p().flows?.length}>
        <Section title={t("flows.title")} plain description={t("flows.hint")}>
          <If when={showFlows()} else={<Button small variant="outline" icon="chevron-down" label={t("flows.show")} onPress={() => showFlows.set(true)} testID="flows-show" class="self-start" />}>
            <Flows profile={p()} />
          </If>
        </Section>
      </If>
    </Scroll>
  );
}
