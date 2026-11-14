// The window's chrome on the desktops: a title bar of the app's own (the
// toolbar), the sidebar with the places and the current place's list, and the
// application's menus. A terminal has none of this (App.tsx's TerminalShell).

import { ErrorBoundary, type JSX, platform, signal, Storage, Text, View, type ViewRef } from "nsh";
import { Button as BaseButton, Combobox, Toolbar as ToolbarGroup } from "nsh/base";
import { isAvailable, type MenuItem, titleBar } from "nsh/desktop";
import { date } from "../format";
import { t } from "../i18n";
import { areaLabel, sectionLabel, vendorName } from "../labels";
import { api } from "../native";
import { running } from "../run";
import { type Area, area, bucket, shellWidth, crumb, endpoint, endpoints, go, openSession, openSessionFile, section, settings, sessionsTick, sideList, storeTab, useEndpoint } from "../store";
import type { SessionInfo } from "../types";
import { Icon, type IconName } from "./icon";
import { programOf, scrubCommand } from "./sessionview";
import { BREAK, Button, CONTROL, DOT, FOCUS, FOCUS_WITHIN, glass, ICON, IconButton, Kbd, material, Notice, POPUP, POPUP_MOTION, popupMenu, PRESS, ROWS, Strong, Tip, TONE, WIDTH, SURFACE, ListRow, TEXT } from "./kit";
import { ActivityButton, activityOpen } from "./activity";
import { sessionName } from "./manage";
import { paletteOpen } from "./palette";

const mac = platform.os === "macos";

const wanted = signal(true);
const SIDEBAR_KEY = "sidebar.open";
void Storage.getItem(SIDEBAR_KEY).then((v) => v === "false" && wanted.set(false)).catch(() => {});
/** Whether the sidebar shows: the user's choice (⌘B). It holds the places
 * and the current place's list, so it never folds away by itself — and it
 * comes back while the place has nothing to show without its list (a bucket
 * or a session to choose, a listing that failed). */
export const sidebarOpen = () => wanted() || sidebarHeld();
/** The place holds the sidebar open: it has nothing to show without its list.
 * The toggle does nothing then, and says why. */
export const sidebarHeld = () => !!sideList()?.needed?.();
export function toggleSidebar() {
  if (sidebarHeld()) return;
  wanted.set(!wanted());
  void Storage.setItem(SIDEBAR_KEY, `${wanted()}`).catch(() => {});
}

export const AREA_ICON: Record<Area, IconName> = { run: "square-terminal", sessions: "list-clock", store: "database", activity: "activity", settings: "settings" };
export const AREA_KEY: Record<Area, string> = { run: "mod+1", sessions: "mod+2", store: "mod+3", activity: "mod+shift+a", settings: "mod+," };

/** File › Open Recent: the newest sessions, by the name the list gives them. */
function recentMenu(): MenuItem[] {
  sessionsTick();
  let list: SessionInfo[] = [];
  try {
    list = api.sessions.list().slice(0, 10);
  } catch {
    // no sessions directory yet
  }
  if (!list.length) return [{ label: t("timeline.empty"), disabled: true }];
  return list.map((s) => ({
    label: `${sessionName(s)} — ${date(s.created)}`,
    onSelect: () => {
      openSession.set(s.path);
      go("sessions");
    },
  }));
}

/** The application's menus: macOS's menu bar; the toolbar's menu button elsewhere. */
export function appMenus(open: (a: Area) => void): { label: string; submenu: MenuItem[] }[] {
  const places: MenuItem[] = (["run", "sessions", "store"] as Area[]).map((a) => ({ label: areaLabel(a), shortcut: AREA_KEY[a], checked: area() === a, onSelect: () => open(a) }));
  return [
    {
      label: t("menu.file"),
      submenu: [
        { label: t("run.start"), shortcut: "mod+r", onSelect: () => open("run") },
        { label: t("sessions.openFile"), shortcut: "mod+o", onSelect: () => void openSessionFile() },
        { label: t("sessions.openRecent"), submenu: recentMenu() },
        { label: t("palette.open"), shortcut: "mod+k", onSelect: () => paletteOpen.set(true) },
        ...(mac ? [] : [{ type: "separator" } as const, { label: t("nav.settings"), shortcut: "mod+,", onSelect: () => open("settings") }, { type: "separator" } as const, { role: "quit", label: t("menu.quit") } as const]),
      ],
    },
    { label: t("menu.edit"), submenu: [{ role: "undo" }, { role: "redo" }, { type: "separator" }, { role: "cut" }, { role: "copy" }, { role: "paste" }, { role: "selectAll" }] },
    { label: t("menu.view"), submenu: [...places, { type: "separator" }, { label: t("nav.activity"), shortcut: "mod+shift+a", onSelect: () => open("activity") }, { label: t("menu.sidebar"), shortcut: "mod+b", checked: sidebarOpen(), disabled: sidebarHeld(), onSelect: toggleSidebar }, { role: "toggleFullScreen" }] },
    { label: t("menu.window"), submenu: [{ role: "minimize" }, { role: "zoom" }] },
  ];
}

/** Where the window is: the place, then what it shows (a session, a bucket, a tab). */
function Breadcrumb() {
  const detail = () => {
    if (area() === "store") return [bucket(), storeTab() === "browse" ? t("nav.objects") : sectionLabel(storeTab())].filter(Boolean).join(" · ") || null;
    if (area() === "settings") return sectionLabel(section());
    return crumb();
  };
  return (
    <View class="min-w-0 flex-row items-center gap-2">
      <Strong>{areaLabel(area())}</Strong>
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

/** The endpoint everything talks to: searchable, with a way to manage them. */
export function EndpointSwitcher(props: { class?: string }) {
  const items = () => endpoints().map((e) => ({ value: e.id, title: e.name }));
  const current = () => endpoints().find((e) => e.id === endpoint());
  return (
    <Combobox
      testID="endpoint-picker"
      accessibilityLabel={t("common.endpoint")}
      items={items()}
      value={endpoint() ? [endpoint()!] : []}
      placeholder={current()?.name ?? t("common.chooseEndpoint")}
      onChange={({ value }) => {
        const v = value[value.length - 1];
        if (v) useEndpoint(v);
      }}
      offset={4}
      class={`${CONTROL.trigger} ${FOCUS_WITHIN} ${props.class ?? (shellWidth() >= BREAK.toolbar ? WIDTH.switcher : WIDTH.switcherTight)}`}
      inputClass={`min-w-0 flex-1 ${TEXT.body} placeholder:text-foreground`}
      popupClass={`${SURFACE.switcherPopup} ${POPUP} ${POPUP_MOTION.class}`}
      popupStyle={glass}
      popupStartingClass={POPUP_MOTION.starting}
      popupEndingClass={POPUP_MOTION.ending}
      itemClass={`group ${ROWS.popup}`}
      prefix={<Icon name="server" />}
      triggerButton={<Icon name="chevrons-up-down" size={ICON.small} />}
      empty={<Text class={`px-2 py-2 ${TEXT.note}`}>{t("endpoints.noneYet")}</Text>}
      renderItem={(item) => {
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
      footer={
        <ListRow divided testID="manage-endpoints" label={t("endpoints.manage")} onPress={() => go("endpoints")}>
          <Icon name="settings" size={ICON.small} />
          <Text class={TEXT.note}>{t("endpoints.manage")}</Text>
        </ListRow>
      }
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

/** The window's title bar, drawn by the app: drags the window; the system's controls keep their place. */
export function Toolbar(props: { open: (a: Area) => void; extra?: JSX.Element }) {
  let menuButton: ViewRef | undefined;
  const height = () => (isAvailable ? titleBar().height || 44 : 44);
  return (
    <View
      testID="toolbar"
      desktop={{ windowDrag: true }}
      class="flex-row items-center gap-2 border-b border-border bg-toolbar"
      style={{ height: height(), flexShrink: 0, paddingLeft: (isAvailable ? titleBar().left : 0) + 10, paddingRight: (isAvailable ? titleBar().right : 0) + 10 }}
    >
      <IconButton icon="panel-left" label={t("menu.sidebar")} shortcut="mod+b" tip={sidebarHeld() ? t("sidebar.held") : undefined} disabled={sidebarHeld()} onPress={toggleSidebar} selected={sidebarOpen()} testID="toggle-sidebar" />
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
      <View testID="sidebar" class={`flex-1 ${windowMaterial() ? "bg-sidebar/40" : "bg-sidebar"} pt-3`}>
        {/* the places: one stop of Tab, ↑ and ↓ between them */}
        <ToolbarGroup axis="vertical" accessibilityLabel={t("sidebar.places")} class="shrink-0 gap-1 px-2">
          <Place area="run" open={props.open} />
          <Place area="sessions" open={props.open} />
          <Place area="store" open={props.open} />
        </ToolbarGroup>
        {/* under them, the place's own list: its sessions, its buckets, the commands run before */}
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
