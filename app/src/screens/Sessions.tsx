// Sessions: what programs did to their object stores. The list on the
// left; a session reads as operations, and when sonde ran the program, its
// output beside them, lined up in time: choose an operation and the lines it
// printed light up, choose a line and the operations running then do. A
// session also shows as a waterfall, replays on another endpoint, compares
// with another session, and opens in a window of its own. In a terminal the
// levels stack.

import { cleanup, computed, effect, Scroll, signal, untracked, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup, type DataTableRef } from "nsh/base";
import { isAvailable, Window } from "nsh/desktop";
import { t } from "../i18n";
import { api } from "../native";
import { openSession, report } from "../store";
import type { LogLine, SessionInfo, SessionView } from "../types";
import { CompareView, ReplayDialog } from "../ui/compare";
import { Button, Choice, Empty, Heading, Muted, Segmented, terminal } from "../ui/kit";
import { linesOf, Observations, OperationDetail, OperationsTable, OutputTable, operationsAt, SessionHeader, SessionList } from "../ui/session";
import { Waterfall } from "../ui/waterfall";
import { RecordDetail } from "./Requests";

/** One session: its operations (and output), the detail of what is chosen, and what can be done with it. */
export function SessionPane(props: { path: string; others: () => SessionInfo[]; tick: () => number; onBack?: () => void; onOpen?: (p: string) => void }) {
  const op = signal<number | null>(null);
  const rec = signal<number | null>(null);
  const opHighlight = signal<number[]>([]);
  const outHighlight = signal<number[]>([]);
  const pane = signal<"operations" | "output" | "waterfall">("operations");
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
  const episode = () => {
    const i = op();
    const v = view();
    return i === null || !v ? null : (v.episodes[i] ?? null);
  };
  const record = () => {
    const id = rec();
    return id === null ? null : api.sessions.record(props.path, id);
  };

  const chooseOperation = (i: number) => {
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
      opsRef?.select({ row: ops[0] });
    }
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

  const detail = (back: boolean) => (
    <If
      when={record()}
      else={
        <If when={episode()} else={<Empty text={t("requests.select")} />}>
          {(e) => <OperationDetail path={props.path} episode={e()} shared={view()?.sharedNotes ?? []} onRecord={(id) => rec.set(id)} onBack={back ? () => op.set(null) : undefined} />}
        </If>
      }
    >
      {(r) => <RecordDetail record={r()} onBack={() => rec.set(null)} copyAs={(kind) => api.sessions.copy(props.path, r().id, kind)} />}
    </If>
  );

  const operations = (v: SessionView) => <OperationsTable view={v} onSelect={chooseOperation} ref={(r) => (opsRef = r)} highlight={opHighlight()} />;
  const output = () => <OutputTable lines={lines()} onSelect={chooseLine} ref={(r) => (outRef = r)} highlight={outHighlight()} />;

  const actions = () => (
    <View class="flex-row flex-wrap items-center gap-2">
      <Segmented
        value={pane()}
        items={[
          { value: "operations", title: t("sessions.operations") },
          ...(terminal() && lines().length ? [{ value: "output", title: t("sessions.output") }] : []),
          ...(terminal() ? [] : [{ value: "waterfall", title: t("sessions.waterfall") }]),
        ]}
        onChange={(p) => pane.set(p as "operations" | "output" | "waterfall")}
      />
      <View class="flex-1" />
      <Choice
        class="w-48"
        value={null}
        placeholder={t("compare.with")}
        items={props
          .others()
          .filter((s) => s.path !== props.path)
          .map((s) => ({ value: s.path, title: s.command ?? s.name }))}
        onChange={(p) => compareWith.set(p)}
        testID="compare-with"
      />
      <Button small label={t("replay.open")} onPress={() => replaying.set(true)} testID="replay-open" />
      <If when={isAvailable && !terminal() && props.onOpen}>
        <Button small variant="ghost" label={t("sessions.window")} onPress={openWindow} testID="open-window" />
      </If>
    </View>
  );

  const body = (v: SessionView) => {
    if (pane() === "waterfall") return <Waterfall view={v} onSelect={chooseOperation} />;
    if (pane() === "output") return output();
    if (!lines().length || terminal()) return operations(v);
    return (
      <ResizablePanelGroup axis="horizontal" class="flex-1">
        <ResizablePanel minSize={240}>
          <View class="flex-1 pr-1">{output()}</View>
        </ResizablePanel>
        <ResizablePanel minSize={280}>
          <View class="flex-1 pl-1">{operations(v)}</View>
        </ResizablePanel>
      </ResizablePanelGroup>
    );
  };

  const main = (v: SessionView) => (
    <If
      when={compareWith()}
      else={
        <View class="flex-1 gap-2">
          <If when={props.onBack}>
            <Button small variant="ghost" label={`‹ ${t("timeline.sessions")}`} onPress={() => props.onBack?.()} testID="sessions-back" />
          </If>
          <SessionHeader view={v} />
          <Observations view={v} />
          {actions()}
          {body(v)}
        </View>
      }
    >
      {(other) => <CompareView a={props.path} b={other()} onClose={() => compareWith.set(null)} />}
    </If>
  );

  return (
    <View class="flex-1" testID="session-pane">
      <If when={view()} else={<Empty text={t("timeline.pick")} />}>
        {(v) => (
          <If
            when={terminal()}
            else={
              <View class="flex-1 flex-row gap-3">
                <View class="flex-1">{main(v())}</View>
                <If when={(episode() || record()) && !compareWith()}>
                  <Scroll class="w-[400px] border-l border-border pl-3">{detail(false)}</Scroll>
                </If>
              </View>
            }
          >
            <If when={episode() || record()} else={main(v())}>
              <Scroll class="flex-1">{detail(true)}</Scroll>
            </If>
          </If>
        )}
      </If>
      <ReplayDialog
        path={props.path}
        open={replaying()}
        onClose={() => replaying.set(false)}
        onDone={(replay) => {
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
    <View class="flex-1 bg-background p-3">
      <SessionPane path={props.path} others={() => api.sessions.list()} tick={tick} />
    </View>
  );
}

export function Sessions() {
  const sessions = signal<SessionInfo[]>(api.sessions.list());
  // a terminal starts on the list; a window on the newest session
  const selected = signal<string | null>(openSession() ?? (terminal() ? null : (sessions()[0]?.path ?? null)));
  const tick = signal(0);

  // another area asked for a session (a run that ended, the palette)
  effect(() => {
    const p = openSession();
    if (!p) return;
    untracked(() => {
      sessions.set(api.sessions.list());
      selected.set(p);
      openSession.set(null);
    });
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
    } catch (e) {
      report(e);
    }
  };
  const list = () => (
    <View class="gap-2">
      <Heading>{t("timeline.sessions")}</Heading>
      <If when={sessions().length > 0} else={<Muted class="text-sm">{t("timeline.empty")}</Muted>}>
        <SessionList sessions={sessions()} selected={selected()} onSelect={(p) => selected.set(p)} />
      </If>
      <Button small variant="ghost" label={t("timeline.example")} onPress={example} testID="open-example" />
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

  return (
    <View class="flex-1 p-3" testID="screen-sessions">
      <If
        when={terminal()}
        else={
          <View class="flex-1 flex-row gap-3">
            <Scroll class="w-64" contentClass="pr-2">
              {list()}
            </Scroll>
            <View class="flex-1">
              <If when={selected()} else={<Empty text={sessions().length ? t("timeline.pick") : t("timeline.empty")} />}>
                {keyed()}
              </If>
            </View>
          </View>
        }
      >
        {/* one level at a time: sessions › session › operation › request */}
        <If when={selected()} else={<Scroll class="flex-1">{list()}</Scroll>}>
          {keyed()}
        </If>
      </If>
    </View>
  );
}
