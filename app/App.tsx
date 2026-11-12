// sonde: one app, three forms. Four places — Run, Sessions, Store and
// Settings — and a command palette over everything. Desktop windows get a
// sidebar and the place beside it; a terminal gets a home list and a
// navigation stack, the F10 menu bar and `:` for the palette.

import { appInfo, batch, effect, keymap, mount, platform, setColorScheme, signal, Text, untracked, UrlScheme, View } from "nsh";
import { createNavStack, NavPage, NavStack, ResizablePanel, ResizablePanelGroup } from "nsh/base";
import { Application, currentWindow, Menu } from "nsh/desktop";
import { Welcome } from "./src/screens/Welcome";
import { t } from "./src/i18n";
import { areaLabel } from "./src/labels";
import { endForQuit, followNotifications, running } from "./src/run";
import { Run } from "./src/screens/Run";
import { Sessions } from "./src/screens/Sessions";
import { StoreArea } from "./src/screens/StoreArea";
import { type Area, area, areaOf, AREAS, bucket, endpoint, endpoints, go, init, openDropped, openLaunch, openLink, openSystemFiles, wouldOpen, openSession, openSessionFile, readOnly, report, section, shellHeight, setOverlay, settings, shellWidth, storeSheet, type StoreTab, storeTab } from "./src/store";
import { openSettingsWindow, SettingsBody } from "./src/screens/SettingsWindow";
import { openCompatWindow } from "./src/screens/CompatWindow";
import { Activity, activityAnchor, activityOpen, runningJobs } from "./src/ui/activity";
import { Back, Badge, FocusRest, ListRow, PANE, Recoverable, SidePanel, SPLIT_HANDLE, terminal, Title, WIDTH } from "./src/ui/kit";
import { count } from "./src/format";
import { rememberedSize } from "./src/ui/sizes";
import { Palette, paletteOpen } from "./src/ui/palette";
import { appMenus, Sidebar, sidebarOpen, toggleSidebar, Toolbar, windowMaterial } from "./src/ui/shell";
import { Toaster } from "./src/ui/toast";
import { applyContrast } from "./src/theme";
import { scrubCommand } from "./src/ui/sessionview";

/** The app's name, as it was built (nsh.json's `name`). */
const APP = appInfo().name ?? "";

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
      <Toolbar open={open} />
      {/* the sidebar and the place: a split, the sidebar's width the user's (kept) */}
      <ResizablePanelGroup class="min-h-0 flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("sidebar.resize")}>
        <ResizablePanel fixed visible={sidebarOpen()} size={sidebarWidth.size()} minSize={PANE.sidebarLeast} maxSize={PANE.sidebarMost} onResize={sidebarWidth.onResize}>
          <Sidebar open={open} />
        </ResizablePanel>
        <ResizablePanel minSize={PANE.mainLeast} class="min-w-0 bg-background">
          {/* rebuilt per area: screens own their jobs, cancelled when they go */}
          <Each of={[view()]} key={(x) => x}>
            {(x) => <Recoverable>{screen(x().split("|")[0] as Area)}</Recoverable>}
          </Each>
        </ResizablePanel>
      </ResizablePanelGroup>
      <SidePanel open={activityOpen()} onClose={() => activityOpen.set(false)} returnFocus={() => activityAnchor}>
        <Activity onClose={() => activityOpen.set(false)} />
      </SidePanel>
      <Palette />
      <If when={dragging()}>
        {(d) => (
          <View testID="drop-hint" class={`absolute inset-3 items-center justify-center gap-1 rounded-(--radius-card) border-2 border-dashed bg-background/80 ${d().opens > 0 || d().opens + d().others === 0 ? "border-ring" : "border-border"}`}>
            <Title center>{d().opens > 0 ? t("app.dropOpens", { n: d().opens }) : d().others > 0 ? t("app.dropNothing") : t("app.dropHint")}</Title>
            <Text class="text-small text-muted-foreground">{d().opens > 0 && d().others > 0 ? t("app.dropSkips", { n: d().others }) : t("app.dropKinds")}</Text>
          </View>
        )}
      </If>
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
        <Back onPress={() => nav.pop()} />
      </If>
      <Title>{props.title}</Title>
      <View class="flex-1">
        <Context />
      </View>
      <StatusChip />
    </View>
  );
}

function Home() {
  // 1–5 open a place from the home list (not while typing, not under another page)
  const digits: Record<string, () => void> = {};
  AREAS.forEach((a, i) => {
    digits[`${i + 1}`] = () => {
      if (nav.routes().length === 1) open(a);
    };
  });
  keymap.bind(digits, { when: "notEditing" });
  return (
    <NavPage title={APP} header={<TermHeader title={APP} />}>
      <View class="flex-1 gap-1 p-1">
        <Text class="text-muted-foreground">{`${t("app.tagline")} — ${t("app.tuiKeys")}`}</Text>
        <Each of={AREAS}>
          {(a, i) => (
            <ListRow plain testID={`nav-${a()}`} label={areaLabel(a())} onPress={() => open(a())} class="px-1">
              <Text class={`${WIDTH.digit} text-muted-foreground`}>{count(i() + 1)}</Text>
              <Text class="flex-1 text-foreground">{areaLabel(a())}</Text>
              <If when={a() === "run" && running()}>
                <Text class="text-primary">● {scrubCommand(running()!.command)}</Text>
              </If>
            </ListRow>
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

/** The place the terminal last showed: a change of it, not its first value, opens a page. */
let opened: Area | null = null;

function TerminalShell() {
  opened = untracked(area);
  // a place chosen elsewhere (the palette, a button) becomes the page on top
  effect(() => {
    const a = area();
    const top = nav.routes()[nav.routes().length - 1];
    if (top.name === "area" && top.area !== a) nav.replace({ name: "area", area: a });
    // from the home list (the palette's `:` then a place): the place opens over it
    else if (top.name === "home" && a !== opened) untracked(() => nav.push({ name: "area", area: a }));
    opened = a;
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
        label: APP,
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

/** The sidebar's width, as the user left it. */
const sidebarWidth = rememberedSize("sidebar.list", 272);

/** A file is being dragged over the window. */
const dragging = signal<{ opens: number; others: number } | null>(null);

function App() {
  init();
  // links into the app (sonde show --app, a shared page) and a pressed notification
  if (platform.idiom !== "terminal") {
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
      // however the app stops, the program it is recording does not outlive it unseen, and its
      // session is a finished file: the system waits 200 ms for this, no longer
      Application.on("quit", () => endForQuit());
    } catch {
      // a host that is not the app's own (a test runner)
    }
    followNotifications((path) => {
      openSession.set(path);
      go("sessions");
    });
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
  return platform.idiom === "terminal" ? <TerminalShell /> : <DesktopShell />;
}

mount(App);
