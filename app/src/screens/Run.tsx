// Run: the program and what it does to S3, side by side. Type a command and
// press Return: sonde starts it in a terminal, its requests signed for the
// endpoint in the toolbar; its operations appear beside it as they happen,
// over a band of live figures. When it ends, a toast and a notification
// say how, and the session opens lined up with the output.

import { cleanup, LayoutAnimation, reduceMotion, signal, Text, View } from "nsh";
import { Button as BaseButton, Input, ResizablePanel, ResizablePanelGroup, Terminal, type TerminalRef } from "nsh/base";
import { isAvailable, Menu } from "nsh/desktop";
import { bytes } from "../format";
import { t } from "../i18n";
import { analysis } from "../analysis";
import { api } from "../native";
import { command, cwd, history, input, lastRun, onOutput, openLastRun, output, policy, resize, running, start, startedAt, stop } from "../run";
import { away, endpoint, endpoints, go, openSession, report, sessionsChanged } from "../store";
import type { SessionView } from "../types";
import { Icon } from "../ui/icon";
import { Button, Dot, Empty, IconButton, Segmented, terminal } from "../ui/kit";
import { type Policy, Observations, OperationsTable, RecordPanel } from "../ui/session";
import { SessionShape } from "../ui/plots";
import { rememberedSize } from "../ui/sizes";
import { seconds } from "../ui/sessionview";

const POLICIES = (): { value: Policy; title: string; icon: "shield-check" | "ban" | "lock" }[] => [
  { value: "allow", title: t("timeline.allow"), icon: "shield-check" },
  { value: "no-delete", title: t("timeline.noDelete"), icon: "ban" },
  { value: "read-only", title: t("timeline.readOnly"), icon: "lock" },
];

/** The command: the one field that matters, with what it needs around it. */
function CommandBar() {
  const ep = () => endpoints().find((e) => e.id === endpoint());
  const run = () => {
    const id = endpoint();
    if (!id) return report({ kind: "config", message: t("run.chooseEndpoint") });
    start(command(), id);
  };
  const recent = () => {
    if (!isAvailable || !history().length) return;
    void Menu.popup(history().map((c) => ({ label: c, onSelect: () => command.set(c) })));
  };
  if (terminal()) {
    return (
      <View class="gap-1">
        <View class="flex-row items-center gap-1">
          <Text class="text-muted-foreground">$</Text>
          <Input testID="run-command" value={command()} placeholder={t("run.placeholder")} onChange={({ value }) => command.set(value)} onSubmit={run} submitBehavior="submit" class="flex-1 text-foreground" />
          <If when={running()} else={<Button label={t("run.start")} onPress={run} disabled={!command().trim() || !endpoint()} testID="run-start" />}>
            <Button variant="destructive" label={t("run.stop")} onPress={stop} testID="run-stop" />
          </If>
        </View>
        <Segmented value={policy()} items={POLICIES()} onChange={(v) => policy.set(v as Policy)} />
      </View>
    );
  }
  return (
    <View class="gap-2.5">
      <View class="h-11 flex-row items-center gap-2 rounded-(--radius-card) border border-input bg-card pl-3.5 pr-1.5 shadow-sm focus-within:border-ring">
        <Text class="font-mono text-heading font-semibold text-muted-foreground">$</Text>
        <Input
          testID="run-command"
          value={command()}
          placeholder={t("run.placeholder")}
          autoCapitalize="none"
          autoCorrect={false}
          onChange={({ value }) => command.set(value)}
          onSubmit={run}
          submitBehavior="submit"
          accessibilityLabel={t("run.command")}
          class="min-w-0 flex-1 font-mono text-body text-foreground placeholder:text-muted-foreground"
        />
        <If when={history().length > 0 && isAvailable}>
          <IconButton icon="clock" label={t("run.recent")} onPress={recent} testID="run-history" />
        </If>
        <If
          when={running()}
          else={<Button variant="primary" icon="play" label={t("run.start")} shortcut="enter" onPress={run} disabled={!command().trim() || !endpoint()} testID="run-start" />}
        >
          <Button variant="destructive" icon="square" label={t("run.stop")} onPress={stop} testID="run-stop" />
        </If>
      </View>
      <View class="flex-row flex-wrap items-center gap-x-4 gap-y-2 px-1">
        <View class="h-7 w-64 flex-row items-center gap-1.5 rounded-(--radius) px-1.5 hover:bg-muted">
          <Icon name="folder" size={14} />
          <Input
            testID="run-cwd"
            value={cwd()}
            placeholder={t("run.cwdHome")}
            autoCapitalize="none"
            autoCorrect={false}
            onChange={({ value }) => cwd.set(value)}
            accessibilityLabel={t("run.cwd")}
            class="min-w-0 flex-1 font-mono text-small text-foreground placeholder:text-muted-foreground"
          />
        </View>
        <View class="flex-row items-center gap-1.5">
          <Icon name="server" size={14} />
          <Text class="text-small text-muted-foreground">{ep() ? t("run.goesTo", { name: ep()!.name }) : t("run.chooseEndpoint")}</Text>
        </View>
        <View class="flex-1" />
        <Segmented testID="run-policy" value={policy()} items={POLICIES()} onChange={(v) => policy.set(v as Policy)} />
      </View>
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
      class={terminal() ? "flex-1 border border-border" : "flex-1 rounded-(--radius-card) border border-border bg-card px-2.5 py-2"}
      testID="run-terminal"
    />
  );
}

function clockOf(ms: number) {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** The run's figures as they change: running for, requests, errors, bytes; then how it ended. */
function StatusBand(props: { view: SessionView | null; now: () => number }) {
  const s = () => props.view?.summary;
  const r = () => lastRun();
  const warnings = () => props.view?.observations.filter((o) => o.level === "warn").length ?? 0;
  return (
    <View testID={r() ? "run-ended" : "run-status"} class={terminal() ? "flex-row gap-2" : "min-h-11 flex-row flex-wrap items-center gap-x-4 gap-y-1 rounded-(--radius-card) border border-border bg-card px-3.5 py-2"}>
      <If
        when={r()}
        else={
          <View class="flex-row items-center gap-2">
            <Dot tone="live" label={t("run.running")} />
            <Text class="tabular-nums text-body font-semibold text-foreground">{clockOf(props.now() - (startedAt() ?? props.now()))}</Text>
          </View>
        }
      >
        {(done) => (
          <View class="flex-row items-center gap-2">
            <Dot tone={done().killed ? "refused" : done().exit === 0 ? "ok" : "server"} />
            <Text class="text-body font-semibold text-foreground">{done().killed ? t("run.killed") : t("run.exited", { code: String(done().exit ?? "?") })}</Text>
          </View>
        )}
      </If>
      <If when={s()}>
        {(x) => (
          <View class="flex-row flex-wrap items-center gap-x-4">
            <Text class="tabular-nums text-small text-foreground">{t("run.bandRequests", { n: x().requests })}</Text>
            <If when={x().errors > 0}>
              <Text class="tabular-nums text-small text-status-server">{t("run.bandErrors", { n: x().errors })}</Text>
            </If>
            <If when={x().misses > 0}>
              <Text class="tabular-nums text-small text-status-miss">{t("run.bandMisses", { n: x().misses })}</Text>
            </If>
            <Text class="tabular-nums text-small text-muted-foreground">{`↑${bytes(x().bytesUp)} ↓${bytes(x().bytesDown)}`}</Text>
            <If when={r() && warnings() > 0}>
              <Text class="text-small text-warn">{t("run.toLook", { n: warnings() })}</Text>
            </If>
            <If when={r()}>
              <Text class="tabular-nums text-small text-muted-foreground">{`${seconds(x().seconds)} s`}</Text>
            </If>
          </View>
        )}
      </If>
      <View class="flex-1" />
      <If when={r()}>
        <Button small variant="outline" icon="list-clock" label={t("run.lineUp")} onPress={() => openLastRun(go)} testID="run-open-session" />
      </If>
    </View>
  );
}

function LiveOperations() {
  const tick = signal(0);
  const now = signal(Date.now());
  const timer = setInterval(() => {
    if (away()) return;
    now.set(Date.now());
    if (running()) {
      // operations arriving slide in rather than jump
      if (!reduceMotion()) LayoutAnimation.easeInEaseOut();
      tick.set(tick() + 1);
    }
  }, 500);
  cleanup(() => clearInterval(timer));
  const path = () => running()?.path ?? lastRun()?.path ?? null;
  // read again as the program's requests arrive, and once more as it ends
  const view = analysis(path, () => (tick(), lastRun())).view;

  return (
    <View class="min-h-0 flex-1 gap-2.5" testID="run-operations">
      <StatusBand view={view()} now={now} />
      <If when={view() && view()!.episodes.length > 0} else={<Empty icon="activity" text={t("run.waiting")} />}>
        <View class="min-h-0 flex-1 gap-2.5">
          {/* the run's requests over time, growing as they arrive */}
          <If when={!terminal() && view()!.profile}>
            <SessionShape profile={view()!.profile} class="h-10 w-full" testID="run-shape" />
          </If>
          <If when={lastRun() && view()}>
            <Observations view={view()!} compact max={3} />
          </If>
          <OperationsTable view={view()!} onSelect={() => lastRun() && openLastRun(go)} compact />
        </View>
      </If>
    </View>
  );
}

/** Before the first run: what Run does, the commands run before, and recording without a program. */
function Intro() {
  const recording = signal(api.sessions.current());
  const example = () => {
    try {
      openSession.set(api.sessions.example());
      sessionsChanged();
      go("sessions");
    } catch (e) {
      report(e);
    }
  };
  return (
    <View class="flex-1 items-center justify-center p-6" testID="run-intro">
      <View class="w-full max-w-[560px] gap-6">
        <View class="gap-2">
          <Text class="text-title font-semibold text-foreground">{t("run.title")}</Text>
          <Text class="text-body leading-5 text-muted-foreground">{t("run.hint")}</Text>
        </View>
        <If when={history().length > 0}>
          <View class="gap-1">
            <Text role="header" class="px-1 text-small font-semibold text-muted-foreground">
              {t("run.recent")}
            </Text>
            <Each of={history().slice(0, 5)}>
              {(c) => (
                <BaseButton accessibilityLabel={c()} onPress={() => command.set(c())} class="flex-row items-center gap-2.5 rounded-(--radius) px-2 py-1.5 hover:bg-muted">
                  <If when={!terminal()}>
                    <Icon name="square-terminal" size={14} />
                  </If>
                  <Text class="flex-1 font-mono text-small text-foreground line-clamp-1">{c()}</Text>
                </BaseButton>
              )}
            </Each>
          </View>
        </If>
        <RecordPanel recording={recording()} onChange={() => recording.set(api.sessions.current())} policy={policy} />
        <BaseButton accessibilityLabel={t("run.example")} onPress={example} testID="open-example" class="flex-row items-center gap-1.5 self-start rounded-(--radius-sm) px-1 py-0.5 hover:bg-muted">
          <If when={!terminal()}>
            <Icon name="scroll-text" size={14} />
          </If>
          <Text class="text-small text-muted-foreground">{t("run.example")}</Text>
        </BaseButton>
      </View>
    </View>
  );
}

const opsWidth = rememberedSize("run.operations", 560);

export function Run() {
  const active = () => running() !== null || lastRun() !== null;
  return (
    <View class={terminal() ? "flex-1 gap-1 p-1" : "flex-1 gap-4 px-5 pb-5 pt-4"} testID="screen-run">
      <CommandBar />
      <If when={active()} else={<Intro />}>
        <If
          when={terminal()}
          else={
            <ResizablePanelGroup axis="horizontal" class="min-h-0 flex-1">
              <ResizablePanel minSize={320}>
                <View class="flex-1 pr-2">
                  <ProgramTerminal />
                </View>
              </ResizablePanel>
              <ResizablePanel size={opsWidth.size()} onResize={opsWidth.onResize} minSize={340}>
                <View class="flex-1 pl-2">
                  <LiveOperations />
                </View>
              </ResizablePanel>
            </ResizablePanelGroup>
          }
        >
          {/* a terminal inside a terminal: the program above, its operations below */}
          <View class="flex-1 gap-1">
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

