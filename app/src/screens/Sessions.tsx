// Sessions: what programs did to their object stores. The list on the left,
// by day; a session reads from its figures and what stands out (each a
// filter) down to its operations — outcome, time, family, place in time —
// and an inspector beside them that goes from an operation to its requests
// to one request in full. When sonde ran the program, its output lines up
// with the operations: choose one and the other lights up. A session also
// shows as a timeline, replays on another endpoint, compares with another
// session, and opens in a window of its own. In a terminal the levels stack.

import { batch, cleanup, computed, effect, LayoutAnimation, reduceMotion, Scroll, signal, untracked, View, type ViewRef } from "nsh";
import { createNavStack, NavStack, ResizablePanel, ResizablePanelGroup, type DataTableRef } from "nsh/base";
import { isAvailable, Menu, type MenuItem, Window } from "nsh/desktop";
import { count, duration, hm } from "../format";
import { t } from "../i18n";
import { analysis } from "../analysis";
import { pagedOperations } from "../paged";
import { api } from "../native";
import { away, chooseBucket, compareOnOpen, copy, crumb, endpoints, go, openSession, paneOnOpen, openSessionFile, reading, report, sessionsChanged, sessionsTick, showInSidebar, startDemo, notify, useEndpoint } from "../store";
import type { LogLine, Observation, SessionInfo, SessionView, Trial } from "../types";
import { CompareView, ReplayDialog } from "../ui/compare";
import { iconSvg } from "../ui/icon";
import { Await, Back, BREAK, Button, Empty, IconButton, More, Notice, Options, PanelHeader, Recoverable, SearchField, Segmented, SessionSkeleton, SideHeader, SidePanel, SIZES, SPLIT_HANDLE, terminal, PANE } from "../ui/kit";
import { Toaster } from "../ui/toast";
import { fileName, removeSession, rename, RenameDialog, sessionName, shareSession } from "../ui/manage";
import { rememberedSize } from "../ui/sizes";
import { linesOf, Observations, OperationDetail, OperationsTable, OutputTable, SessionHeader, SessionList } from "../ui/session";
import { findingMarks, operationsText, storeFixOf } from "../ui/sessionview";
import { episodeTitle } from "../phrases";
import { programOf, ranCommand } from "../ui/sessionview";
import { HistoryPane, ProgramList, scenarioLabel } from "../ui/history";
import { TrialDialog } from "../ui/trial";
import { SinceRun, sinceLastOf, statesOf } from "../ui/fixloop";
import { type GroupBy, GroupByChoice, SplitTable } from "../ui/split";
import { TimePane } from "../ui/profile";
import type { SignalId } from "../signals";
import { SessionShape, SHAPE_STRIP, TICK_ROOM, faultMoments, began } from "../ui/plots";
import { RecordDetail } from "../ui/request";
import { type Range, rangeOnOpen, rangeParam, shownRange } from "../range";
import { running, runAgain } from "../run";

type Pane = "operations" | "aligned" | "time" | "runs";

const program = programOf;

/** Around the findings' card in the pane's body: the body's padding, the findings' heading and the gaps (points). */
const FINDINGS_HEAD = 28 + 36 + 12;
/** The least the operations keep under the findings: their titles and two rows, with the card's border. */
const OPERATIONS_LEAST = 31 + 2 * 30 + 2;
/** How many operations "Copy operations as a table" takes: what a paste into a sheet or a ticket holds well. */
const COPY_ROWS = 20000;
/** The budget a session's program has in its project's sonde.toml (null: none, or no folder to find it from). */
const budgetOf = (path: string) => {
  try {
    return api.sessions.budget(path);
  } catch {
    return null;
  }
};
/** The inspector's width, as the user leaves it. */
const inspectorWidth = rememberedSize("sessions.inspector", PANE.inspector);

/** One session: its figures, its operations (and output), the inspector, and what can be done with it. */

export function SessionPane(props: { path: string; others: () => SessionInfo[]; tick: () => number; onBack?: () => void; onOpen?: (p: string) => void; inWindow?: boolean; /** shown only because it is the newest, not because anyone opened it */ passing?: boolean }) {
  // read here, whatever way led here: an empty run is not tidied away under its reader.
  // The newest session shown on a visit to the place was not opened by anyone: a run
  // the toast said was not kept is not kept by passing it
  if (!props.passing) reading(props.path);
  const op = signal<number | null>(null);
  const rec = signal<number | null>(null);
  const opHighlight = signal<number[]>([]);
  const outHighlight = signal<number[]>([]);
  const filter = signal<number | null>(null);
  // a kind of request chosen (Group by's row, the Time pane's by-kind table): the operations that sent one
  const kind = signal<string | null>(null);
  const compareWith = signal<string | null>(null);
  // run again from here: the new run opens compared with this one
  effect(() => {
    const c = compareOnOpen();
    if (c && c.after === props.path) untracked(() => batch(() => (compareWith.set(c.before), compareOnOpen.set(null))));
  });
  const replaying = signal(false);
  const tryingFaults = signal(false);
  // "Try again what broke", from the Runs pane: the scenarios that did not survive
  const tryingAgain = signal<Trial[] | null>(null);
  // Group by: each operation, or the operations split by a dimension
  const groupBy = signal<GroupBy>("none");
  // a signal pressed in the header: the Time pane, on its chart
  const reveal = signal<{ id: SignalId; n: number } | null>(null);
  const showSignal = (id: SignalId) => batch(() => (reveal.set({ id, n: (reveal()?.n ?? 0) + 1 }), pane.set("time")));
  let opsRef: DataTableRef | undefined;
  let outRef: DataTableRef | undefined;

  const read = analysis(() => props.path, props.tick);
  const view = read.view;
  // the run against its program's run before: its findings' states, what was fixed
  const since = sinceLastOf(() => props.path);
  // a trial's session: the clean run it is judged against, for the Time pane to draw it over
  const clean = analysis(() => view()?.trial?.clean ?? null);
  // the program's budget, for the Time pane's lines: read once per session shown
  const budget = computed(() => budgetOf(props.path));
  // this pane's own size (windowSize() follows whichever window has the focus)
  const box = signal({ w: 1000, h: 760 });
  const lines = computed<LogLine[]>(() => {
    props.tick();
    try {
      return api.sessions.logLines(props.path);
    } catch {
      return [];
    }
  });
  // whether it has any (a boolean: what is drawn does not follow every line that arrives)
  const hasOutput = computed(() => lines().length > 0);
  // a session with output opens lined up with it
  const pane = signal<Pane>(untracked(() => (lines().length && !terminal() ? "aligned" : "operations")));
  // a program opened from the Programs list: its runs
  effect(() => {
    const o = paneOnOpen();
    if (o && o.path === props.path) untracked(() => batch(() => (pane.set(o.pane), filter.set(o.finding ?? null), paneOnOpen.set(null))));
  });
  // a link with a stretch of time: the Time pane, over it
  const linkRange = signal<Range | null>(null);
  effect(() => {
    const o = rangeOnOpen();
    if (o && o.name === fileName(props.path)) untracked(() => batch(() => (linkRange.set(o.range), pane.set("time"), rangeOnOpen.set(null))));
  });
  /** A chart's rows, written as CSV where exports go; said, with the file's path. */
  const exportRows = (name: string, csv: string) => {
    try {
      notify(t("common.exportedTo", { path: api.exportCsv(`${fileName(props.path).replace(/\.sonde(\.gz)?$/, "")}-${name}`, csv) }));
    } catch (e) {
      report(e);
    }
  };
  /** A link to this session, over the stretch its Time pane shows (when one is chosen there). */
  const link = () => {
    const r = pane() === "time" ? (shownRange()[props.path] ?? null) : null;
    const q = r ? rangeParam(r) : "";
    // one of the sessions folder by its name; any other file by its path
    return info() ? `sonde://session/${encodeURIComponent(fileName(props.path))}${q ? `?${q}` : ""}` : `sonde://open?path=${encodeURIComponent(props.path)}${q ? `&${q}` : ""}`;
  };

  if (!props.inWindow) {
    effect(() => crumb.set(program(view()?.header.command) ?? view()?.header.endpoint ?? null));
    cleanup(() => crumb.set(null));
  }

  // its operations: the first page with the view, the rest in pages as the table scrolls,
  // sorted and narrowed where they are kept (a session of a hundred thousand is never whole here)
  const source = pagedOperations(() => props.path, view);
  // the operation chosen, whole (every request's id): read once per choice, and again when the session is
  const episode = computed(() => {
    const i = op();
    return i === null || !view() ? null : source.episode(i);
  });
  // read once per request chosen: the inspector asks for it in several places
  const record = computed(() => {
    const id = rec();
    return id === null ? null : api.sessions.record(props.path, id);
  });
  /** The session's one selection of time: a stretch of its slices (first and last), picked on the
   * Operations strip (a slice) or on the Time pane's timeline (two presses) — the same on both. */
  const stretch = signal<[number, number] | null>(null);
  // a live session's slices change length as it grows: a picked one would point at another time
  let binMs: number | undefined;
  effect(() => {
    const b = view()?.profile?.binMs;
    if (binMs !== undefined && b !== binMs) untracked(() => stretch.set(null));
    binMs = b;
  });
  /** The stretch as the time it covers, from the session's start. */
  const window = (): [number, number] | null => {
    const v = view();
    const z = stretch();
    if (!v || z === null || !v.profile) return null;
    const a = v.profile.rate[z[0]]?.tMs ?? 0;
    return [a, (v.profile.rate[z[1]]?.tMs ?? a) + v.profile.binMs];
  };
  /** The one slice chosen, when the stretch is one. */
  const slice = () => {
    const z = stretch();
    return z && z[0] === z[1] ? z[0] : null;
  };
  /** The operations shown: all, those the chosen finding is about, and those running in the chosen slice —
   * narrowed where they are kept, whatever order the table has them in. */
  effect(() => {
    const f = filter();
    const w = window();
    const k = kind();
    // a finding, a slice or a kind chosen narrows the operations: they show, not their groups
    if (f !== null || w || k !== null) untracked(() => groupBy.set("none"));
    untracked(() => {
      const { finding: _, slice: __, kind: ___, ...kept } = source.query();
      source.ask({ ...kept, ...(f === null ? {} : { finding: f }), ...(w ? { slice: w } : {}), ...(k === null ? {} : { kind: k }) });
    });
  });

  const chooseOperation = (i: number) => {
    // the inspector opening moves the table over, smoothly
    if (op() === null && !reduceMotion()) LayoutAnimation.easeInEaseOut();
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
  const closeInspector = () => {
    if (!reduceMotion()) LayoutAnimation.easeInEaseOut();
    // at once: the inspector goes whole, not its request first and then what held it
    batch(() => {
      op.set(null);
      rec.set(null);
      opHighlight.set([]);
      outHighlight.set([]);
    });
  };

  const openWindow = () => openSessionWindow(props.path);
  /** Another session from here (a trial's, the clean run's, a run of the trend): where this pane's holder shows it. */
  const open = (p: string) => (props.onOpen ? props.onOpen(p) : openSession.set(p));
  /** A trial's session says so under its title, with the way back to the run it is a trial of. */
  const trialNote = (v: () => SessionView) => (
    <If when={v().trial}>
      {(x) => (
        <Notice testID="trial-note" icon="flask-conical" text={t("trial.note", { run: program(v().header.command) ?? x().of, scenario: scenarioLabel(x().scenario) })}>
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
        return api.sessions.records(props.path, e.records).map((r) => r.target);
      } catch {
        return [];
      }
    };
    return [
      { label: t("sessions.copyTitle"), icon: "copy", run: () => copy(episodeTitle(e)) },
      { label: t("sessions.copyKeys"), icon: "file", run: () => copy([...new Set(targets())].join("\n")) },
      ...(hasOutput() ? [{ label: t("sessions.showOutput"), icon: "scroll-text" as const, run: () => pane.set("aligned") }] : []),
    ];
  };
  const operationMenu = (i: number) => {
    if (!view() || !isAvailable) return;
    chooseOperation(i);
    void Menu.popup(operationActions(i).map((a) => ({ label: a.label, icon: iconSvg(a.icon), onSelect: a.run })));
  };
  const compareMenu = () => {
    const others = props.others().filter((s) => s.path !== props.path);
    if (!isAvailable) return;
    // runs of this program first; another program's under its own heading
    const key = (c: string | null | undefined) => (c ?? "").trim().split(/\s+/).join(" ");
    const mine = key(info()?.command);
    const same = others.filter((s) => mine && key(s.command) === mine);
    const rest = others.filter((s) => !same.includes(s));
    const item = (s: SessionInfo) => ({ label: `${program(s.command) ?? s.endpoint} · ${hm(s.created)} · ${count(s.requests)}`, onSelect: () => compareWith.set(s.path) });
    void Menu.popup(
      others.length
        ? [
            ...(same.length ? same.slice(0, 20).map(item) : [{ label: t("compare.noRuns"), disabled: true }]),
            ...(rest.length ? ([{ type: "separator" }, { label: t("compare.otherPrograms"), submenu: rest.slice(0, 20).map(item) }] as MenuItem[]) : []),
          ]
        : [{ label: t("compare.none"), disabled: true }],
    );
  };
  const info = () => props.others().find((s) => s.path === props.path) ?? null;
  const moreMenu = () => {
    if (!isAvailable) return;
    const me = info();
    // where the pane is narrow, the session's other commands are here, by their words (not icons beside the title)
    const narrow = !roomy();
    const tryable = !terminal() && ranCommand(view()?.header.command) && !view()?.trial;
    const extras: MenuItem[] = narrow
      ? [
          ...(tryable ? [{ label: t("trial.try"), icon: iconSvg("flask-conical"), disabled: !!running(), onSelect: () => tryingFaults.set(true) }] : []),
          { label: t("replay.open"), icon: iconSvg("repeat-2"), onSelect: () => replaying.set(true) },
          { label: t("compare.open"), icon: iconSvg("git-compare-arrows"), onSelect: compareMenu },
          { type: "separator" },
        ]
      : [];
    void Menu.popup([
      ...extras,
      { label: t("sessions.window"), icon: iconSvg("app-window"), onSelect: openWindow, disabled: props.inWindow },
      { label: t("sessions.share"), icon: iconSvg("share-2"), onSelect: () => void shareSession(props.path) },
      { label: t("sessions.copyPath"), icon: iconSvg("copy"), onSelect: () => copy(props.path) },
      // a link that opens it in sonde, over the stretch of time the Time pane shows
      { label: pane() === "time" && shownRange()[props.path] ? t("sessions.copyLinkRange") : t("sessions.copyLink"), icon: iconSvg("link"), onSelect: () => copy(link()) },
      // the operations as shown, for a ticket or a spreadsheet
      {
        label: t("sessions.copyTable"),
        icon: iconSvg("copy"),
        // the table is there only on its own pane
        disabled: pane() === "time" || pane() === "runs",
        // in the order shown; figures as numbers in one unit (not "840 ms" beside "2.35 s")
        // (as many as a paste holds well: the rest are said to be left out)
        onSelect: () => {
          const v = view();
          if (v)
            void source
              .first(COPY_ROWS)
              .then(({ rows, total }) => copy(operationsText(v, rows), total > rows.length ? t("sessions.copiedFirst", { n: rows.length, of: total }) : undefined))
              .catch(report);
        },
      },
      ...(me ? ([{ type: "separator" }, { label: t("sessions.rename"), icon: iconSvg("pencil"), onSelect: () => rename(me, props.inWindow ? props.path : null) }, { label: t("sessions.removeMenu"), icon: iconSvg("trash"), disabled: me.live, onSelect: () => void removeSession(me) }] as MenuItem[]) : []),
    ]);
  };

  const detail = (back: boolean) => (
    <If
      when={record()}
      else={
        <If when={episode()} else={<Empty icon="list-filter" text={t("sessions.pickOperation")} />}>
          {(e) => <OperationDetail path={props.path} episode={e()} shared={view()?.sharedNotes ?? []} onRecord={(id) => rec.set(id)} onBack={back ? () => op.set(null) : undefined} />}
        </If>
      }
    >
      {(r) => <RecordDetail path={props.path} record={r()} onBack={() => rec.set(null)} onRecord={chooseRequest} />}
    </If>
  );

  let opsAnchor: ViewRef | undefined;
  // (each part is given the view as an accessor and reads it where it draws: a live session's
  // pane is not rebuilt each second, so a stretch zoomed to or a finding chosen stays)
  const operations = (v: () => SessionView) => (
    <View class="min-h-0 flex-1" ref={(r: ViewRef) => (opsAnchor = r)}>
    <OperationsTable view={v()} source={source} onScrolled={(away) => folded() !== away && folded.set(away)} onSelect={chooseOperation} onContextMenu={isAvailable ? operationMenu : undefined} ref={(r) => ((opsRef = r), showOperation())} highlight={opHighlight()} compact={pane() === "aligned"} follow={props.others().some((x) => x.path === props.path && x.live)} />
    </View>
  );
  const output = () => <OutputTable lines={lines()} onSelect={chooseLine} ref={(r) => (outRef = r)} highlight={outHighlight()} />;

  const v0 = () => view();
  // a run of a program has runs (a trial is no run of its own: its verdict is on the run it is a trial of)
  const hasRuns = () => ranCommand(v0()?.header.command) && !v0()?.trial;
  // the four tabs are there for every session, whatever it holds: one with nothing to show is
  // dimmed and says why, it does not go (and the others do not move)
  const panes = () => [
    { value: "operations", title: t("sessions.operations"), icon: "list-filter" as const },
    { value: "aligned", title: t("sessions.withOutput"), icon: "scroll-text" as const, disabled: !hasOutput(), why: t("sessions.noOutput") },
    { value: "time", title: t("profile.pane"), icon: "chart-pie" as const },
    { value: "runs", title: t("history.pane"), icon: "activity" as const, disabled: !hasRuns(), why: t("sessions.noRuns") },
  ];

  /** A finding whose fix is a tool of Buckets (incomplete uploads): that section, on the bucket and endpoint its requests went to. */
  const openStoreFix = (o: Observation) => {
    const v = view();
    const to = storeFixOf(o);
    if (!v || !to) return;
    try {
      const first = o.operations[0] === undefined ? null : api.sessions.episode(props.path, o.operations[0]);
      const id = first?.records[0];
      const rec = id === undefined ? null : api.sessions.record(props.path, id);
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

  const findings = (v: () => SessionView) => (
    <Observations
      onStore={openStoreFix}
      view={v()}
      dense={dense()}
      // a narrow pane keeps the line for what was found; what it saves is in the finding once chosen
      narrow={!roomy() || inspecting()}
      onExpect={(o) => copy(`expect = [${JSON.stringify(o.identity)}]`, t("expect.copied"))}
      filter={filter()}
      onFilter={(f) => filter.set(f)}
      states={statesOf(since())}
      onOpenCode={(place) => api.sessions.openCode(props.path, place).catch(report)}
      // a short window keeps two findings in view (the rest scroll): the operations need their rows;
      // a finding chosen there gets the room to read what to change
      // (a finding chosen there opens its advice in what the pane's body has left once the operations
      // have their titles and two rows: the advice scrolls within the findings, the table keeps rows)
      // (rounded down to whole findings by the findings themselves: no row is cut through its text)
      maxHeight={box().h < BREAK.short ? (filter() === null ? SIZES.findingsShort : Math.max(SIZES.findingsShortOpen, bodyHeight() - FINDINGS_HEAD - OPERATIONS_LEAST)) : Math.max(SIZES.findingsLeast, Math.round(box().h * (filter() === null ? SIZES.findingsShare : SIZES.findingsShareOpen)))}
    />
  );
  /** When: the session's requests over its time, full width, the findings' numbers where they begin; a press picks a slice. */
  const strip = (v: () => SessionView) => (
    <If when={!terminal() && v().profile && box().h >= BREAK.short}>
      <SessionShape strip axis profile={v().profile} slice={slice()} picked={stretch()} onSlice={(i) => (i === null ? stretch.set(null) : began(v().profile, i, i) && stretch.set([i, i]))} annotate marks={findingMarks(v())} faults={faultMoments(v().profile, v().injected)} injected={v().injected} class={`mt-3 ${TICK_ROOM} ${SHAPE_STRIP} w-full shrink-0`} />
    </If>
  );
  const body = (v: () => SessionView) => {
    if (pane() === "time") return <TimePane onScrolled={(away) => folded() !== away && folded.set(away)} view={v()} groupBy={groupBy() === "none" ? null : (groupBy() as Exclude<GroupBy, "none">)} splitOf={(by) => api.sessions.split(props.path, by)} kind={kind()} onKind={(k) => batch(() => (kind.set(k), k !== null && pane.set("operations")))} stretchOf={(a, b) => api.sessions.stretch(props.path, a, b)} onRequest={chooseRequest} reveal={reveal()} clean={clean.view()} budget={budget()} onExport={exportRows} range={linkRange()} stretch={stretch()} onStretch={(z) => stretch.set(z)} onRange={(r) => shownRange.set({ ...shownRange(), [props.path]: r })} />;
    if (pane() === "runs" && !v().trial) return <HistoryPane path={props.path} onOpen={open} onTryAgain={(trials) => tryingAgain.set(trials)} />;
    if (pane() === "aligned" && hasOutput()) {
      if (terminal()) return output();
      return (
        <View class="min-h-0 flex-1 gap-3">
          {findings(v)}
          <ResizablePanelGroup axis="horizontal" class="min-h-0 flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("sessions.resizeOutput")}>
            <ResizablePanel minSize={PANE.outputLeast}>
              <View class="flex-1 pr-2">{output()}</View>
            </ResizablePanel>
            <ResizablePanel minSize={PANE.besideLeast}>
              <View class="flex-1 pl-2">{operations(v)}</View>
            </ResizablePanel>
          </ResizablePanelGroup>
        </View>
      );
    }
    if (terminal()) return operations(v);
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

  const actions = () => (
    <View class="flex-row items-center gap-2">
      {/* the program again, after a change: it opens compared with this run */}
      {/* the built-in demo is not saved: running a demo session again starts it */}
      <If when={ranCommand(view()?.header.command) && !view()?.trial && (view()!.header.endpoint === "demo" || endpoints().some((e) => e.id === view()!.header.endpoint))}>
        <Button
          small
          variant="primary"
          icon="play"
          label={t("sessions.runAgain")}
          onPress={async () => {
            const h = view()!.header;
            if (!endpoints().some((e) => e.id === h.endpoint) && !(await startDemo())) return;
            runAgain(props.path, h.command!, h.endpoint, h.cwd);
          }}
          disabled={!!running()}
          testID="run-again"
        />
      </If>
      {/* where the pane is narrow, the others are behind ⋯ by their words (no row of unlabelled icons): the
          command keeps its room. The same command under faults is asked first what will run (a trial is no run of its own) */}
      <If when={!terminal() && roomy() && ranCommand(view()?.header.command) && !view()?.trial}>
        <Button small variant="outline" icon="flask-conical" label={t("trial.try")} onPress={() => tryingFaults.set(true)} disabled={!!running()} testID="session-try" />
      </If>
      <If when={roomy() || terminal() || !isAvailable}>
        <Button small variant="outline" icon="repeat-2" label={t("replay.open")} onPress={() => replaying.set(true)} testID="replay-open" />
      </If>
      <If when={isAvailable}>
        <If when={roomy()}>
          <Button small variant="outline" icon="git-compare-arrows" label={t("compare.open")} onPress={compareMenu} testID="compare-open" />
        </If>
        <IconButton icon="ellipsis" label={t("common.more")} onPress={moreMenu} testID="session-more" />
      </If>
    </View>
  );

  const filterNote = () => {
    const v = view();
    const f = filter();
    const w = window();
    const k = kind();
    if (!v || (f === null && !w && k === null)) return null;
    const n = source.total();
    // what narrows them first, then how many of how many
    const of = t("sessions.filtered", { n: count(n), of: count(v.operationsTotal) });
    return [k, w ? `+${duration(w[0])} – +${duration(w[1])}` : null, of].filter(Boolean).join(" · ");
  };

  const chip = () => (
    <If when={filterNote()}>
      {(n) => (
        // what narrows the operations, and the press that lets it go
        <Button small variant="outline" icon="x" label={n()} tip={t("sessions.clearFilter")} onPress={() => batch(() => (filter.set(null), stretch.set(null), kind.set(null)))} testID="clear-filter" />
      )}
    </If>
  );
  // on the Time pane, the same stretch of time as its one chip: every chart follows it, a press lets it go
  const timeChip = () => (
    <If when={window()}>
      {(w) => <Button small variant="outline" icon="x" label={`+${duration(w()[0])} – +${duration(w()[1])}`} tip={t("timeline.unzoom")} onPress={() => stretch.set(null)} testID="clear-filter" />}
    </If>
  );
  const tabs = () => <Segmented testID="session-pane-mode" value={pane()} items={panes()} onChange={(p) => pane.set(p as Pane)} />;
  // what the session is (its command, its figures), then what is shown of it:
  // the pane chosen takes all the room under its tabs
  const main = (v: () => SessionView) => (
    <If
      when={!terminal()}
      else={
        <View class="min-h-0 flex-1">
          <View class="gap-1">
            <If when={props.onBack}>
              <Back label={t("timeline.sessions")} onPress={() => props.onBack?.()} testID="sessions-back" />
            </If>
            <SessionHeader view={v()}>{trialNote(v)}</SessionHeader>
            {findings(v)}
            {actions()}
          </View>
          <View class="flex-row items-center gap-2">
            {tabs()}
            <View class="flex-1" />
            {chip()}
          </View>
          <View class="min-h-0 flex-1">{body(v)}</View>
        </View>
      }
    >
      <View class="min-h-0 flex-1">
        <View class="shrink-0 px-4 pt-4">
          <SessionHeader view={v()} actions={actions()} onSignal={showSignal} short={box().h < BREAK.short} folded={folded()}>
            {trialNote(v)}
            {/* what changed since the program's run before: what was fixed, with what it saved */}
            <If when={!v().trial}>
              <SinceRun since={since()} max={box().h < BREAK.short ? 0 : 2} onCompare={() => since()?.previous && compareWith.set(since()!.previous!)} />
            </If>
          </SessionHeader>
        </View>
        {/* (what does not fit beside the tabs goes under them, never off the pane) */}
        <View class="shrink-0 flex-row flex-wrap items-center gap-x-3 gap-y-2 border-b border-border px-4 pb-3 pt-3">
          {tabs()}
          <View class="flex-1" />
          {/* what narrows the operations, said where they are; and the one way to group them */}
          <If when={pane() === "operations" || pane() === "aligned"} else={<If when={pane() === "time"}>{timeChip()}</If>}>{chip()}</If>
          <If when={pane() === "operations"}>
            <GroupByChoice value={groupBy()} onChange={(g) => batch(() => (g !== "none" && (filter.set(null), stretch.set(null), kind.set(null)), groupBy.set(g)))} />
          </If>
        </View>
        <View class="min-h-0 flex-1 px-4 pb-4 pt-3" onLayout={({ bounds }) => bodyHeight.set(Math.round(bounds.size.height))}>
          {body(v)}
        </View>
      </View>
    </If>
  );

  // the room under the tabs, as it is laid out: what the findings may take of it
  const bodyHeight = signal(0);
  // the pane under the tabs scrolled from its top: the header folds to one line and gives it the room
  // (another pane, or the same back at its top, opens it again)
  const folded = signal(false);
  effect(() => {
    pane();
    untracked(() => folded.set(false));
  });
  // room for the inspector beside the operations, or it comes in from the edge
  const roomy = () => box().w >= BREAK.inspector;
  const inspecting = () => !!(episode() || record());
  // the findings a line each, the one chosen in full, so the operations keep the room; a tall window has it for both
  const dense = () => pane() !== "operations" || box().h < BREAK.tall;

  const inspector = () => (
    <View testID="inspector" class={`flex-1 bg-inspector ${roomy() ? "border-l border-border" : ""}`}>
      <PanelHeader icon={record() ? "radio" : "layers"} title={record() ? t("inspector.request") : t("inspector.operation")} onClose={closeInspector} closeTestID="inspector-close">
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
    <View class="flex-1" testID="session-pane" onLayout={(e) => box.set({ w: Math.round(e.bounds.size.width), h: Math.round(e.bounds.size.height) })}>
      {/* being read (in outline), unreadable (why), or there */}
      <If
        when={view()}
        else={
          <If when={read.reading()} else={<Await loading={false} error={read.error() ?? t("sessions.unreadable")} testID="session"><View /></Await>}>
            <SessionSkeleton />
          </If>
        }
      >
        {(v) => (
          <If when={compareWith()} else={
            <If
              when={terminal()}
              else={
                <If
                  when={roomy()}
                  else={
                    <View class="flex-1">
                      {main(v)}
                      <SidePanel open={inspecting()} onClose={closeInspector} size="narrow" returnFocus={() => opsAnchor}>
                        {inspector()}
                      </SidePanel>
                    </View>
                  }
                >
                  <ResizablePanelGroup axis="horizontal" class="flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("sessions.resizeInspector")}>
                    <ResizablePanel minSize={PANE.sessionLeast}>{main(v)}</ResizablePanel>
                    <If when={inspecting()}>
                      <ResizablePanel size={inspectorWidth.size()} onResize={inspectorWidth.onResize} minSize={PANE.inspectorLeast} maxSize={PANE.inspectorMost}>
                        {inspector()}
                      </ResizablePanel>
                    </If>
                  </ResizablePanelGroup>
                </If>
              }
            >
              <If when={episode() || record()} else={main(v)}>
                <Scroll class="flex-1">{detail(true)}</Scroll>
              </If>
            </If>
          }>
            {(other) => (
              <View class="flex-1 px-4 py-4 terminal:p-0">
                <CompareView a={props.path} b={other()} onClose={() => compareWith.set(null)} />
              </View>
            )}
          </If>
        )}
      </If>
      <If when={tryingFaults()}>
        <TrialDialog path={props.path} onClose={() => tryingFaults.set(false)} onDone={() => pane.set("runs")} onOpen={open} />
      </If>
      <If when={tryingAgain()}>
        {(again) => (
          // a fresh run, then what did not survive: the fresh run opens on its Runs pane
          <TrialDialog path={props.path} again={again()} onClose={() => tryingAgain.set(null)} onOpen={open} onAgain={(fresh) => batch(() => (paneOnOpen.set({ path: fresh, pane: "runs" }), open(fresh)))} />
        )}
      </If>
      <ReplayDialog
        path={props.path}
        open={replaying()}
        onClose={() => replaying.set(false)}
        onDone={(replay) => {
          sessionsChanged();
          // a terminal and a session's own window go to the replay; the main window stays, comparing
          if (terminal() || props.inWindow) props.onOpen?.(replay);
          compareWith.set(replay);
        }}
      />
    </View>
  );
}

const windowId = (path: string) => `session-${fileName(path)}`;

/** Opens a session in a window of its own, or brings its window forward: one
 * window per session, titled as the session is named everywhere. */
function openSessionWindow(path: string) {
  const open = Window.get(windowId(path));
  if (open) {
    open.show();
    open.focus();
    return;
  }
  const me = api.sessions.list().find((s) => s.path === path);
  Window.open(
    () => (
      // a pane that fails to draw says so in its window, and can try again
      <Recoverable>
        <SessionWindow path={path} />
      </Recoverable>
    ),
    { id: windowId(path), title: me ? sessionName(me) : fileName(path), defaultSize: { width: 1100, height: 720 }, restoreBounds: true },
  );
}

/** A session in a window of its own: it grows while live, says what it did
 * here (its own toasts and rename), and opens another run in the main
 * window. Its title follows the session's name; it closes when its session
 * is deleted. */
function SessionWindow(props: { path: string }) {
  const tick = signal(0);
  const others = signal<SessionInfo[]>(api.sessions.list());
  const timer = setInterval(() => {
    if (away() || !others().some((s) => s.path === props.path && s.live)) return;
    others.set(api.sessions.list());
    tick.set(tick() + 1);
  }, 1000);
  cleanup(() => clearInterval(timer));
  effect(() => {
    sessionsTick();
    untracked(() => others.set(api.sessions.list()));
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
          w?.set({ title: sessionName(me) });
        } else if (seen) w?.close();
      } catch {
        // no windows here
      }
    });
  });
  return (
    <View class="flex-1 bg-background">
      <RenameDialog window={props.path} />
      <SessionPane
        path={props.path}
        others={others}
        tick={tick}
        inWindow
        onOpen={(p) => {
          openSession.set(p);
          go("sessions");
        }}
      />
      <Toaster />
    </View>
  );
}

/** What the place was showing, kept while another place is on screen: the
 * session chosen, the search, and whether the list is of runs or programs. */
const selected = signal<string | null>(null);
const query = signal("");
const byProgram = signal(false);

export function Sessions() {
  const sessions = signal<SessionInfo[]>(api.sessions.list());
  // a terminal starts on the list; a window on the session it showed last, or the newest
  // (the newest is shown in passing: nobody opened it, so it is not held as read)
  let passing: string | null = null;
  untracked(() => {
    const was = selected();
    const asked = openSession() ?? (terminal() ? null : was && sessions().some((s) => s.path === was) ? was : null);
    passing = asked || terminal() ? null : (sessions()[0]?.path ?? null);
    selected.set(asked ?? passing);
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

  // another place asked for a session (a run that ended, the palette, the sidebar)
  effect(() => {
    const p = openSession();
    if (!p) return;
    untracked(() => {
      sessions.set(api.sessions.list());
      selected.set(p);
      openSession.set(null);
    });
  });
  // the list changed elsewhere
  effect(() => {
    sessionsTick();
    untracked(() => sessions.set(api.sessions.list()));
  });
  // a live session grows: follow it
  const timer = setInterval(() => {
    if (!away() && sessions().some((s) => s.live)) {
      sessions.set(api.sessions.list());
      tick.set(tick() + 1);
    }
  }, 1000);
  cleanup(() => clearInterval(timer));

  const example = () => {
    try {
      const p = api.sessions.example();
      sessions.set(api.sessions.list());
      selected.set(p);
      sessionsChanged();
    } catch (e) {
      report(e);
    }
  };
  const shown = () => {
    const q = terminal() ? "" : query().trim().toLowerCase();
    if (!q) return sessions();
    return sessions().filter((s) => [s.title, s.command, s.endpoint, s.name].some((f) => f?.toLowerCase().includes(q)));
  };
  const openInWindow = openSessionWindow;
  const remove = async (s: SessionInfo) => {
    const list = shown();
    const at = list.findIndex((x) => x.path === s.path);
    // the one after it takes its place, before the list changes
    await removeSession(s, () => {
      if (selected() === s.path) selected.set(list[at + 1]?.path ?? list[at - 1]?.path ?? null);
    });
    sessions.set(api.sessions.list());
  };
  const rowMenu = (s: SessionInfo, at?: { x: number; y: number }) => {
    if (!isAvailable) return;
    void Menu.popup(
      [
        { label: t("sessions.window"), icon: iconSvg("app-window"), onSelect: () => openInWindow(s.path) },
        { label: t("sessions.share"), icon: iconSvg("share-2"), onSelect: () => void shareSession(s.path) },
        { label: t("sessions.copyPath"), icon: iconSvg("copy"), onSelect: () => copy(s.path) },
        { type: "separator" },
        { label: t("sessions.rename"), icon: iconSvg("pencil"), onSelect: () => rename(s) },
        { label: t("sessions.removeMenu"), icon: iconSvg("trash"), disabled: s.live, onSelect: () => void remove(s) },
      ],
      at,
    );
  };
  const list = () => (
    <View class="flex-1 terminal:gap-1">
      <If when={!terminal()}>
        <SideHeader title={t("nav.sessions")}>
          <IconButton icon="folder-open" label={t("sessions.openFile")} shortcut="mod+o" onPress={() => void openSessionFile()} testID="open-file" />
          <IconButton icon="scroll-text" label={t("timeline.example")} onPress={example} testID="open-example" />
        </SideHeader>
      </If>
      <View class={terminal() ? "flex-row items-center gap-2" : "gap-2 px-2 pb-2"}>
        {/* a run at a time, or a program over its runs */}
        <View class="flex-row">
        <Options
          testID="sessions-by"
          value={byProgram() ? "programs" : "runs"}
          items={[
            { value: "runs", title: t("sessions.byRun") },
            { value: "programs", title: t("sessions.byProgram") },
          ]}
          onChange={(v) => byProgram.set(v === "programs")}
        />
        </View>
        <If when={terminal()}>
          <IconButton icon="scroll-text" label={t("timeline.example")} onPress={example} testID="open-example" />
        </If>
        {/* always there, over runs and over programs: a field that came and went moved the list */}
        <If when={!terminal()}>
          <SearchField small testID="session-search" value={query()} placeholder={t("sessions.search")} onChange={(v) => query.set(v)} />
        </If>
      </View>
      <If when={!byProgram()} else={
        // (inset as the runs are: a program's row lines up with a run's)
        <View class={terminal() ? "flex-1" : "min-h-0 flex-1 px-2"}>
        <ProgramList
          query={terminal() ? "" : query()}
          onOpen={(p) => {
            batch(() => {
              paneOnOpen.set({ path: p, pane: "runs" });
              selected.set(p);
            });
          }}
        />
        </View>
      }>
      <If when={sessions().length > 0} else={<Empty compact text={t("timeline.empty")} />}>
        <If when={shown().length > 0} else={<Empty compact text={t("sessions.noMatch")} />}>
          <View class={terminal() ? "flex-1" : "min-h-0 flex-1 px-2"}>
          <SessionList
            sessions={shown()}
            selected={selected()}
            onSelect={choose}
            onContextMenu={isAvailable ? rowMenu : undefined}
            actions={(s) => [
              { name: "window", label: t("sessions.window"), run: () => openInWindow(s.path) },
              { name: "share", label: t("sessions.share"), run: () => void shareSession(s.path) },
              { name: "rename", label: t("sessions.rename"), run: () => rename(s) },
              ...(s.live ? [] : [{ name: "delete", label: t("sessions.removeMenu"), run: () => void remove(s) }]),
            ]}
          />
          </View>
        </If>
      </If>
      </If>
    </View>
  );
  const pane = (p: string) => (
    <SessionPane
      path={p}
      passing={p === passing}
      others={sessions}
      // only a live session is read again as it grows
      tick={() => (sessions().some((s) => s.path === p && s.live) ? tick() : 0)}
      onBack={terminal() ? () => selected.set(null) : undefined}
      // another session asked for from this one (a trial's, a replay's, a run of the trend): shown
      onOpen={(np) => {
        sessions.set(api.sessions.list());
        selected.set(np);
      }}
    />
  );

  // a pane per session: another session gets a fresh one (no selection carried over)
  // a terminal's pages: the list, and the session chosen over it
  const stack = createNavStack<string | null>([null]);
  effect(() => {
    const p = selected();
    if (terminal()) untracked(() => stack.setRoutes(p ? [null, p] : [null]));
  });
  const keyed = () => (
    <Each of={selected() ? [selected()!] : []} key={(p) => p}>
      {(p) => pane(p())}
    </Each>
  );

  const nothing = () => (
    <Empty icon="list-clock" title={t("sessions.noneTitle")} text={t("sessions.noneText")} testID="sessions-empty">
      <Button variant="outline" icon="play" label={t("sessions.runOne")} onPress={() => go("run")} />
      <Button variant="outline" icon="scroll-text" label={t("timeline.example")} onPress={example} testID={terminal() ? "open-example" : undefined} />
      <If when={!terminal()}>
        <Button variant="ghost" icon="folder-open" label={t("sessions.openFile")} onPress={() => void openSessionFile()} />
      </If>
    </Empty>
  );

  // a session removed elsewhere (its own pane's menu): leave it
  // (a trial's session is in no list: it stays while the run it is a trial of names it)
  effect(() => {
    const p = selected();
    if (p && !sessions().some((s) => s.path === p || s.trials?.some((x) => x.session === p))) untracked(() => selected.set(sessions()[0]?.path ?? null));
  });

  // the sessions are the sidebar's list while this place shows
  // (with none chosen the list is all there is to act on: a hidden sidebar comes back)
  if (!terminal()) cleanup(showInSidebar("sessions", list, () => sessions().length > 0 && !selected()));

  return (
    <View class="flex-1" testID="screen-sessions">
      <RenameDialog />
      <If
        when={terminal()}
        else={
          <If when={sessions().length > 0} else={nothing()}>
            <If when={selected()} else={<Empty icon="list-clock" text={t("timeline.pick")} />}>
              {keyed()}
            </If>
          </If>
        }
      >
        {/* one level at a time: sessions › session › operation › request; the
            list stays mounted under the session, so going back finds it as it was */}
        <NavStack testID="sessions-stack" state={stack} class="flex-1 p-1">
          {(route) => (route === null ? list() : pane(route))}
        </NavStack>
      </If>
    </View>
  );
}
