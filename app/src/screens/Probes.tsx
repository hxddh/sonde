// The compatibility matrix: probes run on several endpoints in parallel,
// vendor × probe cells that open on the raw requests and responses, saved
// runs and the difference between two of them.

import { cleanup, Scroll, signal, Text, View } from "nsh";
import { Button as BaseButton, DataTable } from "nsh/base";
import { ms } from "../format";
import { t } from "../i18n";
import { SYMBOL, verdictLabel, verdictTone } from "../labels";
import { api, watch } from "../native";
import { bucket, endpoint, endpoints, notify, report, showRequest } from "../store";
import type { Change, Matrix, ProbeMeta, ProbeResult, RunSummary } from "../types";
import { Badge, Bar, Button, Card, Check, Choice, Empty, Field, Heading, Mono, Muted, Page, rowHeight, SimpleTable, terminal, Title, Toggle, TYPE } from "../ui/kit";

function CellDetail(props: { probe: ProbeMeta; result: ProbeResult; endpointName: string; onBack: () => void }) {
  return (
    <View class="gap-3" testID="probe-detail">
      <View class="flex-row items-center gap-2">
        <Button small variant="ghost" label={`‹ ${t("common.back")}`} onPress={props.onBack} />
        <Text class="flex-1 text-heading font-semibold text-foreground">{`${props.probe.title} — ${props.endpointName}`}</Text>
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
      <Each of={props.result.exchanges} key={(r) => r.id}>
        {(r) => (
          <Card>
            <View class="flex-row items-center gap-2">
              <Badge label={r().status === null ? "—" : String(r().status)} tone={r().status === null || r().status! >= 400 ? "warn" : "pass"} />
              <Text class="flex-1 text-body font-medium text-foreground">{r().operation}</Text>
              <Button small variant="ghost" label={t("common.showRequest")} onPress={() => showRequest(r().id)} />
            </View>
            <Mono>{`${r().method} ${r().url}`}</Mono>
            <SimpleTable mono max={30} columns={["header", "value"]} rows={[...r().requestHeaders.map((h) => [`→ ${h.name}`, h.value]), ...r().responseHeaders.map((h) => [`← ${h.name}`, h.value])]} />
            <If when={r().errorBody}>{(b) => <Mono class="rounded-(--radius) bg-muted p-2">{b()}</Mono>}</If>
          </Card>
        )}
      </Each>
    </View>
  );
}

function MatrixView(props: { matrix: Matrix; onOpen: (probe: ProbeMeta, col: number) => void }) {
  // a row a probe, a column a store: the probe's column stays while the stores scroll under it
  const columns = () => [
    { key: "probe", title: t("probes.probe"), width: 288, fixed: true, sortable: !terminal() },
    ...props.matrix.columns.map((c, i) => ({ key: `s${i}`, title: c.server ? `${c.name} · ${c.server}` : `${c.name} · ${c.vendor}`, width: 150, align: "start" as const })),
  ];
  const data = () =>
    props.matrix.rows.map((r) => ({
      probe: `${r.probe.title}\n${r.probe.id}`,
      ...Object.fromEntries(r.cells.map((c, i) => [`s${i}`, c ? `${SYMBOL[c.verdict]} ${verdictLabel(c.verdict)}` : "—"])),
    }));
  return (
    <If when={props.matrix.columns.length > 0} else={<Empty text={t("probes.noRuns")} />}>
      <DataTable
        testID="matrix"
        accessibilityLabel={t("probes.matrix")}
        columns={columns()}
        data={data()}
        fontSize={TYPE.small}
        rowHeight={rowHeight(44)}
        renderCell={
          terminal()
            ? undefined
            : (cell) => {
                const row = props.matrix.rows[cell.row];
                if (!row) return <Text>{cell.text}</Text>;
                if (cell.key === "probe")
                  return (
                    <View class="flex-1 justify-center px-2">
                      <Text class="line-clamp-1 text-small text-foreground">{row.probe.title}</Text>
                      <Text class="line-clamp-1 font-mono text-caption text-muted-foreground">{row.probe.id}</Text>
                    </View>
                  );
                const i = Number(cell.key.slice(1));
                const c = row.cells[i];
                if (!c) return <Text class="px-2 text-small text-muted-foreground">—</Text>;
                return (
                  <BaseButton testID={`cell-${row.probe.id}-${i}`} accessibilityLabel={`${row.probe.id} ${props.matrix.columns[i].name} ${c.verdict}`} onPress={() => props.onOpen(row.probe, i)} class="mx-1 self-start rounded-sm px-1 py-0.5 hover:bg-muted">
                    <Badge label={`${SYMBOL[c.verdict]} ${verdictLabel(c.verdict)}`} tone={verdictTone(c.verdict)} />
                  </BaseButton>
                );
              }
        }
        onRowPress={terminal() ? ({ row }) => {
          // in a terminal a row opens its first result
          const r = props.matrix.rows[row];
          const i = r?.cells.findIndex((c) => c) ?? -1;
          if (r && i >= 0) props.onOpen(r.probe, i);
        } : undefined}
        classes={terminal() ? { header: "bg-muted", rowSelected: "bg-accent" } : { header: "bg-card border-b border-border", headerCell: "pl-2 pr-1 text-small font-medium text-muted-foreground", cell: "px-0", rowHover: "bg-row-hover", fixedEdge: "bg-border" }}
        class="rounded-(--radius) border border-border"
        style={terminal() ? { flexGrow: 1 } : { height: Math.min(props.matrix.rows.length + 1, 12) * (rowHeight(44) ?? 44) + 2 }}
      />
    </If>
  );
}

export function Probes() {
  // the endpoint in use, and the bucket open, to start from
  const chosen = signal<Record<string, boolean>>(endpoint() ? { [endpoint()!]: true } : {});
  const scratch = signal(bucket() ?? "");
  const temp = signal(false);
  const slow = signal(true);
  const only = signal("");
  const running = signal<Record<string, string>>({});
  const matrix = signal<Matrix>(api.probeMatrix());
  const runs = signal<RunSummary[]>(api.probeRuns());
  const open = signal<{ probe: ProbeMeta; col: number } | null>(null);
  const diffA = signal<string | null>(null);
  const diffB = signal<string | null>(null);
  const diff = signal<Change[] | null>(null);
  const jobs: string[] = [];
  cleanup(() => {
    for (const j of jobs) api.cancelJob(j);
  });
  const refresh = () => {
    matrix.set(api.probeMatrix());
    runs.set(api.probeRuns());
  };
  const run = async () => {
    const eps = Object.keys(chosen()).filter((k) => chosen()[k]);
    if (eps.length === 0 || !scratch()) return;
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
  const runItems = () => runs().map((r) => ({ value: `${r.id}:${r.endpoint}`, title: `${r.endpointName} · ${r.started.slice(0, 19)} · ✓${r.pass} ✗${r.fail} ≠${r.differs}` }));
  return (
    <If
      when={detail()}
      else={
        <Page testID="screen-probes">
          <Card>
            <Title>{t("probes.run")}</Title>
            <View class="flex-row flex-wrap gap-3">
              <Each of={endpoints()} key={(e) => e.id}>
                {(e) => <Check testID={`probe-ep-${e().id}`} label={e().name} checked={!!chosen()[e().id]} onChange={(v) => chosen.update((c) => ({ ...c, [e().id]: v }))} />}
              </Each>
            </View>
            <View class="flex-row flex-wrap items-end gap-3">
              <Field class="w-56" label={t("probes.scratch")} value={scratch()} onChange={(v) => scratch.set(v)} mono testID="probe-bucket" />
              <Field class="w-72" label={t("probes.only")} value={only()} onChange={(v) => only.set(v)} placeholder="checksum., range." mono testID="probe-only" />
              <Toggle label={t("probes.tempBuckets")} checked={temp()} onChange={(v) => temp.set(v)} />
              <Toggle label={t("probes.slow")} checked={slow()} onChange={(v) => slow.set(v)} />
              <Button variant="primary" label={t("common.run")} onPress={run} disabled={!scratch() || Object.keys(running()).length > 0} testID="probe-run" />
            </View>
            <Each of={Object.entries(running())}>{(r) => <Bar value={null} label={`${r()[0]}: ${r()[1]}`} />}</Each>
          </Card>
          <View class="flex-row items-center gap-2">
            <Title class="flex-1">{t("probes.title")}</Title>
            <Button small label={t("probes.exportMd")} onPress={() => exportAs("markdown")} />
            <Button small label={t("probes.exportJson")} onPress={() => exportAs("json")} />
          </View>
          <View class={terminal() ? "h-[640px]" : "h-[520px]"}>
            <MatrixView matrix={matrix()} onOpen={(probe, col) => open.set({ probe, col })} />
          </View>
          <Card>
            <Title>{t("probes.diff")}</Title>
            <View class="flex-row flex-wrap items-end gap-2">
              <Choice class="w-96" label="A" value={diffA()} items={runItems()} onChange={(v) => diffA.set(v)} placeholder={t("probes.runs")} />
              <Choice class="w-96" label="B" value={diffB()} items={runItems()} onChange={(v) => diffB.set(v)} placeholder={t("probes.runs")} />
              <Button
                label={t("probes.diff")}
                disabled={!diffA() || !diffB()}
                onPress={() => {
                  try {
                    diff.set(api.probeDiff(diffA()!, diffB()!));
                  } catch (e) {
                    report(e);
                  }
                }}
              />
            </View>
            <If when={diff()}>
              {(d) => (
                <If when={d().length > 0} else={<Muted>{t("probes.noChange")}</Muted>}>
                  <SimpleTable columns={["probe", "A", "B", "now"]} rows={d().map((c) => [c.probe, c.before ?? "—", c.after ?? "—", `${c.afterSummary ?? ""}${c.stepsChanged ? " (responses changed)" : ""}`])} />
                </If>
              )}
            </If>
          </Card>
        </Page>
      }
    >
      {(d) => (
        <Scroll class="flex-1" contentClass="p-5">
          <CellDetail probe={d().probe} result={d().result} endpointName={d().name} onBack={() => open.set(null)} />
        </Scroll>
      )}
    </If>
  );
}
