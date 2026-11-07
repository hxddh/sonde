// Two sessions side by side — a recording and its replay on another
// endpoint, or any two — and the dialog that starts a replay.

import { signal, Text, View } from "nsh";
import { Dialog, DiffView } from "nsh/base";
import { t } from "../i18n";
import { api } from "../native";
import { endpoint, endpoints, go, report } from "../store";
import { Badge, Button, Choice, Heading, Muted } from "./kit";

interface Comparison {
  compared: number;
  same: number;
  environment: number;
  differences: { operation: string; original: string; replay: string; count: number; example: string; conditions: string[] }[];
  unmatched: Record<string, number>;
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
  const name = (p: string) => p.split("/").pop() ?? p;
  return (
    <If when={data()}>
      {(d) => (
        <View class="flex-1 gap-2" testID="compare">
          <View class="flex-row items-center gap-2">
            <Button small variant="ghost" label={`‹ ${t("common.back")}`} onPress={props.onClose} testID="compare-back" />
            <Text class="flex-1 text-sm font-semibold text-foreground line-clamp-1">{`${name(props.a)} ↔ ${name(props.b)}`}</Text>
          </View>
          <Text class="text-sm text-foreground" testID="compare-summary">
            {t("compare.summary", { n: d().comparison.compared, same: d().comparison.same })}
            {d().comparison.differences.length ? ` · ${t("compare.differ", { n: d().comparison.differences.reduce((s, x) => s + x.count, 0) })}` : ""}
          </Text>
          <Each of={d().comparison.differences}>
            {(x) => (
              <View class="flex-row flex-wrap items-center gap-2 rounded-(--radius) bg-muted px-2 py-1">
                <Badge label={`×${x().count}`} tone="fail" />
                <Text class="text-sm text-foreground">{`${x().operation}  ${x().original} → ${x().replay}`}</Text>
                <Muted class="text-xs">{`${x().example}${x().conditions.length ? `  (${x().conditions.join(", ")})` : ""}`}</Muted>
              </View>
            )}
          </Each>
          <Heading>{t("compare.transcripts")}</Heading>
          <DiffView testID="compare-diff" oldValue={d().a} newValue={d().b} layout="split" inline="word" contextLines={2} lineNumbers class="flex-1 rounded-(--radius) border border-border" />
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
    <Dialog open={props.open} onOpenChange={({ open }) => !open && props.onClose()} class="w-[520px] max-w-full gap-3 self-center rounded-(--radius) border border-border bg-popover p-4 shadow-lg" testID="replay-dialog">
      <Text class="text-base font-semibold text-foreground">{t("replay.title")}</Text>
      <Muted class="text-sm">{t("replay.hint")}</Muted>
      <Choice label={t("common.endpoint")} value={target()} placeholder={t("replay.choose")} items={endpoints().map((e) => ({ value: e.id, title: e.name }))} onChange={(v) => target.set(v)} testID="replay-endpoint" />
      <If when={plan()}>
        {(p) => (
          <View class="gap-1" testID="replay-plan">
            <Text class="text-sm text-foreground">{t("replay.plan", { n: p().plan.requests, writes: p().plan.writes, deletes: p().plan.deletes, prefix: p().prefix, buckets: p().buckets.join(", ") })}</Text>
            <Muted class="text-xs">{t("replay.cleanup", { prefix: p().prefix })}</Muted>
            <Each of={Object.entries(p().plan.skipped)}>{(e) => <Muted class="text-xs">{`${e()[1]} × ${e()[0]}`}</Muted>}</Each>
            <If when={p().readOnly}>
              <View class="flex-row items-center gap-2">
                <Text class="flex-1 text-sm text-warn">{t("replay.readOnly")}</Text>
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
