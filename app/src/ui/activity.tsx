// Activity: what sonde itself does — its jobs, and the changes it
// made (Changes). One place for "what is sonde doing, what did it
// do": a sheet from the window's edge, opened from the toolbar; a place of
// its own in a terminal.

import { cleanup, effect, Scroll, signal, Text, untracked, View, type ViewRef } from "nsh";
import { count, date, duration, since } from "../format";
import { t } from "../i18n";
import { core } from "../phrases";
import { api } from "../native";
import { ACTIVITY_TABS, type ActivityTab, activityTab } from "../store";
import type { AuditEntry, JobInfo } from "../types";
import { Badge, ProgressBar, Button, Card, Empty, IconButton, PanelHeader, terminal, Row, Segmented, type Tone, TEXT } from "./kit";

export const activityOpen = signal(false);

/** Jobs running now, for the toolbar's badge. */
export const runningJobs = signal(0);
let following = false;
export function watchJobs() {
  if (following) return;
  following = true;
  try {
    // the engine says when the jobs change
    api.on("jobs", (n) => n !== runningJobs() && runningJobs.set(n));
  } catch {
    // the engine is gone
  }
}

/** `f`, at most every `ms` while calls keep coming (the last one wins). */
function throttled(f: () => void, ms: number): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let last = 0;
  cleanup(() => timer && clearTimeout(timer));
  return () => {
    if (timer) return;
    const wait = Math.max(0, last + ms - Date.now());
    timer = setTimeout(() => {
      timer = undefined;
      last = Date.now();
      f();
    }, wait);
  };
}

const TAB_LABEL: Record<ActivityTab, () => string> = { jobs: () => t("activity.jobs"), audit: () => t("activity.changes") };

const JOB_TONE: Record<JobInfo["state"], Tone> = { running: "live", finished: "ok", failed: "fail", cancelled: "quiet" };

/** A row of Activity's lists: hairlines between them, in one card. */

function Jobs() {
  const jobs = signal<JobInfo[]>(api.jobs());
  // the engine says when a job starts, moves on or ends
  const refresh = throttled(() => jobs.set(api.jobs()), 200);
  cleanup(api.on("jobs", refresh));
  // "3 min ago" follows the clock while the panel is open
  const now = signal(Date.now());
  const clockTick = setInterval(() => now.set(Date.now()), 30000);
  cleanup(() => clearInterval(clockTick));
  const age = (started: string) => (now(), duration(since(started), "age"));
  return (
    <If when={jobs().length > 0} else={<Empty icon="timer" text={t("jobs.none")} />}>
      <Scroll class="flex-1" contentClass="pb-4">
        <Card pad="none">
          <Each of={jobs()} key={(j) => j.id}>
            {(j, i) => (
              <Row stack first={i() === 0}>
                <View class="min-h-7 flex-row items-center gap-2">
                  <Text class={`min-w-0 flex-1 ${TEXT.body} line-clamp-1`}>{core(j().titleText, j().title)}</Text>
                  {/* a job done says nothing more: only one at work, failed or cancelled says how it stands */}
                  <If when={j().state !== "finished"}>
                    <Badge tone={JOB_TONE[j().state] ?? "quiet"} label={t(`jobs.state.${j().state}` as never)} />
                  </If>
                  <Text class={`shrink-0 ${TEXT.meta}`}>{age(j().started)}</Text>
                  <If when={j().state === "running"}>
                    <Button small variant="outline" label={t("jobs.cancel")} onPress={() => api.cancelJob(j().id)} />
                  </If>
                </View>
                <If when={j().state === "running"}>
                  <ProgressBar value={j().progress.total ? j().progress.done / Math.max(1, j().progress.total!) : null} label={core(j().progress.messageText, j().progress.message)} />
                </If>
                <If when={j().error}>{(e) => <Text class={TEXT.error}>{e().message}</Text>}</If>
              </Row>
            )}
          </Each>
        </Card>
      </Scroll>
    </If>
  );
}

/** What a change did, in words: its action's name here, else as the engine names it. */
function actionLabel(action: string): string {
  const key = `audit.action.${action}`;
  const said = t(key as never);
  return said === key ? action : said;
}

function Changes() {
  // read again each time the tab is shown: what was changed since the panel was last open is there
  const log = signal(api.auditLog(200));
  effect(() => {
    if (activityOpen() && activityTab() === "audit") untracked(() => log.set(api.auditLog(200)));
  });
  const entries = () => log();
  // started (its outcome not yet written), failed in part, or done
  const outcome = (e: AuditEntry): { tone: Tone; label: string } => (e.stage === "started" ? { tone: "quiet", label: t("audit.started") } : e.failed > 0 ? { tone: "fail", label: t("audit.failed") } : { tone: "ok", label: t("audit.done") });
  return (
    <If when={entries().length > 0} else={<Empty icon="scroll-text" text={t("audit.none")} />}>
      <Scroll class="flex-1" contentClass="pb-4">
        <Card pad="none">
          <Each of={entries()}>
            {(e, i) => (
              <Row stack first={i() === 0}>
                <View class="min-h-7 flex-row items-center gap-2">
                  <Text class={`min-w-0 flex-1 ${TEXT.body} line-clamp-1`}>{[actionLabel(e().action), [e().endpoint, e().bucket].filter(Boolean).join("/")].filter(Boolean).join(" · ")}</Text>
                  {/* a change done says nothing more: only one under way or failed says how it stands */}
                  <If when={e().stage === "started" || e().failed > 0}>
                    <Badge tone={outcome(e()).tone} label={outcome(e()).label} />
                  </If>
                  <If when={e().stage !== "started"}>
                    <Text class={`shrink-0 tabular-nums ${TEXT.meta}`}>{t("matrix.of", { n: count(e().succeeded), of: count(e().items) })}</Text>
                  </If>
                  <Text class={`shrink-0 ${TEXT.meta}`}>{date(e().time)}</Text>
                </View>
                <If when={e().targets.length > 0}>
                  <View selectable>
                    <Each of={e().targets.slice(0, 6)}>{(x) => <Text class={`${TEXT.codeQuiet} line-clamp-1`}>{x()}</Text>}</Each>
                  </View>
                </If>
                <If when={e().errors.length > 0}>
                  <Text class={TEXT.error}>{e().errors.join("; ")}</Text>
                </If>
              </Row>
            )}
          </Each>
        </Card>
      </Scroll>
    </If>
  );
}

/** Activity's content: its three kinds as a segmented control. In a window
 * it fills a side panel, under the panel's header. */
export function Activity(props: { onClose?: () => void }) {
  const body = (tab: ActivityTab) => (tab === "jobs" ? <Jobs /> : <Changes />);
  const tabs = () => <Segmented class="self-center" testID="activity-tab" value={activityTab()} items={ACTIVITY_TABS.map((x) => ({ value: x, title: TAB_LABEL[x]() }))} onChange={(v) => activityTab.set(v as ActivityTab)} />;
  // rebuilt per tab: each owns its timers
  const content = () => (
    <Each of={[activityTab()]} key={(x) => x}>
      {(tab) => <View class="min-h-0 flex-1">{body(tab())}</View>}
    </Each>
  );
  return (
    <If
      when={terminal()}
      else={
        <View testID="activity" class="flex-1">
          <PanelHeader icon="activity" title={t("nav.activity")} onClose={props.onClose} closeTestID="activity-close">
            {tabs()}
          </PanelHeader>
          <View class="min-h-0 flex-1 px-4 pt-3">{content()}</View>
        </View>
      }
    >
      <View testID="activity" class="flex-1 gap-1 p-1">
        <View class="flex-row items-center gap-2">
          <View class="flex-1" />
          {tabs()}
        </View>
        {content()}
      </View>
    </If>
  );
}

/** The toolbar's way in (jobs running show as a count), and where the focus goes back to as the panel closes. */
export let activityAnchor: ViewRef | undefined;

export function ActivityButton(props: { onPress: () => void }) {
  watchJobs();
  return (
    <View ref={(r: ViewRef) => (activityAnchor = r)}>
      <IconButton
        testID="activity-open"
        icon="activity"
        label={t("nav.activity")}
        shortcut="mod+shift+a"
        onPress={props.onPress}
        selected={activityOpen()}
        tint={runningJobs() > 0 ? "primary" : undefined}
        badge={runningJobs() > 0 ? count(runningJobs()) : undefined}
      />
    </View>
  );
}
