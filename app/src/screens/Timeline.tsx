// The Timeline: what an application did to its object store. Sessions on
// the left (recorded with `sonde run`, or here); a session reads as
// operations — a SlateDB manifest write, an Iceberg commit, a multipart
// upload — each opening on its requests, each request on the inspector.
// In a terminal, the same levels stack: sessions › operations › requests ›
// request.

import { cleanup, Clipboard, computed, Scroll, signal, Text, View } from "nsh";
import { Button as BaseButton, DataTable } from "nsh/base";
import { bytes, date, ms } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { endpoint, endpoints, notify, report } from "../store";
import type { Episode, RecordingView, SessionInfo, SessionView } from "../types";
import { Badge, Button, Choice, Empty, Heading, Mono, Muted, Segmented, terminal } from "../ui/kit";
import { RecordDetail } from "./Requests";

type Policy = "allow" | "no-delete" | "read-only";

const seconds = (s: number) => (s < 10 ? s.toFixed(1) : s.toFixed(0));
/** Local time with milliseconds: 14:02:11.204 */
function clock(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

function copy(text: string | null) {
  if (!text) return;
  Clipboard.writeText(text).then(() => notify(t("common.copied")), report);
}

function outcomeBadge(e: Episode) {
  if (e.outcome.kind === "failed") return <Badge label={e.outcome.detail} tone="fail" />;
  if (e.outcome.kind === "refused") return <Badge label="refused" tone="warn" />;
  return null;
}

/** Start a recording, or show the one that runs. */
function RecordPanel(props: { recording: RecordingView | null; onChange: () => void }) {
  const open = signal(false);
  const policy = signal<Policy>("allow");
  const target = signal<string | null>(endpoint());
  const start = async () => {
    const ep = target() ?? endpoint();
    if (!ep) return;
    try {
      await api.sessions.start(ep, policy());
      open.set(false);
      props.onChange();
    } catch (e) {
      report(e);
    }
  };
  return (
    <If
      when={props.recording}
      else={
        <If when={open()} else={<Button label={t("timeline.record")} onPress={() => open.set(true)} testID="record" />}>
          <View class="gap-2 rounded-(--radius) border border-border p-2" testID="record-form">
            <Text class="text-sm font-semibold text-foreground">{t("timeline.recordTitle")}</Text>
            <Muted class="text-xs">{t("timeline.recordHint")}</Muted>
            <Choice label={t("common.endpoint")} value={target() ?? endpoint()} items={endpoints().map((e) => ({ value: e.id, title: e.name }))} onChange={(v) => target.set(v)} />
            <Text class="text-xs text-muted-foreground">{t("timeline.policy")}</Text>
            <Segmented
              value={policy()}
              items={[
                { value: "allow", title: t("timeline.allow") },
                { value: "no-delete", title: t("timeline.noDelete") },
                { value: "read-only", title: t("timeline.readOnly") },
              ]}
              onChange={(v) => policy.set(v as Policy)}
            />
            <View class="flex-row gap-2">
              <Button label={t("timeline.start")} onPress={start} disabled={!(target() ?? endpoint())} testID="record-start" />
              <Button variant="ghost" label={t("common.cancel")} onPress={() => open.set(false)} />
            </View>
          </View>
        </If>
      }
    >
      {(r) => (
        <View class="gap-2 rounded-(--radius) border border-primary p-2" testID="recording">
          <View class="flex-row items-center gap-2">
            <Text class="text-primary">●</Text>
            <Text class="flex-1 text-sm font-semibold text-foreground line-clamp-1">{t("timeline.recording", { url: r().url })}</Text>
          </View>
          <Muted class="text-xs">{t("timeline.pointApp")}</Muted>
          <Mono class="rounded-(--radius) bg-muted p-2 text-xs">{r().exports.trim()}</Mono>
          <View class="flex-row gap-2">
            <Button small label={t("timeline.copyExports")} onPress={() => copy(r().exports)} testID="copy-exports" />
            <Button
              small
              variant="destructive"
              label={t("timeline.stop")}
              onPress={() => {
                api.sessions.stop();
                props.onChange();
              }}
              testID="record-stop"
            />
          </View>
        </View>
      )}
    </If>
  );
}

function SessionList(props: { sessions: SessionInfo[]; selected: string | null; onSelect: (p: string) => void }) {
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

function Observations(props: { view: SessionView }) {
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

function SessionHeader(props: { view: SessionView }) {
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

function OperationsTable(props: { view: SessionView; onSelect: (i: number) => void }) {
  const notes = (e: Episode) => e.notes.filter((n) => !props.view.sharedNotes.includes(n)).join(" · ");
  const formats = () => props.view.episodes.some((e) => e.format);
  return (
    <DataTable
      testID="operations"
      class="flex-1 rounded-(--radius) border border-border"
      columns={[
        { key: "time", title: "Time", width: 100 },
        ...(formats() ? [{ key: "format", title: "", width: 64 }] : []),
        { key: "title", title: "Operation", width: 380 },
        { key: "requests", title: "Req", width: 44, align: "end" as const },
        { key: "duration", title: "Time", width: 68, align: "end" as const },
        { key: "notes", title: "", width: 320 },
      ]}
      data={props.view.episodes.map((e) => ({
        time: clock(e.start),
        format: e.format ?? "",
        title: e.outcome.kind === "ok" ? e.title : `${e.title} ✗`,
        requests: String(e.records.length),
        duration: ms(new Date(e.end).getTime() - new Date(e.start).getTime()),
        notes: notes(e),
      }))}
      rowClasses={props.view.episodes.map((e) => (e.outcome.kind === "failed" ? "text-fail" : e.outcome.kind === "refused" ? "text-warn" : ""))}
      rowSelectable
      onRowPress={({ row }) => props.onSelect(row)}
      empty={<Empty text={t("common.empty")} />}
    />
  );
}

function OperationDetail(props: { path: string; episode: Episode; shared: string[]; onRecord: (id: number) => void; onBack?: () => void }) {
  const rows = () => {
    try {
      return api.sessions.records(props.path, props.episode.records);
    } catch (e) {
      report(e);
      return [];
    }
  };
  const notes = () => props.episode.notes.filter((n) => !props.shared.includes(n));
  return (
    <View class="gap-2" testID="operation-detail">
      <View class="flex-row items-center gap-2">
        <If when={props.onBack}>
          <Button small variant="ghost" label={`‹ ${t("common.back")}`} onPress={() => props.onBack?.()} testID="operation-back" />
        </If>
        <If when={props.episode.format}>{(f) => <Badge label={f()} tone="info" />}</If>
        <Text class="flex-1 text-base font-semibold text-foreground">{props.episode.title}</Text>
        {outcomeBadge(props.episode)}
      </View>
      <Each of={notes()}>{(n) => <Muted class="text-sm">{n()}</Muted>}</Each>
      <Heading>{`${t("timeline.requestsOf")} (${props.episode.records.length})`}</Heading>
      <Each of={rows()} key={(r) => r.id}>
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

export function Timeline() {
  const sessions = signal<SessionInfo[]>(api.sessions.list());
  const recording = signal<RecordingView | null>(api.sessions.current());
  // a terminal starts on the list; a window on the newest session
  const selected = signal<string | null>(terminal() ? null : (sessions()[0]?.path ?? null));
  const op = signal<number | null>(null);
  const rec = signal<number | null>(null);
  const tick = signal(0);

  const refresh = () => {
    const list = api.sessions.list();
    const r = api.sessions.current();
    sessions.set(list);
    recording.set(r);
    if (r && selected() !== r.path) select(r.path);
    tick.set(tick() + 1);
  };
  // a running recording grows: follow it
  const timer = setInterval(() => {
    if (recording()) refresh();
  }, 1000);
  cleanup(() => clearInterval(timer));

  const select = (p: string | null) => {
    selected.set(p);
    op.set(null);
    rec.set(null);
  };
  const view = computed<SessionView | null>(() => {
    tick();
    const p = selected();
    if (!p) return null;
    try {
      return api.sessions.open(p);
    } catch (e) {
      report(e);
      return null;
    }
  });
  const episode = () => {
    const i = op();
    const v = view();
    return i === null || !v ? null : (v.episodes[i] ?? null);
  };
  const record = () => {
    const id = rec();
    const p = selected();
    return id === null || !p ? null : api.sessions.record(p, id);
  };
  const example = () => {
    try {
      const p = api.sessions.example();
      refresh();
      select(p);
    } catch (e) {
      report(e);
    }
  };

  const detail = (back: boolean) => (
    <If
      when={record()}
      else={
        <If when={episode()} else={<Empty text={t("requests.select")} />}>
          {(e) => <OperationDetail path={selected()!} episode={e()} shared={view()?.sharedNotes ?? []} onRecord={(id) => rec.set(id)} onBack={back ? () => op.set(null) : undefined} />}
        </If>
      }
    >
      {(r) => <RecordDetail record={r()} onBack={() => rec.set(null)} copyAs={(kind) => api.sessions.copy(selected()!, r().id, kind)} />}
    </If>
  );

  const sessionPane = (v: SessionView) => (
    <View class="flex-1 gap-2">
      <SessionHeader view={v} />
      <Observations view={v} />
      <OperationsTable view={v} onSelect={(i) => (op.set(i), rec.set(null))} />
      <Muted class="text-xs">{t("timeline.replayHint", { path: v.path })}</Muted>
    </View>
  );

  const sidebar = () => (
    <View class="gap-2">
      <RecordPanel recording={recording()} onChange={refresh} />
      <Heading>{t("timeline.sessions")}</Heading>
      <If when={sessions().length > 0} else={<Muted class="text-sm">{t("timeline.empty")}</Muted>}>
        <SessionList sessions={sessions()} selected={selected()} onSelect={select} />
      </If>
      <Button small variant="ghost" label={t("timeline.example")} onPress={example} testID="open-example" />
    </View>
  );

  return (
    <View class="flex-1 p-3" testID="screen-timeline">
      <If
        when={terminal()}
        else={
          <View class="flex-1 flex-row gap-3">
            <Scroll class="w-72" contentClass="pr-2">
              {sidebar()}
            </Scroll>
            <View class="flex-1">
              <If when={view()} else={<Empty text={sessions().length ? t("timeline.pick") : t("timeline.empty")} />}>
                {(v) => sessionPane(v())}
              </If>
            </View>
            <If when={episode() || record()}>
              <Scroll class="w-[420px] border-l border-border pl-3">{detail(false)}</Scroll>
            </If>
          </View>
        }
      >
        {/* one level at a time: sessions › operations › requests › request */}
        <If when={selected() && view()} else={<Scroll class="flex-1">{sidebar()}</Scroll>}>
          <If when={episode() || record()} else={<View class="flex-1 gap-1">
            <Button small variant="ghost" label={`‹ ${t("timeline.sessions")}`} onPress={() => select(null)} testID="sessions-back" />
            {sessionPane(view()!)}
          </View>}>
            <Scroll class="flex-1">{detail(true)}</Scroll>
          </If>
        </If>
      </If>
    </View>
  );
}
