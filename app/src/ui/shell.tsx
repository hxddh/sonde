// The window's chrome on the desktops: a title bar of the app's own (the
// toolbar), the sidebar with the places and the current place's list, and the
// application's menus. A terminal has none of this (App.tsx's TerminalShell).

import { computed, ErrorBoundary, type JSX, platform, signal, Storage, Text, View, type ViewRef } from "nsh";
import { Button as BaseButton, Select, Toolbar as ToolbarGroup } from "nsh/base";
import { isAvailable, type MenuItem, titleBar } from "nsh/desktop";
import { date } from "../format";
import { t } from "../i18n";
import { areaLabel, sectionLabel, vendorName } from "../labels";
import { api } from "../native";
import { running } from "../run";
import { type Area, area, bucket, shellWidth, crumb, crumbBack, endpoint, endpoints, go, notify, openRun, openRunFile, section, settings, runsTick, sideList, storeTab, useEndpoint } from "../store";
import type { RunInfo } from "../types";
import { Icon, type IconName } from "./icon";
import { programOf, scrubCommand } from "./runview";
import { BREAK, Button, CONTROL, Crumb, DOT, FOCUS, glass, ICON, IconButton, Kbd, material, Notice, POPUP, POPUP_MOTION, popupMenu, PRESS, ROWS, Strong, terminal, Tip, TONE, WIDTH, SURFACE, TEXT } from "./kit";
import { ActivityButton, activityOpen } from "./activity";
import { runName } from "./manage";
import { paletteOpen } from "./palette";

const mac = platform.os === "macos";

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
    <If when={!sidebarOpen() && !terminal()}>
      <Button small variant="outline" icon="panel-left" label={t("sidebar.show")} shortcut="mod+b" onPress={toggleSidebar} testID="show-sidebar" />
    </If>
  );
}

export const AREA_ICON: Record<Area, IconName> = { run: "square-terminal", runs: "list-clock", store: "database", activity: "activity", settings: "settings" };
export const AREA_KEY: Record<Area, string> = { run: "mod+1", runs: "mod+2", store: "mod+3", activity: "mod+shift+a", settings: "mod+," };

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
  const places: MenuItem[] = (["run", "runs", "store"] as Area[]).map((a) => ({ label: areaLabel(a), shortcut: AREA_KEY[a], checked: area() === a, onSelect: () => open(a) }));
  return [
    {
      label: t("menu.file"),
      submenu: [
        { label: t("runs.openFile"), shortcut: "mod+o", onSelect: () => void openRunFile() },
        { label: t("runs.openRecent"), submenu: recentMenu() },
        { label: t("palette.open"), shortcut: "mod+k", onSelect: () => paletteOpen.set(true) },
        ...(mac ? [] : [{ type: "separator" } as const, { label: t("nav.settings"), shortcut: "mod+,", onSelect: () => open("settings") }, { type: "separator" } as const, { role: "quit", label: platform.os === "windows" ? t("menu.exit") : t("menu.quit") } as const]),
      ],
    },
    { label: t("menu.edit"), submenu: [{ role: "undo" }, { role: "redo" }, { type: "separator" }, { role: "cut" }, { role: "copy" }, { role: "paste" }, { role: "selectAll" }] },
    { label: t("menu.view"), submenu: [...places, { type: "separator" }, { label: t("nav.activity"), shortcut: "mod+shift+a", onSelect: () => open("activity") }, { label: t("menu.sidebar"), shortcut: "mod+b", checked: sidebarOpen(), onSelect: toggleSidebar }, { role: "toggleFullScreen" }] },
    // (the system's own Window menu: macOS's; elsewhere the window's title bar has these)
    ...(mac ? [{ label: t("menu.window"), submenu: [{ role: "minimize" }, { role: "zoom" }] as MenuItem[] }] : []),
  ];
}

/** Where the window is: the place, then what it shows (a run, a bucket, a tab). */
function Breadcrumb() {
  const detail = () => {
    // Buckets says its section and the bucket it looks at in the section's own header: the title bar does not repeat them
    if (area() === "store") return null;
    if (area() === "settings") return sectionLabel(section());
    return crumb();
  };
  return (
    <View class="min-w-0 flex-row items-center gap-2">
      {/* the place, as a link back to its list while it shows one of them (a run): a press there lets it go */}
      <If when={detail() ? crumbBack() : null} else={<Strong>{areaLabel(area())}</Strong>}>
        {(back) => (
          <Crumb testID="crumb-place" href={`sonde://${area()}`} label={areaLabel(area())} onOpen={() => back()()}>
            <Strong>{areaLabel(area())}</Strong>
          </Crumb>
        )}
      </If>
      <If when={detail()}>
        {(d) => (
          <>
            <Icon name="chevron-right" size={ICON.small} />
            <Text class={`min-w-0 shrink ${TEXT.bodyQuiet} line-clamp-1`}>{d()}</Text>
          </>
        )}
      </If>
    </View>
  );
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
          onPress={() => go("run")}
          tooltip={<Tip text={scrubCommand(r().command)} />}
          class={`h-7 ${WIDTH.pill} flex-row items-center gap-2 rounded-(--radius) border border-primary/40 bg-primary/10 px-2 hover:bg-primary/15 ${FOCUS}`}
        >
          <View class={`${DOT} ${TONE.live.dot} motion-safe:animate-pulse`} />
          <Text class="text-small text-primary line-clamp-1">{programOf(r().command) ?? ""}</Text>
        </BaseButton>
      )}
    </If>
  );
}

/** The search field's stand-in: opens the command palette; an icon alone
 * in a narrow window, so the breadcrumb keeps its room. */
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

/** The window's title bar, drawn by the app: drags the window; the system's controls keep their place. */
export function Toolbar(props: { open: (a: Area) => void; extra?: JSX.Element }) {
  let menuButton: ViewRef | undefined;
  // (on macOS the window's buttons stand in the sidebar while it shows: the toolbar, over the place, starts clear)
  const left = () => (isAvailable && !(mac && sidebarOpen()) ? titleBar().left : 0);
  return (
    <View
      testID="toolbar"
      desktop={{ windowDrag: true }}
      class="flex-row items-center gap-2 border-b border-border bg-toolbar"
      style={{ height: titleBarHeight(), flexShrink: 0, paddingLeft: left() + 10, paddingRight: (isAvailable ? titleBar().right : 0) + 10 }}
    >
      <IconButton icon="panel-left" label={t("menu.sidebar")} shortcut="mod+b" onPress={toggleSidebar} selected={sidebarOpen()} testID="toggle-sidebar" />
      <View class="ml-1 min-w-0 flex-1">
        <Breadcrumb />
      </View>
      {/* one stop of Tab, the arrows between its commands */}
      <ToolbarGroup accessibilityLabel={t("toolbar.label")} class="flex-row items-center gap-2">
      <RunningPill />
      <EndpointSwitcher />
      <ActivityButton onPress={() => (activityOpen() ? activityOpen.set(false) : go("activity"))} />
      <SearchButton />
      <If when={!mac}>
        <IconButton
          testID="app-menu"
          icon="ellipsis"
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
  return (
    <BaseButton
      testID={`nav-${props.area}`}
      accessibilityLabel={areaLabel(props.area)}
      selected={on()}
      onPress={() => props.open(props.area)}
      tooltip={<Tip text={areaLabel(props.area)} shortcut={AREA_KEY[props.area]} />}
      class={`h-8 flex-row items-center gap-2 rounded-(--radius) px-2 hover:bg-sidebar-accent/50 selected:bg-sidebar-accent ${PRESS.surface} ${FOCUS}`}
    >
      <Icon name={AREA_ICON[props.area]} tint={on() ? "sidebar-accent-foreground" : "muted-foreground"} />
      <Text class={on() ? "flex-1 text-body text-sidebar-accent-foreground" : "flex-1 text-body text-sidebar-foreground"}>{areaLabel(props.area)}</Text>
      <If when={props.area === "run" && running()}>
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
      <View testID="sidebar" class={`flex-1 ${windowMaterial() ? "bg-sidebar/40" : "bg-sidebar"} ${mac ? "" : "pt-3"}`}>
        {/* macOS: the window's height, the title bar's room at its top holding the window's buttons (it drags the
            window, as the toolbar does) */}
        {mac ? <View desktop={{ windowDrag: true }} style={{ height: titleBarHeight(), flexShrink: 0 }} /> : null}
        {/* the places: one stop of Tab, ↑ and ↓ between them */}
        <ToolbarGroup axis="vertical" accessibilityLabel={t("sidebar.places")} class="shrink-0 gap-1 px-2">
          <Place area="run" open={props.open} />
          <Place area="runs" open={props.open} />
          <Place area="store" open={props.open} />
        </ToolbarGroup>
        {/* under them, the place's own list: its runs, its buckets, the commands run before */}
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
