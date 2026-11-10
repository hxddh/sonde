// The compatibility matrix: probes run on several endpoints in parallel,
// vendor × probe cells that open on the raw requests and responses, saved
// runs and the difference between two of them.

import { cleanup, Scroll, signal, Text, View } from "nsh";
import { Button as BaseButton, DataTable } from "nsh/base";
import { count, date, ms } from "../format";
import { t } from "../i18n";
import { SYMBOL, verdictLabel, verdictTone } from "../labels";
import { api, watch } from "../native";
import { endpoint, endpoints, go, notify, readOnly, report, showRequest } from "../store";
import type { Change, Matrix, ProbeMeta, ProbeResult, RunSummary } from "../types";
import { Back, Badge, Bar, Button, Card, Check, Choice, Disclosure, Empty, Field, FOCUS, Heading, KeyValue, Mono, More, Muted, Notice, PageHeader, ROW, rowHeight, SimpleTable, SwitchRow, TABLE, terminal, Title, TYPE } from "../ui/kit";
import { Document } from "../ui/record";
import { StatusMark } from "../ui/sessionview";

function CellDetail(props: { probe: ProbeMeta; result: ProbeResult; endpointName: string; onBack: () => void }) {
  return (
    <View class="gap-4" testID="probe-detail">
      <View class="flex-row items-center gap-2">
        <Back onPress={props.onBack} />
        <Title class="min-w-0 flex-1">{`${props.probe.title} — ${props.endpointName}`}</Title>
        <Badge label={verdictLabel(props.result.verdict)} tone={verdictTone(props.result.verdict)} />
      </View>
      <Muted>{props.probe.description}</Muted>
      <Card>
        <Heading>{t("probes.aws")}</Heading>
        <Text class="text-body text-foreground">{props.probe.awsBehavior}</Text>
        <Heading>{t("probes.observed")}</Heading>
        <Text class="text-body font-medium text-foreground">{props.result.summary}</Text>
        <Each of={props.result.observations}>{(o) => <Mono>{o()}</Mono>}</Each>
        <Muted>{ms(props.result.durationMs)}</Muted>
      </Card>
      <Heading>{t("probes.exchanges")}</Heading>
      {/* each request as the inspector draws one: its status mark, its headers boxed, its error body as a document */}
      <Each of={props.result.exchanges} key={(r) => r.id}>
        {(r, i) => (
          <View selectable class={`gap-2 ${i() > 0 ? "border-t border-border pt-4" : ""}`}>
            <View class="flex-row items-center gap-2">
              <StatusMark status={r().status} />
              <Text class="min-w-0 flex-1 text-body font-medium text-foreground line-clamp-1">{r().operation}</Text>
              <Button small variant="ghost" label={t("common.showRequest")} onPress={() => showRequest(r().id)} />
            </View>
            <Text class="font-mono text-caption text-muted-foreground">{`${r().method} ${r().url}`}</Text>
            <If when={r().errorBody}>{(b) => <Document text={b()} mark />}</If>
            <Disclosure title={t("requests.requestHeaders")} count={r().requestHeaders.length}>
              <KeyValue boxed rows={r().requestHeaders.map((h) => [h.name, h.value])} />
            </Disclosure>
            <Disclosure title={t("requests.responseHeaders")} count={r().responseHeaders.length} open>
              <KeyValue boxed rows={r().responseHeaders.map((h) => [h.name, h.value])} />
            </Disclosure>
          </View>
        )}
      </Each>
    </View>
  );
}

/** The matrix: as tall as its rows, up to `maxRows` of them (then it scrolls). */
function MatrixView(props: { matrix: Matrix; maxRows: number; onOpen: (probe: ProbeMeta, col: number) => void }) {
  // a row a probe, a column a store: the probe's column stays while the stores scroll under it
  const columns = () => [
    { key: "probe", title: t("probes.probe"), width: 288, fixed: true },
    ...props.matrix.columns.map((c, i) => ({ key: `s${i}`, title: c.server ? `${c.name} · ${c.server}` : `${c.name} · ${c.vendor}`, width: 150, align: "start" as const })),
  ];
  const data = () =>
    props.matrix.rows.map((r) => ({
      probe: `${r.probe.title}\n${r.probe.id}`,
      ...Object.fromEntries(r.cells.map((c, i) => [`s${i}`, c ? `${SYMBOL[c.verdict]} ${verdictLabel(c.verdict)}` : "—"])),
    }));
  const shown = () => Math.max(1, Math.min(props.matrix.rows.length, props.maxRows));
  return (
    <DataTable
      testID="matrix"
      accessibilityLabel={t("probes.matrix")}
      columns={columns()}
      data={data()}
      fontSize={TYPE.small}
      rowHeight={rowHeight(ROW.twoLine)}
      renderCell={
        terminal()
          ? undefined
          : (cell) => {
              const row = props.matrix.rows[cell.row];
              if (!row) return <Text>{cell.text}</Text>;
              if (cell.key === "probe")
                return (
                  <View class="flex-1 justify-center">
                    <Text class="line-clamp-1 text-small text-foreground">{row.probe.title}</Text>
                    <Text class="line-clamp-1 font-mono text-caption text-muted-foreground">{row.probe.id}</Text>
                  </View>
                );
              const i = Number(cell.key.slice(1));
              const c = row.cells[i];
              if (!c) return <Text class="text-small text-muted-foreground">—</Text>;
              return (
                <BaseButton testID={`cell-${row.probe.id}-${i}`} accessibilityLabel={`${row.probe.id} ${props.matrix.columns[i].name} ${verdictLabel(c.verdict)}`} onPress={() => props.onOpen(row.probe, i)} class={`self-start rounded-(--radius-sm) px-1 py-0.5 hover:bg-muted active:bg-muted ${FOCUS}`}>
                  <Badge label={`${SYMBOL[c.verdict]} ${verdictLabel(c.verdict)}`} tone={verdictTone(c.verdict)} />
                </BaseButton>
              );
            }
      }
      onRowPress={
        terminal()
          ? ({ row }) => {
              // in a terminal a row opens its first result
              const r = props.matrix.rows[row];
              const i = r?.cells.findIndex((c) => c) ?? -1;
              if (r && i >= 0) props.onOpen(r.probe, i);
            }
          : undefined
      }
      classes={terminal() ? undefined : TABLE}
      // its rows and its titles, and no more: nothing empty under a short matrix
      style={terminal() ? { height: TERMINAL_MATRIX } : { height: (shown() + 1) * (rowHeight(ROW.twoLine) ?? ROW.twoLine) }}
    />
  );
}

/** A terminal's matrix: a box of its own to scroll in (its page scrolls too). */
const TERMINAL_MATRIX = 640;

export function Probes() {
  // the endpoint in use to start from; the bucket probes write to is chosen on purpose
  const chosen = signal<Record<string, boolean>>(endpoint() ? { [endpoint()!]: true } : {});
  const scratch = signal("");
  const temp = signal(false);
  const slow = signal(true);
  const only = signal("");
  // from the press on Run to its last job's end: a second press starts nothing
  const busy = signal(false);
  const running = signal<Record<string, string>>({});
  const matrix = signal<Matrix>(api.probeMatrix());
  const runs = signal<RunSummary[]>(api.probeRuns());
  const open = signal<{ probe: ProbeMeta; col: number } | null>(null);
  const earlier = signal<string | null>(null);
  const later = signal<string | null>(null);
  const diff = signal<Change[] | null>(null);
  // the page's own height: the matrix may take it, not more
  const height = signal(720);
  const jobs: string[] = [];
  cleanup(() => {
    for (const j of jobs) api.cancelJob(j);
  });
  const refresh = () => {
    matrix.set(api.probeMatrix());
    runs.set(api.probeRuns());
  };
  // the endpoints checked that are still there
  const picked = () => endpoints().filter((e) => chosen()[e.id]).map((e) => e.id);
  const canRun = () => !readOnly() && picked().length > 0 && scratch() !== "" && !busy();
  const run = async () => {
    if (!canRun()) return;
    const eps = picked();
    busy.set(true);
    try {
      const ids = await api.runProbes(eps, { bucket: scratch(), allowTempBuckets: temp(), includeSlow: slow(), only: only().split(",").map((s) => s.trim()).filter(Boolean) });
      jobs.push(...ids);
      await Promise.all(
        ids.map(async (id, i) => {
          try {
            await watch(id, (e) => e.type === "Progress" && running.update((r) => ({ ...r, [eps[i]]: `${e.progress.done}/${e.progress.total ?? "?"} ${e.progress.message}` })));
          } catch (e) {
            report(e);
          }
          running.update((r) => {
            const { [eps[i]]: _, ...rest } = r;
            return rest;
          });
        }),
      );
      refresh();
    } catch (e) {
      report(e);
    }
    busy.set(false);
  };
  const detail = () => {
    const o = open();
    if (!o) return null;
    const col = matrix().columns[o.col];
    const r = api.probeRun(col.run, col.endpoint);
    const res = r?.results.find((x) => x.id === o.probe.id);
    return res ? { probe: o.probe, result: res, name: col.name } : null;
  };
  const exportAs = (f: "markdown" | "json") => {
    try {
      notify(t("common.exportedTo", { path: api.exportProbes(f) }));
    } catch (e) {
      report(e);
    }
  };
  const compare = () => {
    try {
      diff.set(api.probeDiff(earlier()!, later()!));
    } catch (e) {
      report(e);
    }
  };
  const nameOf = (id: string) => endpoints().find((e) => e.id === id)?.name ?? id;
  // a run as people read one: where, when, and how it went in words
  const runItems = () => runs().map((r) => ({ value: `${r.id}:${r.endpoint}`, title: `${r.endpointName} · ${date(r.started)} · ${t("probes.tally", { pass: count(r.pass), fail: count(r.fail), differs: count(r.differs) })}` }));
  const rowPoints = () => rowHeight(ROW.twoLine) ?? ROW.twoLine;
  return (
    <View class="flex-1" onLayout={({ bounds }) => height.set(Math.round(bounds.size.height))}>
      <If
        when={detail()}
        else={
          <Scroll testID="screen-probes" class="flex-1" contentClass="gap-4 p-5">
            <PageHeader title={t("compat.title")} subtitle={t("compat.hint")}>
              <More
                testID="probe-more"
                actions={() => [
                  { label: t("probes.exportMd"), icon: "download", run: () => exportAs("markdown"), disabled: matrix().columns.length === 0 },
                  { label: t("probes.exportJson"), icon: "download", run: () => exportAs("json"), disabled: matrix().columns.length === 0 },
                ]}
              />
            </PageHeader>
            {/* probes write: said before Run is tried, with the way to allow it */}
            <If when={readOnly()}>
              <Notice tone="warn" text={t("probes.readOnly")} testID="probe-read-only">
                <Button small label={t("settings.open")} onPress={() => go("settings")} />
              </Notice>
            </If>
            <Card>
              <Heading>{t("probes.run")}</Heading>
              <View class="flex-row flex-wrap items-center gap-3">
                {/* which endpoints: said in place when none is chosen (Run stays off until one is) */}
                <Text class="text-small text-muted-foreground">{picked().length > 0 ? t("endpoints.title") : t("probes.chooseEndpoint")}</Text>
                <Each of={endpoints()} key={(e) => e.id}>
                  {(e) => <Check testID={`probe-ep-${e().id}`} label={e().name} checked={!!chosen()[e().id]} onChange={(v) => chosen.update((c) => ({ ...c, [e().id]: v }))} />}
                </Each>
              </View>
              <View class="flex-row flex-wrap items-center gap-3">
                <Field bare class="w-56" label={t("probes.scratch")} placeholder={t("probes.scratch")} value={scratch()} onChange={(v) => scratch.set(v)} onSubmit={run} mono testID="probe-bucket" />
                <Field bare class="w-72" label={t("probes.only")} placeholder={t("probes.onlyHint")} value={only()} onChange={(v) => only.set(v)} onSubmit={run} mono testID="probe-only" />
                <SwitchRow label={t("probes.tempBuckets")} checked={temp()} onChange={(v) => temp.set(v)} />
                <SwitchRow label={t("probes.slow")} checked={slow()} onChange={(v) => slow.set(v)} />
                <Button variant="primary" label={t("common.run")} onPress={run} disabled={!canRun()} testID="probe-run" />
              </View>
              {/* what a run does to the bucket, before it runs */}
              <Text testID="probe-writes" class="text-small text-muted-foreground">
                {t("probes.writes", { bucket: scratch() || "…" })}
              </Text>
              <Each of={Object.entries(running())}>{(r) => <Bar value={null} label={`${nameOf(r()[0])}: ${r()[1]}`} />}</Each>
            </Card>
            <Heading>{t("probes.title")}</Heading>
            <Card pad="none">
              <If when={matrix().columns.length > 0} else={<Empty icon="flask-conical" text={t("probes.noRuns")} />}>
                {/* as tall as its rows, and never taller than the window leaves it */}
                <MatrixView matrix={matrix()} maxRows={Math.max(4, Math.floor((height() - 96) / rowPoints()) - 1)} onOpen={(probe, col) => open.set({ probe, col })} />
              </If>
            </Card>
            <Card>
              <Heading>{t("probes.diff")}</Heading>
              <View class="flex-row flex-wrap items-center gap-2">
                <Text class="text-small text-muted-foreground">{t("probes.earlier")}</Text>
                <Choice class="w-80" value={earlier()} items={runItems()} onChange={(v) => earlier.set(v)} placeholder={t("probes.runs")} testID="probe-earlier" />
                <Text class="text-small text-muted-foreground">{t("probes.later")}</Text>
                <Choice class="w-80" value={later()} items={runItems()} onChange={(v) => later.set(v)} placeholder={t("probes.runs")} testID="probe-later" />
                <Button label={t("probes.compare")} disabled={!earlier() || !later()} onPress={compare} testID="probe-compare" />
              </View>
              <If when={diff()}>
                {(d) => (
                  <If when={d().length > 0} else={<Muted>{t("probes.noChange")}</Muted>}>
                    <SimpleTable
                      columns={[t("probes.probe"), t("probes.earlier"), t("probes.later"), t("probes.now")]}
                      rows={d().map((c) => [c.probe, c.before ? verdictLabel(c.before) : "—", c.after ? verdictLabel(c.after) : "—", `${c.afterSummary ?? ""}${c.stepsChanged ? ` (${t("probes.responsesChanged")})` : ""}`])}
                    />
                  </If>
                )}
              </If>
            </Card>
          </Scroll>
        }
      >
        {(d) => (
          <Scroll class="flex-1" contentClass="p-5">
            <CellDetail probe={d().probe} result={d().result} endpointName={d().name} onBack={() => open.set(null)} />
          </Scroll>
        )}
      </If>
    </View>
  );
}
