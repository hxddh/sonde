// Store: what is in a bucket. The endpoint's buckets on the left; the tabs
// look at the chosen one: its objects (with layouts decoded), incomplete
// uploads, versions, capacity, layout lenses, and the endpoint's
// compatibility.

import { resource, Scroll, Text, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { t } from "../i18n";
import { sectionLabel } from "../labels";
import { api } from "../native";
import { bucket, chooseBucket, endpoint, go, report, STORE_TABS, type StoreTab, storeTab } from "../store";
import { terminal } from "../ui/kit";
import { TabBar } from "../ui/tabs";
import { Browse } from "./Browse";
import { Capacity } from "./Capacity";
import { Layouts } from "./Layouts";
import { Multipart } from "./Multipart";
import { Probes } from "./Probes";
import { Versions } from "./Versions";

function screen(s: StoreTab) {
  switch (s) {
    case "browse":
      return <Browse />;
    case "multipart":
      return <Multipart />;
    case "versions":
      return <Versions />;
    case "capacity":
      return <Capacity />;
    case "layouts":
      return <Layouts />;
    case "probes":
      return <Probes />;
  }
}

function Buckets() {
  const buckets = resource(endpoint, async (ep) => {
    try {
      return await api.buckets(ep);
    } catch (e) {
      report(e);
      return [];
    }
  });
  return (
    <Scroll class="w-52 border-r border-border" contentClass="gap-0.5 p-2">
      <Text class="px-2 pb-1 pt-1 text-xs font-semibold uppercase text-muted-foreground">{t("browse.buckets")}</Text>
      <If when={buckets.state() === "pending"}>
        <Text class="px-2 text-xs text-muted-foreground">{t("common.loading")}</Text>
      </If>
      <Each of={buckets() ?? []} key={(b) => b.name}>
        {(b) => (
          <BaseButton
            testID={`bucket-${b().name}`}
            accessibilityLabel={b().name}
            selected={bucket() === b().name}
            onPress={() => chooseBucket(b().name)}
            class="flex-row items-center gap-2 rounded-(--radius) px-2 py-1 hover:bg-muted selected:bg-accent"
          >
            <Text class="text-xs text-muted-foreground">▤</Text>
            <Text class={bucket() === b().name ? "flex-1 line-clamp-1 text-sm font-medium text-foreground" : "flex-1 line-clamp-1 text-sm text-foreground"}>{b().name}</Text>
          </BaseButton>
        )}
      </Each>
    </Scroll>
  );
}

export function StoreArea() {
  const tabs = () => <TabBar tabs={STORE_TABS} value={storeTab()} label={(s) => (s === "browse" ? t("nav.objects") : sectionLabel(s))} onChange={(s) => go(s)} />;
  return (
    <If
      when={terminal()}
      else={
        <View class="flex-1 flex-row" testID="screen-store">
          <Buckets />
          <View class="flex-1">
            {tabs()}
            <View class="flex-1">{screen(storeTab())}</View>
          </View>
        </View>
      }
    >
      <View class="flex-1" testID="screen-store">
        {tabs()}
        <View class="flex-1">{screen(storeTab())}</View>
      </View>
    </If>
  );
}
