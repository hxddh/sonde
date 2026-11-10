# sonde's design standards

The app's own decisions on top of Native Shell's Design Guides
(`skills/nsh-design-guides` in the nsh checkout). Pages compose the components in
`src/ui/`; they do not restyle `nsh/base` controls or invent sizes of their own.

## The product

One task: see what a program does to S3. Three places — **Run** (start a program and
watch its requests become operations), **Sessions** (what a run did, what stands out,
compared and replayed), **Buckets** (what is in a store). The window has two columns
and no more: the **sidebar** (the places, and under them the current place's own list —
the commands run before, the sessions, the buckets) and the place; a detail opens
beside the place as an inspector, and nothing else moves when it does. Everything configured once
lives in **Settings** (a window); what sonde itself is doing lives in **Activity** (a
side panel). The core result is *what stands out in a session*: it gets the space, the
words and the next action; counts and chips support it, they do not replace it.

## Platforms

| Target | Form |
|---|---|
| macOS | Hidden title bar with the app's toolbar; the window's sidebar material behind the sidebar (`window.backdrop: "sidebar"`); glass on floating surfaces |
| Linux, Windows | The same layout with opaque fills; the menus behind the toolbar's menu button |
| Terminal (`sonde tui`) | A home list and a navigation stack; tables and sentences instead of charts; text labels for actions |
| Web (`sonde share`) | The session views only, read-only |

## Type

A desktop scale around a 13 pt body (`nsh.json` `tailwind.fontSize`, `TYPE` in
`src/ui/kit.tsx` for numeric props):

| Class | Points | For |
|---|---|---|
| `text-caption` | 11 | metadata, column headers, badges, shortcuts |
| `text-small` | 12 | secondary lines, help under a field |
| `text-body` | 13 | everything else: rows, labels, buttons, prose |
| `text-heading` | 15 | a panel's or section's title, a figure |
| `text-title` | 20 | a page's title (welcome, an empty Run) |

The interface is the platform's font, and monospace the platform's (SF Mono or Menlo,
Cascadia Mono or Consolas, DejaVu Sans Mono): for identifiers, keys, commands, URLs,
code and the columns of times. Figures outside tables use `tabular-nums` so live
counters do not jitter. (A shipped face was tried and dropped: text in a registered
font is measured narrower than it is drawn — FINDINGS 15.)

## Color

Theme tokens only (`theme.json`, light and dark). Status has its own roles —
`status-ok`, `status-miss`, `status-client`, `status-server`, `status-refused`,
`warn`, `info` — and formats theirs (`fmt-*`); never a palette color in a page. The
scrim under modal layers is `bg-scrim`. Color never carries meaning alone: a dot sits
next to a status code, a warning has its icon and its words.

## The kit

Native Shell's base components have behavior and no look; the look is `src/ui/kit.tsx`'s, and only there. Each kit
piece wraps one nsh export with the theme's tokens:

| Piece | Over | Use |
|---|---|---|
| `Button`, `IconButton` | `Button` | primary (one per area), outline (default), ghost (dense rows, toolbars), destructive |
| `Card` | `View` / `Button` | `panel` (p-4, a group), `tile` (a figure in a row), `none` (a list drawing its rows); `onPress` makes the card the button |
| `ListRow` | `Button` | a row a press opens: hover, pressed, selected, focus |
| `Field`, `SearchField`, `NumberField`, `Choice`, `Check`, `SwitchRow` | `Input`, `NumberInput`, `Select`, `Checkbox`, `Switch` | forms; a switch's label is its own |
| `Segmented`, `Options` | `Tabs`, `ToggleGroup` | what a place shows; a value of a few |
| `Disclosure` | `Accordion` | a section that opens under its heading |
| `SidePanel` + `PanelHeader`, `Modal` + `Actions` | `Sheet`, `Dialog` | supplementary work; short decisions, their foot always Cancel then the commit, at the end |
| `Stat`, `Figure` | `Card` (`tile`), — | a figure in a row of tiles; a figure in a line without a card (with how it changed). No third figure |
| `Notice` | — | the one banner: a note, a caution or what went wrong, with what can be done at its end |
| `Await` | — | anything fetched, in its three states said truly: on its way, failed (why, Try again), there |
| `PageHeader`, `SideHeader`, `Back` | — | the head of a page, of a list in the sidebar, and the one way back |
| `Path` | `Text` | a path or key cut in its middle, heard whole |
| `TABLE`, `ROW`, `SimpleTable` | `DataTable` | every table's classes and row heights; rows of text |
| `Badge`, `Dot`, `Legend`, `KeyValue`, `Kbd`, `Empty`, `Loading` | — | display only |

Rules: a page passes no classes of its own to an `nsh/base` control; a class string a second page would repeat
belongs in the kit; `FOCUS` is on everything that takes the keyboard; `selected:bg-selection` is the one selection,
`active:bg-muted` the press. One formatter per kind of value (`format.ts`): `duration` (0.4 ms … 2 h 05 min, the unit chosen after rounding),
`percent` and `change`, `cost`, `clock`, `count`, `bytes`, `date`. `copy()` is the one way to copy, `useEndpoint()`
the one way to change endpoint. `scripts/lint-kit.py` fails CI on a card, a table's classes or a gutter drawn by hand,
on spacing off the scale, and on a control, a row height, a search field or a settings width drawn by hand.

Sizes (`CONTROL`, `ROW`, `FORM`):

- **Two control heights.** h-8 for a field, a choice, a button; h-7 for a small button, an icon button, a picker's
  trigger and a row of segments (each segment h-6 in its track). Nothing between.
- **One middle.** Controls in a row share their vertical middle (`items-center`); no control sets its own
  `self-start` — a column that wants it left says so where it places it.
- **Three row heights.** 30 for a table of rows to press, 26 for a short summary table or program output, 44 for a
  row of two lines.
- **Baseline.** Text in one row shares a baseline: one type size per row where it can, figures at the end of their
  cell in a `flex-1 items-end justify-center` box, an icon in a `shrink-0` box beside its text.
- **Two breakpoints** (`BREAK`): where an inspector sits beside what it details rather than over it, and where the
  Time pane's charts go two abreast. A pane measures itself (`onLayout`); nothing reads the focused window's size.
- **Nothing appears and goes.** What a place may show has its room from the start: a bucket's four tiles, the
  sessions' search field, a pane's tabs. A value arrives in its place; the layout does not change to make it.
- **One settings width.** Settings and Endpoints are `FORM` wide, so their edges line up across the tabs.
- **Tables fit their own width.** The operation's name takes the room the others leave; notes, then bars go as
  the table (not the window) narrows — from the end only: DataTable keeps a column's state by its place.

## One rendering per concept

| Concept | Rule |
|---|---|
| HTTP status | `StatusMark`: a dot and the code in mono; success quiet, anything else in its tone |
| Count | `count()`, tabular, never mono; a section's count is "Title · n" |
| Size | `bytes()`, tabular, at the end of its column |
| Duration | `offset()` (840 ms, 2.35 s); a column of them is titled "Took" |
| Time | clock time in lists (`14:02:11.204`), `date()` in headers, never raw ISO |
| Cost | `cost()` with its currency |
| Key or path | mono, cut in the middle, once |
| Command | mono, `$ ` before it, variables' values hidden |
| Opening | "Open …", a chevron on rows |
| Removing | "Delete", the trash icon, a confirmation |

## Surfaces

- **Base**: `bg-background`, regions separated by hairlines (`border-border`), no
  shadows. Cards (`bg-card`, `rounded-(--radius-card)`) group a few related values;
  no card inside a card.
- **Popups** (menus, selects, the endpoint switcher): `POPUP` — the popover surface,
  one border, one radius, one shadow, and glass on macOS.
- **Side panels** (Activity, a bucket's detail, the endpoint form, a narrow window's
  inspector): `SidePanel` in three widths (narrow 420, default 600, wide 760) with a
  `PanelHeader`; they start below the title bar and close with Escape, the scrim or
  Close.
- **Dialogs**: short decisions only (a typed confirmation, the replay target), over
  the scrim.

## Spacing and size

Tailwind's 4-point scale by relationship: 4 inside a control, 8 between related
controls, 12 inside a group, 16 between groups and as panel padding, 24 between
sections. Rows: 28 (Activity), 30 (operations, objects). Widths that are not on the
scale belong to a component (`SidePanel`, the palette, the inspector) and nowhere else.

## Actions

- **Primary** is the default commit of its area, the one Return runs: Run, Save, the
  typed confirmation, Start recording. Adding, opening, downloading and scanning again
  are outline or ghost.
- A command that opens a panel, a dialog or a window ends with `…` (Add endpoint…,
  Replay on…, Check compatibility…).
- Secondary commands live in a visible menu (`⋯`) or a context menu, and every one has
  another path (the palette, a shortcut, the inspector).
- Destructive actions are read-only by default and go through a dry run and a typed
  confirmation; the result is in the audit log (Activity › Changes).

## Charts

Every chart is Native Shell's `Plot` (`src/ui/plots.tsx`, `src/ui/charts.tsx`): drawn natively from the rows, the
pointer, tooltips and fading followed in Rust, never a hand-drawn canvas.

- **One question per chart**, said in words beside it and in its `accessibilityLabel`: what it shows and what stands
  out ("Most of the waiting: PutObject, 65%"). A terminal gets the sentence and a table.
- **Colors are roles.** Outcomes in their status tokens (`bar` for ok, `status-miss`, `status-client`,
  `status-server`); a quantity in `chart-1`, a second in `chart-4`; tails and percentiles in a quiet
  `muted-foreground` (dashed for a line), never an error's color; retries in `info`; before is
  `muted-foreground/30`, after its outcome (`status-ok` fewer, `status-server` more). What a selection leaves out
  turns `bar-track`, it does not disappear.
- **One frame** (`Chart`): a card with the question as its title, a sentence that answers it, how it is cut at the
  title's end, the plot, its legend. On the Time pane the frames make a board, two abreast where there is room,
  under one timeline whose chosen stretch every chart over time follows.
- **One geometry**: rows' names in one gutter (`GUTTER`, `AXIS_LABEL`), time ticks `TIME_TICK` (three), value ticks
  inside on a chip (`VALUE_TICK`) with their unit; one row pitch (28), one band or line under the pointer, one fade
  for what it is not on (0.4); latencies on a log scale drawn as logs, the ticks saying times.
- **Quiet frames**: hairline grids (`stroke-border`), no axis lines, small tabular labels in `text-caption`
  `muted-foreground`; bars' tips rounded, their bases square; labels at the bars' ends rather than a value axis.
- **The tooltip** is the popup surface (`Tip`): a title and one or two lines of figures, beside the pointer.
- **A press acts on what the pointer is over** (a slice narrows the operations; a dot or a bar opens its request);
  the plot sits in a pressable view, as the plot takes no presses itself.
- **Motion**: an appear of 420 ms ease-out per new data (`appearGeneration`), updates gliding in 260 ms; nothing
  moves with reduced motion.
- **Lanes**: every request on the first lane free at its start, a run of cells from start to end in its
  outcome's color; requests of a lane alternate two shades. What ran at once is stacked, what waited is in a row.
- **Fewer is the ok tone, more the error's**, in every chart that compares; a throttle is `warn`, a failure
  `status-server`, a retry `info`; the slow tail is `chart-tail`, never an error's color.
- **Flows and grids**: a Sankey shows few nodes (top operations and prefixes, the rest "other"), outcomes in their
  status colors, ribbons a gradient from source to target; a heat map ramps from `bar-track` to `chart-1` by the square root of its counts.
- **Annotations over legends**: a chart names its notable point in place (`PlotLabel`), a baseline is a dashed line.
- Times read from the session's start (`+230 ms`, `2.35 s`); costs to two significant digits under a cent.

## Motion

Short, and only to explain a change: overview cards rise in, operations slide into
the live list, sheets come from their edge. Nothing moves with reduced motion.

## Words

Sentence case, no final period on labels, `…` for commands that ask for more. Nouns
for places (Run, Sessions, Buckets, Activity), verbs for commands. Chinese is written,
not translated: `桶`, `会话`, `未完成的上传`. What sonde says about a session comes from
phrases (`src/phrases.ts`) in both languages.
