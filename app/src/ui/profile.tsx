// A session's time: what the program waited on, by kind of request, and how
// long its requests took. The figures say it in words; the charts show it.

import { type JSX, Scroll, Text, View } from "nsh";
import { t } from "../i18n";
import type { SessionView } from "../types";
import { terminal } from "./kit";
import { cost, Latencies, offset, shapeSentence, TimeSpent } from "./plots";

function Figure(props: { value: string; label: string; testID?: string }) {
  return (
    <View testID={props.testID} class="gap-0.5">
      <Text class="tabular-nums text-heading font-semibold text-foreground">{props.value}</Text>
      <Text class="text-caption text-muted-foreground">{props.label}</Text>
    </View>
  );
}

function Section(props: { title: string; hint: string; children: JSX.Element }) {
  return (
    <View class="gap-3">
      <View class="gap-0.5">
        <Text role="header" class="text-body font-semibold text-foreground">{props.title}</Text>
        <Text class="text-small text-muted-foreground">{props.hint}</Text>
      </View>
      {props.children}
    </View>
  );
}

/** The Time pane of a session. */
export function TimePane(props: { view: SessionView; onRequest?: (id: number) => void }) {
  const p = () => props.view.profile;
  return (
    <Scroll testID="time-pane" class="min-h-0 flex-1" contentClass={terminal() ? "gap-2" : "gap-7 pb-4 pr-2"}>
      <If when={!terminal()} else={<Text class="text-foreground">{shapeSentence(p())}</Text>}>
        <View class="flex-row gap-8 rounded-(--radius-card) border border-border bg-card px-4 py-3">
          <Figure testID="profile-waiting" value={offset(p().busyMs)} label={`${t("profile.waiting")} · ${Math.round((p().busyMs * 100) / Math.max(p().wallMs, 1e-9))}%`} />
          <Figure value={String(p().peakInFlight)} label={t("profile.peak")} />
          <Figure testID="profile-cost" value={cost(p().cost.amount, p().cost.currency)} label={t("profile.cost")} />
        </View>
      </If>
      <Section title={t("profile.spentTitle")} hint={t("profile.spentHint")}>
        <TimeSpent profile={p()} />
      </Section>
      <If when={!terminal()}>
        <Section title={t("profile.latencyTitle")} hint={t("profile.latencyHint")}>
          <Latencies profile={p()} onRequest={props.onRequest} />
        </Section>
      </If>
    </Scroll>
  );
}
