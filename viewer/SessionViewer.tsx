// A shared session: what `sonde share` publishes, read in a browser. The
// app's own views — the figures, what stands out (each a filter), the
// operations (each placed in time), the program's output lined up, and an
// inspector from an operation to its requests to one request in full —
// over the session's precomputed `session.json`.

import { batch, computed, Scroll, signal, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup, type DataTableRef } from "nsh/base";
import { duration } from "../app/src/format";
import { t } from "../app/src/i18n";
import type { Episode, LogLine, RecordSummary, RequestRecord, SessionView } from "../app/src/types";
import { BREAK, Button, Muted, PanelHeader, Segmented, SPLIT_HANDLE, Strong } from "../app/src/ui/kit";
import { SessionShape } from "../app/src/ui/plots";
import { RecordView } from "../app/src/ui/record";
import { findingMarks, linesOf, Observations, OperationsTable, OperationView, OutputTable, operationsAt, SessionHeader } from "../app/src/ui/sessionview";
import { TimePane } from "../app/src/ui/profile";

/** What `sonde share` writes. */
export interface SharedSession extends SessionView {
  format: "sonde-share";
  version: number;
  records: RequestRecord[];
  lines: LogLine[];
}

type Pane = "operations" | "aligned" | "time";

export function SessionViewer(props: { data: SharedSession }) {
  const d = props.data;
  const byId = new Map(d.records.map((r) => [r.id, r]));
  const op = signal<number | null>(null);
  const rec = signal<number | null>(null);
  const opHighlight = signal<number[]>([]);
  const outHighlight = signal<number[]>([]);
  const filter = signal<number | null>(null);
  const pane = signal<Pane>(d.lines.length ? "aligned" : "operations");
  let opsRef: DataTableRef | undefined;
  let outRef: DataTableRef | undefined;
  // the operations in the order their table shows them (sorted by a column)
  let opsOrder: (() => number[]) | undefined;

  const episode = computed<Episode | null>(() => {
    const i = op();
    return i === null ? null : (d.episodes[i] ?? null);
  });
  // a request chosen on the Time pane has no operation open: the inspector shows it all the same
  const record = computed(() => (rec() === null ? null : (byId.get(rec()!) ?? null)));
  const inspecting = () => !!episode() || !!record();
  // this page's own size: what it shows of a finding and whether the shape has room follow it
  const box = signal({ w: 1280, h: 800 });
  /** A slice of the session's shape, picked on the strip: the operations running then. */
  const slice = signal<number | null>(null);
  const window = (): [number, number] | null => {
    const i = slice();
    if (i === null || !d.profile) return null;
    const a = d.profile.rate[i]?.tMs ?? 0;
    return [a, a + d.profile.binMs];
  };
  const start = d.episodes.reduce((m, e) => Math.min(m, new Date(e.start).getTime()), Infinity);
  /** The operations shown: all, those the chosen finding is about, and those running in the chosen slice. */
  const shown = () => {
    const f = filter();
    const w = window();
    if (f === null && !w) return undefined;
    const list = f === null ? d.episodes.map((_, i) => i) : (d.observations[f]?.episodes ?? []);
    return w ? list.filter((i) => new Date(d.episodes[i].start).getTime() - start < w[1] && new Date(d.episodes[i].end).getTime() - start >= w[0]) : list;
  };
  const filterNote = () => {
    const w = window();
    if (filter() === null && !w) return null;
    const n = shown()?.length ?? 0;
    return w ? t("profile.sliceFilter", { a: duration(w[0]), b: duration(w[1]), n, of: d.episodes.length }) : t("sessions.filtered", { n, of: d.episodes.length });
  };
  const rows = (e: Episode): RecordSummary[] => e.records.map((id) => byId.get(id)).filter((r): r is RequestRecord => !!r);
  const chooseOperation = (i: number) => {
    op.set(i);
    rec.set(null);
    opHighlight.set([i]);
    const ls = linesOf(d.lines, d.episodes[i]);
    outHighlight.set(ls);
    if (ls.length) outRef?.scrollToRow(ls[0]);
  };
  const chooseLine = (i: number) => {
    outHighlight.set([i]);
    const ops = operationsAt(d.episodes, d.lines[i]);
    opHighlight.set(ops);
    if (ops.length) {
      op.set(ops[0]);
      rec.set(null);
      // the row as the table shows it: sorted by a column, it is not the operation's index
      const at = (opsOrder?.() ?? shown() ?? d.episodes.map((_, k) => k)).indexOf(ops[0]);
      if (at >= 0) opsRef?.select({ row: at });
    }
  };
  const close = () => {
    op.set(null);
    rec.set(null);
    opHighlight.set([]);
  };

  const operations = () => <OperationsTable view={d} rows={shown()} order={(o) => (opsOrder = o)} onSelect={chooseOperation} ref={(r) => (opsRef = r)} highlight={opHighlight()} compact={pane() === "aligned" || inspecting()} />;
  const output = () => <OutputTable lines={d.lines} onSelect={chooseLine} ref={(r) => (outRef = r)} highlight={outHighlight()} />;
  // the app's session page, read-only: its header, then what is shown of it (the tabs), then
  // the pane chosen — the shape of the session and what stands out over its operations
  const findings = () => <Observations view={d} dense narrow={box().w < BREAK.inspector} filter={filter()} onFilter={(f) => filter.set(f)} maxHeight={Math.max(104, Math.round(box().h * 0.3))} />;
  const strip = () => (
    <If when={d.profile && box().h >= BREAK.short}>
      <SessionShape strip profile={d.profile} slice={slice()} onSlice={(i) => slice.set(i)} annotate marks={findingMarks(d)} class="mt-3 h-11 w-full shrink-0" />
    </If>
  );
  const body = () => {
    if (pane() === "time" && d.profile) return <TimePane view={d} onRequest={(id) => rec.set(id)} />;
    if (pane() === "operations" || !d.lines.length)
      return (
        <View class="min-h-0 flex-1 gap-3">
          {strip()}
          {findings()}
          {operations()}
        </View>
      );
    return (
      <View class="min-h-0 flex-1 gap-3">
        {findings()}
        <ResizablePanelGroup axis="horizontal" class="min-h-0 flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("sessions.resizeOutput")}>
          <ResizablePanel minSize={240}>
            <View class="flex-1 pr-2">{output()}</View>
          </ResizablePanel>
          <ResizablePanel minSize={320}>
            <View class="flex-1 pl-2">{operations()}</View>
          </ResizablePanel>
        </ResizablePanelGroup>
      </View>
    );
  };

  const main = () => (
    <View class="min-h-0 flex-1">
      <View class="shrink-0 px-4 pt-4">
        <SessionHeader
          view={d}
          actions={
            // where the app has a session's commands: whose page this is
            <View class="shrink-0 flex-row items-baseline gap-2">
              <Strong>sonde</Strong>
              <Muted class="text-small">{t("viewer.shared")}</Muted>
            </View>
          }
        />
      </View>
      <View class="shrink-0 flex-row items-center gap-3 border-b border-border px-4 pb-3 pt-3">
        <Segmented
          testID="session-pane-mode"
          value={pane()}
          items={[
            { value: "operations", title: t("sessions.operations"), icon: "list-filter" },
            ...(d.lines.length ? [{ value: "aligned", title: t("sessions.withOutput"), icon: "scroll-text" as const }] : []),
            ...(d.profile ? [{ value: "time", title: t("profile.pane"), icon: "chart-pie" as const }] : []),
          ]}
          onChange={(p) => pane.set(p as Pane)}
        />
        <View class="flex-1" />
        {/* what narrows the operations, said where they are */}
        <If when={pane() !== "time" ? filterNote() : null}>
          {(n) => <Button small variant="outline" icon="x" label={n()} tip={t("sessions.clearFilter")} onPress={() => batch(() => (filter.set(null), slice.set(null)))} testID="clear-filter" />}
        </If>
      </View>
      <View class="min-h-0 flex-1 px-4 pb-4 pt-3">{body()}</View>
    </View>
  );

  return (
    <View class="flex-1 bg-background" testID="viewer" onLayout={(e) => box.set({ w: Math.round(e.bounds.size.width), h: Math.round(e.bounds.size.height) })}>
      <ResizablePanelGroup axis="horizontal" class="flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("sessions.resizeInspector")}>
        <ResizablePanel minSize={460}>{main()}</ResizablePanel>
        <If when={inspecting()}>
          <ResizablePanel size={420} minSize={320} maxSize={680}>
            <View testID="inspector" class="flex-1 border-l border-border bg-inspector">
              <PanelHeader icon={record() ? "radio" : "layers"} title={record() ? t("inspector.request") : t("inspector.operation")} onClose={close} closeTestID="inspector-close" />
              <Scroll class="flex-1" contentClass="p-4">
                <If when={record()} else={<If when={episode()}>{(e) => <OperationView episode={e()} shared={d.sharedNotes} rows={rows(e())} onRecord={(id) => rec.set(id)} />}</If>}>
                  {(r) => <RecordView record={r()} onBack={() => rec.set(null)} />}
                </If>
              </Scroll>
            </View>
          </ResizablePanel>
        </If>
      </ResizablePanelGroup>
    </View>
  );
}
