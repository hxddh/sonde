// Two sessions side by side — a recording and its replay on another
// endpoint, or any two — and the dialog that starts a replay.

import { resource, Scroll, signal, Text, untracked, View } from "nsh";
import { Dialog, DiffView } from "nsh/base";
import { t } from "../i18n";
import { api } from "../native";
import { endpoint, endpoints, go, report } from "../store";
import type { RunChange } from "../types";
import { Icon } from "./icon";
import { Badge, Button, Choice, Heading, Muted, SCRIM, Segmented } from "./kit";
import { cost, offset, RunBars } from "./plots";

interface Comparison {
  compared: number;
  same: number;
  environment: number;
  differences: { operation: string; original: string; replay: string; count: number; example: string; conditions: string[] }[];
  unmatched: Record<string, number>;
}

/** A change in a figure, before → after, and how much. */
function Delta(props: { label: string; a: number; b: number; text: (v: number) => string; testID?: string }) {
  const d = () => props.b - props.a;
  const pctText = () => (props.a > 0 ? ` ${d() > 0 ? "+" : ""}${Math.round((d() * 100) / props.a)}%` : "");
  const tone = () => (Math.abs(d()) < 1e-9 ? "text-muted-foreground" : d() < 0 ? "text-status-ok" : "text-status-server");
  return (
    <View testID={props.testID} class="gap-0.5">
      <Text class="tabular-nums text-heading font-semibold text-foreground">{`${props.text(props.a)} → ${props.text(props.b)}`}</Text>
      <Text class="text-caption text-muted-foreground">
        {props.label}
        <Text class={`tabular-nums font-medium ${tone()}`}>{pctText()}</Text>
      </Text>
    </View>
  );
}

/** Two runs of a program, before and after a change: the figures, by kind of request, and the findings gone and new. */
export function RunChangeView(props: { change: RunChange }) {
  const c = () => props.change;
  const n = (v: number) => String(Math.round(v));
  return (
    <Scroll testID="run-change" class="min-h-0 flex-1" contentClass="gap-5 pb-4">
      <View class="flex-row flex-wrap gap-x-8 gap-y-3 rounded-(--radius-card) border border-border bg-card px-4 py-3">
        <Delta testID="run-change-requests" label={t("figure.requests")} a={c().requests[0]} b={c().requests[1]} text={n} />
        <Delta label={t("profile.waiting")} a={c().busyMs[0]} b={c().busyMs[1]} text={offset} />
        <Delta label={t("figure.took")} a={c().wallMs[0]} b={c().wallMs[1]} text={offset} />
        <Delta label={t("figure.errors")} a={c().errors[0]} b={c().errors[1]} text={n} />
        <Delta label={t("profile.cost")} a={c().cost[0]} b={c().cost[1]} text={(v) => cost(v, c().currency)} />
      </View>
      <RunBars change={c()} />
      <If when={c().resolved.length + c().appeared.length > 0}>
        <View class="gap-1.5">
          <Each of={c().resolved}>
            {(x) => (
              <View class="flex-row items-start gap-2">
                <Icon name="circle-check" size={14} tint="status-ok" />
                <Text class="flex-1 text-body text-foreground">
                  <Text class="font-medium text-status-ok">{`${t("profile.resolved")} · `}</Text>
                  {x()}
                </Text>
              </View>
            )}
          </Each>
          <Each of={c().appeared}>
            {(x) => (
              <View class="flex-row items-start gap-2">
                <Icon name="triangle-alert" size={14} tint="warn" />
                <Text class="flex-1 text-body text-foreground">
                  <Text class="font-medium text-warn">{`${t("profile.appeared")} · `}</Text>
                  {x()}
                </Text>
              </View>
            )}
          </Each>
        </View>
      </If>
    </Scroll>
  );
}

export function CompareView(props: { a: string; b: string; onClose: () => void }) {
  const data = () => {
    try {
      return api.sessions.compare(props.a, props.b) as { comparison: Comparison; a: string; b: string };
    } catch (e) {
      report(e);
      return null;
    }
  };
  const runs = resource(
    () => `${props.a}\n${props.b}`,
    async () => {
      try {
        return await api.sessions.compareRuns(props.a, props.b);
      } catch (e) {
        report(e);
        return null;
      }
    },
  );
  // a replay's answers match the original's; two runs of a program compare as before and after
  const replay = () => {
    try {
      return api.sessions.list().some((x) => (x.path === props.a || x.path === props.b) && x.command?.startsWith("replay of "));
    } catch {
      return false;
    }
  };
  const mode = signal<"runs" | "answers">(untracked(() => (replay() ? "answers" : "runs")));
  const name = (p: string) => p.split("/").pop() ?? p;
  return (
    <If when={data()}>
      {(d) => (
        <View class="flex-1 gap-3" testID="compare">
          <View class="flex-row items-center gap-2">
            <Button small variant="ghost" label={`‹ ${t("common.back")}`} onPress={props.onClose} testID="compare-back" />
            <Text class="flex-1 text-body font-semibold text-foreground line-clamp-1">{`${name(props.a)} → ${name(props.b)}`}</Text>
            <Segmented
              testID="compare-mode"
              value={mode()}
              items={[
                { value: "runs", title: t("profile.runsTitle") },
                { value: "answers", title: t("compare.transcripts") },
              ]}
              onChange={(v) => mode.set(v as "runs" | "answers")}
            />
          </View>
          <If
            when={mode() === "answers"}
            else={
              <If when={runs()} else={<Text class="text-small text-muted-foreground">{t("common.loading")}</Text>}>
                {(r) => <RunChangeView change={r()} />}
              </If>
            }
          >
            <View class="min-h-0 flex-1 gap-2">
              <Text class="text-body text-foreground" testID="compare-summary">
                {t("compare.summary", { n: d().comparison.compared, same: d().comparison.same })}
                {d().comparison.differences.length ? ` · ${t("compare.differ", { n: d().comparison.differences.reduce((s, x) => s + x.count, 0) })}` : ""}
              </Text>
              <Each of={d().comparison.differences}>
                {(x) => (
                  <View class="flex-row flex-wrap items-center gap-2 rounded-(--radius) bg-muted px-2 py-1">
                    <Badge label={`×${x().count}`} tone="fail" />
                    <Text class="text-body text-foreground">{`${x().operation}  ${x().original} → ${x().replay}`}</Text>
                    <Muted class="text-small">{`${x().example}${x().conditions.length ? `  (${x().conditions.join(", ")})` : ""}`}</Muted>
                  </View>
                )}
              </Each>
              <DiffView testID="compare-diff" oldValue={d().a} newValue={d().b} layout="split" inline="word" contextLines={2} lineNumbers class="flex-1 rounded-(--radius) border border-border" />
            </View>
          </If>
        </View>
      )}
    </If>
  );
}

interface ReplayPlan {
  plan: { requests: number; writes: number; deletes: number; skipped: Record<string, number> };
  prefix: string;
  buckets: string[];
  url: string;
  readOnly: boolean;
}

/** Replays a session on an endpoint the user picks; `onDone` gets the replay's session. */
export function ReplayDialog(props: { path: string; open: boolean; onClose: () => void; onDone: (replay: string) => void }) {
  const target = signal<string | null>(endpoint());
  const busy = signal(false);
  const plan = (): ReplayPlan | null => {
    const ep = target();
    if (!ep) return null;
    try {
      return api.sessions.replayPlan(props.path, ep) as ReplayPlan;
    } catch (e) {
      report(e);
      return null;
    }
  };
  const run = async () => {
    const p = plan();
    const ep = target();
    if (!p || !ep) return;
    busy.set(true);
    try {
      const r = (await api.sessions.replay(props.path, ep, p.prefix)) as { path: string };
      props.onClose();
      props.onDone(r.path);
    } catch (e) {
      report(e);
    } finally {
      busy.set(false);
    }
  };
  return (
    <Dialog open={props.open} onOpenChange={({ open }) => !open && props.onClose()} backdropClass={SCRIM} class="w-[520px] max-w-[95%] gap-3 self-center rounded-(--radius-card) border border-border bg-popover p-5 shadow-xl" testID="replay-dialog">
      <Text class="text-heading font-semibold text-foreground">{t("replay.title")}</Text>
      <Muted class="text-body">{t("replay.hint")}</Muted>
      <Choice label={t("common.endpoint")} value={target()} placeholder={t("replay.choose")} items={endpoints().map((e) => ({ value: e.id, title: e.name }))} onChange={(v) => target.set(v)} testID="replay-endpoint" />
      <If when={plan()}>
        {(p) => (
          <View class="gap-1" testID="replay-plan">
            <Text class="text-body text-foreground">{t("replay.plan", { n: p().plan.requests, writes: p().plan.writes, deletes: p().plan.deletes, prefix: p().prefix, buckets: p().buckets.join(", ") })}</Text>
            <Muted class="text-small">{t("replay.cleanup", { prefix: p().prefix })}</Muted>
            <Each of={Object.entries(p().plan.skipped)}>{(e) => <Muted class="text-small">{`${e()[1]} × ${e()[0]}`}</Muted>}</Each>
            <If when={p().readOnly}>
              <View class="flex-row items-center gap-2">
                <Text class="flex-1 text-body text-warn">{t("replay.readOnly")}</Text>
                <Button small label={t("nav.settings")} onPress={() => (props.onClose(), go("settings"))} />
              </View>
            </If>
          </View>
        )}
      </If>
      <View class="flex-row justify-end gap-2">
        <Button variant="ghost" label={t("common.cancel")} onPress={props.onClose} />
        <Button label={busy() ? t("replay.running") : t("replay.run")} onPress={run} disabled={busy() || !plan() || plan()!.readOnly} testID="replay-run" />
      </View>
    </Dialog>
  );
}
