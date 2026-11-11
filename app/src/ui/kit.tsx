// sonde's styled controls, drawn on nsh/base's behavior with the theme's tokens.

import { effect, ErrorBoundary, screenReaderEnabled, fontScale, type JSX, keymap, platform, Scroll, setAccessibilityFocus, signal, Text, token, View, type ViewRef } from "nsh";
import { Accordion, AccordionHeader, AccordionItem, AccordionPanel, AccordionTrigger, Button as BaseButton, Checkbox as BaseCheckbox, ContextMenu, DataTable, type DataTableRef, type DataTableSort, Dialog, MenuItem, Input, NumberInput, Popover, Progress, Radio, RadioGroup, Select, Sheet, Switch, Tab, Table, TableBody, TableCell, TableRow, Tabs, Tooltip } from "nsh/base";
import { t } from "../i18n";
import { count, middle, separators } from "../format";
import { isAvailable, Menu } from "nsh/desktop";
import { Icon, type IconName, iconSvg, type Tint } from "./icon";

export const terminal = () => platform.idiom === "terminal";
/** Figures as text: a terminal, or a screen reader (a chart's table reads row by row, its picture not at all). */
export const textual = () => terminal() || screenReaderEnabled();

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
/** An icon's three sizes, in points, and no others: `small` beside caption
 * text and at a row's end, `default` (what `Icon` draws without a size) in
 * buttons, rows and notices, `large` in an icon tile. */
export const ICON = { small: 12, default: 16, large: 20 } as const;
/** Weight that is not a heading's: a status word, a counter, a mark in a
 * chart. A heading is `Title` or `Heading`; text set heavier in a page is
 * `Strong`, and this class only where a prop takes classes, not children
 * (lint-kit asks each use for an `emphasis:` comment saying why). */
export const EMPHASIS = "font-semibold";

/** A table row's height for the user's text size: rows grow with their
 * text instead of clipping it; a terminal's rows are one cell. */
/** Every table's rows, three heights: a table of rows to press (30), a short
 * summary table or program output (26), a row of two lines (44). */
export const ROW = { table: 30, dense: 26, twoLine: 44 } as const;
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
/** Where the keyboard is: one ring on every control that takes focus, shown
 * for focus the keyboard moved (Native Shell's `focus-visible:`), never after
 * a click. A text field shows it whenever it has focus, as a browser's does. */
export const FOCUS = "focus-visible:ring-2 focus-visible:ring-ring/50";
/** The same ring around a control whose focus is a text input inside it (a
 * field's box, the endpoint switcher): it shows while that input has focus. */
export const FOCUS_WITHIN = "focus-within:ring-2 focus-within:ring-ring/50";
/** The one surface of a group: a hairline card. Cards never nest. */
export const CARD = "rounded-(--radius-card) border border-border bg-card";
/** Two control heights, and nothing between: h-8 for a field, a choice or a
 * button; h-7 for a small button, an icon button or a row of segments (each
 * segment h-6 inside its track). Controls in one row share their middle. */
export const CONTROL = {
  field: "h-8",
  small: "h-7",
  // off-scale: a segment (h-6) sits 2 pt inside its track (h-7)
  track: "h-7 shrink-0 flex-row items-center rounded-(--radius) bg-muted p-0.5",
  segment: "h-6 flex-row items-center gap-1 rounded-(--radius-sm) px-2",
  /** A small outline button drawn by a component of its own (a picker's trigger). */
  trigger: "h-7 flex-row items-center gap-1 rounded-(--radius) border border-input bg-background px-2 hover:bg-muted",
  /** A field's box: with `CONTROL.field` or `.small`, around an input and what stands before or after it. */
  box: `flex-row items-center gap-2 overflow-hidden rounded-(--radius) border border-input bg-background px-2 focus-within:border-ring ${FOCUS_WITHIN}`,
} as const;
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
/** The one width of a settings page, its forms and lists: Settings and Endpoints line up. */
export const FORM = "w-full max-w-[720px]";
/** The one width of a centered column to read (the welcome, an empty Run's intro). */
export const READING = "w-full max-w-[560px]";
/** Every size at which a layout changes, and no others (lint-kit fails a
 * number compared with a width or a height anywhere else). Two widths: from
 * `toolbar` the window's toolbar shows its wide controls (the endpoint's
 * name in full, the search field rather than its button); from `inspector`
 * a pane holds an inspector beside what it details (under it, the inspector
 * comes in from the edge, and rows drop what ranks last). Two heights of a
 * session's pane: under `short` the shape strip goes and the findings keep
 * to a line or two; from `tall` each finding has its lines in full. */
export const BREAK = { toolbar: 1040, inspector: 900, short: 600, tall: 880 } as const;
/** A row under the pointer, pressed, and chosen: one look in every list, tree and menu. */
const ROW_STATE = "hover:bg-row-hover active:bg-muted selected:bg-selection";
/** The row's look for the controls that take classes, not children: a tree's
 * items (the sidebar's buckets) and the palette's. Rows of a list are `ListRow`. */
export const ROWS = {
  tree: `group h-7 rounded-(--radius-sm) ${ROW_STATE} focus:bg-row-hover disabled:opacity-60`,
  palette: `group ${CONTROL.field} rounded-(--radius-sm) px-2 focus:bg-selection`,
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
      <If when={props.icon}>{(i) => <Icon name={i()} tint="muted-foreground" />}</If>
      <Title>{props.title}</Title>
      <Path path={props.subtitle ?? ""} max={56} class="min-w-0 flex-1 text-muted-foreground" />
      {props.children}
      <If when={props.onClose}>
        <IconButton icon="x" label={t("common.close")} onPress={() => props.onClose?.()} testID={props.closeTestID} />
      </If>
    </View>
  );
}

/** The head of a list in the sidebar: what it lists, and its few commands. */
export function SideHeader(props: { title: string; children?: JSX.Element }) {
  return (
    <View class="h-9 shrink-0 flex-row items-center gap-1 pl-4 pr-2">
      <Heading quiet class="min-w-0 flex-1 line-clamp-1">
        {props.title}
      </Heading>
      {props.children}
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
      class={`flex-row items-center justify-center gap-1 rounded-(--radius) ${props.small ? "h-7 px-2" : "h-8 px-3"} ${buttonClass[v()]} ${FOCUS} ${props.class ?? ""}`}
    >
      <If when={props.icon}>
        <Icon name={props.icon!} tint={iconTint[v()]} />
      </If>
      <Text class={`${props.small ? "text-small" : "text-body"} ${buttonText[v()]}`}>{props.label}</Text>
    </BaseButton>
  );
}

/** A button that shows only its icon, named for screen readers (a terminal
 * shows the name in its place), explained by its tooltip. */
export function IconButton(props: { icon: IconName; label: string; onPress?: () => void; shortcut?: string; /** why it does nothing now, in its tooltip */ tip?: string; disabled?: boolean; selected?: boolean; tint?: Tint; testID?: string; class?: string; /** a small count at its corner (jobs running) */ badge?: string; /** at a field's end, inside it: as high as the field, no height of its own */ inField?: boolean }) {
  return (
    <BaseButton
      testID={props.testID}
      onPress={props.onPress}
      disabled={props.disabled}
      selected={props.selected}
      accessibilityLabel={props.label}
      tooltip={<Tip text={props.tip ?? props.label} shortcut={props.tip ? undefined : props.shortcut} />}
      class={`${props.inField ? "-mr-2 w-7 shrink-0 self-stretch" : "size-7 rounded-(--radius)"} items-center justify-center hover:bg-muted active:bg-muted selected:bg-selection disabled:opacity-40 ${FOCUS} ${props.class ?? ""}`}
    >
      <Icon name={props.icon} tint={props.tint ?? (props.selected ? "foreground" : "muted-foreground")} />
      <If when={props.badge}>
        <View class="absolute -right-1.5 -top-1 h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1">
          <Text class={`tabular-nums text-caption ${EMPHASIS} leading-4 text-primary-foreground`}>{props.badge}</Text>
        </View>
      </If>
    </BaseButton>
  );
}

/** A panel's or a page's title: the first of the two heading levels. `page`
 * is the one title of a page that is only an introduction (the welcome, an
 * empty Run); `mono` for a title that is a command, a key or a bucket. A
 * terminal has one weight and no sizes. */
export function Title(props: { children: JSX.Element; page?: boolean; /** at the body's size: a title that is a long command */ small?: boolean; mono?: boolean; lines?: 1 | 2; center?: boolean; class?: string; testID?: string }) {
  return (
    <Text
      testID={props.testID}
      role="header"
      class={`${terminal() ? "" : props.page ? "text-title" : props.small ? "text-body" : "text-heading"} font-semibold text-foreground ${props.mono ? "font-mono" : ""} ${props.center ? "text-center" : ""} ${props.lines === 1 ? "line-clamp-1" : props.lines === 2 ? "line-clamp-2" : ""} ${props.class ?? ""}`}
    >
      {props.children}
    </Text>
  );
}

/** A section's title: the second and last heading level (as `Section`'s),
 * with how many it holds ("Title · n"). `quiet` is the same level said
 * small over a list (a day of sessions, the sidebar's lists). */
export function Heading(props: { children: JSX.Element; count?: number; quiet?: boolean; mono?: boolean; class?: string; testID?: string }) {
  return (
    <Text testID={props.testID} role="header" class={`${props.quiet ? "text-caption text-muted-foreground" : "text-body text-foreground"} font-semibold ${props.mono ? "font-mono" : ""} ${props.class ?? ""}`}>
      {props.children}
      <If when={props.count !== undefined}>
        <Text class="font-normal text-muted-foreground">{` · ${count(props.count ?? 0)}`}</Text>
      </If>
    </Text>
  );
}

/** Words set heavier than the text around them, which are not a heading: how
 * a run ended, a counter, a status. In `tone`'s color where it says an outcome. */
export function Strong(props: { children: JSX.Element; class?: string; testID?: string }) {
  return (
    <Text testID={props.testID} class={`text-body font-semibold ${props.class ?? "text-foreground"}`}>
      {props.children}
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

const CARD_PAD = { panel: "gap-3 p-4", tile: "gap-1 px-3 py-2", none: "overflow-hidden" } as const;

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

/** The one row a press opens: hover, pressed, selected and focus, one look
 * in every list (sessions, programs, requests, findings, hits; `plain` for a terminal's line). `lines={2}`
 * stacks two lines of text; `divided` is a row of a card that draws its
 * rows edge to edge under hairlines (`first` has none); `top` keeps what
 * stands beside wrapped text at its first line. What ends the row
 * (`trailing`: a count, a time, a chevron) keeps its width and its middle. */
export function ListRow(props: { children: JSX.Element; onPress?: () => void; selected?: boolean; disabled?: boolean; label?: string; testID?: string; class?: string; trailing?: JSX.Element; lines?: 1 | 2; divided?: boolean; first?: boolean; top?: boolean; /** a terminal's line: the states, no room around it */ plain?: boolean }) {
  const shape = () => (props.plain ? "" : props.divided ? `px-3 py-2 ${props.first ? "" : "border-t border-border"}` : "min-h-7 rounded-(--radius-sm) px-2 py-1");
  const flow = () => (props.lines === 2 ? "flex-col items-stretch justify-center gap-1" : `flex-row gap-2 ${props.top ? "items-start" : "items-center"}`);
  return (
    <BaseButton
      testID={props.testID}
      accessibilityLabel={props.label}
      selected={props.selected}
      disabled={props.disabled}
      onPress={props.onPress}
      class={`group ${flow()} ${shape()} ${ROW_STATE} disabled:hover:bg-transparent ${FOCUS} ${props.class ?? ""}`}
    >
      {props.children}
      <If when={props.trailing}>
        <View class={`ms-auto shrink-0 flex-row items-center gap-2 ${props.top ? "min-h-5" : ""}`}>{props.trailing}</View>
      </If>
    </BaseButton>
  );
}

/** The one row that is only read: a line of a card's list under a hairline
 * (`first` has none), its parts on one middle (`top`: at their first line),
 * in one padding (12 by 8) wherever a card lists rows. */
export function Row(props: { children: JSX.Element; first?: boolean; top?: boolean; wrap?: boolean; /** lines one under another (a job: what it is, then how far it got) */ stack?: boolean; class?: string; testID?: string }) {
  const flow = () => (props.stack ? "gap-1" : `flex-row ${props.wrap ? "flex-wrap gap-x-3 gap-y-1" : "gap-3"} ${props.top ? "items-start" : "items-center"}`);
  return (
    <View testID={props.testID} class={`${flow()} px-3 py-2 ${props.first ? "" : "border-t border-border"} ${props.class ?? ""}`}>
      {props.children}
    </View>
  );
}

/** The one tile an icon stands in: round and quiet, the icon at its large size. */
export function IconTile(props: { icon: IconName; tint?: Tint }) {
  return (
    <View class="size-11 shrink-0 items-center justify-center rounded-full bg-muted">
      <Icon name={props.icon} size={ICON.large} tint={props.tint} />
    </View>
  );
}
/** How much of itself a tile shows, by the room `Tiles` has for it: `full`
 * (what it counts, the figure and its mark, a line about it), `tight` (a tile
 * narrower than it reads in: what it counts and the figure) or `line` (a
 * short pane: what it counts and the figure on one line). What is left out
 * stays in the tile's label, for a screen reader. */
export type TileLook = "full" | "tight" | "line";
/** A figure in a row of them, the one tile: what it counts, the figure, a
 * line about it; with `onPress`, the tile is a button and says where it goes.
 * In a row, the caller gives it its share (`min-w-44 flex-1`); never a
 * height of its own, which collapses it in a column. */
export function Stat(props: { label: string; value: string; detail?: string; tone?: "plain" | "warn" | "ok" | "bad" | "quiet"; icon?: IconName; onPress?: () => void; pending?: boolean; testID?: string; class?: string; /** a mark beside the figure: its line over a session (a `Sparkline`) */ mark?: JSX.Element; /** what a press does, heard and in its label */ hint?: string; /** how much of it shows (`Tiles` says) */ look?: TileLook }) {
  const tint = () => ({ plain: "text-foreground", warn: "text-warn", ok: "text-status-ok", bad: "text-status-server", quiet: "text-muted-foreground" })[props.tone ?? "plain"];
  const look = () => props.look ?? "full";
  const said = () => [`${props.label}: ${props.value}`, props.detail, props.hint].filter(Boolean).join(", ");
  const name = () => (
    <>
      {/* the icon only where the name has room beside it */}
      <If when={props.icon && look() === "full"}>
        <Icon name={props.icon!} size={ICON.small} tint={props.tone === "warn" ? "warn" : "muted-foreground"} />
      </If>
      <Text class="min-w-0 flex-1 text-caption text-muted-foreground line-clamp-1">{props.label}</Text>
    </>
  );
  const chevron = () => (
    <If when={props.onPress}>
      <Icon name="chevron-right" size={ICON.small} />
    </If>
  );
  const figure = () => <Text class={`text-heading font-semibold tabular-nums line-clamp-1 ${look() === "line" ? "shrink-0" : "min-w-0 flex-1"} ${tint()}`}>{props.value}</Text>;
  return (
    <Card pad="tile" class={props.class} testID={props.testID} label={props.onPress || look() !== "full" ? said() : undefined} onPress={props.onPress}>
      <If
        when={look() !== "line"}
        else={
          <View class="flex-row items-center gap-2">
            {name()}
            <If when={!props.pending} else={<View class="h-4 w-12 rounded-(--radius-sm) bg-muted" />}>
              {figure()}
            </If>
            {chevron()}
          </View>
        }
      >
        <View class="flex-row items-center gap-1">
          {name()}
          {chevron()}
        </View>
        <If when={!props.pending} else={<View class="my-1 h-4 w-24 max-w-full rounded-(--radius-sm) bg-muted" />}>
          <View class="flex-row items-center gap-2">
            {figure()}
            <If when={look() === "full"}>{props.mark}</If>
          </View>
        </If>
        <If when={look() === "full"}>
          <If when={!props.pending} else={<View class="h-3 w-32 max-w-full rounded-(--radius-sm) bg-muted/70" />}>
            <Text class={`text-caption text-muted-foreground ${props.mark ? "line-clamp-1" : "line-clamp-2"}`}>{props.detail ?? ""}</Text>
          </If>
        </If>
      </If>
    </Card>
  );
}

/** A path or key too long for its place, cut in its middle so both its
 * start and the end that tells it apart stay; screen readers hear it whole. */
export function Path(props: { path: string; max?: number; class?: string; testID?: string }) {
  return (
    // a window cuts it where it runs out of room (Native Shell's middle ellipsis); a terminal by a count of characters
    <Text testID={props.testID} accessibilityLabel={props.path} class={`font-mono text-caption line-clamp-1 ${props.class ?? "text-foreground"}`} style={terminal() ? undefined : { textOverflow: "ellipsis-middle" }}>
      {terminal() ? middle(props.path, props.max ?? 48) : props.path}
    </Text>
  );
}

/** Something the reader should know before going on, in place: a note
 * (info), a caution (warn) or what went wrong (error); its icon, its words,
 * and at its end what can be done about it. The one banner. */
export function Notice(props: { text: string; title?: string; tone?: "info" | "warn" | "error"; icon?: IconName; children?: JSX.Element; testID?: string; class?: string }) {
  const tone = () => props.tone ?? "info";
  const look = () => ({ info: "border-border bg-muted/60", warn: "border-warn/30 bg-warn/10", error: "border-destructive/30 bg-destructive/10" })[tone()];
  const icon = (): IconName => props.icon ?? (tone() === "info" ? "info" : "triangle-alert");
  const tint = (): Tint => (tone() === "warn" ? "warn" : tone() === "error" ? "destructive" : "muted-foreground");
  return (
    <View testID={props.testID} role={tone() === "error" ? "alert" : undefined} class={`flex-row items-start gap-2 rounded-(--radius) border px-3 py-2 ${look()} ${props.class ?? ""}`}>
      <If when={!terminal()}>
        <View class="h-5 shrink-0 justify-center">
          <Icon name={icon()} tint={tint()} />
        </View>
      </If>
      <View selectable class="min-w-0 flex-1 gap-1">
        <If when={props.title}>
          <Text class="text-body font-medium text-foreground">{props.title}</Text>
        </If>
        <Text class="text-small leading-5 text-foreground">{props.text}</Text>
      </View>
      <If when={props.children}>
        <View class="shrink-0 flex-row items-center gap-2">{props.children}</View>
      </If>
    </View>
  );
}

/** Something fetched, in its three states, each said truly: on its way
 * (what is being waited for), failed (why, and Try again), or there. A
 * place never shows its empty words for a failure, nor waits forever. */
export function Await(props: { loading: boolean; error?: unknown; retry?: () => void; waiting?: string; children: JSX.Element; testID?: string; class?: string }) {
  const message = () => {
    const e = props.error;
    if (!e) return null;
    return e instanceof Error ? e.message : typeof e === "object" && e !== null && "message" in e ? String((e as { message: unknown }).message) : String(e);
  };
  return (
    <If
      when={message()}
      else={
        <If when={props.loading} else={props.children}>
          <View class={`flex-1 items-center justify-center p-6 ${props.class ?? ""}`}>
            <Loading text={props.waiting} testID={props.testID ? `${props.testID}-loading` : undefined} />
          </View>
        </If>
      }
    >
      {(m) => (
        <Empty icon="triangle-alert" title={t("error.loadTitle")} text={m()} testID={props.testID ? `${props.testID}-error` : "load-error"}>
          <If when={props.retry}>
            <Button variant="outline" icon="refresh-cw" label={t("error.retry")} onPress={() => props.retry?.()} />
          </If>
        </Empty>
      )}
    </If>
  );
}

/** The foot of a dialog or a form: Cancel (quiet) then what it commits, at
 * the end, in that order everywhere; what is beside them (a note) at the start. */
export function Actions(props: { children: JSX.Element; note?: JSX.Element; class?: string }) {
  return (
    <View class={`flex-row items-center gap-2 ${props.class ?? ""}`}>
      <View class="min-w-0 flex-1">{props.note}</View>
      {props.children}
    </View>
  );
}

/** Back to where this was opened from: one control, a chevron and the place's name. */
export function Back(props: { label?: string; onPress: () => void; testID?: string }) {
  return <Button small variant="ghost" icon="chevron-left" label={props.label ?? t("common.back")} onPress={props.onPress} testID={props.testID} class="-ml-2 self-start" />;
}

/** The head of a place or a panel's page: what it is, a line about it, and
 * its commands at the end, on one middle. A page that is a window's whole
 * content gives no `title`: the window's title bar says it, once. */
export function PageHeader(props: { title?: string; subtitle?: string; mono?: boolean; children?: JSX.Element; testID?: string }) {
  return (
    <View testID={props.testID} class="min-h-8 flex-row items-center gap-3">
      <View class="min-w-0 flex-1 gap-1">
        <If when={props.title}>
          <Title mono={props.mono} lines={1}>
            {props.title}
          </Title>
        </If>
        <If when={props.subtitle}>
          <Text class="text-small text-muted-foreground line-clamp-2">{props.subtitle}</Text>
        </If>
      </View>
      <If when={props.children}>
        <View class="shrink-0 flex-row items-center gap-2">{props.children}</View>
      </If>
    </View>
  );
}

/** A figure in a line of them, without a card: the number, and under it
 * what it counts. `delta` says how it changed (better in the ok tone, worse
 * in the error's). With `preview`, a press (or Return) opens what the figure
 * is made of and Escape closes it (Native Shell's Popover): the keyboard,
 * touch and screen readers reach it as the pointer does. The one inline
 * figure; in a row of tiles, `Stat`. */
export function Figure(props: { value: string; label: string; tone?: "plain" | "ok" | "warn" | "bad" | "quiet"; delta?: string; deltaTone?: "ok" | "bad" | "quiet"; small?: boolean; preview?: JSX.Element; testID?: string }) {
  const tint = () => ({ plain: "text-foreground", ok: "text-status-ok", warn: "text-warn", bad: "text-status-server", quiet: "text-muted-foreground" })[props.tone ?? "plain"];
  const dt = () => ({ ok: "text-status-ok", bad: "text-status-server", quiet: "text-muted-foreground" })[props.deltaTone ?? "quiet"];
  const body = (
    <View class="gap-1">
      <View class="flex-row items-baseline gap-2">
        <Text class={`tabular-nums font-semibold ${props.small ? "text-body" : "text-heading"} ${tint()}`}>{props.value}</Text>
        <If when={props.delta}>
          <Text class={`tabular-nums text-caption font-medium ${dt()}`}>{props.delta}</Text>
        </If>
      </View>
      <Text class="text-caption text-muted-foreground line-clamp-1">{props.label}</Text>
    </View>
  );
  if (!props.preview || terminal()) return <View testID={props.testID}>{body}</View>;
  return (
    <Popover
      placement="bottom"
      align="start"
      offset={6}
      trigger={
        // the press area reaches past the figure without moving it off the line's edge
        <BaseButton testID={props.testID} accessibilityLabel={`${props.value} ${props.label}`} class={`-mx-2 -my-1 rounded-(--radius-sm) px-2 py-1 hover:bg-muted active:bg-muted ${FOCUS}`}>
          {body}
        </BaseButton>
      }
      popupClass={`w-[300px] gap-2 ${POPUP} p-3`}
      popupStartingClass="opacity-0 -translate-y-1"
      popupEndingClass="opacity-0"
    >
      {props.preview}
    </Popover>
  );
}

/** Figures in a line that never wraps mid-row: as many as the line's width
 * holds, in the order given (which is their rank), the rest left out — a
 * line of figures broken in two reads as two. The line measures itself;
 * each figure has `FIGURE_WIDTH` of it. */
export const FIGURE_WIDTH = 124;
/** How many figures a line `width` wide holds (all of them before it is measured; two at least). */
export const figuresFit = (width: number, n: number) => (width > 0 ? Math.min(n, Math.max(2, Math.floor(width / FIGURE_WIDTH))) : n);
export function Figures(props: { items: { /** whether it has anything to say now (a session's errors: only when there are some) */ show?: () => boolean; figure: JSX.Element }[]; testID?: string; label?: string }) {
  const width = signal(0);
  const on = (i: number) => props.items[i].show?.() ?? true;
  // its place among those shown: the rank the line's room is given by
  const rank = (i: number) => props.items.slice(0, i).filter((_, k) => on(k)).length;
  const fit = () => figuresFit(width(), props.items.filter((_, k) => on(k)).length);
  return (
    <View testID={props.testID} accessibilityLabel={props.label} class="flex-row items-start" onLayout={({ bounds }) => width.set(Math.round(bounds.size.width))}>
      {props.items.map((f, i) => (
        <View style={{ width: FIGURE_WIDTH }} class={on(i) && rank(i) < fit() ? "shrink-0 pr-4" : "hidden"}>
          {f.figure}
        </View>
      ))}
    </View>
  );
}

/** How many tiles go in a row `width` wide, of `n` that each need `min`: all
 * abreast where they fit, else rows of equal count (four are 2×2, six 3×3,
 * five 3 and 2), never a row of all but one and never one column: where two
 * do not fit abreast they are two all the same, and `tight`. */
export function tilesPerRow(width: number, n: number, min: number, gap = 12): number {
  if (width <= 0) return n;
  const fit = Math.max(1, Math.floor((width + gap) / (min + gap)));
  if (fit >= n) return n;
  const half = Math.ceil(n / 2);
  return fit >= half ? half : Math.min(n, 2);
}
/** How many tiles a single row `width` wide holds, of `n` that each need `min`: two at least. */
export const tilesFit = (width: number, n: number, min: number, gap = 12) => (width > 0 ? Math.min(n, Math.max(2, Math.floor((width + gap) / (min + gap)))) : n);
/** Tiles in rows by that rule: the row measures itself and gives each tile its width.
 * `ranked` keeps them to one row: as many as fit, in the order given (their
 * rank), the last ranked left out — as `Figures` does a line of figures.
 * A tile given as a function is told its `TileLook`: `line` in a pane under
 * `BREAK.short` (`short`: the rows under the tiles need the room), `tight`
 * where it is narrower than `min`, `full` otherwise. */
export function Tiles(props: { items: (JSX.Element | ((look: () => TileLook) => JSX.Element))[]; min?: number; testID?: string; ranked?: boolean; label?: string; /** the pane's height is under `BREAK.short` */ short?: boolean }) {
  const GAP = 12;
  const width = signal(0);
  const min = () => props.min ?? 144;
  const per = () => (props.ranked ? tilesFit(width(), props.items.length, min(), GAP) : tilesPerRow(width(), props.items.length, min(), GAP));
  const each = () => (width() > 0 ? Math.floor((width() - GAP * (per() - 1)) / per()) : undefined);
  const look = (): TileLook => (props.short ? "line" : (each() ?? min()) < min() ? "tight" : "full");
  return (
    <View testID={props.testID} accessibilityLabel={props.label} class={`flex-row gap-3 ${props.ranked ? "" : "flex-wrap"}`} onLayout={({ bounds }) => width.set(Math.round(bounds.size.width))}>
      {props.items.map((tile, i) => (
        <View style={each() ? { width: each() } : undefined} class={props.ranked && i >= per() ? "hidden" : each() ? "" : "min-w-36 flex-1"}>
          {typeof tile === "function" ? tile(look) : tile}
        </View>
      ))}
    </View>
  );
}

/** A modal dialog: one surface, one width per size, over the scrim. */
const MODAL = { narrow: "w-[420px]", default: "w-[560px]", wide: "w-[880px]" } as const;

export function Modal(props: { open: boolean; onClose: () => void; size?: keyof typeof MODAL; children: JSX.Element; testID?: string }) {
  return (
    <Dialog
      open={props.open}
      onOpenChange={(e) => !e.open && props.onClose()}
      backdropClass={SCRIM}
      testID={props.testID}
      class={`m-auto ${MODAL[props.size ?? "default"]} max-w-[94%] gap-4 ${CARD} bg-popover p-4 shadow-xl`}
    >
      {props.children}
    </Dialog>
  );
}

/** A section that opens and closes under its title (Native Shell's
 * Accordion of one item: a heading, a button that says whether it is
 * expanded, the panel revealed). `trailing` stands beside the title (what
 * the section comes to, a command); `expanded` and `onToggle` let what holds
 * it keep whether it is open. The one open/close section. */
export function Disclosure(props: { title: string; count?: number; open?: boolean; expanded?: boolean; onToggle?: (open: boolean) => void; children: JSX.Element; testID?: string; itemTestID?: string; level?: number; trailing?: JSX.Element }) {
  const own = signal<string[]>(props.open ? ["x"] : []);
  const value = () => (props.expanded === undefined ? own() : props.expanded ? ["x"] : []);
  return (
    <Accordion
      value={value()}
      onChange={(e) => {
        own.set(e.value);
        props.onToggle?.(e.value.includes("x"));
      }}
    >
      <AccordionItem value="x" testID={props.itemTestID} class="gap-2">
        <AccordionHeader level={props.level ?? 3} class="flex-row items-center gap-3">
          <AccordionTrigger testID={props.testID} class={`group ${props.trailing ? "shrink-0" : "flex-1"} flex-row items-center gap-1 rounded-(--radius-sm) py-1 pr-1 ${FOCUS}`}>
            <View class="group-aria-expanded:rotate-90 motion-safe:transition-transform motion-safe:duration-150">
              <Icon name="chevron-right" size={ICON.small} tint="muted-foreground" />
            </View>
            <Text class="text-body font-semibold text-foreground">{props.title}</Text>
            <If when={props.count !== undefined}>
              <Text class="text-body text-muted-foreground">{`· ${count(props.count ?? 0)}`}</Text>
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
    <View testID={props.testID} accessibilityLabel={props.label} class={`h-5 justify-center self-start rounded-full px-2 ${bg}`}>
      <Text class={`text-small font-medium ${fg}`}>{props.label}</Text>
    </View>
  );
}

/** Names and their values: muted names, selectable values. `boxed` lists
 * them in a bordered card, compact and monospaced (a request's headers). */
export function KeyValue(props: { rows: [string, string][]; mono?: boolean; boxed?: boolean }) {
  if (props.boxed)
    return (
      // a table to a screen reader: names in the first column, values in the second
      <View selectable>
      <Table class="overflow-hidden rounded-(--radius) border border-border bg-card">
        <TableBody>
          <Each of={props.rows}>
            {(row, i) => (
              <TableRow class={`flex-row gap-3 px-2 py-1 ${i() > 0 ? "border-t border-border" : ""}`}>
                <TableCell class="w-[136px] shrink-0">
                  <Text class="font-mono text-caption text-muted-foreground line-clamp-1">{row()[0]}</Text>
                </TableCell>
                <TableCell class="min-w-0 flex-1">
                  <Text class={`text-caption text-foreground ${props.mono === false ? "" : "font-mono"}`}>{row()[1]}</Text>
                </TableCell>
              </TableRow>
            )}
          </Each>
        </TableBody>
      </Table>
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

/** A line of text to type. `bare` leaves its label to screen readers (and
 * its placeholder): a field in a toolbar row, on the row's one middle.
 * `leading` stands before the text inside the field (a prompt, an icon) and
 * `trailing` after it (an `IconButton inField`); `small` is the second
 * control height. A field that matters more is wider and in mono, never higher. */
export function Field(props: { label: string; value: string; onChange: (v: string) => void; onSubmit?: () => void; placeholder?: string; testID?: string; mono?: boolean; class?: string; bare?: boolean; small?: boolean; leading?: JSX.Element; trailing?: JSX.Element; /** shown and selectable, not changed (Native Shell's `readonly`) */ readonly?: boolean; /** not for now: no focus, no typing, dimmed (Native Shell's `disabled`) */ disabled?: boolean }) {
  const boxed = () => !!props.leading || !!props.trailing;
  const text = () => `${props.small ? "text-small" : "text-body"} text-foreground placeholder:text-muted-foreground ${props.mono ? "font-mono" : ""} disabled:opacity-50`;
  const input = (cls: string) => (
    <Input
      testID={props.testID}
      accessibilityLabel={props.label}
      value={props.value}
      placeholder={props.placeholder}
      autoCapitalize="none"
      autoCorrect={false}
      readonly={props.readonly}
      disabled={props.disabled}
      onChange={({ value }) => props.onChange(value)}
      onSubmit={props.onSubmit ? () => props.onSubmit?.() : undefined}
      submitBehavior={props.onSubmit ? "submit" : undefined}
      class={cls}
    />
  );
  return (
    <View class={`gap-1 ${props.class ?? ""}`}>
      <If when={!props.bare}>
        <Text class="text-small font-medium text-muted-foreground">{props.label}</Text>
      </If>
      <If when={boxed()} else={input(`${props.small ? CONTROL.small : CONTROL.field} rounded-(--radius) border border-input bg-background px-2 focus-visible:border-ring ${FOCUS} ${text()}`)}>
        <View class={`${props.small ? CONTROL.small : CONTROL.field} ${CONTROL.box}`}>
          {props.leading}
          {input(`min-w-0 flex-1 ${text()}`)}
          {props.trailing}
        </View>
      </If>
    </View>
  );
}

/** A control that has no label of its own (a row of options, a switch, a
 * value only shown) under the label a `Field` has, on a field's height: in a
 * form, every control's name is above it. */
export function Labeled(props: { label: string; children: JSX.Element; class?: string }) {
  return (
    <View class={`gap-1 ${props.class ?? ""}`}>
      <Text class="text-small font-medium text-muted-foreground">{props.label}</Text>
      <View class="min-h-8 flex-row items-center">{props.children}</View>
    </View>
  );
}

/** The one search field: a magnifier, the text, a clear button while it
 * holds any, and what the place puts after it (a "search everywhere"). */
export function SearchField(props: { value: string; onChange: (v: string) => void; onSubmit?: () => void; placeholder: string; label?: string; small?: boolean; testID?: string; class?: string; children?: JSX.Element }) {
  return (
    <View class={terminal() ? `flex-row items-center gap-1 ${props.class ?? ""}` : `${props.small ? CONTROL.small : CONTROL.field} ${CONTROL.box} ${props.class ?? ""}`}>
      <If when={!terminal()}>
        <View class="shrink-0">
          <Icon name="search" tint="muted-foreground" />
        </View>
      </If>
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
      <If when={props.value && !terminal()}>
        <IconButton inField icon="x" label={t("common.clear")} onPress={() => props.onChange("")} />
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
      inputClass={terminal() ? undefined : "min-w-0 flex-1 px-2 text-body tabular-nums text-foreground placeholder:text-muted-foreground"}
      controlsClass={terminal() ? undefined : "h-full border-l border-input"}
      incrementButtonClass={terminal() ? undefined : "flex-1 items-center justify-center px-2 hover:bg-muted active:bg-accent"}
      decrementButtonClass={terminal() ? undefined : "flex-1 items-center justify-center px-2 hover:bg-muted active:bg-accent"}
      incrementButton={terminal() ? undefined : <View style={{ transform: [{ rotate: "180deg" }] }}><Icon name="chevron-down" size={12} /></View>}
      decrementButton={terminal() ? undefined : <Icon name="chevron-down" size={12} />}
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
      // the thumb centered in its track, top to bottom
      // off-scale: the thumb (16) sits 2 pt inside its track (20)
      trackClass="h-5 w-9 shrink-0 justify-center rounded-full bg-input px-0.5 checked:bg-primary motion-safe:transition-colors"
      thumbClass="h-4 w-4 rounded-full bg-switch-thumb shadow-sm checked:translate-x-4 motion-safe:transition-transform"
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
        // below its trigger (Linux, Windows): a gap, as every other popup has
        offset={4}
        popupClass={POPUP}
        popupStyle={glass}
        itemClass="rounded-sm px-2 py-1 text-body text-popover-foreground focus:bg-accent selected:font-semibold"
        trigger={
          <View class={`h-8 flex-row items-center justify-between gap-2 rounded-(--radius) border border-input bg-background px-2 hover:border-ring/60 ${FOCUS}`}>
            <Text class="min-w-0 flex-1 text-body text-foreground line-clamp-1">
              {title()}
            </Text>
            <View class="shrink-0">
              <Icon name="chevrons-up-down" size={12} />
            </View>
          </View>
        }
      />
    </View>
  );
}

/** One of a few views or modes: Tabs, a pill sliding behind the chosen one. */
export function Segmented(props: { value: string; items: { value: string; title: string; icon?: IconName }[]; onChange: (v: string) => void; testID?: string; class?: string }) {
  return (
    <Tabs testID={props.testID} class={`${CONTROL.track} ${props.class ?? ""}`} indicatorClass="inset-y-0.5 rounded-(--radius-sm) bg-background shadow-sm motion-safe:transition-all motion-safe:duration-200">
      <Each of={props.items} key={(i) => i.value}>
        {(item) => (
          <Tab
            testID={props.testID ? `${props.testID}-${item().value}` : undefined}
            accessibilityLabel={item().title}
            selected={props.value === item().value}
            onPress={() => props.onChange(item().value)}
            class={`group ${CONTROL.segment} ${FOCUS}`}
          >
            <If when={item().icon}>
              <Icon name={item().icon!} size={12} tint={props.value === item().value ? "foreground" : "muted-foreground"} />
            </If>
            <Text class="text-small text-muted-foreground group-selected:font-medium group-selected:text-foreground">{item().title}</Text>
          </Tab>
        )}
      </Each>
    </Tabs>
  );
}

/** One value of a few, as a row of radios (Native Shell's RadioGroup: one
 * is checked, arrow keys move between them, screen readers hear a set of
 * radio buttons and which is chosen). For a value; `Segmented` (tabs)
 * switches what a place shows. */
export function Options(props: { value: string; items: { value: string; title: string; icon?: IconName }[]; onChange: (v: string) => void; testID?: string; class?: string }) {
  return (
    <RadioGroup testID={props.testID} class={`${CONTROL.track} ${props.class ?? ""}`}>
      <Each of={props.items} key={(i) => i.value}>
        {(item) => (
          <Radio
            testID={props.testID ? `${props.testID}-${item().value}` : undefined}
            accessibilityLabel={item().title}
            checked={props.value === item().value}
            onChange={() => props.onChange(item().value)}
            class={`group ${CONTROL.segment} checked:bg-background checked:shadow-sm ${FOCUS}`}
          >
            <If when={item().icon}>
              <Icon name={item().icon!} size={12} tint={props.value === item().value ? "foreground" : "muted-foreground"} />
            </If>
            <Text class="text-small text-muted-foreground group-checked:font-medium group-checked:text-foreground">{item().title}</Text>
          </Radio>
        )}
      </Each>
    </RadioGroup>
  );
}

/** A status mark: a small dot in a status color, with its meaning for screen readers. */
export function Dot(props: { tone: "ok" | "miss" | "client" | "server" | "refused" | "injected" | "live"; label?: string }) {
  const bg = { ok: "bg-status-ok", miss: "bg-status-miss", client: "bg-status-client", server: "bg-status-server", refused: "bg-status-refused", injected: "bg-status-injected", live: "bg-primary" }[props.tone];
  return <View accessibilityLabel={props.label} class={`size-2 rounded-full ${bg}`} />;
}

/** A titled group of settings or facts: a hairline-bounded surface. */
export function Section(props: { title?: string; description?: string; children: JSX.Element; testID?: string; action?: JSX.Element; plain?: boolean }) {
  return (
    <View testID={props.testID} class="gap-3">
      <If when={props.title}>
        <View class="flex-row items-end gap-3">
          <View class="flex-1 gap-1">
            <Heading>{props.title}</Heading>
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

/** Progress: `value` is a fraction, 0–1 (`null` while how much is not known). */
export function Bar(props: { value: number | null; label?: string }) {
  return (
    <View class="gap-1">
      <Progress
        // Native Shell's Progress counts 0–100
        value={props.value === null ? undefined : Math.min(100, Math.max(0, props.value * 100))}
        indeterminate={props.value === null}
        accessibilityLabel={props.label ?? t("common.progress")}
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

/** An empty place: what is missing, and the next step. The one way to say
 * "nothing here": whole (an icon in its tile, a title, the words, what to do)
 * where it fills a place; `compact` (the words only, quiet, centered in the
 * room `class` gives it) inside a chart, a list or a card. */
export function Empty(props: { text: string; title?: string; icon?: IconName; children?: JSX.Element; testID?: string; compact?: boolean; class?: string }) {
  if (props.compact)
    return (
      <View testID={props.testID} class={`items-center justify-center p-3 ${props.class ?? ""}`}>
        <Text class="text-center text-small text-muted-foreground">{props.text}</Text>
      </View>
    );
  return (
    <View testID={props.testID} class={`flex-1 items-center justify-center gap-3 p-6 ${props.class ?? ""}`}>
      <If when={props.icon && !terminal()}>
        <IconTile icon={props.icon!} />
      </If>
      <If when={props.title}>
        <Title center>{props.title}</Title>
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
    <View testID="session-reading" accessibilityLabel={t("sessions.reading")} class="flex-1 gap-3 px-4 pt-4">
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
    <Scroll testID={props.testID} class="flex-1" contentClass="gap-4 p-4">
      {props.children}
    </Scroll>
  );
}

/** A table's columns by rank, the one rule of every table that narrows:
 * of `ranked` (first the one that matters most) as many as fit beside what
 * always shows (`used` points of it) while the column that takes the rest
 * keeps `room`. The first that does not fit goes, and every one ranked after
 * it with it: a column is dropped whole, none is ever clipped, and (the
 * ranked ones standing last, in rank order) only columns at the table's end
 * go — DataTable keeps a column's state by its place. A width of 0 (not
 * measured yet) keeps them all. */
export function fitColumns<K extends string>(width: number, used: number, room: number, ranked: { key: K; width: number }[]): K[] {
  const shown: K[] = [];
  for (const c of ranked) {
    if (width > 0 && width - used - c.width < room) break;
    shown.push(c.key);
    used += c.width;
  }
  return shown;
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
      rowHeight={rowHeight(ROW.dense)}
      classes={terminal() ? undefined : TABLE}
      class={CARD}
      style={terminal() ? undefined : { height: (shown() + 1) * (rowHeight(ROW.dense) ?? 26) + 2 }}
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

/** The first row of a bucket tool's sheet, built one way: what it looks at
 * (`scope`), its command and Cancel while it runs (`children`), and at the
 * row's end what else it offers (`end`: a `More`). One middle, small controls. */
export function ToolBar(props: { scope?: string; children?: JSX.Element; end?: JSX.Element; testID?: string }) {
  return (
    <View testID={props.testID} class="min-h-8 flex-row flex-wrap items-center gap-2">
      <If when={props.scope}>{(s) => <Path path={s()} max={40} class="shrink text-muted-foreground" />}</If>
      {props.children}
      {/* what ends the row is pushed to its end; without it, a field among the children may take the room */}
      <If when={props.end}>
        <View class="flex-1" />
        {props.end}
      </If>
    </View>
  );
}

/** What else a place offers (exports, copies): a `⋯` button opening the
 * system's menu; where there is none (the web), the commands as small buttons. */
export function More(props: { actions: () => { label: string; icon?: IconName; run: () => void; disabled?: boolean }[]; testID?: string }) {
  return (
    <If
      when={isAvailable}
      else={
        <Each of={props.actions()}>{(a) => <Button small variant="outline" icon={a().icon} label={a().label} onPress={a().run} disabled={a().disabled} />}</Each>
      }
    >
      <IconButton
        icon="ellipsis"
        label={t("common.more")}
        testID={props.testID}
        onPress={() => void Menu.popup(props.actions().map((a) => ({ label: a.label, icon: a.icon ? iconSvg(a.icon) : undefined, disabled: a.disabled, onSelect: a.run })))}
      />
    </If>
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
              <If when={a().icon}>{(_) => <Icon name={a().icon!} />}</If>
              <Text class="text-body text-foreground">{a().label}</Text>
            </MenuItem>
          )}
        </Each>
      </ContextMenu>
    </If>
  );
}
