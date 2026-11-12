// Activity: what sonde itself did — its requests, its jobs, the changes it
// made (Changes). One place for "what is sonde doing, what did it
// do": a sheet from the window's edge, opened from the toolbar; a place of
// its own in a terminal.

import { batch, cleanup, computed, effect, Scroll, signal, Text, untracked, View, type ViewRef } from "nsh";
import { DataTable } from "nsh/base";
import { isAvailable, Menu } from "nsh/desktop";
import { clock, count, date, middle, duration, since } from "../format";
import { t } from "../i18n";
import { core } from "../phrases";
import { api } from "../native";
import { RecordDetail } from "../screens/Requests";
import { ACTIVITY_TABS, type ActivityTab, activityTab, focusRecord, go, notify, openSession, report, sessionsChanged } from "../store";
import type { AuditEntry, JobInfo, RecordSummary, RequestFilter } from "../types";
import { iconSvg } from "./icon";
import { Badge, Bar, Button, Card, Empty, IconButton, Options, PanelHeader, terminal, Row, ROW, rowHeight, SearchField, Segmented, type Tone, TYPE, TABLE, confirmAlert, COLUMN } from "./kit";
import { StatusMark } from "./sessionview";

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

const TAB_LABEL: Record<ActivityTab, () => string> = { requests: () => t("activity.requests"), jobs: () => t("activity.jobs"), audit: () => t("activity.changes") };

/** The least room a request's target has in the table. */
const TARGET = 120;

function Requests() {
  const text = signal("");
  const errors = signal(false);
  const latest = signal(api.latestRequest());
  const selected = signal<number | null>(focusRecord());
  effect(() => {
    const f = focusRecord();
    if (f !== null) batch(() => (selected.set(f), focusRecord.set(null)));
  });
  // new records refresh the list, a few times a second at most
  const refresh = throttled(() => {
    const l = api.latestRequest();
    if (l !== latest()) latest.set(l);
  }, 300);
  cleanup(api.on("request", refresh));
  const filter = (): RequestFilter => ({ status: errors() ? { kind: "errors" } : null, text: text() || null });
  // asked of the engine once per change of the log or the filter, not once per cell drawn
  const rows = computed((): RecordSummary[] => {
    latest();
    try {
      return api.requests(filter(), 5000);
    } catch (e) {
      // said after this pass: a toast is not shown from inside a derivation
      queueMicrotask(() => report(e));
      return [];
    }
  });
  const record = computed(() => {
    const id = selected();
    return id === null ? null : api.request(id);
  });
  // the log is gone for good: asked first, and what was open in it closes
  const clear = async () => {
    if (!(await confirmAlert({ title: t("requests.clearTitle"), text: t("requests.clearText"), commit: t("common.clear") }))) return;
    try {
      api.clearRequests();
      batch(() => {
        selected.set(null);
        latest.set(api.latestRequest());
      });
    } catch (e) {
      report(e);
    }
  };
  const asSession = () => {
    try {
      const path = api.sessions.fromRequestLog();
      sessionsChanged();
      batch(() => {
        openSession.set(path);
        activityOpen.set(false);
        go("sessions");
      });
    } catch (e) {
      report(e);
    }
  };
  const more = () => {
    if (!isAvailable) return;
    const exportAs = (f: "json" | "har") => {
      try {
        notify(t("common.exportedTo", { path: api.exportRequests(f, filter()) }));
      } catch (e) {
        report(e);
      }
    };
    void Menu.popup([
      { label: t("requests.exportJson"), icon: iconSvg("download"), onSelect: () => exportAs("json") },
      { label: t("requests.exportHar"), icon: iconSvg("download"), onSelect: () => exportAs("har") },
      { type: "separator" },
      { label: t("requests.clear"), icon: iconSvg("trash"), onSelect: () => void clear() },
    ]);
  };
  // the table's own width: its target column takes what the others leave
  const tableWidth = signal(0);
  // the operation's column as wide as its longest name here (at the body's size, with the cell's inset),
  // and in a narrow panel no wider than half the room the figures leave: the target has the other half
  const rest = () => tableWidth() - (terminal() ? 0 : COLUMN.status) - COLUMN.took - COLUMN.clock - 2;
  const opWidth = () => Math.min(COLUMN.operation, Math.ceil(Math.max(0, ...rows().map((r) => r.operation.length)) * TYPE.body * 0.6) + 16, tableWidth() > 0 ? Math.max(COLUMN.operationLeast, Math.floor(rest() / 2)) : COLUMN.operation);
  const targetWidth = () => (tableWidth() > 0 ? Math.max(TARGET, tableWidth() - (terminal() ? 0 : COLUMN.status) - opWidth() - COLUMN.took - COLUMN.clock - 2) : TARGET);
  const list = () => (
    <View class="min-h-0 flex-1 gap-2">
      <View class="flex-row items-center gap-2">
        <SearchField small class="min-w-0 flex-1" value={text()} placeholder={t("activity.search")} label={t("common.search")} onChange={(v) => text.set(v)} />
        <Options
          testID="requests-filter"
          value={errors() ? "errors" : "all"}
          items={[
            { value: "all", title: t("requests.all") },
            { value: "errors", title: t("requests.errors") },
          ]}
          onChange={(v) => errors.set(v === "errors")}
        />
        {/* sonde's own requests read as any program's: findings, time, compared */}
        <Button small variant="outline" icon="list-clock" label={t("requests.asSession")} onPress={asSession} testID="requests-as-session" />
        <If when={isAvailable}>
          <IconButton icon="ellipsis" label={t("common.more")} onPress={more} testID="requests-more" />
        </If>
      </View>
      <Card pad="none" class="min-h-0 flex-1" onWidth={(w) => tableWidth.set(w)}>
      <DataTable
        testID="requests"
        accessibilityLabel={t("activity.requests")}
        class="min-h-0 flex-1"
        columns={[
          // a dot and the code, as StatusMark draws them
          ...(terminal() ? [] : [{ key: "status", title: "", width: COLUMN.status, resizable: false }]),
          // fits the panel: the figures keep what they need, the operation the room its longest
          // name in these rows takes (never more than S3's longest), and the target all the rest
          // (the table measures itself)
          { key: "operation", title: t("requests.operation"), width: opWidth() },
          { key: "target", title: t("activity.target"), width: targetWidth(), minWidth: COLUMN.targetLeast },
          { key: "latency", title: t("sessions.took"), width: COLUMN.took, align: "end" as const },
          // to the millisecond, as operations show theirs
          { key: "time", title: t("column.time"), width: COLUMN.clock },
        ]}
        data={rows().map((r) => ({ status: r.status === null ? "—" : `${r.status}`, operation: r.operation, target: terminal() ? middle(r.target, 30) : r.target, latency: duration(r.latencyMs), time: clock(r.time, "ms") }))}
        fontSize={TYPE.body}
        rowHeight={rowHeight(ROW.table)}
        classes={terminal() ? undefined : TABLE}
        renderCell={
          terminal()
            ? undefined
            : (c) => {
                const r = rows()[c.row];
                if (c.key === "status" && r) return <StatusMark status={r.status} />;
                if (c.key === "latency") return <View class="flex-1 items-end justify-center"><Text class="text-small tabular-nums text-foreground">{c.text}</Text></View>;
                // a long name or key is cut in its middle, by the text itself: both its ends tell two apart
                if (c.key === "time") return <Text class="font-mono text-caption text-muted-foreground">{c.text}</Text>;
                return (
                  <Text class={`line-clamp-1 ${c.key === "operation" ? "text-body text-foreground" : "font-mono text-caption text-muted-foreground"}`} style={{ textOverflow: "ellipsis-middle" }}>
                    {c.text}
                  </Text>
                );
              }
        }
        rowSelectable
        onRowPress={({ row }) => selected.set(rows()[row]?.id ?? null)}
        empty={<Empty icon="activity" text={t("activity.noRequests")} />}
      />
      </Card>
    </View>
  );
  return (
    <If when={record()} else={list()}>
      {(r) => (
        <Scroll class="flex-1" contentClass="pb-4">
          <RecordDetail record={r()} onBack={() => selected.set(null)} />
        </Scroll>
      )}
    </If>
  );
}

const JOB_TONE: Record<JobInfo["state"], Tone> = { running: "primary", finished: "pass", failed: "fail", cancelled: "neutral" };

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
                  <View class="shrink-0">
                    <Badge tone={JOB_TONE[j().state] ?? "neutral"} label={t(`jobs.state.${j().state}` as never)} />
                  </View>
                  <Text class="min-w-0 flex-1 text-body text-foreground line-clamp-1">{core(j().titleText, j().title)}</Text>
                  <Text class="shrink-0 text-caption text-muted-foreground">{age(j().started)}</Text>
                  <If when={j().state === "running"}>
                    <Button small variant="outline" label={t("jobs.cancel")} onPress={() => api.cancelJob(j().id)} />
                  </If>
                </View>
                <If when={j().state === "running"}>
                  <Bar value={j().progress.total ? j().progress.done / Math.max(1, j().progress.total!) : null} label={core(j().progress.messageText, j().progress.message)} />
                </If>
                <If when={j().error}>{(e) => <Text class="text-small text-destructive">{e().message}</Text>}</If>
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
  const outcome = (e: AuditEntry): { tone: Tone; label: string } => (e.stage === "started" ? { tone: "neutral", label: t("audit.started") } : e.failed > 0 ? { tone: "fail", label: t("audit.failed") } : { tone: "pass", label: t("audit.done") });
  return (
    <If when={entries().length > 0} else={<Empty icon="scroll-text" text={t("audit.none")} />}>
      <Scroll class="flex-1" contentClass="pb-4">
        <Card pad="none">
          <Each of={entries()}>
            {(e, i) => (
              <Row stack first={i() === 0}>
                <View class="min-h-7 flex-row items-center gap-2">
                  <View class="shrink-0">
                    <Badge tone={outcome(e()).tone} label={outcome(e()).label} />
                  </View>
                  <Text class="min-w-0 flex-1 text-body text-foreground line-clamp-1">{[actionLabel(e().action), [e().endpoint, e().bucket].filter(Boolean).join("/")].filter(Boolean).join(" · ")}</Text>
                  <If when={e().stage !== "started"}>
                    <Text class="shrink-0 tabular-nums text-caption text-muted-foreground">{t("matrix.of", { n: count(e().succeeded), of: count(e().items) })}</Text>
                  </If>
                  <Text class="shrink-0 text-caption text-muted-foreground">{date(e().time)}</Text>
                </View>
                <If when={e().targets.length > 0}>
                  <View selectable>
                    <Each of={e().targets.slice(0, 6)}>{(x) => <Text class="font-mono text-caption text-muted-foreground line-clamp-1">{x()}</Text>}</Each>
                  </View>
                </If>
                <If when={e().errors.length > 0}>
                  <Text class="text-small text-destructive">{e().errors.join("; ")}</Text>
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
  const body = (tab: ActivityTab) => (tab === "requests" ? <Requests /> : tab === "jobs" ? <Jobs /> : <Changes />);
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
