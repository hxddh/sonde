// A new run: the command bar Runs shows when no run is chosen, and what a
// run started from it needs — the program's own terminal (its output as it
// prints, keys back to it) and, for a program that sent sonde nothing, how
// each common SDK is pointed at it. Type a command and press Return: sonde
// starts it, its requests signed for the endpoint in the toolbar, and its
// run opens as the newest in the list.

import { cleanup, effect, resource, signal, Text, View, type ViewRef } from "nsh";
import { Terminal, type TerminalRef } from "nsh/base";
import { t } from "../i18n";
import { api } from "../native";
import { command, cwd, input, lastRun, onOutput, output, policy, resize, running, start, startedAt } from "../run";
import { endpoint, endpoints, report } from "../store";
import type { SuggestedCommand } from "../types";
import { Icon } from "../ui/icon";
import { Button, Card, CARD, Empty, Field, IconButton, KeyValue, popupMenu, SURFACE, TEXT, Tip, Title } from "../ui/kit";
import { scrubCommand } from "../ui/runview";
import { type Policy, RecordPanel } from "../ui/runparts";

const POLICIES = (): { value: Policy; title: string; icon: "shield-check" | "ban" | "lock" }[] => [
  { value: "allow", title: t("timeline.allow"), icon: "shield-check" },
  { value: "no-delete", title: t("timeline.noDelete"), icon: "ban" },
  { value: "read-only", title: t("timeline.readOnly"), icon: "lock" },
];

/** The last part of a folder's path: what its prompt says. */
const folderName = (dir: string) => dir.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || dir;

/** The command: the one field that matters, its Run at its end; where it runs and what it may do behind its `⋯`,
 * said at the field's start (as a shell's prompt does) only when they are not the usual. */
function CommandBar() {
  let more: ViewRef | undefined;
  // a press says why it cannot run (no command, no endpoint), never a control that does nothing
  const run = () => {
    if (!command().trim()) return report({ kind: "config", message: t("run.needCommand") });
    const id = endpoint();
    if (!id) return report({ kind: "config", message: t("run.chooseEndpoint") });
    start(command(), id);
  };
  const chosen = () => POLICIES().find((p) => p.value === policy());
  const options = () =>
    void popupMenu(
      [
        { label: t("run.cwdPick"), icon: "folder", run: () => void api.runs.pickFolder().then((f) => f && cwd.set(f), report) },
        { label: t("run.cwdHome"), checked: !cwd(), onSelect: () => cwd.set("") },
        ...(cwd() ? [{ label: cwd(), checked: true, onSelect: () => {} }] : []),
        { type: "separator" as const },
        ...POLICIES().map((p) => ({ label: p.title, checked: policy() === p.value, onSelect: () => policy.set(p.value) })),
      ],
      more,
    );
  return (
    <View class="gap-2">
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
          leading={
            <View class="flex-row items-center gap-1" tooltip={cwd() || policy() !== "allow" ? <Tip text={[cwd(), policy() !== "allow" ? chosen()?.title : ""].filter(Boolean).join(" · ")} /> : undefined}>
              <If when={policy() !== "allow" ? chosen() : null}>{(p) => <Icon name={p().icon} />}</If>
              <Text class={`font-mono ${TEXT.bodyQuiet}`}>{cwd() ? `${folderName(cwd())} $` : "$"}</Text>
            </View>
          }
          trailing={<IconButton inField ref={(r) => (more = r)} icon="ellipsis" label={t("run.options")} testID="run-options" onPress={options} />}
        />
        {/* the one primary of the place */}
        <Button variant="primary" icon="play" label={t("run.start")} shortcut="enter" onPress={run} disabled={!!running()} testID="run-start" />
      </View>
      <If when={!endpoints().some((e) => e.id === endpoint())}>
        <View class="flex-row items-center gap-2">
          <Icon name="server" />
          <Text class={`min-w-0 flex-1 ${TEXT.note} line-clamp-1`}>{t("run.chooseEndpoint")}</Text>
        </View>
      </If>
      <Suggestions />
    </View>
  );
}

/** What the chosen folder likely runs, while the field is empty: a press puts one in it (none found: nothing said). */
function Suggestions() {
  const asked = () => (running() || command().trim() ? null : cwd().trim() || null);
  const found = resource(asked, async (dir): Promise<SuggestedCommand[]> => {
    if (!dir) return [];
    try {
      return await api.runs.suggest(dir);
    } catch {
      return [];
    }
  });
  return (
    <If when={asked() && (found() ?? []).length > 0 ? found() : null}>
      {(list) => (
        <View testID="run-suggestions" class="flex-row flex-wrap items-center gap-2">
          <Text class={TEXT.meta}>{t("run.suggested")}</Text>
          <Each of={list()} key={(c) => c.command}>
            {(c, i) => <Button small variant="outline" icon="square-terminal" label={c().command} tip={t("run.suggestedFrom", { file: c().from })} onPress={() => command.set(c().command)} testID={`run-suggest-${i()}`} />}
          </Each>
        </View>
      )}
    </If>
  );
}

/** The running program's own terminal: what it printed so far, then each piece as it comes; keys go back to it. */
export function ProgramTerminal() {
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
      class={`flex-1 ${CARD} p-2`}
      testID="run-terminal"
    />
  );
}

/** A run that sent sonde nothing: how each common SDK is pointed at it (a program not found, or stopped first, says
 * that instead). */
export function NoRequests() {
  return (
    <If when={lastRun() && !lastRun()!.killed && lastRun()!.exit !== 127} else={<Empty icon="activity" text={lastRun()?.killed ? t("run.stoppedEmpty") : t("run.notFound")} />}>
      <View testID="run-no-requests" class="flex-1 items-center justify-center gap-3 p-6">
        <Title center>{t("run.noneTitle")}</Title>
        <Text class={`${SURFACE.note} text-center ${TEXT.bodyQuiet}`}>{t("run.noneText")}</Text>
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
        <Text class={`${SURFACE.note} text-center ${TEXT.note}`}>{t("run.noneKeys")}</Text>
      </View>
    </If>
  );
}

/** Runs with no run chosen: the command bar, and under it recording without a program — or, after a run that sent
 * nothing and was not kept, why. */
export function NewRun() {
  const recording = signal(api.runs.current());
  // the last run sent nothing: it was not kept, and what to point at sonde is said here
  const empty = () => !running() && !!lastRun()?.removed && !lastRun()?.deleted;
  return (
    <View class="flex-1" testID="run-new">
      <View class="shrink-0 px-4 pt-4">
        <CommandBar />
      </View>
      <If when={empty()} else={<View class="p-4"><RecordPanel recording={recording()} onChange={() => recording.set(api.runs.current())} policy={policy} /></View>}>
        <NoRequests />
      </If>
    </View>
  );
}
