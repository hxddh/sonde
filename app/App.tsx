// sonde: one app, three forms. Four places — Run, Sessions, Store and
// Settings — and a command palette over everything. Desktop windows get a
// sidebar and the place beside it; a terminal gets a home list and a
// navigation stack, the F10 menu bar and `:` for the palette.

import { batch, effect, keymap, mount, platform, setColorScheme, Storage, Text, UrlScheme, View } from "nsh";
import { createNavStack, NavPage, NavStack } from "nsh/base";
import { currentWindow, isAvailable, Menu, Window, windowState } from "nsh/desktop";
import { Welcome } from "./src/screens/Welcome";
import { t } from "./src/i18n";
import { areaLabel } from "./src/labels";
import { followNotifications, openLastRun, running } from "./src/run";
import { Run } from "./src/screens/Run";
import { Sessions } from "./src/screens/Sessions";
import { StoreArea } from "./src/screens/StoreArea";
import { type Area, area, areaOf, AREAS, bucket, endpoint, endpoints, go, init, openDropped, openLink, openSessionFile, readOnly, report, section, setOverlay, settings, shellHeight, shellWidth, storeSheet, type StoreTab, storeTab } from "./src/store";
import { openSettingsWindow, SettingsBody } from "./src/screens/SettingsWindow";
import { openCompatWindow } from "./src/screens/CompatWindow";
import { Activity, activityAnchor, activityOpen, runningJobs } from "./src/ui/activity";
import { Badge, Recoverable, SidePanel, terminal } from "./src/ui/kit";
import { Palette, paletteOpen } from "./src/ui/palette";
import { appMenus, Sidebar, toggleSidebar, Toolbar, windowMaterial } from "./src/ui/shell";
import { Toaster } from "./src/ui/toast";
import { applyContrast } from "./src/theme";

/** The view of an area: a plain function, called in a binding, so the view follows `area()`.
 * Until there is an endpoint, Run and Store open on the welcome. */
/** What the window shows: the area, and whether it welcomes instead. A
 * place is drawn afresh when it changes, inside a boundary of its own. */
const view = () => `${area()}|${endpoints().length === 0}`;

function screen(a: Area) {
  if ((a === "run" || a === "store") && endpoints().length === 0) return <Welcome />;
  switch (a) {
    case "run":
      return <Run />;
    case "sessions":
      return <Sessions />;
    case "store":
      return <StoreArea />;
    case "settings":
      return <SettingsBody />;
    case "activity":
      return <Activity />;
  }
}

function StatusChip() {
  return <Badge label={readOnly() ? t("common.readOnlyOn") : t("common.readOnlyOff")} tone={readOnly() ? "info" : "warn"} />;
}

function Context() {
  const ep = () => endpoints().find((e) => e.id === endpoint());
  return (
    <Text class="text-small text-muted-foreground line-clamp-1">
      {ep() ? `${ep()!.name}${area() === "store" && bucket() ? ` › ${bucket()}` : ""}` : t("common.chooseEndpoint")}
    </Text>
  );
}

// --- desktop -------------------------------------------------------------

function DesktopShell() {
  // a program or a job at work shows on the Dock's icon and the taskbar button
  const win = currentWindow();
  effect(() => {
    const busy = running() !== null || runningJobs() > 0;
    win.set({ progress: busy ? { state: "indeterminate" } : null });
  });
  // Settings open in a window of their own, Activity from the edge: neither is a place here
  setOverlay((s) => {
    const a = areaOf(s);
    if (a === "settings") {
      try {
        openSettingsWindow();
      } catch (e) {
        report(e);
      }
      return true;
    }
    if (a === "activity") {
      activityOpen.set(true);
      return true;
    }
    // compatibility is a window of its own, opened from the endpoints
    if (s === "probes") {
      try {
        openCompatWindow();
      } catch (e) {
        report(e);
      }
      return true;
    }
    // a bucket's details open over its objects
    if (a === "store") {
      storeTab.set("browse");
      storeSheet.set(s === "browse" ? null : (s as StoreTab));
      section.set("browse");
      return true;
    }
    return false;
  });
  return (
    // on macOS the window's own sidebar material shows through the sidebar
    <View
      testID="shell"
      class={`flex-1 ${windowMaterial() ? "" : "bg-background"}`}
      onLayout={(e) => batch(() => (shellWidth.set(e.bounds.size.width), shellHeight.set(e.bounds.size.height)))}
      // a .sonde file dropped anywhere on the window opens it
      onFileDrop={(e) => openDropped(e.uris)}
    >
      <Toolbar open={open} />
      <View class="min-h-0 flex-1 flex-row">
        <Sidebar open={open} />
        <View class="min-w-0 flex-1 bg-background">
          {/* rebuilt per area: screens own their jobs, cancelled when they go */}
          <Each of={[view()]} key={(x) => x}>
            {(x) => <Recoverable>{screen(x().split("|")[0] as Area)}</Recoverable>}
          </Each>
        </View>
      </View>
      <SidePanel open={activityOpen()} onClose={() => activityOpen.set(false)} returnFocus={() => activityAnchor}>
        <Activity onClose={() => activityOpen.set(false)} />
      </SidePanel>
      <Palette />
      <Toaster />
    </View>
  );
}

// --- terminal ------------------------------------------------------------

type Route = { name: "home" } | { name: "area"; area: Area };
const nav = createNavStack<Route>([{ name: "home" }]);

function TermHeader(props: { title: string }) {
  return (
    <View class="flex-row items-center gap-2 border-b border-border px-1">
      <If when={nav.routes().length > 1}>
        <Text role="button" class="text-primary" onPress={() => nav.pop()}>
          ‹ {t("common.back")}
        </Text>
      </If>
      <Text class="font-semibold text-foreground">{props.title}</Text>
      <View class="flex-1">
        <Context />
      </View>
      <StatusChip />
    </View>
  );
}

function Home() {
  // 1–4 open a place from the home list (not while typing, not under another page)
  const digits: Record<string, () => void> = {};
  AREAS.forEach((a, i) => {
    digits[String(i + 1)] = () => {
      if (nav.routes().length === 1) open(a);
    };
  });
  keymap.bind(digits, { when: "notEditing" });
  return (
    <NavPage title="sonde" header={<TermHeader title="sonde" />}>
      <View class="flex-1 gap-1 p-1">
        <Text class="text-muted-foreground">{t("app.tagline")} — : search · F10: menu · Tab: next · Esc: back</Text>
        <Each of={AREAS}>
          {(a, i) => (
            <View role="button" testID={`nav-${a()}`} onPress={() => open(a())} class="flex-row gap-2 px-1 hover:bg-muted">
              <Text class="w-4 text-muted-foreground">{String(i() + 1)}</Text>
              <Text class="flex-1 text-foreground">{areaLabel(a())}</Text>
              <If when={a() === "run" && running()}>
                <Text class="text-primary">● {running()!.command}</Text>
              </If>
            </View>
          )}
        </Each>
      </View>
    </NavPage>
  );
}

/** Goes to a place; in a terminal, as a page over the home list. */
function open(a: Area) {
  go(a);
  if (terminal()) {
    const top = nav.routes()[nav.routes().length - 1];
    if (top.name === "area") nav.replace({ name: "area", area: a });
    else nav.push({ name: "area", area: a });
  }
}

function TerminalShell() {
  // a place chosen elsewhere (the palette, a button) becomes the page on top
  effect(() => {
    const a = area();
    const top = nav.routes()[nav.routes().length - 1];
    if (top.name === "area" && top.area !== a) nav.replace({ name: "area", area: a });
  });
  return (
    <View class="flex-1 bg-background">
      <NavStack state={nav} class="flex-1">
        {(route) =>
          route.name === "home" ? (
            <Home />
          ) : (
            <NavPage title={areaLabel(route.area)} header={<TermHeader title={areaLabel(route.area)} />}>
              <View class="flex-1">
                <Each of={[view()]} key={(x) => x}>
                  {(x) => <Recoverable>{screen(x().split("|")[0] as Area)}</Recoverable>}
                </Each>
              </View>
            </NavPage>
          )
        }
      </NavStack>
      <Palette />
      <Toaster />
    </View>
  );
}

// --- menus, shortcuts, window -------------------------------------------------

function installMenus() {
  effect(() => {
    Menu.setApplicationMenu([
      platform.os === "macos" && {
        label: "sonde",
        submenu: [{ role: "about" }, { type: "separator" }, { label: t("nav.settings"), shortcut: "mod+,", onSelect: () => open("settings") }, { type: "separator" }, { role: "hide" }, { role: "hideOthers" }, { role: "showAll" }, { type: "separator" }, { role: "quit" }],
      },
      ...appMenus(open),
    ]);
  });
  keymap.bind({
    "mod+1": () => open("run"),
    "mod+2": () => open("sessions"),
    "mod+3": () => open("store"),
    "mod+,": () => open("settings"),
    "mod+shift+a": () => open("activity"),
    "mod+k": () => paletteOpen.set(!paletteOpen()),
    "mod+b": () => !terminal() && toggleSidebar(),
    "mod+o": () => !terminal() && void openSessionFile(),
  });
  if (terminal()) keymap.bind({ "shift+;": () => paletteOpen.set(true) }, { when: "notEditing" });
}

const WINDOW_KEY = "window.placement";

/** Opens the main window where the user left it: size, place, maximized or full screen. */
async function rememberWindow() {
  if (!isAvailable || terminal()) return;
  try {
    const saved = await Storage.getItem(WINDOW_KEY);
    const main = Window.get("main");
    const placement = saved ? JSON.parse(saved) : null;
    // a placement smaller than the window's least size (a phone-sized window
    // some platforms open before applying nsh.json) is not restored
    const size = placement?.bounds?.size ?? placement?.normal?.size;
    if (placement && (!size || (size.width >= 900 && size.height >= 560))) main?.setPlacement(placement);
    else if ((main?.state()?.bounds.size.width ?? 0) < 900) main?.setBounds({ size: { width: 1280, height: 820 } });
  } catch {
    // a bad record is dropped
  }
  let timer: ReturnType<typeof setTimeout> | undefined;
  effect(() => {
    const w = windowState();
    if (!w) return;
    const placement = w.placement;
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void Storage.setItem(WINDOW_KEY, JSON.stringify(placement)), 500);
  });
}

function App() {
  init();
  // links into the app (sonde show --app, a shared page) and a pressed notification
  if (platform.idiom !== "terminal") {
    try {
      UrlScheme.on("open", ({ url }) => openLink(url));
    } catch {
      // no links here
    }
    followNotifications(() => openLastRun(go));
  }
  effect(() => {
    const theme = settings()?.theme ?? "system";
    setColorScheme(theme === "light" || theme === "dark" ? theme : "system");
  });
  // a terminal draws in its own colors
  effect(() => {
    if (platform.idiom !== "terminal") applyContrast(settings()?.["high-contrast"] ?? false);
  });
  installMenus();
  void rememberWindow();
  return platform.idiom === "terminal" ? <TerminalShell /> : <DesktopShell />;
}

mount(App);
