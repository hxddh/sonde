// sonde's styled controls, drawn on nsh/base's behavior with the theme's tokens.

import { effect, ErrorBoundary, fontScale, type JSX, keymap, platform, Scroll, setAccessibilityFocus, signal, Text, token, View, type ViewRef } from "nsh";
import { Accordion, AccordionHeader, AccordionItem, AccordionPanel, AccordionTrigger, Button as BaseButton, Checkbox as BaseCheckbox, ContextMenu, DataTable, type DataTableRef, type DataTableSort, Dialog, MenuItem, Input, NumberInput, Progress, Select, Sheet, Switch, Tab, Tabs, Toggle as BaseToggle, ToggleGroup, Tooltip } from "nsh/base";
import { t } from "../i18n";
import { separators } from "../format";
import { Icon, type IconName, type Tint } from "./icon";

export const terminal = () => platform.idiom === "terminal";

/** The look of every split's handle (Native Shell's ResizablePanelGroup):
 * a hairline that thickens under the pointer. */
export const SPLIT_HANDLE = "bg-border group-hover:w-[3px] group-hover:bg-ring/60 group-active:w-[3px] group-active:bg-ring";
/** A shortcut as the platform writes it: "mod+shift+k" → "⇧⌘K" on macOS,
 * "Ctrl+Shift+K" elsewhere (Native Shell's keymap). */
export function keyLabel(shortcut: string): string {
  try {
    return keymap.label(shortcut);
  } catch {
    return shortcut;
  }
}

/** The type scale, in points: the `text-caption` … `text-title` classes
 * (nsh.json's `tailwind.fontSize`), for the props that take a number
 * (DataTable's `fontSize`). A desktop app's scale around a 13 pt body. */
export const TYPE = { caption: 11, small: 12, body: 13, heading: 15, title: 20 } as const;

/** A table row's height for the user's text size: rows grow with their
 * text instead of clipping it; a terminal's rows are one cell. */
export const rowHeight = (points: number) => (terminal() ? undefined : Math.round(points * Math.max(1, fontScale())));

/** Floating surfaces (the palette, sheets, cards on hover, menus): on macOS a
 * material over what is behind them (Liquid Glass on macOS 26, a blur
 * before) on a translucent fill; elsewhere, where a backdrop blur is not
 * drawn everywhere, the fill stays opaque so nothing shows through. */
export const material = platform.os === "macos";
export const glass = material ? { backdropFilter: "blur(24px) saturate(1.5)", glassEffect: "regular" as const } : {};
/** The fill of a floating surface: the translucent class where a material
 * is drawn behind it, the opaque one elsewhere. */
export const fill = (translucent: string, opaque: string) => (material ? translucent : opaque);

/** The one treatment of every anchored popup — menus, selects, the endpoint
 * switcher, cards on hover — so they do not drift apart. */
export const POPUP = `rounded-(--radius-card) border border-border ${fill("bg-popover/90", "bg-popover")} p-1 shadow-lg`;
/** The scrim under a modal layer, from the theme (`scrim`, light and dark). */
export const SCRIM = "bg-scrim";
/** Where the keyboard is: one ring on every control that takes focus. */
export const FOCUS = "focus:ring-2 focus:ring-ring/50";
/** The one surface of a group: a hairline card. Cards never nest. */
export const CARD = "rounded-(--radius-card) border border-border bg-card";
/** Every DataTable's look: a hairline header, cells with one inset that
 * lines up with their titles, a hover and a selection, no stripes. */
export const TABLE = {
  header: "bg-card border-b border-border",
  headerCell: "px-2 text-caption font-medium text-muted-foreground",
  cell: "px-2",
  rowHover: "bg-row-hover",
  rowSelected: "bg-selection",
  fixedEdge: "border-r border-border",
} as const;
/** The title bar's height: sheets start below it, so it stays a title bar. */
export const TITLE_BAR = 44;

const PANEL = { narrow: "w-[420px] max-w-[90%]", default: "w-[600px] max-w-[94%]", wide: "w-[760px] max-w-[94%]" } as const;

/** A panel from the window's trailing edge, over a scrim: supplementary work
 * beside what it is about (Activity, a bucket's detail, a form). One width
 * per size, one surface, one header; Escape, the scrim and the close button
 * dismiss it. */
export function SidePanel(props: { open: boolean; onClose: () => void; size?: keyof typeof PANEL; children: JSX.Element; returnFocus?: () => ViewRef | undefined }) {
  return (
    <Sheet
      open={props.open}
      side="right"
      onOpenChange={(e) => !e.open && props.onClose()}
      // a screen reader goes back to what opened it, not to the window's start
      onClose={() => {
        const r = props.returnFocus?.();
        if (r) setAccessibilityFocus(r);
      }}
      popupClass={`${PANEL[props.size ?? "default"]} border-l border-t border-border ${fill("bg-background/90", "bg-background")} shadow-xl`}
      popupStyle={{ marginTop: TITLE_BAR, ...glass }}
      backdropClass={SCRIM}
    >
      {props.children}
    </Sheet>
  );
}

/** A side panel's header: its icon, title and what it is about, the panel's
 * own controls, and Close. */
export function PanelHeader(props: { title: string; icon?: IconName; subtitle?: string; children?: JSX.Element; onClose?: () => void; closeTestID?: string }) {
  return (
    <View class="h-12 shrink-0 flex-row items-center gap-2 border-b border-border ps-4 pe-3">
      <If when={props.icon}>{(i) => <Icon name={i()} size={16} tint="muted-foreground" />}</If>
      <Text role="header" class="text-heading font-semibold text-foreground">
        {props.title}
      </Text>
      <Text class="min-w-0 flex-1 font-mono text-caption text-muted-foreground line-clamp-1">{props.subtitle ?? ""}</Text>
      {props.children}
      <If when={props.onClose}>
        <IconButton icon="x" label={t("common.close")} onPress={() => props.onClose?.()} testID={props.closeTestID} />
      </If>
    </View>
  );
}

/** Four kinds of button: one primary per area, outline by default, ghost in
 * dense rows and toolbars, destructive for what removes data. */
type Variant = "primary" | "outline" | "ghost" | "destructive";

const buttonClass: Record<Variant, string> = {
  primary: "bg-primary hover:bg-primary/90 active:bg-primary/80 disabled:opacity-50",
  outline: "border border-input bg-background hover:bg-muted active:bg-muted disabled:opacity-50",
  ghost: "hover:bg-muted active:bg-muted disabled:opacity-50",
  destructive: "bg-destructive hover:bg-destructive/90 active:bg-destructive/80 disabled:opacity-50",
};

const buttonText: Record<Variant, string> = {
  primary: "font-medium text-primary-foreground",
  outline: "text-foreground",
  ghost: "text-foreground",
  destructive: "font-medium text-destructive-foreground",
};

const iconTint: Record<Variant, Tint> = {
  primary: "primary-foreground",
  outline: "foreground",
  ghost: "muted-foreground",
  destructive: "primary-foreground",
};

/** The app's tooltip: a short hint, and the shortcut when there is one. */
export function Tip(props: { text: string; shortcut?: string }) {
  return (
    <Tooltip placement="bottom" class="flex-row items-center gap-2 rounded-(--radius-sm) bg-foreground px-2 py-1 shadow-md transition-opacity duration-100" startingClass="opacity-0" endingClass="opacity-0">
      <Text class="text-small text-background">{props.text}</Text>
      <If when={props.shortcut}>{(k) => <Text class="font-mono text-caption text-background opacity-70">{keyLabel(k())}</Text>}</If>
    </Tooltip>
  );
}

/** A shortcut, as the platform writes it (⌘K, Ctrl+K). */
export function Kbd(props: { keys: string; class?: string }) {
  return <Text class={`font-mono text-caption text-muted-foreground ${props.class ?? ""}`}>{keyLabel(props.keys)}</Text>;
}

export function Button(props: {
  label: string;
  onPress?: () => void;
  variant?: Variant | "secondary";
  disabled?: boolean;
  small?: boolean;
  icon?: IconName;
  /** A hint on hover, with `shortcut`. */
  tip?: string;
  shortcut?: string;
  testID?: string;
  class?: string;
}) {
  const v = (): Variant => (props.variant === "secondary" || !props.variant ? "outline" : props.variant);
  return (
    <BaseButton
      testID={props.testID}
      onPress={props.onPress}
      disabled={props.disabled}
      accessibilityLabel={props.label}
      tooltip={props.tip || props.shortcut ? <Tip text={props.tip ?? props.label} shortcut={props.shortcut} /> : undefined}
      class={`flex-row items-center justify-center gap-1.5 rounded-(--radius) ${props.small ? "h-7 px-2.5" : "h-8 px-3"} ${buttonClass[v()]} ${FOCUS} ${props.class ?? ""}`}
    >
      <If when={props.icon}>
        <Icon name={props.icon!} size={props.small ? 14 : 15} tint={iconTint[v()]} />
      </If>
      <Text class={`${props.small ? "text-small" : "text-body"} ${buttonText[v()]}`}>{props.label}</Text>
    </BaseButton>
  );
}

/** A button that shows only its icon, named for screen readers (a terminal
 * shows the name in its place), explained by its tooltip. */
export function IconButton(props: { icon: IconName; label: string; onPress?: () => void; shortcut?: string; disabled?: boolean; selected?: boolean; tint?: Tint; testID?: string; class?: string }) {
  return (
    <BaseButton
      testID={props.testID}
      onPress={props.onPress}
      disabled={props.disabled}
      selected={props.selected}
      accessibilityLabel={props.label}
      tooltip={<Tip text={props.label} shortcut={props.shortcut} />}
      class={`size-7 items-center justify-center rounded-(--radius) hover:bg-muted active:bg-muted selected:bg-selection disabled:opacity-40 ${FOCUS} ${props.class ?? ""}`}
    >
      <Icon name={props.icon} size={16} tint={props.tint ?? (props.selected ? "foreground" : "muted-foreground")} />
    </BaseButton>
  );
}

/** A panel's or a page's title: the first of the two heading levels. */
export function Title(props: { children: JSX.Element; class?: string }) {
  return (
    <Text role="header" class={`text-heading font-semibold text-foreground ${props.class ?? ""}`}>
      {props.children}
    </Text>
  );
}

/** A section's title: the second and last heading level (as `Section`'s). */
export function Heading(props: { children: JSX.Element; count?: number }) {
  return (
    <Text role="header" class="text-body font-semibold text-foreground">
      {props.children}
      <If when={props.count !== undefined}>
        <Text class="font-normal text-muted-foreground">{` · ${props.count}`}</Text>
      </If>
    </Text>
  );
}

export function Muted(props: { children: JSX.Element; class?: string }) {
  return <Text class={`text-body text-muted-foreground ${props.class ?? ""}`}>{props.children}</Text>;
}

export function Mono(props: { children: JSX.Element; class?: string }) {
  return (
    <Text class={`font-mono text-small text-foreground ${props.class ?? ""}`}>
      {props.children}
    </Text>
  );
}

const CARD_PAD = { panel: "gap-3 p-4", tile: "gap-1 px-3 py-2.5", none: "overflow-hidden" } as const;

/** The one card: `panel` for a group (the default), `tile` for a figure in a
 * row of them, `none` for a list that draws its own rows. With `onPress` the
 * whole card is a button (hover, focus, a press). Never one inside another. */
export function Card(props: { children: JSX.Element; pad?: keyof typeof CARD_PAD; class?: string; testID?: string; onPress?: () => void; label?: string; selected?: boolean }) {
  const cls = () => `${CARD} ${CARD_PAD[props.pad ?? "panel"]} ${props.class ?? ""}`;
  return (
    <If when={props.onPress} else={<View testID={props.testID} class={cls()}>{props.children}</View>}>
      <BaseButton testID={props.testID} accessibilityLabel={props.label} selected={props.selected} onPress={() => props.onPress?.()} class={`${cls()} items-stretch hover:border-ring/40 hover:bg-muted/40 active:bg-muted selected:border-ring/60 ${FOCUS}`}>
        {props.children}
      </BaseButton>
    </If>
  );
}

/** A row of a list a press opens: hover, pressed, selected and focus, one
 * look in every list (sessions, programs, requests, hits). */
export function ListRow(props: { children: JSX.Element; onPress: () => void; selected?: boolean; label?: string; testID?: string; class?: string }) {
  return (
    <BaseButton
      testID={props.testID}
      accessibilityLabel={props.label}
      selected={props.selected}
      onPress={props.onPress}
      class={`flex-row items-center gap-2 rounded-(--radius-sm) px-2 py-1.5 hover:bg-muted active:bg-muted selected:bg-selection ${FOCUS} ${props.class ?? ""}`}
    >
      {props.children}
    </BaseButton>
  );
}

/** A modal dialog: one surface, one width per size, over the scrim. */
export function Modal(props: { open: boolean; onClose: () => void; size?: "narrow" | "default"; children: JSX.Element; testID?: string }) {
  return (
    <Dialog
      open={props.open}
      onOpenChange={(e) => !e.open && props.onClose()}
      backdropClass={SCRIM}
      testID={props.testID}
      class={`m-auto ${props.size === "narrow" ? "w-[420px]" : "w-[560px]"} max-w-[94%] gap-4 ${CARD} bg-popover p-5 shadow-xl`}
    >
      {props.children}
    </Dialog>
  );
}

/** A section that opens and closes under its title (Native Shell's
 * Accordion of one item: a heading, a button that says whether it is
 * expanded, the panel revealed). */
export function Disclosure(props: { title: string; count?: number; open?: boolean; children: JSX.Element; testID?: string; level?: number; trailing?: JSX.Element }) {
  const value = signal<string[]>(props.open ? ["x"] : []);
  return (
    <Accordion value={value()} onChange={(e) => value.set(e.value)}>
      <AccordionItem value="x" class="gap-2">
        <AccordionHeader level={props.level ?? 3} class="flex-row items-center gap-2">
          <AccordionTrigger testID={props.testID} class={`group flex-1 flex-row items-center gap-1.5 rounded-(--radius-sm) py-0.5 ${FOCUS}`}>
            <View class="group-aria-expanded:rotate-90 motion-safe:transition-transform motion-safe:duration-150">
              <Icon name="chevron-right" size={13} tint="muted-foreground" />
            </View>
            <Text class="text-body font-semibold text-foreground">{props.title}</Text>
            <If when={props.count !== undefined}>
              <Text class="text-body text-muted-foreground">{`· ${props.count}`}</Text>
            </If>
          </AccordionTrigger>
          {props.trailing}
        </AccordionHeader>
        <AccordionPanel reveal accessibilityLabel={props.title}>
          {props.children}
        </AccordionPanel>
      </AccordionItem>
    </Accordion>
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

export function Badge(props: { label: string; tone?: Tone; testID?: string }) {
  const [bg, fg] = toneClass[props.tone ?? "neutral"].split(" ");
  return (
    <View testID={props.testID} accessibilityLabel={props.label} class={`self-start rounded-full px-2 py-0.5 ${bg}`}>
      <Text class={`text-small font-medium ${fg}`}>{props.label}</Text>
    </View>
  );
}

/** Names and their values: muted names, selectable values. `boxed` lists
 * them in a bordered card, compact and monospaced (a request's headers). */
export function KeyValue(props: { rows: [string, string][]; mono?: boolean; boxed?: boolean }) {
  if (props.boxed)
    return (
      <View selectable class="overflow-hidden rounded-(--radius) border border-border bg-card">
        <Each of={props.rows}>
          {(row, i) => (
            <View class={`flex-row gap-3 px-2.5 py-1 ${i() > 0 ? "border-t border-border" : ""}`}>
              <Text class="w-[136px] shrink-0 font-mono text-caption text-muted-foreground line-clamp-1">{row()[0]}</Text>
              <Text class={`min-w-0 flex-1 text-caption text-foreground ${props.mono === false ? "" : "font-mono"}`}>{row()[1]}</Text>
            </View>
          )}
        </Each>
      </View>
    );
  return (
    <View class="gap-1">
      <Each of={props.rows}>
        {(row) => (
          <View class="flex-row gap-3">
            <Text class="w-44 text-body text-muted-foreground line-clamp-1">
              {row()[0]}
            </Text>
            <Text class={`flex-1 text-body text-foreground ${props.mono ? "font-mono" : ""}`}>
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
      <Text class="text-small font-medium text-muted-foreground">{props.label}</Text>
      <Input
        testID={props.testID}
        accessibilityLabel={props.label}
        value={props.value}
        placeholder={props.placeholder}
        autoCapitalize="none"
        autoCorrect={false}
        onChange={({ value }) => props.onChange(value)}
        onSubmit={props.onSubmit ? () => props.onSubmit?.() : undefined}
        submitBehavior={props.onSubmit ? "submit" : undefined}
        class={`h-8 rounded-(--radius) border border-input bg-background px-2.5 text-body text-foreground focus:border-ring ${FOCUS} ${props.mono ? "font-mono" : ""}`}
      />
    </View>
  );
}

/** The one search field: a magnifier, the text, a clear button while it
 * holds any, and what the place puts after it (a "search everywhere"). */
export function SearchField(props: { value: string; onChange: (v: string) => void; onSubmit?: () => void; placeholder: string; label?: string; testID?: string; class?: string; children?: JSX.Element }) {
  return (
    <View class={`h-8 flex-row items-center gap-2 rounded-(--radius) border border-input bg-background px-2.5 focus-within:border-ring focus-within:ring-2 focus-within:ring-ring/50 ${props.class ?? ""}`}>
      <Icon name="search" size={14} tint="muted-foreground" />
      <Input
        testID={props.testID}
        value={props.value}
        placeholder={props.placeholder}
        autoCapitalize="none"
        autoCorrect={false}
        onChange={({ value }) => props.onChange(value)}
        onSubmit={props.onSubmit ? () => props.onSubmit?.() : undefined}
        submitBehavior={props.onSubmit ? "submit" : undefined}
        accessibilityLabel={props.label ?? props.placeholder}
        class="min-w-0 flex-1 text-body text-foreground placeholder:text-muted-foreground"
      />
      <If when={props.value}>
        <IconButton icon="x" label={t("common.clear")} onPress={() => props.onChange("")} class="size-5" />
      </If>
      {props.children}
    </View>
  );
}

/** A number to type or step (Native Shell's NumberInput: arrows and the
 * buttons step it, screen readers hear a spin button); `null` is empty, for
 * a number that may be unset. */
export function NumberField(props: { label: string; value: number | null; onChange: (v: number | null) => void; min?: number; max?: number; step?: number; placeholder?: string; suffix?: string; testID?: string; class?: string; bare?: boolean }) {
  const input = (
    <NumberInput
      testID={props.testID}
      accessibilityLabel={props.label}
      value={props.value}
      min={props.min}
      max={props.max}
      step={props.step ?? 1}
      placeholder={props.placeholder}
      onChange={({ value }) => props.onChange(value)}
      controlsRight
      // a field like every other: a border, a ring when focused, steppers at its end
      class={terminal() ? undefined : "h-8 flex-row items-center overflow-hidden rounded-(--radius) border border-input bg-background focus-within:border-ring"}
      inputClass={terminal() ? undefined : "min-w-0 flex-1 px-2.5 text-body tabular-nums text-foreground placeholder:text-muted-foreground"}
      controlsClass={terminal() ? undefined : "h-full border-l border-input"}
      incrementButtonClass={terminal() ? undefined : "flex-1 items-center justify-center px-1.5 hover:bg-muted active:bg-accent"}
      decrementButtonClass={terminal() ? undefined : "flex-1 items-center justify-center px-1.5 hover:bg-muted active:bg-accent"}
      incrementButton={terminal() ? undefined : <View style={{ transform: [{ rotate: "180deg" }] }}><Icon name="chevron-down" size={11} /></View>}
      decrementButton={terminal() ? undefined : <Icon name="chevron-down" size={11} />}
      suffix={props.suffix ? <Text class="pr-2 text-small text-muted-foreground">{props.suffix}</Text> : undefined}
    />
  );
  if (props.bare) return <View class={props.class}>{input}</View>;
  return (
    <View class={`gap-1 ${props.class ?? ""}`}>
      <Text class="text-small font-medium text-muted-foreground">{props.label}</Text>
      {input}
    </View>
  );
}

/** A setting that is on or off, its label (and hint) the switch's own: a
 * press on the words turns it too, and screen readers say them with it. */
export function SwitchRow(props: { label: string; checked: boolean; onChange: (v: boolean) => void; hint?: string; testID?: string; disabled?: boolean }) {
  return (
    <Switch
      testID={props.testID}
      checked={props.checked}
      disabled={props.disabled}
      accessibilityLabel={props.label}
      onChange={({ checked }) => props.onChange(checked)}
      class={`flex-row items-center gap-3 rounded-(--radius-sm) disabled:opacity-50 ${FOCUS}`}
      trackClass="h-5 w-9 shrink-0 rounded-full bg-input px-0.5 checked:bg-primary motion-safe:transition-colors"
      thumbClass="h-4 w-4 rounded-full bg-background shadow-sm checked:translate-x-4 motion-safe:transition-transform"
    >
      <View class="flex-1">
        <Text class="text-body text-foreground">{props.label}</Text>
        <If when={props.hint}>
          <Text class="text-small text-muted-foreground">{props.hint}</Text>
        </If>
      </View>
    </Switch>
  );
}

export function Check(props: { label: string; checked: boolean; onChange: (v: boolean) => void; testID?: string }) {
  return (
    <BaseCheckbox
      testID={props.testID}
      checked={props.checked}
      accessibilityLabel={props.label}
      onChange={({ checked }) => props.onChange(checked)}
      class={`flex-row items-center gap-2 rounded-(--radius-sm) ${FOCUS}`}
      indicatorClass="h-4 w-4 items-center justify-center rounded-sm border border-input checked:bg-primary checked:border-primary"
      indicator={<Text class="text-small text-primary-foreground">✓</Text>}
    >
      <Text class="text-body text-foreground">{props.label}</Text>
    </BaseCheckbox>
  );
}

export function Choice(props: { label?: string; value: string | null; items: { value: string; title: string }[]; onChange: (v: string) => void; placeholder?: string; testID?: string; class?: string }) {
  const title = () => props.items.find((i) => i.value === props.value)?.title ?? props.placeholder ?? "";
  return (
    <View class={`gap-1 ${props.class ?? ""}`}>
      <If when={props.label}>
        <Text class="text-small font-medium text-muted-foreground">{props.label}</Text>
      </If>
      <Select
        testID={props.testID}
        items={props.items}
        value={props.value}
        placeholder={props.placeholder}
        accessibilityLabel={props.label ?? props.placeholder}
        onChange={({ value }) => value !== null && props.onChange(value)}
        popupClass={POPUP}
        popupStyle={glass}
        itemClass="rounded-sm px-2 py-1.5 text-body text-popover-foreground focus:bg-accent selected:font-semibold"
        trigger={
          <View class="h-8 flex-row items-center justify-between gap-2 rounded-(--radius) border border-input bg-background px-2.5 hover:border-ring/60">
            <Text class="text-body text-foreground line-clamp-1">
              {title()}
            </Text>
            <Icon name="chevrons-up-down" size={13} />
          </View>
        }
      />
    </View>
  );
}

/** One of a few views or modes: Tabs, a pill sliding behind the chosen one. */
export function Segmented(props: { value: string; items: { value: string; title: string; icon?: IconName }[]; onChange: (v: string) => void; testID?: string; class?: string }) {
  return (
    <Tabs testID={props.testID} class={`flex-row rounded-(--radius) bg-muted p-0.5 ${props.class ?? "self-start"}`} indicatorClass="inset-y-0.5 rounded-(--radius-sm) bg-background shadow-sm motion-safe:transition-all motion-safe:duration-200">
      <Each of={props.items} key={(i) => i.value}>
        {(item) => (
          <Tab
            testID={props.testID ? `${props.testID}-${item().value}` : undefined}
            accessibilityLabel={item().title}
            selected={props.value === item().value}
            onPress={() => props.onChange(item().value)}
            class={`group flex-row items-center gap-1.5 rounded-(--radius-sm) px-2.5 py-1 ${FOCUS}`}
          >
            <If when={item().icon}>
              <Icon name={item().icon!} size={13} tint={props.value === item().value ? "foreground" : "muted-foreground"} />
            </If>
            <Text class="text-small text-muted-foreground group-selected:font-medium group-selected:text-foreground">{item().title}</Text>
          </Tab>
        )}
      </Each>
    </Tabs>
  );
}

/** One value of a few, as a row of toggles (Native Shell's ToggleGroup:
 * arrow keys move between them, screen readers hear toggle buttons). For a
 * value; `Segmented` (tabs) switches what a place shows. */
export function Options(props: { value: string; items: { value: string; title: string; icon?: IconName }[]; onChange: (v: string) => void; testID?: string; class?: string }) {
  return (
    <ToggleGroup testID={props.testID} class={`flex-row rounded-(--radius) bg-muted p-0.5 ${props.class ?? "self-start"}`}>
      <Each of={props.items} key={(i) => i.value}>
        {(item) => (
          <BaseToggle
            testID={props.testID ? `${props.testID}-${item().value}` : undefined}
            accessibilityLabel={item().title}
            pressed={props.value === item().value}
            onChange={() => props.onChange(item().value)}
            class={`group flex-row items-center gap-1.5 rounded-(--radius-sm) px-2.5 py-1 aria-pressed:bg-background aria-pressed:shadow-sm ${FOCUS}`}
          >
            <If when={item().icon}>
              <Icon name={item().icon!} size={13} tint={props.value === item().value ? "foreground" : "muted-foreground"} />
            </If>
            <Text class="text-small text-muted-foreground group-aria-pressed:font-medium group-aria-pressed:text-foreground">{item().title}</Text>
          </BaseToggle>
        )}
      </Each>
    </ToggleGroup>
  );
}

/** A status mark: a small dot in a status color, with its meaning for screen readers. */
export function Dot(props: { tone: "ok" | "miss" | "client" | "server" | "refused" | "live"; label?: string }) {
  const bg = { ok: "bg-status-ok", miss: "bg-status-miss", client: "bg-status-client", server: "bg-status-server", refused: "bg-status-refused", live: "bg-primary" }[props.tone];
  return <View accessibilityLabel={props.label} class={`size-2 rounded-full ${bg}`} />;
}

/** A titled group of settings or facts: a hairline-bounded surface. */
export function Section(props: { title?: string; description?: string; children: JSX.Element; testID?: string; action?: JSX.Element; plain?: boolean }) {
  return (
    <View testID={props.testID} class="gap-3">
      <If when={props.title}>
        <View class="flex-row items-end gap-3">
          <View class="flex-1 gap-0.5">
            <Text role="header" class="text-body font-semibold text-foreground">
              {props.title}
            </Text>
            <If when={props.description}>
              <Text class="text-small text-muted-foreground">{props.description}</Text>
            </If>
          </View>
          {props.action}
        </View>
      </If>
      <If when={!props.plain} else={props.children}>
        <View class={`gap-3 ${CARD} p-4`}>{props.children}</View>
      </If>
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
        <Text class="text-small text-muted-foreground">{props.label}</Text>
      </If>
    </View>
  );
}

/** An empty place: what is missing, and the next step. */
export function Empty(props: { text: string; title?: string; icon?: IconName; children?: JSX.Element; testID?: string }) {
  return (
    <View testID={props.testID} class="flex-1 items-center justify-center gap-3 p-8">
      <If when={props.icon}>
        <View class="mb-1 size-11 items-center justify-center rounded-full bg-muted">
          <Icon name={props.icon!} size={20} />
        </View>
      </If>
      <If when={props.title}>
        <Text class="text-center text-heading font-semibold text-foreground">{props.title}</Text>
      </If>
      <Text class="max-w-[380px] text-center text-body text-muted-foreground">{props.text}</Text>
      <If when={props.children}>
        <View class="mt-1 flex-row items-center gap-2">{props.children}</View>
      </If>
    </View>
  );
}

/** Something on its way: one look for every wait in the app — a soft pulse
 * (still with reduced motion) and what is being waited for. */
export function Loading(props: { text?: string; class?: string; testID?: string }) {
  return (
    <View testID={props.testID ?? "loading"} class={`flex-row items-center gap-2 ${props.class ?? ""}`}>
      <View class="size-2 rounded-full bg-muted-foreground/60 motion-safe:animate-pulse" />
      <Text class="text-small text-muted-foreground">{props.text ?? t("common.loading")}</Text>
    </View>
  );
}

/** Where a session will be while it is read: its header, findings and rows, in outline. */
export function SessionSkeleton(props: { rows?: number }) {
  const bar = (w: string, h = "h-3") => <View class={`${h} ${w} rounded-(--radius-sm) bg-muted`} />;
  return (
    <View testID="session-reading" accessibilityLabel={t("sessions.reading")} class="flex-1 gap-3 px-5 pt-4">
      <View class="gap-2">
        {bar("w-56", "h-4")}
        {bar("w-80")}
      </View>
      <View class="h-16 rounded-(--radius-card) bg-muted/60" />
      <View class="gap-2 pt-1">
        <Each of={Array.from({ length: props.rows ?? 8 }, (_, i) => i)} key={(i) => i}>
          {(i) => (
            <View class="flex-row items-center gap-3">
              {bar("w-16")}
              {bar(i() % 3 === 0 ? "w-72" : i() % 3 === 1 ? "w-96" : "w-60")}
              <View class="flex-1" />
              {bar("w-12")}
            </View>
          )}
        </Each>
      </View>
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

/** What a sortable cell sorts by: its number where it reads as one
 * ("1.5 d", "23 h", "176 MiB", "12%", "3 · 172 MiB" by its first figure, in
 * the units it names), else its text. */
export function sortValue(text: string): number | string {
  // numbers as this locale writes them (1.234,5 in German, 1 234,5 in French)
  const { group, decimal } = separators();
  const m = /^\s*[<>~+\u2212-]?\s*([\d.,\u00a0\u202f' ]*\d)\s*(ms|s|min|h|d|B|KiB|MiB|GiB|TiB|PiB|%|×|毫秒|秒|分钟|小时|天)?/.exec(text);
  if (!m) return text.toLowerCase();
  const digits = m[1].split(group).join("").replace(/[\u00a0\u202f' ]/g, "").split(decimal).join(".");
  const n = Number(digits);
  if (!Number.isFinite(n)) return text.toLowerCase();
  const unit: Record<string, number> = { ms: 1, s: 1e3, min: 6e4, h: 3.6e6, d: 8.64e7, 毫秒: 1, 秒: 1e3, 分钟: 6e4, 小时: 3.6e6, 天: 8.64e7, B: 1, KiB: 1024, MiB: 1024 ** 2, GiB: 1024 ** 3, TiB: 1024 ** 4, PiB: 1024 ** 5 };
  const sign = /^\s*[\u2212-]/.test(text) ? -1 : 1;
  return sign * n * (unit[m[2] ?? ""] ?? 1);
}

/** Rows in the order a table's sort says (Native Shell reports the sort; the
 * app orders the rows): by `value` of the sorted column, numbers before text. */
export function sortRows<T>(rows: readonly T[], sort: DataTableSort | null, value: (row: T, key: string) => number | string): T[] {
  if (!sort) return [...rows];
  const dir = sort.direction === "descending" ? -1 : 1;
  return [...rows].sort((a, b) => {
    const x = value(a, sort.key);
    const y = value(b, sort.key);
    const c = typeof x === "number" && typeof y === "number" ? x - y : typeof x === "number" ? -1 : typeof y === "number" ? 1 : String(x).localeCompare(String(y), undefined, { numeric: true });
    return c * dir;
  });
}

/** Rows of text as a table: Native Shell's DataTable (virtual, so every row,
 * sortable by a press on a title: by number where a cell reads as one), each
 * column as wide as what it holds. `max` is how many rows show before it scrolls. */
export function SimpleTable(props: { columns: string[]; rows: string[][]; mono?: boolean; max?: number }) {
  const sort = signal<DataTableSort | null>(null);
  // the widths measured by Native Shell once the rows are in (CJK, mono and proportional alike)
  let table: DataTableRef | undefined;
  effect(() => {
    props.rows;
    if (terminal() || !table) return;
    const t = table;
    setTimeout(() => props.columns.forEach((_, c) => void t.fitColumn(`c${c}`).catch(() => {})), 0);
  });
  const per = () => (props.mono ? 7.4 : 6.6);
  const columns = () =>
    props.columns.map((title, c) => {
      const longest = Math.max(title.length, ...props.rows.slice(0, 400).map((r) => (r[c] ?? "").length));
      return { key: `c${c}`, title, width: Math.round(Math.min(360, Math.max(56, longest * per() + 20))), sortable: true };
    });
  const data = () => sortRows(props.rows, sort(), (r, key) => sortValue(r[Number(key.slice(1))] ?? "")).map((r) => Object.fromEntries(r.map((cell, c) => [`c${c}`, cell])));
  const shown = () => Math.min(props.rows.length, props.max ?? 12);
  return (
    <DataTable
      ref={(r) => (table = r)}
      columns={columns()}
      data={data()}
      sort={sort()}
      onSortChange={({ sort: next }) => sort.set(next)}
      fontSize={TYPE.small}
      fontFamily={props.mono ? token("font-mono") : undefined}
      rowHeight={rowHeight(26)}
      classes={terminal() ? undefined : TABLE}
      class={CARD}
      style={terminal() ? undefined : { height: (shown() + 1) * (rowHeight(26) ?? 26) + 2 }}
    />
  );
}

/** A place that failed to draw says so, and can try again, instead of
 * leaving the window blank; the rest of the app keeps working. */
export function Recoverable(props: { children: JSX.Element }) {
  return (
    <ErrorBoundary
      fallback={(error, reset) => (
        <Empty icon="triangle-alert" title={t("error.placeTitle")} text={error instanceof Error ? error.message : String(error)} testID="place-error">
          <Button variant="outline" icon="refresh-cw" label={t("error.retry")} onPress={reset} />
        </Empty>
      )}
    >
      {props.children}
    </ErrorBoundary>
  );
}

/** What a chart's colors stand for: a swatch (its `bg-*` class) and a name
 * each, in a row that wraps. One legend for every chart. */
export function Legend(props: { items: { label: string; swatch: string }[]; class?: string; square?: boolean }) {
  return (
    <View class={`flex-row flex-wrap gap-x-4 gap-y-1 ${props.class ?? ""}`}>
      <Each of={props.items} key={(it) => it.label}>
        {(it) => (
          <View class="flex-row items-center gap-1.5">
            <View class={`size-2 ${props.square ? "rounded-sm" : "rounded-full"} ${it().swatch}`} />
            <Text class="text-caption text-muted-foreground">{it().label}</Text>
          </View>
        )}
      </Each>
    </View>
  );
}

/** A right press (or Shift+F10) on `children` offers `actions` in Native
 * Shell's own menu: for where the system has no menus to pop up (a
 * terminal, the web). Elsewhere the system's menu does it and this is only
 * `children`. */
export function InWindowMenu(props: { enabled: boolean; actions: () => { label: string; icon?: IconName; run: () => void }[]; class?: string; children: JSX.Element }) {
  return (
    <If when={props.enabled} else={props.children}>
      <ContextMenu class={props.class} trigger={props.children} popupClass={`${POPUP} min-w-48`}>
        <Each of={props.actions()}>
          {(a) => (
            <MenuItem onSelect={a().run} class="flex-row items-center gap-2 rounded-(--radius-sm) px-2 py-1 focus:bg-accent">
              <If when={a().icon}>{(_) => <Icon name={a().icon!} size={14} tint="muted-foreground" />}</If>
              <Text class="text-body text-foreground">{a().label}</Text>
            </MenuItem>
          )}
        </Each>
      </ContextMenu>
    </If>
  );
}
