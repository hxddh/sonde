// The command palette: every place and action of the app behind one search
// field. ⌘K on a desktop, `:` (or Ctrl+K) in a terminal.

import { setColorScheme, signal } from "nsh";
import { Command, Dialog } from "nsh/base";
import { language, t } from "../i18n";
import { areaLabel, sectionLabel } from "../labels";
import { api } from "../native";
import { lastRun, openLastRun, running, stop } from "../run";
import { AREAS, go, openSession, readOnly, refreshSettings, report, SETTINGS_TABS, settings, STORE_TABS } from "../store";

export const paletteOpen = signal(false);

interface Action {
  id: string;
  label: string;
  group: string;
  shortcut?: string;
  keywords?: string[];
  checked?: boolean;
  run: () => void;
}

function actions(): Action[] {
  const list: Action[] = [];
  AREAS.forEach((a, i) => list.push({ id: `area:${a}`, label: areaLabel(a), group: t("palette.go"), shortcut: a === "settings" ? "mod+," : `mod+${i + 1}`, run: () => go(a) }));
  STORE_TABS.forEach((s) => list.push({ id: `store:${s}`, label: `${areaLabel("store")} › ${sectionLabel(s)}`, group: t("palette.store"), run: () => go(s) }));
  SETTINGS_TABS.forEach((s) => list.push({ id: `settings:${s}`, label: `${areaLabel("settings")} › ${sectionLabel(s)}`, group: t("palette.settings"), run: () => go(s) }));
  if (running()) list.push({ id: "run:stop", label: t("run.stop"), group: t("palette.actions"), run: stop });
  if (lastRun()) list.push({ id: "run:open", label: t("run.lineUp"), group: t("palette.actions"), run: () => openLastRun(go) });
  list.push({
    id: "example",
    label: t("timeline.example"),
    group: t("palette.actions"),
    run: () => {
      openSession.set(api.sessions.example());
      go("sessions");
    },
  });
  list.push({ id: "updates", label: t("updates.check"), group: t("palette.actions"), keywords: ["upgrade", "version"], run: () => go("settings") });
  list.push({
    id: "read-only",
    label: t("palette.readOnly"),
    group: t("palette.actions"),
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
  const items = () => actions();
  return (
    <Dialog open={paletteOpen()} onOpenChange={({ open }) => paletteOpen.set(open)} class="w-[560px] max-w-full self-center rounded-(--radius) border border-border bg-popover shadow-lg" testID="palette-dialog">
      <Command
        testID="palette-command"
        autoFocus
        placeholder={t("palette.placeholder")}
        items={items().map((a) => ({ id: a.id, label: a.label, group: a.group, shortcut: a.shortcut, keywords: a.keywords, checked: a.checked }))}
        onSelect={({ id }) => {
          const a = items().find((x) => x.id === id);
          close();
          a?.run();
        }}
        onCancel={close}
        searchClass="h-10 px-3 text-sm text-foreground"
        listClass="max-h-[360px]"
        itemClass="px-3 py-1.5 text-sm text-foreground highlighted:bg-accent checked:font-semibold"
        groupClass="px-3 pb-1 pt-2 text-xs font-semibold uppercase text-muted-foreground"
      />
    </Dialog>
  );
}
