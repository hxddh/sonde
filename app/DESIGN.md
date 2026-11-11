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
| Web (`sonde share`) | The app's session page, read-only: its header, its tabs, the pane — the shape strip and the findings over the operations |

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
| `ListRow`, `Row` | `Button`, `View` | the one row a press opens (hover, pressed, selected, focus; `lines={2}`, `divided` in a card, `trailing`), and the one row that is only read (a hairline, 12 by 8). `ROWS` for a tree's and the palette's items, which take classes |
| `Title`, `Heading`, `Strong` | `Text` | the two heading levels (`page` for an introduction's one title; `quiet` over a list), and words set heavier that are not a heading |
| `Field`, `SearchField`, `NumberField`, `Choice`, `Check`, `SwitchRow` | `Input`, `NumberInput`, `Select`, `Checkbox`, `Switch` | forms; a switch's label is its own |
| `Segmented`, `Options` | `Tabs`, `ToggleGroup` | what a place shows; a value of a few |
| `Disclosure` | `Accordion` | a section that opens under its heading |
| `SidePanel` + `PanelHeader`, `Modal` + `Actions` | `Sheet`, `Dialog` | supplementary work; short decisions, their foot always Cancel then the commit, at the end |
| `Stat`, `Figure` | `Card` (`tile`), — | a figure in a row of tiles; a figure in a line without a card (with how it changed, and with `preview` what it is made of). No third figure. `Tiles` lays tiles in rows of equal count, `Figures` a line of figures that never wraps |
| `IconTile`, `ICON` | — | the one tile an icon stands in; an icon's three sizes |
| `Notice` | — | the one banner: a note, a caution or what went wrong, with what can be done at its end |
| `Await` | — | anything fetched, in its three states said truly: on its way, failed (why, Try again), there |
| `PageHeader`, `SideHeader`, `Back` | — | the head of a page, of a list in the sidebar, and the one way back |
| `Path` | `Text` | a path or key cut in its middle, heard whole |
| `TABLE`, `ROW`, `SimpleTable` | `DataTable` | every table's classes and row heights; rows of text |
| `Badge`, `Dot`, `KeyValue`, `Kbd`, `Empty`, `Loading` | — | display only; `Empty` is the one way to say nothing is here (`compact` inside a chart, a list or a card) |

Rules: a page passes no classes of its own to an `nsh/base` control; a class string a second page would repeat
belongs in the kit; `FOCUS` is on everything that takes the keyboard; `selected:bg-selection` is the one selection,
`active:bg-muted` the press. One formatter per kind of value (`format.ts`): `duration` (0.4 ms … 2 h 05 min, the unit chosen after rounding),
`percent` and `change`, `cost`, `clock`, `count`, `bytes`, `date`, `factor`, `rate`. `copy()` is the one way to copy, `useEndpoint()`
the one way to change endpoint.

### One of each, and the lint that keeps it so

Each rule below is enforced by `scripts/lint-kit.py` (the name in brackets is what it prints), `scripts/lint-charts.py`
(which lint-kit runs) or `scripts/check-locales.py`; CI runs them on every push. A rule is broken only in
`src/ui/kit.tsx`, or where the lint's own list names the exception and why.

| One | Rule | Lint |
|---|---|---|
| Figures | Two renderings, `Stat` and `Figure`, both the kit's. No component named Figure or Stat elsewhere, no large tabular number set by hand | `[figure]` |
| Headings | `Title`, `Heading`, `Section` only: no `role="header"` and no `font-semibold`/`font-bold` in a page. Weight that is not a heading is `Strong`; where only classes can be given, `EMPHASIS` with an `emphasis:` comment saying why | `[heading]` |
| Rows | `ListRow` for every row a press opens, `Row` for every row only read; `ROW_STATE` stays in the kit, no hairline row by hand | `[row]` |
| Open and close | `Disclosure` (with `trailing` for what stands beside its title): the findings use it | `[heading]` (its title is the kit's) |
| Layout thresholds | The four `BREAK` names and no number compared with a width or a height anywhere else; a key nothing uses fails | `[threshold]` |
| Duration | `duration()` in `format.ts`, under that name only: no alias, no milliseconds written by hand, no other exported function that writes one | `[duration]` |
| Numbers | Every count through `count()`, every share through `percent()`/`pct()`, a ratio through `factor()`, a rate through `rate()`: no `String(` and no `.toFixed(` in `src/ui`, `src/screens` or `viewer` | `[number]` |
| Spacing | 4, 8, 12, 16, 24 (`gap-1 2 3 4 6`, and `p-`, `m-` alike), every value checked, not two; in the kit an off-scale inset carries an `off-scale:` comment | `[spacing]` |
| Control heights | Two, `h-8` and `h-7`; a button inside a field is `IconButton inField` (as high as the field) | `[control]`, `[control-height]` |
| Icons | Three sizes (`ICON.small` 12, default 16, `ICON.large` 20), one tile (`IconTile`) | `[icon-size]`, `[icon-tile]` |
| Nothing here | `Empty`, whole or `compact` | `[empty]` |
| Words | Every key in every language with the same placeholders, and every key used | `check-locales.py` |
| Charts | One frame, one left edge, one row pitch, one treemap height, one legend, one tooltip, one fade, one outline (see Charts) | `lint-charts.py` |
| Tables in the bucket's sheets | Their columns fit the sheet | `lint-layout.py` |

Sizes (`CONTROL`, `ROW`, `FORM`):

- **Two control heights.** h-8 for a field, a choice, a button; h-7 for a small button, an icon button, a picker's
  trigger and a row of segments (each segment h-6 in its track). Nothing between.
- **One middle.** Controls in a row share their vertical middle (`items-center`); no control sets its own
  `self-start` — a column that wants it left says so where it places it.
- **Three row heights.** 30 for a table of rows to press, 26 for a short summary table or program output, 44 for a
  row of two lines.
- **Baseline.** Text in one row shares a baseline: one type size per row where it can, figures at the end of their
  cell in a `flex-1 items-end justify-center` box, an icon in a `shrink-0` box beside its text.
- **Four thresholds** (`BREAK`), and no others: two widths — `toolbar` (1040), from which the window's toolbar shows
  the endpoint's name in full and the search field rather than its button, and `inspector` (900), from which a pane
  holds an inspector beside what it details (under it the inspector comes in from the edge); two heights of a
  session's pane — under `short` (600) the shape strip goes and the findings keep to a line or two, from `tall` (880)
  each finding has its lines in full. A pane measures itself (`onLayout`); nothing reads the focused window's size.
- **A pane that narrows reflows by rule**, never by clipping:
  - *figures first*: a line of figures (`Figures`) holds as many as fit, in rank order (a session's: requests,
    waiting, errors, took, cost, operations, not found); the last ranked go, none wraps;
  - *then columns by rank* (`fitColumns`): a table shows its always-columns, then the ranked ones while the column
    that takes the rest keeps its room; the first that does not fit goes with every one ranked after it. The
    operations: the outcome, the operation and Took stay; then requests, time, when, format, notes. The objects: name
    and size stay; then Modified, Class, Parts. Ranked columns stand last, in rank order, because DataTable keeps a
    column's state by its place;
  - a finding's words keep their width: what acting on it saves goes first, then the count's words;
  - a session's sentence wraps to two lines and is cut only at the second line's end;
  - tiles (`Tiles`) go in rows of equal count: four are four abreast or two and two, never three and one;
  - a chart's sentence wraps and is never cut; a row's name that does not fit its gutter is cut in its middle
    (`axisName`), a path at its start (`tail`).
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
| Duration | `duration()` (0.4 ms, 840 ms, 2.35 s, 3 min 05 s). One word per meaning: **Took** for how long an operation or a run lasted, **Waiting** for a program's time on the store, **Latency** for one request |
| Time | clock time in lists (`14:02:11.204`), `date()` in headers, never raw ISO |
| Cost | `cost()` with its currency |
| Key or path | mono, cut in the middle, once |
| Command | mono, `$ ` before it, variables' values hidden |
| Opening | "Open …", a chevron on rows |
| Removing | "Delete" for data (a session, a version, objects), "Remove" for configuration (an endpoint); the trash icon, a confirmation, so both end with `…` |

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

Tailwind's 4-point scale by relationship, and only these: 4 inside a control, 8 between related controls, 12 inside
a group, 16 between groups and as panel padding, 24 between sections and around a centered introduction or a
dialog's edge. **The page gutter is 16** (`p-4`) everywhere: a place, a sheet, a window's page, the shared page —
20 was not on the scale. Rows: 28 (a list's row, a chart's row), 30 (operations, objects), 44 (two lines). Widths
that are not on the scale belong to a component (`SidePanel`, the palette, the inspector, a chart's `GUTTER`) and
nowhere else.

## Actions

- **Primary** is the default commit of its area, the one Return runs: Run, Save, the
  typed confirmation, Start recording. Adding, opening, downloading and scanning again
  are outline or ghost.
- `…` ends a command exactly when it asks something more before it acts: a dialog, a sheet, a menu or a window
  that needs input (Add endpoint…, Replay on…, Compare…, Rename…, Delete…, Check compatibility…). A command that
  acts at once, or only shows something, has none (Open in a window, Share as a web page, Versions, Measure, Show
  code change). A state has its verb as its label ("Clear baseline", not "Baseline ✓ — clear").
- Secondary commands live in a visible menu (`⋯`) or a context menu, and every one has
  another path (the palette, a shortcut, the inspector).
- Destructive actions are read-only by default and go through a dry run and a typed
  confirmation; the result is in the audit log (Activity › Changes).

## Charts

Every chart is Native Shell's `Plot`, drawn natively from the rows, the pointer, tooltips and fading followed in
Rust, never a hand-drawn canvas. **One chart system** (`src/ui/plots.tsx`; Capacity's three charts in
`src/ui/charts.tsx` are drawn with its constants): every constant a chart is drawn with is exported from there and
defined nowhere else, and `scripts/lint-charts.py` counts them.

- **One question per chart**, said in words beside it and in its `accessibilityLabel`: what it shows and what stands
  out ("Most of the waiting: PutObject, 65%"). A terminal gets the sentence and a table.
- **One frame** (`Chart`), used by every chart — the Time pane's ten, Capacity's, Compare's, the trends, the
  waterfall, Run's live panel, the shared page: a card with the question as its title, a sentence that answers it
  (it wraps, never cut), how it is cut at the title's end (`action`), the plot, its `legend`, and `empty` said in
  the plot's room. The Time pane is a list of chart descriptors drawn by one loop. `CHARTS` names the chart
  components (a `Plot` anywhere else fails the lint, as does a chart used outside a frame); `FRAMED` those that
  draw their own frame (`RunTrend`); `MARKS` what is not a chart and stands in none: a session's `Sparkline` in its
  row and the shape strip under a session's header (`SessionShape strip`).
- **One left edge**: rows' names in one gutter (`GUTTER`, 176 pt, `AXIS_LABEL`), wide enough for
  "GetObject, not found" whole; a legend or a note under a chart of rows starts at the plot's edge (`gutter`). Flows
  have the same gutter either side.
- **One row pitch**: 28 (`PITCH`, `rowsHeight(n)`) for every chart of rows (time spent, chains, the waterfall, the
  heat map, the dumbbells, the kinds over time, Capacity's bars). Charts that are not rows have a height of their
  own, each named once: a map of areas (`TREEMAP`, one height for all three), requests against time or size
  (`PLOT`), lanes (as many as ran at once), flows (`FLOWS`), and the thin strips (pressure, the trends, the stack).
- **One legend** (`Legend`): a round swatch, one gap, in a row that wraps, with what else is to be said under the
  chart after it (`ChartNote`). **One tooltip** (`ChartTip`): the popup surface, a title and a line or two of
  figures, beside the pointer at one offset — every chart has one, Flows, the trends and the distribution
  comparison included. **One fade** (`INACTIVE`, 0.4) for what the pointer is not on, **one outline** (`HOVER`) for
  what it is on, one band or line under it (`CROSS_BAND`, `CROSS_LINE`).
- **Colors are roles.** Outcomes in their status tokens (`bar` for ok, `status-miss`, `status-client`,
  `status-server`); a quantity in `chart-1`, a second in `chart-4`; tails and percentiles in a quiet
  `muted-foreground` (dashed for a line), never an error's color; retries in `info`; before is
  `muted-foreground/30`, after its outcome (`status-ok` fewer, `status-server` more). What a selection leaves out
  turns `bar-track`, it does not disappear.
- **Ticks**: time ticks `TIME_TICK` (three, each once: `ticks3`), value ticks inside on a chip (`VALUE_TICK`) with
  their unit; latencies on a log scale drawn as logs, the ticks saying times.
- **Quiet frames**: hairline grids (`stroke-border`), no axis lines, small tabular labels in `text-caption`
  `muted-foreground`; bars' tips rounded, their bases square; labels at the bars' ends rather than a value axis.
- **A press acts on what the pointer is over** (a slice narrows the operations; a dot or a bar opens its request);
  the plot sits in a pressable view, as the plot takes no presses itself.
- **One moment across a pane's charts**: the moment the pointer rests on in one time chart is marked in the others
  of the same pane (`focusOf()`, the pane's own: two windows do not mark each other's).
- **Findings on the shape**: a number over a slice is the finding of that number, where it begins; the frame's
  legend says so, and the slice's tooltip names the findings that begin in it.
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

Sentence case, no final period on labels, `…` for commands that ask for more (and only those: see Actions).
A trial's scenario is said in words (`scenario.<name>`: "The store asks to slow down") with its id in mono beside
it, as `sonde run --trial` takes it. Every literal a reader sees goes through `t()`, in both languages; a key
nothing uses fails `check-locales.py`. Nouns
for places (Run, Sessions, Buckets, Activity), verbs for commands. Chinese is written,
not translated: `桶`, `会话`, `未完成的上传`. What sonde says about a session comes from
phrases (`src/phrases.ts`) in both languages.
