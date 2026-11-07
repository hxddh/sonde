// sonde's styled controls, drawn on nsh/base's behavior with the theme's tokens.

import { type JSX, platform, reduceMotion, Scroll, split, Text, View } from "nsh";
import { Button as BaseButton, Checkbox as BaseCheckbox, Input, Progress, Select, Switch, Tab, Tabs, Tooltip } from "nsh/base";
import { t } from "../i18n";
import { Icon, type IconName, type Tint } from "./icon";
import { keyLabel } from "./keys";

export const terminal = () => platform.idiom === "terminal";

type Variant = "primary" | "secondary" | "outline" | "ghost" | "destructive";

const buttonClass: Record<Variant, string> = {
  primary: "bg-primary hover:bg-primary/90 active:bg-primary/80 disabled:opacity-50",
  secondary: "bg-secondary border border-border hover:bg-muted active:bg-accent disabled:opacity-50",
  outline: "border border-input bg-background hover:bg-muted active:bg-accent disabled:opacity-50",
  ghost: "hover:bg-muted active:bg-accent disabled:opacity-50",
  destructive: "bg-destructive hover:bg-destructive/90 active:bg-destructive/80 disabled:opacity-50",
};

const buttonText: Record<Variant, string> = {
  primary: "text-primary-foreground",
  secondary: "text-secondary-foreground",
  outline: "text-foreground",
  ghost: "text-foreground",
  destructive: "text-destructive-foreground",
};

const iconTint: Record<Variant, Tint> = {
  primary: "primary-foreground",
  secondary: "foreground",
  outline: "foreground",
  ghost: "muted-foreground",
  destructive: "primary-foreground",
};

/** The app's tooltip: a short hint, and the shortcut when there is one. */
export function Tip(props: { text: string; shortcut?: string }) {
  return (
    <Tooltip placement="bottom" class="flex-row items-center gap-2 rounded-(--radius-sm) bg-foreground px-2 py-1 shadow-md transition-opacity duration-100" startingClass="opacity-0" endingClass="opacity-0">
      <Text class="text-xs text-background">{props.text}</Text>
      <If when={props.shortcut}>{(k) => <Text class="font-mono text-[11px] text-background opacity-70">{keyLabel(k())}</Text>}</If>
    </Tooltip>
  );
}

/** A shortcut, as the platform writes it (⌘K, Ctrl+K). */
export function Kbd(props: { keys: string; class?: string }) {
  return <Text class={`font-mono text-[11px] text-muted-foreground ${props.class ?? ""}`}>{keyLabel(props.keys)}</Text>;
}

export function Button(props: {
  label: string;
  onPress?: () => void;
  variant?: Variant;
  disabled?: boolean;
  small?: boolean;
  icon?: IconName;
  /** A hint on hover, with `shortcut`. */
  tip?: string;
  shortcut?: string;
  testID?: string;
  class?: string;
}) {
  const v = () => props.variant ?? "secondary";
  return (
    <BaseButton
      testID={props.testID}
      onPress={props.onPress}
      disabled={props.disabled}
      accessibilityLabel={props.label}
      tooltip={props.tip || props.shortcut ? <Tip text={props.tip ?? props.label} shortcut={props.shortcut} /> : undefined}
      class={`flex-row items-center justify-center gap-1.5 rounded-(--radius) ${props.small ? "h-7 px-2.5" : "h-8 px-3"} ${buttonClass[v()]} ${props.class ?? ""}`}
    >
      <If when={props.icon && !terminal()}>
        <Icon name={props.icon!} size={props.small ? 14 : 15} tint={iconTint[v()]} />
      </If>
      <Text class={`${props.small ? "text-xs" : "text-[13px]"} font-medium ${buttonText[v()]}`}>{props.label}</Text>
    </BaseButton>
  );
}

/** A button that shows only its icon (its label in a terminal), named for screen readers, explained by its tooltip. */
export function IconButton(props: { icon: IconName; label: string; onPress?: () => void; shortcut?: string; disabled?: boolean; selected?: boolean; tint?: Tint; testID?: string; class?: string }) {
  if (terminal()) return <Button small variant="ghost" label={props.label} onPress={props.onPress} disabled={props.disabled} testID={props.testID} />;
  return (
    <BaseButton
      testID={props.testID}
      onPress={props.onPress}
      disabled={props.disabled}
      selected={props.selected}
      accessibilityLabel={props.label}
      tooltip={<Tip text={props.label} shortcut={props.shortcut} />}
      class={`size-7 items-center justify-center rounded-(--radius) hover:bg-muted active:bg-accent selected:bg-accent disabled:opacity-40 ${props.class ?? ""}`}
    >
      <Icon name={props.icon} size={16} tint={props.tint ?? (props.selected ? "foreground" : "muted-foreground")} />
    </BaseButton>
  );
}

export function Title(props: { children: JSX.Element; class?: string }) {
  return <Text class={`text-lg font-semibold text-foreground ${props.class ?? ""}`}>{props.children}</Text>;
}

export function Heading(props: { children: JSX.Element }) {
  return (
    <Text role="header" class="text-xs font-semibold text-muted-foreground">
      {props.children}
    </Text>
  );
}

export function Muted(props: { children: JSX.Element; class?: string }) {
  return <Text class={`text-sm text-muted-foreground ${props.class ?? ""}`}>{props.children}</Text>;
}

export function Mono(props: { children: JSX.Element; class?: string }) {
  return (
    <Text class={`font-mono text-xs text-foreground ${props.class ?? ""}`}>
      {props.children}
    </Text>
  );
}

export function Card(props: { children: JSX.Element; class?: string; testID?: string }) {
  return (
    <View testID={props.testID} class={`gap-2 rounded-(--radius-card) border border-border bg-card p-4 ${props.class ?? ""}`}>
      {props.children}
    </View>
  );
}

export type Tone = "neutral" | "pass" | "fail" | "differs" | "unsupported" | "warn" | "info" | "primary";

const toneClass: Record<Tone, string> = {
  neutral: "bg-muted text-muted-foreground",
  pass: "bg-pass/15 text-pass",
  fail: "bg-fail/15 text-fail",
  differs: "bg-differs/15 text-differs",
  unsupported: "bg-unsupported/15 text-unsupported",
  warn: "bg-warn/15 text-warn",
  info: "bg-info/15 text-info",
  primary: "bg-primary/15 text-primary",
};

export function Badge(props: { label: string; tone?: Tone }) {
  const [bg, fg] = toneClass[props.tone ?? "neutral"].split(" ");
  return (
    <View class={`self-start rounded-full px-2 py-0.5 ${bg}`}>
      <Text class={`text-xs font-medium ${fg}`}>{props.label}</Text>
    </View>
  );
}

export function KeyValue(props: { rows: [string, string][]; mono?: boolean }) {
  return (
    <View class="gap-1">
      <Each of={props.rows}>
        {(row) => (
          <View class="flex-row gap-3">
            <Text class="w-44 text-sm text-muted-foreground line-clamp-1">
              {row()[0]}
            </Text>
            <Text class={`flex-1 text-sm text-foreground ${props.mono ? "font-mono" : ""}`}>
              {row()[1]}
            </Text>
          </View>
        )}
      </Each>
    </View>
  );
}

export function Field(props: { label: string; value: string; onChange: (v: string) => void; onSubmit?: () => void; placeholder?: string; testID?: string; mono?: boolean; class?: string }) {
  return (
    <View class={`gap-1 ${props.class ?? ""}`}>
      <Text class="text-xs font-medium text-muted-foreground">{props.label}</Text>
      <Input
        testID={props.testID}
        value={props.value}
        placeholder={props.placeholder}
        autoCapitalize="none"
        autoCorrect={false}
        onChange={({ value }) => props.onChange(value)}
        onSubmit={props.onSubmit ? () => props.onSubmit?.() : undefined}
        submitBehavior={props.onSubmit ? "submit" : undefined}
        class={`h-8 rounded-(--radius) border border-input bg-background px-2.5 text-sm text-foreground focus:border-ring ${props.mono ? "font-mono" : ""}`}
      />
    </View>
  );
}

export function Toggle(props: { label: string; checked: boolean; onChange: (v: boolean) => void; hint?: string; testID?: string; disabled?: boolean }) {
  return (
    <View class="flex-row items-center gap-3">
      <Switch
        testID={props.testID}
        checked={props.checked}
        disabled={props.disabled}
        accessibilityLabel={props.label}
        onChange={({ checked }) => props.onChange(checked)}
        class="h-5 w-9 justify-center rounded-full bg-input px-0.5"
        checkedStyle={{}}
        trackClass="h-5 w-9 rounded-full bg-input checked:bg-primary"
        thumbClass="h-4 w-4 rounded-full bg-background shadow-sm checked:translate-x-4"
      />
      <View class="flex-1">
        <Text class="text-sm text-foreground">{props.label}</Text>
        <If when={props.hint}>
          <Text class="text-xs text-muted-foreground">{props.hint}</Text>
        </If>
      </View>
    </View>
  );
}

export function Check(props: { label: string; checked: boolean; onChange: (v: boolean) => void; testID?: string }) {
  return (
    <BaseCheckbox
      testID={props.testID}
      checked={props.checked}
      accessibilityLabel={props.label}
      onChange={({ checked }) => props.onChange(checked)}
      class="flex-row items-center gap-2"
      indicatorClass="h-4 w-4 items-center justify-center rounded-sm border border-input checked:bg-primary checked:border-primary"
      indicator={<Text class="text-xs text-primary-foreground">✓</Text>}
    >
      <Text class="text-sm text-foreground">{props.label}</Text>
    </BaseCheckbox>
  );
}

export function Choice(props: { label?: string; value: string | null; items: { value: string; title: string }[]; onChange: (v: string) => void; placeholder?: string; testID?: string; class?: string }) {
  const title = () => props.items.find((i) => i.value === props.value)?.title ?? props.placeholder ?? "";
  return (
    <View class={`gap-1 ${props.class ?? ""}`}>
      <If when={props.label}>
        <Text class="text-xs font-medium text-muted-foreground">{props.label}</Text>
      </If>
      <Select
        testID={props.testID}
        items={props.items}
        value={props.value}
        placeholder={props.placeholder}
        accessibilityLabel={props.label ?? props.placeholder}
        onChange={({ value }) => value !== null && props.onChange(value)}
        popupClass="rounded-(--radius) border border-border bg-popover p-1 shadow-lg"
        itemClass="rounded-sm px-2 py-1.5 text-sm text-popover-foreground focus:bg-accent selected:font-semibold"
        trigger={
          <View class="h-8 flex-row items-center justify-between gap-2 rounded-(--radius) border border-input bg-background px-2.5 hover:border-ring/60">
            <Text class="text-sm text-foreground line-clamp-1">
              {title()}
            </Text>
            {terminal() ? <Text class="text-xs text-muted-foreground">⌄</Text> : <Icon name="chevrons-up-down" size={13} />}
          </View>
        }
      />
    </View>
  );
}

/** One of a few views or modes: Tabs, a pill sliding behind the chosen one. */
export function Segmented(props: { value: string; items: { value: string; title: string; icon?: IconName }[]; onChange: (v: string) => void; testID?: string }) {
  return (
    <Tabs testID={props.testID} class="flex-row self-start rounded-(--radius) bg-muted p-0.5" indicatorClass={`inset-y-0.5 rounded-(--radius-sm) bg-background shadow-sm ${reduceMotion() ? "" : "transition-all duration-200"}`}>
      <Each of={props.items}>
        {(item) => (
          <Tab
            testID={props.testID ? `${props.testID}-${item().value}` : undefined}
            accessibilityLabel={item().title}
            selected={props.value === item().value}
            onPress={() => props.onChange(item().value)}
            class="group flex-row items-center gap-1.5 rounded-(--radius-sm) px-2.5 py-1"
          >
            <If when={item().icon && !terminal()}>
              <Icon name={item().icon!} size={13} tint={props.value === item().value ? "foreground" : "muted-foreground"} />
            </If>
            <Text class="text-xs text-muted-foreground group-selected:font-medium group-selected:text-foreground">{item().title}</Text>
          </Tab>
        )}
      </Each>
    </Tabs>
  );
}

/** A status mark: a small dot in a status color, with its meaning for screen readers. */
export function Dot(props: { tone: "ok" | "miss" | "client" | "server" | "refused" | "live"; label?: string }) {
  const bg = { ok: "bg-status-ok", miss: "bg-status-miss", client: "bg-status-client", server: "bg-status-server", refused: "bg-status-refused", live: "bg-primary" }[props.tone];
  return <View accessibilityLabel={props.label} class={`size-2 rounded-full ${bg}`} />;
}

/** A titled group of settings or facts: a hairline-bounded surface. */
export function Section(props: { title?: string; description?: string; children: JSX.Element; testID?: string; action?: JSX.Element }) {
  return (
    <View testID={props.testID} class="gap-3">
      <If when={props.title}>
        <View class="flex-row items-end gap-3">
          <View class="flex-1 gap-0.5">
            <Text role="header" class="text-[13px] font-semibold text-foreground">
              {props.title}
            </Text>
            <If when={props.description}>
              <Text class="text-xs text-muted-foreground">{props.description}</Text>
            </If>
          </View>
          {props.action}
        </View>
      </If>
      <View class="gap-3 rounded-(--radius-card) border border-border bg-card p-4">{props.children}</View>
    </View>
  );
}

export function Bar(props: { value: number | null; label?: string }) {
  return (
    <View class="gap-1">
      <Progress
        value={props.value ?? undefined}
        indeterminate={props.value === null}
        accessibilityLabel={props.label ?? "progress"}
        class="h-1.5 rounded-full"
        trackClass="h-1.5 rounded-full bg-muted"
        indicatorClass="h-1.5 rounded-full bg-primary"
      />
      <If when={props.label}>
        <Text class="text-xs text-muted-foreground">{props.label}</Text>
      </If>
    </View>
  );
}

/** An empty place: what is missing, and the next step. */
export function Empty(props: { text: string; title?: string; icon?: IconName; children?: JSX.Element; testID?: string }) {
  return (
    <View testID={props.testID} class="flex-1 items-center justify-center gap-3 p-8">
      <If when={props.icon && !terminal()}>
        <View class="mb-1 size-11 items-center justify-center rounded-full bg-muted">
          <Icon name={props.icon!} size={20} />
        </View>
      </If>
      <If when={props.title}>
        <Text class="text-center text-[15px] font-semibold text-foreground">{props.title}</Text>
      </If>
      <Text class="max-w-[380px] text-center text-[13px] text-muted-foreground">{props.text}</Text>
      <If when={props.children}>
        <View class="mt-1 flex-row items-center gap-2">{props.children}</View>
      </If>
    </View>
  );
}

/** A scrolling page body with even padding. */
export function Page(props: { children: JSX.Element; testID?: string }) {
  return (
    <Scroll testID={props.testID} class="flex-1" contentClass="gap-4 p-5">
      {props.children}
    </Scroll>
  );
}

/** A small table of strings (layout tables, headers). Large data uses DataTable. */
export function SimpleTable(props: { columns: string[]; rows: string[][]; mono?: boolean; max?: number }) {
  const [own] = split(props, ["columns"]);
  return (
    <View class="rounded-(--radius) border border-border">
      <View class="flex-row border-b border-border bg-muted px-2 py-1">
        <Each of={own.columns}>{(c) => <Text class="flex-1 text-xs font-semibold text-muted-foreground">{c()}</Text>}</Each>
      </View>
      <Each of={props.rows.slice(0, props.max ?? 200)}>
        {(row) => (
          <View class="flex-row border-b border-border/60 px-2 py-1">
            <Each of={row()}>
              {(cell) => (
                <Text class={`flex-1 line-clamp-2 text-xs text-foreground ${props.mono ? "font-mono" : ""}`}>
                  {cell()}
                </Text>
              )}
            </Each>
          </View>
        )}
      </Each>
      <If when={props.rows.length > (props.max ?? 200)}>
        <Text class="px-2 py-1 text-xs text-muted-foreground">… {props.rows.length - (props.max ?? 200)} more</Text>
      </If>
    </View>
  );
}
