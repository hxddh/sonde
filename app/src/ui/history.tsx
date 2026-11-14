// A program over its runs: the same command run again and again, each run's
// requests, waiting, cost and warnings, its baseline, and this run held to it.

import { type JSX, List, resource, Scroll, signal, Text, View } from "nsh";
import { DatePicker, DiffView, TextView } from "nsh/base";
import { costColumn, count, date, day, duration, factor, hm } from "../format";
import { t } from "../i18n";
import { say } from "../phrases";
import { scenarioLabel } from "../trials";
import { api } from "../native";
import { copy, report, sessionsChanged, sessionsTick } from "../store";
import type { CodeChange, GitStamp, ProgramHistory, Trial } from "../types";
import { Icon } from "./icon";
import { ModalBody, Actions, Await, Badge, Button, Card, CONTROL, Row, Disclosure, Empty, ICON, ListRow, Modal, Muted, Notice, PageHeader, POPUP, POPUP_MOTION, Section, SimpleTable, Title, terminal, textual, SIZES, WIDTH, TEXT } from "./kit";
import { cost, RunTrend } from "./plots";
import { programOf, scrubCommand } from "./sessionview";

export { scenarioLabel };


export const STANDING = { survived: "ok", degraded: "warn", broke: "fail", untested: "quiet", refused: "refused" } as const;

/** The standings "Try again what broke" takes up: what did not survive, and what the policy refused (never judged). */
export const AGAIN: Trial["standing"][] = ["broke", "degraded", "refused"];
/** The verdicts to try again: the last of each scenario that did not survive. */
export const toTryAgain = (trials: Trial[] | undefined): Trial[] => (trials ?? []).filter((x) => AGAIN.includes(x.standing));

/** When a verdict was given: its time of day where that is today, else its day and time. */
const moment = (at: string) => {
  const d = new Date(at);
  return !Number.isNaN(d.getTime()) && d.toDateString() === new Date().toDateString() ? hm(at) : date(at);
};

/** A scenario's verdicts over the program's runs, in a few words, when there are two or more: how many of
 * them stood as the last did and since when — "Broke in 3 of 3 runs, since 08:33", or "Survived in 1 of 3
 * runs, since 08:40; broke before". */
export function trialSince(x: Trial): string | null {
  const h = x.history ?? [];
  const last = h[h.length - 1];
  if (!last || h.length < 2) return null;
  const word = (s: Trial["standing"]) => t(`trial.${s}`).toLowerCase();
  let i = h.length - 1;
  while (i >= 0 && h[i].standing === last.standing) i--;
  const streak = h.length - 1 - i;
  const at = moment(h[i + 1].at);
  const said = i >= 0 ? t("trial.sinceChanged", { now: word(last.standing), at, k: count(streak), n: count(h.length), was: word(h[i].standing) }) : t("trial.sinceSame", { now: word(last.standing), at, k: count(streak), n: count(h.length) });
  return said.charAt(0).toUpperCase() + said.slice(1);
}

/** A scenario of a trial as the verdicts list it: its verdict once it has one (`trial`), else how it stands so
 * far; `onOpen` opens its own run. */
export interface VerdictRow {
  scenario: string;
  trial: Trial | null;
  state: "waiting" | "running" | "done";
  onOpen?: () => void;
}

/** What to change for a verdict, in the reader's language (a sentence of its own: its first letter raised);
 * none for one that survived or gives no advice. */
export function adviceOf(x: Trial | null): string | null {
  const a = !x || x.standing === "survived" ? null : x.advice;
  const said = a ? say([{ key: a.key, args: a.args ?? {} }], "") : "";
  return said ? said.charAt(0).toUpperCase() + said.slice(1) : null;
}

/** The verdicts grouped by their advice, each advice said once over the scenarios it is for, in the order the
 * advice first comes; then the verdicts with none (survived, untested); then the scenarios not judged yet. */
export function groupByAdvice<R extends { trial: Trial | null }>(rows: R[]): { advice: string | null; rows: R[] }[] {
  const groups = new Map<string, { advice: string | null; rows: R[] }>();
  const plain: R[] = [];
  const pending: R[] = [];
  for (const r of rows) {
    const a = adviceOf(r.trial);
    if (!r.trial) pending.push(r);
    else if (!a) plain.push(r);
    else groups.set(a, { advice: a, rows: [...(groups.get(a)?.rows ?? []), r] });
  }
  return [...groups.values(), ...(plain.length ? [{ advice: null, rows: plain }] : []), ...(pending.length ? [{ advice: null, rows: pending }] : [])];
}

/** What a trial found, in figures: the faults and their retries (and those
 * no retry was owed for), the wait before a retry, what it did to the load
 * and to the run's time. What went wrong is said once, by its reasons
 * ([`trialReasons`]). What the verdict does not say (whether the waits were
 * jittered, with too few of them) is left unsaid. */
export function trialText(x: Trial): string {
  const parts: string[] = [];
  if (x.faults > 0) parts.push(x.retried === x.faults ? t("trial.allRetried", { n: count(x.faults) }) : t("trial.retried", { n: count(x.faults), m: count(x.retried) }));
  if (x.notRetryable) parts.push(t("trial.notRetryable", { n: count(x.notRetryable) }));
  if (x.backoffMs) {
    const [lo, , hi] = x.backoffMs;
    // jittered or in step only where the verdict says which
    parts.push(hi - lo < 1 ? t("trial.after", { t: duration(lo) }) : t(x.jitter === undefined || x.jitter === null ? "trial.afterRange" : x.jitter ? "trial.afterJitter" : "trial.afterStep", { lo: duration(lo), hi: duration(hi) }));
  }
  if (x.delayed) parts.push(t("trial.delayed", { n: count(x.delayed) }));
  // more requests than the clean run: so many times its load; fewer: the two counts (never "0.1×")
  if (x.cleanRequests > 0 && x.requests > x.cleanRequests) parts.push(t("trial.load", { x: factor(x.requests / x.cleanRequests) }));
  if (x.cleanRequests > 0 && x.requests < x.cleanRequests) parts.push(t("trial.loadFewer", { n: count(x.requests), of: count(x.cleanRequests) }));
  // how long it took against the clean run, once that is more than a blink apart
  if (x.wallMs && x.cleanWallMs && Math.abs(x.wallMs - x.cleanWallMs) >= Math.max(50, x.cleanWallMs * 0.1)) parts.push(t("trial.took", { t: duration(x.wallMs), clean: duration(x.cleanWallMs) }));
  if (x.wroteTwice > 0) parts.push(t("trial.twice", { n: count(x.wroteTwice) }));
  return parts.join(" · ");
}

/** A verdict's scenario as it was asked for: its id, and how hard it pressed when that was more than once (`slowdown:3`, what `sonde run --fault` takes). */
export const trialAsked = (x: Trial): string => (x.times && x.times > 1 ? `${x.scenario}:${x.times}` : x.scenario);

/** Why a trial is not "survived", a sentence each, in the reader's language:
 * Rust's phrases, its English where a reason has no phrase.
 * The one place each of these is said: [`trialText`] is the figures only. */
export function trialReasons(x: Trial): string[] {
  return x.reasons.map((en, i) => {
    const said = x.reasonsText?.[i];
    return said?.length ? say(said.map((p) => ({ key: p.key, args: p.args ?? {} })), en) : en;
  });
}

/** A verdict in a line: the scenario and its id, why it is not "survived" (or what it found) and how it stood
 * over the program's runs, cut at the line's end; its standing at the row's end, and a chevron where a press
 * opens its run. A scenario not judged yet says how it stands so far. */
function VerdictLine(props: { row: VerdictRow; first: boolean; testID: string; historyID: string }) {
  const x = () => props.row.trial;
  const why = () => {
    const v = x();
    if (!v) return "";
    return (v.standing === "survived" ? trialText(v) : (trialReasons(v)[0] ?? trialText(v))) || "";
  };
  const gone = () => !!x() && !x()!.session;
  return (
    <ListRow
      divided
      first={props.first}
      testID={props.testID}
      label={x() ? `${t(`trial.${x()!.standing}`)}: ${scenarioLabel(props.row.scenario)}` : scenarioLabel(props.row.scenario)}
      disabled={!props.row.onOpen}
      onPress={() => props.row.onOpen?.()}
      trailing={
        <>
          <If when={x()} else={<Badge label={props.row.state === "running" ? t("trial.running") : t("trial.waiting")} tone={props.row.state === "running" ? "info" : "quiet"} />}>
            {(v) => <Badge label={t(`trial.${v().standing}`)} tone={STANDING[v().standing]} />}
          </If>
          <If when={props.row.onOpen}>
            <Icon name="chevron-right" size={ICON.small} />
          </If>
        </>
      }
    >
      <View class={`min-w-0 flex-1 flex-row items-baseline gap-2 ${gone() ? "opacity-60" : ""}`}>
        <Text class={`shrink-0 ${TEXT.body}`}>{scenarioLabel(props.row.scenario)}</Text>
        <Text testID="trial-id" class={`shrink-0 ${TEXT.codeQuiet}`}>{x() ? trialAsked(x()!) : props.row.scenario}</Text>
        {/* why, then over the program's runs (or that its run is gone): one quiet line, cut at its end */}
        <View class="min-w-0 flex-1 flex-row items-baseline gap-2">
          {/* how it stood over the program's runs (a few words, kept whole), then why, cut at the line's end */}
          <If when={x() && trialSince(x()!)}>{(h) => <Text testID={props.historyID} class={`shrink-0 ${TEXT.note}`}>{h()}</Text>}</If>
          <Text class={`min-w-0 flex-1 ${TEXT.note} line-clamp-1`}>{[why(), gone() ? t("trial.sessionGone") : ""].filter(Boolean).join(" · ")}</Text>
        </View>
      </View>
    </ListRow>
  );
}

/** A trial's verdicts, one way wherever they are drawn (the trial's dialog, the Runs pane): grouped by what to
 * change, each advice said once over its scenarios, a line each. `id` names the rows (`${id}-${scenario}`). */
export function TrialVerdicts(props: { rows: VerdictRow[]; id: string; historyID: string }) {
  const groups = () => groupByAdvice(props.rows);
  return (
    <Card pad="none" testID={`${props.id}s`}>
      <Each of={groups()}>
        {(g, gi) => (
          <>
            <If when={g().advice}>
              {(a) => (
                <Row first={gi() === 0} testID="trial-advice">
                  <View class="min-h-5 shrink-0 justify-center">
                    <Icon name="sparkles" size={ICON.small} />
                  </View>
                  <Text class={`min-w-0 flex-1 ${TEXT.small} leading-5`}>{a()}</Text>
                </Row>
              )}
            </If>
            <Each of={g().rows}>
              {(r, i) => <VerdictLine row={r()} first={gi() === 0 && i() === 0 && !g().advice} testID={`${props.id}-${r().scenario}`} historyID={`${props.historyID}-${r().scenario}`} />}
            </Each>
          </>
        )}
      </Each>
    </Card>
  );
}

/** The run under faults, on the Runs pane: its verdicts grouped by what to change; a press opens a trial's own
 * run (a row whose run is gone only says what it found). */
function Trials(props: { trials: Trial[]; onOpen?: (path: string) => void; action?: JSX.Element }) {
  const rows = (): VerdictRow[] => props.trials.map((x) => ({ scenario: x.scenario, trial: x, state: "done", onOpen: props.onOpen && x.session ? () => props.onOpen!(x.session!) : undefined }));
  return (
    <Section testID="history-trials" title={t("trial.title")} action={props.action} plain>
      <TrialVerdicts rows={rows()} id="trial" historyID="trial-history" />
    </Section>
  );
}

function Verdicts(props: { h: ProgramHistory }) {
  const c = () => props.h.check!;
  const open = signal(false);
  const failed = () => c().verdicts.filter((v) => !v.ok);
  // each figure in its own unit: times as times, cost as cost, the rest counted
  const value = (what: string, v: number) => (what === "waiting" || what === "p95" ? duration(v) : what === "cost" ? cost(v, props.h.runs[0]?.currency ?? "USD") : count(Math.round(v)));
  // what went past, figure by figure, then the warnings that are new
  const lines = () => [...failed().map((v) => `${t(`verdict.${v.what}` as never)}: ${value(v.what, v.value)} > ${value(v.what, v.limit)}`), ...c().newWarnings].join("\n");
  return (
    <>
      <Notice testID="history-check" tone={c().passed ? "info" : "warn"} icon={c().passed ? "circle-check" : undefined} title={c().passed ? t("history.passed") : t("history.failed")} text={lines()}>
        {/* what CI would comment on the pull request, read here first */}
        <If when={props.h.markdown && !terminal()}>
          <Button small variant="outline" icon="scroll-text" label={t("history.commentOpen")} onPress={() => open.set(true)} testID="history-comment" />
        </If>
      </Notice>
      <If when={open()}>
        <Modal open size="wide" onClose={() => open.set(false)} testID="comment-preview">
          <Title>{t("history.comment")}</Title>
          <Muted class="text-small">{t("history.commentHint")}</Muted>
          <Card pad="none">
            <Scroll style={{ maxHeight: SIZES.dialogScroll }} contentClass="p-4">
              <TextView testID="comment-markdown" markdown={props.h.markdown!} class={TEXT.body} />
            </Scroll>
          </Card>
          <Actions>
            <Button variant="ghost" label={t("common.close")} onPress={() => open.set(false)} />
            <Button variant="primary" icon="copy" label={t("history.copyComment")} onPress={() => copy(props.h.markdown!)} testID="comment-copy" />
          </Actions>
        </Modal>
      </If>
    </>
  );
}

const commitText = (g?: GitStamp | null) => (g ? `${g.commit.slice(0, 7)}${g.dirty ? "+" : ""}` : "");

/** Where the program went past its baseline: the run, its commit, the last
 * within; and the code that changed between them, a press away. */
function SinceRow(props: { h: ProgramHistory }) {
  const s = () => props.h.since!;
  const open = signal(false);
  const text = () =>
    t("history.since", {
      n: s().runsAgo + 1,
      commit: commitText(s().firstPastCommit) || t("history.noCommit"),
      last: commitText(s().lastWithinCommit) || t("history.noCommit"),
    });
  const canDiff = () => !!(s().lastWithin && s().firstPastCommit && s().lastWithinCommit && s().firstPastCommit!.commit !== s().lastWithinCommit!.commit);
  return (
    <View testID="history-since" class="flex-row items-center gap-2">
      <Icon name="git-compare-arrows" tint="warn" />
      <Text class={`min-w-0 flex-1 tabular-nums ${TEXT.small}`}>{text()}</Text>
      <If when={!terminal() && canDiff()}>
        <Button small variant="outline" icon="git-compare-arrows" label={t("history.codeChangeOpen")} onPress={() => open.set(true)} testID="history-code-change" />
      </If>
      <If when={open()}>
        <CodeChangeDialog before={s().lastWithin!} after={s().firstPast} onClose={() => open.set(false)} />
      </If>
    </View>
  );
}

/** The code between two runs, file by file (the ones whose lines sent the
 * later run's findings' requests first), those findings on their lines. */
function CodeChangeDialog(props: { before: string; after: string; onClose: () => void }) {
  // a failure (no repository, a commit gone) is said in place
  const change = resource(
    () => [props.before, props.after] as const,
    ([a, b]): Promise<CodeChange> => api.sessions.codeChange(a, b),
  );
  const ext = (p: string) => p.split(".").pop() ?? "";
  return (
    // one frame while its change loads and once it is there: the dialog keeps its size
    <Modal open steady size="wide" onClose={props.onClose} testID="code-change">
      <Title>{t("history.codeChange")}</Title>
      <ModalBody>
      <Await loading={change.loading()} error={change.error()} retry={() => change.refetch()}>
        <If when={change()}>
          {(x) => (
            <View class="gap-3">
              <Muted class="font-mono text-small">{`${x().from.slice(0, 7)} → ${x().to.slice(0, 7)}`}</Muted>
              <If when={x().files.length} else={<Muted>{t("history.noCodeChange")}</Muted>}>
                <Each of={x().files} key={(f) => f.path}>
                  {(f, i) => (
                    <Disclosure title={f().path} count={f().notes.length || undefined} open={i() === 0}>
                      <DiffView
                        oldValue={f().old}
                        newValue={f().new}
                        language={ext(f().path)}
                        layout="unified"
                        inline="word"
                        contextLines={3}
                        annotations={f().notes.map((n, k) => ({ id: `${k}`, side: "new" as const, line: Math.max(0, n.line - 1) }))}
                        renderAnnotation={(a) => <Notice tone="warn" icon="radio" class="mx-2 my-1" text={f().notes[Number(a.id)]?.text ?? ""} />}
                        class="rounded-(--radius) border border-border"
                        style={{ height: SIZES.dialogScroll }}
                      />
                    </Disclosure>
                  )}
                </Each>
              </If>
              <If when={x().more > 0}>
                <Muted class="text-small">{t("history.moreFiles", { n: x().more })}</Muted>
              </If>
            </View>
          )}
        </If>
      </Await>
      </ModalBody>
      <Actions>
        <Button variant="primary" label={t("common.close")} onPress={props.onClose} testID="code-change-close" />
      </Actions>
    </Modal>
  );
}

/** The Runs pane of a session: its program over its runs. */
export function HistoryPane(props: { path: string; onOpen?: (path: string) => void; /** "Try again what broke": a fresh run, then the scenarios that did not survive */ onTryAgain?: (trials: Trial[]) => void }) {
  // the program's own budget, where its project gives one
  const budget = () => {
    try {
      return api.sessions.budget(props.path);
    } catch {
      return null;
    }
  };
  // a failure is said as one (with Try again), never as "one run so far"
  // read again when the sessions change: a run ended, one was deleted, a baseline set elsewhere
  const h = resource(
    () => [props.path, sessionsTick()] as const,
    ([p]) => api.sessions.history(p),
  );
  const isBase = () => h()?.baseline === props.path;
  const toggle = () => {
    try {
      api.sessions.setBaseline(props.path, !isBase());
      sessionsChanged();
      h.refetch();
    } catch (e) {
      report(e);
    }
  };
  const n = (v: number) => count(Math.round(v));
  // the runs' costs in one unit, said once in the trend's name
  const currencyOf = (h: ProgramHistory) => h.runs[0]?.currency ?? "USD";
  const costs = (h: ProgramHistory) => costColumn(shown(h).runs.map((r) => r.cost), currencyOf(h));
  // a moment's day on the reader's calendar, as the picker's days are: YYYY-MM-DD
  const localDay = (at: string | number) => {
    const d = new Date(at);
    return Number.isNaN(d.getTime()) ? `${at}`.slice(0, 10) : `${d.getFullYear()}-${`${d.getMonth() + 1}`.padStart(2, "0")}-${`${d.getDate()}`.padStart(2, "0")}`;
  };
  // the runs of a stretch of days: a range, or every run
  const range = signal<[string | null, string | null] | null>(null);
  const within = (created: string) => {
    const r = range();
    const d = localDay(created);
    return !r || ((!r[0] || d >= r[0]) && (!r[1] || d <= r[1]));
  };
  // the runs of the range, each still with its commit
  const shown = (x: ProgramHistory): ProgramHistory => {
    const keep = x.runs.map((r) => within(r.created));
    return { ...x, runs: x.runs.filter((_, i) => keep[i]), commits: x.commits?.filter((_, i) => keep[i]) };
  };
  const daysAgo = (k: number) => localDay(Date.now() - k * 86400000);
  // a picker's day is a local one: read at noon, so no zone moves it to the day before
  const rangeText = () => {
    const r = range();
    return r ? `${r[0] ? day(`${r[0]}T12:00:00`) : "…"} – ${r[1] ? day(`${r[1]}T12:00:00`) : "…"}` : t("history.allRuns");
  };
  return (
    // asked again (a baseline set), what is shown stays until the answer is in
    <Await loading={h.loading() && !h()} error={h.error()} retry={() => h.refetch()}>
      <If when={h()} else={<Empty icon="list-clock" text={t("history.none")} />}>
        {(x) => (
          <Scroll testID="history-pane" class="min-h-0 flex-1" contentClass="gap-4 pb-4 pr-2 terminal:gap-2 terminal:pb-0 terminal:pr-0">
            {/* the command is the session's header, over this pane: not said again; the dashed line said where one is drawn */}
            <PageHeader title={t("history.title", { n: x().runs.length })} subtitle={!textual() && x().baseline && x().runs.length > 1 && shown(x()).runs.length > 0 ? t("history.hint") : undefined}>
              <If when={!terminal()}>
                <DatePicker
                  range
                  value={range()}
                  onChange={({ value }) => range.set(Array.isArray(value) ? value : null)}
                  cleanable
                  presets={[
                    { label: t("history.last7"), value: [daysAgo(7), daysAgo(0)] },
                    { label: t("history.last30"), value: [daysAgo(30), daysAgo(0)] },
                  ]}
                  accessibilityLabel={t("history.range")}
                  align="end"
                  offset={4}
                  popupClass={`${POPUP} p-2 ${POPUP_MOTION.class}`}
                  popupStartingClass={POPUP_MOTION.starting}
                  popupEndingClass={POPUP_MOTION.ending}
                  contentClass="flex-row gap-2"
                  presetsClass="gap-1"
                  presetClass={`rounded-(--radius-sm) px-2 py-1 ${TEXT.small} hover:bg-muted`}
                  itemClass={`size-8 items-center justify-center rounded-(--radius-sm) ${TEXT.small} hover:bg-muted selected:bg-primary selected:text-primary-foreground`}
                  trigger={
                    // a small outline button, as the baseline's beside it
                    <View testID="history-range" class={CONTROL.trigger}>
                      <Icon name="calendar-clock" />
                      <Text class={TEXT.small}>{rangeText()}</Text>
                      <Icon name="chevron-down" size={ICON.small} />
                    </View>
                  }
                />
              </If>
              {/* one button, its words the action: set, or (already the baseline) clear */}
              <Button small variant="outline" icon={isBase() ? "x" : "check"} label={isBase() ? t("history.clearBaseline") : t("history.makeBaseline")} tip={isBase() ? t("history.isBaseline") : undefined} onPress={toggle} testID="history-baseline" />
            </PageHeader>
            <If when={x().check}>
              <Verdicts h={x()} />
            </If>
            <If when={(x().trials?.length ?? 0) > 0}>
              <Trials
                trials={x().trials!}
                onOpen={props.onOpen}
                action={
                  // after a change to the program: only what did not survive, on a fresh run
                  <If when={props.onTryAgain && !terminal() && toTryAgain(x().trials).length > 0}>
                    <Button small variant="outline" icon="rotate-ccw" label={t("trial.again")} onPress={() => props.onTryAgain?.(toTryAgain(x().trials))} testID="trial-again" />
                  </If>
                }
              />
            </If>
            <If when={x().since}>
              <SinceRow h={x()} />
            </If>
            <If when={shown(x()).runs.length > 0 && x().runs.length > 1} else={<Empty compact text={x().runs.length > 1 ? t("history.noneInRange") : t("history.none")} />}>
              <If
                when={!textual()}
                else={<SimpleTable columns={[t("column.time"), t("figure.requests"), t("profile.waiting"), t("profile.cost")]} rows={shown(x()).runs.map((r) => [date(r.created), n(r.requests), duration(r.waitingMs), cost(r.cost, r.currency)])} />}
              >
                <View class="flex-row flex-wrap gap-3">
                  {/* each named as a tile is (a capital, the thing counted), the cost's unit in its name so its figure is short */}
                  <RunTrend history={shown(x())} measure="requests" title={t("trend.requests")} format={n} onOpen={props.onOpen} />
                  {/* the project's own limit on the waiting (sonde.toml's max-waiting), as a line */}
                  <RunTrend history={shown(x())} measure="waitingMs" title={t("trend.waiting")} format={duration} onOpen={props.onOpen} budget={budget()?.maxWaitingMs} />
                  <RunTrend history={shown(x())} measure="cost" title={t("trend.cost", { unit: costs(x()).unit })} format={(v) => costColumn([v, ...shown(x()).runs.map((r) => r.cost)], currencyOf(x())).figures[0]} onOpen={props.onOpen} />
                  <RunTrend history={shown(x())} measure="errors" title={t("trend.errors")} format={n} onOpen={props.onOpen} budget={budget()?.maxErrors} />
                  <RunTrend history={shown(x())} measure="p95Ms" title={t("trend.p95")} format={duration} onOpen={props.onOpen} budget={budget()?.maxP95Ms} />
                  <RunTrend history={shown(x())} measure="warnings" title={t("trend.warnings")} format={n} onOpen={props.onOpen} />
                </View>
              </If>
            </If>
          </Scroll>
        )}
      </If>
    </Await>
  );
}
