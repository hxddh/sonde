// sonde: one app, two forms. Desktop windows get a sidebar (endpoints and
// buckets), a list and a detail pane; a terminal gets one column with a
// navigation stack, the F10 menu bar and keyboard shortcuts.

// sonde: one app, three forms. Four places — Run, Sessions, Store and
// Settings — and a command palette over everything. Desktop windows get a
// sidebar and the place beside it; a terminal gets a home list and a
// navigation stack, the F10 menu bar and `:` for the palette.

import { effect, keymap, mount, platform, setColorScheme, Storage, Text, View } from "nsh";
import { createNavStack, NavPage, NavStack } from "nsh/base";
import { isAvailable, Menu, Window, windowState } from "nsh/desktop";
import { t } from "./src/i18n";
import { areaLabel } from "./src/labels";
import { running } from "./src/run";
import { Run } from "./src/screens/Run";
import { Sessions } from "./src/screens/Sessions";
import { SettingsArea } from "./src/screens/SettingsArea";
import { StoreArea } from "./src/screens/StoreArea";
import { type Area, area, AREAS, bucket, endpoint, endpoints, go, init, readOnly, settings } from "./src/store";
import { ErrorBanner, NoticeLine } from "./src/ui/banner";
import { Badge, terminal } from "./src/ui/kit";
import { Palette, paletteOpen } from "./src/ui/palette";
import { Sidebar } from "./src/ui/sidebar";

/** The view of an area: a plain function, called in a binding, so the view follows `area()`. */
function screen(a: Area) {
  switch (a) {
    case "run":
      return <Run />;
    case "sessions":
      return <Sessions />;
    case "store":
      return <StoreArea />;
    case "settings":
      return <SettingsArea />;
  }
}

function StatusChip() {
  return <Badge label={readOnly() ? t("common.readOnlyOn") : t("common.readOnlyOff")} tone={readOnly() ? "info" : "warn"} />;
}

function Context() {
  const ep = () => endpoints().find((e) => e.id === endpoint());
  return (
    <Text class="text-xs text-muted-foreground line-clamp-1">
      {ep() ? `${ep()!.name}${area() === "store" && bucket() ? ` › ${bucket()}` : ""}` : t("common.chooseEndpoint")}
    </Text>
  );
}

// --- desktop -------------------------------------------------------------

function DesktopShell() {
  return (
    <View class="flex-1 flex-row bg-background">
      <Sidebar />
      <View class="flex-1">
        <View class="h-11 flex-row items-center gap-3 border-b border-border px-4">
          <Text class="text-sm font-semibold text-foreground">{areaLabel(area())}</Text>
          <View class="flex-1">
            <Context />
          </View>
          <StatusChip />
        </View>
        <ErrorBanner />
        <View class="flex-1">
          {/* rebuilt per area: screens own their jobs, cancelled when they go */}
          {screen(area())}
        </View>
        <NoticeLine />
      </View>
      <Palette />
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
                <ErrorBanner />
                {screen(area())}
                <NoticeLine />
              </View>
            </NavPage>
          )
        }
      </NavStack>
      <Palette />
    </View>
  );
}

// --- menus, shortcuts, window -------------------------------------------------

function installMenus() {
  effect(() => {
    const items = AREAS.map((a, i) => ({ label: areaLabel(a), shortcut: a === "settings" ? "mod+," : `mod+${i + 1}`, checked: area() === a, onSelect: () => open(a) }));
    Menu.setApplicationMenu([
      { label: "sonde", submenu: [{ role: "about" }, { type: "separator" }, { label: t("nav.settings"), shortcut: "mod+,", onSelect: () => open("settings") }, { type: "separator" }, { role: "quit" }] },
      { label: "File", submenu: [{ label: t("run.start"), shortcut: "mod+r", onSelect: () => open("run") }, { label: t("palette.open"), shortcut: "mod+k", onSelect: () => paletteOpen.set(true) }] },
      { label: "Edit", submenu: [{ role: "undo" }, { role: "redo" }, { type: "separator" }, { role: "cut" }, { role: "copy" }, { role: "paste" }, { role: "selectAll" }] },
      { label: "View", submenu: items },
      { label: "Window", submenu: [{ role: "minimize" }, { role: "zoom" }, { role: "toggleFullScreen" }] },
    ]);
  });
  keymap.bind({
    "mod+1": () => open("run"),
    "mod+2": () => open("sessions"),
    "mod+3": () => open("store"),
    "mod+,": () => open("settings"),
    "mod+k": () => paletteOpen.set(!paletteOpen()),
  });
  if (terminal()) keymap.bind({ "shift+;": () => paletteOpen.set(true) }, { when: "notEditing" });
}

const WINDOW_KEY = "window.placement";

/** Opens the main window where the user left it: size, place, maximized or full screen. */
async function rememberWindow() {
  if (!isAvailable || terminal()) return;
  try {
    const saved = await Storage.getItem(WINDOW_KEY);
    if (saved) Window.get("main")?.setPlacement(JSON.parse(saved));
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
  effect(() => {
    const theme = settings()?.theme ?? "system";
    setColorScheme(theme === "light" || theme === "dark" ? theme : "system");
  });
  installMenus();
  void rememberWindow();
  return platform.idiom === "terminal" ? <TerminalShell /> : <DesktopShell />;
}

mount(App);
