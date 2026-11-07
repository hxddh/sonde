// The desktop sidebar: the three places, then the endpoint, Settings and
// the palette. Buckets live in Store.

import { Text, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { areaLabel } from "../labels";
import { t } from "../i18n";
import { running } from "../run";
import { type Area, area, go } from "../store";
import { EndpointPicker } from "./picker";
import { paletteOpen } from "./palette";

const GLYPH: Record<Area, string> = { run: "▶", sessions: "≡", store: "▤", settings: "⚙" };

function Item(props: { area: Area; shortcut?: string }) {
  return (
    <BaseButton
      testID={`nav-${props.area}`}
      accessibilityLabel={areaLabel(props.area)}
      selected={area() === props.area}
      onPress={() => go(props.area)}
      class="flex-row items-center gap-2.5 rounded-(--radius) px-2 py-1.5 hover:bg-sidebar-accent/60 selected:bg-sidebar-accent"
    >
      <Text class="w-4 text-center text-sm text-muted-foreground">{GLYPH[props.area]}</Text>
      <Text class={area() === props.area ? "flex-1 text-sm font-medium text-sidebar-accent-foreground" : "flex-1 text-sm text-sidebar-foreground"}>{areaLabel(props.area)}</Text>
      <If when={props.area === "run" && running()}>
        <Text class="text-xs text-primary">●</Text>
      </If>
      <If when={props.shortcut}>{(s) => <Text class="text-xs text-muted-foreground">{s()}</Text>}</If>
    </BaseButton>
  );
}

export function Sidebar() {
  return (
    <View class="w-56 gap-1 border-r border-sidebar-border bg-sidebar px-2 pb-3 pt-4">
      <View class="flex-row items-baseline gap-2 px-2 pb-3">
        <Text class="text-base font-bold text-sidebar-foreground">sonde</Text>
      </View>
      <Item area="run" shortcut="⌘1" />
      <Item area="sessions" shortcut="⌘2" />
      <Item area="store" shortcut="⌘3" />
      <View class="flex-1" />
      <View class="gap-2 px-1">
        <EndpointPicker class="w-full" />
      </View>
      <Item area="settings" shortcut="⌘," />
      <BaseButton testID="palette" accessibilityLabel={t("palette.open")} onPress={() => paletteOpen.set(true)} class="flex-row items-center gap-2.5 rounded-(--radius) px-2 py-1.5 hover:bg-sidebar-accent/60">
        <Text class="w-4 text-center text-sm text-muted-foreground">⌕</Text>
        <Text class="flex-1 text-sm text-sidebar-foreground">{t("palette.open")}</Text>
        <Text class="text-xs text-muted-foreground">⌘K</Text>
      </BaseButton>
    </View>
  );
}
