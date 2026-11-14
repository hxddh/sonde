// A session on screen, with no state of the app's own: its header and
// figures, what stands out (each a filter of the operations), the
// operations with their outcome and place in time, the program's output,
// and how output and operations line up. The app and the web viewer
// (sonde share) both draw with these.

import { Clipboard, effect, type JSX, List, Scroll, signal, Text, token, untracked, View, type ViewRef } from "nsh";
import { DataTable, type DataTableRef, type DataTableSort, TextView } from "nsh/base";
import { change, clock, count, date, duration, middle, transfer } from "../format";
import { t } from "../i18n";
import { episodeNotes, episodeTitle, observationSaving, observationText, observationWhy, say } from "../phrases";
import { Chart, sessionStory, Sparkline, timed, Waterfall, MARK } from "./plots";
import { type SignalId, signalsOf, signalTiles } from "../signals";
import type { Operations } from "../operations";
import type { EpisodeRow, FindingChange, LogLine, Observation, RecordSummary, SessionView } from "../types";
import { Icon } from "./icon";
import { Badge, Button, Card, CARD, Disclosure, Dot, EMPHASIS, Empty, fitColumns, Heading, ICON, ListRow, Options, Path, ROW, rowHeight, Stat, TABLE, terminal, type TileLook, Tiles, Title, TYPE, SIZES, WIDTH, COLUMN } from "./kit";
import { rememberedSize } from "./sizes";

import { commandLine } from "../command";

export { commandLine, programOf, ranCommand, scrubCommand } from "../command";
export { SessionList, tellApart } from "./sessionlist";

export { clock };

const time = (iso: string) => new Date(iso).getTime();

/** Where a finding stands against its program's run before (crates/proxy/src/fixloop.rs). */
export type FindingState = FindingChange["state"];
const STATE_TONE: Record<FindingState, "pass" | "info" | "warn" | "neutral"> = { fixed: "pass", less: "info", back: "warn", new: "info", open: "neutral" };
/** A state as the reader's language says it ("Fixed", "又出现"). */
export const stateLabel = (s: FindingState): string => {
  const w = say([{ key: "fix." + s, args: {} }], s);
  return w.charAt(0).toUpperCase() + w.slice(1);
};
/** A finding's state, as a badge. */
export function StateBadge(props: { state: FindingState; testID?: string }) {
  return <Badge testID={props.testID} label={stateLabel(props.state)} tone={STATE_TONE[props.state]} />;
}

export type Tone = "ok" | "miss" | "client" | "server" | "refused";

/** How an operation ended, as a status color: lookups that found nothing are not errors. */
export function episodeTone(e: Pick<EpisodeRow, "outcome" | "miss" | "notFound" | "requests">): Tone {
  if (e.outcome.kind === "refused") return "refused";
  if (e.outcome.kind === "failed") return /^4\d\d/.test(e.outcome.detail) ? "client" : "server";
  if (e.miss) return "miss";
  // every request a lookup that found nothing
  if (e.notFound && e.notFound === e.requests) return "miss";
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
  miss: "text-muted-foreground",
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

/** An HTTP status, the same everywhere: a dot in its tone and the code, a
 * success quiet, anything else in its tone (none: the request never got
 * an answer). A request a fault scenario answered (`injected`: its name) is
 * in the injected tone whatever it looked like, and says so to a screen reader:
 * the store is not blamed for it. */
export function StatusMark(props: { status: number | null; text?: string; injected?: string | null; /** S3's code for it, where known */ code?: string | null }) {
  // a key that is not there is a lookup (a miss); a bucket that is not there is a mistake, as the errors count it
  const tone = () => (props.status === 404 && props.code === "NoSuchBucket" ? "client" : statusTone(props.status));
  const code = () => props.text ?? (props.status === null ? "—" : `${props.status}`);
  return (
    <View class="flex-row items-center gap-2" accessibilityLabel={props.injected ? `${code()}, ${t("plots.injected")}` : undefined}>
      <Dot tone={props.injected ? "injected" : tone()} />
      {/* emphasis: a status that is not a success stands out in its tone */}
      <Text class={`font-mono text-caption tabular-nums ${props.injected ? `${EMPHASIS} text-status-injected` : tone() === "ok" ? "text-muted-foreground" : `${EMPHASIS} ${TONE_TEXT[tone()]}`}`}>{code()}</Text>
    </View>
  );
}

const FORMAT_TEXT: Record<string, string> = { SlateDB: "text-fmt-slatedb", Iceberg: "text-fmt-iceberg", Delta: "text-fmt-delta", Lance: "text-fmt-lance" };
/** An operation's kind as a tag, in the app's language. */
const kindLabel = (k: EpisodeRow["kind"]): string => (k === "multipart" ? t("kind.multipart") : k === "retry" ? t("kind.retry") : k === "listing" ? t("kind.listing") : k === "probe" ? t("kind.probe") : "");

/** What kind of thing an operation is: its storage format, else its kind. */
function family(e: Pick<EpisodeRow, "format" | "kind">): { label: string; class: string } {
  if (e.format) return { label: e.format, class: FORMAT_TEXT[e.format] ?? "text-fmt-other" };
  const k = kindLabel(e.kind);
  return k ? { label: k, class: e.kind === "multipart" ? "text-fmt-mpu" : "text-fmt-other" } : { label: "", class: "" };
}

/** How long an operation ran, in ms (a number: `duration` writes it). */
export const episodeMs = (e: Pick<EpisodeRow, "start" | "end">) => time(e.end) - time(e.start);

/** The session's span in time: its first operation's start, its last one's end. */
export function span(v: SessionView): [number, number] {
  if (!v.span) return [0, 1];
  const a = time(v.span.start);
  return [a, Math.max(time(v.span.end), a + 1)];
}

/** The commit a run's code was at, as a person reads it: a1b2c3d (main), changed. */
export function gitText(g: SessionView["header"]["git"]): string {
  if (!g) return "";
  return `${g.commit.slice(0, 7)}${g.branch ? ` (${g.branch})` : ""}${g.dirty ? ` · ${t("git.dirty")}` : ""}`;
}

/** The command and where it ran, then the session's figures. */
export function SessionHeader(props: { view: SessionView; actions?: JSX.Element; /** a press on a signal: its chart, on the Time pane */ onSignal?: (id: SignalId) => void; /** what marks the session (a trial of another run), under its title */ children?: JSX.Element; /** the pane is under `BREAK.short`: the signals keep to a line */ short?: boolean }) {
  const s = () => props.view.summary;
  const line = () => {
    const parts = [t("timeline.operations", { n: count(s().requests), ops: t("figure.operationsN", { n: count(props.view.operationsTotal) }), secs: duration(s().seconds * 1000) })];
    if (s().errors) parts.push(t("timeline.errors", { n: s().errors }));
    if (s().misses) parts.push(t("timeline.notFound", { n: s().misses }));
    parts.push(transfer(s().bytesUp, s().bytesDown));
    return parts.join(" · ");
  };
  const command = () => props.view.header.command;
  if (terminal()) {
    return (
      <View class="gap-0">
        <Title>{command() ? commandLine(command()!) : props.view.header.endpoint}</Title>
        {props.children}
        <Text class="text-muted-foreground" testID="session-summary">
          {line()}
        </Text>
        <Text class="text-foreground" testID="session-story">{sessionStory(props.view.profile)}</Text>
      </View>
    );
  }
  const profile = () => props.view.profile;
  // the four signals, then in flight: by rank, so a narrow pane drops the last first
  const tiles = () => {
    const x = signalsOf(props.view);
    return x ? signalTiles(x) : [];
  };
  const tile = (i: number) => (look: () => TileLook) => {
    const x = () => tiles()[i];
    return (
      <If when={x()}>
        <Stat
          look={look()}
          testID={`signal-${SIGNALS[i]}`}
          label={x().label}
          value={x().value}
          detail={x().detail}
          tone={x().tone}
          hint={props.onSignal ? t("signal.open") : undefined}
          onPress={props.onSignal ? () => props.onSignal?.(SIGNALS[i]) : undefined}
          mark={<Sparkline shape={x().series} id={`signal:${props.view.path}:${SIGNALS[i]}`} still class={MARK.tile} kind={x().kind} tone={((k) => (k === "quiet" ? "plain" : k))(x().tone)} />}
        />
      </If>
    );
  };
  return (
    <View class="gap-3">
      <View class="flex-row items-start gap-3">
        <View selectable class="min-w-0 flex-1 gap-1">
          <Title mono small lines={2}>
            {command() ? commandLine(command()!) : props.view.header.endpoint}
          </Title>
          <Text testID="session-meta" class="text-small text-muted-foreground line-clamp-1">{[props.view.header.endpoint, date(props.view.header.created), gitText(props.view.header.git), props.view.sdk ? t("session.sdk", { sdk: props.view.sdk }) : ""].filter(Boolean).join(" · ")}</Text>
        </View>
        {props.actions}
      </View>
      {/* the session in a sentence: where its time went, and the first thing to do. Across the pane
          (not held to the title's column beside the commands); it wraps to two lines as the pane
          narrows, and what is past them is cut at the second line's end */}
      <If when={profile()?.groups.length}>
        <Text testID="session-story" class="text-body leading-5 text-foreground line-clamp-2">{sessionStory(profile())}</Text>
      </If>
      {props.children}
      {/* one reading of a run: rate, errors, latency, bytes, and how many at once; each opens its chart */}
      {/* (a page shared before sessions had a profile: its figures in a line) */}
      <If when={tiles().length > 0} else={<Text testID="session-summary" accessibilityLabel={line()} class="tabular-nums text-small text-muted-foreground">{line()}</Text>}>
        <Tiles ranked short={props.short} min={SIZES.signalTile} testID="session-summary" label={line()} items={SIGNALS.map((_, i) => tile(i))} />
      </If>
    </View>
  );
}

/** The signals in rank order (the least width a tile of one reads in is the kit's `SIZES.signalTile`). */
const SIGNALS: SignalId[] = ["rate", "errors", "latency", "bytes", "inFlight"];

const SDKS = { python: "Python · boto3", java: "Java SDK v2", go: "Go SDK v2", js: "JavaScript v3" } as const;
type Sdk = keyof typeof SDKS | "config";
/** A fix's tab: the SDK's own name, or (a change to the bucket, not the program) "Bucket" in the reader's language. */
const sdkTitle = (sdk: Sdk) => (sdk === "config" ? t("fix.bucket") : SDKS[sdk]);
const LANG = { python: "python", java: "java", go: "go", js: "javascript", config: "xml" } as const;

/** What to change for a finding: its fix in each SDK, one shown. */
/** The fix's SDK for the program's own: its user agent named it. */
const sdkOfAgent = (sdk: string | undefined): Sdk | null => {
  if (!sdk) return null;
  const s = sdk.toLowerCase();
  return s.startsWith("boto") || s.startsWith("aws-cli") || s.startsWith("s3fs") ? "python" : s.includes("java") || s.includes("hadoop") ? "java" : s.includes("go") ? "go" : s.includes("-js") ? "js" : null;
};

/** "Expected": the finding's line for sonde.toml's `expect`, copied (the app's own; not in a shared page). */
function ExpectButton(props: { o: Observation; onExpect?: (o: Observation) => void }) {
  return (
    <If when={props.onExpect && props.o.identity && !terminal()}>
      <Button small variant="ghost" icon="check" label={t("expect.copy")} tip={t("expect.hint")} onPress={() => props.onExpect?.(props.o)} testID="finding-expect" />
    </If>
  );
}

/** A fix's code as written, without the blank lines around it. */
const sample = (code: string) => code.replace(/^\s*\n|\s+$/g, "");

/** The findings whose fix is a tool of Buckets: the section that acts on it. */
export const STORE_FIX: Record<string, "multipart"> = { "obs.unfinished": "multipart" };
export const storeFixOf = (o: Observation) => STORE_FIX[o.phrases?.[0]?.key ?? ""] ?? null;

/** Where a finding's fix is in Buckets (an incomplete upload's: the uploads, to abort them or add a rule): a press goes there. */
function StoreFixButton(props: { o: Observation; onStore?: (o: Observation) => void }) {
  return (
    <If when={props.onStore && storeFixOf(props.o)}>
      <Button small variant="outline" icon="layers" label={t("fix.openUploads")} onPress={() => props.onStore?.(props.o)} testID="finding-store" />
    </If>
  );
}

function FixPanel(props: { o: Observation; /** the SDK the fixes are shown for: the pane's own, kept from finding to finding */ sdk: { (): Sdk; set: (s: Sdk) => void }; onOpenCode?: (place: string) => void; onExpect?: (o: Observation) => void; onStore?: (o: Observation) => void }) {
  const fixSdk = props.sdk;
  const fixes = () => props.o.fixes ?? [];
  // the program's own SDK first, when its requests said which; else the one last chosen
  const own = () => sdkOfAgent(props.o.sdk);
  const shown = () => fixes().find((f) => f.sdk === fixSdk()) ?? fixes().find((f) => f.sdk === own()) ?? fixes()[0];
  effect(() => {
    const o = own();
    if (o && fixes().some((f) => f.sdk === o)) untracked(() => fixSdk.set(o));
  });
  return (
    <View testID="fix" class="gap-2">
      <View class="flex-row flex-wrap items-center gap-2">
        <Icon name="sparkles" size={ICON.small} />
        <Heading class="flex-1">{t("fix.title")}</Heading>
        <Options testID="fix-sdk" value={shown()?.sdk ?? ""} items={fixes().map((f) => ({ value: f.sdk, title: sdkTitle(f.sdk) }))} onChange={(v) => fixSdk.set(v as Sdk)} />
        <If when={shown() && !terminal()}>
          <Button small variant="ghost" icon="copy" label={t("fix.copy")} onPress={() => shown() && void Clipboard.writeText(sample(shown()!.code)).catch(() => {})} testID="fix-copy" />
        </If>
        {/* the store's side of the fix, where it has one; or no change: a habit kept on purpose, which sonde check then expects */}
        <StoreFixButton o={props.o} onStore={props.onStore} />
        <ExpectButton o={props.o} onExpect={props.onExpect} />
      </View>
      {/* why it matters and what it saves, written out */}
      <If when={observationWhy(props.o)}>
        {(why) => (
          <TextView
            testID="fix-why"
            markdown={`${why()}${observationSaving(props.o) ? `\n\n**${observationSaving(props.o)}**` : ""}`}
            // emphasis: what the change saves, within the reason for it
            classes={{ paragraph: "text-small leading-snug text-muted-foreground", strong: `${EMPHASIS} text-info` }}
          />
        )}
      </If>
      {/* where in the program, when it said: the place to change */}
      <If when={props.o.callers?.length}>
        <View testID="fix-callers" class="gap-1">
          <Each of={props.o.callers!} key={(c) => c.place}>
            {(c) => (
              // a shared page cannot open the code: the place is said, not offered (a row that does nothing on a press)
              <ListRow label={props.onOpenCode ? t("fix.openCode", { place: c().place }) : c().place} disabled={!props.onOpenCode} onPress={() => props.onOpenCode?.(c().place)} trailing={<Text class="tabular-nums text-caption text-muted-foreground">{t("run.bandRequests", { n: c().requests })}</Text>}>
                <Icon name="file" size={ICON.small} />
                <Text class={`min-w-0 flex-1 font-mono text-small line-clamp-1 ${props.onOpenCode ? "text-primary" : "text-foreground"}`}>{c().place}</Text>
              </ListRow>
            )}
          </Each>
        </View>
      </If>
      <If when={shown()}>
        {(f) => (
          <If when={!terminal()} else={<Text class="font-mono text-foreground">{f().code}</Text>}>
            {/* highlighted; a line too long for the card scrolls sideways under its own indicator (Native Shell's
                TextView); its Copy is beside the SDKs, so no row stands empty over the code for it */}
            <TextView
              testID="fix-code"
              // (the sample as written, without the blank lines around it)
              markdown={`\`\`\`${LANG[f().sdk]}\n${sample(f().code)}\n\`\`\``}
              classes={{ codeBlock: "rounded-(--radius) border border-border bg-background p-2 font-mono text-small" }}
            />
          </If>
        )}
      </If>
    </View>
  );
}

/** Where each finding begins, by its number in the list: for the shape. */
export function findingMarks(v: SessionView): { tMs: number; n: number }[] {
  const t0 = v.span ? Date.parse(v.span.start) : 0;
  return v.observations.flatMap((o, i) => (o.firstStart ? [{ tMs: Math.max(0, Date.parse(o.firstStart) - t0), n: i + 1 }] : []));
}

/** Acting on every finding at once, as an estimate: requests, waiting, cost. */
export function whatIfText(v: SessionView): string | null {
  const w = v.whatIf;
  if (!w) return null;
  // the cost as what changes: amounts of thousandths of a cent read as nothing
  const cut = w.cost[0] > 0 ? change(w.cost[0], w.cost[1]) : "—";
  // a log without times says nothing of waiting
  if (!timed(v.profile) || !w.waitingMs) return t("whatIf.untimed", { n: w.findings, a: w.requests[0], b: w.requests[1], cut });
  return t("whatIf.text", { n: w.findings, a: w.requests[0], b: w.requests[1], wa: duration(w.waitingMs[0]), wb: duration(w.waitingMs[1]), cut });
}

/** What stands out in a session — its core result. Each finding says what
 * happened and why it matters, and narrows the operations to those it is
 * about. `compact` (beside a run's live operations): the sentences only. */
export function Observations(props: { view: SessionView; filter?: number | null; onFilter?: (i: number | null) => void; compact?: boolean; dense?: boolean; narrow?: boolean; max?: number; maxHeight?: number; onOpenCode?: (place: string) => void; onExpect?: (o: Observation) => void; /** a finding whose fix is a tool of Buckets: open it there */ onStore?: (o: Observation) => void; /** what acting on them saves as a line under the findings (a narrow place), not beside their heading */ whatIfUnder?: boolean; /** each finding's state against the program's run before, by its identity (fixloop's `statesOf`) */ states?: Record<string, FindingChange>; /** what acting on a finding saves under its words, never beside them (a narrow column: Run's) */ savingUnder?: boolean }) {
  const press = (i: number) => props.onFilter?.(props.filter === i ? null : i);
  /** How many operations a finding is about (a view lists the first of them only). */
  const about = (o: Observation) => o.operationsTotal ?? o.operations?.length ?? 0;
  // a finding pressed opens what to change inside its own row, under its title; the row comes
  // to the top of the findings' room, so its title stays in view over its advice
  // (Native Shell's `reveal`: the scroll views around a view bring it to show)
  const places: (ViewRef | undefined)[] = [];
  // each finding's height as drawn: a bounded room ends on a finding's edge, never through its text
  const heights = signal<number[]>([]);
  const measured = (i: number) => ({ bounds }: { bounds: { size: { height: number } } }) => {
    const h = Math.round(bounds.size.height);
    if (heights()[i] === h) return;
    const next = [...heights()];
    next[i] = h;
    heights.set(next);
  };
  /** The room the findings take, and how many show whole in it: all of them where they fit; else
   * as many whole ones as fit (the rest scroll, and the heading says so). A finding opened taller
   * than the room keeps the room: its advice scrolls within it. */
  const room = (): { height?: number; shown: number } => {
    const n = list().length;
    const max = props.maxHeight;
    const hs = heights().slice(0, n);
    if (!max || hs.length < n || hs.some((h) => h === undefined)) return { height: max, shown: n };
    // (the card's border, above and below)
    let used = 2;
    let shown = 0;
    for (const h of hs) {
      if (used + h > max) break;
      used += h;
      shown++;
    }
    if (shown === n) return { height: max, shown };
    if (shown === 0 || hs.some((h) => h > max)) return { height: max, shown: Math.max(shown, 1) };
    return { height: used, shown };
  };
  const showChosen = () => {
    const f = props.filter;
    if (f != null) places[f]?.reveal({ vertical: "start" });
  };
  effect(() => {
    if (props.filter != null) showChosen();
  });
  /** Whether a finding has anything to open: its fixes, or (the app's own) a line for sonde.toml. */
  const advised = (o: Observation) => !!o.fixes?.length || !!(props.onExpect && o.identity) || !!(props.onStore && storeFixOf(o));
  const list = () => props.view.observations.slice(0, props.max ?? props.view.observations.length);
  /** Its state against the run before, where it tells something: new, back or less (open is left unmarked). */
  const state = (o: Observation) => {
    const f = o.identity ? props.states?.[o.identity] : undefined;
    return f && f.state !== "open" && f.state !== "fixed" ? f.state : null;
  };
  if (terminal()) {
    return (
      <If when={props.view.observations.length > 0}>
        <View testID="observations">
          <Each of={list()}>
            {(o, i) => (
              // a finding narrows the operations to its own, as on the desktop
              <ListRow
                plain
                testID={`observation-${i()}`}
                label={observationText(o())}
                selected={props.filter === i()}
                disabled={!props.onFilter || !about(o())}
                onPress={() => press(i())}
              >
                <Text class={o().level === "warn" ? "text-warn" : "text-muted-foreground"}>
                  {o().level === "warn" ? "! " : "· "}
                  <If when={state(o())}>{(st) => <Text class="text-info">{`[${stateLabel(st())}] `}</Text>}</If>
                  {observationText(o())}
                  <If when={!props.compact && observationSaving(o())}>{(s) => <Text class="text-status-ok">{` — ${s()}`}</Text>}</If>
                </Text>
              </ListRow>
            )}
          </Each>
        </View>
      </If>
    );
  }
  // a line each, but the one chosen in full
  const dense = () => props.compact || !!props.dense;
  // whether the findings are folded away, and the SDK their fixes are shown for: this pane's own
  const open = signal(true);
  const fixSdk = signal<Sdk>("python");
  const saving = (v: () => string, i: () => number) => (
    <View class="shrink-0 flex-row items-center gap-1">
      <Icon name="trending-down" size={ICON.small} tint="info" />
      <Text testID={`observation-saving-${i()}`} class="tabular-nums text-small text-info">{v()}</Text>
    </View>
  );
  /** A finding whose advice is open under it, with fixes: the advice says why and what it saves. */
  const opened = (o: () => Observation, i: () => number) => !props.compact && props.filter === i() && !!o().fixes?.length;
  const row = (o: () => Observation, i: () => number) => (
    <ListRow
      divided
      top
      first={i() === 0}
      testID={`observation-${i()}`}
      label={observationText(o())}
      selected={props.filter === i()}
      disabled={!props.onFilter || !about(o())}
      onPress={() => press(i())}
      class={dense() ? "py-1" : ""}
      // by rank as the pane narrows: the finding's words keep their width; what acting on it
      // saves goes first, then the count's words (its chevron stays)
      trailing={
        <>
          <If when={dense() && props.filter !== i() && !props.compact && !props.narrow && !props.savingUnder && observationSaving(o())}>{(v) => saving(v as () => string, i)}</If>
          <If when={props.onFilter && about(o())}>
            <View class={`${props.narrow ? "" : WIDTH.count} flex-row items-center justify-end gap-1`}>
              <If when={!props.narrow}>
                <Text class="tabular-nums text-caption text-muted-foreground group-selected:text-foreground">{t("figure.operationsN", { n: count(about(o())) })}</Text>
              </If>
              <Icon name={props.filter === i() ? "x" : "chevron-right"} size={ICON.small} />
            </View>
          </If>
        </>
      }
    >
      {/* its number, in its level's tone: where it stands on the session's shape */}
      <View class={`mt-1 size-4 shrink-0 items-center justify-center rounded-full ${o().level === "warn" ? "bg-warn/15" : "bg-muted"}`}>
        {/* emphasis: the number the shape marks this finding with */}
        <Text class={`tabular-nums text-caption ${EMPHASIS} ${o().level === "warn" ? "text-warn" : "text-muted-foreground"}`}>{count(i() + 1)}</Text>
      </View>
      <View class="min-w-0 flex-1 gap-1">
        <View class="flex-row items-start gap-2">
          {/* where it stands against the program's run before: new, back, less */}
          <If when={state(o())}>
            {(st) => (
              <View class="min-h-5 justify-center">
                <StateBadge state={st()} testID={`observation-state-${i()}`} />
              </View>
            )}
          </If>
          <Text class={`min-w-0 flex-1 text-body text-foreground ${dense() && props.filter !== i() ? "line-clamp-1" : "line-clamp-2"}`}>{observationText(o())}</Text>
        </View>
        {/* why it matters and what acting on it saves: here, or (the finding opened, with fixes) once, in its advice */}
        <If when={(!dense() || props.filter === i()) && !opened(o, i) && observationWhy(o())}>{(w) => <Text class="text-small leading-snug text-muted-foreground line-clamp-2">{w()}</Text>}</If>
        <If when={(!dense() || props.filter === i() || props.savingUnder) && !opened(o, i) && !props.compact && observationSaving(o())}>{(v) => saving(v as () => string, i)}</If>
      </View>
    </ListRow>
  );
  const findings = () => (
    // a short window keeps room for the operations: the findings scroll
    <Scroll class={`overflow-hidden ${CARD}`} style={room().height ? { maxHeight: room().height } : undefined} scrollbar={false}>
      <Each of={list()}>
        {(o: () => Observation, i) => (
          <View ref={(r) => (places[i()] = r)} onLayout={measured(i())}>
            {row(o, i)}
            {/* the finding pressed: what to change, inside its own row — never under another finding's title */}
            <If when={!props.compact && props.filter === i() && advised(o())}>
              <View
                testID="finding-advice"
                ref={() => showChosen()}
                class="border-t border-border bg-muted/40 p-3"
              >
                <If
                  when={o().fixes?.length}
                  else={
                    <View class="flex-row items-center gap-2">
                      <Text class="min-w-0 flex-1 text-small text-muted-foreground">{t("expect.hint")}</Text>
                      <StoreFixButton o={o()} onStore={props.onStore} />
                      <ExpectButton o={o()} onExpect={props.onExpect} />
                    </View>
                  }
                >
                  <FixPanel o={o()} sdk={fixSdk} onOpenCode={props.onOpenCode} onExpect={props.onExpect} onStore={props.onStore} />
                </If>
              </View>
            </If>
          </View>
        )}
      </Each>
    </Scroll>
  );
  return (
    <If when={props.view.observations.length > 0}>
      {/* beside a run's live operations: the sentences only, under no heading */}
      <If when={!props.compact} else={<View testID="observations">{findings()}</View>}>
        {/* the findings open and close under their heading; what acting on them saves is the
            section's subtitle beside it, not a third voice */}
        <Disclosure
          title={t("obs.title")}
          count={props.view.observations.length}
          expanded={open()}
          onToggle={(o) => open.set(o)}
          itemTestID="observations"
          testID="observations-toggle"
          trailing={
            <>
              {/* the room shows some of them whole: the rest scroll, and that is said */}
              <If when={open() && room().shown < list().length}>
                <Text testID="observations-scroll" class="shrink-0 tabular-nums text-caption text-muted-foreground">{t("obs.scrolls", { n: count(room().shown), of: count(list().length) })}</Text>
              </If>
              {/* the first few of more: said, so the count beside the heading is not read as theirs */}
              <If when={list().length < props.view.observations.length}>
                <Text testID="observations-top" class="shrink-0 tabular-nums text-caption text-muted-foreground">{t("obs.top", { n: count(list().length), of: count(props.view.observations.length) })}</Text>
              </If>
              <If when={!props.whatIfUnder && whatIfText(props.view)}>{(w) => <Text testID="what-if" class="min-w-0 flex-1 tabular-nums text-small text-muted-foreground line-clamp-1">{w()}</Text>}</If>
            </>
          }
        >
          <If when={props.whatIfUnder && whatIfText(props.view)} else={findings()}>
            {(w) => (
              <View class="gap-2">
                {findings()}
                {/* the one figure for acting on them: a line of its own, which wraps rather than being cut */}
                <Text testID="what-if" class="px-1 tabular-nums text-small text-muted-foreground">{w()}</Text>
              </View>
            )}
          </If>
        </Disclosure>
      </If>
    </If>
  );
}

const tableClasses = {
  ...TABLE,
  rowActive: "bg-row-hover",
  resizeHandle: "bg-transparent group-hover:bg-border",
};

/** Whether a table following its end goes to its newest row: rows were added
 * (`total` of them now, `known` before) and its view ended at the last row
 * there was (`endSeen`: the end of the rows in view at its last report, or 0
 * when it has reported none). A reader who went up to look at a row is left
 * there; back at the end, the table follows again. */
export const followsEnd = (total: number, known: number, endSeen: number): boolean => total > known && endSeen >= known;

/** A session's operations: outcome, time, family, title, requests, duration,
 * place in the session's time and notes. `rows` (default all) are the
 * episodes shown, by index; events name episodes by index too. */
/** The operations' columns as the user last sized them (kept with the app's sizes). */
const opsWidths = { bar: rememberedSize("ops.bar", 150), notes: rememberedSize("ops.notes", 320) };
const kept = (key: keyof typeof opsWidths, fallback: number) => untracked(() => opsWidths[key].size()) || fallback;

/** The operations as a table to paste (tab-separated), in the order given:
 * figures as plain numbers in one unit each, named in the column's title —
 * what the table shows ("840 ms", "2.35 s") would not sort or sum. */
export function operationsText(view: SessionView, rows: EpisodeRow[]): string {
  const plain = (s: string) => s.replace(/[\t\r\n]+/g, " ");
  const head = [t("column.time"), t("column.operation"), t("column.requests"), t("sessions.tookMs"), t("column.notes")];
  // plain numbers on purpose (to sort and sum where they are pasted): not `count()`, not `duration()`
  const body = rows.map((e) => [e.start, plain(episodeTitle(e)), `${e.requests}`, `${Math.round(episodeMs(e) * 1000) / 1000}`, plain(episodeNotes(e, view.sharedNotes).join(" · "))]);
  return [head, ...body].map((r) => r.join("\t")).join("\n");
}

/** The operations' columns by rank. The outcome's dot, the operation and how
 * long it took always show; then, while the operation keeps `OP_NAME` of
 * room: requests, time, when (its bar), format, notes — in that order, so a
 * table that narrows loses its last-ranked columns whole and none is ever
 * clipped. They stand in rank order too: DataTable keeps a column's state
 * by its place, so only columns at the end may go. */
export type OpColumn = "status" | "title" | "duration" | "requests" | "time" | "bar" | "format" | "notes";
export const OP_NAME = 240;
const OP_WIDTH: Record<Exclude<OpColumn, "title" | "bar" | "notes">, number> = { status: 26, duration: 96, requests: 92, time: 110, format: 86 };
const OP_RANKED = ["requests", "time", "bar", "format", "notes"] as const;
/** Which columns a table `width` wide shows (0: not measured yet, all that are
 * wanted), and the room the operation's name has. */
export function operationColumns(width: number, wanted: { format: boolean; bar: boolean; notes: boolean }, sized: { bar: number; notes: number }): { shown: OpColumn[]; title: number; widths: Record<string, number> } {
  const widths: Record<string, number> = { ...OP_WIDTH, bar: sized.bar, notes: sized.notes };
  const want = (c: (typeof OP_RANKED)[number]) => (c === "format" ? wanted.format : c === "bar" ? wanted.bar : c === "notes" ? wanted.notes : true);
  const always = widths.status + widths.duration;
  // (2: the table's border)
  const ranked = fitColumns(width, always + 2, OP_NAME, OP_RANKED.filter(want).map((key) => ({ key, width: widths[key] })));
  const used = always + ranked.reduce((n, c) => n + widths[c], 0);
  return { shown: ["status", "title", "duration", ...ranked], title: width > 0 ? Math.max(COLUMN.titleLeast, width - used - 2) : COLUMN.title, widths };
}

export function OperationsTable(props: {
  view: SessionView;
  /** The rows: a window over however many operations there are, sorted and narrowed where they are kept. */
  source: Operations;
  /** A row pressed: its operation, by index. */
  onSelect: (index: number) => void;
  onContextMenu?: (index: number) => void;
  ref?: (r: DataTableRef) => void;
  /** The operations marked (chosen, or running when an output line was printed), by index. */
  highlight?: number[];
  compact?: boolean;
  /** A session still being recorded: the table opens at its newest operation and stays at its end as more arrive, until the reader scrolls up (back at the end, it follows again). */
  follow?: boolean;
}) {
  const notes = (e: EpisodeRow) => episodeNotes(e, props.view.sharedNotes).join(" · ");
  // the slowest or busiest first, at a press on their column's title: sorted where the operations are kept
  const sort = signal<DataTableSort | null>(null);
  const sortBy = (next: DataTableSort | null) => {
    sort.set(next);
    const { sort: _, descending: __, ...rest } = props.source.query();
    props.source.ask(next ? { ...rest, sort: next.key === "duration" ? "duration" : next.key === "requests" ? "requests" : "time", descending: next.direction === "descending" } : rest);
  };
  // following the end: where the table's view ended at its last report, against how many rows there were then
  let table: DataTableRef | undefined;
  let endSeen = 0;
  let known = 0;
  effect(() => {
    const n = props.source.total();
    const follows = !!props.follow && sort() === null;
    untracked(() => {
      if (follows && followsEnd(n, known, endSeen)) setTimeout(() => table?.scrollToRow(n - 1), 0);
      known = n;
    });
  });
  const held = () => props.source.rows();
  const start = () => props.source.start();
  const formats = () => Object.keys(props.view.families.formats).length > 0 || ["multipart", "retry", "listing", "probe"].some((k) => (props.view.families.kinds[k] ?? 0) > 0);
  const range = () => span(props.view);
  // a row of the table (counted from the first of them all) as the operation it shows, once its page is held
  const ep = (row: number): EpisodeRow | undefined => held()[row - start()];
  // the table's own width decides what it shows (`operationColumns`); the operation's name takes the rest
  const width = signal(0);
  const layout = () =>
    operationColumns(
      terminal() ? 0 : width(),
      // beside the output or a terminal there is no room for a tag most rows do not have
      { format: formats() && !props.compact, bar: !terminal() && !props.compact, notes: !props.compact },
      { bar: kept("bar", 150), notes: kept("notes", 320) },
    );
  const titleWidth = () => (terminal() || width() > 0 ? layout().title : props.compact ? COLUMN.titleCompact : COLUMN.title);
  const columns = () => {
    const l = layout();
    const column = (key: OpColumn) => {
      switch (key) {
        case "status":
          return { key, title: "", width: l.widths.status, resizable: false, selectable: false };
        case "title":
          return { key, title: t("column.operation"), width: titleWidth(), minWidth: COLUMN.titleLeast };
        case "duration":
          return { key, title: t("sessions.took"), width: l.widths.duration, align: "end" as const, sortable: true };
        case "requests":
          return { key, title: t("column.requests"), width: l.widths.requests, align: "end" as const, sortable: true };
        case "time":
          return { key, title: t("column.time"), width: l.widths.time, sortable: true };
        case "bar":
          return { key, title: t("column.when"), width: l.widths.bar, minWidth: COLUMN.whenLeast };
        case "format":
          return { key, title: t("column.format"), width: l.widths.format };
        case "notes":
          return { key, title: t("column.notes"), width: l.widths.notes, minWidth: COLUMN.notesLeast };
      }
    };
    // a terminal draws no dot and reads left to right as a log does: when, what, how many, how long
    const order: OpColumn[] = terminal() ? (["time", "format", "title", "requests", "duration", "notes"] as OpColumn[]).filter((k) => l.shown.includes(k)) : l.shown;
    return order.map(column);
  };
  const data = () =>
    held().map((e) => ({
      status: episodeTone(e),
      time: clock(e.start),
      format: family(e).label,
      // a terminal cuts it by a count of characters; a window where it runs out of room (the cell's own middle ellipsis)
      title: terminal() ? middle(e.outcome.kind !== "ok" ? `${episodeTitle(e)} ✗` : episodeTitle(e), props.compact ? 40 : 72) : episodeTitle(e),
      requests: count(e.requests),
      duration: duration(episodeMs(e)),
      bar: "",
      notes: notes(e),
    }));
  const cell = (c: { row: number; key: string; text: string }) => {
    const e = ep(c.row);
    if (!e) return <Text class="text-body">{c.text}</Text>;
    switch (c.key) {
      case "status":
        return (
          <View class="flex-1 items-center justify-center">
            <Dot tone={episodeTone(e)} label={e.outcome.kind === "ok" ? undefined : e.outcome.kind === "refused" ? t("sessions.refused") : e.outcome.detail} />
          </View>
        );
      case "format":
        // what wrote it, as a tag: a mark in its color and its name, quiet
        return (
          <View class="flex-1 flex-row items-center gap-1">
            <View class={`size-1.5 rounded-full ${family(e).class.replace("text-", "bg-")}`} />
            <Text class="min-w-0 flex-1 text-caption text-muted-foreground line-clamp-1">{c.text}</Text>
          </View>
        );
      case "title":
        // cut in its middle, by the text itself: a name's end (a key's file, an id) is what tells two rows apart
        return (
          <Text class={`text-body line-clamp-1 ${e.outcome.kind === "ok" ? "text-foreground" : TONE_TEXT[episodeTone(e)]}`} style={{ textOverflow: "ellipsis-middle" }}>
            {c.text}
          </Text>
        );
      case "bar": {
        const [a, b] = range();
        const total = b - a;
        const left = Math.max(0, (time(e.start) - a) / total);
        const width = Math.max(0.006, episodeMs(e) / total);
        return (
          <View class="flex-1 flex-row items-center">
            <View class="h-1.5 flex-1 flex-row overflow-hidden rounded-full bg-bar-track">
              <View style={{ flex: left }} />
              <View class={`rounded-full ${TONE_BAR[episodeTone(e)]}`} style={{ flex: width, minWidth: 3 }} />
              <View style={{ flex: Math.max(0, 1 - left - width) }} />
            </View>
          </View>
        );
      }
      case "notes":
        return <Text class="text-small text-muted-foreground line-clamp-1">{c.text}</Text>;
      case "time":
        // from the episode, not the cell's text: DataTable fits that to the column with its own font
        return <Text class="font-mono text-caption text-muted-foreground">{clock(e.start)}</Text>;
      case "requests":
      case "duration":
        // figures line up at their end, under their titles
        return <View class="flex-1 items-end justify-center"><Text class="text-small text-foreground">{c.text}</Text></View>;
      default:
        return <Text class="text-body text-foreground line-clamp-1">{c.text}</Text>;
    }
  };
  return (
    <View class={`min-h-0 flex-1 ${CARD}`} onLayout={({ bounds }) => width.set(Math.round(bounds.size.width))}>
    <DataTable
      ref={(r) => {
        table = r;
        props.ref?.(r);
      }}
      testID="operations"
      accessibilityLabel={t("sessions.operations")}
      class="min-h-0 flex-1"
      columns={columns()}
      // the rows held, of however many there are: the table asks for the rest as it scrolls
      data={data()}
      dataStart={start()}
      rowCount={props.source.total()}
      onVisibleRowsChange={({ range }) => {
        endSeen = range.end;
        props.source.show(range.start, range.end);
      }}
      fontSize={TYPE.body}
      rowHeight={rowHeight(ROW.table)}
      resizableColumns={!terminal()}
      onColumnResize={({ key, width }) => {
        if (!props.compact && key in opsWidths) opsWidths[key as keyof typeof opsWidths].onResize({ size: width });
      }}
      classes={terminal() ? undefined : tableClasses}
      rowClasses={held().map((e) => (props.highlight?.includes(e.index) ? "bg-selection" : ""))}
      renderCell={terminal() ? undefined : cell}
      rowSelectable
      sort={sort()}
      onSortChange={({ sort: next }) => sortBy(next)}
      onRowPress={({ row }) => {
        const e = ep(row);
        if (e) props.onSelect(e.index);
      }}
      onContextMenu={props.onContextMenu ? (e) => { const at = e.row === undefined || e.row === null ? undefined : ep(e.row); if (at) props.onContextMenu?.(at.index); } : undefined}
      empty={<Empty compact text={t("common.empty")} />}
    />
    </View>
  );
}

/** The program's output, one timed line per row. */
export function OutputTable(props: { lines: LogLine[]; onSelect: (i: number) => void; ref?: (r: DataTableRef) => void; highlight?: number[] }) {
  return (
    <DataTable
      ref={props.ref}
      testID="output"
      accessibilityLabel={t("sessions.output")}
      class={`min-h-0 flex-1 ${CARD}`}
      columns={[
        { key: "time", title: t("column.time"), width: COLUMN.outputTime },
        { key: "line", title: t("sessions.output"), width: COLUMN.outputLine, minWidth: COLUMN.titleLeast },
      ]}
      data={props.lines.map((l) => ({ time: clock(l.time), line: l.text }))}
      fontSize={TYPE.small}
      fontFamily={terminal() ? undefined : token("font-mono")}
      rowHeight={rowHeight(ROW.dense)}
      classes={terminal() ? undefined : tableClasses}
      rowClasses={props.lines.map((_, i) => (props.highlight?.includes(i) ? "bg-selection" : ""))}
      rowSelectable
      onRowPress={({ row }) => props.onSelect(row)}
      empty={<Empty compact text={t("common.empty")} />}
    />
  );
}

/** The output lines printed while an operation ran (or the last one before it). */
export function linesOf(lines: LogLine[], e: Pick<EpisodeRow, "start" | "end">): number[] {
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

/** An operation in the inspector: what it is, its notes, its requests; `onRecord` opens one. */
export function OperationView(props: { episode: EpisodeRow; shared: string[]; rows: RecordSummary[]; onRecord: (id: number) => void; onBack?: () => void; back?: JSX.Element }) {
  // one request of the operation: its status (a scenario's answer marked), what it was, how long it took
  const request = (r: () => RecordSummary, i: () => number) => (
    <View style={{ height: rowHeight(ROW.twoLine) }}>
      <ListRow divided first={i() === 0} testID={`op-request-${i()}`} label={`${r().operation} ${r().target}`} onPress={() => props.onRecord(r().id)} class="flex-1 py-0" trailing={<><Text class="text-small tabular-nums text-muted-foreground">{duration(r().latencyMs)}</Text><Icon name="chevron-right" size={ICON.small} /></>}>
        <View class={WIDTH.status}>
          <StatusMark status={r().status} injected={r().injected} />
        </View>
        <View class="min-w-0 flex-1">
          <View class="flex-row items-center gap-2">
            <Text class="min-w-0 shrink text-small text-foreground line-clamp-1">{r().operation}</Text>
            {/* a fault scenario answered it, not the store */}
            <If when={r().injected}>
              <Text testID="op-request-injected" class="shrink-0 text-caption text-status-injected">{t("plots.injected")}</Text>
            </If>
          </View>
          <Path path={r().target} max={40} class="text-muted-foreground" />
        </View>
      </ListRow>
    </View>
  );
  const e = () => props.episode;
  const notes = () => episodeNotes(e(), props.shared);
  const fam = () => family(e());
  return (
    <View class="gap-4" testID="operation-detail">
      <View class="gap-2">
        <View class="flex-row items-center gap-2">
          {props.back}
          <If when={fam().label}>
            <Text class={`text-small ${fam().class}`}>{fam().label}</Text>
          </If>
          <View class="flex-1" />
          <If when={e().outcome.kind !== "ok"}>
            <View class="flex-row items-center gap-2">
              <Dot tone={episodeTone(e())} />
              {/* emphasis: how it failed, in its tone */}
              <Text class={`text-small ${EMPHASIS} ${TONE_TEXT[episodeTone(e())]}`}>{e().outcome.kind === "failed" ? (e().outcome as { detail: string }).detail : t("sessions.refused")}</Text>
            </View>
          </If>
        </View>
        <Title class="leading-5">{episodeTitle(e())}</Title>
        <Text class="tabular-nums text-caption text-muted-foreground">{`${clock(e().start)} · ${duration(episodeMs(e()))} · ${transfer(e().bytesUp, e().bytesDown)}`}</Text>
      </View>
      <If when={notes().length > 0}>
        <View class="gap-2">
          <Each of={notes()}>
            {(n) => (
              <View class="flex-row gap-2">
                <View class="min-h-5 justify-center"><View class="size-1 rounded-full bg-muted-foreground" /></View>
                <Text class="flex-1 text-body leading-5 text-foreground">{n()}</Text>
              </View>
            )}
          </Each>
        </View>
      </If>
      {/* its requests in time, in the one chart frame: a bar from each one's start to its end */}
      <If when={!terminal() && props.rows.length > 1}>
        <Chart title={t("waterfall.title")} text={t("profile.waterfallText", { n: props.rows.length, time: duration(Math.max(1, ...props.rows.map((r) => new Date(r.time).getTime() + r.latencyMs)) - Math.min(...props.rows.map((r) => new Date(r.time).getTime()))) })}>
          <Waterfall records={props.rows} onRecord={props.onRecord} />
        </Chart>
      </If>
      <View class="gap-2">
        <Heading count={e().requests}>{t("timeline.requestsOf")}</Heading>
        {/* a few are drawn whole; an operation may hold thousands of requests, and then only those in view are */}
        <If
          when={props.rows.length > 40}
          else={
            <Card pad="none" testID="op-requests">
              <Each of={props.rows} key={(r) => `${r.id}`}>{(r, i) => request(r, i)}</Each>
            </Card>
          }
        >
          <List
            testID="op-requests"
            each={props.rows}
            keyExtractor={(r) => `${r.id}`}
            estimatedItemSize={rowHeight(ROW.twoLine) ?? 32}
            class={CARD}
            style={{ height: 10 * (rowHeight(ROW.twoLine) ?? 32) + 2 }}
          >
            {(r, i) => request(r, i)}
          </List>
        </If>
      </View>
    </View>
  );
}
