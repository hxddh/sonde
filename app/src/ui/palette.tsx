// The command palette: every place and action of the app, and the things
// people come back to — recent runs, endpoints, buckets, what the screen
// offers — behind one search field. ⌘K (Ctrl+K).

import { computed, effect, resource, setColorScheme, signal, Text, untracked, View } from "nsh";
import { Command, Dialog } from "nsh/base";
import { date } from "../format";
import { language, t } from "../i18n";
import { areaLabel, sectionLabel } from "../labels";
import { api } from "../native";
import { running, stop, stoppable } from "../run";
import { checkRequested } from "./updates";
import { shownCommands, ACTIVITY_TABS, chooseBucket, endpoint, endpoints, go, newRun, openExample, openRun, openRunFile, refreshSettings, SETTINGS_TABS, runsTick, settings, settingsTab, shellHeight, startDemo, STORE_TABS, useEndpoint } from "../store";
import type { BucketEntry, RunInfo } from "../types";
import { Icon, type IconName } from "./icon";
import { BREAK, CONTROL, EMPHASIS, Empty, glass, HIGHLIGHT, Kbd, keyLabel, POPUP, PALETTE_SCRIM, ROWS, SURFACE, Row, TEXT } from "./kit";
import { programOf, scrubCommand } from "./runview";

export const paletteOpen = signal(false);

interface Action {
  id: string;
  label: string;
  group: string;
  icon: IconName;
  hint?: string;
  shortcut?: string;
  keywords?: string[];
  checked?: boolean;
  /** its label is a command: in mono, after `$ `, as a command is everywhere */
  command?: boolean;
  /** its hint is a command */
  commandHint?: boolean;
  run: () => void;
}

/** What is typed in the palette: with a word, every run is searched. */
const typed = signal("");
/** Whether a word is typed: the list is rebuilt when that changes, not at every key. */
const searching = computed(() => typed().trim() !== "");

function actions(buckets: BucketEntry[]): Action[] {
  const list: Action[] = [];
  // the places: Runs; Buckets', Settings' and Activity's by their tabs, below (a row of a place alone would repeat one)
  list.push({ id: "area:runs", label: areaLabel("runs"), group: t("palette.go"), icon: "list-clock", shortcut: "mod+1", run: () => go("runs") });
  // what is at work (a program, a trial, a recording)
  if (stoppable(running())) list.push({ id: "run:stop", label: t("run.stop"), group: t("palette.actions"), icon: "square", run: stop });
  // what is on screen: a run's commands besides running it again, a Buckets section's, an object's
  shownCommands().forEach((e) =>
    e.list().forEach((c) => {
      if (!c.disabled) list.push({ id: `cmd:${c.id}`, label: c.label, group: e.group(), icon: c.icon, shortcut: c.shortcut, run: () => c.run() });
    }),
  );
  // the newest runs; every one once a word is typed (by its command, title or endpoint)
  runsTick();
  let runs: RunInfo[] = [];
  try {
    runs = api.runs.list().slice(0, searching() ? 1000 : 6);
  } catch {
    // none yet
  }
  runs.forEach((s) =>
    list.push({
      id: `run:${s.path}`,
      // by its title, else its program, and when it ran: the command itself is searched, not shown
      label: s.title || programOf(s.command) || s.name,
      group: t("palette.runs"),
      icon: "list-clock",
      keywords: [s.endpoint, s.name, ...(s.command ? [scrubCommand(s.command)] : [])],
      hint: `${date(s.created)} · ${s.endpoint} · ${t("run.bandRequests", { n: s.requests })}`,
      run: () => {
        openRun.set(s.path);
        go("runs");
      },
    }),
  );
  // where requests go; the built-in demo among them before it has started (chosen, it starts)
  if (!endpoints().some((e) => e.id === "demo")) list.push({ id: "endpoint:demo", label: t("endpoints.demoName"), group: t("palette.endpoints"), icon: "server", keywords: ["demo"], run: () => void startDemo() });
  endpoints().forEach((e) =>
    list.push({
      id: `endpoint:${e.id}`,
      label: e.name,
      group: t("palette.endpoints"),
      icon: "server",
      hint: e.url,
      checked: endpoint() === e.id,
      keywords: [e.vendorLabel, e.id],
      run: () => useEndpoint(e.id),
    }),
  );
  buckets.forEach((b) =>
    list.push({
      id: `bucket:${b.name}`,
      label: b.name,
      group: t("palette.buckets"),
      icon: "database",
      keywords: ["bucket"],
      run: () => {
        chooseBucket(b.name);
        go("browse");
      },
    }),
  );
  STORE_TABS.forEach((s, i) => list.push({ id: `store:${s}`, label: `${areaLabel("store")} › ${s === "browse" ? t("nav.objects") : sectionLabel(s)}`, group: t("palette.store"), icon: "database", shortcut: `mod+alt+${i + 1}`, run: () => go(s) }));
  // (a tab of Settings by itself: "settings" is also the place, which opens on the tab it had)
  SETTINGS_TABS.forEach((s) => list.push({ id: `settings:${s}`, label: `${areaLabel("settings")} › ${sectionLabel(s)}`, group: t("palette.settings"), icon: "settings", run: () => (settingsTab.set(s), go(s)) }));
  ACTIVITY_TABS.forEach((s) => list.push({ id: `activity:${s}`, label: `${areaLabel("activity")} › ${sectionLabel(s)}`, group: areaLabel("activity"), icon: "activity", run: () => go(s) }));
  // the example: its one door after the welcome (found by its English word in any language)
  list.push({ id: "example", label: t("example.open"), group: t("palette.actions"), icon: "scroll-text", keywords: ["example", "slatedb"], run: openExample });
  list.push({ id: "new-run", label: t("run.new"), group: t("palette.actions"), icon: "plus", shortcut: "mod+n", keywords: ["run", "command"], run: newRun });
  list.push({ id: "open-file", label: t("runs.openFile"), group: t("palette.actions"), icon: "folder-open", shortcut: "mod+o", keywords: ["sonde", "run", "import"], run: () => void openRunFile() });
  list.push({ id: "updates", label: t("updates.check"), group: t("palette.actions"), icon: "refresh-cw", keywords: ["upgrade", "version"], run: () => (checkRequested.update((n) => n + 1), settingsTab.set("settings"), go("settings")) });
  const theme = settings()?.theme ?? "system";
  (["light", "dark", "system"] as const).forEach((m) =>
    list.push({
      id: `theme:${m}`,
      label: t(`palette.${m}`),
      group: t("palette.theme"),
      icon: m === "light" ? "sun" : m === "dark" ? "moon" : "monitor",
      checked: theme === m,
      run: () => {
        const s = settings();
        if (!s) return;
        api.saveSettings({ ...s, theme: m });
        refreshSettings();
        setColorScheme(m);
      },
    }),
  );
  (["en", "zh"] as const).forEach((l) =>
    list.push({
      id: `lang:${l}`,
      label: l === "en" ? "English" : "中文",
      group: t("palette.language"),
      icon: "languages",
      checked: language() === l,
      run: () => {
        const s = settings();
        if (!s) return;
        api.saveSettings({ ...s, language: l });
        refreshSettings();
      },
    }),
  );
  return list;
}

/** A row's words with what is typed marked in them (bold, at its first place): why the row is there. */
function Marked(props: { text: string }) {
  const at = () => {
    const q = typed().trim().toLowerCase();
    return q ? props.text.toLowerCase().indexOf(q) : -1;
  };
  const n = () => typed().trim().length;
  return (
    <If when={at() >= 0} else={props.text}>
      <>
        {props.text.slice(0, at())}
        {/* emphasis: the part of the row that matches what is typed */}
        <Text class={EMPHASIS}>{props.text.slice(at(), at() + n())}</Text>
        {props.text.slice(at() + n())}
      </>
    </If>
  );
}

export function Palette() {
  const close = () => paletteOpen.set(false);
  // however it closes (a choice, Escape, ⌘K, a press outside), it opens again on an empty field
  effect(() => {
    if (!paletteOpen()) untracked(() => typed.set(""));
  });
  // the endpoint's buckets, fetched when the palette opens
  const buckets = resource(
    () => (paletteOpen() ? endpoint() : null),
    async (ep) => {
      try {
        return await api.buckets(ep);
      } catch {
        return [] as BucketEntry[];
      }
    },
  );
  // built once per change of what it lists (and only while open), not per row drawn or key typed
  const items = computed(() => (paletteOpen() ? actions(buckets() ?? []) : []));
  const byId = computed(() => new Map(items().map((a) => [a.id, a])));
  return (
    <Dialog
      open={paletteOpen()}
      onOpenChange={({ open }) => paletteOpen.set(open)}
      animation="fade"
      backdropClass={PALETTE_SCRIM}
      // the popup surface, its hairlines from edge to edge
      // (nearer the top of a short window: its list and its hint stay inside the least one)
      style={{ marginTop: shellHeight() < BREAK.short ? 40 : 96, padding: 0, ...glass }}
      class={`${SURFACE.palette} self-center overflow-hidden ${POPUP}`}
      testID="palette-dialog"
    >
      <Command
        testID="palette-command"
        autoFocus
        accessibilityLabel={t("palette.open")}
        placeholder={t("palette.placeholder")}
        items={items().map((a) => ({ id: a.id, label: a.label, group: a.group, shortcut: a.shortcut, keywords: a.keywords, checked: a.checked }))}
        onSelect={({ id }) => {
          const a = byId().get(id);
          close();
          a?.run();
        }}
        onCancel={close}
        query={typed()}
        onQueryChange={({ query }) => typed.set(query)}
        searchPrefix={<Icon name="search" />}
        // a field at the kit's height in a row with a row's padding (12 by 8): no height of its own
        searchBarClass={"flex-row items-center gap-2 border-b border-border px-3 py-2"}
        searchClass={`${CONTROL.field} flex-1 ${TEXT.body} placeholder:text-muted-foreground`}
        listClass={`${SURFACE.paletteList} p-1`}
        itemClass={ROWS.palette}
        groupClass={`px-2 pb-1 pt-2 ${TEXT.meta}`}
        separatorClass="my-1 h-px bg-border"
        renderItem={
          (entry) => {
                const a = byId().get(entry.id);
                return (
                  <View class={`${CONTROL.field} flex-row items-center gap-2`}>
                    <Icon name={a?.icon ?? "chevron-right"} tint={a?.checked ? "primary" : "muted-foreground"} />
                    <Text class={`min-w-0 shrink line-clamp-1 ${a?.command ? "font-mono text-small" : "text-body"} ${a?.checked ? "text-primary" : "text-popover-foreground"}`}>
                      <Marked text={a?.command ? `$ ${entry.label}` : entry.label} />
                    </Text>
                    <If when={a?.hint}>
                      <Text class={`min-w-0 flex-1 ${TEXT.meta} line-clamp-1 ${a?.commandHint ? "font-mono" : ""}`}>{a?.commandHint ? `$ ${a.hint}` : a!.hint!}</Text>
                    </If>
                    <If when={!a?.hint}>
                      <View class="flex-1" />
                    </If>
                    <If when={a?.checked}>
                      <Icon name="check" tint="primary" />
                    </If>
                    <If when={a?.shortcut}>
                      <Kbd keys={a!.shortcut!} />
                    </If>
                  </View>
                );
              }
        }
        empty={<Empty compact text={t("palette.nothing")} />}
        footer={
          (
            <Row>
              <Text class={TEXT.meta}>{`↑↓ ${t("palette.move")}`}</Text>
              <Text class={TEXT.meta}>{`${keyLabel("enter")} ${t("palette.choose")}`}</Text>
              <Text class={TEXT.meta}>{`${keyLabel("escape")} ${t("palette.close")}`}</Text>
            </Row>
          )
        }
      />
    </Dialog>
  );
}
