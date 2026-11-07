// Run: the program and what it does to S3, side by side. Type a command,
// choose where its requests go, and sonde starts it in a terminal; its
// operations appear beside it as they happen. When it ends, the session
// opens in Sessions with the output lined up.

import { cleanup, computed, signal, Text, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup, Terminal, type TerminalRef } from "nsh/base";
import { t } from "../i18n";
import { api } from "../native";
import { command, input, lastRun, onOutput, openLastRun, output, policy, resize, running, start, stop } from "../run";
import { endpoint, endpoints, go, openSession, report } from "../store";
import type { RecordingView, SessionView } from "../types";
import { Badge, Button, Choice, Empty, Field, Muted, Segmented, terminal } from "../ui/kit";
import { type Policy, Observations, OperationsTable, RecordPanel, SessionHeader } from "../ui/session";

function Bar() {
  const target = signal<string | null>(endpoint());
  const chosen = () => target() ?? endpoint();
  const run = () => {
    const ep = chosen();
    if (!ep) return report({ kind: "config", message: t("run.chooseEndpoint") });
    start(command(), ep);
  };
  return (
    <View class="gap-2">
      <View class="flex-row flex-wrap items-end gap-2">
        <Field class="min-w-64 flex-1" mono label="$" value={command()} onChange={(v) => command.set(v)} placeholder={t("run.placeholder")} testID="run-command" onSubmit={run} />
        <Choice class="w-44" label={t("common.endpoint")} value={chosen()} items={endpoints().map((e) => ({ value: e.id, title: e.name }))} onChange={(v) => target.set(v)} testID="run-endpoint" />
        <If when={running()} else={<Button label={t("run.start")} onPress={run} disabled={!command().trim() || !chosen()} testID="run-start" />}>
          <Button variant="destructive" label={t("run.stop")} onPress={stop} testID="run-stop" />
        </If>
      </View>
      <Segmented
        value={policy()}
        items={[
          { value: "allow", title: t("timeline.allow") },
          { value: "no-delete", title: t("timeline.noDelete") },
          { value: "read-only", title: t("timeline.readOnly") },
        ]}
        onChange={(v) => policy.set(v as Policy)}
      />
    </View>
  );
}

function ProgramTerminal() {
  let term: TerminalRef | undefined;
  const off = onOutput((d) => term?.write(d));
  cleanup(off);
  return (
    <Terminal
      ref={(r) => {
        term = r;
        // what was printed before this terminal was on screen
        const before = output();
        if (before) r.write(before);
      }}
      onData={({ data }) => input(data)}
      onResize={({ cols, rows }) => resize(cols, rows)}
      accessibilityLabel={running()?.command ?? lastRun()?.command ?? "Program"}
      cursorClass="bg-foreground/50"
      class="flex-1 rounded-(--radius) border border-border bg-card px-2 py-1"
      testID="run-terminal"
    />
  );
}

function LiveOperations() {
  const tick = signal(0);
  const timer = setInterval(() => {
    if (running()) tick.set(tick() + 1);
  }, 700);
  cleanup(() => clearInterval(timer));
  const path = () => running()?.path ?? lastRun()?.path ?? null;
  const view = computed<SessionView | null>(() => {
    tick();
    const p = path();
    if (!p) return null;
    try {
      return api.sessions.open(p);
    } catch {
      return null;
    }
  });
  return (
    <View class="flex-1 gap-2" testID="run-operations">
      <If when={lastRun()}>
        {(r) => (
          <View class="flex-row items-center gap-2" testID="run-ended">
            <Badge label={r().killed ? t("run.killed") : t("run.exited", { code: String(r().exit ?? "?") })} tone={r().exit === 0 ? "pass" : "warn"} />
            <View class="flex-1" />
            <Button small label={t("run.lineUp")} onPress={() => openLastRun(go)} testID="run-open-session" />
          </View>
        )}
      </If>
      <If when={view()} else={<Empty text={t("run.waiting")} />}>
        {(v) => (
          <View class="flex-1 gap-2">
            <SessionHeader view={v()} />
            <Observations view={v()} />
            <OperationsTable view={v()} onSelect={() => openLastRun(go)} />
          </View>
        )}
      </If>
    </View>
  );
}

function Intro() {
  const recording = signal<RecordingView | null>(api.sessions.current());
  const example = () => {
    try {
      openSession.set(api.sessions.example());
      go("sessions");
    } catch (e) {
      report(e);
    }
  };
  return (
    <View class="flex-1 items-center justify-center gap-4 p-6" testID="run-intro">
      <View class="max-w-xl gap-2">
        <Text class="text-lg font-semibold text-foreground">{t("run.title")}</Text>
        <Muted class="text-sm">{t("run.hint")}</Muted>
      </View>
      <View class="w-full max-w-xl gap-2">
        <RecordPanel recording={recording()} onChange={() => recording.set(api.sessions.current())} policy={policy} />
        <Button small variant="ghost" label={t("run.example")} onPress={example} testID="open-example" />
      </View>
    </View>
  );
}

export function Run() {
  const active = () => running() !== null || lastRun() !== null;
  return (
    <View class="flex-1 gap-3 p-3" testID="screen-run">
      <Bar />
      <If when={active()} else={<Intro />}>
        <If
          when={terminal()}
          else={
            <ResizablePanelGroup axis="horizontal" class="flex-1">
              <ResizablePanel minSize={280}>
                <View class="flex-1 pr-1">
                  <ProgramTerminal />
                </View>
              </ResizablePanel>
              <ResizablePanel minSize={280}>
                <View class="flex-1 pl-1">
                  <LiveOperations />
                </View>
              </ResizablePanel>
            </ResizablePanelGroup>
          }
        >
          {/* a terminal inside a terminal: the program above, its operations below */}
          <View class="flex-1 gap-2">
            <View class="h-[45%]">
              <ProgramTerminal />
            </View>
            <LiveOperations />
          </View>
        </If>
      </If>
    </View>
  );
}
