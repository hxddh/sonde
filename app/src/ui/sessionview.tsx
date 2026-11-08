// A session on screen, with no state of the app's own: its header and
// figures, what stands out (each a filter of the operations), the
// operations with their outcome and place in time, the program's output,
// and how output and operations line up. The app and the web viewer
// (sonde share) both draw with these.

import { type ContextMenuEvent, type JSX, List, platform, Scroll, signal, Text, View } from "nsh";
import { Button as BaseButton, DataTable, type DataTableRef, type DataTableSort, HoverCard } from "nsh/base";
import { bytes, date, day, ms } from "../format";
import { t } from "../i18n";
import { episodeNotes, episodeTitle, observationSaving, observationText, observationWhy } from "../phrases";
import { groupName, offset, sessionStory, SessionShape, Waterfall } from "./plots";
import type { Episode, LogLine, Observation, Profile, ProfileGroup, RecordSummary, SessionInfo, SessionView } from "../types";
import { Icon } from "./icon";
import { Dot, Empty, POPUP, rowHeight, terminal, TYPE } from "./kit";

const SHELL = new Set(["for", "while", "until", "if", "case", "cd", "env", "export", "sh", "bash", "zsh", "time", "sudo", "nohup", "exec", "(", "{"]);

/** What a command runs, for lists: its program's name, or the command itself when it starts with the shell's own words. */
export function programOf(command: string | null | undefined): string | null {
  if (!command) return null;
  const first = command.trim().split(/\s+/)[0] ?? "";
  if (SHELL.has(first) || first.includes("=")) return command.trim().length > 40 ? `${command.trim().slice(0, 40)}…` : command.trim();
  return first.split("/").pop() || command;
}

export const seconds = (s: number) => (s < 10 ? s.toFixed(1) : s.toFixed(0));

/** Local time with milliseconds: 14:02:11.204 */
export function clock(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

const time = (iso: string) => new Date(iso).getTime();

export type Tone = "ok" | "miss" | "client" | "server" | "refused";

/** How an operation ended, as a status color: lookups that found nothing are not errors. */
export function episodeTone(e: Episode): Tone {
  if (e.outcome.kind === "refused") return "refused";
  if (e.outcome.kind === "failed") return /^4\d\d/.test(e.outcome.detail) ? "client" : "server";
  if (e.kind === "probe" || e.title.includes("found nothing")) return "miss";
  // every request a lookup that found nothing
  if (e.notFound && e.notFound === e.records.length) return "miss";
  return "ok";
}

/** A request's status as a status color. */
export function statusTone(s: number | null): Tone {
  if (s === null) return "server";
  if (s === 404) return "miss";
  if (s >= 500) return "server";
  if (s >= 400) return "client";
  return "ok";
}

const TONE_TEXT: Record<Tone, string> = {
  ok: "text-status-ok",
  miss: "text-status-miss",
  client: "text-status-client",
  server: "text-status-server",
  refused: "text-status-refused",
};
const TONE_BAR: Record<Tone, string> = {
  ok: "bg-bar",
  miss: "bg-status-miss",
  client: "bg-status-client",
  server: "bg-status-server",
  refused: "bg-status-refused",
};
export const toneText = (t: Tone) => TONE_TEXT[t];

const FORMAT_TEXT: Record<string, string> = { SlateDB: "text-fmt-slatedb", Iceberg: "text-fmt-iceberg", Delta: "text-fmt-delta" };
const KIND_LABEL: Partial<Record<Episode["kind"], string>> = { multipart: "MPU", retry: "retry", listing: "list", probe: "lookup" };

/** What kind of thing an operation is: its storage format, else its kind. */
function family(e: Episode): { label: string; class: string } {
  if (e.format) return { label: e.format, class: FORMAT_TEXT[e.format] ?? "text-fmt-other" };
  const k = KIND_LABEL[e.kind];
  return k ? { label: k, class: e.kind === "multipart" ? "text-fmt-mpu" : "text-fmt-other" } : { label: "", class: "" };
}

export const duration = (e: Episode) => time(e.end) - time(e.start);

/** The session's span in time: its first start, its last end. */
export function span(v: SessionView): [number, number] {
  if (!v.episodes.length) return [0, 1];
  let a = Infinity;
  let b = -Infinity;
  for (const e of v.episodes) {
    a = Math.min(a, time(e.start));
    b = Math.max(b, time(e.end));
  }
  return [a, Math.max(b, a + 1)];
}

/** A figure of the session: a number, and what it counts. */
function Figure(props: { value: string; label: string; tone?: Tone; preview?: JSX.Element; testID?: string }) {
  const figure = (
    <View testID={props.testID} class="gap-0.5 pr-5">
      <Text class={`tabular-nums text-heading font-semibold ${props.tone ? TONE_TEXT[props.tone] : "text-foreground"}`}>{props.value}</Text>
      <Text class="text-caption text-muted-foreground">{props.label}</Text>
    </View>
  );
  if (!props.preview || terminal()) return figure;
  // resting on a figure shows what it is made of
  return (
    <HoverCard
      openDelay={450}
      closeDelay={120}
      placement="bottom"
      align="start"
      offset={6}
      trigger={figure}
      popupClass={`w-[300px] gap-2 ${POPUP} p-3`}
      popupStartingClass="opacity-0 -translate-y-1"
      popupEndingClass="opacity-0"
    >
      {props.preview}
    </HoverCard>
  );
}

/** What a session's requests were, by kind: a figure's preview. */
function KindsPreview(props: { profile: Profile; by: "requests" | "time" }) {
  const rows = () => [...props.profile.groups].sort((a, b) => (props.by === "requests" ? b.requests - a.requests : b.busyMs - a.busyMs)).slice(0, 5);
  const total = () => (props.by === "requests" ? props.profile.groups.reduce((s, g) => s + g.requests, 0) : props.profile.busyMs) || 1;
  const value = (g: ProfileGroup) => (props.by === "requests" ? g.requests : g.busyMs);
  return (
    <View testID={`preview-${props.by}`} class="gap-1.5">
      <Text class="text-caption font-medium text-muted-foreground">{props.by === "requests" ? t("figure.requests") : t("profile.spentTitle")}</Text>
      <Each of={rows()}>
        {(g) => (
          <View class="gap-0.5">
            <View class="flex-row items-baseline gap-2">
              <Text class="min-w-0 flex-1 text-small text-popover-foreground line-clamp-1">{groupName(g())}</Text>
              <Text class="tabular-nums text-caption text-muted-foreground">{props.by === "requests" ? String(g().requests) : offset(g().busyMs)}</Text>
            </View>
            <View class="h-1 overflow-hidden rounded-full bg-muted">
              <View class="h-full rounded-full bg-chart-1" style={{ width: `${Math.max(2, Math.round((value(g()) * 100) / total()))}%` }} />
            </View>
          </View>
        )}
      </Each>
    </View>
  );
}

/** The command and where it ran, then the session's figures. */
export function SessionHeader(props: { view: SessionView; actions?: JSX.Element; compact?: boolean; slice?: number | null; onSlice?: (i: number | null) => void; shape?: boolean }) {
  const s = () => props.view.summary;
  const line = () => {
    const parts = [t("timeline.operations", { n: s().requests, ops: props.view.episodes.length, secs: seconds(s().seconds) })];
    if (s().errors) parts.push(t("timeline.errors", { n: s().errors }));
    if (s().misses) parts.push(t("timeline.notFound", { n: s().misses }));
    parts.push(`↑${bytes(s().bytesUp)} ↓${bytes(s().bytesDown)}`);
    return parts.join(" · ");
  };
  const command = () => props.view.header.command;
  if (terminal()) {
    return (
      <View class="gap-0">
        <Text class="font-semibold text-foreground">{command() ? `$ ${command()}` : props.view.header.endpoint}</Text>
        <Text class="text-muted-foreground" testID="session-summary">
          {line()}
        </Text>
      </View>
    );
  }
  return (
    <View class="gap-3">
      <View class="flex-row items-start gap-3">
        <View selectable class="min-w-0 flex-1 gap-1">
          <Text class="font-mono text-body font-semibold text-foreground line-clamp-2">
            {command() ? `$ ${command()}` : props.view.header.endpoint}
          </Text>
          <Text class="text-small text-muted-foreground line-clamp-1">{`${props.view.header.endpoint} · ${props.view.header.url} · ${date(props.view.header.created)}`}</Text>
          {/* the session in a sentence: where its time went, and the first thing to do */}
          <If when={props.view.profile?.groups.length && props.shape !== false}>
            <Text testID="session-story" class="pt-1 text-body leading-5 text-foreground line-clamp-2">{sessionStory(props.view.profile, props.view.observations)}</Text>
          </If>
        </View>
        {props.actions}
      </View>
      <View class={props.compact ? "gap-2" : "flex-row items-center gap-5"}>
      <View testID="session-summary" accessibilityLabel={line()} class="flex-row overflow-hidden">
          <Figure testID="figure-requests" value={String(s().requests)} label={t("figure.requests")} preview={props.view.profile?.groups.length ? <KindsPreview profile={props.view.profile} by="requests" /> : undefined} />
          <Figure value={String(props.view.episodes.length)} label={t("figure.operations")} />
          <Figure value={`${seconds(s().seconds)} s`} label={t("figure.took")} preview={props.view.profile?.groups.length ? <KindsPreview profile={props.view.profile} by="time" /> : undefined} />
          <If when={s().errors > 0}>
            <Figure value={String(s().errors)} label={t("figure.errors")} tone="server" />
          </If>
          <If when={s().misses > 0}>
            <Figure value={String(s().misses)} label={t("figure.notFound")} tone="miss" />
          </If>
          <If when={!props.compact}>
            <Figure value={`↑${bytes(s().bytesUp)}`} label={t("figure.up")} />
            <Figure value={`↓${bytes(s().bytesDown)}`} label={t("figure.down")} />
          </If>
        </View>
        {/* the session's shape: requests by outcome over time, and in flight */}
        <If when={props.view.profile && props.shape !== false}>
          <SessionShape profile={props.view.profile} slice={props.slice} onSlice={props.onSlice} annotate={!props.compact} class={props.compact ? "h-10 w-full" : "mt-3 h-12 min-w-[200px] flex-1"} />
        </If>
      </View>
    </View>
  );
}

/** Whether a session's findings are folded away (kept while the app runs). */
const findingsOpen = signal(true);

/** What stands out in a session — its core result. Each finding says what
 * happened and why it matters, and narrows the operations to those it is
 * about. `compact` (beside a run's live operations): the sentences only. */
export function Observations(props: { view: SessionView; filter?: number | null; onFilter?: (i: number | null) => void; compact?: boolean; max?: number; maxHeight?: number }) {
  const press = (i: number) => props.onFilter?.(props.filter === i ? null : i);
  const list = () => props.view.observations.slice(0, props.max ?? props.view.observations.length);
  if (terminal()) {
    return (
      <If when={props.view.observations.length > 0}>
        <View testID="observations">
          <Each of={list()}>
            {(o) => (
              <Text class={o().level === "warn" ? "text-warn" : "text-muted-foreground"}>
                {o().level === "warn" ? "! " : "· "}
                {observationText(o())}
              </Text>
            )}
          </Each>
        </View>
      </If>
    );
  }
  const row = (o: () => Observation, i: () => number) => (
    <BaseButton
      testID={`observation-${i()}`}
      accessibilityLabel={observationText(o())}
      selected={props.filter === i()}
      disabled={!props.onFilter || !o().episodes?.length}
      onPress={() => press(i())}
      class={`group flex-row items-start gap-2.5 px-3 ${props.compact ? "py-1.5" : "py-2"} hover:bg-row-hover selected:bg-selection disabled:hover:bg-transparent ${i() > 0 ? "border-t border-border" : ""}`}
    >
      <View class="h-5 w-4 items-center justify-center">
        <Icon name={o().level === "warn" ? "triangle-alert" : "info"} size={14} tint={o().level === "warn" ? "warn" : "muted-foreground"} />
      </View>
      <View class="min-w-0 flex-1 gap-0.5">
        <Text class={`text-body text-foreground ${props.compact ? "line-clamp-1" : "line-clamp-2"}`}>{observationText(o())}</Text>
        <If when={!props.compact && observationWhy(o())}>{(w) => <Text class="text-small leading-snug text-muted-foreground line-clamp-2">{w()}</Text>}</If>
        <If when={!props.compact && observationSaving(o())}>
          {(v) => (
            <View class="flex-row items-center gap-1 pt-0.5">
              <Icon name="trending-down" size={12} tint="info" />
              <Text testID={`observation-saving-${i()}`} class="tabular-nums text-small font-medium text-info">{v()}</Text>
            </View>
          )}
        </If>
      </View>
      <If when={props.onFilter && o().episodes?.length}>
        <View class="h-5 flex-row items-center gap-1">
          <Text class="tabular-nums text-caption text-muted-foreground group-selected:text-foreground">{t("figure.operationsN", { n: o().episodes.length })}</Text>
          <Icon name={props.filter === i() ? "x" : "chevron-right"} size={12} />
        </View>
      </If>
    </BaseButton>
  );
  return (
    <If when={props.view.observations.length > 0}>
      <View testID="observations" class="gap-1.5">
        <If when={!props.compact}>
          <BaseButton testID="observations-toggle" accessibilityLabel={t("obs.title")} selected={findingsOpen()} onPress={() => findingsOpen.set(!findingsOpen())} class="flex-row items-center gap-1 self-start rounded-(--radius-sm) px-1 hover:bg-muted">
            <Icon name={findingsOpen() ? "chevron-down" : "chevron-right"} size={12} />
            <Text role="header" class="text-caption font-medium text-muted-foreground">{`${t("obs.title")} · ${props.view.observations.length}`}</Text>
          </BaseButton>
        </If>
        <If when={props.compact || findingsOpen()}>
          {/* a short window keeps room for the operations: the findings scroll */}
          <Scroll class="overflow-hidden rounded-(--radius-card) border border-border bg-card" style={props.maxHeight ? { maxHeight: props.maxHeight } : undefined} scrollbar={false}>
            <Each of={list()}>{(o: () => Observation, i) => row(o, i)}</Each>
          </Scroll>
        </If>
      </View>
    </If>
  );
}

const tableClasses = {
  header: "bg-card border-b border-border",
  headerCell: "pl-2 pr-1 text-small font-medium text-muted-foreground",
  cell: "px-2",
  rowOdd: "bg-muted/30",
  rowHover: "bg-row-hover",
  rowSelected: "bg-selection",
  rowActive: "bg-row-hover",
  resizeHandle: "bg-transparent group-hover:bg-border",
};

/** A session's operations: outcome, time, family, title, requests, duration,
 * place in the session's time and notes. `rows` (default all) are the
 * episodes shown, by index; events name episodes by index too. */
export function OperationsTable(props: {
  view: SessionView;
  rows?: number[];
  onSelect: (i: number) => void;
  onContextMenu?: (i: number) => void;
  ref?: (r: DataTableRef) => void;
  highlight?: number[];
  compact?: boolean;
  /** Gets the order the table shows the operations in (sorted by a column, or as given). */
  order?: (shown: () => number[]) => void;
}) {
  const notes = (e: Episode) => episodeNotes(e, props.view.sharedNotes).join(" · ");
  // the slowest or busiest first, at a press on their column's title
  const sort = signal<DataTableSort | null>(null);
  const shown = () => {
    const list = props.rows ?? props.view.episodes.map((_, i) => i);
    const s = sort();
    if (!s) return list;
    const by = (i: number) => {
      const e = props.view.episodes[i];
      return s.key === "duration" ? duration(e) : s.key === "requests" ? e.records.length : time(e.start);
    };
    const dir = s.direction === "descending" ? -1 : 1;
    return [...list].sort((a, b) => (by(a) - by(b)) * dir || a - b);
  };
  props.order?.(() => shown());
  const formats = () => props.view.episodes.some((e) => family(e).label);
  const range = () => span(props.view);
  const ep = (row: number) => props.view.episodes[shown()[row]];
  const columns = () => [
    ...(terminal() ? [] : [{ key: "status", title: "", width: 26, resizable: false, selectable: false }]),
    { key: "time", title: t("column.time"), width: 110, sortable: !terminal() },
    ...(formats() ? [{ key: "format", title: "", width: 86 }] : []),
    { key: "title", title: t("column.operation"), width: props.compact ? 220 : 340, minWidth: 160 },
    { key: "requests", title: t("column.requests"), width: 56, align: "end" as const, sortable: !terminal() },
    { key: "duration", title: t("sessions.took"), width: 78, align: "end" as const, sortable: !terminal() },
    ...(terminal() || props.compact ? [] : [{ key: "bar", title: t("column.when"), width: 150, minWidth: 80 }]),
    ...(props.compact ? [] : [{ key: "notes", title: t("column.notes"), width: 320, minWidth: 120 }]),
  ];
  const data = () =>
    shown().map((i) => {
      const e = props.view.episodes[i];
      return {
        status: episodeTone(e),
        time: clock(e.start),
        format: family(e).label,
        title: terminal() && e.outcome.kind !== "ok" ? `${episodeTitle(e)} ✗` : episodeTitle(e),
        requests: String(e.records.length),
        duration: ms(duration(e)),
        bar: "",
        notes: notes(e),
      };
    });
  const cell = (c: { row: number; key: string; text: string }) => {
    const e = ep(c.row);
    if (!e) return <Text class="px-2 text-body">{c.text}</Text>;
    switch (c.key) {
      case "status":
        return (
          <View class="flex-1 items-center justify-center">
            <Dot tone={episodeTone(e)} label={e.outcome.kind === "ok" ? undefined : e.outcome.kind === "refused" ? "refused" : e.outcome.detail} />
          </View>
        );
      case "format":
        return <Text class={`px-2 text-small font-medium ${family(e).class}`}>{c.text}</Text>;
      case "title":
        return <Text class={`px-2 text-body line-clamp-1 ${e.outcome.kind === "ok" ? "text-foreground" : TONE_TEXT[episodeTone(e)]}`}>{c.text}</Text>;
      case "bar": {
        const [a, b] = range();
        const total = b - a;
        const left = Math.max(0, (time(e.start) - a) / total);
        const width = Math.max(0.006, duration(e) / total);
        return (
          <View class="flex-1 flex-row items-center px-2">
            <View class="h-1.5 flex-1 flex-row overflow-hidden rounded-full bg-bar-track">
              <View style={{ flex: left }} />
              <View class={`rounded-full ${TONE_BAR[episodeTone(e)]}`} style={{ flex: width, minWidth: 3 }} />
              <View style={{ flex: Math.max(0, 1 - left - width) }} />
            </View>
          </View>
        );
      }
      case "notes":
        return <Text class="px-2 text-small text-muted-foreground line-clamp-1">{c.text}</Text>;
      case "time":
        // from the episode, not the cell's text: DataTable fits that to the column with its own font
        return <Text class="px-2 font-mono text-caption text-foreground">{clock(e.start)}</Text>;
      default:
        return <Text class={`px-2 text-foreground ${c.key === "time" || c.key === "requests" || c.key === "duration" ? "font-mono text-caption" : "text-body line-clamp-1"}`}>{c.text}</Text>;
    }
  };
  return (
    <DataTable
      ref={props.ref}
      testID="operations"
      accessibilityLabel={t("sessions.operations")}
      class="min-h-0 flex-1 rounded-(--radius-card) border border-border bg-card"
      columns={columns()}
      data={data()}
      fontSize={TYPE.body}
      rowHeight={rowHeight(30)}
      resizableColumns={!terminal()}
      classes={terminal() ? undefined : tableClasses}
      rowClasses={shown().map((i) => (props.highlight?.includes(i) ? "bg-selection" : ""))}
      renderCell={terminal() ? undefined : cell}
      rowSelectable
      sort={sort()}
      onSortChange={({ sort: next }) => sort.set(next)}
      onRowPress={({ row }) => props.onSelect(shown()[row])}
      onContextMenu={props.onContextMenu ? (e) => { if (e.row !== undefined && e.row !== null) props.onContextMenu?.(shown()[e.row]); } : undefined}
      empty={<Empty text={t("common.empty")} />}
    />
  );
}

/** The program's output, one timed line per row. */
export function OutputTable(props: { lines: LogLine[]; onSelect: (i: number) => void; ref?: (r: DataTableRef) => void; highlight?: number[] }) {
  return (
    <DataTable
      ref={props.ref}
      testID="output"
      accessibilityLabel={t("sessions.output")}
      class="min-h-0 flex-1 rounded-(--radius-card) border border-border bg-card"
      columns={[
        { key: "time", title: t("column.time"), width: 110 },
        { key: "line", title: t("sessions.output"), width: 520, minWidth: 160 },
      ]}
      data={props.lines.map((l) => ({ time: clock(l.time), line: l.text }))}
      fontSize={TYPE.small}
      fontFamily={platform.idiom === "terminal" ? undefined : "monospace"}
      rowHeight={rowHeight(24)}
      classes={terminal() ? undefined : tableClasses}
      rowClasses={props.lines.map((_, i) => (props.highlight?.includes(i) ? "bg-selection" : ""))}
      rowSelectable
      onRowPress={({ row }) => props.onSelect(row)}
      empty={<Empty text={t("common.empty")} />}
    />
  );
}

/** The output lines printed while an operation ran (or the last one before it). */
export function linesOf(lines: LogLine[], e: Episode): number[] {
  const a = time(e.start);
  const b = time(e.end);
  const inside = lines.flatMap((l, i) => (time(l.time) >= a && time(l.time) <= b ? [i] : []));
  if (inside.length) return inside;
  let before = -1;
  lines.forEach((l, i) => {
    if (time(l.time) <= a) before = i;
  });
  return before >= 0 ? [before] : [];
}

/** The operations running when a line was printed, or else the nearest one.
 * Output is read a moment after it is written, so a line can come in just
 * after the request it announced: the nearest operation, before or after. */
export function operationsAt(episodes: Episode[], line: LogLine): number[] {
  const at = time(line.time);
  const running = episodes.flatMap((e, i) => (time(e.start) <= at && at <= time(e.end) ? [i] : []));
  if (running.length) return running;
  let best = -1;
  let gap = Infinity;
  episodes.forEach((e, i) => {
    const d = at < time(e.start) ? time(e.start) - at : at - time(e.end);
    if (d < gap) {
      gap = d;
      best = i;
    }
  });
  return best >= 0 ? [best] : [];
}

/** An operation in the inspector: what it is, its notes, its requests; `onRecord` opens one. */
export function OperationView(props: { episode: Episode; shared: string[]; rows: RecordSummary[]; onRecord: (id: number) => void; onBack?: () => void; back?: JSX.Element }) {
  const e = () => props.episode;
  const notes = () => episodeNotes(e(), props.shared);
  const fam = () => family(e());
  return (
    <View class="gap-4" testID="operation-detail">
      <View class="gap-1.5">
        <View class="flex-row items-center gap-2">
          {props.back}
          <If when={fam().label}>
            <Text class={`text-small font-semibold ${fam().class}`}>{fam().label}</Text>
          </If>
          <View class="flex-1" />
          <If when={e().outcome.kind !== "ok"}>
            <View class="flex-row items-center gap-1.5">
              <Dot tone={episodeTone(e())} />
              <Text class={`text-small font-medium ${TONE_TEXT[episodeTone(e())]}`}>{e().outcome.kind === "failed" ? (e().outcome as { detail: string }).detail : t("sessions.refused")}</Text>
            </View>
          </If>
        </View>
        <Text class="text-heading font-semibold leading-5 text-foreground">
          {episodeTitle(e())}
        </Text>
        <Text class="tabular-nums text-caption text-muted-foreground">{`${clock(e().start)} · ${ms(duration(e()))} · ↑${bytes(e().bytesUp)} ↓${bytes(e().bytesDown)}`}</Text>
      </View>
      <If when={notes().length > 0}>
        <View class="gap-1.5">
          <Each of={notes()}>
            {(n) => (
              <View class="flex-row gap-2">
                <View class="mt-[7px] size-1 rounded-full bg-muted-foreground" />
                <Text class="flex-1 text-body leading-5 text-foreground">{n()}</Text>
              </View>
            )}
          </Each>
        </View>
      </If>
      <View class="gap-1">
        <Text role="header" class="text-small font-semibold text-muted-foreground">{`${t("timeline.requestsOf")} (${e().records.length})`}</Text>
        <Waterfall records={props.rows} onRecord={props.onRecord} />
        <View class="overflow-hidden rounded-(--radius) border border-border bg-card">
          <Each of={props.rows} key={(r) => r.id}>
            {(r, i) => (
              <BaseButton
                testID={`op-request-${i()}`}
                accessibilityLabel={`${r().operation} ${r().target}`}
                onPress={() => props.onRecord(r().id)}
                class={`flex-row items-center gap-2 px-2.5 py-1.5 hover:bg-row-hover active:bg-selection ${i() > 0 ? "border-t border-border" : ""}`}
              >
                <Text class={`w-8 font-mono text-caption font-semibold ${TONE_TEXT[statusTone(r().status)]}`}>{r().status === null ? "—" : String(r().status)}</Text>
                <View class="min-w-0 flex-1">
                  <Text class="text-small font-medium text-foreground line-clamp-1">{r().operation}</Text>
                  <Text class="font-mono text-caption text-muted-foreground line-clamp-1">{r().target}</Text>
                </View>
                <Text class="font-mono text-caption text-muted-foreground">{ms(r().latencyMs)}</Text>
                <If when={!terminal()}>
                  <Icon name="chevron-right" size={13} />
                </If>
              </BaseButton>
            )}
          </Each>
        </View>
      </View>
    </View>
  );
}

const DAY = 86400000;
function dayLabel(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const diff = Math.floor((today.getTime() - new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()) / DAY);
  if (diff <= 0) return t("time.today");
  if (diff === 1) return t("time.yesterday");
  return day(iso);
}

/** The sessions, newest first, by day: the program (or the endpoint), its time and requests. */
export function SessionList(props: { sessions: SessionInfo[]; selected: string | null; onSelect: (p: string) => void; onContextMenu?: (s: SessionInfo, at?: { x: number; y: number }) => void }) {
  // one list, the days as headers that stay in view while their sessions scroll
  type Row = { day: string } | { s: SessionInfo; i: number };
  const rows = (): Row[] => {
    const out: Row[] = [];
    let last = "";
    props.sessions.forEach((s, i) => {
      const day = dayLabel(s.created);
      if (day !== last) out.push({ day: (last = day) });
      out.push({ s, i });
    });
    return out;
  };
  const dayOf = (r: Row) => ("day" in r ? r.day : null);
  const headers = () => rows().flatMap((r, i) => ("day" in r ? [i] : []));
  const name = (s: SessionInfo) => s.title || programOf(s.command) || s.endpoint;
  const args = (s: SessionInfo) => (!s.title && s.command && programOf(s.command) !== s.command.trim() && !programOf(s.command)!.endsWith("…") ? s.command.trim().split(/\s+/).slice(1).join(" ") : "");
  const row = (x: () => { s: SessionInfo; i: number }) => (
    <View onContextMenu={props.onContextMenu ? (e: ContextMenuEvent) => props.onContextMenu!(x().s, e.pagePosition) : undefined}>
    <BaseButton
      testID={`session-${x().i}`}
      accessibilityLabel={x().s.title ?? x().s.command ?? x().s.name}
      selected={props.selected === x().s.path}
      onPress={() => props.onSelect(x().s.path)}
      class="mb-0.5 gap-0.5 rounded-(--radius) px-2 py-1.5 hover:bg-row-hover selected:bg-selection"
    >
      <View class="flex-row items-center gap-2">
        <If when={x().s.live}>
          <Dot tone="live" label={t("timeline.live")} />
        </If>
        <Text class="min-w-0 shrink text-body font-medium text-foreground line-clamp-1">{name(x().s)}</Text>
        <Text class="min-w-0 flex-1 font-mono text-caption text-muted-foreground line-clamp-1">{args(x().s)}</Text>
      </View>
      <Text class="text-caption text-muted-foreground line-clamp-1">{`${clock(x().s.created).slice(0, 5)} · ${x().s.endpoint} · ${t("sessions.requestsN", { n: x().s.requests })}`}</Text>
    </BaseButton>
    </View>
  );
  return (
    <List
      testID="session-list"
      class="flex-1"
      each={rows()}
      stickyHeaderIndices={headers()}
      estimatedItemSize={46}
      keyExtractor={(r) => ("day" in r ? `day:${r.day}` : r.s.path)}
      scrollbar={false}
    >
      {(r) => (
        // a recycled cell: what it shows follows the item it is given
        <If when={dayOf(r())} else={row(() => r() as { s: SessionInfo; i: number })}>
          {(day) => (
            <View class="bg-background px-2 pb-1 pt-2">
              <Text role="header" class="text-caption font-medium text-muted-foreground">
                {day()}
              </Text>
            </View>
          )}
        </If>
      )}
    </List>
  );
}
