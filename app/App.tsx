// sonde: two places — Runs and Buckets — with Settings in a window of their
// own, Activity from the edge, and a command palette over everything. The
// window has a sidebar and the place beside it.

import { appInfo, batch, effect, keymap, mount, platform, setColorScheme, signal, Text, UrlScheme, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup } from "nsh/base";
import { Application, currentWindow, Menu } from "nsh/desktop";
import { Welcome } from "./src/screens/Welcome";
import { t } from "./src/i18n";
import { endForQuit, followNotifications, running } from "./src/run";
import { Runs } from "./src/screens/Runs";
import { StoreArea } from "./src/screens/StoreArea";
import { type Area, area, areaOf, endpoints, go, init, newRun, openDropped, openLaunch, openLink, openSystemFiles, wouldOpen, openRun, openRunFile, report, shellHeight, setOverlay, settings, shellWidth, STORE_TABS } from "./src/store";
import { openSettingsWindow, SettingsBody } from "./src/screens/SettingsWindow";
import { Activity, activityAnchor, activityOpen, runningJobs, watchJobs } from "./src/ui/activity";
import { BREAK, FocusRest, PANE, Recoverable, SidePanel, SPLIT_HANDLE, TEXT, Title, windowFace } from "./src/ui/kit";
import { rememberedSize } from "./src/ui/sizes";
import { Palette, paletteOpen } from "./src/ui/palette";
import { appMenus, Sidebar, sidebarOpen, toggleSidebar, Toolbar, windowMaterial } from "./src/ui/shell";
import { Toaster } from "./src/ui/toast";

const mac = platform.os === "macos";

/** The app's name, as it was built (nsh.json's `name`). */
const APP = appInfo().name ?? "";

/** A run opened while there is no endpoint yet (a file, the example): Runs shows it rather than the welcome. */
const runOpened = signal(false);
effect(() => {
  if (openRun()) runOpened.set(true);
});

/** What the window shows: the area, and whether it welcomes instead (no endpoint yet, and no run opened). A
 * place is drawn afresh when it changes, inside a boundary of its own. */
const view = () => `${area()}|${endpoints().length === 0 && (area() === "store" || !runOpened())}`;

function screen(a: Area, welcome: boolean) {
  if (welcome && (a === "runs" || a === "store")) return <Welcome />;
  switch (a) {
    case "runs":
      return <Runs />;
    case "store":
      return <StoreArea />;
    case "settings":
      return <SettingsBody />;
    case "activity":
      return <Activity />;
  }
}

/** Goes to a place. */
const open = (a: Area) => go(a);

function Shell() {
  // a program or a job at work shows on the Dock's icon and the taskbar button
  const win = currentWindow();
  watchJobs();
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
    // (a bucket's tools are sections of Buckets: the place itself)
    return false;
  });
  return (
    // on macOS the window's own sidebar material shows through the sidebar
    <View
      testID="shell"
      class={`flex-1 ${windowMaterial() ? "" : "bg-background"} ${windowFace()}`}
      onLayout={(e) => batch(() => (shellWidth.set(e.bounds.size.width), shellHeight.set(e.bounds.size.height)))}
      // a .sonde file dropped anywhere on the window opens it; while one is over it, it says so
      onFileDrop={(e) => {
        dragging.set(null);
        openDropped(e.uris);
      }}
      // what the drag carries is known before the drop: the hint says what will open, and what will not
      onFileDragEnter={(e) => dragging.set(wouldOpen(e.uris))}
      onFileDragLeave={() => dragging.set(null)}
    >
      <FocusRest />
      {/* the toolbar across the window; on macOS over the place only, the sidebar running the window's height with
          the window's buttons in it, as a Mac app's does */}
      {mac ? null : <Toolbar open={open} />}
      {/* the sidebar and the place: a split, the sidebar's width the user's (kept) */}
      <ResizablePanelGroup class="min-h-0 flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("sidebar.resize")}>
        <ResizablePanel fixed visible={sidebarOpen()} size={shellWidth() < BREAK.toolbar ? Math.min(sidebarWidth.size(), PANE.sidebarNarrow) : sidebarWidth.size()} minSize={PANE.sidebarLeast} maxSize={PANE.sidebarMost} onResize={sidebarWidth.onResize}>
          <Sidebar open={open} />
        </ResizablePanel>
        <ResizablePanel minSize={PANE.mainLeast} class="min-w-0 bg-background">
          {mac ? <Toolbar open={open} /> : null}
          {/* rebuilt per area: screens own their jobs, cancelled when they go */}
          <Each of={[view()]} key={(x) => x}>
            {(x) => <Recoverable>{screen(x().split("|")[0] as Area, x().split("|")[1] === "true")}</Recoverable>}
          </Each>
        </ResizablePanel>
      </ResizablePanelGroup>
      <SidePanel open={activityOpen()} onClose={() => activityOpen.set(false)} label={t("nav.activity")} returnFocus={() => activityAnchor}>
        <Activity onClose={() => activityOpen.set(false)} />
      </SidePanel>
      <Palette />
      <If when={dragging()}>
        {(d) => (
          <View testID="drop-hint" class={`absolute inset-3 items-center justify-center gap-1 rounded-(--radius-card) border-2 border-dashed bg-background/80 ${d().opens > 0 || d().opens + d().others === 0 ? "border-ring" : "border-border"}`}>
            <Title center>{d().opens > 0 ? t("app.dropOpens", { n: d().opens }) : d().others > 0 ? t("app.dropNothing") : t("app.dropHint")}</Title>
            <Text class={TEXT.note}>{d().opens > 0 && d().others > 0 ? t("app.dropSkips", { n: d().others }) : t("app.dropKinds")}</Text>
          </View>
        )}
      </If>
      <Toaster />
    </View>
  );
}

// --- menus, shortcuts, window -------------------------------------------------

function installMenus() {
  effect(() => {
    Menu.setApplicationMenu([
      platform.os === "macos" && {
        label: APP,
        submenu: [{ role: "about" }, { type: "separator" }, { label: t("menu.settings"), shortcut: "mod+,", onSelect: () => open("settings") }, { type: "separator" }, { role: "hide" }, { role: "hideOthers" }, { role: "showAll" }, { type: "separator" }, { role: "quit" }],
      },
      ...appMenus(open),
    ]);
  });
  keymap.bind({
    "mod+1": () => open("runs"),
    "mod+2": () => open("store"),
    "mod+,": () => open("settings"),
    "mod+shift+a": () => open("activity"),
    "mod+k": () => paletteOpen.set(!paletteOpen()),
    "mod+b": () => toggleSidebar(),
    "mod+o": () => void openRunFile(),
    "mod+n": newRun,
  });
  // Buckets' sections, in their row's order; not while typing, where Windows reports AltGr
  // (which types `²`, `{`, `~` on many layouts) as Ctrl+Alt
  keymap.bind(Object.fromEntries(STORE_TABS.map((s, i) => [`mod+alt+${i + 1}`, () => go(s)])), { when: "notEditing" });
}

/** The sidebar's width, as the user left it. */
const sidebarWidth = rememberedSize("sidebar.list", 272);

/** A file is being dragged over the window. */
const dragging = signal<{ opens: number; others: number } | null>(null);

function App() {
  init();
  // links into the app (sonde show --app, a shared page) and a pressed notification
  try {
    UrlScheme.on("open", ({ url }) => openLink(url));
  } catch {
    // no links here
  }
  // sonde runs once (nsh.json's `singleInstance`): a second launch brings this window
  // forward and hands over what it was asked to open
  try {
    Application.on("secondInstance", ({ args, cwd }) => openLaunch(args, cwd));
    // a .sonde file opened from the system (a double click, Open With, a drop on the app's
    // icon; nsh.json's `fileTypes`): as one dropped on the window, the launch's own included
    Application.on("openFiles", ({ paths }) => openSystemFiles(paths));
    // the user quits (the Quit item, its key, the Dock): the program being recorded is ended and its run
    // written whole first, however long that takes, then the app quits (Native Shell's quit request)
    Application.on("quitRequest", () => void endForQuit().finally(() => Application.quit()));
    // the system ends the app (logging out, shutting down) without asking: the same, in the 200 ms it waits
    Application.on("quit", () => endForQuit());
  } catch {
    // a host that is not the app's own (a test runner)
  }
  followNotifications((path) => {
    openRun.set(path);
    go("runs");
  });
  effect(() => {
    const theme = settings()?.theme ?? "system";
    setColorScheme(theme === "light" || theme === "dark" ? theme : "system");
  });
  installMenus();
  return <Shell />;
}

mount(App);
