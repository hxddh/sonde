// A session's time: what the program waited on, by kind of request, and how
// long its requests took. The figures say it in words; the charts show it.

import { type JSX, Scroll, Text, View } from "nsh";
import { t } from "../i18n";
import type { SessionView } from "../types";
import { Section, terminal } from "./kit";
import { Figure } from "./sessionview";
import { Chains, cost, Flows, Heat, LatencyBand, offset, shapeSentence, Spread, TimeSpent } from "./plots";

/** The Time pane of a session. */
export function TimePane(props: { view: SessionView; onRequest?: (id: number) => void }) {
  const p = () => props.view.profile;
  return (
    <Scroll testID="time-pane" class="min-h-0 flex-1" contentClass={terminal() ? "gap-2" : "gap-7 pb-4 pr-2"}>
      <If when={!terminal()} else={<Text class="text-foreground">{shapeSentence(p())}</Text>}>
        <View class="flex-row gap-3 rounded-(--radius-card) border border-border bg-card px-4 py-3">
          <Figure testID="profile-waiting" value={offset(p().busyMs)} label={`${t("profile.waiting")} · ${Math.round((p().busyMs * 100) / Math.max(p().wallMs, 1e-9))}%`} />
          <Figure value={String(p().peakInFlight)} label={t("profile.peak")} />
          <Figure testID="profile-cost" value={cost(p().cost.amount, p().cost.currency)} label={t("profile.cost")} />
        </View>
      </If>
      <Section title={t("profile.spentTitle")} plain description={t("profile.spentHint")}>
        <TimeSpent profile={p()} />
      </Section>
      <If when={p().chains?.length}>
        <Section title={t("chains.title")} plain description={t("chains.hint")}>
          <Chains profile={p()} />
        </Section>
      </If>
      <If when={p().flows?.length}>
        <Section title={t("flows.title")} plain description={t("flows.hint")}>
          <Flows profile={p()} />
        </Section>
      </If>
      <If when={!terminal() && p().heat?.length}>
        <Section title={t("heat.title")} plain description={t("heat.hint")}>
          <Heat profile={p()} />
        </Section>
      </If>
      <If when={!terminal() && p().rate.filter((b) => b.ok + b.miss + b.client + b.server > 0).length > 2}>
        <Section title={t("band.title")} plain description={t("live.latency")}>
          <LatencyBand profile={p()} class="h-32 w-full" testID="time-band" />
        </Section>
      </If>
      <Section title={t("spread.title")} plain description={t("spread.hint")}>
        <Spread profile={p()} />
      </Section>
    </Scroll>
  );
}
