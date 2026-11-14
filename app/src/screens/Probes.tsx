// "Check compatibility…" on an endpoint (Settings › Endpoints): every probe
// sonde has, run against that endpoint in a bucket the user names (the one
// chosen in Buckets, filled in), then the matrix of what each store answered
// — the latest check of every endpoint, a column each — whose cells open on
// the raw requests and responses. A check takes minutes: it outlives the
// dialog (its job is in Activity, where it stops), and the dialog, opened
// again, shows how far it got.

import { effect, signal, Text, untracked, View } from "nsh";
import { DataTable } from "nsh/base";
import { duration } from "../format";
import { t } from "../i18n";
import { core, endpointName } from "../phrases";
import { SYMBOL, verdictLabel, verdictTone } from "../labels";
import { api, watch } from "../native";
import { notify, report } from "../store";
import type { EndpointView, Matrix, ProbeMeta, ProbeResult } from "../types";
import { titleCells, Actions, Back, Badge, Button, Card, Disclosure, Empty, Field, Heading, KeyValue, Modal, ModalBody, Mono, More, Muted, ProgressBar, ROW, rowHeight, SwitchRow, TABLE, terminal, Title, TYPE, Pressable, SIZES, COLUMN, TEXT } from "../ui/kit";
import { Document } from "../ui/record";
import { StatusMark } from "../ui/sessionview";

function CellDetail(props: { probe: ProbeMeta; result: ProbeResult; endpointName: string; onBack: () => void }) {
  return (
    <View class="gap-4" testID="probe-detail">
      <View class="flex-row items-center gap-2">
        <Back onPress={props.onBack} testID="probe-back" />
        <Heading class="min-w-0 flex-1">{`${core(props.probe.titleText, props.probe.title)} — ${props.endpointName}`}</Heading>
        <Badge label={verdictLabel(props.result.verdict)} tone={verdictTone(props.result.verdict)} />
      </View>
      <Muted>{core(props.probe.descriptionText, props.probe.description)}</Muted>
      <Card>
        <Heading>{t("probes.aws")}</Heading>
        <Text class={TEXT.body}>{core(props.probe.awsBehaviorText, props.probe.awsBehavior)}</Text>
        <Heading>{t("probes.observed")}</Heading>
        <Text class={TEXT.body}>{core(props.result.summaryText, props.result.summary)}</Text>
        <Each of={props.result.observations}>{(o, i) => <Mono>{core(props.result.observationsText?.[i()], o())}</Mono>}</Each>
        <Muted>{duration(props.result.durationMs)}</Muted>
      </Card>
      <Heading>{t("probes.exchanges")}</Heading>
      {/* each request as the inspector draws one: its status mark, its headers listed, its error body as a document */}
      <Each of={props.result.exchanges} key={(r) => r.id}>
        {(r, i) => (
          <View selectable class={`gap-2 ${i() > 0 ? "pt-2" : ""}`}>
            <View class="flex-row items-center gap-2">
              <StatusMark status={r().status} />
              <Text class={`min-w-0 flex-1 ${TEXT.body} line-clamp-1`}>{r().kind}</Text>
            </View>
            <Text class={TEXT.codeQuiet}>{`${r().method} ${r().url}`}</Text>
            <If when={r().errorBody}>{(b) => <Document text={b()} mark />}</If>
            <Disclosure title={t("requests.requestHeaders")} count={r().requestHeaders.length}>
              <KeyValue dense mono rows={r().requestHeaders.map((h) => [h.name, h.value])} />
            </Disclosure>
            <Disclosure title={t("requests.responseHeaders")} count={r().responseHeaders.length} open>
              <KeyValue dense mono rows={r().responseHeaders.map((h) => [h.name, h.value])} />
            </Disclosure>
          </View>
        )}
      </Each>
    </View>
  );
}

/** The matrix: a row a probe, a column a store's latest check; a cell opens what it answered. */
function MatrixView(props: { matrix: Matrix; onOpen: (probe: ProbeMeta, col: number) => void }) {
  // the probe's column stays while the stores scroll under it
  const columns = () => [
    { key: "probe", title: t("probes.probe"), width: COLUMN.probe, fixed: true },
    ...props.matrix.columns.map((c, i) => ({ key: `s${i}`, title: c.server ? `${endpointName({ id: c.endpoint, name: c.name })} · ${c.server}` : `${endpointName({ id: c.endpoint, name: c.name })} · ${c.vendor}`, width: COLUMN.probeStore, align: "start" as const })),
  ];
  const data = () =>
    props.matrix.rows.map((r) => ({
      probe: `${core(r.probe.titleText, r.probe.title)}\n${r.probe.id}`,
      ...Object.fromEntries(r.cells.map((c, i) => [`s${i}`, c ? `${SYMBOL[c.verdict]} ${verdictLabel(c.verdict)}` : "—"])),
    }));
  return (
    <DataTable
      testID="matrix"
      accessibilityLabel={t("probes.matrix")}
      columns={columns()}
      renderHeaderCell={terminal() ? undefined : titleCells(columns)}
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
                    <Text class={`line-clamp-1 ${TEXT.small}`}>{core(row.probe.titleText, row.probe.title)}</Text>
                    <Text class={`line-clamp-1 ${TEXT.codeQuiet}`}>{row.probe.id}</Text>
                  </View>
                );
              const i = Number(cell.key.slice(1));
              const c = row.cells[i];
              if (!c) return <Text class={TEXT.note}>—</Text>;
              return (
                // at its cell's start: the press area is the badge's, not the cell's
                <View class="flex-row">
                  <Pressable testID={`cell-${row.probe.id}-${i}`} label={`${row.probe.id} ${props.matrix.columns[i].name} ${verdictLabel(c.verdict)}`} onPress={() => props.onOpen(row.probe, i)}>
                    <Badge label={verdictLabel(c.verdict)} tone={verdictTone(c.verdict)} />
                  </Pressable>
                </View>
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
      style={{ height: SIZES.dialogScroll }}
    />
  );
}

// a check is the app's, not the dialog's: from the press on Run to its job's end a second press starts nothing;
// how far it got, by endpoint; bumped as one ends (the matrix read again)
const running = signal<Record<string, string>>({});
const ran = signal(0);

/** "Check compatibility…" of `endpoint`: the bucket its probes write to (`bucket`, the one chosen in Buckets,
 * filled in), whether they may make buckets of their own, then every probe, then the matrix. */
export function CompatDialog(props: { endpoint: EndpointView; bucket: string | null; onClose: () => void }) {
  const scratch = signal(props.bucket ?? "");
  const temp = signal(false);
  const matrix = signal<Matrix>(api.probeMatrix());
  const open = signal<{ probe: ProbeMeta; col: number } | null>(null);
  const busy = () => props.endpoint.id in running();
  // a check that ended while the dialog was closed, or open
  let seen = untracked(ran);
  effect(() => {
    const n = ran();
    if (n !== seen) untracked(() => matrix.set(api.probeMatrix()));
    seen = n;
  });
  const run = async () => {
    const ep = props.endpoint.id;
    if (!scratch() || busy()) return;
    running.update((r) => ({ ...r, [ep]: t("common.loading") }));
    try {
      const [id] = await api.runProbes([ep], { bucket: scratch(), allowTempBuckets: temp(), includeSlow: true, only: [] });
      await watch(id, (e) => e.type === "Progress" && running.update((r) => ({ ...r, [ep]: `${e.progress.done}/${e.progress.total ?? "?"} ${core(e.progress.messageText, e.progress.message)}` })));
    } catch (e) {
      report(e);
    }
    running.update((r) => {
      const { [ep]: _, ...rest } = r;
      return rest;
    });
    ran.update((n) => n + 1);
  };
  const detail = () => {
    const o = open();
    if (!o) return null;
    const col = matrix().columns[o.col];
    const r = api.probeRun(col.run, col.endpoint);
    const res = r?.results.find((x) => x.id === o.probe.id);
    return res ? { probe: o.probe, result: res, name: endpointName({ id: col.endpoint, name: col.name }) } : null;
  };
  const exportAs = (f: "markdown" | "json") => {
    try {
      notify(t("common.exportedTo", { path: api.exportProbes(f) }));
    } catch (e) {
      report(e);
    }
  };
  return (
    <Modal open steady size="wide" onClose={props.onClose} testID="compat-dialog" describe={t("compat.hint")}>
      <View class="flex-row items-center gap-2">
        <Title class="min-w-0 flex-1">{t("compat.title", { name: props.endpoint.name })}</Title>
        <If when={matrix().columns.length > 0}>
          <More
            testID="probe-more"
            actions={() => [
              { label: t("probes.exportMd"), icon: "download", run: () => exportAs("markdown") },
              { label: t("probes.exportJson"), icon: "download", run: () => exportAs("json") },
            ]}
          />
        </If>
      </View>
      <ModalBody testID="screen-probes">
        <If
          when={detail()}
          else={
            <>
              <Muted>{t("compat.hint")}</Muted>
              {/* the bucket its probes write to, and the one choice a check has */}
              <View class="flex-row flex-wrap items-center gap-3">
                <Field bare class="min-w-56 flex-1" label={t("probes.scratch")} placeholder={t("probes.scratch")} value={scratch()} onChange={(v) => scratch.set(v)} onSubmit={run} mono testID="probe-bucket" />
                <SwitchRow label={t("probes.tempBuckets")} checked={temp()} onChange={(v) => temp.set(v)} testID="probe-temp" />
              </View>
              {/* what a check does to the bucket, before it runs */}
              <Text testID="probe-writes" class={TEXT.note}>
                {t("probes.writes", { bucket: scratch() || "…" })}
              </Text>
              <If when={running()[props.endpoint.id]}>{(p) => <ProgressBar value={null} label={p()} />}</If>
              <Heading>{t("probes.results")}</Heading>
              <If when={matrix().columns.length > 0} else={<Empty compact icon="flask-conical" text={t("probes.noRuns")} />}>
                <Card pad="none">
                  <MatrixView matrix={matrix()} onOpen={(probe, col) => open.set({ probe, col })} />
                </Card>
              </If>
            </>
          }
        >
          {(d) => <CellDetail probe={d().probe} result={d().result} endpointName={d().name} onBack={() => open.set(null)} />}
        </If>
      </ModalBody>
      <Actions note={busy() ? <Muted class="text-small">{t("compat.goesOn")}</Muted> : undefined}>
        <Button variant="ghost" label={t("common.close")} onPress={props.onClose} testID="compat-close" />
        <Button variant="primary" icon="play" label={t("compat.run")} onPress={run} disabled={!scratch() || busy()} loading={busy()} testID="probe-run" />
      </Actions>
    </Modal>
  );
}
