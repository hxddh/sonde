// Run: the command bar and the program it runs. Type a command and press
// Return: sonde starts it in a terminal, its requests signed for the
// endpoint in the toolbar; its operations appear beside it as they happen,
// over a band of live figures. When it ends, the run's own view takes the
// place (its header and panes, opened on its Output), as Runs shows it.

import { cleanup, effect, LayoutAnimation, reduceMotion, resource, Scroll, signal, Text, untracked, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup, Terminal, type TerminalRef } from "nsh/base";
import { count, duration, percent, rate } from "../format";
import { t } from "../i18n";
import { analysis } from "../analysis";
import { pagedOperations } from "../paged";
import { api } from "../native";
import { command, cwd, history, input, lastRun, onOutput, output, policy, resize, running, start, startedAt, stop } from "../run";
import { away, endpoint, endpoints, go, openRun, report, runsTick, showInSidebar } from "../store";
import type { RunInfo, RunView, Stretch } from "../types";
import { Icon } from "../ui/icon";
import { Button, Card, CARD, Dot, Empty, Field, Figure, Heading, KeyValue, ListRow, Notice, Options, READING, SideHeader, Strong, terminal, SPLIT_GAP, Title, SURFACE, WIDTH, PANE, TEXT } from "../ui/kit";
import { programOf, scrubCommand } from "../ui/runview";
import { type Policy, OperationsTable, RecordPanel } from "../ui/runparts";
import { Chart, focusOf, LatencyBand, liveFigures, liveSaid, RunShape, STRIP } from "../ui/plots";
import { rememberedSize } from "../ui/sizes";
import { RunPane } from "./Runs";

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
      {/* the command: the kit's field at the kit's height, the one that matters by its width and its mono, with
          what runs it at its end; it rests at its start (a long command shows what it runs, not its last arguments) */}
      <View class="flex-row items-center gap-2">
        <Field
          bare
          mono
          rest
          class="min-w-0 flex-1"
          testID="run-command"
          label={t("run.command")}
          value={command()}
          placeholder={t("run.placeholder")}
          onChange={(v) => command.set(v)}
          onSubmit={run}
          leading={<Text class={`font-mono ${TEXT.bodyQuiet}`}>$</Text>}
        />
        {/* the one primary of the place: Run, Stop while it runs */}
        <If when={running()} else={<Button variant="primary" icon="play" label={t("run.start")} shortcut="enter" onPress={run} disabled={!command().trim() || !endpoint()} testID="run-start" />}>
          <Button variant="destructive" icon="square" label={t("run.stop")} onPress={stop} testID="run-stop" />
        </If>
      </View>
      {/* where it runs and what it may do: one row, one height (where its requests go is the title bar's
          endpoint, said once; only its absence is said here) */}
      <View class="flex-row flex-wrap items-center gap-x-3 gap-y-2">
        <Field bare small mono class={WIDTH.long} testID="run-cwd" label={t("run.cwd")} value={cwd()} placeholder={t("run.cwdHome")} onChange={(v) => cwd.set(v)} leading={<Icon name="folder" />} />
        <View class="min-w-0 flex-1 flex-row items-center gap-2">
          <If when={!ep()}>
            <Icon name="server" />
            <Text class={`min-w-0 flex-1 ${TEXT.note} line-clamp-1`}>{t("run.chooseEndpoint")}</Text>
          </If>
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
  // a run starts on a clean terminal: the one before is its run's now
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

/** The run's figures as they change: running for, requests, errors, not found; once it ended having sent
 * nothing to show, how it ended. */
function StatusBand(props: { view: RunView | null; now: () => number }) {
  const s = () => props.view?.summary;
  const r = () => lastRun();
  // the errors in the tone their rows are drawn in: the client's where every one was a 4xx
  const errorTone = () => {
    const c = props.view?.signals?.errors.counts;
    return c && c.client > 0 && c.server + c.transport === 0 ? "client" : "fail";
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
              <Dot tone={done().killed ? "refused" : done().exit === 0 ? "ok" : "fail"} />
              <Strong>{done().killed ? t("run.killed") : t("run.exited", { code: `${done().exit ?? "?"}` })}</Strong>
            </View>
          )}
        </If>
        <If when={s()}>
          {(x) => (
            // the kit's inline figure, one each; a terminal's one line of words
            <If
              when={!terminal()}
              else={<Text class="tabular-nums text-foreground">{[t("run.bandRequests", { n: x().requests }), x().errors > 0 ? t("run.bandErrors", { n: x().errors }) : "", x().misses > 0 ? t("run.bandMisses", { n: x().misses }) : ""].filter(Boolean).join(" · ")}</Text>}
            >
              <View class="flex-row flex-wrap items-center gap-x-6 gap-y-2">
                <Figure small value={count(x().requests)} label={t("figure.requests")} />
                <If when={x().errors > 0}>
                  <Figure small value={count(x().errors)} label={t("figure.errors")} tone={errorTone()} />
                </If>
                <If when={x().misses > 0}>
                  <Figure small value={count(x().misses)} label={t("figure.notFound")} />
                </If>
              </View>
            </If>
          )}
        </If>
      </View>
    </Card>
  );
}

/** The run as it goes, from its last half minute alone (`live`: the requests that started in it, reckoned by
 * themselves, never the whole run at each tick): requests a second, failures, in flight and the p95 now; requests
 * over time beside how long they take, in the window's own slices. */
function LivePanel(props: { live: Stretch }) {
  const p = () => props.live.profile;
  const f = () => liveFigures(p());
  // the first requests have no shape yet: the figures alone, the charts once there is one
  const shaped = () => p().rate.length > 1 && p().samples.length >= 12;
  // the moment the pointer rests on in one of the two, marked in the other
  const focus = focusOf();
  return (
    <Chart testID="live" class="shrink-0" title={t("live.title")} text={liveSaid(props.live.signals)} how={t("live.hint")}>
      <View class="flex-row items-center gap-6">
        <Figure small testID="live-rate" value={rate(f().perSecond)} label={t("live.perSecond")} />
        <Figure small testID="live-failed" value={percent(f().failedShare)} label={t("live.failed")} tone={f().failedShare > 0 ? "fail" : "plain"} />
        <Figure small testID="live-in-flight" value={count(f().inFlight)} label={t("live.inFlight")} />
        <Figure small testID="live-p95" value={duration(f().p95)} label={t("live.p95")} />
      </View>
      <If when={shaped()}>
        <View class="flex-row gap-4">
          <RunShape profile={p()} focus={focus} class={`${STRIP} min-w-0 flex-1`} testID="live-shape" />
          <View class="min-w-0 flex-1">
            <LatencyBand profile={p()} focus={focus} class={STRIP} testID="run-band" />
          </View>
        </View>
      </If>
    </Chart>
  );
}

/** The program's requests as they become operations, over a band of live figures; once it ended having sent
 * nothing to show, why (a run that did shows its own view). */
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
  // the live panel's half minute, read with each tick while the program runs (the last reading kept meanwhile)
  const lastHalf = resource(
    () => (running() ? `${path()}\n${tick()}` : null),
    async () => {
      const p = path();
      return p ? await api.runs.live(p) : null;
    },
  );
  let held: Stretch | null = null;
  const live = () => (lastHalf() ? (held = lastHalf()!) : held);
  // its operations, in pages: the first of them as they arrive
  const operations = pagedOperations(() => path() ?? "", view);
  return (
    <View class="min-h-0 flex-1 gap-2" testID="run-operations">
      <StatusBand view={view()} now={now} />
      <If
        when={view() && view()!.operationsTotal > 0}
        else={
          <If when={lastRun() && view()} else={<Empty icon="activity" text={t("run.waiting")} />}>
            {/* the SDK hints only for a program that ran and still sent nothing */}
            <If when={!lastRun()!.killed && lastRun()!.exit !== 127} else={<Empty icon="activity" text={lastRun()!.killed ? t("run.stoppedEmpty") : t("run.notFound")} />}>
              <NoRequests />
            </If>
          </If>
        }
      >
        <View class="min-h-0 flex-1 gap-2">
          {/* while it runs: its requests over time, growing as they arrive */}
          <If when={!terminal() && !lastRun() ? live() : null}>{(l) => <LivePanel live={l()} />}</If>
          <OperationsTable view={view()!} source={operations} onSelect={() => {}} compact follow={!!running()} />
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
      <Text class={terminal() ? "text-muted-foreground" : `${SURFACE.note} text-center ${TEXT.bodyQuiet}`}>{t("run.noneText")}</Text>
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
      <Text class={terminal() ? "text-muted-foreground" : `${SURFACE.note} text-center ${TEXT.note}`}>{t("run.noneKeys")}</Text>
    </View>
  );
}

/** Before the first run: what Run does, and recording without a program (a terminal, which has no sidebar,
 * lists the commands run before here). */
function Intro() {
  const recording = signal(api.runs.current());
  return (
    <View class="flex-1 items-center justify-center p-6" testID="run-intro">
      <View class={`${READING} gap-6`}>
        <View class="gap-2">
          <Title page>{t("run.title")}</Title>
          <Text class={`${TEXT.bodyQuiet} leading-5`}>{t("run.hint")}</Text>
        </View>
        <If when={terminal() && history().length > 0}>
          <View class="gap-1">
            <Heading quiet class="px-1">{t("run.recent")}</Heading>
            <Each of={history().slice(0, 5)}>
              {(c) => (
                <ListRow label={scrubCommand(c())} onPress={() => command.set(c())}>
                  <Text class={`flex-1 font-mono ${TEXT.small} line-clamp-1`}>{scrubCommand(c())}</Text>
                </ListRow>
              )}
            </Each>
          </View>
        </If>
        <RecordPanel recording={recording()} onChange={() => recording.set(api.runs.current())} policy={policy} />
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
                <Text class={`${TEXT.body} line-clamp-1`}>{programOf(c()) ?? scrubCommand(c())}</Text>
                <Text class={`${TEXT.codeQuiet} line-clamp-1`}>{scrubCommand(c())}</Text>
              </ListRow>
            )}
          </Each>
        </Scroll>
      </If>
    </View>
  );
}

/** The program and its operations side by side while it runs (and when it ended having sent nothing). */
function Live() {
  // once a program ended with nothing to show, its output yields room to why
  const done = () => running() === null && lastRun() !== null;
  return (
    <If
      when={terminal()}
      else={
        <ResizablePanelGroup axis="horizontal" class="min-h-0 flex-1" handleClass={SPLIT_GAP} handleAccessibilityLabel={t("run.resizeOperations")}>
          <ResizablePanel minSize={done() ? PANE.terminalDone : PANE.terminalLeast}>
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
  );
}

export function Run() {
  // the run that ended, while it is kept: its own view, as Runs shows it
  const ended = () => {
    const r = lastRun();
    return running() === null && r && !r.removed && r.path ? r.path : null;
  };
  const read = analysis(ended).view;
  const shown = () => (ended() && (read()?.summary.requests ?? 0) > 0 ? ended() : null);
  // the runs, for the view's Compare… and its name: read again as they change
  const runs = signal<RunInfo[]>([]);
  effect(() => {
    runsTick();
    untracked(() => {
      try {
        runs.set(api.runs.list());
      } catch {
        // none yet
      }
    });
  });
  const active = () => running() !== null || (lastRun() !== null && !lastRun()!.deleted);
  if (!terminal()) cleanup(showInSidebar("run", () => <Recent />));
  return (
    <View class="flex-1" testID="screen-run">
      <View class="shrink-0 px-4 pt-4 terminal:p-1">
        <CommandBar />
      </View>
      <If when={active()} else={<Intro />}>
        <If
          when={shown()}
          else={
            <View class="min-h-0 flex-1 p-4 terminal:p-1">
              <Live />
            </View>
          }
        >
          {/* one view a run: the pane keyed by its run, so another run gets a fresh one */}
          <Each of={[shown()!]} key={(p) => p}>
            {(p) => (
              <RunPane
                path={p()}
                others={runs}
                tick={() => 0}
                inRun
                // how the program ended, where that is news: stopped, or an exit other than 0
                note={
                  <If when={lastRun() && (lastRun()!.killed || lastRun()!.exit !== 0) ? lastRun() : null}>
                    {(r) => <Notice testID="run-exit" tone="warn" text={r().killed ? t("run.stoppedNote") : t("run.exitNote", { code: `${r().exit ?? "?"}` })} />}
                  </If>
                }
                onOpen={(other) => {
                  openRun.set(other);
                  go("runs");
                }}
              />
            )}
          </Each>
        </If>
      </If>
    </View>
  );
}
