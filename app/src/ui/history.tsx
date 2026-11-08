// A program over its runs: the same command run again and again, each run's
// requests, waiting, cost and warnings, its baseline, and this run held to it.

import { resource, Scroll, signal, Text, View } from "nsh";
import { DatePicker } from "nsh/base";
import { day } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { report, sessionsChanged } from "../store";
import type { ProgramHistory } from "../types";
import { Icon } from "./icon";
import { Button, Empty, POPUP, SimpleTable, terminal } from "./kit";
import { cost, offset, RunTrend } from "./plots";

const TOLERANCE = "10%";

function Verdicts(props: { h: ProgramHistory }) {
  const c = () => props.h.check!;
  const failed = () => c().verdicts.filter((v) => !v.ok);
  const value = (what: string, v: number) => (what === "waiting" ? offset(v) : what === "cost" ? cost(v, props.h.runs[0]?.currency ?? "USD") : String(Math.round(v)));
  return (
    <View testID="history-check" class={`flex-row items-start gap-2.5 rounded-(--radius-card) border px-3.5 py-2.5 ${c().passed ? "border-border bg-card" : "border-warn/40 bg-warn/10"}`}>
      <Icon name={c().passed ? "circle-check" : "triangle-alert"} size={16} tint={c().passed ? "status-ok" : "warn"} />
      <View class="flex-1 gap-0.5">
        <Text class="text-body font-medium text-foreground">{c().passed ? t("history.passed") : t("history.failed")}</Text>
        <Each of={failed()}>
          {(v) => <Text class="tabular-nums text-small text-muted-foreground">{`${t(`history.verdict.${v().what}` as never)}: ${value(v().what, v().value)} > ${value(v().what, v().limit)}`}</Text>}
        </Each>
        <Each of={c().newWarnings}>{(w) => <Text class="text-small text-muted-foreground">{`! ${w()}`}</Text>}</Each>
      </View>
    </View>
  );
}

/** The Runs pane of a session: its program over its runs. */
export function HistoryPane(props: { path: string; onOpen?: (path: string) => void }) {
  const h = resource(
    () => props.path,
    async (p) => {
      try {
        return await api.sessions.history(p);
      } catch (e) {
        report(e);
        return null;
      }
    },
  );
  const isBase = () => h()?.baseline === props.path;
  const toggle = () => {
    try {
      api.sessions.setBaseline(props.path, !isBase());
      sessionsChanged();
      h.refetch();
    } catch (e) {
      report(e);
    }
  };
  const n = (v: number) => String(Math.round(v));
  // the runs of a stretch of days: a range, or every run
  const range = signal<[string | null, string | null] | null>(null);
  const within = (created: string) => {
    const r = range();
    const d = created.slice(0, 10);
    return !r || ((!r[0] || d >= r[0]) && (!r[1] || d <= r[1]));
  };
  const shown = (x: ProgramHistory): ProgramHistory => ({ ...x, runs: x.runs.filter((r) => within(r.created)) });
  const daysAgo = (k: number) => new Date(Date.now() - k * 86400000).toISOString().slice(0, 10);
  const rangeText = () => {
    const r = range();
    return r ? `${r[0] ? day(r[0]) : "…"} – ${r[1] ? day(r[1]) : "…"}` : t("history.allRuns");
  };
  return (
    <If when={h()} else={<Empty icon="list-clock" text={h.loading() ? t("common.loading") : t("history.none")} />}>
      {(x) => (
        <Scroll testID="history-pane" class="min-h-0 flex-1" contentClass={terminal() ? "gap-2" : "gap-4 pb-4 pr-2"}>
          <View class="flex-row items-center gap-3">
            <View class="min-w-0 flex-1 gap-0.5">
              <Text class="font-mono text-body font-semibold text-foreground line-clamp-1">{t("history.title", { program: x().program, n: x().runs.length })}</Text>
              <Text class="text-small text-muted-foreground">{t("history.hint")}</Text>
            </View>
            <If when={!terminal()}>
              <DatePicker
                range
                value={range()}
                onChange={({ value }) => range.set(Array.isArray(value) ? value : null)}
                cleanable
                presets={[
                  { label: t("history.last7"), value: [daysAgo(7), daysAgo(0)] },
                  { label: t("history.last30"), value: [daysAgo(30), daysAgo(0)] },
                ]}
                accessibilityLabel={t("history.range")}
                align="end"
                offset={4}
                popupClass={`${POPUP} p-2`}
                contentClass="flex-row gap-2"
                presetsClass="gap-1"
                presetClass="rounded-(--radius-sm) px-2 py-1 text-small text-foreground hover:bg-muted"
                itemClass="size-8 items-center justify-center rounded-(--radius-sm) text-small text-foreground hover:bg-muted selected:bg-primary selected:text-primary-foreground"
                trigger={
                  <View testID="history-range" class="h-7 flex-row items-center gap-1.5 rounded-(--radius) border border-border px-2 hover:bg-muted">
                    <Icon name="calendar-clock" size={13} />
                    <Text class="text-small text-foreground">{rangeText()}</Text>
                  </View>
                }
              />
            </If>
            <Button small variant={isBase() ? "ghost" : "outline"} icon={isBase() ? "x" : "check"} label={isBase() ? t("history.clearBaseline") : t("history.makeBaseline")} onPress={toggle} testID="history-baseline" />
          </View>
          <If when={x().check}>
            <Verdicts h={x()} />
          </If>
          <If when={x().runs.length > 1} else={<Text class="text-small text-muted-foreground">{t("history.none")}</Text>}>
            <If
              when={!terminal()}
              else={<SimpleTable columns={["", t("figure.requests"), t("profile.waiting"), t("profile.cost")]} rows={x().runs.map((r) => [r.created.slice(0, 16), n(r.requests), offset(r.busyMs), cost(r.cost, r.currency)])} />}
            >
              <View class="flex-row flex-wrap gap-3">
                <RunTrend history={shown(x())} measure="requests" title={t("figure.requests")} format={n} onOpen={props.onOpen} />
                <RunTrend history={shown(x())} measure="busyMs" title={t("profile.waiting")} format={offset} onOpen={props.onOpen} />
                <RunTrend history={shown(x())} measure="cost" title={t("profile.cost")} format={(v) => cost(v, x().runs[0]?.currency ?? "USD")} onOpen={props.onOpen} />
                <RunTrend history={shown(x())} measure="warnings" title={t("obs.warnings")} format={n} onOpen={props.onOpen} />
              </View>
            </If>
          </If>
        </Scroll>
      )}
    </If>
  );
}

export { TOLERANCE };
