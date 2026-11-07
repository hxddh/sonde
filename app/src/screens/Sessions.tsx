// Sessions: what programs did to their object stores. The list on the left,
// by day; a session reads from its figures and what stands out (each a
// filter) down to its operations — outcome, time, family, place in time —
// and an inspector beside them that goes from an operation to its requests
// to one request in full. When sonde ran the program, its output lines up
// with the operations: choose one and the other lights up. A session also
// shows as a timeline, replays on another endpoint, compares with another
// session, and opens in a window of its own. In a terminal the levels stack.

import { cleanup, Clipboard, computed, effect, LayoutAnimation, reduceMotion, Scroll, signal, Text, untracked, View, windowSize } from "nsh";
import { Button as BaseButton, ResizablePanel, ResizablePanelGroup, Sheet, type DataTableRef } from "nsh/base";
import { isAvailable, Menu, type MenuItem, Window } from "nsh/desktop";
import { t } from "../i18n";
import { api } from "../native";
import { crumb, go, notify, openSession, openSessionFile, report, sessionsChanged, sessionsTick, shellWidth } from "../store";
import type { LogLine, SessionInfo, SessionView } from "../types";
import { CompareView, ReplayDialog } from "../ui/compare";
import { Icon, iconSvg } from "../ui/icon";
import { Button, Empty, IconButton, Segmented, terminal } from "../ui/kit";
import { linesOf, Observations, OperationDetail, OperationsTable, OutputTable, operationsAt, SessionHeader, SessionList } from "../ui/session";
import { episodeTitle } from "../phrases";
import { programOf } from "../ui/sessionview";
import { Timeline } from "../ui/timeline";
import { RecordDetail } from "./Requests";

type Pane = "operations" | "aligned" | "timeline";

const program = programOf;

/** One session: its figures, its operations (and output), the inspector, and what can be done with it. */
export function SessionPane(props: { path: string; others: () => SessionInfo[]; tick: () => number; onBack?: () => void; onOpen?: (p: string) => void; inWindow?: boolean }) {
  const op = signal<number | null>(null);
  const rec = signal<number | null>(null);
  const opHighlight = signal<number[]>([]);
  const outHighlight = signal<number[]>([]);
  const filter = signal<number | null>(null);
  const compareWith = signal<string | null>(null);
  const replaying = signal(false);
  let opsRef: DataTableRef | undefined;
  let outRef: DataTableRef | undefined;

  const view = computed<SessionView | null>(() => {
    props.tick();
    try {
      return api.sessions.open(props.path);
    } catch (e) {
      report(e);
      return null;
    }
  });
  const lines = computed<LogLine[]>(() => {
    props.tick();
    try {
      return api.sessions.logLines(props.path);
    } catch {
      return [];
    }
  });
  // a session with output opens lined up with it
  const pane = signal<Pane>(untracked(() => (lines().length && !terminal() ? "aligned" : "operations")));

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
  /** The operations shown: all, or those the chosen observation is about. */
  const rows = () => {
    const v = view();
    const f = filter();
    if (!v || f === null) return undefined;
    return v.observations[f]?.episodes;
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
      const shown = rows();
      const at = shown ? shown.indexOf(ops[0]) : ops[0];
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

  const openWindow = () => {
    const v = view();
    Window.open(() => <SessionWindow path={props.path} />, {
      id: `session-${props.path.split("/").pop()}`,
      title: v?.header.command ?? props.path.split("/").pop() ?? "sonde",
      defaultSize: { width: 1100, height: 720 },
      restoreBounds: true,
    });
  };
  const copy = (text: string) => void Clipboard.writeText(text).then(() => notify(t("common.copied")), report);

  /** The operation's commands: native menus where there are any. */
  const operationMenu = (i: number) => {
    const v = view();
    const e = v?.episodes[i];
    if (!e || !isAvailable) return;
    chooseOperation(i);
    const targets = () => {
      try {
        return api.sessions.records(props.path, e.records).map((r) => r.target);
      } catch {
        return [];
      }
    };
    void Menu.popup([
      { label: t("sessions.copyTitle"), icon: iconSvg("copy"), onSelect: () => copy(episodeTitle(e)) },
      { label: t("sessions.copyKeys"), icon: iconSvg("file"), onSelect: () => copy([...new Set(targets())].join("\n")) },
      ...(lines().length ? ([{ type: "separator" }, { label: t("sessions.showOutput"), icon: iconSvg("scroll-text"), onSelect: () => pane.set("aligned") }] as MenuItem[]) : []),
    ]);
  };
  const compareMenu = () => {
    const others = props.others().filter((s) => s.path !== props.path);
    if (!isAvailable) return;
    void Menu.popup(
      others.length
        ? others.slice(0, 20).map((s) => ({ label: `${program(s.command) ?? s.endpoint} · ${s.created.slice(11, 16)} · ${s.requests}`, onSelect: () => compareWith.set(s.path) }))
        : [{ label: t("compare.none"), disabled: true }],
    );
  };
  const moreMenu = () => {
    if (!isAvailable) return;
    void Menu.popup([
      { label: t("sessions.window"), icon: iconSvg("app-window"), onSelect: openWindow, disabled: props.inWindow },
      { label: t("sessions.copyShare"), icon: iconSvg("share-2"), onSelect: () => copy(`sonde share --serve ${props.path}`) },
      { label: t("sessions.copyPath"), icon: iconSvg("copy"), onSelect: () => copy(props.path) },
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

  const operations = (v: SessionView) => (
    <OperationsTable view={v} rows={rows()} onSelect={chooseOperation} onContextMenu={isAvailable && !terminal() ? operationMenu : undefined} ref={(r) => (opsRef = r)} highlight={opHighlight()} compact={pane() === "aligned" || (inspecting() && roomy())} />
  );
  const output = () => <OutputTable lines={lines()} onSelect={chooseLine} ref={(r) => (outRef = r)} highlight={outHighlight()} />;

  const panes = () => [
    { value: "operations", title: t("sessions.operations"), icon: "list-filter" as const },
    ...(lines().length ? [{ value: "aligned", title: t("sessions.withOutput"), icon: "scroll-text" as const }] : []),
    ...(terminal() ? [] : [{ value: "timeline", title: t("sessions.timeline"), icon: "chart-gantt" as const }]),
  ];

  const body = (v: SessionView) => {
    if (pane() === "timeline") return <Timeline view={v} rows={rows()} selected={op()} onSelect={chooseOperation} />;
    if (pane() === "aligned" && lines().length) {
      if (terminal()) return output();
      return (
        <ResizablePanelGroup axis="horizontal" class="min-h-0 flex-1">
          <ResizablePanel minSize={240}>
            <View class="flex-1 pr-1.5">{output()}</View>
          </ResizablePanel>
          <ResizablePanel minSize={320}>
            <View class="flex-1 pl-1.5">{operations(v)}</View>
          </ResizablePanel>
        </ResizablePanelGroup>
      );
    }
    return operations(v);
  };

  const actions = () => (
    <View class="flex-row items-center gap-1.5">
      <Button small variant="outline" icon="repeat-2" label={t("replay.open")} onPress={() => replaying.set(true)} testID="replay-open" />
      <If when={isAvailable && !terminal()}>
        <Button small variant="outline" icon="git-compare-arrows" label={t("compare.open")} onPress={compareMenu} testID="compare-open" />
        <IconButton icon="ellipsis" label={t("common.more")} onPress={moreMenu} testID="session-more" />
      </If>
    </View>
  );

  const filterNote = () => {
    const v = view();
    const f = filter();
    if (!v || f === null) return null;
    return t("sessions.filtered", { n: v.observations[f]?.episodes.length ?? 0, of: v.episodes.length });
  };

  const main = (v: SessionView) => (
    <View class="min-h-0 flex-1">
      <View class={terminal() ? "gap-1" : "gap-3.5 border-b border-border px-5 pb-3.5 pt-4"}>
        <If when={props.onBack}>
          <Button small variant="ghost" icon="chevron-left" label={t("timeline.sessions")} onPress={() => props.onBack?.()} testID="sessions-back" class="self-start" />
        </If>
        <SessionHeader view={v} actions={terminal() ? undefined : actions()} compact={(inspecting() && roomy()) || (props.inWindow ? windowSize().width : shellWidth()) < 1180} />
        <Observations view={v} filter={filter()} onFilter={(f) => filter.set(f)} oneLine={!roomy()} />
        <If when={terminal()}>{actions()}</If>
      </View>
      <View class={terminal() ? "flex-row items-center gap-2" : "flex-row items-center gap-3 px-5 py-2.5"}>
        <Segmented testID="session-pane-mode" value={pane()} items={panes()} onChange={(p) => pane.set(p as Pane)} />
        <View class="flex-1" />
        <If when={filterNote()}>
          {(n) => (
            <BaseButton testID="clear-filter" accessibilityLabel={t("sessions.clearFilter")} onPress={() => filter.set(null)} class="flex-row items-center gap-1.5 rounded-full bg-selection px-2.5 py-1 hover:bg-accent">
              <Text class="text-xs text-foreground">{n()}</Text>
              <If when={!terminal()}>
                <Icon name="x" size={12} />
              </If>
            </BaseButton>
          )}
        </If>
      </View>
      <View class={terminal() ? "min-h-0 flex-1" : "min-h-0 flex-1 px-5 pb-4"}>{body(v)}</View>
    </View>
  );

  // room for the inspector beside the operations, or it comes in from the edge
  const roomy = () => (props.inWindow ? windowSize().width : shellWidth()) >= (props.inWindow ? 980 : 1240);
  const inspecting = () => !!(episode() || record());

  const inspector = () => (
    <View testID="inspector" class="flex-1 border-l border-border bg-inspector">
      <View class="h-10 flex-row items-center gap-2 border-b border-border px-3">
        <Text class="flex-1 text-xs font-semibold text-muted-foreground">{record() ? t("inspector.request") : t("inspector.operation")}</Text>
        <IconButton icon="x" label={t("common.close")} onPress={closeInspector} testID="inspector-close" />
      </View>
      <Scroll class="flex-1" contentClass="p-4">
        {detail(false)}
      </Scroll>
    </View>
  );

  return (
    <View class="flex-1" testID="session-pane">
      <If when={view()} else={<Empty icon="list-clock" text={t("timeline.pick")} />}>
        {(v) => (
          <If when={compareWith()} else={
            <If
              when={terminal()}
              else={
                <If
                  when={roomy()}
                  else={
                    <View class="flex-1">
                      {main(v())}
                      <Sheet open={inspecting()} side="right" onOpenChange={(e) => !e.open && closeInspector()} popupClass="w-[420px] max-w-[90%] border-t border-border shadow-xl" popupStyle={{ marginTop: 44 }} backdropClass="bg-black/20">
                        {inspector()}
                      </Sheet>
                    </View>
                  }
                >
                  <ResizablePanelGroup axis="horizontal" class="flex-1">
                    <ResizablePanel minSize={460}>{main(v())}</ResizablePanel>
                    <If when={inspecting()}>
                      <ResizablePanel size={420} minSize={320} maxSize={680}>
                        {inspector()}
                      </ResizablePanel>
                    </If>
                  </ResizablePanelGroup>
                </If>
              }
            >
              <If when={episode() || record()} else={main(v())}>
                <Scroll class="flex-1">{detail(true)}</Scroll>
              </If>
            </If>
          }>
            {(other) => (
              <View class={terminal() ? "flex-1" : "flex-1 px-5 py-4"}>
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

/** A session in a window of its own. */
function SessionWindow(props: { path: string }) {
  const tick = signal(0);
  return (
    <View class="flex-1 bg-background">
      <SessionPane path={props.path} others={() => api.sessions.list()} tick={tick} inWindow />
    </View>
  );
}

export function Sessions() {
  const sessions = signal<SessionInfo[]>(api.sessions.list());
  // a terminal starts on the list; a window on the newest session
  const selected = signal<string | null>(openSession() ?? (terminal() ? null : (sessions()[0]?.path ?? null)));
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
    if (sessions().some((s) => s.live)) {
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
  const list = () => (
    <View class={terminal() ? "gap-1" : "gap-3"}>
      <View class="flex-row items-center gap-2 px-2">
        <Text role="header" class="flex-1 text-[13px] font-semibold text-foreground">
          {t("timeline.sessions")}
        </Text>
        <If when={!terminal()}>
          <IconButton icon="folder-open" label={t("sessions.openFile")} shortcut="mod+o" onPress={() => void openSessionFile()} testID="open-file" />
        </If>
        <IconButton icon="scroll-text" label={t("timeline.example")} onPress={example} testID="open-example" />
      </View>
      <If when={sessions().length > 0} else={<Text class="px-2 text-xs text-muted-foreground">{t("timeline.empty")}</Text>}>
        <SessionList sessions={sessions()} selected={selected()} onSelect={(p) => selected.set(p)} />
      </If>
    </View>
  );
  const pane = (p: string) => (
    <SessionPane
      path={p}
      others={sessions}
      tick={tick}
      onBack={terminal() ? () => selected.set(null) : undefined}
      onOpen={(np) => {
        sessions.set(api.sessions.list());
        if (terminal()) selected.set(np);
      }}
    />
  );

  // a pane per session: another session gets a fresh one (no selection carried over)
  const keyed = () => (
    <Each of={selected() ? [selected()!] : []} key={(p) => p}>
      {(p) => pane(p())}
    </Each>
  );

  const nothing = () => (
    <Empty icon="list-clock" title={t("sessions.noneTitle")} text={t("sessions.noneText")} testID="sessions-empty">
      <Button variant="primary" icon="play" label={t("sessions.runOne")} onPress={() => go("run")} />
      <Button variant="outline" icon="scroll-text" label={t("timeline.example")} onPress={example} testID="open-example" />
      <If when={!terminal()}>
        <Button variant="ghost" icon="folder-open" label={t("sessions.openFile")} onPress={() => void openSessionFile()} />
      </If>
    </Empty>
  );

  return (
    <View class="flex-1" testID="screen-sessions">
      <If
        when={terminal()}
        else={
          <If when={sessions().length > 0} else={nothing()}>
            <View class="flex-1 flex-row">
              <Scroll class={shellWidth() < 1100 ? "w-56 border-r border-border" : "w-64 border-r border-border"} contentClass="px-2 py-4">
                {list()}
              </Scroll>
              <View class="min-w-0 flex-1">
                <If when={selected()} else={<Empty icon="list-clock" text={t("timeline.pick")} />}>
                  {keyed()}
                </If>
              </View>
            </View>
          </If>
        }
      >
        {/* one level at a time: sessions › session › operation › request */}
        <View class="flex-1 p-1">
          <If when={selected()} else={<Scroll class="flex-1">{list()}</Scroll>}>
            {keyed()}
          </If>
        </View>
      </If>
    </View>
  );
}
