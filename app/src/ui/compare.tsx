// Two sessions side by side — a recording and its replay on another
// endpoint, or any two — and the dialog that starts a replay.

import { computed, effect, resource, Scroll, signal, Text, untracked, View } from "nsh";
import { DataTable, DiffView } from "nsh/base";
import { change, count, date, duration, percent, rate, transfer, costParts } from "../format";
import { t } from "../i18n";
import { say } from "../phrases";
import { api } from "../native";
import { endpoint, endpoints, go, reading, report, sessionsChanged } from "../store";
import type { LatencyChange, RunChange, SessionInfo, Signals } from "../types";
import { Icon } from "./icon";
import { Actions, Await, Back, Badge, Button, Card, Check, Choice, Disclosure, Empty, Heading, Modal, Muted, Notice, PageHeader, Row, ROW, rowHeight, Stat, TABLE, terminal, Tiles, Title, TYPE, EMPHASIS, SIZES, SURFACE, COLUMN } from "./kit";
import { fileName, sessionName } from "./manage";
import { brief, FEW, handful, signalsOf } from "../signals";
import { Chart, cost, DistributionCompare, groupName, PrefixChangeMap, RunBars, StoreLatency, MARK, Sparkline } from "./plots";

interface Comparison {
  compared: number;
  same: number;
  environment: number;
  differences: { operation: string; original: string; replay: string; count: number; example: string; conditions: string[] }[];
  unmatched: Record<string, number>;
}

/** Two runs in five tiles, each fact once: requests, errors, the median
 * latency, the waiting, the cost — the figure before → after, by how much it
 * changed, and both runs' lines over their time (before quiet, after in
 * color). Two equal figures are said once, with no change beside them. Fewer
 * is better and more is worse between two runs of one program; between two
 * programs (`!sameProgram`) nothing is better or worse, and every tile is plain. */
function ChangeTiles(props: { change: RunChange; signals?: [Signals, Signals] | null }) {
  const c = () => props.change;
  const s = () => props.signals ?? null;
  const n = (v: number) => count(Math.round(v));
  type Fact = { id: string; label: string; a: number | null; b: number | null; text: (v: number) => string; detail?: () => string; series?: (x: Signals) => number[]; kind?: "bars" | "line"; /** a percentile of fewer than `FEW` requests in that run: marked `~` */ few?: [boolean, boolean] };
  const of = <T,>(f: (x: Signals) => T): [T, T] | null => (s() ? [f(s()![0]), f(s()![1])] : null);
  const pair = (a: string, b: string) => (a === b ? a : `${a} → ${b}`);
  const oneUnit = () => costParts(c().cost[0], c().currency).per1000 === costParts(c().cost[1], c().currency).per1000;
  const facts = (): Fact[] => {
    const pace = of((x) => (brief(x) || x.rate.value == null ? null : t("signal.perSecond", { n: rate(x.rate.value) })));
    const share = of((x) => percent(x.errors.value ?? 0));
    const p50 = of((x) => x.latency.p50Ms);
    // a percentile of a handful of requests is one of them: marked, in each run that had a handful
    const few = of((x) => x.rate.requests < FEW) ?? undefined;
    const mark = (i: 0 | 1, text: string) => (few?.[i] ? `~${text}` : text);
    const p95 = of((x) => (x.latency.p95Ms == null ? null : duration(x.latency.p95Ms)));
    // "p95" once, before the pair: "p95 ~5.7 ms → 69 ms"
    const tails = () => (p95 && p95[0] && p95[1] ? `p95 ${p95[0] === p95[1] && !few?.[0] && !few?.[1] ? p95[0] : `${mark(0, p95[0])} → ${mark(1, p95[1])}`}` : "");
    const moved = of((x) => transfer(x.bytes.upBytes, x.bytes.downBytes));
    return [
      { id: "requests", label: t("compare.requests"), a: c().requests[0], b: c().requests[1], text: n, detail: () => (pace && pace[0] && pace[1] ? pair(pace[0], pace[1]) : ""), series: (x) => x.rate.series },
      { id: "errors", label: t("signal.errors"), a: c().errors[0], b: c().errors[1], text: n, detail: () => (share ? pair(share[0], share[1]) : ""), series: (x) => x.errors.series },
      { id: "latency", label: t("signal.latency"), a: p50?.[0] ?? null, b: p50?.[1] ?? null, text: duration, detail: tails, series: (x) => x.latency.series, kind: "line", few },
      { id: "waiting", label: t("compare.waiting"), a: c().busyMs[0], b: c().busyMs[1], text: duration, detail: () => t("compare.took", { t: pair(duration(c().wallMs[0]), duration(c().wallMs[1])) }) },
      // (both in one unit: the unit is said once, with the tile's name, so the two figures fit)
      { id: "cost", label: oneUnit() ? `${t("compare.cost")} · ${costParts(c().cost[1], c().currency).unit}` : t("compare.cost"), a: c().cost[0], b: c().cost[1], text: (v) => (oneUnit() ? costParts(v, c().currency).figure : cost(v, c().currency)), detail: () => (moved ? pair(moved[0], moved[1]) : "") },
    ];
  };
  const tile = (f: () => Fact) => {
    const known = () => f().a != null && f().b != null;
    const same = () => known() && (Math.abs(f().b! - f().a!) < 1e-9 || f().text(f().a!) === f().text(f().b!));
    const side = (i: 0 | 1, v: number) => (f().few?.[i] ? `~${f().text(v)}` : f().text(v));
    const value = () => (!known() ? t("signal.notTimed") : same() && !f().few?.[0] && !f().few?.[1] ? f().text(f().a!) : `${side(0, f().a!)} → ${side(1, f().b!)}`);
    // how much it changed, then what else there is to say of it; the same in both runs: said so, no "−0%"
    // (from nothing there is no share to say: the two figures say it)
    const detail = () => [!known() ? "" : same() ? t("compare.sameBoth") : f().a === 0 ? "" : change(f().a!, f().b!), f().detail?.() ?? ""].filter(Boolean).join(" · ");
    const tone = () => (!known() || same() || !c().sameProgram ? "plain" : f().b! < f().a! ? "ok" : "bad");
    const lines = () => (f().series && s() && !handful(s()![0]) && !handful(s()![1]) ? [f().series!(s()![0]), f().series!(s()![1])] : null);
    return (
      <Stat
        testID={`compare-${f().id}`}
        label={f().label}
        value={value()}
        detail={detail()}
        tone={tone()}
        mark={
          <If when={lines()}>
            {(l) => (
              <View class="shrink-0 flex-row items-center gap-1" accessibilityLabel="">
                <Sparkline still id={`compare:a:${f().id}`} shape={l()[0]} kind={f().kind} tone="quiet" class={MARK.pair} />
                <Sparkline still id={`compare:b:${f().id}`} shape={l()[1]} kind={f().kind} class={MARK.pair} />
              </View>
            )}
          </If>
        }
      />
    );
  };
  return <Tiles min={SIZES.compareTile} testID="compare-figures" items={facts().map((_, i) => tile(() => facts()[i]))} />;
}

/** Two runs of a program, before and after a change: the figures, by kind of request, and the findings gone and new. */
export function RunChangeView(props: { change: RunChange; inline?: boolean; /** both runs' signals, when they are read: the tiles' lines and second figures */ signals?: [Signals, Signals] | null }) {
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
      <If when={!terminal()} else={<Text testID="compare-figures" class="text-foreground">{[`${t("compare.requests")} ${n(c().requests[0])} → ${n(c().requests[1])}`, `${t("signal.errors")} ${n(c().errors[0])} → ${n(c().errors[1])}`, `${t("compare.waiting")} ${duration(c().busyMs[0])} → ${duration(c().busyMs[1])}`].join(" · ")}</Text>}>
        <ChangeTiles change={c()} signals={props.signals} />
      </If>
      <If
        when={!unchanged()}
        else={
          <View testID="run-unchanged">
            <Empty icon="circle-check" text={t("compare.same")} />
          </View>
        }
      >
        <Chart title={t("compare.byKind")} text={t("compare.byKindHint")}>
          <RunBars change={c()} neutral={!c().sameProgram} />
        </Chart>
        {/* where it changed: each prefix by the later run's requests, colored by how they moved */}
        <If when={(c().prefixes?.length ?? 0) > 1}>
          <Chart title={t("prefixChange.title")} text={t("prefixChange.hint")}>
            <PrefixChangeMap prefixes={c().prefixes!} neutral={!c().sameProgram} />
          </Chart>
        </If>
        {/* both runs' latency: where the curves part says for which requests it changed */}
        <If when={c().latency && c().latency![0].length > 1 && c().latency![1].length > 1}>
          <Chart title={t("cdf.title")} text={t("cdf.hint")} legend={[{ label: t("profile.before"), swatch: "bg-muted-foreground/70" }, { label: t("profile.after"), swatch: "bg-chart-1" }]}>
            <DistributionCompare latency={c().latency!} names={[t("profile.before"), t("profile.after")]} />
          </Chart>
        </If>
        <If when={findings() > 0}>
          <View class="gap-2">
            <Heading>{t("obs.title")}</Heading>
            <Card pad="none">
              <Each of={[...c().resolved.map((x) => ({ x, gone: true })), ...c().appeared.map((x) => ({ x, gone: false }))]}>
                {(f, i) => (
                  <Row top first={i() === 0}>
                    <View class="min-h-5 shrink-0 justify-center">
                      <Icon name={f().gone ? "circle-check" : "triangle-alert"} tint={f().gone ? "status-ok" : "warn"} />
                    </View>
                    <Text class="min-w-0 flex-1 text-body leading-5 text-foreground">
                      {/* emphasis: whether the finding is gone or new, before its words */}
                      <Text class={`${EMPHASIS} ${f().gone ? "text-status-ok" : "text-warn"}`}>{`${f().gone ? t("profile.resolved") : t("profile.appeared")} · `}</Text>
                      {say(f().x.phrases, f().x.text)}
                    </Text>
                  </Row>
                )}
              </Each>
            </Card>
          </View>
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
  // both are being read: neither is tidied away meanwhile
  effect(() => (reading(props.a), reading(props.b)));
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
  // both runs by their four signals, as each session's header has them
  const signals = resource(
    () => `${props.a}\n${props.b}`,
    async () => {
      try {
        const [a, b] = await Promise.all([api.sessions.view(props.a), api.sessions.view(props.b)]);
        const [x, y] = [signalsOf(a), signalsOf(b)];
        return x && y ? ([x, y] as [Signals, Signals]) : null;
      } catch {
        // the figures below still say what changed
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
  // one view: a replay's answers first, then before and after; the transcripts, request by request, on demand
  const answersPart = (d: Answers) => (
    <View class="gap-4">
      <Text class="text-body text-foreground" testID="compare-summary">
        {t("compare.summary", { n: d.comparison.compared, same: d.comparison.same })}
        {d.comparison.differences.length ? ` · ${t("compare.differ", { n: d.comparison.differences.reduce((s, x) => s + x.count, 0) })}` : ""}
      </Text>
      <If when={d.latency?.length}>
        <Chart title={t("latency.title")} text={t("latency.hint")}>
          <StoreLatency changes={d.latency!} names={d.endpoints ?? ["A", "B"]} />
        </Chart>
      </If>
      <If when={d.comparison.differences.length > 0}>
        <Card pad="none" testID="compare-differences">
          <Each of={d.comparison.differences}>
            {(x, i) => (
              <Row wrap first={i() === 0}>
                <View class="shrink-0">
                  <Badge label={t("compare.times", { n: x().count })} tone="fail" />
                </View>
                <Text class="text-body text-foreground">{x().operation}</Text>
                <Text class="font-mono text-small text-foreground">{`${x().original} → ${x().replay}`}</Text>
                <Muted class="min-w-0 flex-1 text-small line-clamp-1">{[x().example, x().conditions.join(", ")].filter(Boolean).join(" · ")}</Muted>
              </Row>
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
      class={`${SURFACE.diff} rounded-(--radius) border border-border`}
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
        {/* the two runs in five tiles (each fact once), then what changed by kind, by prefix and in latency */}
        <Await loading={runs.loading()} error={runs.error()} retry={() => runs.refetch()}>
          <If when={runs()}>{(r) => <RunChangeView change={r()} inline signals={signals()} />}</If>
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
    { key: "what", title: "", width: COLUMN.matrixWhat, fixed: true },
    { key: "recorded", title: t("matrix.recorded"), width: COLUMN.matrixRecorded, align: "end" as const },
    ...props.runs.map((r, i) => ({ key: `s${i}`, title: r.endpoint, width: COLUMN.matrixRun, align: "end" as const })),
  ];
  const rows = () => [
    { what: t("matrix.differ"), recorded: "—", ...Object.fromEntries(props.runs.map((r, i) => [`s${i}`, t("matrix.of", { n: r.differ, of: r.compared })])) },
    ...kinds().map((k) => ({
      what: `${groupName(k)} · p95`,
      recorded: duration(k.p95Ms[0]),
      ...Object.fromEntries(props.runs.map((r, i) => [`s${i}`, p95(r, k) === undefined ? "" : duration(p95(r, k)!)])),
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
                // emphasis: a figure much slower or much faster than the recording's stands out in its tone
                const tone = r >= 1.5 ? `text-status-server ${EMPHASIS}` : r <= 0.67 ? `text-status-ok ${EMPHASIS}` : c.key === "what" ? "text-foreground" : "text-muted-foreground";
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
    <Modal open={props.open} onClose={props.onClose} testID="replay-dialog" describe={t("replay.hint")}>
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
          <Button variant="ghost" label={t("common.cancel")} onPress={props.onClose} testID="replay-cancel" />
          <Button variant="primary" label={busy() ? t("replay.running") : t("replay.run")} onPress={run} disabled={busy() || !plan()} testID="replay-run" />
        </Actions>
      </If>
    </Modal>
  );
}
