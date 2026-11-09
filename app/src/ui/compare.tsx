// Two sessions side by side — a recording and its replay on another
// endpoint, or any two — and the dialog that starts a replay.

import { resource, Scroll, signal, Text, untracked, View } from "nsh";
import { DataTable, DiffView } from "nsh/base";
import { ms } from "../format";
import { t } from "../i18n";
import { say } from "../phrases";
import { api } from "../native";
import { endpoint, endpoints, go, report, sessionsChanged } from "../store";
import type { LatencyChange, RunChange } from "../types";
import { Icon } from "./icon";
import { Badge, Button, CARD, Check, Choice, Heading, Loading, Modal, Muted, rowHeight, Section, Segmented, TABLE, terminal, Title, TYPE } from "./kit";
import { cost, DistributionCompare, groupName, offset, RunBars, StoreLatency } from "./plots";

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
export function RunChangeView(props: { change: RunChange; inline?: boolean }) {
  const c = () => props.change;
  const n = (v: number) => String(Math.round(v));
  return (
    <Scroll testID="run-change" class={props.inline ? "" : "min-h-0 flex-1"} contentClass="gap-5 pb-4" scrollable={!props.inline}>
      <If when={!c().sameProgram}>
        <View testID="other-program" class="flex-row items-center gap-2 rounded-(--radius) border border-warn/40 bg-warn/10 px-3 py-2">
          <Icon name="triangle-alert" size={14} tint="warn" />
          <Text class="flex-1 text-body text-foreground">{t("compare.otherProgram")}</Text>
        </View>
      </If>
      <View class={`flex-row flex-wrap gap-x-8 gap-y-3 ${CARD} p-4`}>
        <Delta testID="run-change-requests" label={t("figure.requests")} a={c().requests[0]} b={c().requests[1]} text={n} />
        <Delta label={t("profile.waiting")} a={c().busyMs[0]} b={c().busyMs[1]} text={offset} />
        <Delta label={t("figure.took")} a={c().wallMs[0]} b={c().wallMs[1]} text={offset} />
        <Delta label={t("figure.errors")} a={c().errors[0]} b={c().errors[1]} text={n} />
        <Delta label={t("profile.cost")} a={c().cost[0]} b={c().cost[1]} text={(v) => cost(v, c().currency)} />
      </View>
      <RunBars change={c()} />
      {/* both runs' latency: where the curves part says for which requests it changed */}
      <If when={c().latency && c().latency![0].length > 1 && c().latency![1].length > 1}>
        <Section title={t("cdf.title")} plain>
          <DistributionCompare latency={c().latency!} names={[t("profile.before"), t("profile.after")]} />
        </Section>
      </If>
      <If when={c().resolved.length + c().appeared.length > 0}>
        <View class="gap-1.5">
          <Each of={c().resolved}>
            {(x) => (
              <View class="flex-row items-start gap-2">
                <Icon name="circle-check" size={14} tint="status-ok" />
                <Text class="flex-1 text-body text-foreground">
                  <Text class="font-medium text-status-ok">{`${t("profile.resolved")} · `}</Text>
                  {say(x().phrases, x().text)}
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
                  {say(x().phrases, x().text)}
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
  // which one is a replay: its answers are compared with its original's, whichever was opened first
  const replayOf = () => {
    try {
      const list = api.sessions.list();
      const isReplay = (p: string) => list.some((x) => x.path === p && x.command?.startsWith("replay of "));
      return isReplay(props.a) ? "a" : isReplay(props.b) ? "b" : null;
    } catch {
      return null;
    }
  };
  const answers = resource(
    () => `${props.a}\n${props.b}`,
    async () => {
      // the original first: a replay's records name the original's
      const [o, r] = replayOf() === "a" ? [props.b, props.a] : [props.a, props.b];
      try {
        return (await api.sessions.compare(o, r)) as { comparison: Comparison; a: string; b: string; latency?: LatencyChange[]; endpoints?: [string, string] };
      } catch (e) {
        report(e);
        return null;
      }
    },
  );
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
  // each difference marked where it first shows in the replay's transcript
  const notes = (d: { comparison: Comparison; b: string }) => {
    const lines = d.b.split("\n");
    return d.comparison.differences.flatMap((x, i) => {
      const line = lines.findIndex((l) => l.includes(x.operation) && l.includes(x.example));
      return line < 0 ? [] : [{ id: `d${i}`, line, text: t("compare.note", { op: x.operation, a: x.original, b: x.replay, n: x.count }) }];
    });
  };
  const name = (p: string) => p.split(/[\\/]/).pop() || p;
  // one view: a replay's answers first, then before and after; the transcripts, request by request, on demand
  const transcripts = signal(false);
  const answersPart = (d: NonNullable<ReturnType<typeof answers>>) => (
    <View class="gap-2">
      <Text class="text-body text-foreground" testID="compare-summary">
        {t("compare.summary", { n: d.comparison.compared, same: d.comparison.same })}
        {d.comparison.differences.length ? ` · ${t("compare.differ", { n: d.comparison.differences.reduce((s, x) => s + x.count, 0) })}` : ""}
      </Text>
      <If when={d.latency?.length}>
        <View class={`gap-2 ${CARD} p-4`}>
          <Text role="header" class="text-body font-semibold text-foreground">{t("latency.title")}</Text>
          <Text class="text-small text-muted-foreground">{t("latency.hint")}</Text>
          <StoreLatency changes={d.latency!} names={d.endpoints ?? ["A", "B"]} />
        </View>
      </If>
      <Each of={d.comparison.differences}>
        {(x) => (
          <View class="flex-row flex-wrap items-center gap-2 rounded-(--radius) bg-muted px-2 py-1">
            <Badge label={`×${x().count}`} tone="fail" />
            <Text class="text-body text-foreground">{`${x().operation}  ${x().original} → ${x().replay}`}</Text>
            <Muted class="text-small">{`${x().example}${x().conditions.length ? `  (${x().conditions.join(", ")})` : ""}`}</Muted>
          </View>
        )}
      </Each>
    </View>
  );
  const diff = (d: NonNullable<ReturnType<typeof answers>>) => (
    <DiffView
      testID="compare-diff"
      oldValue={d.a}
      newValue={d.b}
      layout="split"
      inline="word"
      contextLines={2}
      lineNumbers
      annotations={notes(d).map((n) => ({ id: n.id, side: "new" as const, line: n.line }))}
      renderAnnotation={(a) => {
        const n = notes(d).find((x) => x.id === a.id);
        return (
          <View class="mx-2 my-1 flex-row items-center gap-2 rounded-(--radius-sm) border border-status-server/40 bg-status-server/10 px-2 py-1">
            <Icon name="circle-alert" size={12} tint="status-server" />
            <Text class="text-small text-foreground">{n?.text ?? ""}</Text>
          </View>
        );
      }}
      class="h-[440px] rounded-(--radius) border border-border"
    />
  );
  return (
    <View class="flex-1 gap-3" testID="compare">
      <View class="flex-row items-center gap-2">
        <Button small variant="ghost" label={`‹ ${t("common.back")}`} onPress={props.onClose} testID="compare-back" />
        <Text class="flex-1 text-body font-semibold text-foreground line-clamp-1">{`${name(props.a)} → ${name(props.b)}`}</Text>
      </View>
      <Scroll class="min-h-0 flex-1" contentClass="gap-5 pb-4">
        <If when={replayOf() && answers()}>{(d) => answersPart(d())}</If>
        <If when={runs()} else={<Loading />}>
          {(r) => <RunChangeView change={r()} inline />}
        </If>
        <If when={answers()}>
          {(d) => (
            <View class="gap-2">
              <Button
                small
                variant="ghost"
                icon={transcripts() ? "chevron-down" : "chevron-right"}
                label={t("compare.transcripts")}
                onPress={() => transcripts.set(!transcripts())}
                testID="compare-transcripts"
                class="self-start"
              />
              <If when={transcripts()}>
                <If when={!replayOf()}>{answersPart(d())}</If>
                {diff(d())}
              </If>
            </View>
          )}
        </If>
      </Scroll>
    </View>
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
/** One session replayed on several stores: each store's answers and each kind's p95, beside the recording's. */
interface StoreRun {
  endpoint: string;
  path: string;
  compared: number;
  differ: number;
  latency: LatencyChange[];
}

function StoreMatrix(props: { runs: StoreRun[]; onOpen: (path: string) => void }) {
  const kinds = () => {
    const seen = new Map<string, LatencyChange>();
    for (const r of props.runs) for (const l of r.latency) seen.set(`${l.operation}\u0000${l.missed}`, l);
    return [...seen.values()];
  };
  const p95 = (r: StoreRun, k: LatencyChange) => r.latency.find((l) => l.operation === k.operation && l.missed === k.missed)?.p95Ms[1];
  const columns = () => [
    { key: "what", title: "", width: 190, fixed: true },
    { key: "recorded", title: t("matrix.recorded"), width: 90, align: "end" as const },
    ...props.runs.map((r, i) => ({ key: `s${i}`, title: r.endpoint, width: 100, align: "end" as const })),
  ];
  const rows = () => [
    { what: t("matrix.differ"), recorded: "—", ...Object.fromEntries(props.runs.map((r, i) => [`s${i}`, t("matrix.of", { n: r.differ, of: r.compared })])) },
    ...kinds().map((k) => ({
      what: `${groupName(k)} · p95`,
      recorded: ms(k.p95Ms[0]),
      ...Object.fromEntries(props.runs.map((r, i) => [`s${i}`, p95(r, k) === undefined ? "" : ms(p95(r, k)!)])),
    })),
  ];
  return (
    <View testID="store-matrix" class="gap-2">
      <DataTable
        columns={columns()}
        data={rows()}
        // the stores under one heading
        groups={[[{ label: "", span: 2 }, { label: t("matrix.stores"), span: props.runs.length }]]}
        rowHeight={rowHeight(28)}
        fontSize={TYPE.body}
        // a store's p95 against the recording's: much slower in red, much faster in green
        renderCell={
          terminal()
            ? undefined
            : (c) => {
                const k = c.row > 0 && c.key.startsWith("s") ? kinds()[c.row - 1] : null;
                const v = k ? p95(props.runs[Number(c.key.slice(1))], k) : undefined;
                const r = k && v !== undefined && k.p95Ms[0] > 0 ? v / k.p95Ms[0] : 1;
                const tone = r >= 1.5 ? "text-status-server font-medium" : r <= 0.67 ? "text-status-ok font-medium" : c.key === "what" ? "text-foreground" : "text-muted-foreground";
                return <Text class={`flex-1 px-2 tabular-nums text-body ${c.key === "what" ? "" : "text-right"} ${tone}`}>{c.text}</Text>;
              }
        }
        classes={terminal() ? undefined : { ...TABLE, cell: "px-0" }}
        class="h-[220px] rounded-(--radius) border border-border bg-card"
      />
      <View class="flex-row flex-wrap gap-2">
        <Each of={props.runs}>{(r) => <Button small variant="outline" icon="git-compare-arrows" label={t("matrix.open", { name: r().endpoint })} onPress={() => props.onOpen(r().path)} />}</Each>
      </View>
    </View>
  );
}

export function ReplayDialog(props: { path: string; open: boolean; onClose: () => void; onDone: (replay: string) => void }) {
  const target = signal<string | null>(endpoint());
  const busy = signal(false);
  // more stores at once: the same requests on each, side by side
  const also = signal<string[]>([]);
  const runs = signal<StoreRun[] | null>(null);
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
      const stores = [ep, ...also().filter((x) => x !== ep)];
      if (stores.length === 1) {
        const r = (await api.sessions.replay(props.path, ep, p.prefix)) as { path: string };
        sessionsChanged();
        props.onClose();
        props.onDone(r.path);
        return;
      }
      // each store's replay on its own: one that fails leaves the others' results
      const settled = await Promise.allSettled(
        stores.map(async (id) => {
          const r = (await api.sessions.replay(props.path, id, p.prefix)) as { path: string; comparison: { compared: number; differences: { count: number }[] } };
          const c = (await api.sessions.compare(props.path, r.path)) as { latency?: LatencyChange[] };
          return { endpoint: endpoints().find((e) => e.id === id)?.name ?? id, path: r.path, compared: r.comparison.compared, differ: r.comparison.differences.reduce((s, d) => s + d.count, 0), latency: c.latency ?? [] };
        }),
      );
      for (const x of settled) if (x.status === "rejected") report(x.reason);
      const done = settled.flatMap((x) => (x.status === "fulfilled" ? [x.value] : []));
      runs.set(done);
      // the replays are sessions: the list shows them
      sessionsChanged();
    } catch (e) {
      report(e);
    } finally {
      busy.set(false);
    }
  };
  return (
    <Modal open={props.open} onClose={props.onClose} testID="replay-dialog">
      <Title>{t("replay.title")}</Title>
      <Muted class="text-body">{t("replay.hint")}</Muted>
      <Choice label={t("common.endpoint")} value={target()} placeholder={t("replay.choose")} items={endpoints().map((e) => ({ value: e.id, title: e.name }))} onChange={(v) => target.set(v)} testID="replay-endpoint" />
      <If when={endpoints().length > 1}>
        <View class="gap-1">
          <Muted class="text-small">{t("replay.also")}</Muted>
          <View class="flex-row flex-wrap gap-x-4 gap-y-1">
            <Each of={endpoints().filter((e) => e.id !== target())}>
              {(e) => <Check label={e().name} checked={also().includes(e().id)} onChange={(on) => also.set(on ? [...also(), e().id] : also().filter((x) => x !== e().id))} testID={`replay-also-${e().id}`} />}
            </Each>
          </View>
        </View>
      </If>
      <If when={runs()}>{(r) => <StoreMatrix runs={r()} onOpen={(path) => (props.onClose(), props.onDone(path))} />}</If>
      <If when={plan()}>
        {(p) => (
          <View class="gap-1" testID="replay-plan">
            <Text class="text-body text-foreground">{t("replay.plan", { n: p().plan.requests, writes: t("replay.writes", { n: p().plan.writes }), deletes: t("replay.deletes", { n: p().plan.deletes }), prefix: p().prefix, buckets: p().buckets.join(", ") })}</Text>
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
    </Modal>
  );
}
