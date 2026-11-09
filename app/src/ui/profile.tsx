// A session's time: what the program waited on, by kind of request, and how
// long its requests took. The charts follow the order a question about S3
// runs: when, how long (each request), which kind and when, what pushed
// back, where the time went (by kind, or by the code that asked), and
// whether a prefix ran into its limits. A stretch picked on the timeline
// zooms every chart drawn over time.

import { batch, Scroll, signal, Text, View } from "nsh";
import { t } from "../i18n";
import type { SessionView } from "../types";
import { Button, Section, Segmented, SimpleTable, terminal } from "./kit";
import { findingMarks } from "./sessionview";
import { Chains, cost, Distribution, Flows, Heat, KindsOverTime, offset, PrefixLimits, Pressure, RequestScatter, SessionShape, shapeSentence, TimeSpent } from "./plots";

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
  const scatterBy = signal<"time" | "size">("time");
  const spentBy = signal<"kind" | "code">("kind");
  const pushedBack = () => p().rate.some((b) => (b.throttled ?? 0) > 0 || (b.retried ?? 0) > 0 || b.client + b.server > 0);
  const showFlows = signal(false);
  return (
    <Scroll testID="time-pane" class="min-h-0 flex-1" contentClass={terminal() ? "gap-2" : "gap-6 pb-4 pr-2"}>
      {/* the figures are the session header's; here, a terminal's sentence */}
      <If when={terminal()}>
        <Text class="text-foreground">{shapeSentence(p())}</Text>
      </If>
      {/* when: the whole session, its findings marked; two presses zoom the charts over time */}
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
          <SessionShape profile={p()} annotate axis marks={findingMarks(props.view)} range={zoom()} slice={zoomFrom()} onSlice={pick} class="mt-4 h-32 w-full" testID="time-shape" />
        </Section>
      </If>
      {/* how long: every request, when it ran or how large it was; a press opens it */}
      <If when={!terminal() && p().samples.length > 1}>
        <Section
          title={t("scatter.title")}
          plain
          description={t("scatter.text", { n: p().samples.length, p95: offset(p().p95Ms ?? 0) })}
          action={<Segmented testID="scatter-by" value={scatterBy()} items={[{ value: "time", title: t("scatter.byTime") }, { value: "size", title: t("scatter.bySize") }]} onChange={(v) => scatterBy.set(v as "time" | "size")} />}
        >
          <RequestScatter profile={p()} by={scatterBy()} domainMs={zoomMs()} onRequest={props.onRequest} />
        </Section>
      </If>
      <If when={!terminal() && (p().groupRate?.length ?? 0) > 0}>
        <Section title={t("kinds.title")} plain>
          <KindsOverTime profile={p()} domainMs={zoomMs()} />
        </Section>
      </If>
      <If when={pushedBack()}>
        <Section title={t("pressure.title")} plain description={t("pressure.hint")}>
          <Pressure profile={p()} range={zoom()} />
        </Section>
      </If>
      <If when={p().latency?.length}>
        <Section title={t("dist.title")} plain description={t("dist.hint")}>
          <Distribution profile={p()} />
        </Section>
      </If>
      <Section
        title={t("profile.spentTitle")}
        plain
        description={t("profile.spentHint")}
        action={
          <If when={(p().callers?.length ?? 0) > 0}>
            <Segmented testID="spent-by" value={spentBy()} items={[{ value: "kind", title: t("spent.byKind") }, { value: "code", title: t("spent.byCode") }]} onChange={(v) => spentBy.set(v as "kind" | "code")} />
          </If>
        }
      >
        <TimeSpent profile={p()} by={spentBy()} />
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
      {/* what it cost, by prefix (by kind is in the header's figure and the bars above) */}
      <If when={p().prefixes.some((x) => (x.cost ?? 0) > 0)}>
        <Section title={t("costs.title")} plain description={t("costs.hint")}>
          <SimpleTable columns={[t("costs.byPrefix"), t("figure.requests"), t("profile.cost")]} rows={p().prefixes.filter((x) => (x.cost ?? 0) > 0).map((x) => [x.prefix === "other" ? t("flows.otherPrefixes") : x.prefix, String(x.requests), cost(x.cost ?? 0, p().cost.currency)])} max={8} />
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
