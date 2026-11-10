// A program over its runs: the same command run again and again, each run's
// requests, waiting, cost and warnings, its baseline, and this run held to it.

import { List, resource, Scroll, signal, Text, View } from "nsh";
import { DatePicker, DiffView, TextView } from "nsh/base";
import { count, date, day, duration, money } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { copy, report, sessionsChanged, sessionsTick } from "../store";
import type { CodeChange, GitStamp, ProgramHistory } from "../types";
import { Icon } from "./icon";
import { Actions, Await, Badge, Button, Card, CONTROL, Disclosure, Empty, ListRow, Modal, Muted, Notice, PageHeader, POPUP, SimpleTable, Title, terminal, textual } from "./kit";
import { cost, offset, RunTrend, Sparkline } from "./plots";
import { programOf, scrubCommand } from "./sessionview";

/** How high a dialog's scrolling part (a comment, a file's diff) grows before it scrolls: one cap for all. */
const TALL = 480;

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
            <Scroll style={{ maxHeight: TALL }} contentClass="p-4">
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
function SinceRow(props: { h: ProgramHistory; onOpen?: (path: string) => void }) {
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
      <Icon name="git-compare-arrows" size={14} tint="warn" />
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
            <Scroll style={{ maxHeight: TALL }} contentClass="gap-3">
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
                        annotations={f().notes.map((n, k) => ({ id: String(k), side: "new" as const, line: Math.max(0, n.line - 1) }))}
                        renderAnnotation={(a) => <Notice tone="warn" icon="radio" class="mx-2 my-1" text={f().notes[Number(a.id)]?.text ?? ""} />}
                        class="rounded-(--radius) border border-border"
                        style={{ height: TALL }}
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
export function HistoryPane(props: { path: string; onOpen?: (path: string) => void }) {
  // a failure is said as one (with Try again), never as "one run so far"
  const h = resource(
    () => props.path,
    (p) => api.sessions.history(p),
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
    return Number.isNaN(d.getTime()) ? String(at).slice(0, 10) : `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
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
          <Scroll testID="history-pane" class="min-h-0 flex-1" contentClass={terminal() ? "gap-2" : "gap-4 pb-4 pr-2"}>
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
                      <Icon name="calendar-clock" size={14} />
                      <Text class="text-small text-foreground">{rangeText()}</Text>
                      <Icon name="chevron-down" size={12} />
                    </View>
                  }
                />
              </If>
              {/* one button, its words the action: set, or (already the baseline) clear */}
              <Button small variant="outline" icon={isBase() ? undefined : "check"} label={isBase() ? t("history.clearBaseline") : t("history.makeBaseline")} onPress={toggle} testID="history-baseline" />
            </PageHeader>
            <If when={x().check}>
              <Verdicts h={x()} />
            </If>
            <If when={x().since}>
              <SinceRow h={x()} onOpen={props.onOpen} />
            </If>
            <If when={shown(x()).runs.length > 0 && x().runs.length > 1} else={<Muted class="text-small">{x().runs.length > 1 ? t("history.noneInRange") : t("history.none")}</Muted>}>
              <If
                when={!textual()}
                else={<SimpleTable columns={[t("column.time"), t("figure.requests"), t("profile.waiting"), t("profile.cost")]} rows={shown(x()).runs.map((r) => [date(r.created), n(r.requests), offset(r.busyMs), cost(r.cost, r.currency)])} />}
              >
                <View class="flex-row flex-wrap gap-3">
                  <RunTrend history={shown(x())} measure="requests" title={t("figure.requests")} format={n} onOpen={props.onOpen} />
                  <RunTrend history={shown(x())} measure="busyMs" title={t("profile.waiting")} format={offset} onOpen={props.onOpen} />
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
export function ProgramList(props: { onOpen: (path: string) => void }) {
  // a failure is said as one, not as "no program has run here yet"
  const programs = resource(
    () => sessionsTick(),
    async () => (await api.sessions.programs()) as ProgramSummary[],
  );
  return (
    <Await loading={programs.loading() && !programs()} error={programs.error()} retry={() => programs.refetch()}>
    <If when={programs()}>
      {(list) => (
        <If when={list().length > 0} else={<Muted class="px-2 text-small">{t("programs.none")}</Muted>}>
          <List testID="program-list" class="flex-1" each={list()} keyExtractor={(p) => p.program} estimatedItemSize={terminal() ? 2 : 52} scrollbar={false}>
            {(p) => (
              <ListRow
                testID="program-row"
                label={`${programOf(p().program) ?? scrubCommand(p().program)}, ${t("programs.runs", { n: p().runs })}${p().regressed ? `, ${t("programs.regressed")}` : ""}`}
                onPress={() => p().latest && props.onOpen(p().latest!)}
                class="flex-col items-stretch gap-1"
              >
                <View class="flex-row items-center gap-2">
                  <Text class="min-w-0 flex-1 text-body font-medium text-foreground line-clamp-1">{programOf(p().program) ?? scrubCommand(p().program)}</Text>
                  <If when={p().regressed}>
                    <View testID="program-regressed"><Badge label={t("programs.regressed")} tone="warn" /></View>
                  </If>
                </View>
                <View class="flex-row items-center gap-2">
                  {/* how often it runs, and what a month of it costs at that pace */}
                  <Text class="min-w-0 flex-1 text-caption text-muted-foreground line-clamp-1">{[t("programs.runs", { n: p().runs }), day(p().created), p().monthly ? t("programs.monthly", { cost: money(p().monthly!.cost, p().points[0]?.currency ?? "USD") }) : ""].filter(Boolean).join(" · ")}</Text>
                  <If when={p().points.length > 1}>
                    <Sparkline shape={p().points.map((x) => x.requests)} id={`program:${p().program}`} class="h-4 w-20" />
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
