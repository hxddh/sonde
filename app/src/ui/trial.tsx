// A trial started from the app: the run's command run again, once per
// fault scenario, sonde answering some requests in the store's place. The
// dialog says first what will run (each scenario and how hard it presses,
// what the program may do, where the runs write, why none can run) and
// closes as it starts: the run's History follows it scenario by scenario,
// and keeps the verdicts; what did not survive is a finding of the run.

import { announce, batch, resource, signal, Text, View } from "nsh";
import { asError, api } from "../native";
import { duration } from "../format";
import { t } from "../i18n";
import { running, stop } from "../run";
import type { Policy } from "./runparts";
import { report, runsChanged } from "../store";
import { foldTrial, scenarioAbout, scenarioLabel, type TrialStep, trialSteps } from "../trials";
import type { Confirmed, TrialPlan } from "../types";
import { Actions, Button, Card, Check, Labeled, Loading, Modal, ModalBody, Muted, Notice, Options, Path, Prose, Row, Title, TEXT } from "./kit";
import { ExactCommand } from "./external";
import { exactText } from "./runview";

/** The trial under way, or the one that just ended, on the run it tries: its scenarios as they go
 * (followed on that run's History), when it started and ended. One at a time, as a program is. */
export const liveTrial = signal<{ path: string; steps: TrialStep[]; started: number; ended: number | null } | null>(null);
/** How many trials have ended with their verdicts kept: a run's view is read again (its findings take them in). */
export const trialsKept = signal(0);
/** Whether a trial is running. */
export const trying = () => !!liveTrial() && liveTrial()!.ended === null;
/** Now, a second at a time while a trial runs: what its elapsed time is reckoned from. */
export const trialNow = signal(0);
let clock: ReturnType<typeof setInterval> | undefined;

/** Tries `path`'s program (`command`) under `scenarios` (each as asked: `slowdown:3`), the program limited by
 * `policy`: its verdicts fill the run's History as they come; ended, they are kept there. At work as a program is
 * (one at a time, the toolbar's pill, Stop in the palette). `confirmed`: for a run from elsewhere, the command and
 * folder the dialog showed whole, which starting it there says yes to (Rust refuses its trial without). */
export function startTrial(path: string, command: string, scenarios: { name: string; about: string }[], asked: string[], policy: Policy, confirmed: Confirmed | null = null) {
  if (running() || !scenarios.length) return;
  const started = Date.now();
  batch(() => (running.set({ kind: "trial", command, path }), liveTrial.set({ path, steps: trialSteps(scenarios), started, ended: null }), trialNow.set(started)));
  clearInterval(clock);
  clock = setInterval(() => trialNow.set(Date.now()), 1000);
  const end = () => {
    clearInterval(clock);
    clock = undefined;
    running.set(null);
  };
  api.runs
    .trial(path, asked, policy, 100, 30, confirmed, (e) => {
      const l = liveTrial();
      if (l?.path === path) liveTrial.set({ ...l, steps: foldTrial(l.steps, e) });
      // a verdict as it arrives, told to a screen reader: the pane's text alone changes in silence
      if ("state" in e && e.state === "done" && e.trial) announce(`${scenarioLabel(e.scenario)}: ${t(`trial.${e.trial.standing}`)}`);
    })
    .then((r) => {
      end();
      const l = liveTrial();
      const at = Date.now();
      // the History reads the kept verdicts (and the run its findings) again; until they are in, it shows these.
      // Stopped, nothing was kept: the History shows what it had
      runsChanged();
      batch(() => (trialNow.set(at), liveTrial.set(r.cancelled || !l ? null : { ...l, ended: at }), !r.cancelled && trialsKept.set(trialsKept() + 1)));
      announce(r.cancelled ? t("trial.cancelled") : t("trial.endedSaid", { t: duration(Math.max(0, at - started)) }));
    })
    .catch((e) => {
      end();
      liveTrial.set(null);
      report(e);
    });
}

/** Stops the trial: it ends the program, and the trial keeps nothing. */
export const stopTrial = stop;

/** What the program may do while it is tried: the same three limits a run has. */
const LIMITS = (): { value: Policy; title: string; icon: "shield-check" | "ban" | "lock" }[] => [
  { value: "allow", title: t("timeline.allow"), icon: "shield-check" },
  { value: "no-delete", title: t("timeline.noDelete"), icon: "ban" },
  { value: "read-only", title: t("timeline.readOnly"), icon: "lock" },
];

/** "Try under faults…": what will run, then (started) the run's History follows it. */
export function TrialDialog(props: {
  path: string;
  onClose: () => void;
  /** the trial started: the run's History follows it */
  onStarted?: () => void;
}) {
  // what would run, or why nothing can: read once, as the dialog opens
  const plan = resource(
    () => props.path,
    async (p): Promise<{ plan: TrialPlan } | { refused: string }> => {
      try {
        return { plan: api.runs.trialPlan(p) };
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
  // what the program may do while tried: no deletes unless changed here (a trial's own
  // faults could push a program down a path it would not take)
  const limit = signal<Policy>("no-delete");
  const start = () => {
    const list = chosen();
    const p = the();
    if (!p || !list.length || running()) return;
    // a run from elsewhere: Start is the yes to the command and folder shown above it, whole
    startTrial(props.path, p.command, list, list.map(asked), limit(), p.external ? { command: p.command, cwd: p.cwd } : null);
    props.onClose();
    props.onStarted?.();
  };
  return (
    // as wide as a dialog that lists, as high as the window allows, its body scrolling inside
    <Modal open steady size="wide" onClose={props.onClose} testID="trial-dialog" describe={refused() ?? t("trial.dialogHint")}>
      <Title>{t("trial.dialogTitle")}</Title>
      <If when={plan.loading() && !plan()}>
        <Loading />
      </If>
      {/* no trial of this run: why, and nothing to press but Close */}
      <If when={refused()}>{(why) => <Notice testID="trial-refused" tone="warn" text={why()} />}</If>
      {/* (no body to fill the frame: the actions stay at its foot) */}
      <If when={!the()}>
        <View class="flex-1" />
      </If>
      <If when={the()}>
        {(p) => (
          // what will run, how hard each scenario presses, what the program may do
          <ModalBody testID="trial-plan">
            <Muted>{t("trial.dialogHint")}</Muted>
            {/* what will run, as it will run (nothing of it hidden: it stands over the button that starts it): on one
                line, cut in its middle; of a run from elsewhere whole, with its folder, and said to be someone else's */}
            <If when={p().external} else={<Path path={`$ ${exactText(p().command)}`} />}>
              <View class="gap-3">
                <Notice testID="trial-external" tone="warn" text={t("external.trial")} />
                <ExactCommand command={p().command} cwd={p().cwd} />
              </View>
            </If>
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
                      <Text class={`shrink-0 ${TEXT.codeQuiet}`}>{asked(s())}</Text>
                    </View>
                    <Muted class="pl-6 text-small">{scenarioAbout(s().name, s().about, p().project, pressed(s()))}</Muted>
                  </Row>
                )}
              </Each>
            </Card>
            <If when={p().project}>
              <Prose class={TEXT.bodyQuiet} text={t("trial.project")} />
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
            <If when={p().sharesPlace}>
              <Notice testID="trial-shares" tone="warn" text={t("trial.sharesPlace")} />
            </If>
            <If when={!!running()}>
              <Notice tone="info" text={t("trial.busy")} />
            </If>
          </ModalBody>
        )}
      </If>
      <Actions>
        {/* refused, the footer's primary is Close */}
        <Button variant={refused() ? "primary" : "ghost"} label={refused() ? t("common.close") : t("common.cancel")} onPress={props.onClose} testID="trial-close" />
        <If when={the()}>
          <Button variant="primary" icon="play" label={t("trial.run", { n: chosen().length })} disabled={!chosen().length || !!running()} onPress={start} testID="trial-start" />
        </If>
      </Actions>
    </Modal>
  );
}
