// The window's chrome: a title bar of the app's own (the toolbar), the
// sidebar with the places and the current place's list, and the
// application's menus.

import { computed, ErrorBoundary, type JSX, platform, signal, Storage, Text, View, type ViewRef } from "nsh";
import { Button as BaseButton, Select, Toolbar as ToolbarGroup } from "nsh/base";
import { isAvailable, type MenuItem, titleBar } from "nsh/desktop";
import { date } from "../format";
import { t } from "../i18n";
import { areaLabel, vendorName } from "../labels";
import { api } from "../native";
import { running, workName } from "../run";
import { type Area, area, shellWidth, endpoint, endpoints, go, newRun, openRun, openRunFile, settings, runsTick, sideList, useEndpoint } from "../store";
import type { RunInfo } from "../types";
import { Icon, type IconName } from "./icon";
import { scrubCommand } from "./runview";
import { BREAK, Button, CONTROL, DOT, FOCUS, glass, ICON, IconButton, Kbd, material, Notice, POPUP, POPUP_MOTION, popupMenu, PRESS, ROWS, Tip, TONE, WIDTH, SURFACE, TEXT } from "./kit";
import { ActivityButton, activityOpen, runningJobs } from "./activity";
import { runName } from "./manage";
import { paletteOpen } from "./palette";

const mac = platform.os === "macos";
/** The shell's shape on macOS and Linux (a Mac app's, libadwaita's split view): the sidebar the window's height, its
 * top strip holding its toggle (on macOS beside the window's buttons), the toolbar over the place only. Windows' toolbar
 * runs across the window over both. */
export const tallSidebar = mac || platform.os === "linux";

const wanted = signal(true);
const SIDEBAR_KEY = "sidebar.open";
void Storage.getItem(SIDEBAR_KEY).then((v) => v === "false" && wanted.set(false)).catch(() => {});
/** Whether the sidebar shows: the user's choice (⌘B), and only theirs. It holds
 * the places and the current place's list, so it never folds away by itself;
 * a pane with nothing to show without the list (a run or a bucket to choose)
 * offers to show it (`ShowSidebar`). */
export const sidebarOpen = () => wanted();
/** The toggle: it always toggles. */
export function toggleSidebar() {
  wanted.set(!wanted());
  void Storage.setItem(SIDEBAR_KEY, `${wanted()}`).catch(() => {});
}

/** Under an empty pane's words, while the sidebar is hidden: the list it waits on, a press away. */
export function ShowSidebar() {
  return (
    <If when={!sidebarOpen()}>
      <Button small variant="outline" icon="panel-left" label={t("sidebar.show")} shortcut="mod+b" onPress={toggleSidebar} testID="show-sidebar" />
    </If>
  );
}

export const AREA_ICON: Record<Area, IconName> = { runs: "list-clock", store: "database", activity: "activity", settings: "settings" };
export const AREA_KEY: Record<Area, string> = { runs: "mod+1", store: "mod+2", activity: "mod+shift+a", settings: "mod+," };

/** The newest runs, read again only as the list changes: a menu rebuilt for a place's check mark reads none. */
const recentRuns = computed<RunInfo[]>(() => {
  runsTick();
  try {
    return api.runs.list().slice(0, 10);
  } catch {
    // no runs directory yet
    return [];
  }
});

/** File › Open Recent: the newest runs, by the name the list gives them. */
function recentMenu(): MenuItem[] {
  const list = recentRuns();
  if (!list.length) return [{ label: t("timeline.empty"), disabled: true }];
  return list.map((s) => ({
    label: `${runName(s)} — ${date(s.created)}`,
    onSelect: () => {
      openRun.set(s.path);
      go("runs");
    },
  }));
}

/** The application's menus: macOS's menu bar; the toolbar's menu button elsewhere. */
export function appMenus(open: (a: Area) => void): { label: string; submenu: MenuItem[] }[] {
  const places: MenuItem[] = (["runs", "store"] as Area[]).map((a) => ({ label: areaLabel(a), shortcut: AREA_KEY[a], checked: area() === a, onSelect: () => open(a) }));
  return [
    {
      label: t("menu.file"),
      submenu: [
        { label: t("run.new"), shortcut: "mod+n", onSelect: newRun },
        { label: t("runs.openFile"), shortcut: "mod+o", onSelect: () => void openRunFile() },
        { label: t("runs.openRecent"), submenu: recentMenu() },
        { label: t("palette.open"), shortcut: "mod+k", onSelect: () => paletteOpen.set(!paletteOpen()) },
        ...(mac ? [] : [{ type: "separator" } as const, { label: t("nav.settings"), shortcut: "mod+,", onSelect: () => open("settings") }, { type: "separator" } as const, { role: "quit", label: platform.os === "windows" ? t("menu.exit") : t("menu.quit") } as const]),
      ],
    },
    { label: t("menu.edit"), submenu: [{ role: "undo" }, { role: "redo" }, { type: "separator" }, { role: "cut" }, { role: "copy" }, { role: "paste" }, { role: "selectAll" }] },
    { label: t("menu.view"), submenu: [...places, { type: "separator" }, { label: t("nav.activity"), shortcut: "mod+shift+a", onSelect: () => open("activity") }, { label: t("menu.sidebar"), shortcut: "mod+b", checked: sidebarOpen(), onSelect: toggleSidebar }, { role: "toggleFullScreen" }] },
    // (the system's own Window menu: macOS's; elsewhere the window's title bar has these)
    ...(mac ? [{ label: t("menu.window"), submenu: [{ role: "minimize" }, { role: "zoom" }] as MenuItem[] }] : []),
  ];
}

/** The endpoint everything talks to: a press anywhere on it opens the list (searchable), whose last row
 * manages them. */
const MANAGE = "\u0000manage";
export function EndpointSwitcher(props: { class?: string }) {
  const items = () => [...endpoints().map((e) => ({ value: e.id, title: e.name })), { value: MANAGE, title: t("endpoints.manage") }];
  const current = () => endpoints().find((e) => e.id === endpoint());
  return (
    <Select
      testID="endpoint-picker"
      accessibilityLabel={t("common.endpoint")}
      items={items()}
      value={endpoint()}
      searchable={endpoints().length > 5}
      onChange={({ value }) => {
        if (value === MANAGE) go("endpoints");
        else if (value) useEndpoint(value);
      }}
      offset={4}
      popupClass={`${SURFACE.switcherPopup} ${POPUP} ${POPUP_MOTION.class}`}
      popupStyle={glass}
      popupStartingClass={POPUP_MOTION.starting}
      popupEndingClass={POPUP_MOTION.ending}
      itemClass={`group ${ROWS.popup}`}
      empty={<Text class={`px-2 py-2 ${TEXT.note}`}>{t("endpoints.noneYet")}</Text>}
      trigger={
        <View class={`${CONTROL.trigger} ${FOCUS} ${props.class ?? (shellWidth() >= BREAK.toolbar || !current() ? WIDTH.switcher : WIDTH.switcherTight)}`}>
          <Icon name="server" />
          <Text class={`min-w-0 shrink ${TEXT.body} line-clamp-1`}>{current()?.name ?? t("common.chooseEndpoint")}</Text>
          <Icon name="chevrons-up-down" size={ICON.small} />
        </View>
      }
      renderItem={(item) => {
        if (item.value === MANAGE)
          return (
            <View testID="manage-endpoints" class="flex-row items-center gap-2">
              <Icon name="settings" size={ICON.small} />
              <Text class={TEXT.note}>{item.title}</Text>
            </View>
          );
        const e = endpoints().find((x) => x.id === item.value);
        return (
          <View class="flex-row items-center gap-2">
            <View class="min-w-0 flex-1">
              <Text class="text-body text-popover-foreground line-clamp-1">{item.title}</Text>
              <Text class={`${TEXT.codeQuiet} line-clamp-1`}>{e?.url ?? ""}</Text>
            </View>
            <Text class={TEXT.meta}>{e ? vendorName(e) : ""}</Text>
          </View>
        );
      }}
    />
  );
}

/** The program running now: a pill that goes back to it. */
function RunningPill() {
  return (
    <If when={running()}>
      {(r) => (
        <BaseButton
          testID="running-pill"
          accessibilityLabel={t("run.running")}
          onPress={() => {
            if (r().path) openRun.set(r().path);
            go("runs");
          }}
          tooltip={<Tip text={scrubCommand(r().command) || workName(r())} />}
          class={`h-7 ${WIDTH.pill} flex-row items-center gap-2 rounded-(--radius) border border-primary/40 bg-primary/10 px-2 hover:bg-primary/15 ${FOCUS}`}
        >
          <View class={`${DOT} ${TONE.live.dot} motion-safe:animate-pulse`} />
          <Text class="text-small text-primary line-clamp-1">{workName(r())}</Text>
        </BaseButton>
      )}
    </If>
  );
}

/** The search field's stand-in: opens the command palette; an icon alone
 * in a narrow window. */
function SearchButton() {
  return (
    <If when={shellWidth() >= BREAK.toolbar} else={<IconButton testID="palette" icon="search" label={t("palette.open")} shortcut="mod+k" onPress={() => paletteOpen.set(true)} />}>
    <BaseButton
      testID="palette"
      accessibilityLabel={t("palette.open")}
      onPress={() => paletteOpen.set(true)}
      class={`${CONTROL.trigger} ${WIDTH.search} ${FOCUS}`}
    >
      <Icon name="search" />
      <Text class={`flex-1 ${TEXT.bodyQuiet}`}>{t("palette.open")}</Text>
      <Kbd keys="mod+k" />
    </BaseButton>
    </If>
  );
}

/** The title bar's height: the system's, where it says. */
const titleBarHeight = () => (isAvailable ? titleBar().height || 44 : 44);

/** The sidebar's toggle: it always toggles, and it is a command, not a state (no pressed look while the sidebar
 * shows, as the platforms' own toggles have none). */
function SidebarToggle() {
  return <IconButton icon="panel-left" label={t("menu.sidebar")} shortcut="mod+b" onPress={toggleSidebar} testID="toggle-sidebar" />;
}

/** The window's title bar, drawn by the app: drags the window; the system's controls keep their place. */
export function Toolbar(props: { open: (a: Area) => void; extra?: JSX.Element }) {
  let menuButton: ViewRef | undefined;
  // (the sidebar's own strip holds its toggle, and on macOS the window's buttons, while it shows: the toolbar, over the
  // place, starts clear)
  const inSidebar = () => tallSidebar && sidebarOpen();
  const left = () => (isAvailable && !inSidebar() ? titleBar().left : 0);
  return (
    <View
      testID="toolbar"
      desktop={{ windowDrag: true }}
      class="flex-row items-center gap-2 border-b border-border bg-toolbar"
      style={{ height: titleBarHeight(), flexShrink: 0, paddingLeft: left() + 10, paddingRight: (isAvailable ? titleBar().right : 0) + 10 }}
    >
      <If when={!inSidebar()}>
        <SidebarToggle />
      </If>
      <View class="min-w-0 flex-1" />
      {/* one stop of Tab, the arrows between its commands */}
      <ToolbarGroup accessibilityLabel={t("toolbar.label")} class="flex-row items-center gap-2">
      <RunningPill />
      <EndpointSwitcher />
      {/* sonde at work (a scan, a trial): its count, a press away from Activity; nothing while it rests */}
      <If when={runningJobs() > 0}>
        <ActivityButton onPress={() => (activityOpen() ? activityOpen.set(false) : go("activity"))} />
      </If>
      <SearchButton />
      <If when={!mac}>
        <IconButton
          testID="app-menu"
          icon={platform.os === "linux" ? "menu" : "ellipsis"}
          label={t("menu.menu")}
          ref={(r) => (menuButton = r)}
          onPress={() => void popupMenu(appMenus(props.open).map((m) => ({ label: m.label, submenu: m.submenu })), menuButton)}
        />
      </If>
      </ToolbarGroup>
    </View>
  );
}

function Place(props: { area: Area; open: (a: Area) => void }) {
  const on = () => area() === props.area;
  // Runs pressed in Runs: a new run, its command bar (as ⌘N), said in its tip
  const anew = () => on() && props.area === "runs";
  return (
    <BaseButton
      testID={`nav-${props.area}`}
      accessibilityLabel={areaLabel(props.area)}
      selected={on()}
      onPress={() => (anew() ? newRun() : props.open(props.area))}
      tooltip={<Tip text={anew() ? t("run.new") : areaLabel(props.area)} shortcut={anew() ? "mod+n" : AREA_KEY[props.area]} />}
      class={`h-8 flex-row items-center gap-2 rounded-(--radius) px-2 hover:bg-sidebar-accent/50 selected:bg-sidebar-accent ${PRESS.surface} ${FOCUS}`}
    >
      <Icon name={AREA_ICON[props.area]} tint={on() ? "sidebar-accent-foreground" : "muted-foreground"} />
      <Text class={on() ? "flex-1 text-body text-sidebar-accent-foreground" : "flex-1 text-body text-sidebar-foreground"}>{areaLabel(props.area)}</Text>
      <If when={props.area === "runs" && running()}>
        <View class={`${DOT} ${TONE.live.dot}`} />
      </If>
    </BaseButton>
  );
}


/** Whether the window's own material (macOS's sidebar backdrop) shows through
 * the shell: it follows the system's appearance, so only while the app does
 * too; a light or dark chosen in Settings gets opaque surfaces instead. */
export const windowMaterial = () => material && (settings()?.theme ?? "system") === "system";

export function Sidebar(props: { open: (a: Area) => void }) {
  return (
    <If when={sidebarOpen()}>
      <View testID="sidebar" class={`flex-1 ${windowMaterial() ? "bg-sidebar/40" : "bg-sidebar"} ${tallSidebar ? "" : "pt-3"}`}>
        {/* the window's height (macOS, Linux): the title bar's room at its top holds the sidebar's toggle, on macOS
            after the window's buttons (it drags the window, as the toolbar does) */}
        {tallSidebar ? (
          <View testID="sidebar-strip" desktop={{ windowDrag: true }} class="flex-row items-center" style={{ height: titleBarHeight(), flexShrink: 0, paddingLeft: (isAvailable ? titleBar().left : 0) + 10 }}>
            <SidebarToggle />
          </View>
        ) : null}
        {/* the places: one stop of Tab, ↑ and ↓ between them */}
        <ToolbarGroup axis="vertical" accessibilityLabel={t("sidebar.places")} class="shrink-0 gap-1 px-2">
          <Place area="runs" open={props.open} />
          <Place area="store" open={props.open} />
        </ToolbarGroup>
        {/* under them, the place's own list: its runs, its buckets */}
        <Each of={sideList() ? [sideList()!] : []} key={(l) => l.key}>
          {(l) => (
            <View class="min-h-0 flex-1 pt-3">
              <View class="mx-3 h-px shrink-0 bg-border" />
              {/* a list that fails to draw says why, here: the place's own boundary does not hold it */}
              <View class="min-h-0 flex-1">
                <ErrorBoundary
                  fallback={(error, reset) => (
                    <View class="p-3">
                      <Notice tone="error" text={error instanceof Error ? error.message : `${error}`} testID="sidebar-error">
                        <Button small variant="outline" label={t("error.retry")} onPress={reset} />
                      </Notice>
                    </View>
                  )}
                >
                  {l().render()}
                </ErrorBoundary>
              </View>
            </View>
          )}
        </Each>
      </View>
    </If>
  );
}
