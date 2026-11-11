// A trial started from the app: the session's command run again, once per
// fault scenario, sonde answering some requests in the store's place. The
// dialog says first what will run (the scenarios, where the runs write, why
// none can run), then follows the trial scenario by scenario, and ends on
// the verdicts — which the run's Runs pane keeps.

import { batch, resource, signal, Text, View } from "nsh";
import { asError, api } from "../native";
import { t } from "../i18n";
import { policy, running } from "../run";
import { report, sessionsChanged } from "../store";
import { foldTrial, trialDone, type TrialStep, trialSteps } from "../trials";
import type { Trial, TrialPlan } from "../types";
import { scenarioLabel, STANDING, trialText } from "./history";
import { Actions, Badge, Button, Card, Check, Loading, Modal, Mono, Muted, Notice, Row, Title } from "./kit";
import { scrubCommand } from "./sessionview";

/** Whether a trial is running (one at a time, as a program is). */
export const trying = signal(false);

/** "Try under faults…": the plan, the trial as it goes, its verdicts. */
export function TrialDialog(props: { path: string; onClose: () => void; /** it ended with verdicts: the run's Runs pane shows them */ onDone?: (trials: Trial[]) => void }) {
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
  const steps = signal<TrialStep[] | null>(null);
  const ended = signal<{ cancelled: boolean } | null>(null);
  const going = () => steps() !== null && !ended();
  const start = () => {
    const list = chosen();
    if (!list.length || trying()) return;
    batch(() => (steps.set(trialSteps(list)), ended.set(null), trying.set(true)));
    api.sessions
      .trial(props.path, list.map((s) => s.name), policy(), 100, 30, (e) => steps.set(foldTrial(steps() ?? [], e)))
      .then((r) => {
        batch(() => (ended.set({ cancelled: r.cancelled }), trying.set(false)));
        sessionsChanged();
        if (!r.cancelled && r.trials.length) props.onDone?.(r.trials);
      })
      .catch((e) => {
        batch(() => (steps.set(null), trying.set(false)));
        report(e);
      });
  };
  const cancel = () => {
    try {
      api.sessions.kill();
    } catch (e) {
      report(e);
    }
  };
  // closing while it runs would leave a program running unseen: Cancel first
  const close = () => !going() && props.onClose();
  const row = (s: () => TrialStep, i: () => number) => (
    <Row first={i() === 0} top testID={`trial-step-${s().scenario}`}>
      <View class="w-24 shrink-0">
        <If when={s().trial} else={<Badge label={s().state === "running" ? t("trial.running") : t("trial.waiting")} tone={s().state === "running" ? "info" : "neutral"} />}>
          {(x) => <Badge label={t(`trial.${x().standing}`)} tone={STANDING[x().standing]} />}
        </If>
      </View>
      <View class="min-w-0 flex-1 gap-1">
        <View class="flex-row flex-wrap items-baseline gap-x-2">
          <Text class="text-body text-foreground">{scenarioLabel(s().scenario)}</Text>
          <Text class="font-mono text-caption text-muted-foreground">{s().scenario}</Text>
        </View>
        <If when={s().trial}>{(x) => <Muted class="text-small">{[trialText(x()), ...x().reasons].filter(Boolean).join(" · ")}</Muted>}</If>
      </View>
    </Row>
  );
  return (
    <Modal open onClose={close} testID="trial-dialog">
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
              // before it runs: what will run, and where it writes
              <View class="gap-3" testID="trial-plan">
                <Muted class="text-small">{t("trial.dialogHint")}</Muted>
                <Mono class="line-clamp-2">{`$ ${scrubCommand(p().command)}`}</Mono>
                <Card pad="none">
                  <Each of={p().scenarios} key={(s) => s.name}>
                    {(s, i) => (
                      <Row first={i() === 0} top>
                        <View class="min-h-5 justify-center">
                          <Check testID={`trial-scenario-${s().name}`} label={scenarioLabel(s().name)} checked={!off().includes(s().name)} onChange={(on) => off.set(on ? off().filter((n) => n !== s().name) : [...off(), s().name])} />
                        </View>
                        <View class="min-w-0 flex-1 gap-1">
                          <Text class="font-mono text-caption text-muted-foreground">{s().name}</Text>
                          <Muted class="text-small">{s().about}</Muted>
                        </View>
                      </Row>
                    )}
                  </Each>
                </Card>
                <If when={p().project}>
                  <Muted class="text-small">{t("trial.project")}</Muted>
                </If>
                <If when={p().sharesPlace}>
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
                <Muted class="text-small">{ended() ? (ended()!.cancelled ? t("trial.cancelled") : t("trial.ended")) : t("trial.progress", { n: Math.min(trialDone(list()) + 1, list().length), of: list().length })}</Muted>
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
          <Button variant="destructive" icon="square" label={t("common.cancel")} onPress={cancel} testID="trial-cancel" />
        </If>
      </Actions>
    </Modal>
  );
}
