// A session on screen, with no state of the app's own: its header,
// observations, operations, output lines and how they line up. The app and
// the web viewer (sonde share) both draw with these.

import { type JSX, Text, View } from "nsh";
import { Button as BaseButton, DataTable, type DataTableRef } from "nsh/base";
import { bytes, date, ms } from "../format";
import { t } from "../i18n";
import type { Episode, LogLine, RecordSummary, SessionInfo, SessionView } from "../types";
import { Badge, Empty, Heading, Mono, Muted } from "./kit";

export const seconds = (s: number) => (s < 10 ? s.toFixed(1) : s.toFixed(0));

/** Local time with milliseconds: 14:02:11.204 */
export function clock(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

export function outcomeBadge(e: Episode) {
  if (e.outcome.kind === "failed") return <Badge label={e.outcome.detail} tone="fail" />;
  if (e.outcome.kind === "refused") return <Badge label="refused" tone="warn" />;
  return null;
}

export function SessionList(props: { sessions: SessionInfo[]; selected: string | null; onSelect: (p: string) => void }) {
  return (
    <Each of={props.sessions} key={(s) => s.path}>
      {(s, i) => (
        <BaseButton
          testID={`session-${i()}`}
          accessibilityLabel={s().name}
          selected={props.selected === s().path}
          onPress={() => props.onSelect(s().path)}
          class="gap-0.5 rounded-(--radius) px-2 py-1.5 hover:bg-muted selected:bg-accent"
        >
          <View class="flex-row items-center gap-2">
            <Text class="flex-1 text-sm text-foreground line-clamp-1">{s().command ?? s().name}</Text>
            <If when={s().live}>
              <Badge label={t("timeline.live")} tone="primary" />
            </If>
          </View>
          <Text class="text-xs text-muted-foreground line-clamp-1">{`${date(s().created).slice(0, 16)} · ${s().endpoint} · ${s().requests}`}</Text>
        </BaseButton>
      )}
    </Each>
  );
}

export function Observations(props: { view: SessionView }) {
  return (
    <If when={props.view.observations.length > 0}>
      <View class="gap-1 rounded-(--radius) bg-muted p-2" testID="observations">
        <Each of={props.view.observations}>
          {(o) => (
            <View class="flex-row gap-2">
              <Text class={o().level === "warn" ? "text-warn" : "text-muted-foreground"}>{o().level === "warn" ? "!" : "·"}</Text>
              <Text class="flex-1 text-sm text-foreground">{o().text}</Text>
            </View>
          )}
        </Each>
      </View>
    </If>
  );
}

export function SessionHeader(props: { view: SessionView }) {
  const s = () => props.view.summary;
  const line = () => {
    const parts = [t("timeline.operations", { n: s().requests, ops: props.view.episodes.length, secs: seconds(s().seconds) })];
    if (s().errors) parts.push(t("timeline.errors", { n: s().errors }));
    if (s().misses) parts.push(t("timeline.notFound", { n: s().misses }));
    parts.push(`↑${bytes(s().bytesUp)} ↓${bytes(s().bytesDown)}`);
    return parts.join(" · ");
  };
  return (
    <View class="gap-1">
      <Mono class="text-sm font-semibold">{props.view.header.command ? `$ ${props.view.header.command}` : props.view.header.endpoint}</Mono>
      <Muted class="text-xs">{`${props.view.header.endpoint} · ${props.view.header.url} · ${date(props.view.header.created)}`}</Muted>
      <Text class="text-sm text-foreground" testID="session-summary">
        {line()}
      </Text>
    </View>
  );
}

export function OperationsTable(props: { view: SessionView; onSelect: (i: number) => void; ref?: (r: DataTableRef) => void; highlight?: number[] }) {
  const notes = (e: Episode) => e.notes.filter((n) => !props.view.sharedNotes.includes(n)).join(" · ");
  const formats = () => props.view.episodes.some((e) => e.format);
  return (
    <DataTable
      ref={props.ref}
      testID="operations"
      class="flex-1 rounded-(--radius) border border-border"
      columns={[
        { key: "time", title: "Time", width: 100 },
        ...(formats() ? [{ key: "format", title: "", width: 64 }] : []),
        { key: "title", title: "Operation", width: 380 },
        { key: "requests", title: "Req", width: 44, align: "end" as const },
        { key: "duration", title: t("sessions.took"), width: 72, align: "end" as const },
        { key: "notes", title: "", width: 340, cellClass: "pl-4 text-muted-foreground" },
      ]}
      data={props.view.episodes.map((e) => ({
        time: clock(e.start),
        format: e.format ?? "",
        title: e.outcome.kind === "ok" ? e.title : `${e.title} ✗`,
        requests: String(e.records.length),
        duration: ms(new Date(e.end).getTime() - new Date(e.start).getTime()),
        notes: notes(e),
      }))}
      rowClasses={props.view.episodes.map((e, i) =>
        e.outcome.kind === "failed" ? "text-fail" : e.outcome.kind === "refused" ? "text-warn" : props.highlight?.includes(i) ? "bg-accent" : "",
      )}
      rowSelectable
      onRowPress={({ row }) => props.onSelect(row)}
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
      class="flex-1 rounded-(--radius) border border-border"
      columns={[
        { key: "time", title: "Time", width: 100 },
        { key: "line", title: t("sessions.output"), width: 420, minWidth: 160 },
      ]}
      data={props.lines.map((l) => ({ time: clock(l.time), line: l.text }))}
      rowClasses={props.lines.map((_, i) => (props.highlight?.includes(i) ? "bg-accent" : ""))}
      rowSelectable
      onRowPress={({ row }) => props.onSelect(row)}
      empty={<Empty text={t("common.empty")} />}
    />
  );
}

const time = (iso: string) => new Date(iso).getTime();

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

/** An operation: its title, notes and requests (`rows`); `onRecord` opens one. */
export function OperationView(props: { episode: Episode; shared: string[]; rows: RecordSummary[]; onRecord: (id: number) => void; onBack?: () => void; back?: JSX.Element }) {
  const notes = () => props.episode.notes.filter((n) => !props.shared.includes(n));
  return (
    <View class="gap-2" testID="operation-detail">
      <View class="flex-row items-center gap-2">
        {props.back}
        <If when={props.episode.format}>{(f) => <Badge label={f()} tone="info" />}</If>
        <Text class="flex-1 text-base font-semibold text-foreground">{props.episode.title}</Text>
        {outcomeBadge(props.episode)}
      </View>
      <Each of={notes()}>{(n) => <Muted class="text-sm">{n()}</Muted>}</Each>
      <Heading>{`${t("timeline.requestsOf")} (${props.episode.records.length})`}</Heading>
      <Each of={props.rows} key={(r) => r.id}>
        {(r, i) => (
          <BaseButton testID={`op-request-${i()}`} accessibilityLabel={`${r().operation} ${r().target}`} onPress={() => props.onRecord(r().id)} class="flex-row gap-2 rounded-(--radius) px-1 py-0.5 hover:bg-muted">
            <Text class={r().status === null || r().status! >= 500 ? "w-9 text-xs text-fail" : r().status! >= 400 ? "w-9 text-xs text-warn" : "w-9 text-xs text-muted-foreground"}>{r().status === null ? "—" : String(r().status)}</Text>
            <Text class="w-40 text-xs text-foreground line-clamp-1">{r().operation}</Text>
            <Text class="flex-1 text-xs text-muted-foreground line-clamp-1">{r().target}</Text>
            <Text class="w-14 text-right text-xs text-muted-foreground">{ms(r().latencyMs)}</Text>
          </BaseButton>
        )}
      </Each>
    </View>
  );
}
