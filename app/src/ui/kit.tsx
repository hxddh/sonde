// sonde's styled controls, drawn on nsh/base's behavior with the theme's tokens.

import { type JSX, platform, Scroll, split, Text, View } from "nsh";
import { Button as BaseButton, Checkbox as BaseCheckbox, Input, Progress, Select, Switch } from "nsh/base";
import { t } from "../i18n";
import { lastError, notice, showRequest } from "../store";

export const terminal = () => platform.idiom === "terminal";

type Variant = "primary" | "secondary" | "ghost" | "destructive";

const buttonClass: Record<Variant, string> = {
  primary: "bg-primary active:bg-primary/85 disabled:opacity-50",
  secondary: "bg-secondary border border-border active:bg-muted disabled:opacity-50",
  ghost: "active:bg-muted hover:bg-muted disabled:opacity-50",
  destructive: "bg-destructive active:bg-destructive/85 disabled:opacity-50",
};

const buttonText: Record<Variant, string> = {
  primary: "text-primary-foreground",
  secondary: "text-secondary-foreground",
  ghost: "text-foreground",
  destructive: "text-destructive-foreground",
};

export function Button(props: { label: string; onPress?: () => void; variant?: Variant; disabled?: boolean; small?: boolean; testID?: string; class?: string }) {
  return (
    <BaseButton
      testID={props.testID}
      onPress={props.onPress}
      disabled={props.disabled}
      accessibilityLabel={props.label}
      class={`flex-row items-center justify-center rounded-(--radius) ${props.small ? "h-7 px-2.5" : "h-8 px-3.5"} ${buttonClass[props.variant ?? "secondary"]} ${props.class ?? ""}`}
    >
      <Text class={`${props.small ? "text-xs" : "text-sm"} font-medium ${buttonText[props.variant ?? "secondary"]}`}>{props.label}</Text>
    </BaseButton>
  );
}

export function Title(props: { children: JSX.Element; class?: string }) {
  return <Text class={`text-lg font-semibold text-foreground ${props.class ?? ""}`}>{props.children}</Text>;
}

export function Heading(props: { children: JSX.Element }) {
  return <Text class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{props.children}</Text>;
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

export function Field(props: { label: string; value: string; onChange: (v: string) => void; placeholder?: string; testID?: string; mono?: boolean; class?: string }) {
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
          <View class="h-8 flex-row items-center justify-between gap-2 rounded-(--radius) border border-input bg-background px-2.5">
            <Text class="text-sm text-foreground line-clamp-1">
              {title()}
            </Text>
            <Text class="text-xs text-muted-foreground">⌄</Text>
          </View>
        }
      />
    </View>
  );
}

export function Segmented(props: { value: string; items: { value: string; title: string }[]; onChange: (v: string) => void }) {
  return (
    <View class="flex-row self-start rounded-(--radius) bg-muted p-0.5">
      <Each of={props.items}>
        {(item) => (
          <BaseButton
            accessibilityLabel={item().title}
            onPress={() => props.onChange(item().value)}
            selected={props.value === item().value}
            class="rounded-sm px-2.5 py-1 selected:bg-background selected:shadow-sm"
          >
            <Text class={props.value === item().value ? "text-xs font-semibold text-foreground" : "text-xs text-muted-foreground"}>{item().title}</Text>
          </BaseButton>
        )}
      </Each>
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

export function Empty(props: { text: string; children?: JSX.Element }) {
  return (
    <View class="flex-1 items-center justify-center gap-3 p-8">
      <Text class="text-center text-sm text-muted-foreground">{props.text}</Text>
      {props.children}
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

/** The global error: readable, with the request id and a way to the record. */
export function ErrorBanner() {
  return (
    <If when={lastError()}>
      {(e) => (
        <View testID="error-banner" role="alert" class="mx-3 mb-2 flex-row items-center gap-3 rounded-(--radius) border border-destructive/40 bg-destructive/10 px-3 py-2">
          <View class="flex-1 gap-0.5">
            <Text class="text-sm font-medium text-destructive">
              {e().message}
            </Text>
            <If when={e().requestId}>
              <Text class="font-mono text-xs text-muted-foreground">
                {t("common.requestId")}: {e().requestId}
              </Text>
            </If>
          </View>
          <If when={e().recordId !== null && e().recordId !== undefined}>
            <Button small label={t("common.showRequest")} onPress={() => showRequest(e().recordId)} />
          </If>
          <Button small variant="ghost" label={t("common.close")} onPress={() => lastError.set(null)} />
        </View>
      )}
    </If>
  );
}

export function NoticeLine() {
  return (
    <If when={notice()}>
      {(n) => (
        <View class="mx-3 mb-2 self-center rounded-full bg-foreground px-3 py-1">
          <Text class="text-xs text-background">{n()}</Text>
        </View>
      )}
    </If>
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
