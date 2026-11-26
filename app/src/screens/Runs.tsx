// Runs: what programs did to their object stores, and where a program is
// run. The list on the left, by day, the newest first (a program running
// now is the newest row); with none chosen, the command bar. A run reads
// from its figures and what stands out (each a filter) down to its
// operations — outcome, time, family, place in time — and an inspector
// beside them that goes from an operation to its requests to one request in
// full. When sonde ran the program, its Output tab lines its output up
// beside the operations (its terminal, while it runs): choose one and the
// other lights up. A run also shows as a timeline, replays on another endpoint, compares
// with another run, and opens in a window of its own.

import { batch, cleanup, computed, effect, type JSX, resource, Scroll, signal, untracked, View, type ViewRef } from "nsh";
import { ResizablePanel, ResizablePanelGroup, type DataTableRef } from "nsh/base";
import { isAvailable, type MenuItem, Window } from "nsh/desktop";
import { count, duration, moments } from "../format";
import { t } from "../i18n";
import { analysis } from "../analysis";
import { pagedOperations } from "../paged";
import { api } from "../native";
import { applyDemoFix, isDemoJob } from "../demo";
import { type PlaceCommand, offerCommands, away, chooseBucket, copy, endpoints, findingOnOpen, go, newRunAsked, openRun, paneOnOpen, openRunFile, reading, report, runsChanged, runsTick, showInSidebar, shownRun, startDemo, useEndpoint } from "../store";
import type { Diagnosis, LogLine, Observation, RunInfo, RunView } from "../types";
import { CompareView, ReplayDialog } from "../ui/compare";
import { iconSvg } from "../ui/icon";
import { Await, BREAK, Button, Empty, IconButton, More, Notice, PanelHeader, popupMenu, Recoverable, Segmented, RunSkeleton, SidePanel, SIZES, SPLIT_HANDLE, SPLIT_GAP, PANE, windowFace, glideLayout } from "../ui/kit";
import { Toaster } from "../ui/toast";
import { fileName, removeRun, rename, RenameDialog, runName, shareRun } from "../ui/manage";
import { rememberedSize } from "../ui/sizes";
import { linesOf, OperationDetail, OperationsTable, OutputTable, RecordingNote, RunHeader, RunList } from "../ui/runparts";
import { Diagnoses, diagnosesOf, findingMarks, FixDetail, headOf, operationsText, runMeta, storeFixOf } from "../ui/runview";
import { episodeTitle } from "../phrases";
import { programOf, ranCommand } from "../ui/runview";
import { HistoryPane, scenarioLabel } from "../ui/history";
import { TrialDialog, trialsKept } from "../ui/trial";
import { ConfirmRunDialog } from "../ui/external";
import type { Confirmed } from "../types";
import { SinceRun, sinceLastOf, statesOf } from "../ui/fixloop";
import { type GroupBy, GROUP_BY, SplitTable } from "../ui/split";
import { TimePane } from "../ui/profile";
import type { SignalId } from "../signals";
import { RunShape, SHAPE_STRIP, TICK_ROOM, faultMoments, began } from "../ui/plots";
import { RecordDetail } from "../ui/request";
import { type Range, rangeOnOpen, rangeParam, shownRange } from "../range";
import { busy, lastRun, recordingPath, running, runAgain, stop, stoppable } from "../run";
import { NewRun, NoRequests, ProgramTerminal } from "./Run";

type Pane = "operations" | "output" | "time" | "runs";

const program = programOf;

/** How many operations "Copy operations as a table" takes: what a paste into a sheet or a ticket holds well. */
const COPY_ROWS = 20000;
/** The budget a run's program has in its project's sonde.toml (null: none, or no folder to find it from). */
const budgetOf = (path: string) => {
  try {
    return api.runs.budget(path);
  } catch {
    return null;
  }
};
/** The inspector's width, as the user leaves it. */
const inspectorWidth = rememberedSize("runs.inspector", PANE.inspector);

/** One run: its figures, its operations (and output), the inspector, and what can be done with it. */

export function RunPane(props: { path: string; others: () => RunInfo[]; tick: () => number; onOpen?: (p: string) => void; inWindow?: boolean; /** shown only because it is the newest, not because anyone opened it */ passing?: boolean }) {
  // read here, whatever way led here: an empty run is not tidied away under its reader.
  // The newest run shown on a visit to the place was not opened by anyone: a run
  // the toast said was not kept is not kept by passing it
  if (!props.passing) reading(props.path);
  const op = signal<number | null>(null);
  const rec = signal<number | null>(null);
  const opHighlight = signal<number[]>([]);
  const outHighlight = signal<number[]>([]);
  // the diagnosis chosen: its operations narrowed, its fix in the inspector
  const filter = signal<number | null>(null);
  // a kind of request chosen (Group by's row, the Time pane's by-kind table): the operations that sent one
  const kind = signal<string | null>(null);
  const compareWith = signal<string | null>(null);
  const replaying = signal(false);
  const tryingFaults = signal(false);
  // a run from elsewhere, asked to run again: its command and folder, asked about before anything starts
  const askAgain = signal<Confirmed | null>(null);
  // Group by: each operation, or the operations split by a dimension
  const groupBy = signal<GroupBy>("none");
  // a signal pressed in the header: the Time pane, on its chart
  const reveal = signal<{ id: SignalId; n: number } | null>(null);
  const showSignal = (id: SignalId) => batch(() => (reveal.set({ id, n: (reveal()?.n ?? 0) + 1 }), pane.set("time")));
  let opsRef: DataTableRef | undefined;
  let outRef: DataTableRef | undefined;

  // read again as it grows, and when a trial of it ends (what broke is among its findings)
  const read = analysis(() => props.path, () => `${props.tick()}:${trialsKept()}`);
  const view = read.view;
  // the run against its program's run before: its findings' states, what was fixed
  const since = sinceLastOf(() => props.path, () => recordingPath() === props.path);
  // a trial's run: the clean run it is judged against, for the Time pane to draw it over
  const clean = analysis(() => view()?.trial?.clean ?? null);
  // the program's budget, for the Time pane's lines: read once per run shown
  const budget = computed(() => budgetOf(props.path));
  // this pane's own size (in a run's window too, where windowSize() is the main window's)
  const box = signal({ w: 1000, h: 760 });
  // its output, read off the interface's thread (a large run is read whole the first time), again as it grows
  const linesRead = resource(
    computed(() => `${props.tick()}\n${props.path}`),
    async () => {
      const path = untracked(() => props.path);
      // (read while the program still ran: more may come)
      const ran = untracked(() => recordingPath() === path);
      try {
        return { path, ran, lines: await api.runs.logLines(path) };
      } catch {
        return { path, ran, lines: [] as LogLine[] };
      }
    },
  );
  const lines = (): LogLine[] => {
    const r = linesRead();
    return r && r.path === props.path ? r.lines : [];
  };
  // whether it has any (a boolean: what is drawn does not follow every line that arrives)
  const hasOutput = computed(() => lines().length > 0);
  // recorded now (a program, the demo's job, Record…): its terminal (or what to point an application at) stands
  // where its output will
  const live = () => recordingPath() === props.path;
  // a run opens on its operations; a program running now on its Output, its terminal
  const pane = signal<Pane>(untracked(live) ? "output" : "operations");
  // a run with no output (a program that ended having printed nothing) has no Output to stay on: once its lines are
  // read after it ended (not before: a read while it ran, or one under way as it ends, says nothing of its last lines)
  effect(() => {
    const r = linesRead();
    if (pane() === "output" && !live() && r && r.path === props.path && !r.ran && r.lines.length === 0 && !linesRead.loading()) untracked(() => pane.set("operations"));
  });
  // a trial tried again: its fresh run, on its runs
  effect(() => {
    const o = paneOnOpen();
    if (o && o.path === props.path) untracked(() => batch(() => (pane.set(o.pane), paneOnOpen.set(null))));
  });
  // the demo's run: its first diagnosis chosen, its line of code and its fix in the inspector
  effect(() => {
    const o = findingOnOpen();
    const v = view();
    // (once the run is read with it: read while it was still recorded it may have none yet)
    if (o && v && o.path === props.path && o.index < diagnosesOf(v).length) untracked(() => batch(() => (filter.set(o.index), findingOnOpen.set(null))));
  });
  // (a run with no finding there opens on all its operations: what it asked goes with its pane)
  cleanup(() => untracked(findingOnOpen)?.path === props.path && findingOnOpen.set(null));
  /** The diagnosis chosen, while the run has it. */
  const diagnosis = () => {
    const v = view();
    const i = filter();
    return v && i !== null ? (diagnosesOf(v)[i] ?? null) : null;
  };
  /** The finding whose operations the chosen diagnosis narrows the table to: its headline, when it has any. */
  const narrowedTo = (): number | null => {
    const v = view();
    const d = diagnosis();
    const o = v && d ? headOf(v, d) : undefined;
    return d && o && (o.operationsTotal ?? o.operations.length) > 0 ? d.findings[0] : null;
  };
  // a link with a stretch of time: the Time pane, over it
  const linkRange = signal<Range | null>(null);
  effect(() => {
    const o = rangeOnOpen();
    if (o && o.name === fileName(props.path)) untracked(() => batch(() => (linkRange.set(o.range), pane.set("time"), rangeOnOpen.set(null))));
  });

  /** A link to this run, over the stretch its Time pane shows (when one is chosen there). */
  const link = () => {
    const r = pane() === "time" ? (shownRange()[props.path] ?? null) : null;
    const q = r ? rangeParam(r) : "";
    // one of the runs folder by its name; any other file by its path
    return info() ? `sonde://run/${encodeURIComponent(fileName(props.path))}${q ? `?${q}` : ""}` : `sonde://open?path=${encodeURIComponent(props.path)}${q ? `&${q}` : ""}`;
  };

  // its operations: the first page with the view, the rest in pages as the table scrolls,
  // sorted and narrowed where they are kept (a run of a hundred thousand is never whole here)
  const source = pagedOperations(() => props.path, view);
  // the operation chosen, whole (every request's id): read once per choice, and again when the run is
  const episode = computed(() => {
    const i = op();
    return i === null || !view() ? null : source.episode(i);
  });
  // read once per request chosen, off the interface's thread (a large run's from its file):
  // the inspector asks for it in several places
  const recordRead = resource(
    () => {
      const id = rec();
      return id === null ? false : ([id] as const);
    },
    async ([id]) => {
      try {
        return { id, record: await api.runs.record(props.path, id) };
      } catch (e) {
        report(e);
        return { id, record: null };
      }
    },
  );
  const record = () => {
    const id = rec();
    const r = recordRead();
    return id !== null && r?.id === id ? r.record : null;
  };
  /** The run's one selection of time: a stretch of its slices (first and last), picked on the
   * Operations strip (a slice) or on the Time pane's timeline (two presses) — the same on both. */
  const stretch = signal<[number, number] | null>(null);
  // a live run's slices change length as it grows: a picked one would point at another time
  let binMs: number | undefined;
  effect(() => {
    const b = view()?.profile?.binMs;
    if (binMs !== undefined && b !== binMs) untracked(() => stretch.set(null));
    binMs = b;
  });
  /** A failure bar pressed on the Time pane: the operations that failed in its slice (ms from the run's start). */
  const failedIn = signal<[number, number] | null>(null);
  /** The stretch as the time it covers, from the run's start (a failure bar's slice, while one is pressed). */
  const window = (): [number, number] | null => {
    const v = view();
    const z = stretch();
    if (failedIn()) return failedIn();
    if (!v || z === null || !v.profile) return null;
    const a = v.profile.rate[z[0]]?.tMs ?? 0;
    return [a, (v.profile.rate[z[1]]?.tMs ?? a) + v.profile.binMs];
  };
  /** The one slice chosen, when the stretch is one. */
  const slice = () => {
    const z = stretch();
    return z && z[0] === z[1] ? z[0] : null;
  };
  /** The operations shown: all, those the chosen diagnosis is about, and those running in the chosen slice —
   * narrowed where they are kept, whatever order the table has them in. */
  effect(() => {
    const f = narrowedTo();
    const w = window();
    const k = kind();
    const failed = failedIn() !== null;
    // a finding, a slice or a kind chosen narrows the operations: they show, not their groups
    if (f !== null || w || k !== null) untracked(() => groupBy.set("none"));
    untracked(() => {
      const { finding: _, slice: __, kind: ___, failed: ____, ...kept } = source.query();
      source.ask({ ...kept, ...(f === null ? {} : { finding: f }), ...(w ? { slice: w } : {}), ...(k === null ? {} : { kind: k }), ...(failed ? { failed: true } : {}) });
    });
  });

  const chooseOperation = (i: number) => {
    // the inspector opening moves the table over, smoothly
    if (op() === null) glideLayout();
    op.set(i);
    rec.set(null);
    opHighlight.set([i]);
    const e = hasOutput() ? source.episode(i) : null;
    if (!e) return;
    const ls = linesOf(lines(), e);
    outHighlight.set(ls);
    if (ls.length) outRef?.scrollToRow(ls[0]);
  };
  const chooseLine = (i: number) => {
    const v = view();
    const line = lines()[i];
    outHighlight.set([i]);
    if (!v || !line) return;
    const ops = source.at(line.time);
    opHighlight.set(ops);
    if (ops.length) {
      op.set(ops[0]);
      rec.set(null);
      // the row as the table shows it: sorted by a column, it is not the operation's index
      const at = source.rowOf(ops[0]);
      if (at >= 0) opsRef?.select({ row: at });
    }
  };
  /** The chosen operation's row, in view: where the table shows it, sorted or narrowed. */
  const showOperation = () => {
    const i = op();
    if (i === null || !view()) return;
    const at = source.rowOf(i);
    if (at >= 0) opsRef?.scrollToRow(at);
  };
  /** A request pressed on a chart: its inspector, and the operation it belongs to chosen with
   * it — marked in the operations, its row in view when they show. */
  const chooseRequest = (id: number) => {
    const i = source.ownerOf(id) ?? -1;
    batch(() => {
      if (i >= 0) {
        op.set(i);
        opHighlight.set([i]);
      }
      rec.set(id);
    });
    showOperation();
  };
  /** Closes what the inspector shows: an operation or a request (back to the chosen diagnosis's fix, if one is
   * chosen), else the fix, letting the diagnosis go. */
  const closeInspector = () => {
    glideLayout();
    // at once: the inspector goes whole, not its request first and then what held it
    batch(() => {
      if (op() === null && rec() === null) filter.set(null);
      op.set(null);
      rec.set(null);
      opHighlight.set([]);
      outHighlight.set([]);
    });
  };

  const openWindow = () => openRunWindow(props.path);
  /** Another run from here (a trial's, the clean run's, a run of the trend): where this pane's holder shows it. */
  const open = (p: string) => (props.onOpen ? props.onOpen(p) : openRun.set(p));
  /** What sonde answered of a trial's requests: all of them said as such, else how many of how many. */
  const trialSaid = (v: RunView, run: string, scenario: string) => {
    const n = v.summary.requests;
    const answered = v.injected?.length ?? 0;
    return answered >= n ? t("trial.noteAll", { run, scenario, n: count(n) }) : t("trial.note", { run, scenario, answered: count(answered), n: count(n) });
  };
  /** A trial's run says so under its title, with the way back to the run it is a trial of. */
  const trialNote = (v: () => RunView) => (
    <If when={v().trial}>
      {(x) => (
        <Notice testID="trial-note" icon="flask-conical" text={trialSaid(v(), program(v().header.command) ?? x().of, scenarioLabel(x().scenario))}>
          <If when={x().clean}>
            <Button small variant="outline" icon="chevron-left" label={t("trial.openClean")} onPress={() => open(x().clean!)} testID="trial-open-clean" />
          </If>
        </Notice>
      )}
    </If>
  );

  /** The operation's commands: native menus where there are any. */
  const operationActions = (i: number): { label: string; icon: "copy" | "file" | "scroll-text"; run: () => void }[] => {
    const e = view() ? source.episode(i) : null;
    if (!e) return [];
    const targets = () => {
      try {
        return api.runs.records(props.path, e.records).map((r) => r.target);
      } catch {
        return [];
      }
    };
    return [
      { label: t("runs.copyTitle"), icon: "copy", run: () => copy(episodeTitle(e)) },
      { label: t("runs.copyKeys"), icon: "file", run: () => copy([...new Set(targets())].join("\n")) },
    ];
  };
  const operationMenu = (i: number) => {
    if (!view() || !isAvailable) return;
    chooseOperation(i);
    void popupMenu(operationActions(i));
  };
  let compareButton: ViewRef | undefined;
  let moreButton: ViewRef | undefined;
  const compareMenu = (anchor?: ViewRef) => {
    const others = props.others().filter((s) => s.path !== props.path);
    if (!isAvailable) return;
    // runs of this program first; another program's under its own heading
    const mine = info()?.program;
    const same = others.filter((s) => mine && s.program === mine);
    const rest = others.filter((s) => !same.includes(s));
    // runs of other days by their date too
    const when = moments([info()?.created, ...same.slice(0, 20).map((s) => s.created), ...rest.slice(0, 20).map((s) => s.created)]);
    const item = (s: RunInfo) => ({ label: `${program(s.command) ?? s.endpoint} · ${when(s.created)} · ${count(s.requests)}`, onSelect: () => compareWith.set(s.path) });
    void popupMenu(
      [
        ...(others.length
          ? [
              ...(same.length ? same.slice(0, 20).map(item) : [{ label: t("compare.noRuns"), disabled: true }]),
              ...(rest.length ? ([{ type: "separator" }, { label: t("compare.otherPrograms"), submenu: rest.slice(0, 20).map(item) }] as MenuItem[]) : []),
            ]
          : [{ label: t("compare.none"), disabled: true }]),
      ],
      // under its button, or under ⋯ where it was chosen from there
      anchor,
    );
  };
  const info = () => props.others().find((s) => s.path === props.path) ?? null;
  // the run's commands besides its one primary: behind ⋯, and in the palette while it is shown
  const runCommands = (): PlaceCommand[] => {
    if (!view()) return [];
    const tryable = ranCommand(view()?.header.command) && !view()?.trial;
    return [
      // the same command under faults is asked first what will run (a trial is no run of its own)
      ...(tryable ? [{ id: "run-try", label: t("trial.try"), icon: "flask-conical" as const, disabled: !!running(), run: () => tryingFaults.set(true) }] : []),
      ...(isAvailable ? [{ id: "compare-open", label: t("compare.open"), icon: "git-compare-arrows" as const, run: (anchor?: ViewRef) => compareMenu(anchor) }] : []),
      // the same requests on another store: will it work there? (one level, in ⋯ and the palette)
      { id: "run-another", label: t("compare.another"), icon: "repeat-2" as const, disabled: !!running(), run: () => replaying.set(true) },
    ];
  };
  // (a run's own window has no palette)
  if (!props.inWindow) offerCommands(() => t("palette.run"), runCommands);
  const moreMenu = () => {
    if (!isAvailable) return;
    const me = info();
    // the run's other commands, by their words, first: then what copies or shares it
    const extras: MenuItem[] = [
      // where and when it ran, its commit and who sent its requests (the title's tooltip says it too)
      ...(view() ? ([{ label: runMeta(view()!), disabled: true }, { type: "separator" }] as MenuItem[]) : []),
      // (each run once this menu has closed: macOS shows no menu, Compare's, opened while one is closing)
      ...runCommands().map((c) => ({ label: c.label, icon: iconSvg(c.icon), disabled: c.disabled, onSelect: () => setTimeout(() => c.run(moreButton), 0) })),
      { type: "separator" },
    ];
    void popupMenu([
      ...extras,
      { label: t("runs.window"), icon: iconSvg("app-window"), onSelect: openWindow, disabled: props.inWindow },
      { label: t("runs.share"), icon: iconSvg("share-2"), onSelect: () => void shareRun(props.path) },
      { label: t("runs.copyPath"), icon: iconSvg("copy"), onSelect: () => copy(props.path) },
      // a link that opens it in sonde, over the stretch of time the Time pane shows
      { label: pane() === "time" && shownRange()[props.path] ? t("runs.copyLinkRange") : t("runs.copyLink"), icon: iconSvg("link"), onSelect: () => copy(link()) },
      // the operations as shown, for a ticket or a spreadsheet
      {
        label: t("runs.copyTable"),
        icon: iconSvg("copy"),
        // the table is there only on its own pane
        disabled: pane() !== "operations",
        // in the order shown; figures as numbers in one unit (not "840 ms" beside "2.35 s")
        // (as many as a paste holds well: the rest are said to be left out)
        onSelect: () => {
          const v = view();
          if (v)
            void source
              .first(COPY_ROWS)
              .then(({ rows, total }) => copy(operationsText(v, rows), total > rows.length ? t("runs.copiedFirst", { n: rows.length, of: total }) : undefined))
              .catch(report);
        },
      },
      // a run written elsewhere (`-o`) is listed and opened here, never renamed or removed: its file is its writer's
      ...(me && !me.elsewhere ? ([{ type: "separator" }, { label: t("runs.rename"), icon: iconSvg("pencil"), onSelect: () => rename(me, props.inWindow ? props.path : null) }, { label: t("runs.removeMenu"), icon: iconSvg("trash"), disabled: me.live, onSelect: () => void removeRun(me) }] as MenuItem[]) : []),
    ], moreButton);
  };

  const detail = (back: boolean) => (
    <If
      when={record()}
      else={
        <If when={episode()} else={<If when={diagnosis()} else={<Empty icon="list-filter" text={t("runs.pickOperation")} />}>{(d) => fix(d)}</If>}>
          {(e) => <OperationDetail path={props.path} episode={e()} shared={view()?.sharedNotes ?? []} onRecord={(id) => rec.set(id)} onBack={back ? () => op.set(null) : undefined} />}
        </If>
      }
    >
      {(r) => <RecordDetail path={props.path} record={r()} onBack={() => rec.set(null)} onRecord={chooseRequest} />}
    </If>
  );

  let opsAnchor: ViewRef | undefined;
  // Group by, from the operations' title: each operation, or the operations split by a dimension
  const groupTo = (g: GroupBy) => batch(() => (g !== "none" && (filter.set(null), stretch.set(null), kind.set(null)), groupBy.set(g)));
  const groupMenu = () => GROUP_BY.map((g) => ({ label: t(`split.by.${g}`), checked: groupBy() === g, onSelect: () => groupTo(g) }));
  // (each part is given the view as an accessor and reads it where it draws: a live run's
  // pane is not rebuilt each second, so a stretch zoomed to or a finding chosen stays)
  const operations = (v: () => RunView) => (
    <View class="min-h-0 flex-1" ref={(r: ViewRef) => (opsAnchor = r)}>
      <OperationsTable view={v()} source={source} onSelect={chooseOperation} onContextMenu={isAvailable ? operationMenu : undefined} groupMenu={groupMenu} ref={(r) => ((opsRef = r), showOperation())} highlight={opHighlight()} follow={props.others().some((x) => x.path === props.path && x.live)} />
    </View>
  );
  // the program's output: its terminal while it runs (a recording with no program: what to point one at), its lines
  // once it has ended
  const output = () => (
    <If when={live()} else={<OutputTable lines={lines()} onSelect={chooseLine} ref={(r) => (outRef = r)} highlight={outHighlight()} />}>
      <If when={running()?.kind === "record" ? running() : null} else={<ProgramTerminal />}>
        {(r) => <RecordingNote url={r().url ?? ""} exports={r().exports ?? ""} />}
      </If>
    </If>
  );

  const v0 = () => view();
  // a run of a program has runs (a trial is no run of its own: its verdict is on the run it is a trial of)
  const hasRuns = () => ranCommand(v0()?.header.command) && !v0()?.trial;
  // the four tabs are there for every run, whatever it holds: one with nothing to show is
  // dimmed and says why, it does not go (and the others do not move)
  const panes = () => [
    { value: "operations", title: t("runs.operations"), icon: "list-filter" as const },
    { value: "output", title: t("runs.output"), icon: "scroll-text" as const, disabled: !live() && !hasOutput(), why: t("runs.noOutput") },
    { value: "time", title: t("profile.pane"), icon: "chart-pie" as const },
    { value: "runs", title: t("history.pane"), icon: "clock" as const, disabled: !hasRuns(), why: t("runs.noRuns") },
  ];

  /** A finding whose fix is a tool of Buckets (incomplete uploads): that section, on the bucket and endpoint its requests went to. */
  const openStoreFix = async (o: Observation) => {
    const v = view();
    const to = storeFixOf(o);
    if (!v || !to) return;
    try {
      const first = o.operations[0] === undefined ? null : api.runs.episode(props.path, o.operations[0]);
      const id = first?.records[0];
      const rec = id === undefined ? null : await api.runs.record(props.path, id);
      const ep = v.header.endpoint;
      batch(() => {
        if (ep === "demo" || endpoints().some((e) => e.id === ep)) useEndpoint(ep);
        if (rec?.bucket) chooseBucket(rec.bucket);
        go(to);
      });
    } catch (e) {
      report(e);
    }
  };

  const findings = (v: () => RunView) => (
    <Diagnoses
      view={v()}
      dense={dense()}
      // a narrow pane keeps the line for what was found; what it saves is in the inspector
      narrow={!roomy() || inspecting()}
      selected={filter()}
      onSelect={(i) => {
        // the fix opening moves the table over, smoothly; an operation or a request shown gives way to it
        if (!inspecting()) glideLayout();
        batch(() => (filter.set(i), op.set(null), rec.set(null), opHighlight.set([])));
      }}
      states={statesOf(since())}
      // a short window keeps a few rows in view (the rest scroll): the operations need their rows
      maxHeight={box().h < BREAK.short ? SIZES.findingsShort : Math.max(SIZES.findingsLeast, Math.round(box().h * SIZES.findingsShare))}
    />
  );
  /** A diagnosis in the inspector: its fix, and what can be done with it. */
  const fixState = (d: Diagnosis) => {
    const id = headOf(view()!, d)?.identity;
    const f = id ? statesOf(since())[id] : undefined;
    return f && f.state !== "open" && f.state !== "fixed" ? f.state : null;
  };
  const fix = (d: () => Diagnosis) => (
    <FixDetail
      view={view()!}
      diagnosis={d()}
      state={fixState(d())}
      onStore={openStoreFix}
      onExpect={(o) => copy(`expect = { ${JSON.stringify(o.identity)} = ${JSON.stringify(t("expect.reason"))} }`, t("expect.copied"))}
      onOpenCode={(place) => api.runs.openCode(props.path, place).catch(report)}
      // the demo's job: its fix applied to its code, and run again
      onApply={isDemoJob(view()!.header) ? () => void applyDemoFix(view()!.header.cwd) : undefined}
    />
  );
  /** When: the run's requests over its time, full width, the findings' numbers where they begin; a press picks a slice. */
  const strip = (v: () => RunView) => (
    <If when={v().profile && box().h >= BREAK.short}>
      <RunShape strip axis profile={v().profile} slice={slice()} picked={stretch()} onSlice={(i) => (i === null ? stretch.set(null) : began(v().profile, i, i) && stretch.set([i, i]))} annotate marks={findingMarks(v())} faults={faultMoments(v().profile, v().injected)} injected={v().injected} class={`mt-3 ${TICK_ROOM} ${SHAPE_STRIP} w-full shrink-0`} />
    </If>
  );
  const body = (v: () => RunView) => {
    if (pane() === "time") return <TimePane view={v()} groupBy={groupBy() === "none" ? null : (groupBy() as Exclude<GroupBy, "none">)} splitOf={(by) => api.runs.split(props.path, by)} kind={kind()} onKind={(k) => batch(() => (kind.set(k), k !== null && pane.set("operations")))} stretchOf={(a, b) => api.runs.stretch(props.path, a, b)} onRequest={chooseRequest} reveal={reveal()} clean={clean.view()} budget={budget()} range={linkRange()} onFailures={(ms) => batch(() => (failedIn.set(ms), pane.set("operations")))} stretch={stretch()} onStretch={(z) => batch(() => (stretch.set(z), failedIn.set(null)))} onRange={(r) => shownRange.set({ ...shownRange(), [props.path]: r })} />;
    if (pane() === "runs" && !v().trial) return <HistoryPane path={props.path} elsewhere={!!info()?.elsewhere} onOpen={open} onTrials={() => tryingFaults.set(true)} />;
    // a program that ended having sent nothing: how to point it at sonde
    if (!live() && v().operationsTotal === 0 && lastRun()?.path === props.path) return <NoRequests />;
    // Output: the program's output (its terminal, while it runs) beside the operations, each lighting up the other
    if (pane() === "output" && (live() || hasOutput()))
      return (
        <ResizablePanelGroup axis="horizontal" class="min-h-0 flex-1" handleClass={SPLIT_GAP} handleAccessibilityLabel={t("runs.resizeOutput")}>
          <ResizablePanel minSize={PANE.outputLeast}>
            <View class="flex-1 pr-2">{output()}</View>
          </ResizablePanel>
          <ResizablePanel minSize={PANE.outputLeast}>
            <View class="flex-1 pl-2">{operations(v)}</View>
          </ResizablePanel>
        </ResizablePanelGroup>
      );
    // Operations, on every run: when (the strip), what stands out, and the operations
    return (
      <View class="min-h-0 flex-1 gap-3">
        {strip(v)}
        {findings(v)}
        <If when={groupBy() !== "none"} else={operations(v)}>
          <SplitTable path={props.path} by={groupBy() as Exclude<GroupBy, "none">} onKind={(k) => kind.set(k)} />
        </If>
      </View>
    );
  };

  // the run's command again; `confirmed`: the user's yes to it, for a run from elsewhere (Rust starts nothing of one
  // without, and says what to ask about: asked then, whatever this view took the run for)
  const again = async (confirmed: Confirmed | null) => {
    const h = view()?.header;
    if (!h?.command) return;
    if (!endpoints().some((e) => e.id === h.endpoint) && !(await startDemo())) return;
    runAgain({ path: props.path, confirmed, onAsk: (ask) => askAgain.set(ask) }, h.command, h.endpoint, h.cwd);
  };
  const actions = () => (
    <View class="flex-row items-center gap-2">
      {/* the program running now: Stop, its one primary */}
      <If when={live() && stoppable(running())}>
        <Button small variant="destructive" icon="square" label={t("run.stop")} onPress={stop} testID="run-stop" />
      </If>
      {/* the program again, after a change: its new run opens, saying what changed since this one */}
      {/* the built-in demo is not saved: running a demo run again starts it */}
      <If when={!live() && ranCommand(view()?.header.command) && !view()?.trial && (view()!.header.endpoint === "demo" || endpoints().some((e) => e.id === view()!.header.endpoint))}>
        <Button
          small
          // one primary a screen: a diagnosis's Apply, while it shows, is; while anything is at work (a trial of this
          // run), its Stop is
          variant={applying() || running() ? "outline" : "primary"}
          icon="play"
          label={t("runs.runAgain")}
          onPress={() => {
            // one program at a time: a press while another runs says so, with its Stop
            if (running()) return busy();
            const h = view()!.header;
            // a run sonde did not record here: its command is someone else's, read whole and confirmed first
            if (view()!.external) return askAgain.set({ command: h.command!, cwd: h.cwd ?? null });
            void again(null);
          }}
          testID="run-again"
        />
      </If>
      {/* one primary, and the rest behind ⋯ by their words (and in the palette): Try under faults, Compare…, On
          another store… — the command keeps its room */}
      <If when={isAvailable}>
        <IconButton ref={(r) => (moreButton = r)} icon="ellipsis" label={t("common.more")} onPress={moreMenu} testID="run-more" />
      </If>
    </View>
  );

  const filterNote = () => {
    const v = view();
    const f = narrowedTo();
    const w = window();
    const k = kind();
    if (!v || (f === null && !w && k === null)) return null;
    const n = source.total();
    // what narrows them first, then how many of how many
    const of = t("runs.filtered", { n: count(n), of: count(v.operationsTotal) });
    return [k, failedIn() ? t("runs.failedOnly") : null, w ? `+${duration(w[0])} – +${duration(w[1])}` : null, of].filter(Boolean).join(" · ");
  };

  const chip = () => (
    <If when={filterNote()}>
      {(n) => (
        // what narrows the operations, and the press that lets it go
        <Button small variant="outline" icon="x" label={n()} tip={t("runs.clearFilter")} onPress={() => batch(() => (filter.set(null), stretch.set(null), kind.set(null), failedIn.set(null)))} testID="clear-filter" />
      )}
    </If>
  );
  // on the Time pane, the same stretch of time as its one chip: every chart follows it, a press lets it go
  const timeChip = () => (
    <If when={window()}>
      {(w) => <Button small variant="outline" icon="x" label={`+${duration(w()[0])} – +${duration(w()[1])}`} tip={t("timeline.unzoom")} onPress={() => stretch.set(null)} testID="clear-filter" />}
    </If>
  );
  const tabs = () => <Segmented testID="run-pane-mode" value={pane()} items={panes()} onChange={(p) => pane.set(p as Pane)} />;
  // how the program ended, where that is news: stopped, or an exit other than 0 (the run it ran as)
  const exitNote = () => (
    <If when={lastRun()?.path === props.path && (lastRun()!.killed || lastRun()!.exit !== 0) ? lastRun() : null}>
      {(r) => <Notice testID="run-exit" tone="warn" text={r().killed ? t("run.stoppedNote") : t("run.exitNote", { code: `${r().exit ?? "?"}` })} />}
    </If>
  );
  // what the run is (its command, its figures), then what is shown of it:
  // the pane chosen takes all the room under its tabs
  const main = (v: () => RunView) => (
    <View class="min-h-0 flex-1">
      <View class="shrink-0 px-4 pt-4">
        <RunHeader view={v()} actions={actions} onSignal={showSignal}>
          {trialNote(v)}
          {exitNote()}
          {/* what changed since the program's run before, in a sentence, on every pane; Compare opens the
              two side by side */}
          <If when={!v().trial}>
            <SinceRun since={since()} onCompare={() => since()?.previous && compareWith.set(since()!.previous!)} />
          </If>
        </RunHeader>
      </View>
      {/* (what does not fit beside the tabs goes under them, never off the pane) */}
      <View class="shrink-0 flex-row flex-wrap items-center gap-x-3 gap-y-2 border-b border-border px-4 pb-3 pt-3">
        {tabs()}
        <View class="flex-1" />
        {/* what narrows the operations, said where they are; and how they are grouped, its ✕ the way back */}
        <If when={pane() === "operations" || pane() === "output"} else={<If when={pane() === "time"}>{timeChip()}</If>}>
          {chip()}
          <If when={pane() === "operations" && groupBy() !== "none"}>
            <Button small variant="outline" icon="x" label={`${t("split.groupBy")}: ${t(`split.by.${groupBy()}`)}`} tip={t("split.by.none")} onPress={() => groupBy.set("none")} testID="clear-group" />
          </If>
        </If>
      </View>
      <View class="min-h-0 flex-1 px-4 pb-4 pt-3">
        {body(v)}
      </View>
    </View>
  );

  // room for the inspector beside the operations, or it comes in from the edge
  const roomy = () => box().w >= BREAK.inspector;
  const inspecting = () => !!(episode() || record() || diagnosis());
  /** The inspector shows a diagnosis's Apply (the demo's job): the screen's one primary. */
  const applying = () => !episode() && !record() && !!diagnosis() && isDemoJob(view()?.header) && !!headOf(view()!, diagnosis()!)?.fixes?.length;
  const inspectorTitle = () => (record() ? t("inspector.request") : episode() ? t("inspector.operation") : diagnosis() ? t("fix.title") : t("inspector.operation"));
  // the findings a line each, the one chosen in full, so the operations keep the room; a tall window has it for both
  const dense = () => box().h < BREAK.tall;
  // the inspector beside the run where there is room, from the edge where not; on Output (two columns already) from the edge
  const beside = () => roomy() && pane() !== "output";

  const inspector = () => (
    <View testID="inspector" class={`flex-1 bg-inspector ${beside() ? "border-l border-border" : ""}`}>
      <PanelHeader icon={record() ? "radio" : episode() || !diagnosis() ? "layers" : "sparkles"} title={inspectorTitle()} onClose={closeInspector} closeTestID="inspector-close">
        {/* the operation's commands, as its context menu has them: reached by the keyboard too */}
        <If when={!record() && op() !== null}>
          <More testID="operation-more" actions={() => (op() === null ? [] : operationActions(op()!))} />
        </If>
      </PanelHeader>
      <Scroll testID="inspector-scroll" class="flex-1" contentClass="p-4">
        {detail(false)}
      </Scroll>
    </View>
  );

  return (
    <View class="flex-1" testID="run-pane" onLayout={(e) => box.set({ w: Math.round(e.bounds.size.width), h: Math.round(e.bounds.size.height) })}>
      {/* being read (in outline), unreadable (why), or there */}
      <If
        when={view()}
        else={
          <If when={read.reading()} else={<Await loading={false} error={read.error() ?? t("runs.unreadable")} testID="run"><View /></Await>}>
            <RunSkeleton />
          </If>
        }
      >
        {(v) => (
          <If when={compareWith()} else={
            <If
              when={beside()}
              else={
                <View class="flex-1">
                  {main(v)}
                  <SidePanel open={inspecting()} onClose={closeInspector} size="narrow" label={inspectorTitle()} returnFocus={() => opsAnchor}>
                    {inspector()}
                  </SidePanel>
                </View>
              }
            >
              <ResizablePanelGroup axis="horizontal" class="flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("runs.resizeInspector")}>
                <ResizablePanel minSize={PANE.runLeast}>{main(v)}</ResizablePanel>
                <If when={inspecting()}>
                  <ResizablePanel size={inspectorWidth.size()} onResize={inspectorWidth.onResize} minSize={PANE.inspectorLeast} maxSize={PANE.inspectorMost}>
                    {inspector()}
                  </ResizablePanel>
                </If>
              </ResizablePanelGroup>
            </If>
          }>
            {(other) => (
              <View class="flex-1 px-4 py-4">
                <CompareView a={props.path} b={other()} onClose={() => compareWith.set(null)} />
              </View>
            )}
          </If>
        )}
      </If>
      <If when={askAgain()}>
        {(ask) => (
          <ConfirmRunDialog
            ask={ask()}
            endpoint={view()?.header.endpoint}
            onClose={() => askAgain.set(null)}
            onConfirm={() => {
              const yes = ask();
              askAgain.set(null);
              void again(yes);
            }}
          />
        )}
      </If>
      <If when={tryingFaults()}>
        {/* started, the run's History follows it */}
        <TrialDialog path={props.path} onClose={() => tryingFaults.set(false)} onStarted={() => pane.set("runs")} />
      </If>
      <ReplayDialog
        path={props.path}
        open={replaying()}
        onClose={() => replaying.set(false)}
        onDone={(replay) => {
          runsChanged();
          // a run's own window goes to the replay; the main window stays, comparing
          if (props.inWindow) props.onOpen?.(replay);
          compareWith.set(replay);
        }}
      />
    </View>
  );
}

const windowId = (path: string) => `run-${fileName(path)}`;

/** Opens a run in a window of its own, or brings its window forward: one
 * window per run, titled as the run is named everywhere. */
function openRunWindow(path: string) {
  const open = Window.get(windowId(path));
  if (open) {
    open.focus();
    return;
  }
  const me = api.runs.list().find((s) => s.path === path);
  Window.open(
    () => (
      // a pane that fails to draw says so in its window, and can try again
      <View class={`flex-1 ${windowFace()}`}>
        <Recoverable>
          <RunWindow path={path} />
        </Recoverable>
      </View>
    ),
    { id: windowId(path), title: me ? runName(me) : fileName(path), defaultSize: { width: 1100, height: 720 }, restoreBounds: true },
  );
}

/** The list a second later, while a run is live: its row's count from the recording (no folder read each
 * second); the whole list read again once it has ended. */
function withLive(list: RunInfo[]): RunInfo[] {
  const now = api.runs.current();
  if (!now) return api.runs.list();
  return list.map((s) => (s.live && s.path === now.path ? { ...s, requests: now.requests } : s));
}

/** A run in a window of its own: it grows while live, says what it did
 * here (its own toasts and rename), and opens another run in the main
 * window. Its title follows the run's name; it closes when its run
 * is deleted. */
function RunWindow(props: { path: string }) {
  const tick = signal(0);
  const others = signal<RunInfo[]>(api.runs.list());
  const timer = setInterval(() => {
    if (away() || !others().some((s) => s.path === props.path && s.live)) return;
    others.set(withLive(others()));
    tick.set(tick() + 1);
  }, 1000);
  cleanup(() => clearInterval(timer));
  effect(() => {
    runsTick();
    untracked(() => others.set(api.runs.list()));
  });
  // it was there when the window opened: gone from the list, it was deleted
  let seen = false;
  effect(() => {
    const me = others().find((s) => s.path === props.path);
    untracked(() => {
      try {
        const w = Window.get(windowId(props.path));
        if (me) {
          seen = true;
          w?.set({ title: runName(me) });
        } else if (seen) w?.close();
      } catch {
        // no windows here
      }
    });
  });
  return (
    <View class="flex-1 bg-background">
      <RenameDialog window={props.path} />
      <RunPane
        path={props.path}
        others={others}
        tick={tick}
        inWindow
        onOpen={(p) => {
          openRun.set(p);
          go("runs");
        }}
      />
      <Toaster />
    </View>
  );
}

/** What the place was showing, kept while another place is on screen: the run chosen. */
const selected = signal<string | null>(null);

export function Runs() {
  const runs = signal<RunInfo[]>(api.runs.list());
  // the newest run of each program past its baseline: marked on its row (what v21's program list said)
  const past = resource(
    () => [runsTick(), runs().length] as const,
    async () => {
      try {
        return new Set((await api.runs.programs()).filter((p) => p.regressed && p.latest).map((p) => p.latest!));
      } catch {
        return new Set<string>();
      }
    },
  );
  // the run asked for, else the one it showed last, else none: the command bar
  // (a program started from the bar is shown in passing: nobody opened it, so its run is not held as read, and one
  // that sent nothing is not kept)
  let passing: string | null = null;
  untracked(() => {
    const was = selected();
    selected.set(openRun() ?? (was && runs().some((s) => s.path === was) ? was : null));
  });
  // a new run asked for (⌘N, the sidebar's Runs pressed here): the command bar, whatever run was shown
  effect(() => {
    if (newRunAsked()) untracked(() => batch(() => (selected.set(null), newRunAsked.set(false))));
  });
  const choose = (p: string | null) => {
    if (p === passing && p !== null) {
      // chosen now: read from here on
      passing = null;
      reading(p);
    }
    selected.set(p);
  };
  const tick = signal(0);

  // another place asked for a run (a run that ended, the palette, the sidebar)
  effect(() => {
    const p = openRun();
    if (!p) return;
    untracked(() => {
      runs.set(api.runs.list());
      if (p === recordingPath()) passing = p;
      selected.set(p);
      openRun.set(null);
    });
  });
  // the list changed elsewhere
  effect(() => {
    runsTick();
    untracked(() => runs.set(api.runs.list()));
  });
  // a live run grows: follow it
  const timer = setInterval(() => {
    if (!away() && runs().some((s) => s.live)) {
      runs.set(withLive(runs()));
      tick.set(tick() + 1);
    }
  }, 1000);
  cleanup(() => clearInterval(timer));

  const openInWindow = openRunWindow;
  const remove = async (s: RunInfo) => {
    const list = runs();
    const at = list.findIndex((x) => x.path === s.path);
    // the one after it takes its place, before the list changes
    await removeRun(s, () => {
      if (selected() === s.path) selected.set(list[at + 1]?.path ?? list[at - 1]?.path ?? null);
    });
    runs.set(api.runs.list());
  };
  const rowMenu = (s: RunInfo, at?: { x: number; y: number }) => {
    if (!isAvailable) return;
    void popupMenu(
      [
        { label: t("runs.window"), icon: iconSvg("app-window"), onSelect: () => openInWindow(s.path) },
        { label: t("runs.share"), icon: iconSvg("share-2"), onSelect: () => void shareRun(s.path) },
        { label: t("runs.copyPath"), icon: iconSvg("copy"), onSelect: () => copy(s.path) },
        // a run written elsewhere (`-o`) is listed and opened here, never renamed or removed: its file is its writer's
        ...(s.elsewhere
          ? []
          : ([
              { type: "separator" },
              { label: t("runs.rename"), icon: iconSvg("pencil"), onSelect: () => rename(s) },
              { label: t("runs.removeMenu"), icon: iconSvg("trash"), disabled: s.live, onSelect: () => void remove(s) },
            ] as MenuItem[])),
      ],
      at,
    );
  };
  // (the place's name is the row over it, and a run is found by ⌘K: no heading, no field)
  const list = () => (
    <View class="flex-1 pt-2">
      <If when={runs().length > 0} else={<Empty compact text={t("timeline.empty")} />}>
          <View class="min-h-0 flex-1 px-2">
          <RunList
            runs={runs()}
            past={past()}
            selected={selected()}
            onSelect={choose}
            onContextMenu={isAvailable ? rowMenu : undefined}
            actions={(s) => [
              { name: "window", label: t("runs.window"), run: () => openInWindow(s.path) },
              { name: "share", label: t("runs.share"), run: () => void shareRun(s.path) },
              ...(s.elsewhere ? [] : [{ name: "rename", label: t("runs.rename"), run: () => rename(s) }]),
              ...(s.live || s.elsewhere ? [] : [{ name: "delete", label: t("runs.removeMenu"), run: () => void remove(s) }]),
            ]}
          />
          </View>
      </If>
    </View>
  );
  const pane = (p: string) => (
    <RunPane
      path={p}
      passing={p === passing}
      others={runs}
      // only a live run is read again as it grows, and once more as it ends (its ticks count from 1, the ended run's 0)
      tick={() => (runs().some((s) => s.path === p && s.live) ? tick() + 1 : 0)}
      // another run asked for from this one (a trial's, a replay's, a run of the trend): shown
      onOpen={(np) => {
        runs.set(api.runs.list());
        selected.set(np);
      }}
    />
  );

  // a pane per run: another run gets a fresh one (no selection carried over)
  const keyed = () => (
    <Each of={selected() ? [selected()!] : []} key={(p) => p}>
      {(p) => pane(p())}
    </Each>
  );

  // a run removed elsewhere (its own pane's menu, a run that sent nothing not kept): back to the command bar
  // (a trial's run is in no list: it stays while the run it is a trial of names it)
  effect(() => {
    const p = selected();
    if (p && p !== recordingPath() && !runs().some((s) => s.path === p || s.trials?.some((x) => x.run === p))) untracked(() => selected.set(null));
  });

  // the runs are the sidebar's list while this place shows
  cleanup(showInSidebar("runs", list));
  // the run on screen: one that ends here needs no toast
  effect(() => shownRun.set(selected()));
  cleanup(() => shownRun.set(null));

  return (
    <View class="flex-1" testID="screen-runs">
      <RenameDialog />
      {/* none chosen: the command bar; else the run */}
      <If when={selected()} else={<NewRun />}>
        {keyed()}
      </If>
    </View>
  );
}
