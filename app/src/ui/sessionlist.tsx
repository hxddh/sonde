// The sessions in the sidebar, newest first, by day: a row a session — its
// program (or the endpoint), what tells it from its neighbours, its time and
// requests, and a mark of its requests over its time.

import { type ContextMenuEvent, type JSX, List, Text, View } from "nsh";
import { isAvailable } from "nsh/desktop";
import { programOf, scrubCommand } from "../command";
import { clock, day, hm } from "../format";
import { t } from "../i18n";
import { worstStanding } from "../trials";
import type { SessionInfo } from "../types";
import { Badge, Dot, Heading, InWindowMenu, ListRow, terminal } from "./kit";
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
            <View class="bg-sidebar px-2 pb-1 pt-2 terminal:bg-transparent">
              <Heading quiet>{day()}</Heading>
            </View>
          )}
        </If>
      )}
    </List>
  );
}
