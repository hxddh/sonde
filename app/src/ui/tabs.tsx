// A row of tabs, the app's own look: the Store's and the Settings' pages.

import { Text, View } from "nsh";
import { Button as BaseButton } from "nsh/base";

export function TabBar<T extends string>(props: { tabs: T[]; value: T; label: (t: T) => string; onChange: (t: T) => void }) {
  return (
    <View class="flex-row flex-wrap gap-1 border-b border-border px-3" role="tablist">
      <Each of={props.tabs}>
        {(tab) => (
          <BaseButton
            testID={`tab-${tab()}`}
            accessibilityLabel={props.label(tab())}
            selected={props.value === tab()}
            onPress={() => props.onChange(tab())}
            class="border-b-2 border-transparent px-2.5 py-2 selected:border-primary"
          >
            <Text class={props.value === tab() ? "text-sm font-medium text-foreground" : "text-sm text-muted-foreground"}>{props.label(tab())}</Text>
          </BaseButton>
        )}
      </Each>
    </View>
  );
}
