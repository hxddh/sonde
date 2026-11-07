// A row of tabs: Store's and Settings' pages. nsh/base's Tabs, an underline
// sliding to the chosen one.

import { type JSX, reduceMotion, Text, View } from "nsh";
import { Tab, Tabs } from "nsh/base";
import { Icon, type IconName } from "./icon";
import { terminal } from "./kit";

export function TabBar<T extends string>(props: { tabs: T[]; value: T; label: (t: T) => string; icon?: (t: T) => IconName; onChange: (t: T) => void; trailing?: JSX.Element }) {
  return (
    <View class="flex-row items-center border-b border-border px-3">
      <Tabs class="flex-1 flex-row flex-wrap gap-0.5" indicatorClass={`bottom-0 h-0.5 rounded-full bg-primary ${reduceMotion() ? "" : "transition-all duration-200"}`}>
        <Each of={props.tabs}>
          {(tab) => (
            <Tab
              testID={`tab-${tab()}`}
              accessibilityLabel={props.label(tab())}
              selected={props.value === tab()}
              onPress={() => props.onChange(tab())}
              class="group flex-row items-center gap-1.5 rounded-t-(--radius-sm) px-2.5 py-2.5 hover:bg-muted/60"
            >
              <If when={props.icon && !terminal()}>
                <Icon name={props.icon!(tab())} size={14} tint={props.value === tab() ? "foreground" : "muted-foreground"} />
              </If>
              <Text class="text-[13px] text-muted-foreground group-selected:font-medium group-selected:text-foreground">{props.label(tab())}</Text>
            </Tab>
          )}
        </Each>
      </Tabs>
      {props.trailing}
    </View>
  );
}
