// Run: the program and what it does to S3, side by side. Type a command and
// press Return: sonde starts it in a terminal, its requests signed for the
// endpoint in the toolbar; its operations appear beside it as they happen,
// over a band of live figures. When it ends, a toast and a notification
// say how, and the session opens lined up with the output.

import { cleanup, effect, LayoutAnimation, reduceMotion, resource, Scroll, signal, Text, View } from "nsh";
import { Input, ResizablePanel, ResizablePanelGroup, Terminal, type TerminalRef } from "nsh/base";
import { isAvailable, Menu } from "nsh/desktop";
import { count, elapsed, ms, percent } from "../format";
import { t } from "../i18n";
import { analysis } from "../analysis";
import { api } from "../native";
import { command, cwd, history, input, lastRun, onOutput, openLastRun, output, policy, resize, runAgain, running, start, startedAt, stop } from "../run";
import { away, endpoint, endpoints, endpointsMode, go, openSession, report, sessionsChanged, showInSidebar } from "../store";
import type { Profile, SessionView } from "../types";
import { Icon } from "../ui/icon";
import { Badge, Button, Card, CARD, CONTROL, Dot, Empty, Figure, IconButton, KeyValue, ListRow, Options, SideHeader, terminal, SPLIT_HANDLE } from "../ui/kit";
import { programOf, scrubCommand } from "../ui/sessionview";
import { type Policy, Observations, OperationsTable, RecordPanel } from "../ui/session";
import { LatencyBand, liveFigures, offset, SessionShape, sessionStory } from "../ui/plots";
import { rememberedSize } from "../ui/sizes";

const POLICIES = (): { value: Policy; title: string; icon: "shield-check" | "ban" | "lock" }[] => [
  { value: "allow", title: t("timeline.allow"), icon: "shield-check" },
  { value: "no-delete", title: t("timeline.noDelete"), icon: "ban" },
  { value: "read-only", title: t("timeline.readOnly"), icon: "lock" },
];

/** The command: the one field that matters, with what it needs around it. */
function CommandBar() {
  // while a program runs, the endpoint it talks to, whatever is chosen since
  const ep = () => endpoints().find((e) => e.id === (running()?.endpoint ?? endpoint()));
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
        <Options class="self-start" value={policy()} items={POLICIES()} onChange={(v) => policy.set(v as Policy)} />
      </View>
    );
  }
  return (
    <View class="gap-2">
      {/* the command: a field like any other, a size up, with what runs it at its end */}
      <View class="flex-row items-center gap-2">
        <View class={`h-10 min-w-0 flex-1 flex-row items-center gap-2 rounded-(--radius) border border-input bg-background pl-3 pr-1.5 focus-within:border-ring focus-within:ring-2 focus-within:ring-ring/50`}>
          <Text class="font-mono text-body font-semibold text-muted-foreground">$</Text>
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
        </View>
        <If
          when={running()}
          else={<Button variant="primary" icon="play" label={t("run.start")} shortcut="enter" onPress={run} disabled={!command().trim() || !endpoint()} testID="run-start" class="h-10 px-4" />}
        >
          <Button variant="destructive" icon="square" label={t("run.stop")} onPress={stop} testID="run-stop" class="h-10 px-4" />
        </If>
      </View>
      {/* where it runs, where its requests go, what it may do: one row, one height */}
      <View class="flex-row flex-wrap items-center gap-x-3 gap-y-2">
        <View class={`${CONTROL.small} w-64 flex-row items-center gap-1.5 rounded-(--radius) border border-input bg-background px-2 focus-within:border-ring`}>
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
        <View class="min-w-0 flex-1 flex-row items-center gap-1.5">
          <Icon name="server" size={14} />
          <Text class="min-w-0 flex-1 text-small text-muted-foreground line-clamp-1">{ep() ? t("run.goesTo", { name: ep()!.name }) : t("run.chooseEndpoint")}</Text>
        </View>
        <Options testID="run-policy" value={policy()} items={POLICIES()} onChange={(v) => policy.set(v as Policy)} />
      </View>
    </View>
  );
}

function ProgramTerminal() {
  let term: TerminalRef | undefined;
  const off = onOutput((d) => term?.write(d));
  cleanup(off);
  // a run starts on a clean terminal: the one before is its session's now
  let shownRun = startedAt();
  effect(() => {
    const at = startedAt();
    if (at !== shownRun) term?.reset();
    shownRun = at;
  });
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
      accessibilityLabel={scrubCommand(running()?.command ?? lastRun()?.command ?? "") || t("run.program")}
      cursorClass="bg-foreground/50"
      class={terminal() ? "flex-1 border border-border" : `flex-1 ${CARD} p-2`}
      testID="run-terminal"
    />
  );
}

/** The run's figures as they change: running for, requests, errors, bytes; then how it ended. */
function StatusBand(props: { view: SessionView | null; now: () => number }) {
  const s = () => props.view?.summary;
  const r = () => lastRun();
  const warnings = () => props.view?.observations.filter((o) => o.level === "warn").length ?? 0;
  // once it ended: held to its program's baseline, as sonde check would
  const verdict = resource(
    () => (r() && !r()!.removed && r()!.path ? r()!.path : null),
    async (path) => {
      try {
        return await api.sessions.history(path);
      } catch {
        return null;
      }
    },
  );
  const failed = () => verdict()?.check?.verdicts.filter((v) => !v.ok) ?? [];
  const verdictText = () => {
    const c = verdict()?.check;
    if (!c) return null;
    if (c.passed) return t("run.withinBaseline");
    const v = failed()[0];
    if (!v) return t("run.pastBaseline");
    // past a limit of nothing: say how many, not "+0%"
    if (v.limit <= 0) return t("run.pastBaselineN", { what: t(`verdict.${v.what}` as never), n: Math.round(v.value) });
    const grew = Math.round(((v.value - v.limit) * 100) / v.limit);
    return t("run.pastBaselineBy", { what: t(`verdict.${v.what}` as never), pct: grew });
  };
  return (
    <Card pad="none" testID={r() ? "run-ended" : "run-status"} class="min-h-11 shrink-0 flex-row flex-wrap items-center gap-x-4 gap-y-2 px-3 py-2">
      <If
        when={r()}
        else={
          <View class="flex-row items-center gap-2">
            <Dot tone="live" label={t("run.running")} />
            <Text class="tabular-nums text-body font-semibold text-foreground">{elapsed(props.now() - (startedAt() ?? props.now()))}</Text>
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
          <View class="flex-row flex-wrap items-center gap-x-3">
            <Text class="tabular-nums text-small text-foreground">{t("run.bandRequests", { n: x().requests })}</Text>
            <If when={x().errors > 0}>
              <Text class="tabular-nums text-small text-status-server">{t("run.bandErrors", { n: x().errors })}</Text>
            </If>
            <If when={x().misses > 0}>
              <Text class="tabular-nums text-small text-foreground">{t("run.bandMisses", { n: x().misses })}</Text>
            </If>
            <If when={r()}>
              <Text class="tabular-nums text-small text-muted-foreground">{offset(x().seconds * 1000)}</Text>
            </If>
            <If when={r() && warnings() > 0}>
              <Text class="text-small text-warn">{t("run.toLook", { n: warnings() })}</Text>
            </If>
          </View>
        )}
      </If>
      <If when={verdictText()}>{(v) => <Badge testID="run-verdict" label={v()} tone={verdict()?.check?.passed ? "pass" : "fail"} />}</If>
      <View class="flex-1" />
      <If when={r() && !r()!.removed}>
        <View class="flex-row flex-wrap items-center gap-2">
          {/* the demo done: the next step is the user's own program and store */}
          <If when={endpoint() === "demo"}>
            <Button small variant="outline" icon="plus" label={t("run.nowYours")} tip={t("run.nowYoursHint")} onPress={() => (endpointsMode.set("add"), go("endpoints"))} testID="run-now-yours" />
          </If>
          {/* the loop in one place: change the program, run it again, see what it did */}
          <Button small variant="outline" icon="list-clock" label={t("run.lineUp")} onPress={() => openLastRun(go)} testID="run-open-session" />
          {/* the loop's next step, and so the one primary here */}
          <Button small variant="primary" icon="play" label={t("sessions.runAgain")} onPress={() => runAgain(r()!.path, r()!.command, r()!.endpoint, cwd() || null)} testID="run-again-band" />
        </View>
      </If>
    </Card>
  );
}

/** The run as it goes: requests a second, failures, in flight and the p95
 * now; requests over time beside how long they take. */
/** What the live view shows: the last half minute. */
const LIVE_WINDOW_MS = 30_000;
const liveRange = (p: Profile): [number, number] | null => {
  const n = p.rate.length;
  const keep = Math.ceil(LIVE_WINDOW_MS / Math.max(p.binMs, 1));
  return n > keep ? [n - keep, n - 1] : null;
};

function LivePanel(props: { profile: Profile }) {
  const f = () => liveFigures(props.profile);
  // the first requests have no shape yet: the figures alone, the charts once there is one
  const shaped = () => props.profile.rate.length > 1 && props.profile.samples.length >= 12;
  return (
    <Card pad="none" testID="live" class="shrink-0 gap-2 px-3 py-2">
      <View class="flex-row items-center gap-6">
        <Figure small testID="live-rate" value={f().perSecond.toFixed(f().perSecond < 10 ? 1 : 0)} label={t("live.perSecond")} />
        <Figure small testID="live-failed" value={percent(f().failedShare)} label={t("live.failed")} tone={f().failedShare > 0 ? "bad" : "plain"} />
        <Figure small testID="live-in-flight" value={count(f().inFlight)} label={t("live.inFlight")} />
        <Figure small testID="live-p95" value={ms(f().p95)} label={t("live.p95")} />
      </View>
      <If when={shaped()}>
        <View class="flex-row gap-4">
          {/* the last 30 seconds, sliding as the run goes */}
          <SessionShape profile={props.profile} range={liveRange(props.profile)} class="h-16 min-w-0 flex-1" testID="run-shape" />
          <View class="min-w-0 flex-1">
            <LatencyBand profile={props.profile} domainMs={[Math.max(0, props.profile.wallMs - LIVE_WINDOW_MS), Math.max(props.profile.wallMs, 1)]} class="h-16 w-full" testID="run-band" />
          </View>
        </View>
      </If>
    </Card>
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
    <View class="min-h-0 flex-1 gap-2" testID="run-operations">
      <StatusBand view={view()} now={now} />
      <If when={view() && view()!.episodes.length > 0} else={<If when={lastRun() && view()} else={<Empty icon="activity" text={t("run.waiting")} />}>
            {/* the SDK hints only for a program that ran and still sent nothing */}
            <If
              when={!lastRun()!.killed && lastRun()!.exit !== 127}
              else={<Empty icon="activity" text={lastRun()!.killed ? t("run.stoppedEmpty") : t("run.notFound")} />}
            >
              <NoRequests />
            </If>
          </If>}>
        <View class="min-h-0 flex-1 gap-2">
          {/* while it runs: its requests over time, growing as they arrive */}
          <If when={!terminal() && !lastRun() && view()!.profile}>
            <LivePanel profile={view()!.profile} />
          </If>
          {/* once it ended: the run in a sentence and what stands out, where the user already looks */}
          <If when={lastRun() && view()}>
            <If when={view()!.profile?.groups.length}>
              <Text testID="run-story" class="px-1 text-body leading-5 text-foreground">{sessionStory(view()!.profile, view()!.observations)}</Text>
            </If>
            <Observations view={view()!} compact max={3} />
          </If>
          <OperationsTable view={view()!} onSelect={() => lastRun() && openLastRun(go)} compact />
        </View>
      </If>
    </View>
  );
}

/** A run that sent sonde nothing: how each common SDK is pointed at it. */
function NoRequests() {
  return (
    <View testID="run-no-requests" class={terminal() ? "gap-1" : "flex-1 items-center justify-center gap-3 p-6"}>
      <Text class={terminal() ? "font-semibold text-foreground" : "text-center text-heading font-semibold text-foreground"}>{t("run.noneTitle")}</Text>
      <Text class={terminal() ? "text-muted-foreground" : "max-w-[520px] text-center text-body text-muted-foreground"}>{t("run.noneText")}</Text>
      <Card class="w-full max-w-[640px]">
        <KeyValue
          mono
          rows={[
            ["boto3 / botocore", t("run.noneBoto")],
            ["AWS CLI", t("run.noneCli")],
            ["Go SDK v2", t("run.noneGo")],
            ["Java SDK v2 · s3a", t("run.noneJava")],
            ["DuckDB", "SET s3_endpoint=…; SET s3_url_style='path'; SET s3_use_ssl=false"],
            ["Node (@aws-sdk)", t("run.noneNode")],
          ]}
        />
      </Card>
      <Text class={terminal() ? "text-muted-foreground" : "max-w-[520px] text-center text-small text-muted-foreground"}>{t("run.noneKeys")}</Text>
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
        <If when={terminal() && history().length > 0}>
          <View class="gap-1">
            <Text role="header" class="px-1 text-small font-semibold text-muted-foreground">
              {t("run.recent")}
            </Text>
            <Each of={history().slice(0, 5)}>
              {(c) => (
                <ListRow label={scrubCommand(c())} onPress={() => command.set(c())}>
                  <Text class="flex-1 font-mono text-small text-foreground line-clamp-1">{scrubCommand(c())}</Text>
                </ListRow>
              )}
            </Each>
          </View>
        </If>
        <RecordPanel recording={recording()} onChange={() => recording.set(api.sessions.current())} policy={policy} />
        <Button small variant="ghost" icon="scroll-text" label={t("run.example")} onPress={example} testID="open-example" class="-ml-2 self-start" />
      </View>
    </View>
  );
}

const opsWidth = rememberedSize("run.operations", 560);

/** The commands run before, in the sidebar: a press puts one back in the field. */
function Recent() {
  return (
    <View class="flex-1">
      <SideHeader title={t("run.recent")} />
      <If when={history().length > 0} else={<Text class="px-4 text-small text-muted-foreground">{t("run.recentNone")}</Text>}>
        <Scroll class="min-h-0 flex-1" contentClass="gap-0.5 px-2 pb-2" scrollbar={false}>
          <Each of={history().slice(0, 30)}>
            {(c) => (
              <ListRow label={scrubCommand(c())} selected={command() === c()} onPress={() => command.set(c())} class="items-start">
                <View class="min-w-0 flex-1">
                  <Text class="text-body font-medium text-foreground line-clamp-1">{programOf(c()) ?? scrubCommand(c())}</Text>
                  <Text class="font-mono text-caption text-muted-foreground line-clamp-1">{scrubCommand(c())}</Text>
                </View>
              </ListRow>
            )}
          </Each>
        </Scroll>
      </If>
    </View>
  );
}

export function Run() {
  const active = () => running() !== null || lastRun() !== null;
  if (!terminal()) cleanup(showInSidebar("run", () => <Recent />));
  return (
    <View class={terminal() ? "flex-1 gap-1 p-1" : "flex-1 gap-4 px-5 pb-5 pt-4"} testID="screen-run">
      <CommandBar />
      <If when={active()} else={<Intro />}>
        <If
          when={terminal()}
          else={
            <ResizablePanelGroup axis="horizontal" class="min-h-0 flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("run.resizeOperations")}>
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

