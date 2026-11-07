// The command palette: every place and action of the app, and the things
// people come back to — recent sessions and commands, endpoints, buckets —
// behind one search field. ⌘K (Ctrl+K) on a desktop, `:` in a terminal.

import { resource, setColorScheme, signal, Text, View } from "nsh";
import { Command, Dialog } from "nsh/base";
import { language, t } from "../i18n";
import { areaLabel, sectionLabel } from "../labels";
import { api } from "../native";
import { command, history, lastRun, openLastRun, running, stop } from "../run";
import { ACTIVITY_TABS, AREAS, chooseBucket, endpoint, endpoints, go, openSession, readOnly, refreshSettings, report, SETTINGS_TABS, sessionsChanged, sessionsTick, settings, STORE_TABS } from "../store";
import type { BucketEntry, SessionInfo } from "../types";
import { Icon, type IconName } from "./icon";
import { Kbd, terminal } from "./kit";
import { keyLabel } from "./keys";

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
  run: () => void;
}

const AREA_ICON = { run: "square-terminal", sessions: "list-clock", store: "database", activity: "activity", settings: "settings" } as const;

function actions(buckets: BucketEntry[]): Action[] {
  const list: Action[] = [];
  AREAS.forEach((a, i) => list.push({ id: `area:${a}`, label: areaLabel(a), group: t("palette.go"), icon: AREA_ICON[a], shortcut: a === "settings" ? "mod+," : a === "activity" ? "mod+shift+a" : `mod+${i + 1}`, run: () => go(a) }));
  if (running()) list.push({ id: "run:stop", label: t("run.stop"), group: t("palette.actions"), icon: "square", run: stop });
  if (lastRun()) list.push({ id: "run:open", label: t("run.lineUp"), group: t("palette.actions"), icon: "list-clock", hint: lastRun()!.command, run: () => openLastRun(go) });

  // the commands run before: the field filled, ready to run again
  history()
    .slice(0, 5)
    .forEach((c, i) =>
      list.push({
        id: `cmd:${i}`,
        label: c,
        group: t("palette.commands"),
        icon: "square-terminal",
        keywords: ["run", "command"],
        run: () => {
          command.set(c);
          go("run");
        },
      }),
    );
  // the newest sessions
  sessionsTick();
  let sessions: SessionInfo[] = [];
  try {
    sessions = api.sessions.list().slice(0, 6);
  } catch {
    // none yet
  }
  sessions.forEach((s) =>
    list.push({
      id: `session:${s.path}`,
      label: s.command ?? s.name,
      group: t("palette.sessions"),
      icon: "list-clock",
      hint: `${s.created.slice(0, 16).replace("T", " ")} · ${s.requests}`,
      run: () => {
        openSession.set(s.path);
        go("sessions");
      },
    }),
  );
  // where requests go
  endpoints().forEach((e) =>
    list.push({
      id: `endpoint:${e.id}`,
      label: e.name,
      group: t("palette.endpoints"),
      icon: "server",
      hint: e.url,
      checked: endpoint() === e.id,
      keywords: [e.vendorLabel, e.id],
      run: () => {
        endpoint.set(e.id);
        chooseBucket(null);
      },
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
  STORE_TABS.forEach((s) => list.push({ id: `store:${s}`, label: `${areaLabel("store")} › ${s === "browse" ? t("nav.objects") : sectionLabel(s)}`, group: t("palette.store"), icon: "database", run: () => go(s) }));
  SETTINGS_TABS.forEach((s) => list.push({ id: `settings:${s}`, label: `${areaLabel("settings")} › ${sectionLabel(s)}`, group: t("palette.settings"), icon: "settings", run: () => go(s) }));
  ACTIVITY_TABS.forEach((s) => list.push({ id: `activity:${s}`, label: `${areaLabel("activity")} › ${sectionLabel(s)}`, group: t("palette.go"), icon: "activity", run: () => go(s) }));
  list.push({
    id: "example",
    label: t("timeline.example"),
    group: t("palette.actions"),
    icon: "scroll-text",
    run: () => {
      try {
        openSession.set(api.sessions.example());
        sessionsChanged();
        go("sessions");
      } catch (e) {
        report(e);
      }
    },
  });
  list.push({ id: "updates", label: t("updates.check"), group: t("palette.actions"), icon: "refresh-cw", keywords: ["upgrade", "version"], run: () => go("settings") });
  list.push({
    id: "read-only",
    label: t("palette.readOnly"),
    group: t("palette.actions"),
    icon: readOnly() ? "lock" : "lock-open",
    checked: readOnly(),
    keywords: ["write", "safety"],
    run: () => {
      try {
        api.setReadOnly(!readOnly());
        refreshSettings();
      } catch (e) {
        report(e);
      }
    },
  });
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

export function Palette() {
  const close = () => paletteOpen.set(false);
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
  const items = () => actions(buckets() ?? []);
  const byId = () => new Map(items().map((a) => [a.id, a]));
  return (
    <Dialog
      open={paletteOpen()}
      onOpenChange={({ open }) => paletteOpen.set(open)}
      animation="fade"
      backdropClass={terminal() ? undefined : "bg-black/25"}
      style={terminal() ? undefined : { marginTop: 96 }}
      class={terminal() ? "w-full" : "w-[600px] max-w-[92%] self-center overflow-hidden rounded-(--radius-card) border border-border bg-popover shadow-2xl"}
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
        searchPrefix={terminal() ? undefined : <Icon name="search" size={16} />}
        searchBarClass={terminal() ? undefined : "h-12 flex-row items-center gap-2.5 border-b border-border px-4"}
        searchClass={terminal() ? "text-foreground" : "flex-1 text-[15px] text-foreground placeholder:text-muted-foreground"}
        listClass={terminal() ? "max-h-[360px]" : "max-h-[400px] p-1.5"}
        itemClass={terminal() ? "px-1 text-foreground focus:bg-accent checked:font-semibold" : "group h-9 rounded-(--radius) px-2.5 focus:bg-selection"}
        groupClass={terminal() ? "px-1 text-muted-foreground" : "px-2.5 pb-1 pt-2.5 text-[11px] font-medium text-muted-foreground"}
        separatorClass="my-1 h-px bg-border"
        renderItem={
          terminal()
            ? undefined
            : (entry) => {
                const a = byId().get(entry.id);
                return (
                  <View class="h-9 flex-row items-center gap-2.5">
                    <Icon name={a?.icon ?? "chevron-right"} size={15} tint={a?.checked ? "primary" : "muted-foreground"} />
                    <Text class={`min-w-0 shrink text-[13px] line-clamp-1 ${a?.id.startsWith("cmd:") ? "font-mono text-xs" : ""} ${a?.checked ? "font-medium text-primary" : "text-popover-foreground"}`}>{entry.label}</Text>
                    <If when={a?.hint}>
                      <Text class="min-w-0 flex-1 font-mono text-[11px] text-muted-foreground line-clamp-1">{a!.hint!}</Text>
                    </If>
                    <If when={!a?.hint}>
                      <View class="flex-1" />
                    </If>
                    <If when={a?.checked}>
                      <Icon name="check" size={14} tint="primary" />
                    </If>
                    <If when={a?.shortcut}>
                      <Kbd keys={a!.shortcut!} />
                    </If>
                  </View>
                );
              }
        }
        empty={<Text class="px-4 py-6 text-center text-[13px] text-muted-foreground">{t("palette.nothing")}</Text>}
        footer={
          terminal() ? undefined : (
            <View class="h-9 flex-row items-center gap-4 border-t border-border px-4">
              <Text class="text-[11px] text-muted-foreground">{`↑↓ ${t("palette.move")}`}</Text>
              <Text class="text-[11px] text-muted-foreground">{`${keyLabel("enter")} ${t("palette.choose")}`}</Text>
              <Text class="text-[11px] text-muted-foreground">{`${keyLabel("escape")} ${t("palette.close")}`}</Text>
            </View>
          )
        }
      />
    </Dialog>
  );
}
