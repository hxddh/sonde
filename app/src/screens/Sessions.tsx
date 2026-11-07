// Sessions: what programs did to their object stores. The list on the
// left; a session reads as operations, and when sonde ran the program, its
// output beside them, lined up in time: choose an operation and the lines it
// printed light up, choose a line and the operations running then do. An
// operation opens on its requests, a request on the inspector. In a
// terminal the levels stack.

import { cleanup, computed, effect, Scroll, signal, untracked, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup, type DataTableRef } from "nsh/base";
import { t } from "../i18n";
import { api } from "../native";
import { openSession, report } from "../store";
import type { LogLine, SessionInfo, SessionView } from "../types";
import { Button, Empty, Heading, Muted, Segmented, terminal } from "../ui/kit";
import { linesOf, Observations, OperationDetail, OperationsTable, OutputTable, operationsAt, SessionHeader, SessionList } from "../ui/session";
import { RecordDetail } from "./Requests";

export function Sessions() {
  const sessions = signal<SessionInfo[]>(api.sessions.list());
  // a terminal starts on the list; a window on the newest session
  const selected = signal<string | null>(openSession() ?? (terminal() ? null : (sessions()[0]?.path ?? null)));
  const op = signal<number | null>(null);
  const rec = signal<number | null>(null);
  const opHighlight = signal<number[]>([]);
  const outHighlight = signal<number[]>([]);
  const pane = signal<"operations" | "output">("operations");
  const tick = signal(0);
  let opsRef: DataTableRef | undefined;
  let outRef: DataTableRef | undefined;

  const select = (p: string | null) => {
    selected.set(p);
    op.set(null);
    rec.set(null);
    opHighlight.set([]);
    outHighlight.set([]);
  };
  // another area asked for a session (a run that ended, the palette)
  effect(() => {
    const p = openSession();
    if (!p) return;
    untracked(() => {
      sessions.set(api.sessions.list());
      select(p);
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

  const view = computed<SessionView | null>(() => {
    tick();
    const p = selected();
    if (!p) return null;
    try {
      return api.sessions.open(p);
    } catch (e) {
      report(e);
      return null;
    }
  });
  const lines = computed<LogLine[]>(() => {
    tick();
    const p = selected();
    if (!p) return [];
    try {
      return api.sessions.logLines(p);
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
    const p = selected();
    return id === null || !p ? null : api.sessions.record(p, id);
  };
  const example = () => {
    try {
      const p = api.sessions.example();
      sessions.set(api.sessions.list());
      select(p);
    } catch (e) {
      report(e);
    }
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

  const detail = (back: boolean) => (
    <If
      when={record()}
      else={
        <If when={episode()} else={<Empty text={t("requests.select")} />}>
          {(e) => <OperationDetail path={selected()!} episode={e()} shared={view()?.sharedNotes ?? []} onRecord={(id) => rec.set(id)} onBack={back ? () => op.set(null) : undefined} />}
        </If>
      }
    >
      {(r) => <RecordDetail record={r()} onBack={() => rec.set(null)} copyAs={(kind) => api.sessions.copy(selected()!, r().id, kind)} />}
    </If>
  );

  const operations = (v: SessionView) => <OperationsTable view={v} onSelect={chooseOperation} ref={(r) => (opsRef = r)} highlight={opHighlight()} />;
  const output = () => <OutputTable lines={lines()} onSelect={chooseLine} ref={(r) => (outRef = r)} highlight={outHighlight()} />;

  const sessionPane = (v: SessionView) => (
    <View class="flex-1 gap-2">
      <SessionHeader view={v} />
      <Observations view={v} />
      <If
        when={lines().length > 0}
        else={operations(v)}
      >
        <If
          when={terminal()}
          else={
            <ResizablePanelGroup axis="horizontal" class="flex-1">
              <ResizablePanel minSize={240}>
                <View class="flex-1 pr-1">{output()}</View>
              </ResizablePanel>
              <ResizablePanel minSize={280}>
                <View class="flex-1 pl-1">{operations(v)}</View>
              </ResizablePanel>
            </ResizablePanelGroup>
          }
        >
          <Segmented
            value={pane()}
            items={[
              { value: "operations", title: t("sessions.operations") },
              { value: "output", title: t("sessions.output") },
            ]}
            onChange={(p) => pane.set(p as "operations" | "output")}
          />
          {pane() === "output" ? output() : operations(v)}
        </If>
      </If>
      <Muted class="text-xs">{t("timeline.replayHint", { path: v.path })}</Muted>
    </View>
  );

  const list = () => (
    <View class="gap-2">
      <Heading>{t("timeline.sessions")}</Heading>
      <If when={sessions().length > 0} else={<Muted class="text-sm">{t("timeline.empty")}</Muted>}>
        <SessionList sessions={sessions()} selected={selected()} onSelect={select} />
      </If>
      <Button small variant="ghost" label={t("timeline.example")} onPress={example} testID="open-example" />
    </View>
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
              <If when={view()} else={<Empty text={sessions().length ? t("timeline.pick") : t("timeline.empty")} />}>
                {(v) => sessionPane(v())}
              </If>
            </View>
            <If when={episode() || record()}>
              <Scroll class="w-[400px] border-l border-border pl-3">{detail(false)}</Scroll>
            </If>
          </View>
        }
      >
        {/* one level at a time: sessions › operations › requests › request */}
        <If when={selected() && view()} else={<Scroll class="flex-1">{list()}</Scroll>}>
          <If
            when={episode() || record()}
            else={
              <View class="flex-1 gap-1">
                <Button small variant="ghost" label={`‹ ${t("timeline.sessions")}`} onPress={() => select(null)} testID="sessions-back" />
                {sessionPane(view()!)}
              </View>
            }
          >
            <Scroll class="flex-1">{detail(true)}</Scroll>
          </If>
        </If>
      </If>
    </View>
  );
}
