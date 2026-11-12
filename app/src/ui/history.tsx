// A program over its runs: the same command run again and again, each run's
// requests, waiting, cost and warnings, its baseline, and this run held to it.

import { type JSX, List, resource, Scroll, signal, Text, View } from "nsh";
import { DatePicker, DiffView, TextView } from "nsh/base";
import { count, date, day, duration, factor, money } from "../format";
import { t } from "../i18n";
import { say } from "../phrases";
import { scenarioLabel } from "../trials";
import { api } from "../native";
import { copy, report, sessionsChanged, sessionsTick } from "../store";
import type { CodeChange, GitStamp, ProgramHistory, Trial } from "../types";
import { Icon } from "./icon";
import { Actions, Await, Badge, Button, Card, CONTROL, Disclosure, Empty, ICON, ListRow, Modal, Muted, Notice, PageHeader, POPUP, Section, SimpleTable, Title, terminal, textual, SIZES, WIDTH } from "./kit";
import { cost, RunTrend, Sparkline } from "./plots";
import { programOf, scrubCommand } from "./sessionview";

export { scenarioLabel };


export const STANDING = { survived: "pass", degraded: "warn", broke: "fail", untested: "neutral", refused: "refused" } as const;

/** The standings "Try again what broke" takes up: what did not survive, and what the policy refused (never judged). */
export const AGAIN: Trial["standing"][] = ["broke", "degraded", "refused"];
/** The verdicts to try again: the last of each scenario that did not survive. */
export const toTryAgain = (trials: Trial[] | undefined): Trial[] => (trials ?? []).filter((x) => AGAIN.includes(x.standing));

/** A scenario's verdicts over the program's runs, in a few words, when there are two or more: "Broke on Nov 12, survived since", or "Survived since Nov 10". */
export function trialSince(x: Trial): string | null {
  const h = x.history ?? [];
  const last = h[h.length - 1];
  if (!last || h.length < 2) return null;
  const word = (s: Trial["standing"]) => t(`trial.${s}`).toLowerCase();
  let i = h.length - 1;
  while (i >= 0 && h[i].standing === last.standing) i--;
  const said = i >= 0 ? t("trial.sinceChanged", { was: word(h[i].standing), day: day(h[i].at), now: word(last.standing) }) : t("trial.sinceSame", { now: word(last.standing), day: day(h[0].at) });
  return said.charAt(0).toUpperCase() + said.slice(1);
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
 * Rust's phrases, or its English where a verdict kept before 16.0 has none.
 * The one place each of these is said: [`trialText`] is the figures only. */
export function trialReasons(x: Trial): string[] {
  return x.reasons.map((en, i) => {
    const said = x.reasonsText?.[i];
    return said?.length ? say(said.map((p) => ({ key: p.key, args: p.args ?? {} })), en) : en;
  });
}

/** Where a verdict turns, and what to change: the request (its operation and
 * key), the line of the program that sent it when the run named its callers,
 * and the fix in a sentence. Under a trial that broke or degraded. */
export function TrialWhere(props: { trial: Trial }) {
  const w = () => props.trial.where;
  // what to change, said from Rust's phrase in the reader's language (a sentence of its own here: its first letter raised)
  const advice = () => {
    const a = props.trial.advice;
    const said = a ? say([{ key: a.key, args: a.args ?? {} }], "") : "";
    return said ? said.charAt(0).toUpperCase() + said.slice(1) : null;
  };
  return (
    <If when={w() || advice()}>
      <View testID="trial-where" class="gap-1">
        <If when={w()}>
          {(x) => (
            <View class="flex-row items-start gap-2">
              <View class="min-h-5 justify-center">
                <Icon name="circle-dot" size={ICON.small} />
              </View>
              {/* the request, then (after a dot) the program's line when the run named its callers */}
              <Text class="min-w-0 flex-1 text-small leading-5 text-foreground">
                {`${x().operation} `}
                <Text class="font-mono text-caption text-foreground">{x().key}</Text>
                <If when={x().caller}>{(c) => <Text class="font-mono text-caption text-muted-foreground">{` · ${c()}`}</Text>}</If>
              </Text>
            </View>
          )}
        </If>
        <If when={advice()}>
          {(a) => (
            <View class="flex-row items-start gap-2">
              <View class="min-h-5 justify-center">
                <Icon name="sparkles" size={ICON.small} />
              </View>
              <Text testID="trial-advice" class="min-w-0 flex-1 text-small leading-5 text-foreground">{a()}</Text>
            </View>
          )}
        </If>
      </View>
    </If>
  );
}

/** The run under faults: a row a scenario, its standing first, and under a
 * trial that broke or degraded, why; a press opens the trial's own session
 * (a row whose session is gone only says what it found). */
function Trials(props: { trials: Trial[]; onOpen?: (path: string) => void; action?: JSX.Element }) {
  // the refused were never judged: their reasons in the policy's tone, not a failure's
  const reasonTone = (x: Trial) => (x.standing === "broke" ? "text-fail" : x.standing === "refused" ? "text-status-refused" : "text-foreground");
  return (
    <Section testID="history-trials" title={t("trial.title")} description={t("trial.hint")} action={props.action} plain>
      <Card pad="none">
        <Each of={props.trials} key={(x) => x.scenario}>
          {(x, i) => (
            <ListRow divided first={i() === 0} top testID={`trial-${x().scenario}`} label={`${t(`trial.${x().standing}`)}: ${scenarioLabel(x().scenario)}`} disabled={!x().session || !props.onOpen} onPress={() => x().session && props.onOpen?.(x().session!)} trailing={x().session && props.onOpen ? <Icon name="chevron-right" size={ICON.small} /> : undefined}>
              <View class={`${WIDTH.badge} shrink-0 flex-row`}>
                <Badge label={t(`trial.${x().standing}`)} tone={STANDING[x().standing]} />
              </View>
              <View class={`min-w-0 flex-1 gap-1 ${x().session ? "" : "opacity-60"}`}>
                <View class="flex-row flex-wrap items-baseline gap-x-2">
                  <Text class="text-body text-foreground">{scenarioLabel(x().scenario)}</Text>
                  <Text testID="trial-id" class="font-mono text-caption text-muted-foreground">{trialAsked(x())}</Text>
                </View>
                {/* the scenario over the program's runs: when it last changed */}
                <If when={trialSince(x())}>{(h) => <Text testID={`trial-history-${x().scenario}`} class="text-small text-foreground">{h()}</Text>}</If>
                <Muted class="text-small">{trialText(x())}</Muted>
                {/* why it is not "survived", a line each */}
                <If when={x().standing !== "survived" && x().reasons.length > 0}>
                  <View testID="trial-reasons" class="gap-1">
                    <Each of={trialReasons(x())}>{(r) => <Text class={`text-small ${reasonTone(x())}`}>{r()}</Text>}</Each>
                  </View>
                </If>
                {/* where it turned (the request, the program's line), and what to change */}
                <If when={x().standing !== "survived"}>
                  <TrialWhere trial={x()} />
                </If>
                <If when={!x().session}>
                  <Muted class="text-caption">{t("trial.sessionGone")}</Muted>
                </If>
              </View>
            </ListRow>
          )}
        </Each>
      </Card>
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
              <TextView testID="comment-markdown" markdown={props.h.markdown!} class="text-body text-foreground" />
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
      <Text class="min-w-0 flex-1 tabular-nums text-small text-foreground">{text()}</Text>
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
    <Modal open size="wide" onClose={props.onClose} testID="code-change">
      <Title>{t("history.codeChange")}</Title>
      <Await loading={change.loading()} error={change.error()} retry={() => change.refetch()}>
        <If when={change()}>
          {(x) => (
            <Scroll style={{ maxHeight: SIZES.dialogScroll }} contentClass="gap-3">
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
            </Scroll>
          )}
        </If>
      </Await>
      <Actions>
        <Button variant="outline" label={t("common.close")} onPress={props.onClose} testID="code-change-close" />
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
            <PageHeader title={t("history.title", { program: scrubCommand(x().program), n: x().runs.length })} subtitle={t("history.hint")}>
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
                  popupClass={`${POPUP} p-2`}
                  contentClass="flex-row gap-2"
                  presetsClass="gap-1"
                  presetClass="rounded-(--radius-sm) px-2 py-1 text-small text-foreground hover:bg-muted"
                  itemClass="size-8 items-center justify-center rounded-(--radius-sm) text-small text-foreground hover:bg-muted selected:bg-primary selected:text-primary-foreground"
                  trigger={
                    // a small outline button, as the baseline's beside it
                    <View testID="history-range" class={CONTROL.trigger}>
                      <Icon name="calendar-clock" />
                      <Text class="text-small text-foreground">{rangeText()}</Text>
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
                else={<SimpleTable columns={[t("column.time"), t("figure.requests"), t("profile.waiting"), t("profile.cost")]} rows={shown(x()).runs.map((r) => [date(r.created), n(r.requests), duration(r.busyMs), cost(r.cost, r.currency)])} />}
              >
                <View class="flex-row flex-wrap gap-3">
                  <RunTrend history={shown(x())} measure="requests" title={t("figure.requests")} format={n} onOpen={props.onOpen} />
                  {/* the project's own limit on the waiting (sonde.toml's max-waiting), as a line */}
                  <RunTrend history={shown(x())} measure="busyMs" title={t("profile.waiting")} format={duration} onOpen={props.onOpen} budget={budget()?.maxWaitingMs} />
                  <RunTrend history={shown(x())} measure="cost" title={t("profile.cost")} format={(v) => cost(v, x().runs[0]?.currency ?? "USD")} onOpen={props.onOpen} />
                  <RunTrend history={shown(x())} measure="warnings" title={t("obs.warnings")} format={n} onOpen={props.onOpen} />
                </View>
              </If>
            </If>
          </Scroll>
        )}
      </If>
    </Await>
  );
}

/** A program run here: its runs' trend, its baseline, and whether the newest is past it. */
export interface ProgramSummary {
  program: string;
  runs: number;
  latest: string | null;
  created: string;
  points: { requests: number; busyMs: number; currency?: string }[];
  baseline: string | null;
  regressed: boolean;
  /** A month of it at the pace it runs. */
  monthly?: { cost: number; perDay: number } | null;
}

/** The programs run here, newest first: what each did over its runs, at a
 * glance, and the ones past their baseline marked. A press opens its newest
 * run on its runs. */
export function ProgramList(props: { onOpen: (path: string) => void; /** the place's search: the programs whose command holds it */ query?: string }) {
  const matching = (list: ProgramSummary[]) => {
    const q = (props.query ?? "").trim().toLowerCase();
    return q ? list.filter((x) => x.program.toLowerCase().includes(q)) : list;
  };
  // a failure is said as one, not as "no program has run here yet"
  const programs = resource(
    () => sessionsTick(),
    async () => (await api.sessions.programs()) as ProgramSummary[],
  );
  return (
    <Await loading={programs.loading() && !programs()} error={programs.error()} retry={() => programs.refetch()}>
    <If when={programs()}>
      {(list) => (
        <If when={matching(list()).length > 0} else={<Empty compact text={list().length > 0 ? t("sessions.noMatch") : t("programs.none")} />}>
          <List testID="program-list" class="flex-1" each={matching(list())} keyExtractor={(p) => p.program} estimatedItemSize={terminal() ? 2 : 52} scrollbar={false}>
            {(p) => (
              <ListRow
                testID="program-row"
                label={`${programOf(p().program) ?? scrubCommand(p().program)}, ${t("programs.runs", { n: p().runs })}${p().regressed ? `, ${t("programs.regressed")}` : ""}`}
                onPress={() => p().latest && props.onOpen(p().latest!)}
                lines={2}
              >
                <View class="flex-row items-center gap-2">
                  <Text class="min-w-0 flex-1 text-body text-foreground line-clamp-1">{programOf(p().program) ?? scrubCommand(p().program)}</Text>
                  <If when={p().regressed}>
                    <View testID="program-regressed"><Badge label={t("programs.regressed")} tone="warn" /></View>
                  </If>
                </View>
                <View class="flex-row items-center gap-2">
                  {/* how often it runs, and what a month of it costs at that pace */}
                  <Text class="min-w-0 flex-1 text-caption text-muted-foreground line-clamp-1">{[t("programs.runs", { n: p().runs }), day(p().created), p().monthly ? t("programs.monthly", { cost: money(p().monthly!.cost, p().points[0]?.currency ?? "USD") }) : ""].filter(Boolean).join(" · ")}</Text>
                  <If when={p().points.length > 1}>
                    <Sparkline shape={p().points.map((x) => x.requests)} id={`program:${p().program}`} />
                  </If>
                </View>
              </ListRow>
            )}
          </List>
        </If>
      )}
    </If>
    </Await>
  );
}
