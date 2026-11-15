// The runs in the sidebar, newest first, by day: a row a run — its
// program (or the endpoint), what tells it from its neighbours and its time;
// then where it went and its requests, and how it went (its trials, else a
// mark of its requests over its time).

import { computed, type ContextMenuEvent, effect, type JSX, List, type ListRef, Text, untracked, View } from "nsh";
import { isAvailable } from "nsh/desktop";
import { programOf, scrubCommand } from "../command";
import { clock, day, hm } from "../format";
import { t } from "../i18n";
import type { RunInfo } from "../types";
import { Dot, Heading, InWindowMenu, ListRow, terminal, TEXT, TONE } from "./kit";
import { Sparkline } from "./plots";

const DAY = 86400000;
/** A day's heading over the runs, in points (8 above its caption's 16 pt line, 4 under it, its hairline): what a
 * run brought to the top keeps clear of it. */
const DAY_HEAD = 29;
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
 * others shows first: where they all begin alike past a few characters, that
 * beginning is said once as `…` and each starts at what differs, from the
 * edge of its word or of its path's part (`… --table orders`, `… users.csv`),
 * so a column that cuts at its end cuts what they share, never what differs.
 * Alone, or already different at their start, they stay whole. */
export function tellApart(args: string[]): string[] {
  const distinct = [...new Set(args)];
  if (distinct.length < 2) return args;
  let common = distinct[0];
  for (const a of distinct) while (!a.startsWith(common)) common = common.slice(0, -1);
  // back to an edge: a word's, or a part of a path or of an option's value
  const edge = Math.max(...[" ", "/", "=", ":", ","].map((c) => common.lastIndexOf(c)));
  common = common.slice(0, edge + 1);
  // a short beginning is worth keeping whole
  if (common.trim().length < 12) return args;
  return args.map((a) => `… ${a.slice(common.length)}`);
}

/** What sent a run's requests, before where they went: its clients, the dominant first ("Lance, boto3"); "" for
 * none known. */
const clientsOf = (s: RunInfo) => (s.clients ?? []).slice(0, 2).join(", ");

/** The runs, newest first, by day: the program (or the endpoint), its time and requests. */
export function RunList(props: {
  runs: RunInfo[];
  selected: string | null;
  /** The runs past their program's baseline (each program's newest, when it is): marked on their rows. */
  past?: Set<string> | null;
  onSelect: (p: string) => void;
  onContextMenu?: (s: RunInfo, at?: { x: number; y: number }) => void;
  /** The context menu's commands, offered to screen readers on each row. */
  actions?: (s: RunInfo) => { name: string; label: string; run: () => void }[];
}) {
  // one list, the days as headers that stay in view while their runs scroll
  type Row = { day: string } | { s: RunInfo; i: number };
  const rows = (): Row[] => {
    const out: Row[] = [];
    let last = "";
    props.runs.forEach((s, i) => {
      const day = dayLabel(s.created);
      if (day !== last) out.push({ day: (last = day) });
      out.push({ s, i });
    });
    return out;
  };
  const dayOf = (r: Row) => ("day" in r ? r.day : null);
  const headers = () => rows().flatMap((r, i) => ("day" in r ? [i] : []));
  // how many of a run's trials broke (none: nothing is said)
  const broke = (s: RunInfo) => s.trials?.filter((x) => x.standing === "broke").length || null;
  const name = (s: RunInfo) => s.title || programOf(s.command) || s.endpoint;
  // the rest of the command after its program, scrubbed; the whole of it when
  // the program is not its first word (`cd etl && python load.py`)
  const args = (s: RunInfo) => {
    const p = programOf(s.command);
    if (s.title || !s.command || !p || p.endsWith("…")) return "";
    const c = scrubCommand(s.command);
    const [first, ...rest] = c.split(" ");
    if (first.replace(/^['"]|['"]$/g, "").split(/[\\/]/).pop() === p) return rest.join(" ");
    return c === p ? "" : c;
  };
  // runs of one program are told apart: by the argument that differs, and where the
  // command is the same to the letter, by the second they started
  // (worked out once per list, each command read once: rows ask for it as they draw)
  const told = computed(() => {
    const by = new Map<string, { s: RunInfo; args: string }[]>();
    for (const s of props.runs) {
      const n = name(s);
      by.set(n, [...(by.get(n) ?? []), { s, args: args(s) }]);
    }
    const out = new Map<string, { args: string; twin: boolean }>();
    for (const group of by.values()) {
      const shown = tellApart(group.map((x) => x.args));
      const times = new Map<string, number>();
      for (const x of group) times.set(x.args, (times.get(x.args) ?? 0) + 1);
      group.forEach((x, i) => out.set(x.s.path, { args: shown[i], twin: (times.get(x.args) ?? 0) > 1 }));
    }
    return out;
  });
  // the days whose rows say seconds: every row of a day where one needs them (a command run twice in a minute),
  // so a day's times read in one column, alike
  const toSecond = computed(() => new Set(props.runs.filter((s) => told().get(s.path)?.twin).map((s) => dayLabel(s.created))));
  // a run chosen from elsewhere (the palette, a file opened, a run just ended) and not wholly in view is brought
  // to the list's top, just under the day that stays there (its height kept clear), so the day never cuts it; one
  // pressed in the list, or already in view, stays where it is
  let list: ListRef | undefined;
  let pressed = false;
  let whole = new Set<number>();
  effect(() => {
    const sel = props.selected;
    if (pressed) {
      pressed = false;
      return;
    }
    if (!sel) return;
    const at = untracked(rows).findIndex((r) => !("day" in r) && r.s.path === sel);
    if (at >= 0 && !whole.has(at)) setTimeout(() => list?.scrollToIndex({ index: at, viewPosition: 0, viewOffset: DAY_HEAD, animated: false }), 0);
  });
  const select = (path: string) => {
    pressed = true;
    props.onSelect(path);
  };
  // where the system has no menus to pop up (a terminal, the web): the
  // same commands in Native Shell's own, on a right press or Shift+F10
  const inWindowMenu = (x: () => { s: RunInfo; i: number }, body: JSX.Element) => (
    <InWindowMenu enabled={!props.onContextMenu && !!props.actions && !isAvailable} actions={() => (props.actions?.(x().s) ?? []).filter((a) => a.name !== "window")}>
      {body}
    </InWindowMenu>
  );
  const row = (x: () => { s: RunInfo; i: number }) => inWindowMenu(x,
    <View
      onContextMenu={props.onContextMenu ? (e: ContextMenuEvent) => props.onContextMenu!(x().s, e.pagePosition) : undefined}
      accessibilityActions={props.actions?.(x().s).map(({ name, label }) => ({ name, label }))}
      onAccessibilityAction={props.actions ? ({ actionName }) => props.actions!(x().s).find((a) => a.name === actionName)?.run() : undefined}
    >
    <ListRow
      lines={2}
      testID={`run-${x().i}`}
      label={x().s.title ?? (x().s.command ? scrubCommand(x().s.command!) : x().s.name)}
      selected={props.selected === x().s.path}
      onPress={() => select(x().s.path)}
      class="mb-1"
    >
      {/* what ran, what tells it apart and when; then where it went, how much it did, and how it went — its trials'
          verdict, else its requests over its time, never both */}
      <View class="flex-row items-center gap-2">
        <If when={x().s.live}>
          <Dot tone="live" label={t("timeline.live")} />
        </If>
        <Text class={`min-w-0 shrink ${TEXT.body} line-clamp-1`}>{name(x().s)}</Text>
        {/* what tells it from its neighbours (the same command again tells nothing apart by its words: its time does,
            to the second) */}
        <Text class={`min-w-0 flex-1 ${TEXT.codeQuiet} line-clamp-1`}>{told().get(x().s.path)?.twin ? "" : (told().get(x().s.path)?.args ?? args(x().s))}</Text>
        {/* the same command run again: its time to the second, and so every time of its day */}
        <Text testID={told().get(x().s.path)?.twin ? "run-time" : undefined} class={`shrink-0 tabular-nums ${TEXT.meta}`}>{toSecond().has(dayLabel(x().s.created)) ? clock(x().s.created, "s") : hm(x().s.created)}</Text>
      </View>
      <View class="flex-row items-center gap-2">
        {/* where it went and how much it did, always */}
        {/* the count never cut: what sent it and where give way first */}
        <View class="min-w-0 flex-1 flex-row items-center gap-1">
          <Text class={`min-w-0 shrink ${TEXT.meta} line-clamp-1`}>{[clientsOf(x().s), x().s.endpoint].filter(Boolean).join(" · ")}</Text>
          <Text testID="run-requests" class={`shrink-0 ${TEXT.meta}`}>{`${[clientsOf(x().s), x().s.endpoint].some(Boolean) ? "· " : ""}${t("runs.requestsN", { n: x().s.requests })}`}</Text>
        </View>
        {/* its program's newest run, past the baseline its runs are held to */}
        <If when={props.past?.has(x().s.path)}>
          <Dot tone="warn" label={t("runs.pastBaseline")} />
          <Text testID="run-past" class={`shrink-0 ${TEXT.captionSize} ${TONE.warn.text}`}>{t("runs.pastBaseline")}</Text>
        </If>
        {/* tried under faults: how many of its trials broke, quietly — the run itself is not what broke
            (in full to a screen reader) */}
        <If when={broke(x().s)}>
          {(n) => (
            <Text testID="run-broke" class={`shrink-0 ${TEXT.meta}`} accessibilityLabel={t("trial.ofBrokeSaid", { n: n(), of: x().s.trials!.length })}>
              {t("trial.ofBroke", { n: n(), of: x().s.trials!.length })}
            </Text>
          )}
        </If>
        <If when={x().s.shape?.length && !broke(x().s)}>
          <Sparkline shape={x().s.shape!} id={x().s.path} />
        </If>
      </View>
    </ListRow>
    </View>,
  );
  return (
    <List
      ref={(r: ListRef) => (list = r)}
      testID="run-list"
      class="flex-1"
      each={rows()}
      stickyHeaderIndices={headers()}
      estimatedItemSize={46}
      keyExtractor={(r) => ("day" in r ? `day:${r.day}` : r.s.path)}
      scrollbar={false}
      viewabilityConfig={{ itemVisibleThreshold: 1 }}
      onViewableItemsChange={({ viewableItems }) => (whole = new Set(viewableItems.map((v) => v.index)))}
    >
      {(r) => {
        // a recycled cell: what it shows follows the item it is given. A run's row reads the last run the
        // cell was given: as the cell turns into a day's heading (a row deleted above it), the row's words are
        // read once more before it goes, and must not read a heading as a run
        let held: { s: RunInfo; i: number } | undefined;
        const run = () => {
          const v = r();
          if (!("day" in v)) held = v as { s: RunInfo; i: number };
          return held!;
        };
        return (
        <If when={dayOf(r())} else={row(run)}>
          {(day) => (
            // the sidebar's own fill and a hairline under it: what scrolls under the day goes under it whole
            <View class="border-b border-border bg-sidebar px-2 pb-1 pt-2 terminal:border-0 terminal:bg-transparent">
              <Heading quiet>{day()}</Heading>
            </View>
          )}
        </If>
        );
      }}
    </List>
  );
}
