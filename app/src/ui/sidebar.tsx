// The desktop sidebar: endpoint, its buckets, and the sections.

import { resource, Scroll, Text, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { sectionLabel } from "../labels";
import { t } from "../i18n";
import { api } from "../native";
import { bucket, chooseBucket, endpoint, go, report, SECTIONS, section } from "../store";
import { EndpointPicker } from "./picker";

export function Sidebar() {
  const buckets = resource(endpoint, async (ep) => {
    try {
      return await api.buckets(ep);
    } catch (e) {
      report(e);
      return [];
    }
  });
  return (
    <View class="w-60 border-r border-sidebar-border bg-sidebar">
      <View class="gap-2 px-3 pb-3 pt-4">
        <View class="flex-row items-baseline gap-2">
          <Text class="text-base font-bold text-sidebar-foreground">sonde</Text>
          <Text class="text-xs text-muted-foreground">{t("app.tagline")}</Text>
        </View>
        <EndpointPicker />
      </View>
      <Scroll class="flex-1" contentClass="gap-0.5 px-2 pb-3">
        <Text class="px-2 pb-1 pt-2 text-xs font-semibold uppercase text-muted-foreground">{t("browse.buckets")}</Text>
        <If when={buckets.state() === "pending"}>
          <Text class="px-2 text-xs text-muted-foreground">{t("common.loading")}</Text>
        </If>
        <Each of={buckets() ?? []} key={(b) => b.name}>
          {(b) => (
            <BaseButton
              testID={`bucket-${b().name}`}
              accessibilityLabel={b().name}
              selected={bucket() === b().name}
              onPress={() => {
                chooseBucket(b().name);
                if (section() === "endpoints" || section() === "settings") go("browse");
              }}
              class="flex-row items-center gap-2 rounded-(--radius) px-2 py-1 hover:bg-sidebar-accent/60 selected:bg-sidebar-accent"
            >
              <Text class="text-xs text-muted-foreground">▤</Text>
              <Text class={bucket() === b().name ? "flex-1 line-clamp-1 text-sm font-medium text-sidebar-accent-foreground" : "flex-1 line-clamp-1 text-sm text-sidebar-foreground"}>
                {b().name}
              </Text>
            </BaseButton>
          )}
        </Each>
        <Text class="px-2 pb-1 pt-4 text-xs font-semibold uppercase text-muted-foreground">sonde</Text>
        <Each of={SECTIONS}>
          {(s) => (
            <BaseButton
              testID={`nav-${s()}`}
              accessibilityLabel={sectionLabel(s())}
              selected={section() === s()}
              onPress={() => go(s())}
              class="rounded-(--radius) px-2 py-1 hover:bg-sidebar-accent/60 selected:bg-sidebar-accent"
            >
              <Text class={section() === s() ? "text-sm font-medium text-sidebar-accent-foreground" : "text-sm text-sidebar-foreground"}>{sectionLabel(s())}</Text>
            </BaseButton>
          )}
        </Each>
      </Scroll>
    </View>
  );
}
