// A program over its runs: the same command run again and again, each run's
// requests, waiting, cost and warnings, its baseline, and this run held to it.

import { resource, Scroll, Text, View } from "nsh";
import { t } from "../i18n";
import { api } from "../native";
import { report, sessionsChanged } from "../store";
import type { ProgramHistory } from "../types";
import { Icon } from "./icon";
import { Button, Empty, SimpleTable, terminal } from "./kit";
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
  return (
    <If when={h()} else={<Empty icon="list-clock" text={h.loading() ? t("common.loading") : t("history.none")} />}>
      {(x) => (
        <Scroll testID="history-pane" class="min-h-0 flex-1" contentClass={terminal() ? "gap-2" : "gap-4 pb-4 pr-2"}>
          <View class="flex-row items-center gap-3">
            <View class="min-w-0 flex-1 gap-0.5">
              <Text class="font-mono text-body font-semibold text-foreground line-clamp-1">{t("history.title", { program: x().program, n: x().runs.length })}</Text>
              <Text class="text-small text-muted-foreground">{t("history.hint")}</Text>
            </View>
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
                <RunTrend history={x()} measure="requests" title={t("figure.requests")} format={n} onOpen={props.onOpen} />
                <RunTrend history={x()} measure="busyMs" title={t("profile.waiting")} format={offset} onOpen={props.onOpen} />
                <RunTrend history={x()} measure="cost" title={t("profile.cost")} format={(v) => cost(v, x().runs[0]?.currency ?? "USD")} onOpen={props.onOpen} />
                <RunTrend history={x()} measure="warnings" title={t("obs.warnings")} format={n} onOpen={props.onOpen} />
              </View>
            </If>
          </If>
        </Scroll>
      )}
    </If>
  );
}

export { TOLERANCE };
