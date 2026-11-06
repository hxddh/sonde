// sonde: one app, two forms. Desktop windows get a sidebar (endpoints and
// buckets), a list and a detail pane; a terminal gets one column with a
// navigation stack, the F10 menu bar and keyboard shortcuts.

import { effect, keymap, mount, platform, setColorScheme, Storage, Text, View } from "nsh";
import { createNavStack, NavPage, NavStack } from "nsh/base";
import { isAvailable, Menu, Window, windowState } from "nsh/desktop";
import { t } from "./src/i18n";
import { Audit } from "./src/screens/Audit";
import { Browse } from "./src/screens/Browse";
import { Capacity } from "./src/screens/Capacity";
import { Endpoints } from "./src/screens/Endpoints";
import { Jobs } from "./src/screens/Jobs";
import { Layouts } from "./src/screens/Layouts";
import { Multipart } from "./src/screens/Multipart";
import { Plugins } from "./src/screens/Plugins";
import { Probes } from "./src/screens/Probes";
import { Requests } from "./src/screens/Requests";
import { SettingsScreen } from "./src/screens/Settings";
import { Versions } from "./src/screens/Versions";
import { bucket, endpoint, endpoints, go, init, readOnly, type Section, SECTIONS, section, settings } from "./src/store";
import { Badge, ErrorBanner, NoticeLine, terminal } from "./src/ui/kit";
import { Sidebar } from "./src/ui/sidebar";

import { sectionLabel } from "./src/labels";

/** The view of a section: a plain function, called in a binding, so the view follows `section()`. */
function screen(s: Section) {
  switch (s) {
    case "endpoints":
      return <Endpoints />;
    case "browse":
      return <Browse />;
    case "requests":
      return <Requests />;
    case "multipart":
      return <Multipart />;
    case "versions":
      return <Versions />;
    case "capacity":
      return <Capacity />;
    case "probes":
      return <Probes />;
    case "layouts":
      return <Layouts />;
    case "plugins":
      return <Plugins />;
    case "jobs":
      return <Jobs />;
    case "audit":
      return <Audit />;
    case "settings":
      return <SettingsScreen />;
  }
}

function StatusChip() {
  return <Badge label={readOnly() ? t("common.readOnlyOn") : t("common.readOnlyOff")} tone={readOnly() ? "info" : "warn"} />;
}

function Context() {
  const ep = () => endpoints().find((e) => e.id === endpoint());
  return (
    <Text class="text-xs text-muted-foreground line-clamp-1">
      {ep() ? `${ep()!.name}${bucket() ? ` › ${bucket()}` : ""}` : t("common.chooseEndpoint")}
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
          <Text class="text-sm font-semibold text-foreground">{sectionLabel(section())}</Text>
          <View class="flex-1">
            <Context />
          </View>
          <StatusChip />
        </View>
        <ErrorBanner />
        <View class="flex-1">
          {/* rebuilt per section: screens own their jobs, cancelled when they go */}
          {screen(section())}
        </View>
        <NoticeLine />
      </View>
    </View>
  );
}

// --- terminal ------------------------------------------------------------

type Route = { name: "home" } | { name: "section"; section: Section };
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
  // 1–9 open a section from the home list (not while typing, not under another page)
  const digits: Record<string, () => void> = {};
  SECTIONS.slice(0, 9).forEach((s, i) => {
    digits[String(i + 1)] = () => {
      if (nav.routes().length === 1) open(s);
    };
  });
  keymap.bind(digits, { when: "notEditing" });
  return (
    <NavPage title="sonde" header={<TermHeader title="sonde" />}>
      <View class="flex-1 gap-1 p-1">
        <Text class="text-muted-foreground">{t("app.tagline")} — F10: menu · Tab: next · Esc: back</Text>
        <Each of={SECTIONS}>
          {(s, i) => (
            <View role="button" testID={`nav-${s()}`} onPress={() => open(s())} class="flex-row gap-2 px-1 hover:bg-muted">
              <Text class="w-4 text-muted-foreground">{i() < 9 ? String(i() + 1) : " "}</Text>
              <Text class="text-foreground">{sectionLabel(s())}</Text>
            </View>
          )}
        </Each>
      </View>
    </NavPage>
  );
}

function open(s: Section) {
  go(s);
  if (terminal()) {
    const top = nav.routes()[nav.routes().length - 1];
    if (top.name === "section") nav.replace({ name: "section", section: s });
    else nav.push({ name: "section", section: s });
  }
}

function TerminalShell() {
  return (
    <View class="flex-1 bg-background">
      <NavStack state={nav} class="flex-1">
        {(route) =>
          route.name === "home" ? (
            <Home />
          ) : (
            <NavPage title={sectionLabel(route.section)} header={<TermHeader title={sectionLabel(route.section)} />}>
              <View class="flex-1">
                <ErrorBanner />
                {screen(section())}
                <NoticeLine />
              </View>
            </NavPage>
          )
        }
      </NavStack>
    </View>
  );
}

// --- menus, shortcuts, window -------------------------------------------------

function installMenus() {
  effect(() => {
    const items = SECTIONS.map((s, i) => ({ label: sectionLabel(s), shortcut: i < 9 ? `mod+${i + 1}` : undefined, checked: section() === s, onSelect: () => open(s) }));
    Menu.setApplicationMenu([
      { label: "sonde", submenu: [{ role: "about" }, { type: "separator" }, { label: t("nav.settings"), shortcut: "mod+,", onSelect: () => open("settings") }, { type: "separator" }, { role: "quit" }] },
      { label: "File", submenu: [{ label: t("endpoints.add"), shortcut: "mod+n", onSelect: () => open("endpoints") }, { label: t("endpoints.demo"), onSelect: () => open("endpoints") }] },
      { label: "Edit", submenu: [{ role: "undo" }, { role: "redo" }, { type: "separator" }, { role: "cut" }, { role: "copy" }, { role: "paste" }, { role: "selectAll" }] },
      { label: "View", submenu: items },
      { label: "Window", submenu: [{ role: "minimize" }, { role: "zoom" }, { role: "toggleFullScreen" }] },
    ]);
  });
  keymap.bind({
    "mod+,": () => open("settings"),
    "mod+i": () => open("requests"),
    "mod+j": () => open("jobs"),
  });
}

const WINDOW_KEY = "window.bounds";

async function rememberWindow() {
  if (!isAvailable || terminal()) return;
  try {
    const saved = await Storage.getItem(WINDOW_KEY);
    if (saved) Window.setBounds(JSON.parse(saved));
  } catch {
    // a bad record is dropped
  }
  let timer: ReturnType<typeof setTimeout> | undefined;
  effect(() => {
    const w = windowState();
    if (!w) return;
    const bounds = w.windowBounds;
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void Storage.setItem(WINDOW_KEY, JSON.stringify(bounds)), 500);
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
