// A session on screen, with no state of the app's own: its header and
// figures, what stands out (each a filter of the operations), the
// operations with their outcome and place in time, the program's output,
// and how output and operations line up. The app and the web viewer
// (sonde share) both draw with these.

import { type ContextMenuEvent, effect, type JSX, List, Scroll, signal, Text, token, untracked, View, type ViewRef } from "nsh";
import { DataTable, type DataTableRef, type DataTableSort, TextView } from "nsh/base";
import { isAvailable } from "nsh/desktop";
import { change, clock, count, date, day, duration, hm, middle, transfer } from "../format";
import { t } from "../i18n";
import { episodeNotes, episodeTitle, observationSaving, observationText, observationWhy } from "../phrases";
import { Chart, sessionStory, Sparkline, timed, Waterfall, MARK } from "./plots";
import { type SignalId, signalsOf, signalTiles } from "../signals";
import { worstStanding } from "../trials";
import type { Episode, LogLine, Observation, RecordSummary, SessionInfo, SessionView } from "../types";
import { Icon } from "./icon";
import { Badge, Button, Card, CARD, Disclosure, Dot, EMPHASIS, Empty, fitColumns, Heading, ICON, InWindowMenu, ListRow, Options, Path, ROW, rowHeight, Stat, TABLE, terminal, type TileLook, Tiles, Title, TYPE, SIZES, WIDTH } from "./kit";
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
    } else if (c === "\\" && /[\s\\'";|&$`]/.test(command[i + 1] ?? "")) {
      // an escape only before what it escapes: C:\Python\x.exe is a path
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

/** A word with any secret in it hidden: a variable's value, an option that
 * names a secret (`--password=…`), and both inside a quoted command given
 * to a shell (`sh -c 'KEY=… python x.py'`). */
function scrubWord(word: string): string {
  const eq = word.indexOf("=");
  if (eq > 0) {
    const name = word.slice(0, eq);
    if (ASSIGNMENT.test(word)) return `${name}=…`;
    if (name.startsWith("-") && /secret|password|passwd|token|key|credential/i.test(name)) return `${name}=…`;
  }
  for (const q of ["'", '"']) {
    if (word.length >= 2 && word.startsWith(q) && word.endsWith(q) && word.includes("=")) return `${q}${scrubCommand(word.slice(1, -1))}${q}`;
  }
  return word;
}

/** sonde's own words where a command would be: a replay, a watch of logs, an import. */
const OWN_WORDS = ["replay of ", "logs of ", "imported from "];

/** A command the user ran — not sonde's own `replay of …`, `logs of …` or `imported from …`. */
export function ranCommand(command?: string | null): boolean {
  const c = command?.trim();
  return !!c && !OWN_WORDS.some((w) => c.startsWith(w));
}

/** The session's title line: `$ command` for a run, sonde's own words otherwise. */
export function commandLine(command: string): string {
  return ranCommand(command) ? `$ ${scrubCommand(command)}` : command;
}

/** The command with the value of every variable it sets hidden
 * (`AWS_SECRET_ACCESS_KEY=…`), quoted values whole, the rest as typed: what
 * a list, a tooltip or a notification shows. */
export function scrubCommand(command: string): string {
  let out = "";
  let i = 0;
  const text = command.trim();
  while (i < text.length) {
    // a word starts here: an assignment's value runs to the word's end, quotes included
    if (/\s/.test(text[i]) || ";|&".includes(text[i])) {
      out += text[i++];
      continue;
    }
    let j = i;
    let quote: string | null = null;
    while (j < text.length) {
      const c = text[j];
      if (quote) {
        if (c === "\\" && quote === '"') j++;
        else if (c === quote) quote = null;
      } else if (c === "'" || c === '"') quote = c;
      else if (c === "\\") j++;
      else if (/\s/.test(c) || ";|&".includes(c)) break;
      j++;
    }
    out += scrubWord(text.slice(i, j));
    i = j;
  }
  return out;
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

export { clock };

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
const kindLabel = (k: Episode["kind"]): string => (k === "multipart" ? t("kind.multipart") : k === "retry" ? t("kind.retry") : k === "listing" ? t("kind.listing") : k === "probe" ? t("kind.probe") : "");

/** What kind of thing an operation is: its storage format, else its kind. */
function family(e: Episode): { label: string; class: string } {
  if (e.format) return { label: e.format, class: FORMAT_TEXT[e.format] ?? "text-fmt-other" };
  const k = kindLabel(e.kind);
  return k ? { label: k, class: e.kind === "multipart" ? "text-fmt-mpu" : "text-fmt-other" } : { label: "", class: "" };
}

/** How long an operation ran, in ms (a number: `duration` writes it). */
export const episodeMs = (e: Episode) => time(e.end) - time(e.start);

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

/** The commit a run's code was at, as a person reads it: a1b2c3d (main), changed. */
export function gitText(g: SessionView["header"]["git"]): string {
  if (!g) return "";
  return `${g.commit.slice(0, 7)}${g.branch ? ` (${g.branch})` : ""}${g.dirty ? ` · ${t("git.dirty")}` : ""}`;
}

/** The command and where it ran, then the session's figures. */
export function SessionHeader(props: { view: SessionView; actions?: JSX.Element; /** a press on a signal: its chart, on the Time pane */ onSignal?: (id: SignalId) => void; /** what marks the session (a trial of another run), under its title */ children?: JSX.Element; /** the pane is under `BREAK.short`: the signals keep to a line */ short?: boolean }) {
  const s = () => props.view.summary;
  const line = () => {
    const parts = [t("timeline.operations", { n: s().requests, ops: t("figure.operationsN", { n: props.view.episodes.length }), secs: duration(s().seconds * 1000) })];
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
        <Text class="text-foreground" testID="session-story">{sessionStory(props.view.profile, props.view.observations)}</Text>
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
          mark={<Sparkline shape={x().series} id={`signal:${props.view.path}:${SIGNALS[i]}`} still class={MARK.tile} kind={x().kind} tone={x().tone === "bad" ? "bad" : "plain"} />}
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
          <Text class="text-small text-muted-foreground line-clamp-1">{[props.view.header.endpoint, date(props.view.header.created), gitText(props.view.header.git)].filter(Boolean).join(" · ")}</Text>
          {/* the session in a sentence: where its time went, and the first thing to do. It wraps
              to two lines as the pane narrows; what is past them is cut at the second line's end */}
          <If when={profile()?.groups.length}>
            <Text testID="session-story" class="pt-1 text-body leading-5 text-foreground line-clamp-2">{sessionStory(profile(), props.view.observations)}</Text>
          </If>
        </View>
        {props.actions}
      </View>
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

function FixPanel(props: { o: Observation; /** the SDK the fixes are shown for: the pane's own, kept from finding to finding */ sdk: { (): Sdk; set: (s: Sdk) => void }; onOpenCode?: (place: string) => void; onExpect?: (o: Observation) => void }) {
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
        {/* or no change: a habit kept on purpose, which sonde check then expects */}
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
            {/* highlighted, with a copy button over it (Native Shell's TextView) */}
            <TextView
              testID="fix-code"
              // (the sample as written, without the blank lines around it)
              markdown={`\`\`\`${LANG[f().sdk]}\n${f().code.replace(/^\s*\n|\s+$/g, "")}\n\`\`\``}
              codeBlockActions={[{ id: "copy", label: t("fix.copy") }]}
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
  const cut = w.cost[0] > 0 ? change(w.cost[0], w.cost[1]) : "—";
  // a log without times says nothing of waiting
  if (!timed(v.profile)) return t("whatIf.untimed", { n: w.findings, a: w.requests[0], b: w.requests[1], cut });
  return t("whatIf.text", { n: w.findings, a: w.requests[0], b: w.requests[1], wa: duration(w.waitingMs[0]), wb: duration(w.waitingMs[1]), cut });
}

/** What stands out in a session — its core result. Each finding says what
 * happened and why it matters, and narrows the operations to those it is
 * about. `compact` (beside a run's live operations): the sentences only. */
export function Observations(props: { view: SessionView; filter?: number | null; onFilter?: (i: number | null) => void; compact?: boolean; dense?: boolean; narrow?: boolean; max?: number; maxHeight?: number; onOpenCode?: (place: string) => void; onExpect?: (o: Observation) => void }) {
  const press = (i: number) => props.onFilter?.(props.filter === i ? null : i);
  // a finding pressed opens what to change inside its own row, under its title; the row comes
  // to the top of the findings' room, so its title stays in view over its advice
  // (Native Shell's `reveal`: the scroll views around a view bring it to show)
  const places: (ViewRef | undefined)[] = [];
  const showChosen = () => {
    const f = props.filter;
    if (f != null) places[f]?.reveal({ vertical: "start" });
  };
  effect(() => {
    if (props.filter != null) showChosen();
  });
  /** Whether a finding has anything to open: its fixes, or (the app's own) a line for sonde.toml. */
  const advised = (o: Observation) => !!o.fixes?.length || !!(props.onExpect && o.identity);
  const list = () => props.view.observations.slice(0, props.max ?? props.view.observations.length);
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
                disabled={!props.onFilter || !o().episodes?.length}
                onPress={() => press(i())}
              >
                <Text class={o().level === "warn" ? "text-warn" : "text-muted-foreground"}>
                  {o().level === "warn" ? "! " : "· "}
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
  const row = (o: () => Observation, i: () => number) => (
    <ListRow
      divided
      top
      first={i() === 0}
      testID={`observation-${i()}`}
      label={observationText(o())}
      selected={props.filter === i()}
      disabled={!props.onFilter || !o().episodes?.length}
      onPress={() => press(i())}
      class={dense() ? "py-1" : ""}
      // by rank as the pane narrows: the finding's words keep their width; what acting on it
      // saves goes first, then the count's words (its chevron stays)
      trailing={
        <>
          <If when={dense() && props.filter !== i() && !props.compact && !props.narrow && observationSaving(o())}>{(v) => saving(v as () => string, i)}</If>
          <If when={props.onFilter && o().episodes?.length}>
            <View class={`${props.narrow ? "" : WIDTH.count} flex-row items-center justify-end gap-1`}>
              <If when={!props.narrow}>
                <Text class="tabular-nums text-caption text-muted-foreground group-selected:text-foreground">{t("figure.operationsN", { n: o().episodes.length })}</Text>
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
        <Text class={`text-body text-foreground ${dense() && props.filter !== i() ? "line-clamp-1" : "line-clamp-2"}`}>{observationText(o())}</Text>
        <If when={(!dense() || props.filter === i()) && observationWhy(o())}>{(w) => <Text class="text-small leading-snug text-muted-foreground line-clamp-2">{w()}</Text>}</If>
        <If when={(!dense() || props.filter === i()) && !props.compact && observationSaving(o())}>{(v) => saving(v as () => string, i)}</If>
      </View>
    </ListRow>
  );
  const findings = () => (
    // a short window keeps room for the operations: the findings scroll
    <Scroll class={`overflow-hidden ${CARD}`} style={props.maxHeight ? { maxHeight: props.maxHeight } : undefined} scrollbar={false}>
      <Each of={list()}>
        {(o: () => Observation, i) => (
          <View ref={(r) => (places[i()] = r)}>
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
                      <ExpectButton o={o()} onExpect={props.onExpect} />
                    </View>
                  }
                >
                  <FixPanel o={o()} sdk={fixSdk} onOpenCode={props.onOpenCode} onExpect={props.onExpect} />
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
          trailing={<If when={whatIfText(props.view)}>{(w) => <Text testID="what-if" class="min-w-0 flex-1 tabular-nums text-small text-muted-foreground line-clamp-1">{w()}</Text>}</If>}
        >
          {findings()}
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

/** A session's operations: outcome, time, family, title, requests, duration,
 * place in the session's time and notes. `rows` (default all) are the
 * episodes shown, by index; events name episodes by index too. */
/** The operations' columns as the user last sized them (kept with the app's sizes). */
const opsWidths = { bar: rememberedSize("ops.bar", 150), notes: rememberedSize("ops.notes", 320) };
const kept = (key: keyof typeof opsWidths, fallback: number) => untracked(() => opsWidths[key].size()) || fallback;

/** The operations as a table to paste (tab-separated), in the order given:
 * figures as plain numbers in one unit each, named in the column's title —
 * what the table shows ("840 ms", "2.35 s") would not sort or sum. */
export function operationsText(view: SessionView, order: number[]): string {
  const plain = (s: string) => s.replace(/[\t\r\n]+/g, " ");
  const head = [t("column.time"), t("column.operation"), t("column.requests"), t("sessions.tookMs"), t("column.notes")];
  const rows = order.flatMap((i) => {
    const e = view.episodes[i];
    // plain numbers on purpose (to sort and sum where they are pasted): not `count()`, not `duration()`
    return e ? [[e.start, plain(episodeTitle(e)), `${e.records.length}`, `${Math.round(episodeMs(e) * 1000) / 1000}`, plain(episodeNotes(e, view.sharedNotes).join(" · "))]] : [];
  });
  return [head, ...rows].map((r) => r.join("\t")).join("\n");
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
  return { shown: ["status", "title", "duration", ...ranked], title: width > 0 ? Math.max(160, width - used - 2) : 340, widths };
}

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
      return s.key === "duration" ? episodeMs(e) : s.key === "requests" ? e.records.length : time(e.start);
    };
    const dir = s.direction === "descending" ? -1 : 1;
    return [...list].sort((a, b) => (by(a) - by(b)) * dir || a - b);
  };
  props.order?.(() => shown());
  const formats = () => props.view.episodes.some((e) => family(e).label);
  const range = () => span(props.view);
  const ep = (row: number) => props.view.episodes[shown()[row]];
  // the table's own width decides what it shows (`operationColumns`); the operation's name takes the rest
  const width = signal(0);
  const layout = () =>
    operationColumns(
      terminal() ? 0 : width(),
      // beside the output or a terminal there is no room for a tag most rows do not have
      { format: formats() && !props.compact, bar: !terminal() && !props.compact, notes: !props.compact },
      { bar: kept("bar", 150), notes: kept("notes", 320) },
    );
  const titleWidth = () => (terminal() || width() > 0 ? layout().title : props.compact ? 220 : 340);
  const columns = () => {
    const l = layout();
    const column = (key: OpColumn) => {
      switch (key) {
        case "status":
          return { key, title: "", width: l.widths.status, resizable: false, selectable: false };
        case "title":
          return { key, title: t("column.operation"), width: titleWidth(), minWidth: 160 };
        case "duration":
          return { key, title: t("sessions.took"), width: l.widths.duration, align: "end" as const, sortable: true };
        case "requests":
          return { key, title: t("column.requests"), width: l.widths.requests, align: "end" as const, sortable: true };
        case "time":
          return { key, title: t("column.time"), width: l.widths.time, sortable: true };
        case "bar":
          return { key, title: t("column.when"), width: l.widths.bar, minWidth: 80 };
        case "format":
          return { key, title: t("column.format"), width: l.widths.format };
        case "notes":
          return { key, title: t("column.notes"), width: l.widths.notes, minWidth: 120 };
      }
    };
    // a terminal draws no dot and reads left to right as a log does: when, what, how many, how long
    const order: OpColumn[] = terminal() ? (["time", "format", "title", "requests", "duration", "notes"] as OpColumn[]).filter((k) => l.shown.includes(k)) : l.shown;
    return order.map(column);
  };
  const data = () => {
    return shown().map((i) => {
      const e = props.view.episodes[i];
      return {
        status: episodeTone(e),
        time: clock(e.start),
        format: family(e).label,
        // a terminal cuts it by a count of characters; a window where it runs out of room (the cell's own middle ellipsis)
        title: terminal() ? middle(e.outcome.kind !== "ok" ? `${episodeTitle(e)} ✗` : episodeTitle(e), props.compact ? 40 : 72) : episodeTitle(e),
        requests: count(e.records.length),
        duration: duration(episodeMs(e)),
        bar: "",
        notes: notes(e),
      };
    });
  };
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
      ref={props.ref}
      testID="operations"
      accessibilityLabel={t("sessions.operations")}
      class="min-h-0 flex-1"
      columns={columns()}
      data={data()}
      fontSize={TYPE.body}
      rowHeight={rowHeight(ROW.table)}
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
        { key: "time", title: t("column.time"), width: 110 },
        { key: "line", title: t("sessions.output"), width: 2000, minWidth: 160 },
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
        <Heading count={e().records.length}>{t("timeline.requestsOf")}</Heading>
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

const DAY = 86400000;
function dayLabel(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  // rounded: a day of 23 or 25 hours (a clock change) is still a day
  const diff = Math.round((today.getTime() - new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()) / DAY);
  if (diff <= 0) return t("time.today");
  if (diff === 1) return t("time.yesterday");
  return day(iso);
}

/** Arguments of one program's runs, each cut so that what tells it from the
 * others shows: where they all begin alike past a few words, that beginning
 * is said once as `…` and each shows the rest (`… --table orders`,
 * `… --table users`); alone, or already different at their start, they stay whole. */
export function tellApart(args: string[]): string[] {
  const distinct = [...new Set(args)];
  if (distinct.length < 2) return args;
  let common = distinct[0];
  for (const a of distinct) while (!a.startsWith(common)) common = common.slice(0, -1);
  // back to a word's edge; a short beginning is worth keeping whole
  common = common.slice(0, common.lastIndexOf(" ") + 1);
  if (common.length < 16) return args;
  return args.map((a) => `… ${a.slice(common.length)}`);
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
  // runs of one program are told apart: by the argument that differs, and where the
  // command is the same to the letter, by the second they started
  const told = () => {
    const by = new Map<string, SessionInfo[]>();
    for (const x of props.sessions) by.set(name(x), [...(by.get(name(x)) ?? []), x]);
    const out = new Map<string, { args: string; twin: boolean }>();
    for (const group of by.values()) {
      const shown = tellApart(group.map(args));
      group.forEach((x, i) => out.set(x.path, { args: shown[i], twin: group.some((y) => y !== x && args(y) === args(x)) }));
    }
    return out;
  };
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
    <ListRow
      lines={2}
      testID={`session-${x().i}`}
      label={x().s.title ?? (x().s.command ? scrubCommand(x().s.command!) : x().s.name)}
      selected={props.selected === x().s.path}
      onPress={() => props.onSelect(x().s.path)}
      class="mb-1"
    >
      <View class="flex-row items-center gap-2">
        <If when={x().s.live}>
          <Dot tone="live" label={t("timeline.live")} />
        </If>
        <Text class="min-w-0 shrink text-body text-foreground line-clamp-1">{name(x().s)}</Text>
        <Text class="min-w-0 flex-1 font-mono text-caption text-muted-foreground line-clamp-1">{told().get(x().s.path)?.args ?? args(x().s)}</Text>
        {/* the same command run again: its time at the row's end, to the second */}
        <If when={told().get(x().s.path)?.twin}>
          <Text testID="session-time" class="shrink-0 tabular-nums text-caption text-foreground">{clock(x().s.created, "s")}</Text>
        </If>
      </View>
      <View class="flex-row items-center gap-2">
        {/* (a twin's time is at its row's end, to the second: not said twice) */}
        <Text class="min-w-0 flex-1 text-caption text-muted-foreground line-clamp-1">{[told().get(x().s.path)?.twin ? "" : hm(x().s.created), x().s.endpoint, t("sessions.requestsN", { n: x().s.requests })].filter(Boolean).join(" · ")}</Text>
        {/* tried under faults, and the program broke: said on its row, in a word (in full to a screen reader) */}
        <If when={worstStanding(x().s.trials) === "broke"}>
          <View accessibilityLabel={t("trial.brokeBadge")}>
            <Badge testID="session-broke" label={t("trial.broke")} tone="fail" />
          </View>
        </If>
        {/* (a row that says it broke keeps its line for its words: no mark beside the badge) */}
        <If when={x().s.shape?.length && worstStanding(x().s.trials) !== "broke"}>
          <Sparkline shape={x().s.shape!} id={x().s.path} />
        </If>
      </View>
    </ListRow>
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
            <View class={`${terminal() ? "" : "bg-sidebar"} px-2 pb-1 pt-2`}>
              <Heading quiet>{day()}</Heading>
            </View>
          )}
        </If>
      )}
    </List>
  );
}
