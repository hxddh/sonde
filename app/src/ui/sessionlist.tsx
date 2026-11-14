// The sessions in the sidebar, newest first, by day: a row a session — its
// program (or the endpoint), what tells it from its neighbours, its time and
// requests, and a mark of its requests over its time.

import { computed, type ContextMenuEvent, type JSX, List, Text, View } from "nsh";
import { isAvailable } from "nsh/desktop";
import { programOf, scrubCommand } from "../command";
import { clock, day, hm } from "../format";
import { t } from "../i18n";
import type { SessionInfo } from "../types";
import { Dot, Heading, InWindowMenu, ListRow, terminal, TEXT } from "./kit";
import { Sparkline } from "./plots";

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
  // how many of a run's trials broke (none: nothing is said)
  const broke = (s: SessionInfo) => s.trials?.filter((x) => x.standing === "broke").length || null;
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
  // runs of one program are told apart: by the argument that differs, and where the
  // command is the same to the letter, by the second they started
  // (worked out once per list, each command read once: rows ask for it as they draw)
  const told = computed(() => {
    const by = new Map<string, { s: SessionInfo; args: string }[]>();
    for (const s of props.sessions) {
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
        <Text class={`min-w-0 shrink ${TEXT.body} line-clamp-1`}>{name(x().s)}</Text>
        <Text class={`min-w-0 flex-1 ${TEXT.codeQuiet} line-clamp-1`}>{told().get(x().s.path)?.args ?? args(x().s)}</Text>
        {/* the same command run again: its time at the row's end, to the second */}
        <If when={told().get(x().s.path)?.twin}>
          <Text testID="session-time" class={`shrink-0 tabular-nums ${TEXT.caption}`}>{clock(x().s.created, "s")}</Text>
        </If>
      </View>
      <View class="flex-row items-center gap-2">
        {/* (a twin's time is at its row's end, to the second: not said twice) */}
        <Text class={`min-w-0 flex-1 ${TEXT.meta} line-clamp-1`}>{[told().get(x().s.path)?.twin ? "" : hm(x().s.created), x().s.endpoint, t("sessions.requestsN", { n: x().s.requests })].filter(Boolean).join(" · ")}</Text>
        {/* tried under faults: how many of its trials broke, quietly — the run itself is not what broke
            (in full to a screen reader) */}
        <If when={broke(x().s)}>
          {(n) => (
            <Text testID="session-broke" class={`shrink-0 ${TEXT.meta}`} accessibilityLabel={t("trial.ofBrokeSaid", { n: n(), of: x().s.trials!.length })}>
              {t("trial.ofBroke", { n: n(), of: x().s.trials!.length })}
            </Text>
          )}
        </If>
        {/* (a row that says its trials broke keeps its line for its words: no mark beside them) */}
        <If when={x().s.shape?.length && !broke(x().s)}>
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
      {(r) => {
        // a recycled cell: what it shows follows the item it is given. A session's row reads the last session the
        // cell was given: as the cell turns into a day's heading (a row deleted above it), the row's words are
        // read once more before it goes, and must not read a heading as a session
        let held: { s: SessionInfo; i: number } | undefined;
        const session = () => {
          const v = r();
          if (!("day" in v)) held = v as { s: SessionInfo; i: number };
          return held!;
        };
        return (
        <If when={dayOf(r())} else={row(session)}>
          {(day) => (
            <View class="bg-sidebar px-2 pb-1 pt-2 terminal:bg-transparent">
              <Heading quiet>{day()}</Heading>
            </View>
          )}
        </If>
        );
      }}
    </List>
  );
}
