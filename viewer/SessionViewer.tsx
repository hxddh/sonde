// A shared session: what `sonde share` publishes, read in a browser. The
// app's own views — the figures, what stands out (each a filter), the
// operations (each placed in time), the program's output lined up, and an
// inspector from an operation to its requests to one request in full —
// over the session's precomputed `session.json`.

import { computed, Scroll, signal, Text, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup, type DataTableRef } from "nsh/base";
import { t } from "../app/src/i18n";
import type { Episode, LogLine, RecordSummary, RequestRecord, SessionView } from "../app/src/types";
import { Empty, IconButton, Segmented } from "../app/src/ui/kit";
import { RecordView } from "../app/src/ui/record";
import { linesOf, Observations, OperationsTable, OperationView, OutputTable, operationsAt, SessionHeader } from "../app/src/ui/sessionview";
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

  const episode = computed<Episode | null>(() => {
    const i = op();
    return i === null ? null : (d.episodes[i] ?? null);
  });
  const shown = () => (filter() === null ? undefined : d.observations[filter()!]?.episodes);
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
      const at = shown() ? shown()!.indexOf(ops[0]) : ops[0];
      if (at >= 0) opsRef?.select({ row: at });
    }
  };
  const close = () => {
    op.set(null);
    rec.set(null);
    opHighlight.set([]);
  };

  const operations = () => <OperationsTable view={d} rows={shown()} onSelect={chooseOperation} ref={(r) => (opsRef = r)} highlight={opHighlight()} compact={pane() === "aligned" || !!episode()} />;
  const output = () => <OutputTable lines={d.lines} onSelect={chooseLine} ref={(r) => (outRef = r)} highlight={outHighlight()} />;
  const body = () => {
    if (pane() === "time" && d.profile) return <TimePane view={d} onRequest={(id) => rec.set(id)} />;
    if (pane() === "operations" || !d.lines.length) return operations();
    return (
      <ResizablePanelGroup axis="horizontal" class="min-h-0 flex-1">
        <ResizablePanel minSize={240}>
          <View class="flex-1 pr-1.5">{output()}</View>
        </ResizablePanel>
        <ResizablePanel minSize={320}>
          <View class="flex-1 pl-1.5">{operations()}</View>
        </ResizablePanel>
      </ResizablePanelGroup>
    );
  };

  const main = () => (
    <View class="min-h-0 flex-1">
      <View class="gap-3.5 border-b border-border px-5 pb-3.5 pt-4">
        <View class="flex-row items-baseline gap-2">
          <Text class="text-heading font-bold text-foreground">sonde</Text>
          <Text class="text-small text-muted-foreground">{t("viewer.shared")}</Text>
        </View>
        <SessionHeader view={d} compact={!!episode()} />
        <Observations view={d} filter={filter()} onFilter={(f) => filter.set(f)} />
      </View>
      <View class="flex-row items-center px-5 py-2.5">
        <Segmented
          value={pane()}
          items={[
            { value: "operations", title: t("sessions.operations"), icon: "list-filter" },
            ...(d.lines.length ? [{ value: "aligned", title: t("sessions.withOutput"), icon: "scroll-text" as const }] : []),
            ...(d.profile ? [{ value: "time", title: t("profile.pane"), icon: "chart-pie" as const }] : []),
          ]}
          onChange={(p) => pane.set(p as Pane)}
        />
      </View>
      <View class="min-h-0 flex-1 px-5 pb-4">{body()}</View>
    </View>
  );

  return (
    <View class="flex-1 bg-background" testID="viewer">
      <ResizablePanelGroup axis="horizontal" class="flex-1">
        <ResizablePanel minSize={460}>{main()}</ResizablePanel>
        <If when={episode()}>
          {(e) => (
            <ResizablePanel size={440} minSize={320}>
              <View class="flex-1 border-l border-border bg-inspector">
                <View class="h-10 flex-row items-center border-b border-border px-3">
                  <Text class="flex-1 text-small font-semibold text-muted-foreground">{rec() !== null ? t("inspector.request") : t("inspector.operation")}</Text>
                  <IconButton icon="x" label={t("common.close")} onPress={close} />
                </View>
                <Scroll class="flex-1" contentClass="p-4">
                  <If when={rec() !== null ? byId.get(rec()!) : null} else={<OperationView episode={e()} shared={d.sharedNotes} rows={rows(e())} onRecord={(id) => rec.set(id)} />}>
                    {(r) => <RecordView record={r()} onBack={() => rec.set(null)} />}
                  </If>
                </Scroll>
              </View>
            </ResizablePanel>
          )}
        </If>
      </ResizablePanelGroup>
    </View>
  );
}

export function Loading(props: { error?: string | null }) {
  return <Empty icon="scroll-text" text={props.error ?? t("common.loading")} />;
}
