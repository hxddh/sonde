// Two runs side by side — a recording and its replay on another
// endpoint, or any two — and the dialog that starts a replay (a run's
// Compare › On another store…): its answers, what the program relies on
// and how that store answers, and each store's request cost.

import { computed, effect, resource, Scroll, signal, Text, untracked, View } from "nsh";
import { DataTable, DiffView } from "nsh/base";
import { bytes, change, clock, cost as money, count, date, day, dayRange, duration, hm, percent, rate, costParts } from "../format";
import { t } from "../i18n";
import { core, say } from "../phrases";
import { verdictLabel, verdictTone } from "../labels";
import { api } from "../native";
import { endpoint, endpoints, go, reading, report, runsChanged } from "../store";
import type { LatencyChange, Profile, Relied, RunInfo } from "../types";
import { Icon } from "./icon";
import { titleCells, DOT, TONE, Actions, Await, Back, Badge, Button, Card, Check, Choice, Disclosure, Empty, Heading, ICON, Modal, Muted, Notice, PageHeader, Row, ROW, rowHeight, TABLE, Title, TYPE, EMPHASIS, SURFACE, COLUMN, WIDTH, TEXT } from "./kit";
import { RunChangeView } from "./runchange";
import { StateBadge } from "./runview";
import { fileName, runName } from "./manage";
import { Chart, cost, groupName, StoreLatency } from "./plots";

interface Comparison {
  compared: number;
  same: number;
  environment: number;
  differences: { kind: string; original: string; replay: string; count: number; example: string; conditions: string[] }[];
  unmatched: Record<string, number>;
}

/** A store's request cost for a run, at its vendor's prices. */
type StoreCost = { endpoint: string; amount: number; currency: string };

type Answers = { comparison: Comparison; a: string; b: string; latency?: LatencyChange[]; endpoints?: [string, string]; relies?: Relied[]; cost?: StoreCost[] };

const storeName = (id: string) => endpoints().find((e) => e.id === id)?.name ?? id;

/** "What etl relies on, and how rfs answers": a row a probe that matches what the run used, with its verdict. */
function Relies(props: { relied: Relied[]; program: string; store: string }) {
  return (
    <View class="gap-2" testID="compare-relies">
      <Heading>{t("relies.title", { program: props.program, store: props.store })}</Heading>
      <Card pad="none">
        <Each of={props.relied}>
          {(r, i) => (
            <Row wrap first={i() === 0}>
              <View class="shrink-0">
                <Badge label={verdictLabel(r().verdict)} tone={verdictTone(r().verdict)} />
              </View>
              <Text class={TEXT.body}>{core(r().titleText, r().title)}</Text>
              <Text class={`font-mono ${TEXT.small}`}>{`${r().kind} ×${count(r().requests)}`}</Text>
              {/* what showed it where the store passed; what it answered where it did not */}
              <Muted class="min-w-0 flex-1 text-small line-clamp-1">{r().verdict === "pass" ? core(r().seenText, r().seen) : core(r().summaryText, r().summary)}</Muted>
            </Row>
          )}
        </Each>
      </Card>
    </View>
  );
}

/** Two runs side by side, the earlier first: whichever was opened, a run is compared with the
 * one before it ("before → after"), never the other way round. */
export function CompareView(props: { a: string; b: string; onClose: () => void }) {
  const created = (p: string) => {
    try {
      return api.runs.list().find((x) => x.path === p)?.created ?? "";
    } catch {
      return "";
    }
  };
  const later = () => {
    const [a, b] = [created(props.a), created(props.b)];
    return !!a && !!b && Date.parse(a) > Date.parse(b);
  };
  return <ComparePair a={later() ? props.b : props.a} b={later() ? props.a : props.b} onClose={props.onClose} />;
}

function ComparePair(props: { a: string; b: string; onClose: () => void }) {
  // the runs there are, read once per pair: their names, and which one is a replay
  const listed = computed((): RunInfo[] => {
    props.a;
    props.b;
    try {
      return api.runs.list();
    } catch {
      return [];
    }
  });
  const info = (p: string) => listed().find((x) => x.path === p);
  // both are being read: neither is tidied away meanwhile
  effect(() => (reading(props.a), reading(props.b)));
  // which one is a replay: its answers are compared with its original's, whichever was opened first
  const replayOf = computed(() => {
    const isReplay = (p: string) => !!info(p)?.command?.startsWith("replay of ");
    return isReplay(props.a) ? "a" : isReplay(props.b) ? "b" : null;
  });
  // the two runs as runs are named everywhere: the program (or the user's title) and when it ran
  const title = () => {
    const [a, b] = [info(props.a), info(props.b)];
    if (!a || !b) return `${a ? runName(a) : fileName(props.a)} → ${b ? runName(b) : fileName(props.b)}`;
    if (runName(a) !== runName(b)) return `${runName(a)} · ${date(a.created)} → ${runName(b)} · ${date(b.created)}`;
    // one program on one day: the day once, and the two times (to the second where they share a minute)
    if (day(a.created) !== day(b.created)) return `${runName(a)} · ${dayRange(a.created, b.created)}`;
    const fine = hm(a.created) === hm(b.created) ? "s" : "m";
    return `${runName(a)} · ${day(a.created)} · ${clock(a.created, fine)} → ${clock(b.created, fine)}`;
  };
  const answers = resource(
    () => `${props.a}\n${props.b}`,
    async () => {
      // the original first: a replay's records name the original's
      const [o, r] = untracked(replayOf) === "a" ? [props.b, props.a] : [props.a, props.b];
      try {
        return (await api.runs.compare(o, r)) as Answers;
      } catch (e) {
        report(e);
        return null;
      }
    },
  );
  // a failure is said in place, with Try again: never "Loading…" for good
  const runs = resource(
    () => `${props.a}\n${props.b}`,
    () => api.runs.compareRuns(props.a, props.b),
  );
  // both runs' profiles (the two over time)
  const views = resource(
    () => `${props.a}\n${props.b}`,
    async () => {
      try {
        const [a, b] = await Promise.all([api.runs.view(props.a), api.runs.view(props.b)]);
        return { profiles: a.profile && b.profile ? ([a.profile, b.profile] as [Profile, Profile]) : null };
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
      const line = lines.findIndex((l) => l.includes(x.kind) && l.includes(x.example));
      return line < 0 ? [] : [{ id: `d${i}`, line, text: t("compare.note", { op: x.kind, a: x.original, b: x.replay, n: x.count }) }];
    });
  };
  // one view: a replay's answers first, then before and after; the transcripts, request by request, on demand
  // the recording's program, by its name everywhere
  const program = () => {
    const o = info(replayOf() === "a" ? props.b : props.a);
    return o ? runName(o) : "";
  };
  const answersPart = (d: Answers) => (
    <View class="gap-4">
      <Text class={TEXT.body} testID="compare-summary">
        {t("compare.summary", { n: d.comparison.compared, same: count(d.comparison.same) })}
        {d.comparison.differences.length ? ` · ${t("compare.differ", { n: d.comparison.differences.reduce((s, x) => s + x.count, 0) })}` : ""}
      </Text>
      {/* each store's request cost, at its own vendor's prices */}
      <If when={d.cost?.length === 2 ? d.cost : null}>
        {(c) => <Text class={TEXT.note} testID="compare-cost">{t("compare.costStores", { a: money(c()[0].amount, c()[0].currency), x: storeName(c()[0].endpoint), b: money(c()[1].amount, c()[1].currency), y: storeName(c()[1].endpoint) })}</Text>}
      </If>
      <If when={d.relies?.length ? d.relies : null}>{(r) => <Relies relied={r()} program={program()} store={storeName(d.endpoints?.[1] ?? "")} />}</If>
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
                <Text class={TEXT.body}>{x().kind}</Text>
                <Text class={`font-mono ${TEXT.small}`}>{`${x().original} → ${x().replay}`}</Text>
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
      renderAnnotation={(a) => <Notice tone="error" icon="circle-exclamation-point" class="mx-2 my-1" text={notes(d).find((x) => x.id === a.id)?.text ?? ""} />}
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
          <If when={runs()}>{(r) => <RunChangeView change={r()} inline profiles={views()?.profiles} />}</If>
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
  plan: { requests: number; writes: number; deletes: number; seeds: number; seedBytes: number; skipped: Record<string, number> };
  prefix: string;
  buckets: string[];
  url: string;
  /** the probes of what the program relies on, asked of the store afterwards */
  relies: number;
}

/** One run replayed on several stores: each store's answers and each kind's p95, beside the recording's. */
interface StoreRun {
  endpoint: string;
  path: string;
  compared: number;
  differ: number;
  latency: LatencyChange[];
  /** its request cost and the recording's, each at its store's prices */
  cost: StoreCost[];
}

function StoreMatrix(props: { runs: StoreRun[]; onOpen: (path: string) => void }) {
  const kinds = () => {
    const seen = new Map<string, LatencyChange>();
    for (const r of props.runs) for (const l of r.latency) seen.set(`${l.kind}\u0000${l.missed}`, l);
    return [...seen.values()];
  };
  const p95 = (r: StoreRun, k: LatencyChange) => r.latency.find((l) => l.kind === k.kind && l.missed === k.missed)?.p95Ms[1];
  const columns = () => [
    { key: "what", title: "", width: COLUMN.matrixWhat, fixed: true },
    { key: "recorded", title: t("matrix.recorded"), width: COLUMN.matrixRecorded, align: "end" as const },
    ...props.runs.map((r, i) => ({ key: `s${i}`, title: r.endpoint, width: COLUMN.matrixRun, align: "end" as const })),
  ];
  const rows = () => [
    { what: t("matrix.differ"), recorded: "—", ...Object.fromEntries(props.runs.map((r, i) => [`s${i}`, t("matrix.of", { n: r.differ, of: r.compared })])) },
    {
      what: t("matrix.cost"),
      recorded: props.runs[0]?.cost[0] ? money(props.runs[0].cost[0].amount, props.runs[0].cost[0].currency) : "—",
      ...Object.fromEntries(props.runs.map((r, i) => [`s${i}`, r.cost[1] ? money(r.cost[1].amount, r.cost[1].currency) : "—"])),
    },
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
        renderHeaderCell={titleCells(columns)}
        data={rows()}
        // the stores under one heading
        groups={[[{ label: "", span: 2 }, { label: t("matrix.stores"), span: props.runs.length }]]}
        rowHeight={rowHeight(ROW.table)}
        fontSize={TYPE.body}
        // a store's p95 against the recording's: much slower in red, much faster in green
        renderCell={
          (c) => {
                const k = c.row > 1 && c.key.startsWith("s") ? kinds()[c.row - 2] : null;
                const v = k ? p95(props.runs[Number(c.key.slice(1))], k) : undefined;
                const r = k && v !== undefined && k.p95Ms[0] > 0 ? v / k.p95Ms[0] : 1;
                // a figure much slower or much faster than the recording's: read at full colour, its dot in its tone
                const mark = r >= 1.5 ? TONE.fail.dot : r <= 0.67 ? TONE.ok.dot : null;
                if (c.key === "what") return <Text class={`flex-1 px-2 ${TEXT.body}`}>{c.text}</Text>;
                return (
                  <View class="flex-1 flex-row items-center justify-end gap-1 px-2">
                    <If when={mark}>{(m) => <View class={`${DOT} ${m()}`} />}</If>
                    <Text class={`tabular-nums text-body ${mark ? "text-foreground" : "text-muted-foreground"}`}>{c.text}</Text>
                  </View>
                );
              }
        }
        classes={{ ...TABLE, cell: "px-0" }}
        // as high as its rows: the header and each row, no more
        style={{ height: (rows().length + 2) * (rowHeight(ROW.table) ?? ROW.table) + 2 }}
      />
      </Card>
      <View class="flex-row flex-wrap gap-2">
        <Each of={props.runs}>{(r) => <Button small variant="outline" icon="git-compare-arrows" label={t("matrix.open", { name: r().endpoint })} onPress={() => props.onOpen(r().path)} />}</Each>
      </View>
    </View>
  );
}

/** Replays a run on an endpoint the user picks; `onDone` gets the replay's run. */
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
  // the plan reads the run once per store chosen, not once per redraw
  const plan = computed((): ReplayPlan | null => {
    const ep = target();
    if (!ep || !props.open) return null;
    try {
      return untracked(() => api.runs.replayPlan(props.path, ep) as ReplayPlan);
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
        const r = (await api.runs.replay(props.path, ep, p.prefix)) as { path: string };
        runsChanged();
        props.onClose();
        props.onDone(r.path);
        return;
      }
      // each store's replay on its own: one that fails leaves the others' results
      const settled = await Promise.allSettled(
        stores.map(async (id) => {
          const r = (await api.runs.replay(props.path, id, p.prefix)) as { path: string; comparison: { compared: number; differences: { count: number }[] }; cost: StoreCost[] };
          const c = (await api.runs.compare(props.path, r.path)) as { latency?: LatencyChange[] };
          return { endpoint: storeName(id), path: r.path, compared: r.comparison.compared, differ: r.comparison.differences.reduce((s, d) => s + d.count, 0), latency: c.latency ?? [], cost: r.cost ?? [] };
        }),
      );
      for (const x of settled) if (x.status === "rejected") report(x.reason);
      const done = settled.flatMap((x) => (x.status === "fulfilled" ? [x.value] : []));
      runs.set(done);
      // the replays are runs: the list shows them
      runsChanged();
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
            <Text class={TEXT.body}>{t("replay.plan", { n: p().plan.requests, writes: t("replay.writes", { n: p().plan.writes }), deletes: t("replay.deletes", { n: p().plan.deletes }), prefix: p().prefix, buckets: p().buckets.join(", ") })}</Text>
            <If when={p().plan.seeds > 0}>
              <Muted class="text-small">{t("replay.seeds", { n: p().plan.seeds, size: bytes(p().plan.seedBytes) })}</Muted>
            </If>
            <If when={p().relies > 0 && p().buckets.length > 0}>
              <Muted class="text-small">{t("replay.relies", { n: p().relies, bucket: p().buckets[0] })}</Muted>
            </If>
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
            <Button variant="primary" label={t("common.close")} onPress={props.onClose} testID="replay-close" />
          </Actions>
        }
      >
        <Actions>
          <Button variant="ghost" label={t("common.cancel")} onPress={props.onClose} testID="replay-cancel" />
          <Button variant="primary" label={t("replay.run")} onPress={run} loading={busy()} disabled={!plan()} testID="replay-run" />
        </Actions>
      </If>
    </Modal>
  );
}
