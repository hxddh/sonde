// A shared run: what `sonde share` writes, read in a browser. The app's own
// views — the figures, what stands out (each a filter), the operations (each
// placed in time), the program's output lined up, the Time pane, a
// comparison with another run when it was shared against one, and an
// inspector from an operation to its requests to one request in full — over
// the run's precomputed document.

import { batch, computed, effect, Scroll, signal, untracked, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup, type DataTableRef } from "nsh/base";
import { duration } from "../app/src/format";
import { t } from "../app/src/i18n";
import { viewOfWhole, wholeOperations } from "../app/src/operations";
import type { EpisodeRow, LogLine, Profile, RecordSummary, RequestRecord, RunChange, Signals, WholeRun } from "../app/src/types";
import { RunChangeView } from "../app/src/ui/runchange";
import { BREAK, Button, Muted, PanelHeader, Segmented, SPLIT_HANDLE, Strong, PANE, SIZES } from "../app/src/ui/kit";
import { RunShape, SHAPE_STRIP, TICK_ROOM } from "../app/src/ui/plots";
import { RecordView } from "../app/src/ui/record";
import { Diagnoses, diagnosesOf, findingMarks, FixDetail, headOf, linesOf, OperationsTable, OperationView, OutputTable, RunHeader } from "../app/src/ui/runview";
import { TimePane } from "../app/src/ui/profile";
import { type Range, rangeOf, rangeParam } from "../app/src/range";

/** The page's address, where the page has one (a browser): its hash keeps the stretch of time shown. */
type Place = { location?: { hash: string; pathname: string; search: string }; history?: { replaceState: (s: unknown, t: string, u: string) => void }; parent?: Place };
// (the viewer runs in a frame of the page, Native Shell's realm: the address is the page's, its parent's)
const place = (): Place => {
  const g = globalThis as Place;
  try {
    return g.parent?.location ? g.parent : g;
  } catch {
    return g;
  }
};
/** Whether the address asks for the Time pane (`#time`, or a stretch of time), read once as the page opens. */
const hashTime: boolean = (() => {
  try {
    return place().location?.hash === "#time";
  } catch {
    return false;
  }
})();
/** The stretch the address names (`#range=120-480`), read once as the page opens. */
const hashRange: Range | null = (() => {
  try {
    return rangeOf(place().location?.hash ?? "");
  } catch {
    return null;
  }
})();
/** The stretch shown, kept in the address (replaced, not pushed: Back leaves the page), so a copied address opens on it. */
function keepRange(r: Range | null) {
  try {
    const g = place();
    if (!g.location || !g.history) return;
    g.history.replaceState(null, "", `${g.location.pathname}${g.location.search}${r ? `#${rangeParam(r)}` : ""}`);
  } catch {
    // no address to keep it in
  }
}
import type { SignalId } from "../app/src/signals";

/** What `sonde share` writes: the run whole, every operation and request in the page, and the run it was compared
 * with (`--baseline`, or two runs), when it was. */
export interface SharedRun extends WholeRun {
  format: "sonde-share";
  version: number;
  records: RequestRecord[];
  lines: LogLine[];
  compare?: { name: string; change: RunChange; signals: [Signals, Signals]; profiles: [Profile, Profile] };
}

type Pane = "compare" | "operations" | "output" | "time";

export function RunViewer(props: { data: SharedRun }) {
  // the page's run in the shape a view has, and its operations answered from the page as the
  // app's are from the bridge: one table, one inspector, the same narrowing
  const d = viewOfWhole(props.data);
  const source = wholeOperations(() => d);
  const byId = new Map(d.records.map((r) => [r.id, r]));
  const op = signal<number | null>(null);
  const rec = signal<number | null>(null);
  const opHighlight = signal<number[]>([]);
  const outHighlight = signal<number[]>([]);
  // the diagnosis chosen: its operations narrowed, its fix in the inspector
  const filter = signal<number | null>(null);
  const diagnosis = () => (filter() === null ? null : (diagnosesOf(d)[filter()!] ?? null));
  /** The finding whose operations the chosen diagnosis narrows the table to: its headline, when it has any. */
  const narrowedTo = (): number | null => {
    const x = diagnosis();
    const o = x ? headOf(d, x) : undefined;
    return x && o && (o.operationsTotal ?? o.operations.length) > 0 ? x.findings[0] : null;
  };
  // an address that names a stretch of time opens on the Time pane, over it
  // a run shared against another opens on what changed
  const pane = signal<Pane>((hashRange || hashTime) && d.profile ? "time" : props.data.compare ? "compare" : "operations");
  // a signal pressed in the header: the Time pane, on its chart
  const reveal = signal<{ id: SignalId; n: number } | null>(null);
  const showSignal = (id: SignalId) => batch(() => (reveal.set({ id, n: (reveal()?.n ?? 0) + 1 }), pane.set("time")));
  let opsRef: DataTableRef | undefined;
  let outRef: DataTableRef | undefined;

  const episode = computed<EpisodeRow | null>(() => {
    const i = op();
    return i === null ? null : source.episode(i);
  });
  // a request chosen on the Time pane has no operation open: the inspector shows it all the same
  const record = computed(() => (rec() === null ? null : (byId.get(rec()!) ?? null)));
  const inspecting = () => !!episode() || !!record() || !!diagnosis();
  // this page's own size: what it shows of a finding and whether the shape has room follow it
  const box = signal({ w: 1280, h: 800 });
  /** A slice of the run's shape, picked on the strip: the operations running then. */
  const slice = signal<number | null>(null);
  const window = (): [number, number] | null => {
    const i = slice();
    if (i === null || !d.profile) return null;
    const a = d.profile.rate[i]?.tMs ?? 0;
    return [a, a + d.profile.binMs];
  };
  /** The operations shown: all, those the chosen diagnosis is about, and those running in the chosen slice. */
  effect(() => {
    const f = narrowedTo();
    const w = window();
    untracked(() => {
      const { finding: _, slice: __, ...kept } = source.query();
      source.ask({ ...kept, ...(f === null ? {} : { finding: f }), ...(w ? { slice: w } : {}) });
    });
  });
  const filterNote = () => {
    const w = window();
    if (narrowedTo() === null && !w) return null;
    const n = source.total();
    return w ? t("profile.sliceFilter", { a: duration(w[0]), b: duration(w[1]), n, of: d.operationsTotal }) : t("runs.filtered", { n, of: d.operationsTotal });
  };
  const rows = (e: EpisodeRow): RecordSummary[] => e.records.map((id) => byId.get(id)).filter((r): r is RequestRecord => !!r);
  const chooseOperation = (i: number) => {
    op.set(i);
    rec.set(null);
    opHighlight.set([i]);
    const e = source.episode(i);
    const ls = e ? linesOf(d.lines, e) : [];
    outHighlight.set(ls);
    if (ls.length) outRef?.scrollToRow(ls[0]);
  };
  const chooseLine = (i: number) => {
    outHighlight.set([i]);
    const ops = source.at(d.lines[i].time);
    opHighlight.set(ops);
    if (ops.length) {
      op.set(ops[0]);
      rec.set(null);
      // the row as the table shows it: sorted by a column, it is not the operation's index
      const at = source.rowOf(ops[0]);
      if (at >= 0) opsRef?.select({ row: at });
    }
  };
  // an operation or a request shown gives way to the chosen diagnosis's fix; the fix closed lets the diagnosis go
  const close = () => {
    if (op() === null && rec() === null) filter.set(null);
    op.set(null);
    rec.set(null);
    opHighlight.set([]);
  };

  const operations = () => <OperationsTable view={d} source={source} onSelect={chooseOperation} ref={(r) => (opsRef = r)} highlight={opHighlight()} />;
  const output = () => <OutputTable lines={d.lines} onSelect={chooseLine} ref={(r) => (outRef = r)} highlight={outHighlight()} />;
  // the app's run page, read-only: its header, then what is shown of it (the tabs), then
  // the pane chosen — the shape of the run and what stands out over its operations
  const findings = () => <Diagnoses view={d} dense narrow={box().w < BREAK.inspector || inspecting()} selected={filter()} onSelect={(i) => batch(() => (filter.set(i), op.set(null), rec.set(null), opHighlight.set([])))} maxHeight={Math.max(SIZES.findingsLeast, Math.round(box().h * SIZES.findingsShare))} />;
  const strip = () => (
    <If when={d.profile && box().h >= BREAK.short}>
      <RunShape strip axis profile={d.profile} slice={slice()} onSlice={(i) => slice.set(i)} annotate marks={findingMarks(d)} class={`mt-3 ${TICK_ROOM} ${SHAPE_STRIP} w-full shrink-0`} />
    </If>
  );
  const body = () => {
    const c = props.data.compare;
    if (pane() === "compare" && c)
      return (
        <View class="min-h-0 flex-1 gap-2">
          <Muted class="text-small">{t("viewer.against", { name: c.name })}</Muted>
          <RunChangeView change={c.change} profiles={c.profiles} />
        </View>
      );
    if (pane() === "time" && d.profile) return <TimePane view={d} onRequest={(id) => rec.set(id)} reveal={reveal()} range={hashRange} onRange={keepRange} />;
    // Output: the output beside the operations, each lighting up the other
    if (pane() === "output" && d.lines.length)
      return (
        <ResizablePanelGroup axis="horizontal" class="min-h-0 flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("runs.resizeOutput")}>
          <ResizablePanel minSize={PANE.outputLeast}>
            <View class="flex-1 pr-2">{output()}</View>
          </ResizablePanel>
          <ResizablePanel minSize={PANE.outputLeast}>
            <View class="flex-1 pl-2">{operations()}</View>
          </ResizablePanel>
        </ResizablePanelGroup>
      );
    return (
      <View class="min-h-0 flex-1 gap-3">
        {strip()}
        {findings()}
        {operations()}
      </View>
    );
  };

  const main = () => (
    <View class="min-h-0 flex-1">
      <View class="shrink-0 px-4 pt-4">
        <RunHeader
          view={d}
          onSignal={d.profile ? showSignal : undefined}
          actions={() => (
            // where the app has a run's commands: whose page this is
            <View class="shrink-0 flex-row items-baseline gap-2">
              <Strong>sonde</Strong>
              <Muted class="text-small">{t("viewer.shared")}</Muted>
            </View>
          )}
        />
      </View>
      <View class="shrink-0 flex-row items-center gap-3 border-b border-border px-4 pb-3 pt-3">
        <Segmented
          testID="run-pane-mode"
          value={pane()}
          items={[
            ...(props.data.compare ? [{ value: "compare", title: t("viewer.compared"), icon: "git-compare-arrows" as const }] : []),
            { value: "operations", title: t("runs.operations"), icon: "list-filter" as const },
            { value: "output", title: t("runs.output"), icon: "scroll-text" as const, disabled: !d.lines.length, why: t("runs.noOutput") },
            { value: "time", title: t("profile.pane"), icon: "chart-pie" as const, disabled: !d.profile },
          ]}
          onChange={(p) => pane.set(p as Pane)}
        />
        <View class="flex-1" />
        {/* what narrows the operations, said where they are */}
        <If when={pane() !== "time" && pane() !== "compare" ? filterNote() : null}>
          {(n) => <Button small variant="outline" icon="x" label={n()} tip={t("runs.clearFilter")} onPress={() => batch(() => (filter.set(null), slice.set(null)))} testID="clear-filter" />}
        </If>
      </View>
      <View class="min-h-0 flex-1 px-4 pb-4 pt-3">{body()}</View>
    </View>
  );

  return (
    <View class="flex-1 bg-background" testID="viewer" onLayout={(e) => box.set({ w: Math.round(e.bounds.size.width), h: Math.round(e.bounds.size.height) })}>
      <ResizablePanelGroup axis="horizontal" class="flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("runs.resizeInspector")}>
        <ResizablePanel minSize={PANE.runLeast}>{main()}</ResizablePanel>
        <If when={inspecting()}>
          <ResizablePanel size={PANE.inspector} minSize={PANE.inspectorLeast} maxSize={PANE.inspectorMost}>
            <View testID="inspector" class="flex-1 border-l border-border bg-inspector">
              <PanelHeader icon={record() ? "radio" : episode() ? "layers" : "sparkles"} title={record() ? t("inspector.request") : episode() ? t("inspector.operation") : t("fix.title")} onClose={close} closeTestID="inspector-close" />
              <Scroll class="flex-1" contentClass="p-4">
                <If when={record()} else={<If when={episode()} else={<If when={diagnosis()}>{(x) => <FixDetail view={d} diagnosis={x()} />}</If>}>{(e) => <OperationView episode={e()} shared={d.sharedNotes} rows={rows(e())} onRecord={(id) => rec.set(id)} />}</If>}>
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
