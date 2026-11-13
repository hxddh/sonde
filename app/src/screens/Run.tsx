// Run: the program and what it does to S3, side by side. Type a command and
// press Return: sonde starts it in a terminal, its requests signed for the
// endpoint in the toolbar; its operations appear beside it as they happen,
// over a band of live figures. When it ends, a toast and a notification
// say how, and the session opens lined up with the output.

import { batch, cleanup, effect, LayoutAnimation, reduceMotion, resource, Scroll, signal, Text, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup, Terminal, type TerminalRef } from "nsh/base";
import { isAvailable, Menu } from "nsh/desktop";
import { change, count, duration, percent, rate } from "../format";
import { t } from "../i18n";
import { analysis } from "../analysis";
import { pagedOperations } from "../paged";
import { api } from "../native";
import { command, cwd, history, input, lastRun, onOutput, openLastRun, output, policy, resize, runAgain, running, start, startedAt, stop } from "../run";
import { away, compareOnOpen, endpoint, endpoints, endpointsMode, go, openSession, paneOnOpen, report, sessionsChanged, showInSidebar } from "../store";
import { SinceRun, sinceLastOf, statesOf } from "../ui/fixloop";
import { TrialDialog } from "../ui/trial";
import type { Profile, SessionView } from "../types";
import { Icon } from "../ui/icon";
import { Badge, Button, Card, CARD, Dot, Empty, Field, Figure, Heading, IconButton, KeyValue, ListRow, Options, READING, SideHeader, Strong, terminal, SPLIT_HANDLE, Title, ActionRow, SURFACE, WIDTH, PANE } from "../ui/kit";
import { programOf, scrubCommand } from "../ui/sessionview";
import { type Policy, Observations, OperationsTable, RecordPanel } from "../ui/session";
import { Chart, focusOf, LatencyBand, liveFigures, SessionShape, sessionStory, STRIP } from "../ui/plots";
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
    // the same command as the run that ended: run again, compared with it (as the band's Run again)
    const done = lastRun();
    if (done && !done.removed && done.path && command().trim() === done.command.trim()) return runAgain(done.path, done.command, done.endpoint, cwd() || null);
    start(command(), id);
  };
  const recent = () => {
    if (!isAvailable || !history().length) return;
    void Menu.popup(history().map((c) => ({ label: scrubCommand(c), onSelect: () => command.set(c) })));
  };
  if (terminal()) {
    return (
      <View class="gap-1">
        <View class="flex-row items-center gap-1">
          <Text class="text-muted-foreground">$</Text>
          <Field bare mono testID="run-command" label={t("run.placeholder")} value={command()} placeholder={t("run.placeholder")} onChange={(v) => command.set(v)} onSubmit={run} class="flex-1" />
          <If when={running()} else={<Button label={t("run.start")} onPress={run} disabled={!command().trim() || !endpoint()} testID="run-start" />}>
            <Button variant="destructive" label={t("run.stop")} onPress={stop} testID="run-stop" />
          </If>
        </View>
        <View class="flex-row">
          <Options value={policy()} items={POLICIES()} onChange={(v) => policy.set(v as Policy)} />
        </View>
      </View>
    );
  }
  return (
    <View class="gap-2">
      {/* the command: the kit's field at the kit's height, the one that matters by its width and its mono, with what runs it at its end */}
      <View class="flex-row items-center gap-2">
        <Field
          bare
          mono
          class="min-w-0 flex-1"
          testID="run-command"
          label={t("run.command")}
          value={command()}
          placeholder={t("run.placeholder")}
          onChange={(v) => command.set(v)}
          onSubmit={run}
          leading={<Text class="font-mono text-body text-muted-foreground">$</Text>}
          trailing={
            <If when={history().length > 0 && isAvailable}>
              <IconButton inField icon="clock" label={t("run.recent")} onPress={recent} testID="run-history" />
            </If>
          }
        />
        {/* the one primary of the place: Run, Stop while it runs; once a run ended, the band's Run again is the one Run */}
        <If when={running()} else={<If when={!lastRun()}><Button variant="primary" icon="play" label={t("run.start")} shortcut="enter" onPress={run} disabled={!command().trim() || !endpoint()} testID="run-start" /></If>}>
          <Button variant="destructive" icon="square" label={t("run.stop")} onPress={stop} testID="run-stop" />
        </If>
      </View>
      {/* where it runs, where its requests go, what it may do: one row, one height */}
      <View class="flex-row flex-wrap items-center gap-x-3 gap-y-2">
        <Field bare small mono class={WIDTH.long} testID="run-cwd" label={t("run.cwd")} value={cwd()} placeholder={t("run.cwdHome")} onChange={(v) => cwd.set(v)} leading={<Icon name="folder" />} />
        <View class="min-w-0 flex-1 flex-row items-center gap-2">
          <Icon name="server" />
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
  // the errors in the tone their rows are drawn in: the client's where every one was a 4xx
  const errorTone = () => {
    const c = props.view?.signals?.errors.counts;
    return c && c.client > 0 && c.server + c.transport === 0 ? "client" : "bad";
  };
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
  // "Try under faults…": its dialog
  const trying = signal(false);
  let tried = false;
  // once it ended, the band's second row: what to do next, labelled, and the one primary at its end.
  // The row measures itself: the others stand in rank order while they fit beside the primary, the
  // rest behind `⋯` (never on a third line, never under the figures)
  const widths = signal<Record<string, number>>({});
  const measure = (key: string) => ({ bounds }: { bounds: { size: { width: number } } }) => {
    const w = Math.round(bounds.size.width);
    if (widths()[key] !== w) widths.set({ ...widths(), [key]: w });
  };
  // (8 before the primary; never less than nothing)
  const room = () => Math.max(0, (widths().next ?? 0) - (widths().primary ?? 0) - 8);
  // the one Run of the place once a run ended: its command again (compared with it), or the field's
  // command when it was changed since — the field's Return runs the same
  const same = () => !!r() && command().trim() === r()!.command.trim();
  const runNext = () => {
    const done = r();
    if (!done || running()) return;
    if (same() && !done.removed && s()?.requests) return runAgain(done.path, done.command, done.endpoint, cwd() || null);
    const id = same() ? done.endpoint : endpoint();
    if (!id) return report({ kind: "config", message: t("run.chooseEndpoint") });
    start(same() ? done.command : command(), id);
  };
  const others = () => [
    // the loop in one place: change the program, run it again, see what it did
    { id: "run-open-session", label: t("run.lineUp"), icon: "list-clock" as const, run: () => openLastRun(go) },
    // …then under faults: the same command, sonde answering in the store's place, said before it runs
    ...(!terminal() && (s()?.requests ?? 0) > 0 ? [{ id: "run-try", label: t("trial.try"), icon: "flask-conical" as const, run: () => trying.set(true), disabled: !!running() }] : []),
    // the demo done: the next step is the user's own program and store
    ...(endpoint() === "demo" ? [{ id: "run-now-yours", label: t("run.nowYours"), icon: "plus" as const, tip: t("run.nowYoursHint"), run: () => (endpointsMode.set("add"), go("endpoints")) }] : []),
  ];
  const verdictText = () => {
    const c = verdict()?.check;
    if (!c) return null;
    if (c.passed) return t("run.withinBaseline");
    const v = failed()[0];
    if (!v) return t("run.pastBaseline");
    // past a limit of nothing: say how many, not "+0%"
    if (v.limit <= 0) return t("run.pastBaselineN", { what: t(`verdict.${v.what}` as never), n: Math.round(v.value) });
    // never "0% past": a sliver is "<1%" (`change` rounds as every change does)
    if (v.value <= v.limit) return t("run.pastBaseline");
    return t("run.pastBaselineBy", { what: t(`verdict.${v.what}` as never), pct: change(v.limit, v.value).slice(1, -1) });
  };
  return (
    <Card pad="none" testID={r() ? "run-ended" : "run-status"} class="shrink-0 gap-2 px-3 py-2">
      {/* what is said of the run: on one line where it fits, wrapping among itself where not; no figure is cut */}
      <View class="flex-row flex-wrap items-center gap-x-6 gap-y-2">
        <If
          when={r()}
          else={
            <View class="flex-row items-center gap-2">
              <Dot tone="live" label={t("run.running")} />
              <If when={!terminal()} else={<Strong class="tabular-nums text-foreground">{duration(props.now() - (startedAt() ?? props.now()), "counter")}</Strong>}>
                <Figure small value={duration(props.now() - (startedAt() ?? props.now()), "counter")} label={t("run.running")} />
              </If>
            </View>
          }
        >
          {(done) => (
            <View class="flex-row items-center gap-2">
              <Dot tone={done().killed ? "refused" : done().exit === 0 ? "ok" : "server"} />
              <Strong>{done().killed ? t("run.killed") : t("run.exited", { code: `${done().exit ?? "?"}` })}</Strong>
            </View>
          )}
        </If>
        <If when={s()}>
          {(x) => (
            // the kit's inline figure, one each; a terminal's one line of words
            <If
              when={!terminal()}
              else={
                <Text class="tabular-nums text-foreground">
                  {[t("run.bandRequests", { n: x().requests }), x().errors > 0 ? t("run.bandErrors", { n: x().errors }) : "", x().misses > 0 ? t("run.bandMisses", { n: x().misses }) : "", r() ? duration(x().seconds * 1000) : "", r() && warnings() > 0 ? t("run.toLook", { n: warnings() }) : ""].filter(Boolean).join(" · ")}
                </Text>
              }
            >
              <View class="flex-row flex-wrap items-center gap-x-6 gap-y-2">
                <Figure small value={count(x().requests)} label={t("figure.requests")} />
                <If when={x().errors > 0}>
                  <Figure small value={count(x().errors)} label={t("figure.errors")} tone={errorTone()} />
                </If>
                <If when={x().misses > 0}>
                  <Figure small value={count(x().misses)} label={t("figure.notFound")} />
                </If>
                <If when={r()}>
                  <Figure small value={duration(x().seconds * 1000)} label={t("figure.took")} />
                </If>
                <If when={r() && warnings() > 0}>
                  <Figure small value={count(warnings())} label={t("figure.toLook")} tone="warn" />
                </If>
              </View>
            </If>
          )}
        </If>
        <If when={verdictText()}>{(v) => <Badge testID="run-verdict" label={v()} tone={verdict()?.check?.passed ? "pass" : "fail"} />}</If>
      </View>
      <If when={trying() && r() && !r()!.removed}>
        <TrialDialog
          path={r()!.path}
          // read here first; closed after a trial, the run opens on its Runs pane, where the verdicts stay
          onClose={() => {
            trying.set(false);
            if (!tried) return;
            tried = false;
            batch(() => (paneOnOpen.set({ path: r()!.path, pane: "runs" }), openSession.set(r()!.path), go("sessions")));
          }}
          onDone={() => (tried = true)}
          // a finished scenario pressed: its own session (closing then leaves it open, not the Runs pane)
          onOpen={(session) => {
            tried = false;
            batch(() => (openSession.set(session), go("sessions")));
          }}
          // tried again what broke: the fresh run, on its Runs pane
          onAgain={(fresh) => {
            tried = false;
            batch(() => (paneOnOpen.set({ path: fresh, pane: "runs" }), openSession.set(fresh), go("sessions")));
          }}
        />
      </If>
      {/* once it ended, a row of its own: the loop's next steps by their words, then the one Run of the place */}
      <If when={r() && !terminal()}>
        <View class="flex-row items-center gap-2" onLayout={measure("next")}>
          {/* a run that sent nothing kept no session (the toast said so): it is only run again */}
          <View class="min-w-0 flex-1 flex-row items-center">
            <If when={!r()!.removed}>
              <ActionRow testID="run-actions" room={room()} actions={others} />
            </If>
          </View>
          <View class="shrink-0" onLayout={measure("primary")}>
            <Button small variant="primary" icon="play" label={same() ? t("sessions.runAgain") : t("run.start")} onPress={runNext} disabled={!!running() || (!same() && (!command().trim() || !endpoint()))} testID="run-again-band" />
          </View>
        </View>
      </If>
      <If when={r() && terminal() && !r()!.removed}>
        <ActionRow testID="run-actions" room={0} actions={others} />
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
  // the moment the pointer rests on in one of the two, marked in the other
  const focus = focusOf();
  return (
    <Chart testID="live" class="shrink-0" title={t("live.title")} text={t("live.latency")}>
      <View class="flex-row items-center gap-6">
        <Figure small testID="live-rate" value={rate(f().perSecond)} label={t("live.perSecond")} />
        <Figure small testID="live-failed" value={percent(f().failedShare)} label={t("live.failed")} tone={f().failedShare > 0 ? "bad" : "plain"} />
        <Figure small testID="live-in-flight" value={count(f().inFlight)} label={t("live.inFlight")} />
        <Figure small testID="live-p95" value={duration(f().p95)} label={t("live.p95")} />
      </View>
      <If when={shaped()}>
        <View class="flex-row gap-4">
          {/* the last 30 seconds, sliding as the run goes */}
          <SessionShape profile={props.profile} range={liveRange(props.profile)} focus={focus} class={`${STRIP} min-w-0 flex-1`} testID="run-shape" />
          <View class="min-w-0 flex-1">
            <LatencyBand profile={props.profile} domainMs={[Math.max(0, props.profile.wallMs - LIVE_WINDOW_MS), Math.max(props.profile.wallMs, 1)]} focus={focus} class={`${STRIP} w-full`} testID="run-band" />
          </View>
        </View>
      </If>
    </Chart>
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
  // once it ended: against the program's run before (its findings' states, what was fixed)
  const since = sinceLastOf(() => {
    const r = lastRun();
    return r && !r.removed && r.path ? r.path : null;
  });
  const compareBefore = () => {
    const r = lastRun();
    const before = since()?.previous;
    if (!r || !before) return;
    batch(() => (compareOnOpen.set({ before, after: r.path }), openSession.set(r.path), go("sessions")));
  };
  // its operations, in pages: the first of them as they arrive
  const operations = pagedOperations(() => path() ?? "", view);

  return (
    <View class="min-h-0 flex-1 gap-2" testID="run-operations">
      <StatusBand view={view()} now={now} />
      <If when={view() && view()!.episodesTotal > 0} else={<If when={lastRun() && view()} else={<Empty icon="activity" text={t("run.waiting")} />}>
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
          {/* once it ended: the run in a sentence, then its top findings with what each saves and the one
              figure for acting on them; a finding pressed opens the session on it, its fix open */}
          <If when={lastRun() && view()}>
            <If when={view()!.profile?.groups.length}>
              <Text testID="run-story" class="px-1 text-body leading-5 text-foreground">{sessionStory(view()!.profile)}</Text>
            </If>
            <Observations view={view()!} dense max={3} whatIfUnder savingUnder narrow states={statesOf(since())} onFilter={(i) => i !== null && openLastRun(go, i)} />
            {/* beside them, what changed since the program's run before: fixed (with what it saved), back */}
            <SinceRun since={since()} max={3} onCompare={compareBefore} />
          </If>
          <OperationsTable view={view()!} source={operations} onSelect={() => lastRun() && openLastRun(go)} compact follow={!!running()} />
        </View>
      </If>
    </View>
  );
}

/** A run that sent sonde nothing: how each common SDK is pointed at it. */
function NoRequests() {
  return (
    <View testID="run-no-requests" class={terminal() ? "gap-1" : "flex-1 items-center justify-center gap-3 p-6"}>
      <Title center={!terminal()}>{t("run.noneTitle")}</Title>
      <Text class={terminal() ? "text-muted-foreground" : `${SURFACE.note} text-center text-body text-muted-foreground`}>{t("run.noneText")}</Text>
      <Card class={SURFACE.noteCard}>
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
      <Text class={terminal() ? "text-muted-foreground" : `${SURFACE.note} text-center text-small text-muted-foreground`}>{t("run.noneKeys")}</Text>
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
      <View class={`${READING} gap-6`}>
        <View class="gap-2">
          <Title page>{t("run.title")}</Title>
          <Text class="text-body leading-5 text-muted-foreground">{t("run.hint")}</Text>
        </View>
        <If when={terminal() && history().length > 0}>
          <View class="gap-1">
            <Heading quiet class="px-1">{t("run.recent")}</Heading>
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
        <View class="flex-row">
          <Button small variant="ghost" icon="scroll-text" label={t("run.example")} onPress={example} testID="open-example" class="-ml-2" />
        </View>
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
      <If when={history().length > 0} else={<Empty compact text={t("run.recentNone")} />}>
        <Scroll class="min-h-0 flex-1" contentClass="gap-1 px-2 pb-2" scrollbar={false}>
          <Each of={history().slice(0, 30)}>
            {(c) => (
              <ListRow lines={2} label={scrubCommand(c())} selected={command() === c()} onPress={() => command.set(c())}>
                <Text class="text-body text-foreground line-clamp-1">{programOf(c()) ?? scrubCommand(c())}</Text>
                <Text class="font-mono text-caption text-muted-foreground line-clamp-1">{scrubCommand(c())}</Text>
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
    <View class="flex-1 gap-4 p-4 terminal:gap-1 terminal:p-1" testID="screen-run">
      <CommandBar />
      <If when={active()} else={<Intro />}>
        <If
          when={terminal()}
          else={
            <ResizablePanelGroup axis="horizontal" class="min-h-0 flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("run.resizeOperations")}>
              <ResizablePanel minSize={PANE.terminalLeast}>
                <View class="flex-1 pr-2">
                  <ProgramTerminal />
                </View>
              </ResizablePanel>
              <ResizablePanel size={opsWidth.size()} onResize={opsWidth.onResize} minSize={PANE.operationsLeast}>
                <View class="flex-1 pl-2">
                  <LiveOperations />
                </View>
              </ResizablePanel>
            </ResizablePanelGroup>
          }
        >
          {/* a terminal inside a terminal: the program above, its operations below */}
          <View class="flex-1 gap-1">
            <View class={SURFACE.terminalHalf}>
              <ProgramTerminal />
            </View>
            <LiveOperations />
          </View>
        </If>
      </If>
    </View>
  );
}

