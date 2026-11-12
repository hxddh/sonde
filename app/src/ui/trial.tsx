// A trial started from the app: the session's command run again, once per
// fault scenario, sonde answering some requests in the store's place. The
// dialog says first what will run (each scenario and how hard it presses,
// what the program may do, where the runs write, why none can run), then
// follows the trial scenario by scenario with the time it has taken, and
// ends on the verdicts — each a way into its own session, and all kept on
// the run's Runs pane.

import { announce, batch, cleanup, resource, signal, Text, View } from "nsh";
import { asError, api } from "../native";
import { duration } from "../format";
import { t } from "../i18n";
import { policy, running } from "../run";
import type { Policy } from "./session";
import { report, sessionsChanged } from "../store";
import { foldTrial, scenarioAbout, scenarioLabel, trialDone, type TrialStep, trialSteps } from "../trials";
import type { Trial, TrialPlan } from "../types";
import { STANDING, trialAsked, TrialWhere, trialReasons, trialText } from "./history";
import { Icon } from "./icon";
import { Actions, Badge, Button, Card, Check, ICON, Labeled, ListRow, Loading, Modal, Mono, Muted, Notice, Options, Row, Title } from "./kit";
import { scrubCommand } from "./sessionview";

/** Whether a trial is running (one at a time, as a program is). */
export const trying = signal(false);

/** What the program may do while it is tried: the same three limits a run has. */
const LIMITS = (): { value: Policy; title: string; icon: "shield-check" | "ban" | "lock" }[] => [
  { value: "allow", title: t("timeline.allow"), icon: "shield-check" },
  { value: "no-delete", title: t("timeline.noDelete"), icon: "ban" },
  { value: "read-only", title: t("timeline.readOnly"), icon: "lock" },
];

/** "Try under faults…": the plan, the trial as it goes, its verdicts. */
export function TrialDialog(props: { path: string; onClose: () => void; /** it ended with verdicts: the run's Runs pane shows them */ onDone?: (trials: Trial[]) => void; /** a finished scenario pressed: its own session */ onOpen?: (session: string) => void }) {
  // what would run, or why nothing can: read once, as the dialog opens
  const plan = resource(
    () => props.path,
    async (p): Promise<{ plan: TrialPlan } | { refused: string }> => {
      try {
        return { plan: api.sessions.trialPlan(p) };
      } catch (e) {
        return { refused: asError(e).message };
      }
    },
  );
  const the = () => {
    const x = plan();
    return x && "plan" in x ? x.plan : null;
  };
  const refused = () => {
    const x = plan();
    return x && "refused" in x ? x.refused : null;
  };
  // every scenario is on until one is taken off
  const off = signal<string[]>([]);
  const chosen = () => (the()?.scenarios ?? []).filter((s) => !off().includes(s.name));
  // how hard the two that fail a request's first attempt press: its first n attempts, 1 to the
  // most the plan allows (5, as `--fault slowdown:N` takes)
  const times = signal(1);
  const most = () => Math.max(1, ...(the()?.scenarios ?? []).map((s) => s.maxTimes ?? 1));
  const pressed = (s: { name: string; maxTimes?: number | null }) => (s.maxTimes && times() > 1 ? Math.min(times(), s.maxTimes) : 1);
  const asked = (s: { name: string; maxTimes?: number | null }) => (pressed(s) > 1 ? `${s.name}:${pressed(s)}` : s.name);
  // what the program may do while tried: as the last run was limited, until changed here
  const limit = signal<Policy>(policy());
  const steps = signal<TrialStep[] | null>(null);
  const ended = signal<{ cancelled: boolean } | null>(null);
  const going = () => steps() !== null && !ended();
  // how long the trial has taken: a second at a time while it runs, then as it ended
  const started = signal(0);
  const now = signal(0);
  const clock = setInterval(() => going() && now.set(Date.now()), 1000);
  cleanup(() => clearInterval(clock));
  const took = () => duration(Math.max(0, now() - started()), "counter");
  const start = () => {
    const list = chosen();
    if (!list.length || trying()) return;
    batch(() => (steps.set(trialSteps(list)), ended.set(null), trying.set(true), started.set(Date.now()), now.set(Date.now())));
    api.sessions
      .trial(props.path, list.map(asked), limit(), 100, 30, (e) => {
        steps.set(foldTrial(steps() ?? [], e));
        // a verdict as it arrives, told to a screen reader: the dialog's text alone changes in silence
        if ("state" in e && e.state === "done" && e.trial) announce(`${scenarioLabel(e.scenario)}: ${t(`trial.${e.trial.standing}`)}`);
      })
      .then((r) => {
        batch(() => (now.set(Date.now()), ended.set({ cancelled: r.cancelled }), trying.set(false)));
        announce(r.cancelled ? t("trial.cancelled") : t("trial.endedSaid", { t: duration(Math.max(0, now() - started())) }));
        sessionsChanged();
        if (!r.cancelled && r.trials.length) props.onDone?.(r.trials);
      })
      .catch((e) => {
        batch(() => (steps.set(null), trying.set(false)));
        report(e);
      });
  };
  // stopping is not dismissing: it ends the program, and the trial keeps nothing
  const stop = () => {
    try {
      api.sessions.kill();
    } catch (e) {
      report(e);
    }
  };
  // closing while it runs would leave a program running unseen: Stop first
  const close = () => !going() && props.onClose();
  // the session first, then the dialog closed: what closing does after a trial (the run's Runs pane) gives way to it
  const open = (session: string) => {
    props.onOpen?.(session);
    props.onClose();
  };
  const standing = (s: () => TrialStep) => (
    <View class="min-h-5 shrink-0 justify-center">
      <If when={s().trial} else={<Badge label={s().state === "running" ? t("trial.running") : t("trial.waiting")} tone={s().state === "running" ? "info" : "neutral"} />}>
        {(x) => <Badge label={t(`trial.${x().standing}`)} tone={STANDING[x().standing]} />}
      </If>
    </View>
  );
  const row = (s: () => TrialStep, i: () => number) => {
    const session = () => (ended() && !ended()!.cancelled ? (s().trial?.session ?? null) : null);
    return (
      <ListRow
        divided
        top
        first={i() === 0}
        testID={`trial-step-${s().scenario}`}
        label={s().trial ? `${t(`trial.${s().trial!.standing}`)}: ${scenarioLabel(s().scenario)}` : scenarioLabel(s().scenario)}
        // done, and its session kept: a press opens it
        disabled={!session() || !props.onOpen}
        onPress={() => session() && open(session()!)}
        trailing={
          <>
            {standing(s)}
            <If when={session() && props.onOpen}>
              <Icon name="chevron-right" size={ICON.small} />
            </If>
          </>
        }
      >
        <View class="min-w-0 flex-1 gap-1">
          <View class="flex-row flex-wrap items-baseline gap-x-2">
            <Text class="text-body text-foreground">{scenarioLabel(s().scenario)}</Text>
            <Text class="font-mono text-caption text-muted-foreground">{s().trial ? trialAsked(s().trial!) : s().scenario}</Text>
          </View>
          <If when={s().trial}>
            {(x) => (
              <>
                <Muted class="text-small">{[trialText(x()), ...trialReasons(x())].filter(Boolean).join(" · ")}</Muted>
                <If when={x().standing !== "survived"}>
                  <TrialWhere trial={x()} />
                </If>
              </>
            )}
          </If>
        </View>
      </ListRow>
    );
  };
  return (
    <Modal open onClose={close} testID="trial-dialog" describe={refused() ?? (steps() ? (ended() ? (ended()!.cancelled ? t("trial.cancelled") : t("trial.ended")) : undefined) : t("trial.dialogHint"))}>
      <Title>{t("trial.dialogTitle")}</Title>
      <If when={plan.loading() && !plan()}>
        <Loading />
      </If>
      {/* no trial of this session: why, and nothing to press but Close */}
      <If when={refused()}>{(why) => <Notice testID="trial-refused" tone="warn" text={why()} />}</If>
      <If when={the()}>
        {(p) => (
          <If
            when={steps()}
            else={
              // before it runs: what will run, how hard each scenario presses, what the program may do
              <View class="gap-3" testID="trial-plan">
                <Muted class="text-small">{t("trial.dialogHint")}</Muted>
                <Mono class="line-clamp-2">{`$ ${scrubCommand(p().command)}`}</Mono>
                <Card pad="none">
                  <Each of={p().scenarios} key={(s) => s.name}>
                    {(s, i) => (
                      // its name beside its check, its id at the row's end; under them, at the name's
                      // own left edge in every row, what it does and how often
                      <Row stack first={i() === 0}>
                        <View class="flex-row items-center gap-2">
                          <View class="min-w-0 flex-1">
                            <Check testID={`trial-scenario-${s().name}`} label={scenarioLabel(s().name)} checked={!off().includes(s().name)} onChange={(on) => off.set(on ? off().filter((n) => n !== s().name) : [...off(), s().name])} />
                          </View>
                          <Text class="shrink-0 font-mono text-caption text-muted-foreground">{asked(s())}</Text>
                        </View>
                        <Muted class="pl-6 text-small">{scenarioAbout(s().name, s().about, p().project, pressed(s()))}</Muted>
                      </Row>
                    )}
                  </Each>
                </Card>
                <If when={p().project}>
                  <Muted class="text-small">{t("trial.project")}</Muted>
                </If>
                {/* side by side where there is room: the least window is no taller than the dialog with each on a row of its own */}
                <View class="flex-row flex-wrap gap-x-6 gap-y-3">
                  {/* how hard: one number, for the scenarios it means one thing for; a project's own say theirs in sonde.toml */}
                  <If when={p().scenarios.some((s) => s.maxTimes)}>
                    <Labeled label={t("trial.strength")}>
                      <Options testID="trial-times" value={`${times()}`} items={Array.from({ length: most() }, (_, i) => i + 1).map((n) => ({ value: `${n}`, title: t("trial.times", { n }) }))} onChange={(v) => times.set(Number(v))} />
                    </Labeled>
                  </If>
                  {/* the limit on what the program does while tried: its own choice here */}
                  <Labeled label={t("trial.limit")}>
                    <Options testID="trial-limit" value={limit()} items={LIMITS()} onChange={(v) => limit.set(v as Policy)} />
                  </Labeled>
                </View>
                {/* nothing runs while sonde is read-only: that is said alone (both notices are taller than the least window) */}
                <If when={p().sharesPlace && !p().readOnly}>
                  <Notice testID="trial-shares" tone="warn" text={t("trial.sharesPlace")} />
                </If>
                <If when={p().readOnly}>
                  <Notice testID="trial-read-only" tone="warn" text={t("trial.readOnly")} />
                </If>
                <If when={!!running()}>
                  <Notice tone="info" text={t("trial.busy")} />
                </If>
              </View>
            }
          >
            {(list) => (
              // as it goes, and once it ended: a row a scenario, its standing when it has one
              <View class="gap-3" testID="trial-progress">
                <View class="flex-row items-center gap-2">
                  <Muted class="min-w-0 flex-1 text-small">{ended() ? (ended()!.cancelled ? t("trial.cancelled") : t("trial.ended")) : t("trial.progress", { n: Math.min(trialDone(list()) + 1, list().length), of: list().length })}</Muted>
                  <Text testID="trial-elapsed" class="shrink-0 tabular-nums text-small text-muted-foreground">{took()}</Text>
                </View>
                <Card pad="none">
                  <Each of={list()} key={(s) => s.scenario}>
                    {(s, i) => row(s, i)}
                  </Each>
                </Card>
              </View>
            )}
          </If>
        )}
      </If>
      <Actions>
        <If
          when={going()}
          else={
            <>
              <Button variant={steps() || refused() ? "outline" : "ghost"} label={steps() || refused() ? t("common.close") : t("common.cancel")} onPress={props.onClose} testID="trial-close" />
              <If when={the() && !steps()}>
                <Button variant="primary" icon="play" label={t("trial.run", { n: chosen().length })} disabled={!chosen().length || !!the()?.readOnly || !!running() || trying()} onPress={start} testID="trial-start" />
              </If>
            </>
          }
        >
          {/* its own word: Stop ends the program; nothing dismisses the dialog while it runs */}
          <Button variant="destructive" icon="square" label={t("trial.stop")} onPress={stop} testID="trial-cancel" />
        </If>
      </Actions>
    </Modal>
  );
}
