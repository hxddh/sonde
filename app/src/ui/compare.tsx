// Two sessions side by side — a recording and its replay on another
// endpoint, or any two — and the dialog that starts a replay.

import { computed, effect, resource, Scroll, signal, Text, untracked, View } from "nsh";
import { DataTable, DiffView } from "nsh/base";
import { change, count, date, ms } from "../format";
import { t } from "../i18n";
import { say } from "../phrases";
import { api } from "../native";
import { endpoint, endpoints, go, report, sessionsChanged } from "../store";
import type { LatencyChange, RunChange, SessionInfo } from "../types";
import { Icon } from "./icon";
import { Actions, Await, Back, Badge, Button, Card, Check, Choice, Disclosure, Empty, Figure, Modal, Muted, Notice, PageHeader, ROW, rowHeight, Section, TABLE, terminal, Title, TYPE } from "./kit";
import { fileName, sessionName } from "./manage";
import { cost, DistributionCompare, groupName, offset, PrefixChangeMap, RunBars, StoreLatency } from "./plots";

interface Comparison {
  compared: number;
  same: number;
  environment: number;
  differences: { operation: string; original: string; replay: string; count: number; example: string; conditions: string[] }[];
  unmatched: Record<string, number>;
}

/** A figure before → after, and by how much: fewer is better, more is worse. */
function Delta(props: { label: string; a: number; b: number; text: (v: number) => string; testID?: string }) {
  const same = () => Math.abs(props.b - props.a) < 1e-9;
  return <Figure testID={props.testID} value={`${props.text(props.a)} → ${props.text(props.b)}`} label={props.label} delta={same() ? undefined : change(props.a, props.b)} deltaTone={props.b < props.a ? "ok" : "bad"} />;
}

/** Two runs of a program, before and after a change: the figures, by kind of request, and the findings gone and new. */
export function RunChangeView(props: { change: RunChange; inline?: boolean }) {
  const c = () => props.change;
  const n = (v: number) => count(Math.round(v));
  const findings = () => c().resolved.length + c().appeared.length;
  // the same requests of each kind, the same errors, no finding gone or new
  const unchanged = () => c().requests[0] === c().requests[1] && c().errors[0] === c().errors[1] && c().groups.every((g) => g.requests[0] === g.requests[1]) && findings() === 0;
  return (
    <Scroll testID="run-change" class={props.inline ? "" : "min-h-0 flex-1"} contentClass="gap-6 pb-4" scrollable={!props.inline}>
      <If when={!c().sameProgram}>
        <Notice testID="other-program" tone="warn" text={t("compare.otherProgram")} />
      </If>
      <Card>
        <View class="flex-row flex-wrap gap-x-6 gap-y-3">
          <Delta testID="run-change-requests" label={t("figure.requests")} a={c().requests[0]} b={c().requests[1]} text={n} />
          <Delta label={t("profile.waiting")} a={c().busyMs[0]} b={c().busyMs[1]} text={offset} />
          <Delta label={t("figure.took")} a={c().wallMs[0]} b={c().wallMs[1]} text={offset} />
          <Delta label={t("figure.errors")} a={c().errors[0]} b={c().errors[1]} text={n} />
          <Delta label={t("profile.cost")} a={c().cost[0]} b={c().cost[1]} text={(v) => cost(v, c().currency)} />
        </View>
      </Card>
      <If
        when={!unchanged()}
        else={
          <View testID="run-unchanged">
            <Empty icon="circle-check" text={t("compare.same")} />
          </View>
        }
      >
        <Section plain title={t("compare.byKind")} description={t("compare.byKindHint")}>
          <RunBars change={c()} />
        </Section>
        {/* where it changed: each prefix by the later run's requests, colored by how they moved */}
        <If when={(c().prefixes?.length ?? 0) > 1}>
          <Section plain title={t("prefixChange.title")} description={t("prefixChange.hint")}>
            <PrefixChangeMap prefixes={c().prefixes!} />
          </Section>
        </If>
        {/* both runs' latency: where the curves part says for which requests it changed */}
        <If when={c().latency && c().latency![0].length > 1 && c().latency![1].length > 1}>
          <Section plain title={t("cdf.title")} description={t("cdf.hint")}>
            <DistributionCompare latency={c().latency!} names={[t("profile.before"), t("profile.after")]} />
          </Section>
        </If>
        <If when={findings() > 0}>
          <Section plain title={t("obs.title")}>
            <View class="gap-2">
              <Each of={c().resolved}>
                {(x) => (
                  <View class="flex-row items-start gap-2">
                    <View class="h-5 shrink-0 justify-center">
                      <Icon name="circle-check" size={14} tint="status-ok" />
                    </View>
                    <Text class="min-w-0 flex-1 text-body leading-5 text-foreground">
                      <Text class="font-medium text-status-ok">{`${t("profile.resolved")} · `}</Text>
                      {say(x().phrases, x().text)}
                    </Text>
                  </View>
                )}
              </Each>
              <Each of={c().appeared}>
                {(x) => (
                  <View class="flex-row items-start gap-2">
                    <View class="h-5 shrink-0 justify-center">
                      <Icon name="triangle-alert" size={14} tint="warn" />
                    </View>
                    <Text class="min-w-0 flex-1 text-body leading-5 text-foreground">
                      <Text class="font-medium text-warn">{`${t("profile.appeared")} · `}</Text>
                      {say(x().phrases, x().text)}
                    </Text>
                  </View>
                )}
              </Each>
            </View>
          </Section>
        </If>
      </If>
    </Scroll>
  );
}

type Answers = { comparison: Comparison; a: string; b: string; latency?: LatencyChange[]; endpoints?: [string, string] };

export function CompareView(props: { a: string; b: string; onClose: () => void }) {
  // the sessions there are, read once per pair: their names, and which one is a replay
  const sessions = computed((): SessionInfo[] => {
    props.a;
    props.b;
    try {
      return api.sessions.list();
    } catch {
      return [];
    }
  });
  const info = (p: string) => sessions().find((x) => x.path === p);
  // which one is a replay: its answers are compared with its original's, whichever was opened first
  const replayOf = computed(() => {
    const isReplay = (p: string) => !!info(p)?.command?.startsWith("replay of ");
    return isReplay(props.a) ? "a" : isReplay(props.b) ? "b" : null;
  });
  // the two runs as sessions are named everywhere: the program (or the user's title) and when it ran
  const title = () => {
    const [a, b] = [info(props.a), info(props.b)];
    if (!a || !b) return `${a ? sessionName(a) : fileName(props.a)} → ${b ? sessionName(b) : fileName(props.b)}`;
    return sessionName(a) === sessionName(b) ? `${sessionName(a)} · ${date(a.created)} → ${date(b.created)}` : `${sessionName(a)} · ${date(a.created)} → ${sessionName(b)} · ${date(b.created)}`;
  };
  const answers = resource(
    () => `${props.a}\n${props.b}`,
    async () => {
      // the original first: a replay's records name the original's
      const [o, r] = untracked(replayOf) === "a" ? [props.b, props.a] : [props.a, props.b];
      try {
        return (await api.sessions.compare(o, r)) as Answers;
      } catch (e) {
        report(e);
        return null;
      }
    },
  );
  // a failure is said in place, with Try again: never "Loading…" for good
  const runs = resource(
    () => `${props.a}\n${props.b}`,
    () => api.sessions.compareRuns(props.a, props.b),
  );
  // each difference marked where it first shows in the replay's transcript
  const notes = (d: { comparison: Comparison; b: string }) => {
    const lines = d.b.split("\n");
    return d.comparison.differences.flatMap((x, i) => {
      const line = lines.findIndex((l) => l.includes(x.operation) && l.includes(x.example));
      return line < 0 ? [] : [{ id: `d${i}`, line, text: t("compare.note", { op: x.operation, a: x.original, b: x.replay, n: x.count }) }];
    });
  };
  // one view: a replay's answers first, then before and after; the transcripts, request by request, on demand
  const answersPart = (d: Answers) => (
    <View class="gap-4">
      <Text class="text-body text-foreground" testID="compare-summary">
        {t("compare.summary", { n: d.comparison.compared, same: d.comparison.same })}
        {d.comparison.differences.length ? ` · ${t("compare.differ", { n: d.comparison.differences.reduce((s, x) => s + x.count, 0) })}` : ""}
      </Text>
      <If when={d.latency?.length}>
        <Section plain title={t("latency.title")} description={t("latency.hint")}>
          <StoreLatency changes={d.latency!} names={d.endpoints ?? ["A", "B"]} />
        </Section>
      </If>
      <If when={d.comparison.differences.length > 0}>
        <Card pad="none" testID="compare-differences">
          <Each of={d.comparison.differences}>
            {(x, i) => (
              <View class={`flex-row flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 ${i() > 0 ? "border-t border-border" : ""}`}>
                <View class="shrink-0">
                  <Badge label={t("compare.times", { n: x().count })} tone="fail" />
                </View>
                <Text class="text-body font-medium text-foreground">{x().operation}</Text>
                <Text class="font-mono text-small text-foreground">{`${x().original} → ${x().replay}`}</Text>
                <Muted class="min-w-0 flex-1 text-small line-clamp-1">{[x().example, x().conditions.join(", ")].filter(Boolean).join(" · ")}</Muted>
              </View>
            )}
          </Each>
        </Card>
      </If>
    </View>
  );
  const diff = (d: Answers) => (
    <DiffView
      testID="compare-diff"
      oldValue={d.a}
      newValue={d.b}
      layout="split"
      inline="word"
      contextLines={2}
      lineNumbers
      annotations={notes(d).map((n) => ({ id: n.id, side: "new" as const, line: n.line }))}
      renderAnnotation={(a) => <Notice tone="error" icon="circle-alert" class="mx-2 my-1" text={notes(d).find((x) => x.id === a.id)?.text ?? ""} />}
      class="h-[440px] rounded-(--radius) border border-border"
    />
  );
  return (
    <View class="flex-1 gap-3" testID="compare">
      <View class="gap-1">
        <Back onPress={props.onClose} testID="compare-back" />
        <PageHeader title={title()} testID="compare-title" />
      </View>
      <Scroll class="min-h-0 flex-1" contentClass="gap-6 pb-4">
        <If when={replayOf() && answers()}>{(d) => answersPart(d() as Answers)}</If>
        <Await loading={runs.loading()} error={runs.error()} retry={() => runs.refetch()}>
          <If when={runs()}>{(r) => <RunChangeView change={r()} inline />}</If>
        </Await>
        <If when={answers()}>
          {(d) => (
            <Disclosure title={t("compare.transcripts")} testID="compare-transcripts">
              <View class="gap-4 pt-1">
                <If when={!replayOf()}>{answersPart(d())}</If>
                {diff(d())}
              </View>
            </Disclosure>
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
      <Card pad="none">
      <DataTable
        columns={columns()}
        data={rows()}
        // the stores under one heading
        groups={[[{ label: "", span: 2 }, { label: t("matrix.stores"), span: props.runs.length }]]}
        rowHeight={rowHeight(ROW.table)}
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
        // as high as its rows: the header and each row, no more
        style={terminal() ? undefined : { height: (rows().length + 2) * (rowHeight(ROW.table) ?? ROW.table) + 2 }}
      />
      </Card>
      <View class="flex-row flex-wrap gap-2">
        <Each of={props.runs}>{(r) => <Button small variant="outline" icon="git-compare-arrows" label={t("matrix.open", { name: r().endpoint })} onPress={() => props.onOpen(r().path)} />}</Each>
      </View>
    </View>
  );
}

/** Replays a session on an endpoint the user picks; `onDone` gets the replay's session. */
export function ReplayDialog(props: { path: string; open: boolean; onClose: () => void; onDone: (replay: string) => void }) {
  const target = signal<string | null>(endpoint());
  const busy = signal(false);
  // more stores at once: the same requests on each, side by side
  const also = signal<string[]>([]);
  const runs = signal<StoreRun[] | null>(null);
  // opened again, the dialog starts afresh: the endpoint in use now, no
  // other stores chosen, an earlier side-by-side gone
  effect(() => {
    if (props.open)
      untracked(() => {
        target.set(endpoint());
        also.set([]);
        runs.set(null);
      });
  });
  // the plan reads the session once per store chosen, not once per redraw
  const plan = computed((): ReplayPlan | null => {
    const ep = target();
    if (!ep || !props.open) return null;
    try {
      return untracked(() => api.sessions.replayPlan(props.path, ep) as ReplayPlan);
    } catch (e) {
      report(e);
      return null;
    }
  });
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
            <Each of={Object.entries(p().plan.skipped)}>{(e) => <Muted class="text-small">{`${count(e()[1])} × ${e()[0]}`}</Muted>}</Each>
            <If when={p().readOnly}>
              <Notice tone="warn" text={t("replay.readOnly")}>
                <Button small label={t("settings.open")} onPress={() => (props.onClose(), go("settings"))} />
              </Notice>
            </If>
          </View>
        )}
      </If>
      {/* with the stores side by side there is nothing left to run: the dialog only closes */}
      <If
        when={!runs()}
        else={
          <Actions>
            <Button variant="outline" label={t("common.close")} onPress={props.onClose} testID="replay-close" />
          </Actions>
        }
      >
        <Actions>
          <Button variant="ghost" label={t("common.cancel")} onPress={props.onClose} />
          <Button variant="primary" label={busy() ? t("replay.running") : t("replay.run")} onPress={run} disabled={busy() || !plan() || plan()!.readOnly} testID="replay-run" />
        </Actions>
      </If>
    </Modal>
  );
}
