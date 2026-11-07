// A shared session: what `sonde share` publishes, read in a browser. The
// app's own views — header, observations, operations or waterfall, the
// program's output lined up, an operation's requests, a request in full —
// over the session's precomputed `session.json`.

import { computed, Scroll, signal, Text, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup, type DataTableRef } from "nsh/base";
import { t } from "../app/src/i18n";
import type { Episode, LogLine, RecordSummary, RequestRecord, SessionView } from "../app/src/types";
import { Empty, Muted, Segmented } from "../app/src/ui/kit";
import { RecordView } from "../app/src/ui/record";
import { linesOf, Observations, OperationsTable, OperationView, OutputTable, operationsAt, SessionHeader } from "../app/src/ui/sessionview";
import { Waterfall } from "../app/src/ui/waterfall";

/** What `sonde share` writes. */
export interface SharedSession extends SessionView {
  format: "sonde-share";
  version: number;
  records: RequestRecord[];
  lines: LogLine[];
}

export function SessionViewer(props: { data: SharedSession }) {
  const d = props.data;
  const byId = new Map(d.records.map((r) => [r.id, r]));
  const op = signal<number | null>(null);
  const rec = signal<number | null>(null);
  const opHighlight = signal<number[]>([]);
  const outHighlight = signal<number[]>([]);
  const pane = signal<"operations" | "waterfall">("operations");
  let opsRef: DataTableRef | undefined;
  let outRef: DataTableRef | undefined;

  const episode = computed<Episode | null>(() => {
    const i = op();
    return i === null ? null : (d.episodes[i] ?? null);
  });
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
      opsRef?.select({ row: ops[0] });
    }
  };

  const operations = () => <OperationsTable view={d} onSelect={chooseOperation} ref={(r) => (opsRef = r)} highlight={opHighlight()} />;
  const output = () => <OutputTable lines={d.lines} onSelect={chooseLine} ref={(r) => (outRef = r)} highlight={outHighlight()} />;
  const body = () => {
    if (pane() === "waterfall") return <Waterfall view={d} onSelect={chooseOperation} />;
    if (!d.lines.length) return operations();
    return (
      <ResizablePanelGroup axis="horizontal" class="flex-1">
        <ResizablePanel minSize={240}>
          <View class="flex-1 pr-1">{output()}</View>
        </ResizablePanel>
        <ResizablePanel minSize={280}>
          <View class="flex-1 pl-1">{operations()}</View>
        </ResizablePanel>
      </ResizablePanelGroup>
    );
  };

  return (
    <View class="flex-1 flex-row gap-3 bg-background p-4" testID="viewer">
      <View class="flex-1 gap-2">
        <View class="flex-row items-baseline gap-2">
          <Text class="text-base font-bold text-foreground">sonde</Text>
          <Muted class="text-xs">{t("viewer.shared")}</Muted>
        </View>
        <SessionHeader view={d} />
        <Observations view={d} />
        <Segmented
          value={pane()}
          items={[
            { value: "operations", title: t("sessions.operations") },
            { value: "waterfall", title: t("sessions.waterfall") },
          ]}
          onChange={(p) => pane.set(p as "operations" | "waterfall")}
        />
        {body()}
      </View>
      <If when={episode()}>
        {(e) => (
          <Scroll class="w-[420px] border-l border-border pl-3">
            <If when={rec() !== null ? byId.get(rec()!) : null} else={<OperationView episode={e()} shared={d.sharedNotes} rows={rows(e())} onRecord={(id) => rec.set(id)} />}>
              {(r) => <RecordView record={r()} onBack={() => rec.set(null)} />}
            </If>
          </Scroll>
        )}
      </If>
    </View>
  );
}

export function Loading(props: { error?: string | null }) {
  return <Empty text={props.error ?? t("common.loading")} />;
}
