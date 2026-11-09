// A session on screen, with no state of the app's own: its header and
// figures, what stands out (each a filter of the operations), the
// operations with their outcome and place in time, the program's output,
// and how output and operations line up. The app and the web viewer
// (sonde share) both draw with these.

import { type ContextMenuEvent, effect, type JSX, List, platform, reduceMotion, Scroll, type ScrollRef, signal, Text, untracked, View } from "nsh";
import { Accordion, AccordionHeader, AccordionItem, AccordionPanel, AccordionTrigger, Button as BaseButton, DataTable, type DataTableRef, type DataTableSort, Popover, TextView } from "nsh/base";
import { isAvailable } from "nsh/desktop";
import { bytes, count, date, day, middle, ms } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { report } from "../store";
import { episodeNotes, episodeTitle, observationSaving, observationText, observationWhy } from "../phrases";
import { cost, groupName, offset, sessionStory, SessionShape, Sparkline, Waterfall } from "./plots";
import type { Episode, LogLine, Observation, Profile, ProfileGroup, RecordSummary, SessionInfo, SessionView } from "../types";
import { Icon } from "./icon";
import { CARD, Dot, Empty, FOCUS, InWindowMenu, ListRow, Options, POPUP, rowHeight, TABLE, terminal, TYPE } from "./kit";
import { rememberedSize } from "./sizes";

const SHELL = new Set(["for", "while", "until", "if", "case", "sh", "bash", "zsh", "sudo", "(", "{"]);
// steps that set the stage for the program rather than being it
const STAGE = new Set(["cd", "sleep", "echo", "export", "set", "source", ".", "true", "time", "nice", "exec", "printf", "wait", "nohup"]);

const ASSIGNMENT = /^[A-Za-z_][A-Za-z0-9_]*=/;

/** A command's shell words as typed, quotes kept (`TOKEN="ab cd"` is one
 * word), and `;`, `&&`, `||`, `|`, `&` as words of their own: as sonde's
 * own reading of a command (program.rs) has them. */
function shellWords(command: string): string[] {
  const words: string[] = [];
  let word = "";
  let quote: string | null = null;
  const flush = () => {
    if (word) words.push(word);
    word = "";
  };
  for (let i = 0; i < command.length; i++) {
    const c = command[i];
    if (quote) {
      word += c;
      if (c === "\\" && quote === '"' && i + 1 < command.length) word += command[++i];
      else if (c === quote) quote = null;
    } else if (c === "'" || c === '"') {
      quote = c;
      word += c;
    } else if (c === "\\") {
      word += c + (command[++i] ?? "");
    } else if (c === ";" || c === "|" || c === "&") {
      flush();
      words.push(c !== ";" && command[i + 1] === c ? c + command[++i] : c);
    } else if (/\s/.test(c)) flush();
    else word += c;
  }
  flush();
  return words;
}

const SEPARATOR = new Set([";", "&&", "||", "|", "&"]);

/** The command with the value of every variable it sets hidden
 * (`AWS_SECRET_ACCESS_KEY=…`), quoted values whole: what a list, a tooltip
 * or a notification shows. */
export function scrubCommand(command: string): string {
  return shellWords(command)
    .map((w) => (ASSIGNMENT.test(w) ? `${w.slice(0, w.indexOf("="))}=…` : w))
    .join(" ");
}

/** What a command runs, for lists: its program's name (past the variables
 * it sets, `env` and its options, never one of their values; of a compound
 * command the first that is not a step setting the stage), or the command
 * itself, scrubbed, when it starts with the shell's own words. */
export function programOf(command: string | null | undefined): string | null {
  if (!command) return null;
  const words = shellWords(command);
  let first: string | null = null;
  let part: string[] = [];
  const parts: string[][] = [];
  for (const w of words) {
    if (SEPARATOR.has(w)) {
      parts.push(part);
      part = [];
    } else part.push(w);
  }
  parts.push(part);
  for (const p of parts) {
    let i = 0;
    while (i < p.length) {
      if (ASSIGNMENT.test(p[i])) i++;
      else if (p[i] === "env") {
        i++;
        while (i < p.length && (ASSIGNMENT.test(p[i]) || p[i].startsWith("-"))) i += ["-u", "-C", "--unset", "--chdir"].includes(p[i]) ? 2 : 1;
      } else break;
    }
    const w = p[i];
    if (!w) continue;
    if (SHELL.has(w)) {
      const c = scrubCommand(command);
      return c.length > 40 ? `${c.slice(0, 40)}…` : c;
    }
    const name = w.replace(/^['"]|['"]$/g, "").split(/[\\/]/).pop() || null;
    if (name && !STAGE.has(name)) return name;
    first ??= name;
  }
  return first;
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
export const toneText = (t: Tone) => TONE_TEXT[t];

/** An HTTP status, the same everywhere: a dot in its tone and the code, a
 * success quiet, anything else in its tone (none: the request never got
 * an answer). */
export function StatusMark(props: { status: number | null; text?: string }) {
  const tone = () => statusTone(props.status);
  return (
    <View class="flex-row items-center gap-1.5">
      <Dot tone={tone()} />
      <Text class={`font-mono text-caption tabular-nums ${tone() === "ok" ? "text-muted-foreground" : `font-semibold ${TONE_TEXT[tone()]}`}`}>{props.text ?? (props.status === null ? "—" : String(props.status))}</Text>
    </View>
  );
}

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

/** A figure: a number, and what it counts. With `preview`, a press (or
 * Return) opens what it is made of, and Escape closes it (Native Shell's
 * Popover): the keyboard, touch and screen readers reach it as the pointer
 * does. */
export function Figure(props: { value: string; label: string; tone?: Tone; preview?: JSX.Element; testID?: string }) {
  const body = (
    <View class="gap-0.5">
      <Text class={`tabular-nums text-heading font-semibold ${props.tone ? TONE_TEXT[props.tone] : "text-foreground"}`}>{props.value}</Text>
      <Text class="text-caption text-muted-foreground">{props.label}</Text>
    </View>
  );
  if (!props.preview || terminal()) return <View testID={props.testID} class="px-2 py-1">{body}</View>;
  return (
    <Popover
      placement="bottom"
      align="start"
      offset={6}
      trigger={
        <BaseButton testID={props.testID} accessibilityLabel={`${props.value} ${props.label}`} class={`rounded-(--radius-sm) px-2 py-1 hover:bg-muted active:bg-muted ${FOCUS}`}>
          {body}
        </BaseButton>
      }
      popupClass={`w-[300px] gap-2 ${POPUP} p-3`}
      popupStartingClass="opacity-0 -translate-y-1"
      popupEndingClass="opacity-0"
    >
      {props.preview}
    </Popover>
  );
}

/** What a session's requests were, by kind: a figure's preview. */
function KindsPreview(props: { profile: Profile; by: "requests" | "time" }) {
  const rows = () => [...props.profile.groups].sort((a, b) => (props.by === "requests" ? b.requests - a.requests : b.busyMs - a.busyMs)).slice(0, 5);
  // kinds overlap in time: each bar against the largest, not a share of a whole
  const total = () => rows().reduce((m, g) => Math.max(m, props.by === "requests" ? g.requests : g.busyMs), 0) || 1;
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

/** The commit a run's code was at, as a person reads it: a1b2c3d (main), changed. */
export function gitText(g: SessionView["header"]["git"]): string {
  if (!g) return "";
  return `${g.commit.slice(0, 7)}${g.branch ? ` (${g.branch})` : ""}${g.dirty ? ` · ${t("git.dirty")}` : ""}`;
}

/** The command and where it ran, then the session's figures. */
export function SessionHeader(props: { view: SessionView; actions?: JSX.Element; compact?: boolean; slice?: number | null; onSlice?: (i: number | null) => void; shape?: boolean }) {
  const s = () => props.view.summary;
  const line = () => {
    const parts = [t("timeline.operations", { n: s().requests, ops: t("figure.operationsN", { n: props.view.episodes.length }), secs: seconds(s().seconds) })];
    if (s().errors) parts.push(t("timeline.errors", { n: s().errors }));
    if (s().misses) parts.push(t("timeline.notFound", { n: s().misses }));
    parts.push(`↑${bytes(s().bytesUp)} ↓${bytes(s().bytesDown)}`);
    return parts.join(" · ");
  };
  const command = () => props.view.header.command;
  if (terminal()) {
    return (
      <View class="gap-0">
        <Text class="font-semibold text-foreground">{command() ? `$ ${scrubCommand(command()!)}` : props.view.header.endpoint}</Text>
        <Text class="text-muted-foreground" testID="session-summary">
          {line()}
        </Text>
        <Text class="text-foreground" testID="session-story">{sessionStory(props.view.profile, props.view.observations)}</Text>
      </View>
    );
  }
  return (
    <View class="gap-3">
      <View class="flex-row items-start gap-3">
        <View selectable class="min-w-0 flex-1 gap-1">
          <Text class="font-mono text-body font-semibold text-foreground line-clamp-2">
            {command() ? `$ ${scrubCommand(command()!)}` : props.view.header.endpoint}
          </Text>
          <Text class="text-small text-muted-foreground line-clamp-1">{[props.view.header.endpoint, date(props.view.header.created), gitText(props.view.header.git)].filter(Boolean).join(" · ")}</Text>
          {/* the session in a sentence: where its time went, and the first thing to do */}
          <If when={props.view.profile?.groups.length && props.shape !== false && !props.compact}>
            <Text testID="session-story" class="pt-1 text-body leading-5 text-foreground line-clamp-2">{sessionStory(props.view.profile, props.view.observations)}</Text>
          </If>
        </View>
        {props.actions}
      </View>
      <View class={props.compact ? "gap-2" : "flex-row items-center gap-5"}>
      <View testID="session-summary" accessibilityLabel={line()} class="-ml-2 flex-row flex-wrap">
          <Figure testID="figure-requests" value={count(s().requests)} label={t("figure.requests")} preview={props.view.profile?.groups.length ? <KindsPreview profile={props.view.profile} by="requests" /> : undefined} />
          <Figure value={count(props.view.episodes.length)} label={t("figure.operations")} />
          <Figure value={offset(s().seconds * 1000)} label={t("figure.took")} />
          {/* the program's time on S3: one figure, with its share, and what it was made of */}
          <If when={props.view.profile?.groups.length}>
            <Figure testID="profile-waiting" value={offset(props.view.profile.busyMs)} label={`${t("profile.waiting")} · ${Math.round((props.view.profile.busyMs * 100) / Math.max(props.view.profile.wallMs, 1e-9))}%`} preview={<KindsPreview profile={props.view.profile} by="time" />} />
          </If>
          <If when={props.view.profile && props.view.profile.cost.amount > 0}>
            <Figure testID="profile-cost" value={cost(props.view.profile.cost.amount, props.view.profile.cost.currency)} label={t("profile.cost")} />
          </If>
          <If when={s().errors > 0}>
            <Figure value={count(s().errors)} label={t("figure.errors")} tone="server" />
          </If>
          <If when={s().misses > 0}>
            <Figure value={count(s().misses)} label={t("figure.notFound")} />
          </If>
        </View>
        {/* the session's shape: requests by outcome over time, and in flight */}
        <If when={props.view.profile && props.shape !== false}>
          <SessionShape profile={props.view.profile} slice={props.slice} onSlice={props.onSlice} annotate={!props.compact} marks={findingMarks(props.view)} class={props.compact ? "h-10 w-full" : "mt-3 h-12 min-w-[200px] flex-1"} />
        </If>
      </View>
    </View>
  );
}

/** Whether a session's findings are folded away (kept while the app runs). */
const findingsOpen = signal(true);
/** The findings a line each (the session shows a pane of its own below them). */
export const findingsDense = signal(false);

const SDKS = { python: "Python · boto3", java: "Java SDK v2", go: "Go SDK v2", js: "JavaScript v3", config: "Bucket" } as const;
const LANG = { python: "python", java: "java", go: "go", js: "javascript", config: "xml" } as const;
/** The SDK the fixes are shown for, kept while the app runs. */
const fixSdk = signal<keyof typeof SDKS>("python");

/** What to change for a finding: its fix in each SDK, one shown. */
/** The fix's SDK for the program's own: its user agent named it. */
const sdkOfAgent = (sdk: string | undefined): keyof typeof SDKS | null => {
  if (!sdk) return null;
  const s = sdk.toLowerCase();
  return s.startsWith("boto") || s.startsWith("aws-cli") || s.startsWith("s3fs") ? "python" : s.includes("java") || s.includes("hadoop") ? "java" : s.includes("go") ? "go" : s.includes("-js") ? "js" : null;
};

function FixPanel(props: { o: Observation; path?: string }) {
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
        <Icon name="sparkles" size={13} />
        <Text role="header" class="flex-1 text-small font-semibold text-foreground">{t("fix.title")}</Text>
        <Options testID="fix-sdk" value={shown()?.sdk ?? ""} items={fixes().map((f) => ({ value: f.sdk, title: SDKS[f.sdk] }))} onChange={(v) => fixSdk.set(v as keyof typeof SDKS)} />
      </View>
      {/* why it matters and what it saves, written out */}
      <If when={observationWhy(props.o)}>
        {(why) => (
          <TextView
            testID="fix-why"
            markdown={`${why()}${observationSaving(props.o) ? `\n\n**${observationSaving(props.o)}**` : ""}`}
            classes={{ paragraph: "text-small leading-snug text-muted-foreground", strong: "font-semibold text-info" }}
          />
        )}
      </If>
      {/* where in the program, when it said: the place to change */}
      <If when={props.o.callers?.length}>
        <View testID="fix-callers" class="gap-0.5">
          <Each of={props.o.callers!} key={(c) => c.place}>
            {(c) => (
              <ListRow
                label={t("fix.openCode", { place: c().place })}
                onPress={() => props.path && api.sessions.openCode(props.path, c().place).catch(report)}
                class="py-1"
              >
                <Icon name="file" size={12} />
                <Text class="min-w-0 flex-1 font-mono text-small text-primary line-clamp-1">{c().place}</Text>
                <Text class="tabular-nums text-caption text-muted-foreground">{t("run.bandRequests", { n: c().requests })}</Text>
              </ListRow>
            )}
          </Each>
        </View>
      </If>
      <If when={shown()}>
        {(f) => (
          <If when={!terminal()} else={<Text class="font-mono text-foreground">{f().code}</Text>}>
            {/* highlighted, with a copy button over it (Native Shell's TextView) */}
            <TextView
              testID="fix-code"
              markdown={`\`\`\`${LANG[f().sdk]}\n${f().code}\n\`\`\``}
              codeBlockActions={[{ id: "copy", label: t("fix.copy") }]}
              classes={{ codeBlock: "rounded-(--radius) border border-border bg-background p-2.5 font-mono text-small" }}
            />
          </If>
        )}
      </If>
    </View>
  );
}

/** Where each finding begins, by its number in the list: for the shape. */
export function findingMarks(v: SessionView): { tMs: number; n: number }[] {
  const t0 = v.episodes.length ? Date.parse(v.episodes[0].start) : 0;
  return v.observations.flatMap((o, i) => {
    const e = o.episodes?.length ? v.episodes[o.episodes[0]] : undefined;
    return e ? [{ tMs: Math.max(0, Date.parse(e.start) - t0), n: i + 1 }] : [];
  });
}

/** Acting on every finding at once, as an estimate: requests, waiting, cost. */
export function whatIfText(v: SessionView): string | null {
  const w = v.whatIf;
  if (!w) return null;
  // the cost as what changes: amounts of thousandths of a cent read as nothing
  const cut = w.cost[0] > 0 ? `${w.cost[1] <= w.cost[0] ? "−" : "+"}${Math.round((Math.abs(w.cost[0] - w.cost[1]) * 100) / w.cost[0])}%` : "—";
  return t("whatIf.text", { n: w.findings, a: w.requests[0], b: w.requests[1], wa: offset(w.waitingMs[0]), wb: offset(w.waitingMs[1]), cut });
}

/** What stands out in a session — its core result. Each finding says what
 * happened and why it matters, and narrows the operations to those it is
 * about. `compact` (beside a run's live operations): the sentences only. */
export function Observations(props: { view: SessionView; filter?: number | null; onFilter?: (i: number | null) => void; compact?: boolean; max?: number; maxHeight?: number }) {
  const press = (i: number) => props.onFilter?.(props.filter === i ? null : i);
  // a finding pressed brings its fixes into view, under the list
  let scroller: ScrollRef | undefined;
  effect(() => {
    const f = props.filter;
    if (f == null || !props.view.observations[f]?.fixes?.length) return;
    setTimeout(() => scroller?.scrollTo({ y: 1e6, animated: true }), 60);
  });
  const list = () => props.view.observations.slice(0, props.max ?? props.view.observations.length);
  if (terminal()) {
    return (
      <If when={props.view.observations.length > 0}>
        <View testID="observations">
          <Each of={list()}>
            {(o, i) => (
              // a finding narrows the operations to its own, as on the desktop
              <BaseButton
                testID={`observation-${i()}`}
                accessibilityLabel={observationText(o())}
                selected={props.filter === i()}
                disabled={!props.onFilter || !o().episodes?.length}
                onPress={() => press(i())}
                class="focus:bg-row-hover selected:bg-selection"
              >
                <Text class={o().level === "warn" ? "text-warn" : "text-muted-foreground"}>
                  {o().level === "warn" ? "! " : "· "}
                  {observationText(o())}
                  <If when={!props.compact && observationSaving(o())}>{(s) => <Text class="text-status-ok">{` — ${s()}`}</Text>}</If>
                </Text>
              </BaseButton>
            )}
          </Each>
        </View>
      </If>
    );
  }
  const dense = () => props.compact || findingsDense();
  const row = (o: () => Observation, i: () => number) => (
    <BaseButton
      testID={`observation-${i()}`}
      accessibilityLabel={observationText(o())}
      selected={props.filter === i()}
      disabled={!props.onFilter || !o().episodes?.length}
      onPress={() => press(i())}
      class={`group flex-row items-start gap-2 px-3 ${dense() ? "py-1.5" : "py-2"} hover:bg-row-hover selected:bg-selection disabled:hover:bg-transparent ${i() > 0 ? "border-t border-border" : ""}`}
    >
      {/* its number: where it stands on the session's shape */}
      <Text class={`h-5 w-3 text-right tabular-nums text-caption font-semibold ${o().level === "warn" ? "text-warn" : "text-muted-foreground"}`}>{String(i() + 1)}</Text>
      <View class="h-5 w-4 items-center justify-center">
        <Icon name={o().level === "warn" ? "triangle-alert" : "info"} size={14} tint={o().level === "warn" ? "warn" : "muted-foreground"} />
      </View>
      <View class="min-w-0 flex-1 gap-0.5">
        <Text class={`text-body text-foreground ${dense() ? "line-clamp-1" : "line-clamp-2"}`}>{observationText(o())}</Text>
        <If when={!dense() && observationWhy(o())}>{(w) => <Text class="text-small leading-snug text-muted-foreground line-clamp-2">{w()}</Text>}</If>
        <If when={!dense() && observationSaving(o())}>
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
      {/* the findings open and close under their heading (Native Shell's Accordion) */}
      <Accordion value={props.compact || findingsOpen() ? ["findings"] : []} onChange={(e) => findingsOpen.set(e.value.includes("findings"))}>
        <AccordionItem value="findings" testID="observations" class="gap-2">
          <If when={!props.compact}>
            <AccordionHeader level={3} class="flex-row flex-wrap items-center gap-x-3 gap-y-1">
              <AccordionTrigger testID="observations-toggle" class={`group flex-row items-center gap-1.5 rounded-(--radius-sm) py-0.5 pr-1 ${FOCUS}`}>
                <View class="group-aria-expanded:rotate-90 motion-safe:transition-transform motion-safe:duration-150">
                  <Icon name="chevron-right" size={13} tint="muted-foreground" />
                </View>
                <Text class="text-body font-semibold text-foreground">{t("obs.title")}</Text>
                <Text class="text-body text-muted-foreground">{`· ${props.view.observations.length}`}</Text>
              </AccordionTrigger>
              {/* what acting on them saves: the section's subtitle, not a third voice */}
              <If when={whatIfText(props.view)}>{(w) => <Text testID="what-if" class="min-w-0 flex-1 tabular-nums text-small text-muted-foreground line-clamp-1">{w()}</Text>}</If>
            </AccordionHeader>
          </If>
          <AccordionPanel reveal accessibilityLabel={t("obs.title")}>
            {/* a short window keeps room for the operations: the findings scroll */}
            <Scroll ref={(r) => (scroller = r)} class={`overflow-hidden ${CARD}`} style={props.maxHeight ? { maxHeight: props.maxHeight } : undefined} scrollbar={false}>
              <Each of={list()}>{(o: () => Observation, i) => row(o, i)}</Each>
              {/* the finding pressed: what to change, in the findings' own room */}
              <If when={!props.compact && props.filter != null && props.view.observations[props.filter!]?.fixes?.length ? props.view.observations[props.filter!] : null}>
                {(o) => (
                  <View class="border-t border-border p-3">
                    <FixPanel o={o()} path={props.view.path} />
                  </View>
                )}
              </If>
            </Scroll>
          </AccordionPanel>
        </AccordionItem>
      </Accordion>
    </If>
  );
}

const tableClasses = {
  ...TABLE,
  rowActive: "bg-row-hover",
  resizeHandle: "bg-transparent group-hover:bg-border",
};

/** A session's operations: outcome, time, family, title, requests, duration,
 * place in the session's time and notes. `rows` (default all) are the
 * episodes shown, by index; events name episodes by index too. */
/** The operations' columns as the user last sized them (kept with the app's sizes). */
const opsWidths = { title: rememberedSize("ops.title", 340), bar: rememberedSize("ops.bar", 150), notes: rememberedSize("ops.notes", 320) };
const kept = (key: keyof typeof opsWidths, fallback: number) => untracked(() => opsWidths[key].size()) || fallback;

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
    { key: "time", title: t("column.time"), width: 110, sortable: true },
    ...(formats() ? [{ key: "format", title: "", width: 86 }] : []),
    { key: "title", title: t("column.operation"), width: props.compact ? 220 : kept("title", 340), minWidth: 160 },
    { key: "requests", title: t("column.requests"), width: 56, align: "end" as const, sortable: true },
    { key: "duration", title: t("sessions.took"), width: 78, align: "end" as const, sortable: true },
    ...(terminal() || props.compact ? [] : [{ key: "bar", title: t("column.when"), width: kept("bar", 150), minWidth: 80 }]),
    ...(props.compact ? [] : [{ key: "notes", title: t("column.notes"), width: kept("notes", 320), minWidth: 120 }]),
  ];
  const data = () =>
    shown().map((i) => {
      const e = props.view.episodes[i];
      return {
        status: episodeTone(e),
        time: clock(e.start),
        format: family(e).label,
        // cut in the middle: the end of a key is what tells two apart
        title: middle(terminal() && e.outcome.kind !== "ok" ? `${episodeTitle(e)} ✗` : episodeTitle(e), props.compact ? 40 : 72),
        requests: String(e.records.length),
        duration: ms(duration(e)),
        bar: "",
        notes: notes(e),
      };
    });
  const cell = (c: { row: number; key: string; text: string }) => {
    const e = ep(c.row);
    if (!e) return <Text class="text-body">{c.text}</Text>;
    switch (c.key) {
      case "status":
        return (
          <View class="flex-1 items-center justify-center">
            <Dot tone={episodeTone(e)} label={e.outcome.kind === "ok" ? undefined : e.outcome.kind === "refused" ? "refused" : e.outcome.detail} />
          </View>
        );
      case "format":
        // what wrote it, as a tag: a mark in its color and its name, quiet
        return (
          <View class="flex-1 flex-row items-center gap-1.5">
            <View class={`size-1.5 rounded-full ${family(e).class.replace("text-", "bg-")}`} />
            <Text class="min-w-0 flex-1 text-caption text-muted-foreground line-clamp-1">{c.text}</Text>
          </View>
        );
      case "title":
        return <Text class={`text-body line-clamp-1 ${e.outcome.kind === "ok" ? "text-foreground" : TONE_TEXT[episodeTone(e)]}`}>{c.text}</Text>;
      case "bar": {
        const [a, b] = range();
        const total = b - a;
        const left = Math.max(0, (time(e.start) - a) / total);
        const width = Math.max(0.006, duration(e) / total);
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
        return <Text class="font-mono text-caption tabular-nums text-muted-foreground">{clock(e.start)}</Text>;
      case "requests":
      case "duration":
        // figures line up at their end, under their titles
        return <Text class="flex-1 text-right text-small tabular-nums text-foreground">{c.text}</Text>;
      default:
        return <Text class="text-body text-foreground line-clamp-1">{c.text}</Text>;
    }
  };
  return (
    <DataTable
      ref={props.ref}
      testID="operations"
      accessibilityLabel={t("sessions.operations")}
      class={`min-h-0 flex-1 ${CARD}`}
      columns={columns()}
      data={data()}
      fontSize={TYPE.body}
      rowHeight={rowHeight(30)}
      resizableColumns={!terminal()}
      onColumnResize={({ key, width }) => {
        if (!props.compact && key in opsWidths) opsWidths[key as keyof typeof opsWidths].onResize({ size: width });
      }}
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
      class={`min-h-0 flex-1 ${CARD}`}
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
        {/* an operation may hold thousands of requests: only those in view are drawn */}
        <List
          testID="op-requests"
          each={props.rows}
          keyExtractor={(r) => String(r.id)}
          estimatedItemSize={rowHeight(44) ?? 32}
          class={CARD}
          style={{ height: Math.min(props.rows.length, 10) * (rowHeight(44) ?? 32) + 2 }}
        >
            {(r, i) => (
              <View style={{ height: rowHeight(44) }} class={i() > 0 ? "border-t border-border" : ""}>
                <ListRow testID={`op-request-${i()}`} label={`${r().operation} ${r().target}`} onPress={() => props.onRecord(r().id)} class="flex-1 rounded-none px-3">
                  <View class="w-12">
                    <StatusMark status={r().status} />
                  </View>
                  <View class="min-w-0 flex-1">
                    <Text class="text-small text-foreground line-clamp-1">{r().operation}</Text>
                    <Text class="font-mono text-caption text-muted-foreground line-clamp-1">{middle(r().target, 40)}</Text>
                  </View>
                  <Text class="text-small tabular-nums text-muted-foreground">{ms(r().latencyMs)}</Text>
                  <Icon name="chevron-right" size={13} />
                </ListRow>
              </View>
            )}
        </List>
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
export function SessionList(props: {
  sessions: SessionInfo[];
  selected: string | null;
  onSelect: (p: string) => void;
  onContextMenu?: (s: SessionInfo, at?: { x: number; y: number }) => void;
  /** The context menu's commands, offered to screen readers on each row. */
  actions?: (s: SessionInfo) => { name: string; label: string; run: () => void }[];
}) {
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
  // the rest of the command after its program, scrubbed; the whole of it when
  // the program is not its first word (`cd etl && python load.py`)
  const args = (s: SessionInfo) => {
    const p = programOf(s.command);
    if (s.title || !s.command || !p || p.endsWith("…")) return "";
    const c = scrubCommand(s.command);
    const [first, ...rest] = c.split(" ");
    if (first.replace(/^['"]|['"]$/g, "").split(/[\\/]/).pop() === p) return rest.join(" ");
    return c === p ? "" : c;
  };
  // where the system has no menus to pop up (a terminal, the web): the
  // same commands in Native Shell's own, on a right press or Shift+F10
  const inWindowMenu = (x: () => { s: SessionInfo; i: number }, body: JSX.Element) => (
    <InWindowMenu enabled={!props.onContextMenu && !!props.actions && !isAvailable} actions={() => (props.actions?.(x().s) ?? []).filter((a) => a.name !== "window")}>
      {body}
    </InWindowMenu>
  );
  const row = (x: () => { s: SessionInfo; i: number }) => inWindowMenu(x,
    <View
      onContextMenu={props.onContextMenu ? (e: ContextMenuEvent) => props.onContextMenu!(x().s, e.pagePosition) : undefined}
      accessibilityActions={props.actions?.(x().s).map(({ name, label }) => ({ name, label }))}
      onAccessibilityAction={props.actions ? ({ actionName }) => props.actions!(x().s).find((a) => a.name === actionName)?.run() : undefined}
    >
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
      <View class="flex-row items-center gap-2">
        <Text class="min-w-0 flex-1 text-caption text-muted-foreground line-clamp-1">{`${clock(x().s.created).slice(0, 5)} · ${x().s.endpoint} · ${t("sessions.requestsN", { n: x().s.requests })}`}</Text>
        <If when={x().s.shape?.length}>
          <Sparkline shape={x().s.shape!} id={x().s.path} />
        </If>
      </View>
    </BaseButton>
    </View>,
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
