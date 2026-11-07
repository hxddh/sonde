// Settings, in a window of their own on the desktops (⌘, / Ctrl+,): how
// sonde behaves and the endpoints it knows. A terminal shows them as a place.

import { View } from "nsh";
import { Window } from "nsh/desktop";
import { t } from "../i18n";
import { sectionLabel } from "../labels";
import { SETTINGS_TABS, type SettingsTab, settingsTab } from "../store";
import type { IconName } from "../ui/icon";
import { TabBar } from "../ui/tabs";
import { Toaster } from "../ui/toast";
import { Endpoints } from "./Endpoints";
import { SettingsScreen } from "./Settings";

const TAB_ICON: Record<SettingsTab, IconName> = { settings: "sliders-horizontal", endpoints: "server" };

/** The tabs and the page of the one chosen. */
export function SettingsBody() {
  return (
    <View class="flex-1 bg-background" testID="screen-settings-area">
      <TabBar tabs={SETTINGS_TABS} value={settingsTab()} icon={(s) => TAB_ICON[s]} label={sectionLabel} onChange={(s) => settingsTab.set(s)} />
      <View class="flex-1">{settingsTab() === "settings" ? <SettingsScreen /> : <Endpoints />}</View>
    </View>
  );
}

/** Opens the Settings window, or brings it forward. */
export function openSettingsWindow() {
  const open = Window.get("settings");
  if (open) {
    open.show();
    open.focus();
    return;
  }
  Window.open(
    () => (
      <View class="flex-1">
        <SettingsBody />
        <Toaster />
      </View>
    ),
    { id: "settings", title: t("nav.settings"), defaultSize: { width: 780, height: 680 }, minSize: { width: 620, height: 480 }, restoreBounds: true },
  );
}
