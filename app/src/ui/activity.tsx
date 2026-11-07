// Activity: what sonde itself did — its requests, its jobs, the changes it
// made (the audit log). One place for "what is sonde doing, what did it
// do": a sheet from the window's edge, opened from the toolbar; a place of
// its own in a terminal.

import { batch, cleanup, effect, Scroll, signal, Text, View, type ViewRef } from "nsh";
import { Button as BaseButton, DataTable, Input } from "nsh/base";
import { isAvailable, Menu } from "nsh/desktop";
import { ago, date, ms } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { RecordDetail } from "../screens/Requests";
import { ACTIVITY_TABS, type ActivityTab, activityTab, away, focusRecord, notify, report } from "../store";
import type { JobInfo, RecordSummary, RequestFilter } from "../types";
import { Icon, iconSvg } from "./icon";
import { Bar, Button, Dot, Empty, IconButton, PanelHeader, rowHeight, Segmented, terminal, Tip, TYPE } from "./kit";
import { statusTone, toneText } from "./sessionview";

export const activityOpen = signal(false);

/** Jobs running now, for the toolbar's badge. */
export const runningJobs = signal(0);
let poll: ReturnType<typeof setInterval> | undefined;
export function watchJobs() {
  poll ??= setInterval(() => {
    if (away()) return;
    try {
      const n = api.jobs().filter((j) => j.state === "running").length;
      if (n !== runningJobs()) runningJobs.set(n);
    } catch {
      // the engine is gone
    }
  }, 800);
}

const TAB_LABEL: Record<ActivityTab, () => string> = { requests: () => t("activity.requests"), jobs: () => t("activity.jobs"), audit: () => t("activity.changes") };

function Requests() {
  const text = signal("");
  const errors = signal(false);
  const latest = signal(api.latestRequest());
  const selected = signal<number | null>(focusRecord());
  effect(() => {
    const f = focusRecord();
    if (f !== null) batch(() => (selected.set(f), focusRecord.set(null)));
  });
  const timer = setInterval(() => {
    if (away()) return;
    const l = api.latestRequest();
    if (l !== latest()) latest.set(l);
  }, 700);
  cleanup(() => clearInterval(timer));
  const filter = (): RequestFilter => ({ status: errors() ? { kind: "errors" } : null, text: text() || null });
  const rows = (): RecordSummary[] => {
    latest();
    try {
      return api.requests(filter(), 5000);
    } catch (e) {
      report(e);
      return [];
    }
  };
  const record = () => {
    const id = selected();
    return id === null ? null : api.request(id);
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
      { label: t("requests.clear"), icon: iconSvg("trash"), onSelect: () => (api.clearRequests(), latest.set(api.latestRequest())) },
    ]);
  };
  const list = () => (
    <View class="min-h-0 flex-1 gap-2.5">
      <View class="flex-row items-center gap-2">
        <View class="h-8 min-w-0 flex-1 flex-row items-center gap-2 rounded-(--radius) border border-input bg-background px-2.5 focus-within:border-ring">
          <If when={!terminal()}>
            <Icon name="search" size={14} />
          </If>
          <Input value={text()} placeholder={t("activity.search")} onChange={({ value }) => text.set(value)} accessibilityLabel={t("common.search")} autoCapitalize="none" autoCorrect={false} class="min-w-0 flex-1 text-body text-foreground placeholder:text-muted-foreground" />
        </View>
        <Segmented
          testID="requests-filter"
          value={errors() ? "errors" : "all"}
          items={[
            { value: "all", title: t("requests.all") },
            { value: "errors", title: t("requests.errors") },
          ]}
          onChange={(v) => errors.set(v === "errors")}
        />
        <If when={isAvailable && !terminal()}>
          <IconButton icon="ellipsis" label={t("common.more")} onPress={more} testID="requests-more" />
        </If>
      </View>
      <DataTable
        testID="requests"
        accessibilityLabel={t("activity.requests")}
        class="min-h-0 flex-1 rounded-(--radius-card) border border-border bg-card"
        columns={[
          ...(terminal() ? [] : [{ key: "status", title: "", width: 52, resizable: false }]),
          { key: "operation", title: t("requests.operation"), width: 190 },
          { key: "target", title: t("activity.target"), width: 200, minWidth: 120 },
          { key: "latency", title: t("requests.latency"), width: 76, align: "end" as const },
          { key: "time", title: t("column.time"), width: 70 },
        ]}
        data={rows().map((r) => ({ status: r.status === null ? "—" : String(r.status), operation: r.operation, target: r.target, latency: ms(r.latencyMs), time: date(r.time).slice(11, 16) }))}
        fontSize={TYPE.body}
        rowHeight={rowHeight(28)}
        classes={terminal() ? undefined : { header: "bg-card border-b border-border", headerCell: "pl-2 pr-1 text-small font-medium text-muted-foreground", cell: "px-2", rowHover: "bg-row-hover", rowSelected: "bg-selection" }}
        renderCell={
          terminal()
            ? undefined
            : (c) => {
                const r = rows()[c.row];
                if (c.key === "status" && r)
                  return (
                    <View class="flex-1 flex-row items-center gap-1.5 px-2">
                      <Dot tone={statusTone(r.status)} />
                      <Text class={`font-mono text-caption font-semibold ${toneText(statusTone(r.status))}`}>{c.text}</Text>
                    </View>
                  );
                return <Text class={`px-2 line-clamp-1 ${c.key === "target" ? "font-mono text-caption text-muted-foreground" : c.key === "operation" ? "text-body text-foreground" : "font-mono text-caption text-muted-foreground"}`}>{c.text}</Text>;
              }
        }
        rowSelectable
        onRowPress={({ row }) => selected.set(rows()[row]?.id ?? null)}
        empty={<Empty icon="activity" text={t("activity.noRequests")} />}
      />
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

function Jobs() {
  const jobs = signal<JobInfo[]>(api.jobs());
  const timer = setInterval(() => !away() && jobs.set(api.jobs()), 500);
  cleanup(() => clearInterval(timer));
  const tone = (s: string) => (s === "finished" ? "ok" : s === "failed" ? "server" : s === "cancelled" ? "miss" : "live") as "ok" | "server" | "miss" | "live";
  return (
    <If when={jobs().length > 0} else={<Empty icon="timer" text={t("jobs.none")} />}>
      <Scroll class="flex-1" contentClass="gap-2 pb-4">
        <Each of={jobs()} key={(j) => j.id}>
          {(j) => (
            <View class="gap-2 rounded-(--radius-card) border border-border bg-card px-3.5 py-3">
              <View class="flex-row items-center gap-2">
                <Dot tone={tone(j().state)} label={j().state} />
                <Text class="min-w-0 flex-1 text-body font-medium text-foreground line-clamp-1">{j().title}</Text>
                <Text class="text-caption text-muted-foreground">{ago(j().started)}</Text>
                <If when={j().state === "running"}>
                  <Button small variant="outline" label={t("jobs.cancel")} onPress={() => api.cancelJob(j().id)} />
                </If>
              </View>
              <If when={j().state === "running"}>
                <Bar value={j().progress.total ? j().progress.done / Math.max(1, j().progress.total!) : null} label={j().progress.message} />
              </If>
              <If when={j().error}>{(e) => <Text class="text-small text-destructive">{e().message}</Text>}</If>
            </View>
          )}
        </Each>
      </Scroll>
    </If>
  );
}

function Changes() {
  const entries = api.auditLog(200);
  return (
    <If when={entries.length > 0} else={<Empty icon="scroll-text" text={t("audit.none")} />}>
      <Scroll class="flex-1" contentClass="gap-2 pb-4">
        <Each of={entries}>
          {(e) => (
            <View class="gap-1.5 rounded-(--radius-card) border border-border bg-card px-3.5 py-3">
              <View class="flex-row items-center gap-2">
                <Dot tone={e().failed ? "client" : "ok"} />
                <Text class="min-w-0 flex-1 text-body font-medium text-foreground">{`${e().action} · ${e().endpoint}/${e().bucket}`}</Text>
                <Text class="tabular-nums text-caption text-muted-foreground">{`${e().succeeded}/${e().items}`}</Text>
                <Text class="text-caption text-muted-foreground">{date(e().time)}</Text>
              </View>
              <View selectable>
                <Each of={e().targets.slice(0, 6)}>{(x) => <Text class="font-mono text-caption text-muted-foreground line-clamp-1">{x()}</Text>}</Each>
              </View>
              <If when={e().errors.length > 0}>
                <Text class="text-small text-destructive">{e().errors.join("; ")}</Text>
              </If>
            </View>
          )}
        </Each>
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

/** The toolbar's way in: jobs running show as a count. */
/** The toolbar's Activity button: where the focus goes back to as the panel closes. */
export let activityAnchor: ViewRef | undefined;

export function ActivityButton(props: { onPress: () => void }) {
  watchJobs();
  return (
    <View ref={(r: ViewRef) => (activityAnchor = r)}>
    <BaseButton
      testID="activity-open"
      accessibilityLabel={t("nav.activity")}
      onPress={props.onPress}
      selected={activityOpen()}
      tooltip={<Tip text={t("nav.activity")} shortcut="mod+shift+a" />}
      class="h-7 flex-row items-center gap-1.5 rounded-(--radius) px-1.5 hover:bg-muted selected:bg-accent"
    >
      <Icon name="activity" size={16} tint={runningJobs() > 0 ? "primary" : "muted-foreground"} />
      <If when={runningJobs() > 0}>
        <Text class="tabular-nums text-caption font-semibold text-primary">{String(runningJobs())}</Text>
      </If>
    </BaseButton>
    </View>
  );
}
