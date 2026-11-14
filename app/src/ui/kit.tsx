// sonde's styled controls, drawn on nsh/base's behavior with the theme's tokens.

import { Animated, cleanup, createAnimatedValue, Easing, effect, ErrorBoundary, screenReaderEnabled, fontScale, type JSX, keymap, platform, reduceMotion, Scroll, setAccessibilityFocus, signal, Text, token, View, type ViewRef } from "nsh";
import { Accordion, AccordionHeader, AccordionItem, AccordionPanel, AccordionTrigger, AlertDialog, Button as BaseButton, Checkbox as BaseCheckbox, ContextMenu, DataTable, type DataTableRef, type DataTableSort, Dialog, Link, MenuItem, Input, NumberInput, Popover, Progress, Radio, RadioGroup, Select, Sheet, Slider, Switch, Tab, Table, TableBody, TableCell, TableRow, Tabs, Tooltip } from "nsh/base";
import { lang, t } from "../i18n";
import { count, middle, separators } from "../format";
import { isAvailable, Menu, type MenuItem as MenuItemSpec } from "nsh/desktop";
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

/** Chinese is never set under 12 pt: where Latin takes the 11 pt caption,
 * Chinese takes 12 on the same 16 pt line (nsh.json's `caption-cjk`). Read
 * where it is drawn, so a change of language reaches it. */
const cjk = () => lang() === "zh";
const captionSize = () => (cjk() ? "text-caption-cjk" : "text-caption");

/** The type scale, in points: the `text-caption` … `text-title` classes
 * (nsh.json's `tailwind.fontSize`, each with its line: 11/16, 12/18, 13/20,
 * 15/22, 20/28), for the props that take a number (DataTable's `fontSize`).
 * A desktop app's scale around a 13 pt body. */
export const TYPE = {
  get caption() {
    return cjk() ? 12 : 11;
  },
  small: 12,
  body: 13,
  heading: 15,
  title: 20,
};
/** Each size's line, in points (nsh.json sets them with the sizes): for what
 * lays text out by number. */
export const LEADING = { caption: 16, small: 18, body: 20, heading: 22, title: 28 } as const;

/** The text roles: a size and a colour (and a face) that read together, each
 * named once. A page writes the role, not its classes (lint-kit fails
 * `text-caption` outside the kit: Chinese takes 12 pt there, which only the
 * role knows). Read where they are drawn (getters), so a change of language
 * reaches them. What lays the text out (`min-w-0 flex-1`, `line-clamp-1`)
 * stands beside the role. */
export const TEXT = {
  /** rows, labels, buttons, prose */
  body: "text-body text-foreground",
  /** the same, quiet: what stands beside the body and says less */
  bodyQuiet: "text-body text-muted-foreground",
  /** a secondary line under a row's first, help under a field, a field's label */
  note: "text-small text-muted-foreground",
  /** the same at its full colour: a secondary line that is read, not glanced at */
  small: "text-small text-foreground",
  /** what is refused, under what refused it */
  error: "text-small text-status-server",
  /** metadata: a column's title, a count, a time, a shortcut */
  get meta() {
    return `${captionSize()} text-muted-foreground`;
  },
  /** the caption's size in the text's colour: a figure in a dense row */
  get caption() {
    return `${captionSize()} text-foreground`;
  },
  /** the caption's size alone, for words whose colour is a tone's (`TONE[t].text`) */
  get captionSize() {
    return captionSize();
  },
  /** a key, a path, a command, an id */
  get code() {
    return `font-mono ${captionSize()} text-foreground`;
  },
  /** the same, quiet: a key's name beside its value */
  get codeQuiet() {
    return `font-mono ${captionSize()} text-muted-foreground`;
  },
};

/** Motion: four lengths (ms) and one ease (nsh.json's `ease-standard`, a soft
 * settle), and no others; lint-kit fails a `duration-N` outside the kit.
 * `instant` a hover's wash, a colour; `fast` a press, a control's state;
 * `base` a thing moving into place (a selection sliding, a value gliding);
 * `slow` a reveal (a dialog or a popup opening). Leaving is a step faster than
 * entering. Under reduced motion none of it runs (`motion-safe:`). */
export const TIMING = { instant: 120, fast: 180, base: 240, slow: 360 } as const;
/** A control's states change over `TIMING.instant` on the one ease: colours,
 * opacity, shadows and transforms (Native Shell's `transition`). */
export const TRANSITION = "motion-safe:transition motion-safe:duration-120 motion-safe:ease-standard";
/** What moves into place (a selection sliding, a value gliding), over `TIMING.base`. */
export const GLIDE = "motion-safe:transition-all motion-safe:duration-240 motion-safe:ease-standard";
/** A press sinks a little, none under reduced motion: a button, an icon
 * button or a segment to 0.97; a row, a card or a place, wider, to 0.99. */
export const PRESS = {
  control: `${TRANSITION} motion-safe:active:scale-97`,
  surface: `${TRANSITION} motion-safe:active:scale-99`,
} as const;
/** The three shadows (nsh.json's `tailwind.shadow`), and no others; lint-kit
 * fails a `shadow-*` outside the kit. A control raised in its track (a
 * segment, a switch's thumb); a popup (menus, selects, popovers, tooltips,
 * toasts); a modal surface (dialogs, side panels). The base surface has none. */
export const SHADOW = { control: "shadow-control", popup: "shadow-popup", modal: "shadow-modal" } as const;

/** What can no longer be pressed or changed: one dimming, on every control. */
export const DISABLED = "disabled:opacity-40";
/** The one dot that marks a status, a live run, what is new: 8 pt, round. */
export const DOT = "size-2 shrink-0 rounded-full";
/** The row under the keyboard or the pointer in a popup (a menu, a select,
 * the palette, the endpoint switcher): one highlight. */
export const HIGHLIGHT = "bg-accent";
/** What is chosen in a list, a table, a tree or a row of tiles: one selection. */
export const SELECTED = "bg-selection";
/** Chrome that is there only when wanted: a row's trash, pencil or Decode,
 * a tile's chevron. Hidden until the pointer is over its `group` (a row, a
 * tile), the group is selected, or the keyboard is inside it; it keeps its
 * place, so nothing moves as it shows, and it is always reachable. */
export const REVEAL = `opacity-0 group-hover:opacity-100 group-selected:opacity-100 focus-within:opacity-100 ${TRANSITION}`;

/** One vocabulary of tones, and one table of how each is drawn: its words
 * (`text`), a solid mark (`fill`: a bar, a lane), its dot (`dot`), its icon's
 * tint (`icon`), and a quiet wash with words on it (`badge`). The status
 * tones are a request's: `ok` a success, `miss` a key not found, `client`
 * the program's own 4xx, `fail` a failure, `refused` refused by sonde,
 * `injected` a fault scenario's answer. Then `warn`, `info`, `live` (at work
 * now, the primary), `plain` and `quiet`. Colour marks (a dot, an icon, a
 * rule); words in a tone are for a figure that is its tile's point, never a
 * row's whole text. */
export type Tone = "plain" | "quiet" | "ok" | "miss" | "client" | "fail" | "refused" | "injected" | "warn" | "info" | "live";
export const TONE: Record<Tone, { text: string; fill: string; dot: string; icon: Tint; badge: string }> = {
  plain: { text: "text-foreground", fill: "bg-muted-foreground", dot: "bg-muted-foreground", icon: "foreground", badge: "bg-muted text-foreground" },
  quiet: { text: "text-muted-foreground", fill: "bg-status-miss", dot: "bg-status-miss", icon: "muted-foreground", badge: "bg-muted text-muted-foreground" },
  ok: { text: "text-status-ok", fill: "bg-bar", dot: "bg-status-ok", icon: "status-ok", badge: "bg-status-ok/12 text-status-ok" },
  miss: { text: "text-muted-foreground", fill: "bg-status-miss", dot: "bg-status-miss", icon: "status-miss", badge: "bg-muted text-muted-foreground" },
  client: { text: "text-status-client", fill: "bg-status-client", dot: "bg-status-client", icon: "status-client", badge: "bg-status-client/12 text-status-client" },
  fail: { text: "text-status-server", fill: "bg-status-server", dot: "bg-status-server", icon: "status-server", badge: "bg-status-server/12 text-status-server" },
  refused: { text: "text-status-refused", fill: "bg-status-refused", dot: "bg-status-refused", icon: "status-refused", badge: "bg-status-refused/12 text-status-refused" },
  injected: { text: "text-status-injected", fill: "bg-status-injected", dot: "bg-status-injected", icon: "status-injected", badge: "bg-status-injected/12 text-status-injected" },
  warn: { text: "text-warn", fill: "bg-warn", dot: "bg-warn", icon: "warn", badge: "bg-warn/12 text-warn" },
  info: { text: "text-info", fill: "bg-info", dot: "bg-info", icon: "info", badge: "bg-info/12 text-info" },
  live: { text: "text-primary", fill: "bg-primary", dot: "bg-primary", icon: "primary", badge: "bg-primary/12 text-primary" },
};
/** An icon's three sizes, in points, and no others: `small` beside caption
 * text and at a row's end, `default` (what `Icon` draws without a size) in
 * buttons, rows and notices, `large` in an icon tile. */
export const ICON = { small: 12, default: 16, large: 20 } as const;
/** Two weights, and no third: regular, and this one — bold (700), not
 * semibold. A heading must be heavier than its text in every script: the
 * CJK faces a system falls back to have a regular and a bold and nothing
 * between (Noto Sans CJK and WenQuanYi on Linux, Microsoft YaHei on
 * Windows; PingFang's heaviest stands for it on macOS), and a face with no
 * bold is emboldened only when bold is asked for — at 600 a Chinese title
 * was drawn regular beside the bold Latin in it. lint-kit fails
 * `font-medium` and `font-semibold` anywhere.
 *
 * Weight that is not a heading's: a status word, a counter, a mark in a
 * chart. A heading is `Title` or `Heading`; text set heavier in a page is
 * `Strong`, and this class only where a prop takes classes, not children
 * (lint-kit asks each use for an `emphasis:` comment saying why). */
export const EMPHASIS = "font-bold";

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
export const POPUP = `rounded-(--radius-card) border border-border ${fill("bg-popover/90", "bg-popover")} p-1 ${SHADOW.popup}`;
/** How every popup comes and goes (a select's list, the endpoint switcher, a
 * figure's preview, a date picker): from just above its trigger, a little
 * small and clear, settling in over `TIMING.fast`; it leaves by fading. Give
 * `class` with the popup's classes, `starting` and `ending` to its
 * `popupStartingClass` and `popupEndingClass`. Under reduced motion it is
 * there at once. */
export const POPUP_MOTION = {
  class: "motion-safe:transition motion-safe:duration-180 motion-safe:ease-standard",
  starting: "opacity-0 scale-97 -translate-y-1",
  ending: "opacity-0",
} as const;
/** How a toast comes in from the window's corner and goes, over `TIMING.base`. */
export const TOAST_MOTION = "motion-safe:transition-all motion-safe:duration-240 motion-safe:ease-standard";
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
  get headerCell() {
    return `group px-2 ${TEXT.meta}`;
  },
  // the sort mark right after its title, shown on the sorted column (its one direction) and under the pointer
  headerContent: "justify-start gap-1",
  sortAscending: `opacity-0 selected:opacity-100 group-hover:opacity-100 ${TRANSITION}`,
  sortDescending: `opacity-0 selected:opacity-100 group-hover:opacity-100 ${TRANSITION}`,
  // (its text in the theme's own colour: a table that draws its cells itself was black on dark)
  cell: "px-2 text-foreground",
  rowHover: "bg-row-hover",
  rowSelected: SELECTED,
  fixedEdge: "border-r border-border",
} as const;
/** A table inside a card or a chart (`SimpleTable bare`): the same, with no
 * box of its own and no fill under its titles, the hairline under them only. */
export const BARE_TABLE = {
  ...TABLE,
  get headerCell() {
    return TABLE.headerCell;
  },
  header: "border-b border-border",
  root: "border-0 bg-transparent",
};
/** The one width of a settings page, its forms and lists: Settings and Endpoints line up. */
export const FORM = "w-full max-w-[720px] self-center";
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
/** The sizes a page may name, each once: the least width a tile reads in
 * (a session's signal, a comparison's fact with its two lines, a bucket's
 * size), how high a dialog's scrolling part grows before it scrolls, a
 * terminal's matrix, and the findings' room over a session's operations (in
 * a short pane: two lines, or the least a chosen finding's advice reads in;
 * in a taller one its least and its share of the pane, a finding chosen or
 * not; the room is then rounded down to whole findings). lint-kit and
 * lint-layout fail a size constant, a literal least width, a column's literal
 * width or a literal `maxHeight` in a page. */
export const SIZES = {
  signalTile: 168,
  compareTile: 176,
  capacityTile: 128,
  dialogScroll: 480,
  terminalMatrix: 640,
  findingsShort: 68,
  findingsShortOpen: 96,
  findingsLeast: 104,
  findingsShare: 0.3,
  findingsShareOpen: 0.45,
} as const;
/** A table's column widths, each named once (lint-kit fails a literal width
 * in a column object; lint-layout reads these to check a title and the widest
 * cell fit): what a column holds decides its width, the column that takes the
 * rest has a least. */
export const COLUMN = {
  /** a key, an object's name: the least they keep */
  keyLeast: 120,
  /** an upload's mark (an icon) */
  flag: 24,
  /** a date and time in full ("Sep 30, 2026, 12:59 PM") */
  date: 172,
  /** an age ("9,999 d") */
  age: 84,
  /** a count of parts */
  parts: 72,
  /** a size */
  bytes: 104,
  /** a cost a month */
  cost: 112,
  /** a storage class */
  storageClass: 124,
  /** an object's parts, beside its size */
  objectParts: 64,
  /** an object's name before the table has measured itself */
  name: 380,
  /** the store matrix: what is compared, the recorded figure, a store's */
  matrixWhat: 190,
  matrixRecorded: 90,
  matrixRun: 100,
  /** the compatibility matrix: a probe, a store's verdict */
  probe: 288,
  probeStore: 150,
  /** an operation's title: before measuring, beside the output, and the least it keeps */
  title: 340,
  titleCompact: 220,
  titleLeast: 160,
  /** the operations' place-in-time bar and notes: the least they keep */
  whenLeast: 80,
  notesLeast: 120,
  /** the program's output: its clock, and a line as long as lines go */
  outputTime: 110,
  outputLine: 2000,
} as const;

/** The least (and most) a split's panes keep, each named once (lint-kit fails
 * a literal `minSize`/`maxSize`/`size` on a ResizablePanel). */
export const PANE = {
  /** the sidebar */
  sidebarLeast: 208,
  sidebarMost: 440,
  /** the sidebar under `BREAK.toolbar`: the user's width, but never wider than this */
  sidebarNarrow: 220,
  /** the page beside the sidebar */
  mainLeast: 480,
  /** Run's program and its operations */
  terminalLeast: 320,
  /** the program's output once it has ended: it yields to the result beside it */
  terminalDone: 200,
  operationsLeast: 340,
  /** a session's output beside its operations */
  outputLeast: 240,
  besideLeast: 320,
  /** a session beside its inspector */
  sessionLeast: 460,
  inspector: 420,
  inspectorLeast: 320,
  inspectorMost: 680,
  /** a bucket's objects beside an object's details */
  objectsLeast: 420,
  detailLeast: 300,
  detailMost: 720,
} as const;

/** The widths a page may give a control or a column of text, each named
 * once (lint-kit fails a `w-N` and an arbitrary `[…]` size anywhere else):
 * fields by what they hold, columns of a hand-set row by what stands in them. */
export const WIDTH = {
  /** a number: a count of days */
  number: "w-28",
  /** a short choice: how long a link lasts */
  choice: "w-36",
  /** a prefix, a bucket, a key's start */
  field: "w-56",
  /** a folder, a filter, a snapshot among snapshots */
  long: "w-64",
  /** a run among runs */
  wide: "w-80",
  /** a status and its code */
  status: "w-12",
  /** a size, at its column's end */
  size: "w-20",
  /** a standing's badge */
  badge: "w-24",
  /** a count in words ("28 operations") */
  count: "w-28",
  /** a date and time */
  date: "w-36",
  /** a tree row's chevron */
  chevron: "w-3.5",
  /** the digit that opens a place from a terminal's home list */
  digit: "w-4",
  /** the title bar: the endpoint switcher, wide and tight; the search field's stand-in; the running program's pill at most */
  switcher: "w-52",
  switcherTight: "w-40",
  search: "w-44",
  pill: "max-w-[200px]",
} as const;
/** Surfaces and regions with a size of their own, each named once: the
 * palette and its list, a toast, a diff's height, a centered note's and a
 * centered card's width, a trend's least width, the program's share of a
 * terminal's split. */
export const SURFACE = {
  switcherPopup: "min-w-[260px]",
  palette: "w-[600px] max-w-[92%]",
  paletteList: "max-h-[400px]",
  paletteListTerminal: "max-h-[360px]",
  toast: "w-[340px]",
  diff: "h-[440px]",
  note: "max-w-[520px]",
  noteCard: "w-full max-w-[640px]",
  trend: "min-w-[220px]",
  terminalHalf: "h-[45%]",
} as const;
/** A row under the pointer, pressed, and chosen: one look in every list, tree and menu. */
const ROW_STATE = `hover:bg-row-hover active:bg-muted selected:${SELECTED}`;
/** The row's look for the controls that take classes, not children: a tree's
 * items (the sidebar's buckets) and the palette's. Rows of a list are `ListRow`. */
export const ROWS = {
  tree: `group h-7 rounded-(--radius-sm) ${ROW_STATE} focus:bg-row-hover ${DISABLED}`,
  palette: `group ${CONTROL.field} rounded-(--radius-sm) px-2 focus:${HIGHLIGHT}`,
  /** an item of a popup the app draws (a select's list, a menu where the system has none) */
  popup: `rounded-(--radius-sm) px-2 py-1 focus:${HIGHLIGHT}`,
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
      popupClass={`${PANEL[props.size ?? "default"]} border-l border-t border-border ${fill("bg-background/90", "bg-background")} ${SHADOW.modal}`}
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
  primary: "bg-primary hover:bg-primary/90 active:bg-primary/80",
  outline: "border border-input bg-background hover:bg-muted active:bg-muted",
  ghost: "hover:bg-muted active:bg-muted",
  // (dimmed, a destructive red reads as mud on dark: one that cannot act is a quiet grey fill, its label muted)
  destructive: "bg-destructive hover:bg-destructive/90 active:bg-destructive/80 disabled:bg-muted",
};

const buttonText: Record<Variant, string> = {
  primary: "text-primary-foreground",
  outline: "text-foreground",
  ghost: "text-foreground",
  destructive: "text-destructive-foreground group-disabled:text-muted-foreground",
};

const iconTint: Record<Variant, Tint> = {
  primary: "primary-foreground",
  outline: "foreground",
  ghost: "muted-foreground",
  destructive: "primary-foreground",
};

/** The app's tooltip: a short hint, and the shortcut when there is one, on
 * the popup surface every menu and popover has (Native Shell's design guide:
 * one treatment for the popup family). It fades in and out. */
export function Tip(props: { text: string; shortcut?: string }) {
  // (a hint of a sentence or two, how to read a chart, wraps rather than crossing the window)
  return (
    <Tooltip
      placement="bottom"
      class={`max-w-80 flex-row items-center gap-2 rounded-(--radius-sm) border border-border ${fill("bg-popover/95", "bg-popover")} px-2 py-1 ${SHADOW.popup} ${TRANSITION}`}
      style={glass}
      startingClass="opacity-0"
      endingClass="opacity-0"
    >
      <Text class={`min-w-0 shrink ${TEXT.small} text-popover-foreground`}>{props.text}</Text>
      <If when={props.shortcut}>{(k) => <Text class={`font-mono ${TEXT.meta}`}>{keyLabel(k())}</Text>}</If>
    </Tooltip>
  );
}

/** A shortcut, as the platform writes it (⌘K, Ctrl+K). */
export function Kbd(props: { keys: string; class?: string }) {
  return <Text class={`font-mono ${TEXT.meta} ${props.class ?? ""}`}>{keyLabel(props.keys)}</Text>;
}

/** A turning ring: what a `loading` button shows in its icon's place. Still
 * under reduced motion (the button says it is busy all the same). */
function Spinner(props: { tint: Tint }) {
  return <View class={`size-3.5 shrink-0 rounded-full border-2 ${SPINNER[props.tint] ?? SPINNER.foreground} motion-safe:animate-spin`} />;
}
const SPINNER: Partial<Record<Tint, string>> = {
  foreground: "border-foreground/25 border-t-foreground",
  "muted-foreground": "border-muted-foreground/25 border-t-muted-foreground",
  "primary-foreground": "border-primary-foreground/30 border-t-primary-foreground",
};

/** A button: its label (and icon), hover and a small sink when pressed.
 * `loading` is a button whose act is under way: it keeps its label and its
 * width, a turning ring stands in its icon's place (or over its label, where
 * it has no icon), and it takes no presses until done. */
export function Button(props: {
  label: string;
  onPress?: () => void;
  variant?: Variant | "secondary";
  disabled?: boolean;
  /** its act is under way: the label stays, a ring turns, no presses */
  loading?: boolean;
  small?: boolean;
  icon?: IconName;
  /** A hint on hover, with `shortcut`. */
  tip?: string;
  shortcut?: string;
  testID?: string;
  class?: string;
  ref?: (r: ViewRef) => void;
}) {
  const v = (): Variant => (props.variant === "secondary" || !props.variant ? "outline" : props.variant);
  const tint = () => (props.disabled && v() === "destructive" ? "muted-foreground" : iconTint[v()]);
  return (
    <BaseButton
      ref={props.ref}
      testID={props.testID}
      onPress={props.onPress}
      disabled={props.disabled || props.loading}
      accessibilityLabel={props.label}
      tooltip={props.tip || props.shortcut ? <Tip text={props.tip ?? props.label} shortcut={props.shortcut} /> : undefined}
      class={`group flex-row items-center justify-center gap-1 rounded-(--radius) ${props.small ? "h-7 px-2" : "h-8 px-3"} ${buttonClass[v()]} ${props.loading || v() === "destructive" ? "" : DISABLED} ${PRESS.control} ${FOCUS} ${props.class ?? ""}`}
    >
      <If when={props.icon}>
        <If when={props.loading} else={<Icon name={props.icon!} tint={tint()} />}>
          <View class="size-4 items-center justify-center">
            <Spinner tint={tint()} />
          </View>
        </If>
      </If>
      <Text class={`${props.small ? "text-small" : "text-body"} ${buttonText[v()]} ${props.loading && !props.icon ? "opacity-0" : ""}`}>{props.label}</Text>
      {/* without an icon, the ring stands over the label, which keeps the button's width */}
      <If when={props.loading && !props.icon}>
        <View class="absolute inset-0 items-center justify-center">
          <Spinner tint={tint()} />
        </View>
      </If>
    </BaseButton>
  );
}

/** A button that shows only its icon, named for screen readers (a terminal
 * shows the name in its place), explained by its tooltip. */
export function IconButton(props: { icon: IconName; label: string; onPress?: () => void; shortcut?: string; /** why it does nothing now, in its tooltip */ tip?: string; disabled?: boolean; selected?: boolean; tint?: Tint; testID?: string; class?: string; /** a small count at its corner (jobs running) */ badge?: string; /** at a field's end, inside it: as high as the field, no height of its own */ inField?: boolean; ref?: (r: ViewRef) => void }) {
  return (
    <BaseButton
      ref={props.ref}
      testID={props.testID}
      onPress={props.onPress}
      disabled={props.disabled}
      selected={props.selected}
      accessibilityLabel={props.label}
      tooltip={<Tip text={props.tip ?? props.label} shortcut={props.tip ? undefined : props.shortcut} />}
      class={`${props.inField ? "-mr-2 w-7 shrink-0 self-stretch" : "size-7 rounded-(--radius)"} items-center justify-center hover:bg-muted active:bg-muted selected:${SELECTED} ${DISABLED} ${PRESS.control} ${FOCUS} ${props.class ?? ""}`}
    >
      <Icon name={props.icon} tint={props.tint ?? (props.selected ? "foreground" : "muted-foreground")} />
      <If when={props.badge}>
        <View class="absolute -right-1.5 -top-1 h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1">
          <Text class={`tabular-nums ${TEXT.captionSize} ${EMPHASIS} leading-4 text-primary-foreground`}>{props.badge}</Text>
        </View>
      </If>
    </BaseButton>
  );
}

/** A panel's or a page's title: the first of the two heading levels. `page`
 * is the one title of a page that is only an introduction (the welcome, an
 * empty Run); `mono` for a title that is a command, a key or a bucket. A
 * terminal has one weight and no sizes. */
export function Title(props: { children?: JSX.Element; page?: boolean; /** at the body's size: a title that is a long command */ small?: boolean; mono?: boolean; lines?: 1 | 2; center?: boolean; class?: string; testID?: string; /** a session's command: on one line, regular-weight mono at the body's size, cut in its middle (never wrapped through an argument); heard whole */ command?: string }) {
  if (props.command !== undefined)
    return (
      <Text testID={props.testID} role="header" accessibilityLabel={props.command} class={`font-mono ${TEXT.body} line-clamp-1 ${props.class ?? ""}`} style={terminal() ? undefined : { textOverflow: "ellipsis-middle" }}>
        {props.command}
      </Text>
    );
  return (
    <Text
      testID={props.testID}
      role="header"
      class={`${terminal() ? "" : props.page ? "text-title" : props.small ? "text-body" : "text-heading"} font-bold text-foreground ${props.mono ? "font-mono" : ""} ${props.center ? "text-center" : ""} ${props.lines === 1 ? "line-clamp-1" : props.lines === 2 ? "line-clamp-2" : ""} ${props.class ?? ""}`}
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
    <Text testID={props.testID} role="header" class={`${props.quiet ? TEXT.meta : TEXT.body} font-bold ${props.mono ? "font-mono" : ""} ${props.class ?? ""}`}>
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
    <Text testID={props.testID} class={`text-body font-bold ${props.class ?? "text-foreground"}`}>
      {props.children}
    </Text>
  );
}

export function Muted(props: { children: JSX.Element; class?: string }) {
  return <Text class={`${TEXT.bodyQuiet} ${props.class ?? ""}`}>{props.children}</Text>;
}

export function Mono(props: { children: JSX.Element; class?: string }) {
  return (
    <Text class={`font-mono ${TEXT.small} ${props.class ?? ""}`}>
      {props.children}
    </Text>
  );
}

const CARD_PAD = { panel: "gap-3 p-4", tile: "gap-1 px-3 py-2", none: "overflow-hidden" } as const;

/** The one card: `panel` for a group (the default), `tile` for a figure in a
 * row of them, `none` for a list that draws its own rows. With `onPress` the
 * whole card is a button (hover, focus, a press). Never one inside another. */
export function Card(props: { children: JSX.Element; pad?: keyof typeof CARD_PAD; class?: string; testID?: string; onPress?: () => void; label?: string; selected?: boolean; /** its width, once laid out: for what it holds to fit itself to */ onWidth?: (points: number) => void }) {
  const cls = () => `${CARD} ${CARD_PAD[props.pad ?? "panel"]} ${props.class ?? ""}`;
  return (
    <If when={props.onPress} else={<View testID={props.testID} class={cls()} onLayout={props.onWidth ? ({ bounds }) => props.onWidth?.(Math.round(bounds.size.width)) : undefined}>{props.children}</View>}>
      <BaseButton testID={props.testID} accessibilityLabel={props.label} selected={props.selected} onPress={() => props.onPress?.()} class={`group ${cls()} items-stretch hover:border-ring/40 hover:bg-muted/40 active:bg-muted selected:border-ring/60 ${PRESS.surface} ${FOCUS}`}>
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
      class={`group ${flow()} ${shape()} ${ROW_STATE} disabled:hover:bg-transparent disabled:active:scale-100 ${PRESS.surface} ${FOCUS} ${props.class ?? ""}`}
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
export function Row(props: { children: JSX.Element; first?: boolean; top?: boolean; wrap?: boolean; /** lines one under another (a job: what it is, then how far it got) */ stack?: boolean; /** under its hairline, what pads itself (a list that scrolls): no padding of the row's */ bare?: boolean; class?: string; testID?: string }) {
  const flow = () => (props.stack ? "gap-1" : `flex-row ${props.wrap ? "flex-wrap gap-x-3 gap-y-1" : "gap-3"} ${props.top ? "items-start" : "items-center"}`);
  return (
    <View testID={props.testID} class={`group ${flow()} ${props.bare ? "" : "px-3 py-2"} ${props.first ? "" : "border-t border-border"} ${props.class ?? ""}`}>
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
export function Stat(props: { label: string; value: string; detail?: string; tone?: Tone; icon?: IconName; onPress?: () => void; pending?: boolean; testID?: string; class?: string; /** a mark beside the figure: its line over a session (a `Sparkline`) */ mark?: JSX.Element; /** what a press does, heard and in its label */ hint?: string; /** how much of it shows (`Tiles` says) */ look?: TileLook }) {
  // the figure in the text's colour (a quiet one muted): its tone is a mark beside its name, never its figure
  const tint = () => (props.tone === "quiet" ? TONE.quiet.text : "text-foreground");
  const marked = () => !!props.tone && props.tone !== "plain" && props.tone !== "quiet";
  const look = () => props.look ?? "full";
  const said = () => [`${props.label}: ${props.value}`, props.detail, props.hint].filter(Boolean).join(", ");
  const name = () => (
    <>
      {/* the icon only where the name has room beside it, in the tile's tone; a tone without one, its dot */}
      <If when={props.icon && look() === "full"} else={<If when={marked()}><View class="h-4 shrink-0 justify-center"><View class={`${DOT} ${TONE[props.tone!].dot}`} /></View></If>}>
        <Icon name={props.icon!} size={ICON.small} tint={marked() ? TONE[props.tone!].icon : "muted-foreground"} />
      </If>
      {/* (on a line of its own with its figure, a long name takes a second line before it is cut) */}
      <Text class={`min-w-0 flex-1 ${TEXT.meta} ${look() === "line" ? "line-clamp-2" : "line-clamp-1"}`}>{look() === "full" ? props.label : props.label.split(" · ")[0]}</Text>
    </>
  );
  // where a press goes, said under the pointer or the keyboard only
  const chevron = () => (
    <If when={props.onPress}>
      <View class={REVEAL}>
        <Icon name="chevron-right" size={ICON.small} />
      </View>
    </If>
  );
  // a figure is never cut: a figure of words ("SlateDB, Apache Iceberg") takes a second line in its tile
  const figure = () => <Text class={`text-heading font-bold tabular-nums ${look() === "line" ? "shrink-0 line-clamp-1" : "min-w-0 flex-1 line-clamp-2"} ${tint()}`}>{props.value}</Text>;
  return (
    // (in a row of tiles, as high as the row: `Tiles` gives it its look)
    <Card pad="tile" class={`${props.look ? "grow" : ""} ${props.class ?? ""}`} testID={props.testID} label={props.onPress || look() !== "full" ? said() : undefined} onPress={props.onPress}>
      <If
        when={look() !== "line"}
        else={
          // what it counts on the figure's first baseline (Native Shell's `items-baseline`), not at its middle
          <View class="flex-row items-baseline gap-2">
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
            <Text class={`${TEXT.meta} ${props.mark ? "line-clamp-1" : "line-clamp-2"}`}>{props.detail ?? ""}</Text>
          </If>
        </If>
      </If>
    </Card>
  );
}

/** A path or key too long for its place, the one rule for cutting one: in
 * its middle, so its start and the end that tells it apart stay, on one
 * line (never cut and then wrapped), with no space left against the cut.
 * Keys listed together are shortened first (`shortKeys`: their shared
 * folders as "…/"); `whole` is then what a screen reader hears. */
export function Path(props: { path: string; max?: number; class?: string; testID?: string; whole?: string }) {
  // runs of spaces read as one: a cut beside them would leave a gap
  const text = () => props.path.replace(/\s{2,}/g, " ");
  return (
    // a window cuts it where it runs out of room (Native Shell's middle ellipsis); a terminal by a count of characters
    <Text testID={props.testID} accessibilityLabel={props.whole ?? props.path} class={`font-mono ${TEXT.captionSize} line-clamp-1 ${props.class ?? "text-foreground"}`} style={terminal() ? undefined : { textOverflow: "ellipsis-middle" }}>
      {terminal() ? middle(text(), props.max ?? 48) : text()}
    </Text>
  );
}

/** Something the reader should know before going on, in place: a note
 * (info), a caution (warn) or what went wrong (error); its icon, its words,
 * and at its end what can be done about it. The one banner. */
export function Notice(props: { text: string; title?: string; tone?: "info" | "warn" | "error"; icon?: IconName; children?: JSX.Element; testID?: string; class?: string }) {
  const tone = () => props.tone ?? "info";
  const look = () => ({ info: "border-border bg-muted/60", warn: "border-warn/30 bg-warn/10", error: "border-status-server/30 bg-status-server/10" })[tone()];
  const icon = (): IconName => props.icon ?? (tone() === "info" ? "info" : "triangle-alert");
  const tint = (): Tint => TONE[({ info: "quiet", warn: "warn", error: "fail" } as const)[tone()]].icon;
  return (
    <View testID={props.testID} role={tone() === "error" ? "alert" : undefined} class={`flex-row items-start gap-2 rounded-(--radius) border px-3 py-2 ${look()} ${props.class ?? ""}`}>
      <If when={!terminal()}>
        <View class="h-5 shrink-0 justify-center">
          <Icon name={icon()} tint={tint()} />
        </View>
      </If>
      <View selectable class="min-w-0 flex-1 gap-1">
        <If when={props.title}>
          <Text class={`${TEXT.body} font-bold`}>{props.title}</Text>
        </If>
        <Text class={TEXT.small}>{props.text}</Text>
      </View>
      {/* what can be done stands on the words' middle, not above them (the icon keeps to their first line) */}
      <If when={props.children}>
        <View class="shrink-0 flex-row items-center gap-2 self-center">{props.children}</View>
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
  // (at its line's start: the row places it, the button sets no `self-start` of its own)
  return (
    <View class="flex-row">
      <Button small variant="ghost" icon="chevron-left" label={props.label ?? t("common.back")} onPress={props.onPress} testID={props.testID} class="-ml-2" />
    </View>
  );
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
          <Text class={`${TEXT.note} line-clamp-2`}>{props.subtitle}</Text>
        </If>
      </View>
      <If when={props.children}>
        <View class="shrink-0 flex-row items-center gap-2">{props.children}</View>
      </If>
    </View>
  );
}

/** A figure in a line of them, without a card: the number, and under it
 * what it counts. With `preview`, a press (or Return) opens what the figure
 * is made of and Escape closes it (Native Shell's Popover): the keyboard,
 * touch and screen readers reach it as the pointer does. The one inline
 * figure; in a row of tiles, `Stat`. */
export function Figure(props: { value: string; label: string; /** `client`: failures that are all the program's own (4xx), in the tone their rows have */ tone?: Tone; small?: boolean; preview?: JSX.Element; testID?: string }) {
  // the figure in the text's colour; its tone (a status) is a dot before what it counts
  const marked = () => !!props.tone && props.tone !== "plain" && props.tone !== "quiet";
  const body = (
    <View class="gap-1">
      <Text class={`tabular-nums font-bold ${props.small ? "text-body" : "text-heading"} text-foreground`}>{props.value}</Text>
      <View class="flex-row items-center gap-1">
        <If when={marked()}>
          <View class={`${DOT} ${TONE[props.tone!].dot}`} />
        </If>
        <Text class={`${TEXT.meta} line-clamp-1`}>{props.label}</Text>
      </View>
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
        <BaseButton testID={props.testID} accessibilityLabel={`${props.value} ${props.label}`} class={`-mx-2 -my-1 rounded-(--radius-sm) px-2 py-1 hover:bg-muted active:bg-muted ${TRANSITION} ${FOCUS}`}>
          {body}
        </BaseButton>
      }
      // (its entrance runs on the popup's own transition: without one, the starting look was a jump)
      popupClass={`w-[300px] gap-2 ${POPUP} p-3 ${POPUP_MOTION.class}`}
      popupStyle={glass}
      popupStartingClass={POPUP_MOTION.starting}
      popupEndingClass={POPUP_MOTION.ending}
    >
      {props.preview}
    </Popover>
  );
}

/** How many tiles go in a row `width` wide, of `n` that each need `min`: all
 * abreast where they fit, else rows of equal count (four are 2×2, six 3×3
 * or 2×3), never a row shorter than the others (five that do not fit are
 * a line each, never three and two, nor tight enough to cut a label). Where no
 * equal rows fit, each takes a line of its own (one column, and `line`: what
 * it counts at the line's start, its figure at its end) — a tile is never
 * so narrow that its label or its figure is cut. */
export function tilesPerRow(width: number, n: number, min: number, gap = 12): number {
  if (width <= 0) return n;
  const fits = (k: number, least: number) => (width - gap * (k - 1)) / k >= least;
  if (fits(n, min)) return n;
  for (let k = n - 1; k >= 2; k--) if (n % k === 0 && fits(k, min)) return k;
  // no equal rows in a pane of some width: as many abreast as fit (two at least, the last row the
  // shorter), never one per row — a column of one-line tiles reads as a list, not as figures
  if (width >= TILES_ABREAST) return Math.max(2, Math.min(n - 1, Math.floor((width + gap) / (min + gap))));
  // a pane too narrow for two: a line each
  return 1;
}
/** From this width, tiles never stand one per row. */
const TILES_ABREAST = 560;
/** How many tiles a single row `width` wide holds, of `n` that each need `min`: two at least. */
export const tilesFit = (width: number, n: number, min: number, gap = 12) => (width > 0 ? Math.min(n, Math.max(2, Math.floor((width + gap) / (min + gap)))) : n);
/** Tiles in rows by that rule: the row measures itself and gives each tile its width.
 * `ranked` keeps them to one row: as many as fit, in the order given (their
 * rank), the last ranked left out: a row of tiles broken in two reads as two.
 * A tile given as a function is told its `TileLook`: `line` in a pane under
 * `BREAK.short` (`short`: the rows under the tiles need the room) and where
 * the tiles stand one to a row, `tight` where it is narrower than `min`,
 * `full` otherwise. */
export function Tiles(props: { items: (JSX.Element | ((look: () => TileLook) => JSX.Element))[]; min?: number; testID?: string; ranked?: boolean; label?: string; /** the pane's height is under `BREAK.short` */ short?: boolean }) {
  const GAP = 12;
  const width = signal(0);
  const min = () => props.min ?? 144;
  const per = () => (props.ranked ? tilesFit(width(), props.items.length, min(), GAP) : tilesPerRow(width(), props.items.length, min(), GAP));
  const each = () => (width() > 0 ? Math.floor((width() - GAP * (per() - 1)) / per()) : undefined);
  const column = () => !props.ranked && per() === 1 && props.items.length > 1;
  // the one-line tile only where tiles stand one per row: abreast, a short pane keeps what each counts over
  // its figure (a name squeezed beside a figure broke inside its words: "Request / s")
  const alone = () => per() === 1 && props.items.length > 1;
  const look = (): TileLook => (alone() ? "line" : props.short || (each() ?? min()) < min() ? "tight" : "full");
  return (
    <View testID={props.testID} accessibilityLabel={props.label} class={`flex-row ${column() ? "gap-2" : "gap-3"} ${props.ranked ? "" : "flex-wrap"}`} onLayout={({ bounds }) => width.set(Math.round(bounds.size.width))}>
      {props.items.map((tile, i) => (
        // (a tile fills its place in the row: tiles abreast are of one height)
        <View style={each() ? { width: each() } : undefined} class={props.ranked && i >= per() ? "hidden" : each() ? "flex-col" : "min-w-36 flex-1 flex-col"}>
          {typeof tile === "function" ? tile(look) : tile}
        </View>
      ))}
    </View>
  );
}

/** A modal dialog: one surface over the scrim, in one of two widths — 480
 * for a question or a short form, 640 for a dialog that lists (a plan, its
 * verdicts, a comparison). */
const MODAL = { default: "w-[480px]", wide: "w-[640px]" } as const;

export function Modal(props: { open: boolean; onClose: () => void; size?: keyof typeof MODAL; children: JSX.Element; testID?: string; /** what the dialog asks or says, in a sentence: a screen reader hears it after the dialog's name, as it opens */ describe?: string; /** one frame for all its states (a plan, its progress, its verdicts): as high as the window allows, its `ModalBody` scrolling inside, so it never changes size as it goes */ steady?: boolean }) {
  return (
    <Dialog
      open={props.open}
      // every centred dialog comes in one way: the palette's fade (none under reduced motion)
      animation="fade"
      accessibilityDescription={props.describe}
      onOpenChange={(e) => !e.open && props.onClose()}
      backdropClass={SCRIM}
      testID={props.testID}
      // never taller than the window: a body that grows (`ModalBody`) scrolls, its title and actions stay
      class={`m-auto ${MODAL[props.size ?? "default"]} ${props.steady ? "h-[640px]" : ""} max-h-[92%] max-w-[94%] gap-4 ${CARD} bg-popover p-4 ${SHADOW.modal}`}
    >
      {props.children}
    </Dialog>
  );
}

/** A dialog's part that grows with what it holds (a plan, a list of verdicts):
 * it takes what the window leaves under the title and over the actions, and
 * scrolls there, so the dialog's actions are always in view. Where more lies
 * past an edge, that edge fades (in a `steady` dialog it fills the frame). */
export function ModalBody(props: { children: JSX.Element; testID?: string; class?: string }) {
  const FADE = 16;
  // where the view is over what it holds: its height, its content's, how far it has scrolled
  const view = signal(0);
  const content = signal(0);
  const offset = signal(0);
  const mask = () => {
    const h = view();
    const top = offset() > 1;
    const bottom = offset() + h < content() - 1;
    if ((!top && !bottom) || h <= FADE * 2) return undefined;
    return `linear-gradient(to bottom, ${top ? "transparent" : "black"} 0px, black ${FADE}px, black ${h - FADE}px, ${bottom ? "transparent" : "black"} ${h}px)`;
  };
  return (
    <Scroll
      testID={props.testID}
      class="min-h-0 shrink grow"
      scrollEventThrottle={50}
      onScroll={(e) => offset.set(Math.round(e.offset.y))}
      onLayout={({ bounds }) => view.set(Math.round(bounds.size.height))}
      style={terminal() ? undefined : { maskImage: mask() }}
    >
      <View class={`gap-3 ${props.class ?? ""}`} onLayout={({ bounds }) => content.set(Math.round(bounds.size.height))}>
        {props.children}
      </View>
    </Scroll>
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
            <View class={`group-aria-expanded:rotate-90 ${TRANSITION}`}>
              <Icon name="chevron-right" size={ICON.small} tint="muted-foreground" />
            </View>
            <Text class={`${TEXT.body} font-bold`}>{props.title}</Text>
            <If when={props.count !== undefined}>
              <Text class={TEXT.bodyQuiet}>{`· ${count(props.count ?? 0)}`}</Text>
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

/** A state or a standing ("Broke", "Latest", "Missing"): a quiet word in its
 * tone, on no wash and in no pill — a verdict is read, it does not shout.
 * A neutral one (`quiet`, `plain`) is muted. */
export function Badge(props: { label: string; tone?: Tone; testID?: string }) {
  const tone = () => props.tone ?? "quiet";
  return (
    <View testID={props.testID} accessibilityLabel={props.label} class="h-5 shrink-0 flex-row items-center">
      <Text class={`text-small ${tone() === "plain" ? "text-muted-foreground" : TONE[tone()].text}`}>{props.label}</Text>
    </View>
  );
}

/** Names and their values, the one list of them: bare (no box, no rules
 * between its columns), a hairline between rows; names sans and muted,
 * values selectable, in mono only where they are identifiers (`mono`: ids,
 * hashes, headers' values). `dense` is the smaller size of an inspector's
 * long lists (a request's headers). Inside a card or the inspector it is
 * never boxed again. */
export function KeyValue(props: { rows: [string, string][]; mono?: boolean; dense?: boolean }) {
  const size = () => (props.dense ? "text-small" : "text-body");
  return (
    // a table to a screen reader: names in the first column, values in the second
    <View selectable>
      <Table>
        <TableBody>
          <Each of={props.rows}>
            {(row, i) => (
              // off-scale: a long list's rows (6 pt around 20 pt lines) sit between the dense (4) and a card's (8)
              <TableRow class={`flex-row gap-3 ${props.dense ? "py-1" : "py-1.5"} ${i() > 0 ? "border-t border-border" : ""}`}>
                <TableCell class={`${props.dense ? "w-[136px]" : "w-44"} shrink-0`}>
                  <Text class={`${size()} text-muted-foreground line-clamp-1`}>{row()[0]}</Text>
                </TableCell>
                <TableCell class="min-w-0 flex-1">
                  <Text class={`${size()} text-foreground ${props.mono ? "font-mono" : ""}`}>{row()[1]}</Text>
                </TableCell>
              </TableRow>
            )}
          </Each>
        </TableBody>
      </Table>
    </View>
  );
}

/** A line of text to type. `bare` leaves its label to screen readers (and
 * its placeholder): a field in a toolbar row, on the row's one middle.
 * `leading` stands before the text inside the field (a prompt, an icon) and
 * `trailing` after it (an `IconButton inField`); `small` is the second
 * control height. A field that matters more is wider and in mono, never higher. */
export function Field(props: { label: string; value: string; onChange: (v: string) => void; onSubmit?: () => void; placeholder?: string; testID?: string; mono?: boolean; class?: string; bare?: boolean; small?: boolean; leading?: JSX.Element; trailing?: JSX.Element; /** shown and selectable, not changed (Native Shell's `readonly`) */ readonly?: boolean; /** not for now: no focus, no typing, dimmed (Native Shell's `disabled`) */ disabled?: boolean; /** what is typed is refused: said under the field, drawn on its edge, and told to a screen reader (the field is `invalid`, the reason its description) */ error?: string | null; /** refused without words of its own here (the reason stands beside it, or is the label's): the edge and the state only */ invalid?: boolean }) {
  const boxed = () => !!props.leading || !!props.trailing;
  const refused = () => !!props.error || !!props.invalid;
  const text = () => `${props.small ? "text-small" : "text-body"} text-foreground placeholder:text-muted-foreground ${props.mono ? "font-mono" : ""} ${DISABLED}`;
  // the reason under the field is the field's description to a screen reader, read again each time the field is
  // reached (Native Shell's `accessibilityDescribedBy`, the web's `aria-describedby`)
  const reason = signal<ViewRef | null>(null);
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
      accessibilityState={refused() ? { invalid: true } : undefined}
      accessibilityDescribedBy={props.error ? reason() : null}
      onChange={({ value }) => props.onChange(value)}
      onSubmit={props.onSubmit ? () => props.onSubmit?.() : undefined}
      submitBehavior={props.onSubmit ? "submit" : undefined}
      class={cls}
    />
  );
  return (
    <View class={`gap-1 ${props.class ?? ""}`}>
      <If when={!props.bare}>
        <Text class={TEXT.note}>{props.label}</Text>
      </If>
      <If when={boxed()} else={input(`${props.small ? CONTROL.small : CONTROL.field} rounded-(--radius) border ${refused() ? "border-fail" : "border-input"} bg-background px-2 focus-visible:border-ring ${FOCUS} ${text()}`)}>
        <View class={`${props.small ? CONTROL.small : CONTROL.field} ${CONTROL.box} ${refused() ? "border-fail" : ""}`}>
          {props.leading}
          {input(`min-w-0 flex-1 ${text()}`)}
          {props.trailing}
        </View>
      </If>
      {/* why it is refused, under it: the same words a screen reader is given as the field's description */}
      <If when={props.error}>{(why) => <Text ref={(r) => reason.set(r)} testID={props.testID ? `${props.testID}-error` : undefined} class={TEXT.error}>{why()}</Text>}</If>
    </View>
  );
}

/** Where the keyboard rests when no field has it. On GTK a window whose
 * focused field is unfocused by the program (a dialog opens after another
 * window closed, a field goes with its page) gives its first text field the
 * focus: the toolbar's endpoint then stood ringed, its name selected, in
 * every picture (FINDINGS 22). The first text field is this one: nothing to
 * see, nothing to read out, nothing to type into. */
export function FocusRest() {
  return (
    <If when={platform.os === "linux" && !terminal()}>
      <Input testID="focus-rest" value="" readonly accessibilityElementsHidden class="absolute left-0 top-0 h-px w-px opacity-0" />
    </If>
  );
}

/** A control that has no label of its own (a row of options, a switch, a
 * value only shown) under the label a `Field` has, on a field's height: in a
 * form, every control's name is above it. */
export function Labeled(props: { label: string; children: JSX.Element; class?: string }) {
  return (
    <View class={`gap-1 ${props.class ?? ""}`}>
      <Text class={TEXT.note}>{props.label}</Text>
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
        class={`min-w-0 flex-1 ${TEXT.body} placeholder:text-muted-foreground`}
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
      inputClass={terminal() ? undefined : `min-w-0 flex-1 px-2 ${TEXT.body} tabular-nums placeholder:text-muted-foreground`}
      controlsClass={terminal() ? undefined : "h-full border-l border-input"}
      incrementButtonClass={terminal() ? undefined : `flex-1 items-center justify-center px-2 hover:bg-muted active:bg-accent ${TRANSITION}`}
      decrementButtonClass={terminal() ? undefined : `flex-1 items-center justify-center px-2 hover:bg-muted active:bg-accent ${TRANSITION}`}
      incrementButton={terminal() ? undefined : <Icon name="chevron-up" size={ICON.small} />}
      decrementButton={terminal() ? undefined : <Icon name="chevron-down" size={ICON.small} />}
      suffix={props.suffix ? <Text class={`pr-2 ${TEXT.note}`}>{props.suffix}</Text> : undefined}
    />
  );
  if (props.bare) return <View class={props.class}>{input}</View>;
  return (
    <View class={`gap-1 ${props.class ?? ""}`}>
      <Text class={TEXT.note}>{props.label}</Text>
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
      class={`flex-row items-center gap-3 rounded-(--radius-sm) ${DISABLED} ${FOCUS}`}
      // the thumb centered in its track, top to bottom; off, quieter than on (a white thumb off shone on dark)
      // off-scale: the thumb (16) sits 2 pt inside its track (20)
      trackClass={`h-5 w-9 shrink-0 justify-center rounded-full bg-input px-0.5 checked:bg-primary ${TRANSITION}`}
      thumbClass={`h-4 w-4 rounded-full bg-switch-thumb ${SHADOW.control} checked:translate-x-4 checked:bg-primary-foreground ${GLIDE}`}
    >
      <View class="flex-1">
        <Text class={TEXT.body}>{props.label}</Text>
        <If when={props.hint}>
          <Text class={TEXT.note}>{props.hint}</Text>
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
      class={`flex-row items-center gap-2 rounded-(--radius-sm) ${DISABLED} ${FOCUS}`}
      indicatorClass={`h-4 w-4 items-center justify-center rounded-sm border border-input bg-background checked:border-primary checked:bg-primary ${TRANSITION}`}
      indicator={<Icon name="check" size={ICON.small} stroke={3} tint="primary-foreground" />}
    >
      <Text class={TEXT.body}>{props.label}</Text>
    </BaseCheckbox>
  );
}

export function Choice(props: { label?: string; value: string | null; items: { value: string; title: string }[]; onChange: (v: string) => void; placeholder?: string; testID?: string; class?: string; /** the label beside the choice, not over it (a control in a toolbar's row) */ inline?: boolean }) {
  const title = () => props.items.find((i) => i.value === props.value)?.title ?? props.placeholder ?? "";
  return (
    <View class={`${props.inline ? "flex-row items-center gap-2" : "gap-1"} ${props.class ?? ""}`}>
      <If when={props.label}>
        <Text class={TEXT.note}>{props.label}</Text>
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
        popupClass={`${POPUP} ${POPUP_MOTION.class}`}
        popupStyle={glass}
        popupStartingClass={POPUP_MOTION.starting}
        popupEndingClass={POPUP_MOTION.ending}
        itemClass={`${ROWS.popup} text-body text-popover-foreground selected:font-bold`}
        trigger={
          <View class={`h-8 flex-row items-center justify-between gap-2 rounded-(--radius) border border-input bg-background px-2 hover:border-ring/60 ${TRANSITION} ${props.inline ? WIDTH.choice : ""} ${FOCUS}`}>
            <Text class={`min-w-0 flex-1 ${TEXT.body} line-clamp-1`}>
              {title()}
            </Text>
            <View class="shrink-0">
              <Icon name="chevrons-up-down" size={ICON.small} />
            </View>
          </View>
        }
      />
    </View>
  );
}

/** One of a few views or modes: Tabs, a pill sliding behind the chosen one.
 * A view that has nothing to show here keeps its tab, `disabled` (dimmed,
 * no press; `why` is what a screen reader hears after its name): tabs do
 * not come and go with what they would show. */
export function Segmented(props: { value: string; items: { value: string; title: string; icon?: IconName; disabled?: boolean; why?: string }[]; onChange: (v: string) => void; testID?: string; class?: string }) {
  return (
    <Tabs testID={props.testID} class={`${CONTROL.track} ${props.class ?? ""}`} indicatorClass={`inset-y-0.5 rounded-(--radius-sm) bg-segment ${SHADOW.control} ${GLIDE}`}>
      <Each of={props.items} key={(i) => i.value}>
        {(item) => (
          <Tab
            testID={props.testID ? `${props.testID}-${item().value}` : undefined}
            accessibilityLabel={item().disabled && item().why ? `${item().title}, ${item().why}` : item().title}
            selected={props.value === item().value}
            disabled={item().disabled}
            onPress={() => props.onChange(item().value)}
            class={`group ${CONTROL.segment} hover:bg-foreground/5 ${DISABLED} disabled:hover:bg-transparent ${PRESS.control} ${FOCUS}`}
          >
            <If when={item().icon}>
              <Icon name={item().icon!} size={ICON.small} tint={props.value === item().value ? "foreground" : "muted-foreground"} />
            </If>
            <Text class={`${TEXT.note} group-selected:text-foreground`}>{item().title}</Text>
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
            class={`group ${CONTROL.segment} hover:bg-foreground/5 checked:bg-segment checked:${SHADOW.control} ${PRESS.control} ${FOCUS}`}
          >
            <If when={item().icon}>
              <Icon name={item().icon!} size={ICON.small} tint={props.value === item().value ? "foreground" : "muted-foreground"} />
            </If>
            <Text class={`${TEXT.note} group-checked:text-foreground`}>{item().title}</Text>
          </Radio>
        )}
      </Each>
    </RadioGroup>
  );
}

/** A status mark: the one dot (`DOT`) in its tone, with its meaning for screen readers. */
export function Dot(props: { tone: Tone; label?: string }) {
  return <View accessibilityLabel={props.label} class={`${DOT} ${TONE[props.tone].dot}`} />;
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
              <Text class={TEXT.note}>{props.description}</Text>
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

/** How far something has got: `value` a fraction, 0–1, or `null` while how
 * much is not known. A known value glides to each new one (`TIMING.base`); an
 * unknown one is a segment sweeping along the track, still under reduced
 * motion. Screen readers hear a progress bar and its value (Native Shell's
 * Progress). Not a chart's bar: that is the plots' `Bar`. */
export function ProgressBar(props: { value: number | null; label?: string }) {
  const width = signal(0);
  const sweep = createAnimatedValue(0);
  // the unknown's segment: a third of the track, from before its start to past its end, again and again
  effect(() => {
    const w = width();
    if (props.value !== null || w === 0 || terminal() || reduceMotion()) return;
    sweep.set(-w / 3);
    const loop = Animated.loop(Animated.timing(sweep, { toValue: w, duration: 1400, easing: Easing.inOut(Easing.cubic) }), { resetBeforeIteration: true });
    loop.start();
    cleanup(() => loop.stop());
  });
  // a known value: a fill the track's width, slid out of the track's start by what is left; it glides once the
  // track is measured (not from full as it first lays out)
  const left = () => (props.value === null ? 0 : (1 - Math.min(1, Math.max(0, props.value))) * width());
  const settled = signal(false);
  effect(() => {
    if (width() > 0 && !settled()) setTimeout(() => settled.set(true), 0);
  });
  return (
    <View class="gap-1">
      <Progress
        // Native Shell's Progress counts 0–100 and gives the value to screen readers; a window's bar is drawn here,
        // inside it (a terminal draws its own)
        value={props.value === null ? undefined : Math.min(100, Math.max(0, props.value * 100))}
        indeterminate={props.value === null}
        accessibilityLabel={props.label ?? t("common.progress")}
        trackClass={terminal() ? "h-1.5 rounded-full bg-muted" : "hidden"}
        indicatorClass="h-1.5 rounded-full bg-primary"
      >
        <If when={!terminal()}>
          <View class="h-1.5 overflow-hidden rounded-full bg-muted" onLayout={({ bounds }) => width.set(Math.round(bounds.size.width))}>
            <If
              when={props.value !== null}
              else={<View class={`absolute bottom-0 top-0 left-0 rounded-full bg-primary ${reduceMotion() ? "w-full opacity-40" : "w-1/3"}`} style={reduceMotion() ? undefined : { translate: [sweep, 0] }} />}
            >
              <View class={`h-1.5 w-full rounded-full bg-primary ${settled() ? GLIDE : ""}`} style={{ translate: [-left(), 0] }} />
            </If>
          </View>
        </If>
      </Progress>
      <If when={props.label}>
        <Text class={TEXT.note}>{props.label}</Text>
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
        <Text class={`text-center ${TEXT.note}`}>{props.text}</Text>
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
      <Text class={`max-w-[380px] text-center ${TEXT.bodyQuiet}`}>{props.text}</Text>
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
      <View class={`${DOT} bg-muted-foreground/60 motion-safe:animate-pulse`} />
      <Text class={TEXT.note}>{props.text ?? t("common.loading")}</Text>
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
 * column as wide as what it holds. `max` is how many rows show before it
 * scrolls (every row is there: none is left out). `end` names the columns of
 * figures, set at their cell's end under their titles; `fill` the one column
 * that takes the room the others leave, so the table is as wide as its
 * place; `cell` draws a cell its own way (a status with its dot), and
 * returns nothing for the cells it leaves as text. From `ranked` on, the
 * columns go by rank where the table is narrow (`fitColumns`: the last
 * first, whole, never clipped). With `onSelect` a row is chosen by a press
 * (by its place in `rows`, whatever the sort; again lets it go), marked as
 * `selected`. */
export function SimpleTable(props: { columns: string[]; rows: string[][]; mono?: boolean; max?: number; end?: number[]; fill?: number; ranked?: number; selected?: number | null; onSelect?: (row: number | null) => void; label?: string; cell?: (c: { column: number; text: string; /** the row's cells, as text */ cells: string[] }) => JSX.Element | undefined; testID?: string; /** inside a card or a chart: no box of its own, hairlines between its rows only */ bare?: boolean }) {
  const sort = signal<DataTableSort | null>(null);
  const width = signal(0);
  // the widths measured by Native Shell once the rows are in (CJK, mono and proportional alike);
  // a table with a column that fills keeps its own widths, which add up to its place
  let table: DataTableRef | undefined;
  effect(() => {
    props.rows;
    if (terminal() || !table || props.fill !== undefined) return;
    const t = table;
    setTimeout(() => props.columns.forEach((_, c) => void t.fitColumn(`c${c}`).catch(() => {})), 0);
  });
  // a character's width, about: CJK is twice a Latin letter
  const wide = (s: string) => [...s].reduce((n, ch) => n + (ch.charCodeAt(0) > 0x2e7f ? 2 : 1), 0);
  const per = () => (props.mono ? 7.4 : 6.6);
  const columns = () => {
    const own = props.columns.map((title, c) => {
      // a title has its sort mark beside it; a cell its inset
      const longest = Math.max(wide(title) * per() + 44, ...props.rows.slice(0, 400).map((r) => wide(r[c] ?? "") * per() + 24));
      return Math.round(Math.min(360, Math.max(56, longest)));
    });
    // the ranked columns that fit beside the rest, the one that fills keeping its own width
    const from = terminal() || props.ranked === undefined ? props.columns.length : props.ranked;
    const kept = new Set(fitColumns(width(), own.reduce((n, w, c) => (c < from ? n + w : n), 2), 0, own.slice(from).map((w, k) => ({ key: `${from + k}`, width: w }))));
    const shown = props.columns.map((_, c) => c).filter((c) => c < from || kept.has(`${c}`));
    const rest = shown.reduce((n, c) => (c === props.fill ? n : n + own[c]), 0);
    return shown.map((c) => ({ key: `c${c}`, title: props.columns[c], width: c === props.fill && width() > 0 ? Math.max(own[c], width() - rest - 2) : own[c], sortable: true, align: props.end?.includes(c) ? ("end" as const) : undefined }));
  };
  // the rows by their place in `rows`, in the order the sort says
  const order = () => sortRows(props.rows.map((_, i) => i), sort(), (i, key) => sortValue(props.rows[i][Number(key.slice(1))] ?? ""));
  const sorted = () => order().map((i) => props.rows[i]);
  const data = () => sorted().map((r) => Object.fromEntries(r.map((cell, c) => [`c${c}`, cell])));
  const shown = () => Math.min(props.rows.length, props.max ?? 12);
  const custom = () => !terminal() && (!!props.cell || !!props.end?.length);
  const cell = (c: { row: number; key: string; text: string }) => {
    const column = Number(c.key.slice(1));
    const own = props.cell?.({ column, text: c.text, cells: sorted()[c.row] ?? [] });
    const end = props.end?.includes(column);
    const body = own ?? <Text class={`${TEXT.small} line-clamp-1 ${end ? "tabular-nums" : ""} ${props.mono ? "font-mono" : ""}`}>{c.text}</Text>;
    // figures line up at their cell's end, under their titles
    return end ? <View class="flex-1 items-end justify-center">{body}</View> : body;
  };
  return (
    <DataTable
      ref={(r) => (table = r)}
      testID={props.testID}
      columns={columns()}
      data={data()}
      sort={sort()}
      onSortChange={({ sort: next }) => sort.set(next)}
      fontSize={TYPE.small}
      fontFamily={props.mono ? token("font-mono") : undefined}
      rowHeight={rowHeight(ROW.dense)}
      classes={terminal() ? undefined : props.bare ? BARE_TABLE : TABLE}
      renderCell={custom() ? cell : undefined}
      accessibilityLabel={props.label}
      rowSelectable={!!props.onSelect}
      rowClasses={props.onSelect ? order().map((i) => (i === props.selected ? SELECTED : "")) : undefined}
      onRowPress={props.onSelect ? ({ row }) => { const i = order()[row]; if (i !== undefined) props.onSelect!(i === props.selected ? null : i); } : undefined}
      class={props.bare ? "" : CARD}
      onLayout={({ bounds }) => width.set(Math.round(bounds.size.width))}
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

/** The head of every Buckets section, built one way (`PageHeader`'s
 * pattern): the section's title, what it looks at (`scope`: a bucket and
 * prefix), then at the row's end its actions (`children`: its command,
 * Cancel while it runs) and what else it offers (`end`: a `More`). One
 * middle, small controls; a narrow pane wraps the actions under the title,
 * never into it. */
export function ToolBar(props: { title?: string; scope?: string; children?: JSX.Element; end?: JSX.Element; testID?: string }) {
  return (
    <View testID={props.testID} class="min-h-8 flex-row flex-wrap items-center gap-x-3 gap-y-2">
      <View class="min-w-0 shrink flex-row items-baseline gap-2">
        <If when={props.title}>
          <Title lines={1} class="shrink-0">
            {props.title}
          </Title>
        </If>
        <If when={props.scope}>{(s) => <Path path={s()} max={40} class="min-w-0 shrink text-muted-foreground" />}</If>
      </View>
      <View class="flex-1" />
      <If when={props.children}>
        <View class="flex-row flex-wrap items-center gap-2">{props.children}</View>
      </If>
      {props.end}
    </View>
  );
}

/** A command in a menu: its words, its icon, what it runs. */
export type MenuAction = { label: string; icon?: IconName; run: () => void; disabled?: boolean };
/** The system's menu, of the app's commands (`MenuAction`) or of Native
 * Shell's own items (a separator, a submenu, a checked item). With `anchor`
 * (the button that opened it) it opens under that button, its start on the
 * button's, as a menu from a button does; at a point (a context menu's
 * `pagePosition`) there; without, at the pointer. Resolves when it closes.
 * Where the system has no menus (a terminal, the web) it does nothing: what
 * offers it there offers its commands another way. */
export async function popupMenu(items: readonly (MenuAction | MenuItemSpec | false | null | undefined)[], anchor?: ViewRef | { x: number; y: number } | null): Promise<void> {
  if (!isAvailable) return;
  let at: { x: number; y: number } | undefined;
  if (anchor && "measure" in anchor) {
    const m = await anchor.measure().catch(() => null);
    if (m) at = { x: m.pagePosition.x, y: m.pagePosition.y + m.bounds.size.height + 4 };
  } else if (anchor) at = anchor;
  const native = items.map((i) => (i && "run" in i ? { label: i.label, icon: i.icon ? iconSvg(i.icon) : undefined, disabled: i.disabled, onSelect: i.run } : i));
  await Menu.popup(native, at);
}

/** What else a place offers (exports, copies): a `⋯` button opening the
 * system's menu under it; where there is none (the web), the commands as small buttons. */
export function More(props: { actions: () => MenuAction[]; testID?: string }) {
  let button: ViewRef | undefined;
  return (
    <If
      when={isAvailable}
      else={
        <Each of={props.actions()}>{(a) => <Button small variant="outline" icon={a().icon} label={a().label} onPress={a().run} disabled={a().disabled} />}</Each>
      }
    >
      <IconButton ref={(r) => (button = r)} icon="ellipsis" label={t("common.more")} testID={props.testID} onPress={() => void popupMenu(props.actions(), button)} />
    </If>
  );
}

/** Commands in one row that never wraps: in rank order, as many as `room`
 * (points) holds, each a small outline button; the rest behind `⋯`, never on
 * a second line. Each button measures itself while it shows, so what does
 * not fit is known without a threshold; before anything is measured all show. */
export function ActionRow(props: { room: number; actions: () => { id: string; label: string; icon?: IconName; run: () => void; disabled?: boolean; tip?: string }[]; testID?: string; /** by their words alone (no icons): a narrow place's row */ plain?: boolean; /** each button's width as measured, by its id */ onWidths?: (w: Record<string, number>) => void }) {
  const GAP = 8;
  const widths = signal<Record<string, number>>({});
  // (measured by their words alone and with their icons, apart: a row that changes how it shows them
  // shows them all until they are measured again, so none is kept hidden by its other width)
  const key = (id: string) => `${props.plain ? "plain" : "icon"}:${id}`;
  const shown = () => {
    const list = props.actions();
    const all = widths();
    const w: Record<string, number> = Object.fromEntries(list.map((a) => [a.id, all[key(a.id)]]));
    // a terminal lists them all; so does a row not measured yet
    if (terminal() || list.some((a) => w[a.id] === undefined)) return list.length;
    if (list.reduce((n, a) => n + w[a.id] + GAP, 0) <= props.room) return list.length;
    // not all: room is kept for the `⋯` (an icon button and its gap)
    let used = 28 + GAP;
    let k = 0;
    for (const a of list) {
      if (used + w[a.id] + GAP > props.room) break;
      used += w[a.id] + GAP;
      k += 1;
    }
    return k;
  };
  return (
    <View testID={props.testID} class="shrink-0 flex-row items-center gap-2">
      <Each of={props.actions()} key={(a) => a.id}>
        {(a, i) => (
          <View
            class={i() < shown() ? "" : "hidden"}
            onLayout={({ bounds }) => {
              const w = Math.round(bounds.size.width);
              if (w > 0 && widths()[key(a().id)] !== w) {
                widths.set({ ...widths(), [key(a().id)]: w });
                props.onWidths?.({ [a().id]: w });
              }
            }}
          >
            <Button small variant="outline" icon={props.plain ? undefined : a().icon} label={a().label} tip={a().tip} onPress={a().run} disabled={a().disabled} testID={a().id} />
          </View>
        )}
      </Each>
      <If when={shown() < props.actions().length}>
        <More testID={props.testID ? `${props.testID}-more` : undefined} actions={() => props.actions().slice(shown())} />
      </If>
    </View>
  );
}

/** A press area around what is not a button's label (a verdict's badge in a
 * cell): hover, press and focus as every control, the place it stands in
 * decided where it is placed. */
export function Pressable(props: { label: string; onPress: () => void; children: JSX.Element; testID?: string }) {
  return (
    <BaseButton testID={props.testID} accessibilityLabel={props.label} onPress={props.onPress} class={`rounded-(--radius-sm) px-1 py-1 hover:bg-muted active:bg-muted ${PRESS.control} ${FOCUS}`}>
      {props.children}
    </BaseButton>
  );
}

/** A part of a path, as a link to it (Native Shell's Link: a screen reader
 * hears a link, and which one is the current place). */
export function Crumb(props: { href: string; onOpen: () => void; current?: boolean; label: string; children: JSX.Element; testID?: string }) {
  return (
    <Link testID={props.testID} href={props.href} openWith="app" onOpen={props.onOpen} current={props.current} accessibilityLabel={props.label} class={`flex-row items-center gap-1 rounded-(--radius-sm) px-1 py-1 hover:bg-muted ${TRANSITION} ${FOCUS}`}>
      {props.children}
    </Link>
  );
}

/** A value along a range, dragged or stepped with the arrows (Native Shell's Slider). */
export function Scrubber(props: { label: string; value: number; min: number; max: number; step?: number; onChange: (v: number) => void; testID?: string }) {
  return (
    <Slider
      testID={props.testID}
      accessibilityLabel={props.label}
      value={props.value}
      min={props.min}
      max={props.max}
      step={props.step}
      onChange={({ value }) => props.onChange(typeof value === "number" ? value : value[0])}
      class="h-6 justify-center"
      trackClass="h-1.5 rounded-full bg-muted"
      indicatorClass="h-1.5 rounded-full bg-primary"
      thumbClass={`size-4 rounded-full border border-border bg-background ${SHADOW.control}`}
    />
  );
}

/** A short question before something goes for good, in the system's own
 * alert: Cancel, and the commit in the destructive role. True when it was
 * confirmed. */
export async function confirmAlert(ask: { title: string; text: string; commit: string }): Promise<boolean> {
  const { action } = await AlertDialog.show({
    title: ask.title,
    description: ask.text,
    actions: [
      { id: "cancel", label: t("common.cancel"), role: "cancel" },
      { id: "commit", label: ask.commit, role: "destructive" },
    ],
  });
  return action === "commit";
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
            <MenuItem onSelect={a().run} class={`flex-row items-center gap-2 ${ROWS.popup}`}>
              <If when={a().icon}>{(_) => <Icon name={a().icon!} />}</If>
              <Text class={TEXT.body}>{a().label}</Text>
            </MenuItem>
          )}
        </Each>
      </ContextMenu>
    </If>
  );
}
