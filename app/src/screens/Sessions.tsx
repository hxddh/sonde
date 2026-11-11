// Sessions: what programs did to their object stores. The list on the left,
// by day; a session reads from its figures and what stands out (each a
// filter) down to its operations — outcome, time, family, place in time —
// and an inspector beside them that goes from an operation to its requests
// to one request in full. When sonde ran the program, its output lines up
// with the operations: choose one and the other lights up. A session also
// shows as a timeline, replays on another endpoint, compares with another
// session, and opens in a window of its own. In a terminal the levels stack.

import { batch, cleanup, computed, effect, LayoutAnimation, reduceMotion, Scroll, signal, Text, untracked, View, type ViewRef } from "nsh";
import { createNavStack, NavStack, ResizablePanel, ResizablePanelGroup, type DataTableRef } from "nsh/base";
import { isAvailable, Menu, type MenuItem, Window } from "nsh/desktop";
import { count, duration, hm } from "../format";
import { t } from "../i18n";
import { analysis } from "../analysis";
import { api } from "../native";
import { away, compareOnOpen, copy, crumb, endpoints, go, openSession, paneOnOpen, openSessionFile, reading, report, sessionsChanged, sessionsTick, showInSidebar, startDemo } from "../store";
import type { LogLine, SessionInfo, SessionView } from "../types";
import { CompareView, ReplayDialog } from "../ui/compare";
import { iconSvg } from "../ui/icon";
import { Await, Back, BREAK, Button, Empty, IconButton, More, Options, PanelHeader, Recoverable, SearchField, Segmented, SessionSkeleton, SideHeader, SidePanel, SPLIT_HANDLE, terminal } from "../ui/kit";
import { Toaster } from "../ui/toast";
import { fileName, removeSession, rename, RenameDialog, sessionName, shareSession } from "../ui/manage";
import { rememberedSize } from "../ui/sizes";
import { linesOf, Observations, OperationDetail, OperationsTable, OutputTable, operationsAt, SessionHeader, SessionList } from "../ui/session";
import { findingMarks, operationsText } from "../ui/sessionview";
import { episodeTitle } from "../phrases";
import { programOf, ranCommand } from "../ui/sessionview";
import { HistoryPane, ProgramList } from "../ui/history";
import { TimePane } from "../ui/profile";
import { SessionShape } from "../ui/plots";
import { RecordDetail } from "./Requests";
import { running, runAgain } from "../run";

type Pane = "operations" | "aligned" | "time" | "runs";

const program = programOf;

/** The inspector's width, as the user leaves it. */
const inspectorWidth = rememberedSize("sessions.inspector", 420);

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
  const compareWith = signal<string | null>(null);
  // run again from here: the new run opens compared with this one
  effect(() => {
    const c = compareOnOpen();
    if (c && c.after === props.path) untracked(() => batch(() => (compareWith.set(c.before), compareOnOpen.set(null))));
  });
  const replaying = signal(false);
  let opsRef: DataTableRef | undefined;
  let outRef: DataTableRef | undefined;

  const read = analysis(() => props.path, props.tick);
  const view = read.view;
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
    if (o && o.path === props.path) untracked(() => batch(() => (pane.set(o.pane), paneOnOpen.set(null))));
  });

  if (!props.inWindow) {
    effect(() => crumb.set(program(view()?.header.command) ?? view()?.header.endpoint ?? null));
    cleanup(() => crumb.set(null));
  }

  const episode = () => {
    const i = op();
    const v = view();
    return i === null || !v ? null : (v.episodes[i] ?? null);
  };
  const record = () => {
    const id = rec();
    return id === null ? null : api.sessions.record(props.path, id);
  };
  /** A slice of the session's shape: the time it covers, from the session's start. */
  const slice = signal<number | null>(null);
  // a live session's slices change length as it grows: a picked one would point at another time
  let binMs: number | undefined;
  effect(() => {
    const b = view()?.profile?.binMs;
    if (binMs !== undefined && b !== binMs) untracked(() => slice.set(null));
    binMs = b;
  });
  const window = (): [number, number] | null => {
    const v = view();
    const i = slice();
    if (!v || i === null || !v.profile) return null;
    const a = v.profile.rate[i]?.tMs ?? 0;
    return [a, a + v.profile.binMs];
  };
  /** The operations shown: all, those the chosen finding is about, and those running in the chosen slice. */
  const rows = () => {
    const v = view();
    if (!v) return undefined;
    const f = filter();
    const w = window();
    if (f === null && !w) return undefined;
    let list = f === null ? v.episodes.map((_, i) => i) : (v.observations[f]?.episodes ?? []);
    if (w) {
      // from the first request, as the shape counts
      const start = v.episodes.reduce((m, e) => Math.min(m, new Date(e.start).getTime()), Infinity);
      list = list.filter((i) => {
        const e = v.episodes[i];
        return new Date(e.start).getTime() - start < w[1] && new Date(e.end).getTime() - start >= w[0];
      });
    }
    return list;
  };

  const chooseOperation = (i: number) => {
    // the inspector opening moves the table over, smoothly
    if (op() === null && !reduceMotion()) LayoutAnimation.easeInEaseOut();
    op.set(i);
    rec.set(null);
    opHighlight.set([i]);
    const v = view();
    if (!v) return;
    const ls = linesOf(lines(), v.episodes[i]);
    outHighlight.set(ls);
    if (ls.length) outRef?.scrollToRow(ls[0]);
  };
  const chooseLine = (i: number) => {
    const v = view();
    const line = lines()[i];
    outHighlight.set([i]);
    if (!v || !line) return;
    const ops = operationsAt(v.episodes, line);
    opHighlight.set(ops);
    if (ops.length) {
      op.set(ops[0]);
      rec.set(null);
      // the row as the table shows it: sorted by a column, it is not the operation's index
      const at = (opsOrder?.() ?? rows() ?? v.episodes.map((_, i) => i)).indexOf(ops[0]);
      if (at >= 0) opsRef?.select({ row: at });
    }
  };
  const closeInspector = () => {
    if (!reduceMotion()) LayoutAnimation.easeInEaseOut();
    op.set(null);
    rec.set(null);
    opHighlight.set([]);
    outHighlight.set([]);
  };

  const openWindow = () => openSessionWindow(props.path);

  /** The operation's commands: native menus where there are any. */
  const operationActions = (i: number): { label: string; icon: "copy" | "file" | "scroll-text"; run: () => void }[] => {
    const e = view()?.episodes[i];
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
    if (!view()?.episodes[i] || !isAvailable) return;
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
    void Menu.popup([
      { label: t("sessions.window"), icon: iconSvg("app-window"), onSelect: openWindow, disabled: props.inWindow },
      { label: t("sessions.share"), icon: iconSvg("share-2"), onSelect: () => void shareSession(props.path) },
      { label: t("sessions.copyPath"), icon: iconSvg("copy"), onSelect: () => copy(props.path) },
      // the operations as shown, for a ticket or a spreadsheet
      {
        label: t("sessions.copyTable"),
        icon: iconSvg("copy"),
        // the table is there only on its own pane
        disabled: pane() === "time" || pane() === "runs",
        // in the order shown; figures as numbers in one unit (not "840 ms" beside "2.35 s")
        onSelect: () => {
          const v = view();
          if (v) copy(operationsText(v, opsOrder?.() ?? rows() ?? v.episodes.map((_, i) => i)));
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
      {(r) => <RecordDetail record={r()} onBack={() => rec.set(null)} copyAs={(kind) => api.sessions.copy(props.path, r().id, kind)} />}
    </If>
  );

  let opsAnchor: ViewRef | undefined;
  let opsOrder: (() => number[]) | undefined;
  // (each part is given the view as an accessor and reads it where it draws: a live session's
  // pane is not rebuilt each second, so a stretch zoomed to or a finding chosen stays)
  const operations = (v: () => SessionView) => (
    <View class="min-h-0 flex-1" ref={(r: ViewRef) => (opsAnchor = r)}>
    <OperationsTable view={v()} rows={rows()} order={(o) => (opsOrder = o)} onSelect={chooseOperation} onContextMenu={isAvailable ? operationMenu : undefined} ref={(r) => (opsRef = r)} highlight={opHighlight()} compact={pane() === "aligned"} />
    </View>
  );
  const output = () => <OutputTable lines={lines()} onSelect={chooseLine} ref={(r) => (outRef = r)} highlight={outHighlight()} />;

  const v0 = () => view();
  const panes = () => [
    { value: "operations", title: t("sessions.operations"), icon: "list-filter" as const },
    ...(hasOutput() ? [{ value: "aligned", title: t("sessions.withOutput"), icon: "scroll-text" as const }] : []),
    { value: "time", title: t("profile.pane"), icon: "chart-pie" as const },
    // a run of a program: its runs
    ...(ranCommand(v0()?.header.command) ? [{ value: "runs", title: t("history.pane"), icon: "activity" as const }] : []),
  ];

  const findings = (v: () => SessionView) => (
    <Observations
      view={v()}
      dense={dense()}
      // a narrow pane keeps the line for what was found; what it saves is in the finding once chosen
      narrow={!roomy()}
      onExpect={(o) => copy(`expect = [${JSON.stringify(o.identity)}]`, t("expect.copied"))}
      filter={filter()}
      onFilter={(f) => filter.set(f)}
      onOpenCode={(place) => api.sessions.openCode(props.path, place).catch(report)}
      // a short window keeps two findings in view (the rest scroll): the operations need their rows;
      // a finding chosen there gets the room to read what to change
      maxHeight={box().h < BREAK.short ? (filter() === null ? 68 : Math.max(160, Math.round(box().h * 0.4))) : Math.max(104, Math.round(box().h * 0.3))}
    />
  );
  /** When: the session's requests over its time, full width, the findings' numbers where they begin; a press picks a slice. */
  const strip = (v: () => SessionView) => (
    <If when={!terminal() && v().profile && box().h >= BREAK.short}>
      <SessionShape strip profile={v().profile} slice={slice()} onSlice={(i) => slice.set(i)} annotate marks={findingMarks(v())} class="mt-3 h-11 w-full shrink-0" />
    </If>
  );
  const body = (v: () => SessionView) => {
    if (pane() === "time") return <TimePane view={v()} onRequest={(id) => rec.set(id)} />;
    if (pane() === "runs") return <HistoryPane path={props.path} onOpen={(p) => (props.onOpen ? props.onOpen(p) : openSession.set(p))} />;
    if (pane() === "aligned" && hasOutput()) {
      if (terminal()) return output();
      return (
        <View class="min-h-0 flex-1 gap-3">
          {findings(v)}
          <ResizablePanelGroup axis="horizontal" class="min-h-0 flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("sessions.resizeOutput")}>
            <ResizablePanel minSize={240}>
              <View class="flex-1 pr-2">{output()}</View>
            </ResizablePanel>
            <ResizablePanel minSize={320}>
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
        {operations(v)}
      </View>
    );
  };

  const actions = () => (
    <View class="flex-row items-center gap-2">
      {/* the program again, after a change: it opens compared with this run */}
      {/* the built-in demo is not saved: running a demo session again starts it */}
      <If when={ranCommand(view()?.header.command) && (view()!.header.endpoint === "demo" || endpoints().some((e) => e.id === view()!.header.endpoint))}>
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
      <Button small variant="outline" icon="repeat-2" label={t("replay.open")} onPress={() => replaying.set(true)} testID="replay-open" />
      <If when={isAvailable}>
        <Button small variant="outline" icon="git-compare-arrows" label={t("compare.open")} onPress={compareMenu} testID="compare-open" />
        <IconButton icon="ellipsis" label={t("common.more")} onPress={moreMenu} testID="session-more" />
      </If>
    </View>
  );

  const filterNote = () => {
    const v = view();
    const f = filter();
    const w = window();
    if (!v || (f === null && !w)) return null;
    const n = rows()?.length ?? 0;
    if (w) return t("profile.sliceFilter", { a: duration(w[0]), b: duration(w[1]), n, of: v.episodes.length });
    return t("sessions.filtered", { n, of: v.episodes.length });
  };

  const chip = () => (
    <If when={filterNote()}>
      {(n) => (
        // what narrows the operations, and the press that lets it go
        <Button small variant="outline" icon="x" label={n()} tip={t("sessions.clearFilter")} onPress={() => batch(() => (filter.set(null), slice.set(null)))} testID="clear-filter" />
      )}
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
            <SessionHeader view={v()} />
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
          <SessionHeader view={v()} actions={actions()} />
        </View>
        <View class="shrink-0 flex-row items-center gap-3 border-b border-border px-4 pb-3 pt-3">
          {tabs()}
          <View class="flex-1" />
          {/* what narrows the operations, said where they are */}
          <If when={pane() === "operations" || pane() === "aligned"}>{chip()}</If>
        </View>
        <View class="min-h-0 flex-1 px-4 pb-4 pt-3">{body(v)}</View>
      </View>
    </If>
  );

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
      <Scroll class="flex-1" contentClass="p-4">
        {detail(false)}
      </Scroll>
    </View>
  );

  return (
    <View class="flex-1" testID="session-pane" onLayout={(e) => box.set({ w: Math.round(e.bounds.size.width), h: Math.round(e.bounds.size.height) })}>
      {/* being read (in outline), unreadable (why), or there */}
      <If when={view()} else={read.reading() ? <SessionSkeleton /> : <Await loading={false} error={read.error() ?? t("sessions.unreadable")} testID="session"><View /></Await>}>
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
                    <ResizablePanel minSize={460}>{main(v)}</ResizablePanel>
                    <If when={inspecting()}>
                      <ResizablePanel size={inspectorWidth.size()} onResize={inspectorWidth.onResize} minSize={320} maxSize={680}>
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
              <View class={terminal() ? "flex-1" : "flex-1 px-4 py-4"}>
                <CompareView a={props.path} b={other()} onClose={() => compareWith.set(null)} />
              </View>
            )}
          </If>
        )}
      </If>
      <ReplayDialog
        path={props.path}
        open={replaying()}
        onClose={() => replaying.set(false)}
        onDone={(replay) => {
          sessionsChanged();
          props.onOpen?.(replay);
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
    <View class={terminal() ? "flex-1 gap-1" : "flex-1"}>
      <If when={!terminal()}>
        <SideHeader title={t("nav.sessions")}>
          <IconButton icon="folder-open" label={t("sessions.openFile")} shortcut="mod+o" onPress={() => void openSessionFile()} testID="open-file" />
          <IconButton icon="scroll-text" label={t("timeline.example")} onPress={example} testID="open-example" />
        </SideHeader>
      </If>
      <View class={terminal() ? "flex-row items-center gap-2" : "gap-2 px-2 pb-2"}>
        {/* a run at a time, or a program over its runs */}
        <Options
          testID="sessions-by"
          value={byProgram() ? "programs" : "runs"}
          items={[
            { value: "runs", title: t("sessions.byRun") },
            { value: "programs", title: t("sessions.byProgram") },
          ]}
          onChange={(v) => byProgram.set(v === "programs")}
          class="self-start"
        />
        <If when={terminal()}>
          <IconButton icon="scroll-text" label={t("timeline.example")} onPress={example} testID="open-example" />
        </If>
        {/* always there, over runs and over programs: a field that came and went moved the list */}
        <If when={!terminal()}>
          <SearchField small testID="session-search" value={query()} placeholder={t("sessions.search")} onChange={(v) => query.set(v)} />
        </If>
      </View>
      <If when={!byProgram()} else={
        <ProgramList
          query={terminal() ? "" : query()}
          onOpen={(p) => {
            batch(() => {
              paneOnOpen.set({ path: p, pane: "runs" });
              selected.set(p);
            });
          }}
        />
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
      onOpen={(np) => {
        sessions.set(api.sessions.list());
        if (terminal()) selected.set(np);
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
  effect(() => {
    const p = selected();
    if (p && !sessions().some((s) => s.path === p)) untracked(() => selected.set(sessions()[0]?.path ?? null));
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
