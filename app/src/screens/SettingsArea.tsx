// Settings: how sonde behaves, the endpoints it knows, and what it did —
// its own requests, its jobs, the audit log of changes.

import { View } from "nsh";
import { sectionLabel } from "../labels";
import { go, SETTINGS_TABS, type SettingsTab, settingsTab } from "../store";
import { TabBar } from "../ui/tabs";
import { Audit } from "./Audit";
import { Endpoints } from "./Endpoints";
import { Jobs } from "./Jobs";
import { Requests } from "./Requests";
import { SettingsScreen } from "./Settings";

function screen(s: SettingsTab) {
  switch (s) {
    case "settings":
      return <SettingsScreen />;
    case "endpoints":
      return <Endpoints />;
    case "requests":
      return <Requests />;
    case "jobs":
      return <Jobs />;
    case "audit":
      return <Audit />;
  }
}

export function SettingsArea() {
  return (
    <View class="flex-1" testID="screen-settings-area">
      <TabBar tabs={SETTINGS_TABS} value={settingsTab()} label={sectionLabel} onChange={(s) => go(s)} />
      <View class="flex-1">{screen(settingsTab())}</View>
    </View>
  );
}
