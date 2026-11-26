// A run on screen, with no state of the app's own: its header and
// figures, what stands out (each a filter of the operations), the
// operations with their outcome and place in time, the program's output,
// and how output and operations line up. The app and the web viewer
// (sonde share) both draw with these.

import { Clipboard, computed, effect, type JSX, List, Scroll, signal, Text, token, untracked, View, type ViewRef } from "nsh";
import { DataTable, type DataTableRef, type DataTableSort, TextView } from "nsh/base";
import type { MenuItem as MenuItemSpec } from "nsh/desktop";
import { change, clock, count, date, duration, middle, shortKeys, transfer } from "../format";
import { t } from "../i18n";
import { episodeNotes, episodeTitle, observationSaving, observationText, observationWhy, said as saidArg, say } from "../phrases";
import { Chart, timed, Waterfall } from "./plots";
import { type SignalId, type SignalTile, signalsOf, signalTiles } from "../signals";
import type { Operations } from "../operations";
import type { Diagnosis, EngineSetting, EpisodeRow, Fix, FindingChange, LogLine, Observation, RecordSummary, RunView } from "../types";
import { Icon } from "./icon";
import { titleCells, BARE_TABLE, Badge, Button, Card, CARD, Choice, Disclosure, Dot, EMPHASIS, SELECTED, DOT, TONE, type Tone, Empty, fitColumns, FIT_PADDING, Heading, ICON, KeyValue, ListRow, Path, popupMenu, Pressable, ROW, rowHeight, Strong, Tip, Title, TYPE, SIZES, WIDTH, COLUMN, TEXT } from "./kit";
import { rememberedSize } from "./sizes";

import { commandLine, exactText, ranCommand } from "../command";

export { commandLine, exactText, programOf, ranCommand, scrubCommand } from "../command";
export { RunList, tellApart } from "./runlist";

export { clock };

const time = (iso: string) => new Date(iso).getTime();

/** Where a finding stands against its program's run before (crates/analysis/src/fixloop.rs). */
export type FindingState = FindingChange["state"];
const STATE_TONE: Record<FindingState, Tone> = { fixed: "ok", less: "info", more: "warn", new: "info", open: "quiet" };
/** A state as the reader's language says it ("Fixed", "又出现"). */
export const stateLabel = (s: FindingState): string => {
  const w = say([{ key: "fix." + s, args: {} }], s);
  return w.charAt(0).toUpperCase() + w.slice(1);
};
/** A finding's state, as a badge. */
export function StateBadge(props: { state: FindingState; testID?: string }) {
  return <Badge testID={props.testID} label={stateLabel(props.state)} tone={STATE_TONE[props.state]} />;
}

/** The tones a request's outcome takes: the status tones of the kit's one table (`TONE`). */
type StatusTone = Extract<Tone, "ok" | "miss" | "client" | "fail" | "refused" | "injected">;

/** How an operation ended, as a status color: lookups that found nothing are not errors; one that
 * failed on an answer sonde gave in the store's place (`injected`, the run's ids) is in sonde's
 * own tone, never the store's failure. */
export function episodeTone(e: Pick<EpisodeRow, "outcome" | "miss" | "notFound" | "requests"> & { records?: number[] }, injected?: Set<number>): StatusTone {
  if (e.outcome.kind === "refused") return "refused";
  if (e.outcome.kind === "failed" && injected?.size && e.records?.some((id) => injected.has(id))) return "injected";
  if (e.outcome.kind === "failed") return /^4\d\d/.test(e.outcome.detail) ? "client" : "fail";
  if (e.miss) return "miss";
  // every request a lookup that found nothing
  if (e.notFound && e.notFound === e.requests) return "miss";
  return "ok";
}

/** A request's status as a status color. */
export function statusTone(s: number | null): StatusTone {
  if (s === null) return "fail";
  if (s === 404) return "miss";
  if (s >= 500) return "fail";
  if (s >= 400) return "client";
  return "ok";
}


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
      {/* the dot says how it went; the code is read, a success quieter than the rest */}
      <Text class={`font-mono ${TEXT.captionSize} tabular-nums ${!props.injected && tone() === "ok" ? "text-muted-foreground" : "text-foreground"}`}>{code()}</Text>
    </View>
  );
}

const FORMAT_TEXT: Record<string, string> = { SlateDB: "text-fmt-slatedb", Iceberg: "text-fmt-iceberg", Delta: "text-fmt-delta", Lance: "text-fmt-lance" };
/** An operation's kind as a tag, in the app's language. */
const kindLabel = (k: EpisodeRow["shape"]): string => (k === "multipart" ? t("kind.multipart") : k === "retry" ? t("kind.retry") : k === "listing" ? t("kind.listing") : k === "probe" ? t("kind.probe") : "");

/** What kind of thing an operation is: its storage format, else its kind. */
function family(e: Pick<EpisodeRow, "format" | "shape">): { label: string; class: string } {
  if (e.format) return { label: e.format, class: FORMAT_TEXT[e.format] ?? "text-fmt-other" };
  const k = kindLabel(e.shape);
  return k ? { label: k, class: e.shape === "multipart" ? "text-fmt-mpu" : "text-fmt-other" } : { label: "", class: "" };
}

/** How long an operation ran, in ms (a number: `duration` writes it). */
export const episodeMs = (e: Pick<EpisodeRow, "start" | "end">) => time(e.end) - time(e.start);

/** The run's span in time: its first operation's start, its last one's end. */
export function span(v: RunView): [number, number] {
  if (!v.span) return [0, 1];
  const a = time(v.span.start);
  return [a, Math.max(time(v.span.end), a + 1)];
}

/** The commit a run's code was at, as a person reads it: a1b2c3d (main), changed. */
export function gitText(g: RunView["header"]["git"]): string {
  if (!g) return "";
  return `${g.commit.slice(0, 7)}${g.branch ? ` (${g.branch})` : ""}${g.dirty ? ` · ${t("git.dirty")}` : ""}`;
}


/** The command and where it ran, then the run's figures. */
/** Who sent a run's requests: the SDK most did ("from boto3 1.34"); an import (no command, or "imported from …") from
 * several callers (its principals) says how many, not the one SDK most of them used — "12+" only where the profile
 * kept the busiest of more (`callersCut`), never for exactly as many as it keeps. */
export function fromWhom(view: Pick<RunView, "header" | "profile" | "sdk">): string {
  const callers = view.profile?.callers?.length ?? 0;
  const command = view.header.command;
  const imported = !command || command.startsWith("imported from ");
  if (imported && callers > 1) return t("run.callers", { n: view.profile?.callersCut ? `${count(callers)}+` : callers });
  return view.sdk ? t("run.sdk", { sdk: view.sdk }) : "";
}

/** Where and when a run ran, its commit and who sent its requests: "rustfs · Nov 7, 2026, 00:22 · from object_store 0.14". */
export function runMeta(view: Pick<RunView, "header" | "profile" | "sdk">): string {
  return [view.header.endpoint, date(view.header.created), gitText(view.header.git), fromWhom(view)].filter(Boolean).join(" · ");
}

/** The time account's phrases said in its tooltip, not its line: where most of the time went (the program's own, or
 * moving bytes), the demo's times are indicative. */
const ASIDE = ["time.program", "time.bytes", "time.indicative"];
const capital = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** A run's header, one form at every size: its command (where and when it ran in its tooltip), its commands, the run
 * in a sentence, what marks it, and its figures in one line — requests, errors, median, bytes, at once — each
 * opening its chart, its detail (p95 and p99, each way's bytes) in its tooltip. */
export function RunHeader(props: { view: RunView; /** its commands, at the title's end */ actions?: () => JSX.Element; /** a press on a figure: its chart, on the Time pane */ onSignal?: (id: SignalId) => void; /** what marks the run (a trial of another run), under its sentence */ children?: JSX.Element }) {
  const s = () => props.view.summary;
  const line = () => {
    const parts = [t("timeline.operations", { n: count(s().requests), ops: t("figure.operationsN", { n: count(props.view.operationsTotal) }), secs: duration(s().seconds * 1000) })];
    if (s().errors) parts.push(t("timeline.errors", { n: s().errors }));
    if (s().misses) parts.push(t("timeline.notFound", { n: s().misses }));
    parts.push(transfer(s().bytesUp, s().bytesDown));
    return parts.join(" · ");
  };
  const command = () => props.view.header.command;
  // where the program's time went, in a line (a program's run: its own work and its round trips), without the counts
  // the figures under it say; what reads it (where most of it went, the demo's times) in its tooltip
  const story = () => {
    const x = props.view.time;
    if (!x) return "";
    const account = x.phrases.find((p) => p.key === "time.account");
    const rest = x.phrases.filter((p) => p !== account && !ASIDE.includes(p.key));
    return account ? t("run.time", Object.fromEntries(Object.entries(account.args ?? {}).map(([k, v]) => [k, saidArg(k, v)]))) + say(rest, "") : capital(say(x.phrases, x.text));
  };
  const aside = () =>
    (props.view.time?.phrases ?? [])
      .filter((p) => ASIDE.includes(p.key))
      .map((p) => capital(say([p], "").replace(/^[\s·]+/, "").replace(/^[(（](.*)[)）]$/, "$1")))
      .filter(Boolean)
      .join(". ");
  const tiles = () => {
    const x = signalsOf(props.view);
    return x ? signalTiles(x, props.view.summary.misses) : [];
  };
  // each figure said with what it counts: "172 requests · 0% errors · median 1.4 ms · 71.5 KiB · 8 at once"
  const said = (x: SignalTile) =>
    x.id === "rate"
      ? t("run.bandRequests", { n: s().requests })
      : x.id === "errors"
        ? t("run.foldedErrors", { v: x.value })
        : x.value === t("signal.notTimed")
          ? x.value
          : x.id === "latency"
            ? t("run.foldedLatency", { v: x.value })
            : x.id === "inFlight"
              ? t("run.atOnce", { v: x.value })
              : x.value;
  return (
    <View class="gap-2">
      <View class="flex-row items-center gap-3">
        <View selectable class="min-w-0 flex-1" tooltip={<Tip text={runMeta(props.view)} />}>
          {/* a run from elsewhere, beside its Run again: the command whole, nothing of it hidden */}
          <Title command={command() ? (props.view.external && ranCommand(command()) ? `$ ${exactText(command()!.trim())}` : commandLine(command()!)) : props.view.header.endpoint} />
        </View>
        {props.actions?.()}
      </View>
      {/* the run in a sentence: where its time went, whole */}
      <If when={story()}>
        <View tooltip={aside() ? <Tip text={aside()} /> : undefined}>
          <Text testID="run-story" accessibilityLabel={[story(), aside()].filter(Boolean).join(". ")} class="text-body leading-5 text-foreground">{story()}</Text>
        </View>
      </If>
      {props.children}
      {/* its figures in one line, quiet words, each opening its chart; what is not said is in its tooltip */}
      <View testID="run-summary" class="flex-row flex-wrap items-center" accessibilityLabel={line()}>
        <If when={tiles().length > 0} else={<Text class={`tabular-nums ${TEXT.note}`}>{line()}</Text>}>
          <Each of={tiles()}>
            {(x, i) => (
              <>
                <If when={i() > 0}>
                  <Text class={TEXT.note}>·</Text>
                </If>
                <Pressable label={[`${x().label}: ${x().value}`, x().detail].filter(Boolean).join(", ")} tip={x().detail || undefined} onPress={() => props.onSignal?.(x().id)} testID={`signal-${x().id}`}>
                  <View class="flex-row items-center gap-1">
                    <If when={x().tone !== "plain" && x().tone !== "quiet"}>
                      <Dot tone={x().tone as Tone} />
                    </If>
                    <Text class={`tabular-nums ${TEXT.note}`}>{said(x())}</Text>
                  </View>
                </Pressable>
              </>
            )}
          </Each>
        </If>
      </View>
    </View>
  );
}

const SDKS = { python: "Python · boto3", java: "Java SDK v2", go: "Go SDK v2", js: "JavaScript v3", rust: "Rust · object_store", "rust-aws": "Rust · aws-sdk-s3" } as const;
type Sdk = keyof typeof SDKS | "config";
/** A fix's tab: the engine whose setting it is, the SDK's own name, or (a change to the bucket, not the program) "Bucket" in the reader's language. */
const fixTitle = (f: Fix) => f.setting?.engine ?? (f.sdk === "config" ? t("fix.bucket") : SDKS[f.sdk]);
const LANG = { python: "python", java: "java", go: "go", js: "javascript", rust: "rust", "rust-aws": "rust", config: "xml" } as const;
/** A fix's language for its highlighting: an engine's by how its comment begins (Rust's `//`, SQL's `--`). */
const langOf = (f: Fix) => (f.sdk !== "config" ? LANG[f.sdk] : f.code.startsWith("//") ? "rust" : f.code.startsWith("--") ? "sql" : f.setting ? "bash" : LANG.config);
/** An engine's setting as one line to paste: a flag with its value (alone when it is `true`), else `key = value`. */
export const settingLine = (s: EngineSetting) => (s.key.startsWith("-") ? (s.value === "true" ? s.key : `${s.key} ${s.value}`) : `${s.key} = ${s.value}`);

/** The setting a fix changes, first and plainest: what, to what, and where the program sets it now (a press opens it). */
function SettingPanel(props: { s: EngineSetting; onOpenCode?: (place: string) => void }) {
  return (
    <View testID="fix-setting" class="gap-1">
      <View class="flex-row flex-wrap items-center gap-2">
        <Strong class="min-w-0 flex-1">{t("fix.setting", { engine: props.s.engine })}</Strong>
        <Button small variant="outline" icon="copy" label={t("fix.copySetting")} onPress={() => void Clipboard.writeText(settingLine(props.s)).catch(() => {})} testID="fix-setting-copy" />
      </View>
      <KeyValue mono rows={[[t("fix.settingKey"), props.s.key], [t("fix.settingValue"), props.s.value], ...(props.s.where ? [[t("fix.settingWhere"), props.s.where] as [string, string]] : [])]} />
      <If when={props.s.where} else={<Text class={TEXT.meta}>{t("fix.settingUnset", { engine: props.s.engine })}</Text>}>
        {(at) => (
          <If when={props.onOpenCode}>
            <ListRow label={t("fix.openCode", { place: at() })} onPress={() => props.onOpenCode?.(at())} testID="fix-setting-open">
              <Icon name="file" size={ICON.small} />
              <Text class="min-w-0 flex-1 font-mono text-small text-primary line-clamp-1">{at()}</Text>
            </ListRow>
          </If>
        )}
      </If>
    </View>
  );
}

/** The fix's SDK for the program's own: its user agent named it. */
const sdkOfAgent = (sdk: string | undefined): Sdk | null => {
  if (!sdk) return null;
  const s = sdk.toLowerCase();
  return s.startsWith("aws-sdk-rust") ? "rust-aws" : s.startsWith("boto") || s.startsWith("aws-cli") || s.startsWith("s3fs") ? "python" : s.includes("java") || s.includes("hadoop") ? "java" : s.includes("go") ? "go" : s.includes("-js") ? "js" : s.startsWith("object_store") ? "rust" : null;
};

/** "Copy as expected": the finding's line for sonde.toml's `expect`, copied (the app's own; not in a shared page). */
function ExpectButton(props: { o: Observation; onExpect?: (o: Observation) => void }) {
  return (
    <If when={props.onExpect && props.o.identity}>
      <Button small variant="ghost" icon="copy" label={t("expect.copy")} tip={t("expect.hint")} onPress={() => props.onExpect?.(props.o)} testID="finding-expect" />
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

/** A run's plan: its findings by root cause, ranked as given, each a diagnosis with one fix and one saving. A run
 * read by an older sonde (no diagnoses) has each finding as its own. */
export function diagnosesOf(v: Pick<RunView, "diagnoses" | "observations">): Diagnosis[] {
  return v.diagnoses?.length ? v.diagnoses : v.observations.map((o, i) => ({ id: o.identity ?? `${i}`, findings: [i], identities: o.identity ? [o.identity] : [] }));
}

/** A diagnosis's headline: its first finding. */
export const headOf = (v: Pick<RunView, "observations">, d: Diagnosis): Observation | undefined => v.observations[d.findings[0]];

/** What acting on a diagnosis saves, in the reader's language (its one finding's, where it is one). */
export function diagnosisSaving(v: Pick<RunView, "observations">, d: Diagnosis): string | null {
  if (d.saving?.length) return say(d.saving, d.savingText ?? "");
  const h = headOf(v, d);
  return d.findings.length === 1 && h ? observationSaving(h) : null;
}

/** A diagnosis's other findings, a sentence each: its requests one after another said as the rest of the change. */
export function alsoOf(v: Pick<RunView, "observations">, d: Diagnosis): string[] {
  const also = d.findings.slice(1).flatMap((i) => (i === d.serial || !v.observations[i] ? [] : [observationText(v.observations[i])]));
  return d.rest && d.serial !== undefined ? [...also, say([d.rest], "")].filter(Boolean) : also;
}

/** The fix's SDK: the program's own, as its requests' user agent named it (the finding's, else the run's). */
const ownSdk = (o: Observation | undefined, v: Pick<RunView, "sdk">) => sdkOfAgent(o?.sdk ?? v.sdk ?? undefined);

/** A diagnosis in the inspector: what was found and what it saves, why, the lines of code that sent it, and the
 * fix for the program's own SDK (the others behind a small menu), its acts at the end — Apply, where the app can
 * (the demo's job), the one primary. */
export function FixDetail(props: { view: RunView; diagnosis: Diagnosis; state?: FindingState | null; onOpenCode?: (place: string) => void; onExpect?: (o: Observation) => void; onStore?: (o: Observation) => void; onApply?: (o: Observation) => void }) {
  const o = () => headOf(props.view, props.diagnosis)!;
  const fixes = () => o().fixes ?? [];
  // another SDK than the program's own, chosen from the menu for this diagnosis
  const chosen = signal<{ id: string; sdk: Sdk } | null>(null);
  const other = () => (chosen()?.id === props.diagnosis.id ? chosen()!.sdk : null);
  // an engine's setting first: what sent the requests is the engine, not an SDK the program calls
  const shown = () => fixes().find((f) => f.setting) ?? fixes().find((f) => f.sdk === other()) ?? fixes().find((f) => f.sdk === ownSdk(o(), props.view)) ?? fixes()[0];
  // where in the program: the headline's lines, else the first of its findings that named any
  const callers = () => [o(), ...props.diagnosis.findings.slice(1).map((i) => props.view.observations[i])].find((x) => x?.callers?.length)?.callers ?? [];
  const saving = () => diagnosisSaving(props.view, props.diagnosis);
  return (
    <View testID="fix" class="gap-3">
      <View class="flex-row items-start gap-2">
        <If when={props.state}>
          {(st) => (
            <View class="min-h-5 justify-center">
              <StateBadge state={st()} />
            </View>
          )}
        </If>
        <Strong class="min-w-0 flex-1">{observationText(o())}</Strong>
      </View>
      <If when={saving()}>
        {(v) => (
          <View class="flex-row items-center gap-1">
            <Icon name="trending-down" size={ICON.small} tint="info" />
            <Text testID="fix-saving" class="min-w-0 flex-1 tabular-nums text-small text-info">{v()}</Text>
          </View>
        )}
      </If>
      <Each of={alsoOf(props.view, props.diagnosis)}>{(x) => <Text class={TEXT.note}>{x()}</Text>}</Each>
      <If when={observationWhy(o())}>{(why) => <Text testID="fix-why" class="text-small leading-snug text-muted-foreground">{why()}</Text>}</If>
      {/* the engine's setting, the one edit to make: before the code */}
      <If when={shown()?.setting}>{(s) => <SettingPanel s={s()} onOpenCode={props.onOpenCode} />}</If>
      {/* where in the program, when it said: the place to change */}
      <If when={callers().length}>
        <View testID="fix-callers" class="gap-1">
          <Each of={callers()} key={(c) => c.place}>
            {(c) => (
              // a shared page cannot open the code: the place is said, not offered (a row that does nothing on a press)
              <ListRow label={props.onOpenCode ? t("fix.openCode", { place: c().place }) : c().place} disabled={!props.onOpenCode} onPress={() => props.onOpenCode?.(c().place)} trailing={<Text class={`tabular-nums ${TEXT.meta}`}>{t("run.bandRequests", { n: c().requests })}</Text>}>
                <Icon name="file" size={ICON.small} />
                <Text class={`min-w-0 flex-1 font-mono text-small line-clamp-1 ${props.onOpenCode ? "text-primary" : "text-foreground"}`}>{c().place}</Text>
              </ListRow>
            )}
          </Each>
        </View>
      </If>
      <If when={shown()} else={<Text class={TEXT.note}>{t("expect.hint")}</Text>}>
        {(f) => (
          <View class="gap-2">
            <View class="flex-row flex-wrap items-center gap-2">
              {/* (the inspector's title says what this is) the program's own SDK; another, from a small menu */}
              <View class="flex-1" />
              <If when={fixes().length > 1}>
                <Choice inline testID="fix-sdk" value={f().sdk} items={fixes().map((x) => ({ value: x.sdk, title: fixTitle(x) }))} onChange={(v) => chosen.set({ id: props.diagnosis.id, sdk: v as Sdk })} />
              </If>
              <Button small variant="ghost" icon="copy" label={t("fix.copy")} onPress={() => void Clipboard.writeText(sample(f().code)).catch(() => {})} testID="fix-copy" />
            </View>
            {/* highlighted; a line too long scrolls sideways under its own indicator (Native Shell's TextView) */}
            <TextView
              testID="fix-code"
              markdown={`\`\`\`${langOf(f())}\n${sample(f().code)}\n\`\`\``}
              // (a well: a muted fill, no border; the code's own padding only, its first line at the top)
              classes={{ codeBlock: "rounded-(--radius) border-0 bg-muted/60 font-mono text-small", codeBlockCode: "px-3 py-2" }}
            />
          </View>
        )}
      </If>
      {/* what can be done with it, at its end: the demo's fix made in its code and the job run again (the loop in one
          press), the store's side where it has one, or no change: a habit kept on purpose, which sonde check expects */}
      <View class="flex-row flex-wrap items-center gap-2">
        <If when={props.onApply && fixes().length}>
          <Button small variant="primary" icon="play" label={t("fix.apply")} onPress={() => props.onApply?.(o())} testID="finding-apply" />
        </If>
        <StoreFixButton o={o()} onStore={props.onStore} />
        <ExpectButton o={o()} onExpect={props.onExpect} />
      </View>
    </View>
  );
}

/** Where each diagnosis begins, by its number in the plan: for the shape. */
export function findingMarks(v: RunView): { tMs: number; n: number }[] {
  const t0 = v.span ? Date.parse(v.span.start) : 0;
  return diagnosesOf(v).flatMap((d, i) => {
    const at = headOf(v, d)?.firstStart;
    return at ? [{ tMs: Math.max(0, Date.parse(at) - t0), n: i + 1 }] : [];
  });
}

/** Acting on every finding at once, as an estimate: requests, waiting, cost. */
export function whatIfText(v: RunView): string | null {
  const w = v.whatIf;
  if (!w) return null;
  // the cost as what changes: amounts of thousandths of a cent read as nothing
  const cut = w.cost[0] > 0 ? change(w.cost[0], w.cost[1]) : "—";
  // a log without times says nothing of waiting
  if (!timed(v.profile) || !w.waitingMs) return t("whatIf.untimed", { n: w.findings, a: count(w.requests[0]), b: count(w.requests[1]), cut });
  return t("whatIf.text", { n: w.findings, a: count(w.requests[0]), b: count(w.requests[1]), wa: duration(w.waitingMs[0]), wb: duration(w.waitingMs[1]), cut });
}

/** What stands out in a run — its core result, as a plan: a diagnosis a line, ranked by the time it saves, with what
 * acting on it saves; chosen, its other findings under it, and (`onSelect`) its operations narrowed and its fix in
 * the inspector. */
export function Diagnoses(props: { view: RunView; selected?: number | null; onSelect?: (i: number | null) => void; dense?: boolean; narrow?: boolean; maxHeight?: number; /** each finding's state against the program's run before, by its identity (fixloop's `statesOf`) */ states?: Record<string, FindingChange> }) {
  const press = (i: number) => props.onSelect?.(props.selected === i ? null : i);
  const list = () => diagnosesOf(props.view);
  // the row chosen comes into the findings' room (Native Shell's `reveal`: the scroll views around a view bring it to show)
  const places: (ViewRef | undefined)[] = [];
  // each row's height as drawn: a bounded room ends on a row's edge, never through its text
  const heights = signal<number[]>([]);
  const measured = (i: number) => ({ bounds }: { bounds: { size: { height: number } } }) => {
    const h = Math.round(bounds.size.height);
    if (heights()[i] === h) return;
    const next = [...heights()];
    next[i] = h;
    heights.set(next);
  };
  /** The room the plan takes, and how many show whole in it: all where they fit; else as many whole ones as fit
   * (the rest scroll, and the heading says so). */
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
  effect(() => {
    const i = props.selected;
    if (i != null) places[i]?.reveal({ vertical: "start" });
  });
  /** Its state against the run before, where it tells something: new or less (open is left unmarked). */
  const state = (d: Diagnosis) => {
    const id = headOf(props.view, d)?.identity;
    const f = id ? props.states?.[id] : undefined;
    return f && f.state !== "open" && f.state !== "fixed" ? f.state : null;
  };
  // the open plan, and what acting on all of it saves when more than one diagnosis saves something
  const open = signal(true);
  const saving = () => list().filter((d) => diagnosisSaving(props.view, d)).length;
  const row = (d: () => Diagnosis, i: () => number) => {
    const o = () => headOf(props.view, d())!;
    const chosen = () => props.selected === i();
    return (
      <ListRow
        divided
        top
        first={i() === 0}
        testID={`observation-${i()}`}
        label={observationText(o())}
        selected={chosen()}
        disabled={!props.onSelect}
        onPress={() => press(i())}
        class={props.dense ? "py-1" : ""}
        // what acting on it saves at the row's end, but where the pane is narrow (the inspector says it)
        trailing={
          <If when={!props.narrow && diagnosisSaving(props.view, d())}>
            {(v) => (
              <View class="shrink-0 flex-row items-center gap-1">
                <Icon name="trending-down" size={ICON.small} tint="info" />
                <Text testID={`observation-saving-${i()}`} class="tabular-nums text-small text-info">{v()}</Text>
              </View>
            )}
          </If>
        }
      >
        {/* its number, in its level's tone: where it stands on the run's shape */}
        <View class={`mt-1 size-4 shrink-0 items-center justify-center rounded-full ${o().level === "warn" ? "bg-warn/15" : "bg-muted"}`}>
          {/* emphasis: the number the shape marks this diagnosis with */}
          <Text class={`tabular-nums ${TEXT.captionSize} ${EMPHASIS} ${o().level === "warn" ? "text-warn" : "text-muted-foreground"}`}>{count(i() + 1)}</Text>
        </View>
        <View class="min-w-0 flex-1 gap-1">
          <View class="flex-row items-start gap-2">
            {/* where it stands against the program's run before: new, less */}
            <If when={state(d())}>
              {(st) => (
                <View class="min-h-5 justify-center">
                  <StateBadge state={st()} testID={`observation-state-${i()}`} />
                </View>
              )}
            </If>
            <Text class={`min-w-0 flex-1 ${TEXT.body} ${chosen() ? "line-clamp-2" : "line-clamp-1"}`}>{observationText(o())}</Text>
          </View>
          {/* chosen: its other findings, a line each */}
          <If when={chosen()}>
            <Each of={alsoOf(props.view, d())}>{(x) => <Text testID="diagnosis-also" class={`${TEXT.note} line-clamp-2`}>{x()}</Text>}</Each>
          </If>
        </View>
      </ListRow>
    );
  };
  return (
    <If when={list().length > 0}>
      <Disclosure
        title={t("obs.title")}
        count={list().length}
        expanded={open()}
        onToggle={(o) => open.set(o)}
        itemTestID="observations"
        testID="observations-toggle"
        trailing={
          <>
            {/* the room shows some of them whole: the rest scroll, and that is said */}
            <If when={open() && room().shown < list().length}>
              <Text testID="observations-scroll" class={`shrink-0 tabular-nums ${TEXT.meta}`}>{t("obs.scrolls", { n: count(room().shown), of: count(list().length) })}</Text>
            </If>
            <If when={saving() > 1 && whatIfText(props.view)}>{(w) => <Text testID="what-if" class={`min-w-0 flex-1 tabular-nums ${TEXT.note} line-clamp-1`}>{w()}</Text>}</If>
          </>
        }
      >
        {/* a short window keeps room for the operations: the plan scrolls */}
        <Scroll class={`overflow-hidden ${CARD}`} style={room().height ? { maxHeight: room().height } : undefined} scrollbar={false}>
          <Each of={list()}>
            {(d: () => Diagnosis, i) => (
              <View ref={(r) => (places[i()] = r)} onLayout={measured(i())}>
                {row(d, i)}
              </View>
            )}
          </Each>
        </Scroll>
      </Disclosure>
    </If>
  );
}

// the operations and the output fill their pane bare: no box of their own, the hairline under their titles only
const tableClasses = {
  ...BARE_TABLE,
  rowActive: "bg-row-hover",
  resizeHandle: "bg-transparent group-hover:bg-border",
};

/** Whether a table following its end goes to its newest row: rows were added
 * (`total` of them now, `known` before) and its view ended at the last row
 * there was (`endSeen`: the end of the rows in view at its last report, or 0
 * when it has reported none). A reader who went up to look at a row is left
 * there; back at the end, the table follows again. */
export const followsEnd = (total: number, known: number, endSeen: number): boolean => total > known && endSeen >= known;

/** A run's operations: outcome, time, family, title, requests, duration,
 * place in the run's time and notes. `rows` (default all) are the
 * episodes shown, by index; events name episodes by index too. */
/** The operations' columns as the user last sized them (kept with the app's sizes). */
const opsWidths = { bar: rememberedSize("ops.bar", 150), notes: rememberedSize("ops.notes", 320) };
const kept = (key: keyof typeof opsWidths, fallback: number) => untracked(() => opsWidths[key].size()) || fallback;

/** The operations as a table to paste (tab-separated), in the order given:
 * figures as plain numbers in one unit each, named in the column's title —
 * what the table shows ("840 ms", "2.35 s") would not sort or sum. */
export function operationsText(view: RunView, rows: EpisodeRow[]): string {
  const plain = (s: string) => s.replace(/[\t\r\n]+/g, " ");
  const head = [t("column.time"), t("column.operation"), t("column.requests"), t("runs.tookMs"), t("column.notes")];
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
const OP_WIDTH: Record<Exclude<OpColumn, "title" | "bar" | "notes">, number> = { status: 26, duration: 96, requests: 92, time: 128, format: 86 };
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
  view: RunView;
  /** The rows: a window over however many operations there are, sorted and narrowed where they are kept. */
  source: Operations;
  /** A row pressed: its operation, by index. */
  onSelect: (index: number) => void;
  onContextMenu?: (index: number) => void;
  ref?: (r: DataTableRef) => void;
  /** The operations marked (chosen, or running when an output line was printed), by index. */
  highlight?: number[];
  /** A run still being recorded: the table opens at its newest operation and stays at its end as more arrive, until the reader scrolls up (back at the end, it follows again). */
  follow?: boolean;
  /** Group by's choices: the operation's title opens them (a menu of the system's) */
  groupMenu?: () => MenuItemSpec[];
}) {
  // the requests sonde answered in the store's place (a trial's): their operations in sonde's own tone
  const hurt = computed(() => new Set(props.view.injected ?? []));
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
  // a column of tags only where they tell rows apart: none where no row has one, nor where every row has the same
  const formats = () => {
    const total = props.view.operationsTotal;
    const f = Object.entries(props.view.families.formats).filter(([, n]) => n > 0);
    const k = ["multipart", "retry", "listing", "probe"].map((x) => [x, props.view.families.kinds[x] ?? 0] as const).filter(([, n]) => n > 0);
    if (!f.length && !k.length) return false;
    const same = (f.length === 1 && f[0][1] >= total) || (!f.length && k.length === 1 && k[0][1] >= total);
    return !same;
  };
  const range = () => span(props.view);
  // a row of the table (counted from the first of them all) as the operation it shows, once its page is held
  const ep = (row: number): EpisodeRow | undefined => held()[row - start()];
  // the table's own width decides what it shows (`operationColumns`); the operation's name takes the rest
  const width = signal(0);
  const layout = () =>
    operationColumns(
      width(),
      // (no "when" bar beside the time: the time says when, and the strip over the table where in the run;
      // a narrow table drops what ranks last)
      { format: formats(), bar: false, notes: true },
      { bar: kept("bar", 150), notes: kept("notes", 320) },
    );
  const columns = () => {
    const l = layout();
    const column = (key: OpColumn) => {
      switch (key) {
        case "status":
          return { key, title: "", width: l.widths.status, resizable: false, selectable: false };
        case "title":
          // the room the others leave, as the table lays them out
          return { key, title: t("column.operation"), width: COLUMN.titleLeast, minWidth: COLUMN.titleLeast, grow: 1 };
        case "duration":
          return { key, title: t("runs.took"), width: l.widths.duration, align: "end" as const, sortable: true };
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
    return l.shown.map(column);
  };
  const data = () =>
    held().map((e) => ({
      status: episodeTone(e, hurt()),
      time: clock(e.start),
      format: family(e).label,
      // cut where it runs out of room (the cell's own middle ellipsis)
      title: episodeTitle(e),
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
            <Dot tone={episodeTone(e, hurt())} label={e.outcome.kind === "ok" ? undefined : e.outcome.kind === "refused" ? t("runs.refused") : e.outcome.detail} />
          </View>
        );
      case "format":
        // what wrote it, as a tag: a mark in its color and its name, quiet
        return (
          <View class="flex-1 flex-row items-center gap-1">
            <View class={`${DOT} ${family(e).class.replace("text-", "bg-")}`} />
            <Text class={`min-w-0 flex-1 ${TEXT.meta} line-clamp-1`}>{c.text}</Text>
          </View>
        );
      case "title":
        // cut in its middle, by the text itself: a name's end (a key's file, an id) is what tells two rows apart
        return (
          <Text class={`${TEXT.body} line-clamp-1`} style={{ textOverflow: "ellipsis-middle" }}>
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
              <View class={`rounded-full ${TONE[episodeTone(e, hurt())].fill}`} style={{ flex: width, minWidth: 3 }} />
              <View style={{ flex: Math.max(0, 1 - left - width) }} />
            </View>
          </View>
        );
      }
      case "notes":
        return <Text class={`${TEXT.note} line-clamp-1`}>{c.text}</Text>;
      case "time":
        // from the episode, not the cell's text: DataTable fits that to the column with its own font
        return <Text class={TEXT.time}>{clock(e.start)}</Text>;
      case "requests":
      case "duration":
        // figures line up at their end, under their titles
        return <View class="flex-1 items-end justify-center"><Text class={TEXT.small}>{c.text}</Text></View>;
      default:
        return <Text class={`${TEXT.body} line-clamp-1`}>{c.text}</Text>;
    }
  };
  // a row pressed, or Enter on it
  const choose = ({ row }: { row: number }) => {
    const e = ep(row);
    if (e) props.onSelect(e.index);
  };
  const grid = () => (
    <View class="min-h-0 flex-1" onLayout={({ bounds }) => width.set(Math.round(bounds.size.width))}>
    <DataTable
      ref={(r) => {
        table = r;
        props.ref?.(r);
      }}
      testID="operations"
      accessibilityLabel={t("runs.operations")}
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
      resizableColumns
      onColumnResize={({ key, width }) => {
        if (key in opsWidths) opsWidths[key as keyof typeof opsWidths].onResize({ size: width });
      }}
      classes={tableClasses}
      rowClasses={held().map((e) => (props.highlight?.includes(e.index) ? SELECTED : ""))}
      renderCell={cell}
      renderHeaderCell={(h) => (props.groupMenu && columns()[h.column]?.key === "title" ? <GroupHead title={h.title} menu={props.groupMenu} /> : titleCells(columns)(h))}
      rowSelectable
      sort={sort()}
      onSortChange={({ sort: next }) => sortBy(next)}
      onRowPress={choose}
      onRowActivate={choose}
      onContextMenu={props.onContextMenu ? (e) => { const at = e.row === undefined || e.row === null ? undefined : ep(e.row); if (at) props.onContextMenu?.(at.index); } : undefined}
      empty={<Empty compact text={t("common.empty")} />}
    />
    </View>
  );
  return grid();
}

/** The operations' title, as the way to Group by: a press opens its choices under it. */
function GroupHead(props: { title: string; menu: () => MenuItemSpec[] }) {
  let head: ViewRef | undefined;
  return (
    <View class="min-w-0 flex-1 flex-row items-center">
      <Pressable ref={(r) => (head = r)} label={t("split.groupBy")} tip={t("split.groupBy")} onPress={() => void popupMenu(props.menu(), head)} testID="group-by">
        <View class="flex-row items-center gap-1">
          <Text class={`min-w-0 shrink ${TEXT.meta} line-clamp-1`}>{props.title}</Text>
          <Icon name="chevron-down" size={ICON.small} tint="muted-foreground" />
        </View>
      </Pressable>
    </View>
  );
}

/** The program's output, one timed line per row. */
export function OutputTable(props: { lines: LogLine[]; onSelect: (i: number) => void; ref?: (r: DataTableRef) => void; highlight?: number[] }) {
  return (
    <DataTable
      ref={props.ref}
      testID="output"
      accessibilityLabel={t("runs.output")}
      class="min-h-0 flex-1"
      columns={[
        // as wide as its times, in the language's words ("上午 2:02:03.456")
        { key: "time", title: t("column.time"), width: COLUMN.outputTime, autoFit: true, fitPadding: FIT_PADDING },
        { key: "line", title: t("runs.output"), width: COLUMN.outputLine, minWidth: COLUMN.titleLeast },
      ]}
      renderHeaderCell={titleCells(() => [{}, {}])}
      data={props.lines.map((l) => ({ time: clock(l.time), line: l.text }))}
      fontSize={TYPE.small}
      fontFamily={token("font-mono")}
      rowHeight={rowHeight(ROW.dense)}
      classes={tableClasses}
      // its lines in mono, as the program wrote them; their times in the interface's face, as every time is
      renderCell={(c) => <Text class={`${c.key === "time" ? TEXT.time : `font-mono ${TEXT.small}`} line-clamp-1`}>{c.text}</Text>}
      rowClasses={props.lines.map((_, i) => (props.highlight?.includes(i) ? SELECTED : ""))}
      rowSelectable
      onRowPress={({ row }) => props.onSelect(row)}
      onRowActivate={({ row }) => props.onSelect(row)}
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
/** From the first request's start to the last one's end (ms): a loop, as an operation may hold more requests than a
 * call takes arguments (`Math.max(...rows)` throws past about a hundred thousand). */
export function spanMs(rows: Pick<RecordSummary, "time" | "latencyMs">[]): number {
  let first = Infinity;
  let last = -Infinity;
  for (const r of rows) {
    const at = new Date(r.time).getTime();
    first = Math.min(first, at);
    last = Math.max(last, at + r.latencyMs);
  }
  return rows.length ? Math.max(1, last - first) : 0;
}

export function OperationView(props: { episode: EpisodeRow; shared: string[]; rows: RecordSummary[]; onRecord: (id: number) => void; onBack?: () => void; back?: JSX.Element }) {
  // its requests' keys, their shared folders said short: what tells them apart stays in view
  const short = computed(() => shortKeys(props.rows.map((r) => r.target)));
  // one request of the operation: its status (a scenario's answer marked), what it was, how long it took
  const request = (r: () => RecordSummary, i: () => number) => (
    <View style={{ height: rowHeight(ROW.twoLine) }}>
      <ListRow divided first={i() === 0} testID={`op-request-${i()}`} label={`${r().kind} ${r().target}`} onPress={() => props.onRecord(r().id)} class="flex-1 py-0" trailing={<><Text class="text-small tabular-nums text-muted-foreground">{duration(r().latencyMs)}</Text><Icon name="chevron-right" size={ICON.small} /></>}>
        <View class={WIDTH.status}>
          <StatusMark status={r().status} injected={r().injected} />
        </View>
        <View class="min-w-0 flex-1">
          <View class="flex-row items-center gap-2">
            <Text class={`min-w-0 shrink ${TEXT.small} line-clamp-1`}>{r().kind}</Text>
            {/* a fault scenario answered it, not the store */}
            <If when={r().injected}>
              <Text testID="op-request-injected" class={`shrink-0 ${TEXT.meta}`}>{t("plots.injected")}</Text>
            </If>
          </View>
          <Path path={short()[i()] ?? r().target} whole={r().target} max={40} class="text-muted-foreground" />
        </View>
      </ListRow>
    </View>
  );
  const e = () => props.episode;
  // its requests sonde answered in the store's place (a trial's): the outcome in sonde's own tone
  const hurt = () => new Set(props.rows.filter((r) => r.injected).map((r) => r.id));
  const notes = () => episodeNotes(e(), []);
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
              <Dot tone={episodeTone(e(), hurt())} />
              {/* how it failed, beside its dot */}
              <Text class={TEXT.small}>{e().outcome.kind === "failed" ? (e().outcome as { detail: string }).detail : t("runs.refused")}</Text>
            </View>
          </If>
        </View>
        <Title class="leading-5">{episodeTitle(e())}</Title>
        <Text class={`tabular-nums ${TEXT.meta}`}>{`${clock(e().start)} · ${duration(episodeMs(e()))} · ${transfer(e().bytesUp, e().bytesDown)}`}</Text>
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
      <If when={props.rows.length > 1}>
        <Chart title={t("waterfall.title")} text={t("profile.waterfallText", { n: props.rows.length, time: duration(spanMs(props.rows)) })}>
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
