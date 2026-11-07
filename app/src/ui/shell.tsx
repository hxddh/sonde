// The window's chrome on the desktops: a title bar of the app's own (the
// toolbar), the sidebar with the places and recent sessions, and the
// application's menus. A terminal has none of this (App.tsx's TerminalShell).

import { type JSX, platform, reduceMotion, signal, Storage, Text, View, windowSize } from "nsh";
import { Button as BaseButton, Combobox } from "nsh/base";
import { isAvailable, Menu, type MenuItem, titleBar } from "nsh/desktop";
import { t } from "../i18n";
import { areaLabel, sectionLabel } from "../labels";
import { api } from "../native";
import { running } from "../run";
import { type Area, area, bucket, chooseBucket, crumb, endpoint, endpoints, go, openSession, readOnly, section, sessionsTick, storeTab } from "../store";
import type { SessionInfo } from "../types";
import { Icon, type IconName } from "./icon";
import { IconButton, Kbd, Tip } from "./kit";
import { paletteOpen } from "./palette";

const mac = platform.os === "macos";

const wanted = signal(true);
const SIDEBAR_KEY = "sidebar.open";
void Storage.getItem(SIDEBAR_KEY).then((v) => v === "false" && wanted.set(false)).catch(() => {});
/** A narrow window folds the sidebar away until it is asked for. */
const narrow = () => windowSize().width < 1100;
const shownWhileNarrow = signal(false);
/** Whether the sidebar shows: the user's choice, folded away in a narrow window. */
export const sidebarOpen = () => wanted() && (!narrow() || shownWhileNarrow());
export function toggleSidebar() {
  if (narrow()) {
    if (!wanted()) wanted.set(true);
    shownWhileNarrow.set(!sidebarOpen() || !shownWhileNarrow() ? !sidebarOpen() : false);
    return;
  }
  wanted.set(!wanted());
  void Storage.setItem(SIDEBAR_KEY, String(wanted())).catch(() => {});
}

export const AREA_ICON: Record<Area, IconName> = { run: "square-terminal", sessions: "list-clock", store: "database", settings: "settings" };
export const AREA_KEY: Record<Area, string> = { run: "mod+1", sessions: "mod+2", store: "mod+3", settings: "mod+," };

/** The application's menus: macOS's menu bar; the toolbar's menu button elsewhere. */
export function appMenus(open: (a: Area) => void): { label: string; submenu: MenuItem[] }[] {
  const places: MenuItem[] = (["run", "sessions", "store", "settings"] as Area[]).map((a) => ({ label: areaLabel(a), shortcut: AREA_KEY[a], checked: area() === a, onSelect: () => open(a) }));
  return [
    {
      label: t("menu.file"),
      submenu: [
        { label: t("run.start"), shortcut: "mod+r", onSelect: () => open("run") },
        { label: t("palette.open"), shortcut: "mod+k", onSelect: () => paletteOpen.set(true) },
        ...(mac ? [] : [{ type: "separator" } as const, { label: t("nav.settings"), shortcut: "mod+,", onSelect: () => open("settings") }, { type: "separator" } as const, { role: "quit", label: t("menu.quit") } as const]),
      ],
    },
    { label: t("menu.edit"), submenu: [{ role: "undo" }, { role: "redo" }, { type: "separator" }, { role: "cut" }, { role: "copy" }, { role: "paste" }, { role: "selectAll" }] },
    { label: t("menu.view"), submenu: [...places, { type: "separator" }, { label: t("menu.sidebar"), shortcut: "mod+b", checked: sidebarOpen(), onSelect: toggleSidebar }, { role: "toggleFullScreen" }] },
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
    <View class="min-w-0 flex-row items-center gap-1.5">
      <Text class="text-[13px] font-semibold text-foreground">{areaLabel(area())}</Text>
      <If when={detail()}>
        {(d) => (
          <>
            <Icon name="chevron-right" size={13} />
            <Text class="min-w-0 shrink text-[13px] text-muted-foreground line-clamp-1">{d()}</Text>
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
        if (v && v !== endpoint()) {
          endpoint.set(v);
          chooseBucket(null);
        }
      }}
      offset={4}
      class={`h-7 flex-row items-center gap-1.5 rounded-(--radius) border border-input bg-background pl-2 pr-1 hover:border-ring/60 ${props.class ?? "w-52"}`}
      inputClass="min-w-0 flex-1 text-[13px] text-foreground placeholder:text-foreground"
      popupClass="min-w-[260px] rounded-(--radius-card) border border-border bg-popover p-1 shadow-lg"
      itemClass="group rounded-(--radius-sm) px-2 py-1.5 focus:bg-accent"
      prefix={<Icon name="server" size={14} />}
      triggerButton={<Icon name="chevrons-up-down" size={13} />}
      empty={<Text class="px-2 py-2 text-xs text-muted-foreground">{t("endpoints.noneYet")}</Text>}
      renderItem={(item) => {
        const e = endpoints().find((x) => x.id === item.value);
        return (
          <View class="flex-row items-center gap-2">
            <View class="min-w-0 flex-1">
              <Text class="text-[13px] text-popover-foreground line-clamp-1">{item.title}</Text>
              <Text class="font-mono text-[11px] text-muted-foreground line-clamp-1">{e?.url ?? ""}</Text>
            </View>
            <Text class="text-[11px] text-muted-foreground">{e?.vendorLabel ?? ""}</Text>
          </View>
        );
      }}
      footer={
        <BaseButton testID="manage-endpoints" accessibilityLabel={t("endpoints.manage")} onPress={() => go("endpoints")} class="mt-1 flex-row items-center gap-2 rounded-(--radius-sm) border-t border-border px-2 py-1.5 hover:bg-muted">
          <Icon name="settings" size={13} />
          <Text class="text-xs text-muted-foreground">{t("endpoints.manage")}</Text>
        </BaseButton>
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
          tooltip={<Tip text={r().command} />}
          class="h-7 max-w-[220px] flex-row items-center gap-2 rounded-full bg-primary/12 px-3 hover:bg-primary/20"
        >
          <View class={`size-1.5 rounded-full bg-primary ${reduceMotion() ? "" : "animate-pulse"}`} />
          <Text class="text-xs font-medium text-primary line-clamp-1">{r().command.split(" ")[0]}</Text>
        </BaseButton>
      )}
    </If>
  );
}

/** The search field's stand-in: opens the command palette. */
function SearchButton() {
  return (
    <BaseButton
      testID="palette"
      accessibilityLabel={t("palette.open")}
      onPress={() => paletteOpen.set(true)}
      class="h-7 w-44 flex-row items-center gap-2 rounded-(--radius) border border-input bg-background px-2 hover:border-ring/60"
    >
      <Icon name="search" size={14} />
      <Text class="flex-1 text-[13px] text-muted-foreground">{t("palette.open")}</Text>
      <Kbd keys="mod+k" />
    </BaseButton>
  );
}

/** The window's title bar, drawn by the app: drags the window; the system's controls keep their place. */
export function Toolbar(props: { open: (a: Area) => void; extra?: JSX.Element }) {
  const height = () => (isAvailable ? titleBar().height || 44 : 44);
  return (
    <View
      testID="toolbar"
      desktop={{ windowDrag: true }}
      class="flex-row items-center gap-2 border-b border-border bg-toolbar"
      style={{ height: height(), flexShrink: 0, paddingLeft: (isAvailable ? titleBar().left : 0) + 10, paddingRight: (isAvailable ? titleBar().right : 0) + 10 }}
    >
      <IconButton icon="panel-left" label={t("menu.sidebar")} shortcut="mod+b" onPress={toggleSidebar} selected={sidebarOpen()} testID="toggle-sidebar" />
      <View class="ml-1 min-w-0 flex-1">
        <Breadcrumb />
      </View>
      <RunningPill />
      <EndpointSwitcher />
      <IconButton
        testID="toolbar-lock"
        icon={readOnly() ? "lock" : "lock-open"}
        label={readOnly() ? t("common.readOnlyOn") : t("common.readOnlyOff")}
        tint={readOnly() ? "muted-foreground" : "warn"}
        onPress={() => go("settings")}
      />
      <SearchButton />
      <If when={!mac}>
        <IconButton
          testID="app-menu"
          icon="ellipsis"
          label={t("menu.menu")}
          onPress={() => void Menu.popup(appMenus(props.open).map((m) => ({ label: m.label, submenu: m.submenu })))}
        />
      </If>
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
      class="h-8 flex-row items-center gap-2.5 rounded-(--radius) px-2 hover:bg-sidebar-accent/50 selected:bg-sidebar-accent"
    >
      <Icon name={AREA_ICON[props.area]} size={16} tint={on() ? "sidebar-accent-foreground" : "muted-foreground"} />
      <Text class={on() ? "flex-1 text-[13px] font-medium text-sidebar-accent-foreground" : "flex-1 text-[13px] text-sidebar-foreground"}>{areaLabel(props.area)}</Text>
      <If when={props.area === "run" && running()}>
        <View class="size-1.5 rounded-full bg-primary" />
      </If>
    </BaseButton>
  );
}

function ago(iso: string): string {
  const s = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (s < 60) return t("time.now");
  if (s < 3600) return t("time.minutes", { n: Math.floor(s / 60) });
  if (s < 86400) return t("time.hours", { n: Math.floor(s / 3600) });
  return t("time.days", { n: Math.floor(s / 86400) });
}

/** The newest sessions, a press away. */
function Recent(props: { open: (a: Area) => void }) {
  const list = () => {
    sessionsTick();
    try {
      return api.sessions.list().slice(0, 6);
    } catch {
      return [] as SessionInfo[];
    }
  };
  return (
    <If when={list().length > 0}>
      <View class="gap-0.5">
        <Text class="px-2 pb-1 pt-4 text-[11px] font-medium text-muted-foreground">{t("sidebar.recent")}</Text>
        <Each of={list()} key={(s) => s.path}>
          {(s) => (
            <BaseButton
              accessibilityLabel={s().command ?? s().name}
              onPress={() => {
                openSession.set(s().path);
                props.open("sessions");
              }}
              class="flex-row items-center gap-2 rounded-(--radius) px-2 py-1 hover:bg-sidebar-accent/50"
            >
              <View class={`size-1.5 rounded-full ${s().live ? "bg-primary" : "bg-transparent"}`} />
              <Text class="min-w-0 flex-1 text-xs text-sidebar-foreground line-clamp-1">{s().command ? (s().command!.split(" ")[0].split("/").pop() ?? s().name) : s().name}</Text>
              <Text class="text-[11px] text-muted-foreground">{ago(s().created)}</Text>
            </BaseButton>
          )}
        </Each>
      </View>
    </If>
  );
}

export function Sidebar(props: { open: (a: Area) => void }) {
  return (
    <If when={sidebarOpen()}>
      <View testID="sidebar" class="w-56 border-r border-sidebar-border bg-sidebar px-2 pb-2 pt-3">
        <View class="gap-0.5">
          <Place area="run" open={props.open} />
          <Place area="sessions" open={props.open} />
          <Place area="store" open={props.open} />
        </View>
        <Recent open={props.open} />
        <View class="flex-1" />
        <Place area="settings" open={props.open} />
      </View>
    </If>
  );
}
