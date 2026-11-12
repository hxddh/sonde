# sonde's design standards

The app's own decisions on top of Native Shell's Design Guides
(`skills/nsh-design-guides` in the nsh checkout). Pages compose the components in
`src/ui/`; they do not restyle `nsh/base` controls or invent sizes of their own.

## The product

One task: see what a program does to S3. Three places — **Run** (start a program and
watch its requests become operations; once it ends, its top findings, what each saves and
the one figure for acting on them), **Sessions** (what a run did, what stands out,
compared and replayed), **Buckets** (the store's side, in one row of sections: objects, size,
incomplete uploads, versions, layouts, compatibility). The window has two columns
and no more: the **sidebar** (the places, and under them the current place's own list —
the commands run before, the sessions, the buckets) and the place; a detail opens
beside the place as an inspector, and nothing else moves when it does. Everything configured once
lives in **Settings** (a window); what sonde itself is doing lives in **Activity** (a
side panel). The core result is *what stands out in a session*: it gets the space, the
words and the next action; counts and chips support it, they do not replace it.

## One interaction grammar

**Select → inspect → act**, on every screen:

- **Select**: a press, or the arrows and Return, chooses one thing — a row, a finding, a stretch of time, a
  bucket, a section. It is marked once (`selected:bg-selection`), and what shows it follows: a finding narrows
  the operations, a stretch on the timeline is what every Time chart is reckoned over. Press it again to let go.
- **Inspect**: its detail opens in **the one inspector** beside what it details (an operation, a request, an
  object), from the edge where the pane is narrow. Never a second panel, never a dialog; Escape closes it.
- **Act**: what can be done with it stands at the end of its row or of the inspector — the primary, the rest
  behind `⋯` — and every act is also in the palette (⌘K, `:` in a terminal) or on a shortcut: nothing only a
  pointer reaches. A dialog is for a short decision only. A tool has one door: Buckets' sections are reached
  from their row (a tile or a finding whose fix is one of them leads to the same section, not to a window).

## Platforms

| Target | Form |
|---|---|
| macOS | Hidden title bar with the app's toolbar; the window's sidebar material behind the sidebar (`window.backdrop: "sidebar"`); glass on floating surfaces |
| Linux, Windows | The same layout with opaque fills; the menus behind the toolbar's menu button |
| Terminal (`sonde tui`) | A home list and a navigation stack; tables and sentences instead of charts; text labels for actions |
| Web (`sonde share`) | The app's session page, read-only: its header, its tabs, the pane — the shape strip and the findings over the operations |

Where a terminal only spaces or colours a view differently, the class says so (`gap-4 terminal:gap-2`, Native
Shell's `terminal:` variant); `terminal()` in code is for what a terminal draws differently (a sentence for a
chart, a base control left unstyled) and for a set of classes it takes none of.

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

**Two weights, and no third**: regular, and bold (700) for a heading, a figure and words
that must stand out (`EMPHASIS`). Not medium and not semibold: the CJK faces a system
falls back to have a regular and a bold and nothing between (Noto Sans CJK and WenQuanYi
on Linux, Microsoft YaHei on Windows; on macOS PingFang's heaviest, its semibold, stands
for bold), and a face with no bold is emboldened by the system only when bold itself is
asked for. At 600 a Chinese title was drawn regular beside the bold Latin in it.

## Color

Theme tokens only (`theme.json`, light and dark). Status has its own roles —
`status-ok`, `status-miss`, `status-client`, `status-server`, `status-refused`,
`status-injected` (a fault scenario's answer, never the store's), `warn`, `info` — and formats theirs (`fmt-*`); never a palette color in a page. The
scrim under modal layers is `bg-scrim`. Color never carries meaning alone: a dot sits
next to a status code, a warning has its icon and its words.

## The kit

Native Shell's base components have behavior and no look; the look is `src/ui/kit.tsx`'s, and only there. Each kit
piece wraps one nsh export with the theme's tokens:

| Piece | Over | Use |
|---|---|---|
| `Button`, `IconButton` | `Button` | primary (one per area), outline (default), ghost (dense rows, toolbars), destructive |
| `Card` | `View` / `Button` | `panel` (p-4, a group), `tile` (a figure in a row), `none` (a list drawing its rows); `onPress` makes the card the button; `onWidth` tells what it holds how wide it is |
| `ListRow`, `Row` | `Button`, `View` | the one row a press opens (hover, pressed, selected, focus; `lines={2}`, `divided` in a card, `trailing`), and the one row that is only read (a hairline, 12 by 8; `stack` for lines one under another, `bare` over what pads itself). `ROWS` for a tree's and the palette's items, which take classes |
| `ActionRow`, `More` | `Button`, the system's menu | commands in one row that never wraps: in rank order as many as the row has room for (each measures itself), the rest behind `⋯`; `More` alone is the `⋯` |
| `Pressable`, `Crumb`, `Scrubber`, `confirmAlert` | `Button`, `Link`, `Slider`, `AlertDialog` | a press area around what is not a label; a part of a path as a link; a value dragged along a range; the system's short question before something goes |
| `Title`, `Heading`, `Strong` | `Text` | the two heading levels (`page` for an introduction's one title; `quiet` over a list), and words set heavier that are not a heading |
| `Field`, `SearchField`, `NumberField`, `Choice`, `Check`, `SwitchRow` | `Input`, `NumberInput`, `Select`, `Checkbox`, `Switch` | forms; a switch's label is its own. A `Field` that is refused says so in one way: `error` (its edge in `fail`, the reason under it, and to a screen reader the field is `invalid` with the reason as its description); `invalid` alone where the reason stands elsewhere |
| `Segmented`, `Options` | `Tabs`, `RadioGroup` | what a place shows (a tab with nothing to show here is `disabled`, dimmed in its place: it does not go); a value of a few (a set of radios: one is checked) |
| `Disclosure` | `Accordion` | a section that opens under its heading |
| `SidePanel` + `PanelHeader`, `Modal` + `Actions` | `Sheet`, `Dialog` | supplementary work; short decisions, their foot always Cancel then the commit, at the end. A `Modal` that asks or warns gives its sentence as `describe`: a screen reader hears it after the dialog's name |
| `FocusRest` | `Input` | where the keyboard rests in a window on Linux when no field has it (FINDINGS 22): each window's first element, nothing to see or hear |
| `Stat`, `Figure` | `Card` (`tile`), — | a figure in a row of tiles; a figure in a line without a card (with how it changed, and with `preview` what it is made of). A `Stat` may carry a `mark` (a `Sparkline`: a signal's line over its session). No third figure. `Tiles` lays tiles in rows of equal count and tells each how much of itself to show (`TileLook`: `full`, `tight`, `line`) |
| `Icon`, `IconTile`, `ICON` | `Image` (the icon's SVG, tinted by a token) | an icon; the one tile an icon stands in; an icon's three sizes |
| `Notice` | — | the one banner: a note, a caution or what went wrong, with what can be done at its end |
| `Await` | — | anything fetched, in its three states said truly: on its way, failed (why, Try again), there |
| `PageHeader`, `SideHeader`, `Back` | — | the head of a page (without a title where the window's title bar says it), of a list in the sidebar, and the one way back |
| `Path` | `Text` | a path or key cut in its middle (the text's own `ellipsis-middle`), heard whole |
| `TABLE`, `ROW`, `SimpleTable` | `DataTable` | every table's classes and row heights; rows of text, every row there (it scrolls past `max`), sorted by a press on a title, figures at their cell's end (`end`), one column taking the room the others leave (`fill`), a cell drawn its own way (`cell`) |
| `Badge`, `Dot`, `KeyValue`, `Kbd`, `Empty`, `Loading` | — | display only; `Empty` is the one way to say nothing is here (`compact` inside a chart, a list or a card) |

Rules: a page passes no classes of its own to an `nsh/base` control; a class string a second page would repeat
belongs in the kit; `FOCUS` is on everything that takes the keyboard, and shows for focus the keyboard moved (`focus-visible:`), never
after a click (`FOCUS_WITHIN` around a control whose focus is a text input inside it); `selected:bg-selection` is the one selection,
`active:bg-muted` the press. One formatter per kind of value (`format.ts`): `duration` (0.4 ms … 2 h 05 min, the unit chosen after rounding),
`percent` and `change`, `cost`, `clock`, `count`, `bytes`, `transfer` (bytes each way: `↑ 78.1 KiB ↓ 75.1 KiB`), `date` (a day with its time, one writing: "Nov 7, 2026, 00:22", and to the millisecond "Nov 7, 2026, 00:22:25.074"), `factor`, `rate`. `duration` is the one writer of a length of time, in three forms: as measured, as an `age` (now, 5 min, 3 h, 86 d: whole, never "86.0 d") and as a `counter` (0:07). `copy()` is the one way to copy, `useEndpoint()`
the one way to change endpoint.

### One of each, and the lint that keeps it so

Each rule below is enforced by `scripts/lint-kit.py` (the name in brackets is what it prints), `scripts/lint-charts.py`
(which lint-kit runs) or `scripts/check-locales.py`; CI runs them on every push. A rule is broken only in
`src/ui/kit.tsx`, or where the lint's own list names the exception and why.

| One | Rule | Lint |
|---|---|---|
| Figures | Two renderings, `Stat` and `Figure`, both the kit's. No component named Figure or Stat elsewhere, no large tabular number set by hand | `[figure]` |
| Headings | `Title`, `Heading`, `Section` only: no `role="header"` and no `font-semibold`/`font-bold` in a page. Weight that is not a heading is `Strong`; where only classes can be given, `EMPHASIS` with an `emphasis:` comment saying why | `[heading]` |
| Weights | Two: regular and bold. `font-medium`, `font-semibold` and every other weight fail, the kit included | `[weight]` |
| Rows | `ListRow` for every row a press opens, `Row` for every row only read; `ROW_STATE` stays in the kit, no hairline row by hand | `[row]` |
| Base controls | A page takes from `nsh/base` only what has structure and no look (a table, a split, a tree, a terminal, an editor, a diff); a control comes from the kit. The shared pieces that draw one of their own are listed in the lint, each with why | `[base]` |
| Open and close | `Disclosure` (with `trailing` for what stands beside its title): the findings use it | `[heading]` (its title is the kit's) |
| Layout thresholds | The four `BREAK` names and no number compared with a width or a height anywhere else; a key nothing uses fails. A size a page needs (a tile's least width, a dialog's scrolling height, the findings' bound) is a `SIZES` name: none under a name of its own; a literal `maxHeight` fails, and a split's `minSize`/`maxSize` is a `PANE` name. `App.tsx` is read like every page | `[threshold]` |
| Sizes | No `w-N` and no arbitrary `[…]` size in a page: a field's or a column's width is a `WIDTH` name, a surface's a `SURFACE` name, a table column's a `COLUMN` name (no number in a column object); a name nothing uses fails | `[width]`, `[size]`, `[column]` |
| Presses | Nothing pressable by hand: no `role="button"` on a `Text` or a `View` (it has no focus ring); `Button`, `ListRow` or `Back` | `[press]` |
| Duration | `duration()` in `format.ts`, under that name only: no alias, no milliseconds written by hand, no other function that writes one (`elapsed`, `days` and `ago` are gone: an age and a counter are forms of `duration`), an age's words used nowhere else | `[duration]` |
| Numbers | Every count through `count()`, every share through `percent()`/`pct()`, a ratio through `factor()`, a rate through `rate()`: no `String(` and no `.toFixed(` in `src/ui`, `src/screens` or `viewer` | `[number]` |
| Spacing | 4, 8, 12, 16, 24 (`gap-1 2 3 4 6`, and `p-`, `m-` alike), every value checked, not two; in the kit an off-scale inset carries an `off-scale:` comment | `[spacing]` |
| Control heights | Two, `h-8` and `h-7`; a button inside a field is `IconButton inField` (as high as the field) | `[control]`, `[control-height]` |
| Icons | Three sizes (`ICON.small` 12, default 16, `ICON.large` 20), one tile (`IconTile`); an `<Icon` is written on one line, so the size rule reads it | `[icon-size]`, `[icon-tile]`, `[icon]` |
| Nothing here | `Empty`, whole or `compact` | `[empty]` |
| Words | Every key in every language with the same placeholders, and every key used: named whole, or a member of a family a template builds (`t(\`trial.${standing}\`)`), the family listed in the lint with its members, key by key — a prefix alone passes nothing | `check-locales.py` |
| Charts | One frame, one left edge, one row pitch, one legend, one tooltip, one fade, one outline, one time axis, one value scale; bar thickness, tips and heights each a named small set (see Charts) | `lint-charts.py` |
| Tables | A column's title fits its width, and so does the widest text a formatter can write into it (a day, a date, a size, an age), in English and in Chinese; Buckets' sections' tables fit the page at the least window; a column of keys, paths, targets or names is cut in its middle (`ellipsis-middle`) or at its start, never at its end | `lint-layout.py` |

Sizes (`CONTROL`, `ROW`, `FORM`):

- **Two control heights.** h-8 for a field, a choice, a button; h-7 for a small button, an icon button, a picker's
  trigger and a row of segments (each segment h-6 in its track). Nothing between.
- **One middle.** Controls in a row share their vertical middle (`items-center`); no control sets its own
  `self-start` (`Badge` and `Back` included) — a column that wants it left says so where it places it.
- **Three row heights.** 30 for a table of rows to press, 26 for a short summary table or program output, 44 for a
  row of two lines.
- **Baseline.** Text in one row shares a baseline: one type size per row where it can, figures at the end of their
  cell in a `flex-1 items-end justify-center` box, an icon in a `shrink-0` box beside its text.
- **Four thresholds** (`BREAK`), and no others: two widths — `toolbar` (1040), from which the window's toolbar shows
  the endpoint's name in full and the search field rather than its button, and `inspector` (900), from which a pane
  holds an inspector beside what it details (under it the inspector comes in from the edge); two heights of a
  session's pane — under `short` (600) the shape strip goes, the signal tiles keep to one line each (what they count
  and the figure) and the findings keep to a line or two, from `tall` (880)
  each finding has its lines in full. A pane measures itself (`onLayout`); nothing reads the focused window's size.
- **A pane that narrows reflows by rule**, never by clipping:
  - *figures first*: a row of ranked tiles (`Tiles ranked`) holds as many as fit, in rank order; the last ranked
    go, none wraps. A session's header is its **four signals — rate, errors, latency,
    bytes — and in flight**, a tile each (a figure, its line over the session, one line of detail; a press opens
    its chart on the Time pane): in flight goes first, then bytes. A figure a log without times cannot give says
    "Not timed", never 0. **A short run says totals, not rates**: under a second, or under twenty requests, the
    tiles read "9 requests", "2 of 9", "3.0 MiB", and draw no line over a time that is a moment; a percentile of
    fewer than twenty requests is marked wherever one is shown ("of only 9", `~` in a table). A tile's line is of
    its own quantity and drawn as that kind: counts as bars, latency as a line, in flight as steps, on a baseline;
  - *then columns by rank* (`fitColumns`): a table shows its always-columns, then the ranked ones while the column
    that takes the rest keeps its room; the first that does not fit goes with every one ranked after it. The
    operations: the outcome, the operation and Took stay; then requests, time, when, format, notes. The objects: name
    and size stay; then Modified, Class, Parts. Ranked columns stand last, in rank order, because DataTable keeps a
    column's state by its place;
  - a finding's words keep their width: what acting on it saves goes first, then the count's words;
  - a session's sentence wraps to two lines and is cut only at the second line's end;
  - tiles (`Tiles`) go in rows of equal count: four are four abreast or two and two, never three and one; where
    not even two fit as they read (a bucket's tiles beside an inspector) each takes a line of its own — what it
    counts at its start, the figure at its end — before any label or figure would be cut;
  - commands at a row's end (`ActionRow`) stay in one row: the primary, and in rank order as many of the others
    as fit, the rest behind `⋯`; they never wrap to a second line;
  - a finding's advice opens inside its own row, under its title, which comes to the top of the findings' room;
    in a short pane that room is bounded so the operations under it keep rows, ending on a whole finding, and
    says how many are in view;
  - free width goes to the column that would be cut (a target, a key, a name);
  - a chart's sentence wraps and is never cut; a row's name that does not fit its gutter is cut in its middle
    (`axisName`), a path at its start (`tail`); an operation's name in a table is cut in its middle by the text
    itself (`ellipsis-middle`), never at its end.
- **Nothing appears and goes.** What a place may show has its room from the start: a bucket's four tiles, the
  sessions' search field, a pane's four tabs (With output and Runs are dimmed for a session that has none, they
  do not go). A value arrives in its place; the layout does not change to make it.
- **One settings width.** Settings and Endpoints are `FORM` wide, so their edges line up across the tabs.
- **Tables fit their own width.** The operation's name takes the room the others leave; notes, then bars go as
  the table (not the window) narrows — from the end only: DataTable keeps a column's state by its place. A
  table whose columns are set by hand gives its free width to the column that would be cut (a request's
  target, an upload's key), and a column of dates is as wide as the widest date in either language.
- **A session of any size.** A session's view carries its totals, its signals, its findings and the first page
  of its operations; the table holds a window of rows over however many there are (`Operations`, `paged.ts`)
  and asks for the rest as it scrolls, sorts or is narrowed — sorted and narrowed where the operations are
  kept, never in the interface. The inspector reads one operation whole when it is chosen. A shared page holds
  its session whole and answers the same table from it (`operations.ts`).

## One rendering per concept

| Concept | Rule |
|---|---|
| HTTP status | `StatusMark`: a dot and the code in mono; success quiet, anything else in its tone; a request a fault scenario answered in the injected tone, and said so in words beside it |
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
- **Side panels** (Activity, a narrow window's inspector): `SidePanel`
  in three widths (narrow 420, default 600, wide 760) with a `PanelHeader`; they start
  below the title bar and close with Escape, the scrim or Close. The endpoint form is a
  card in place over the list, which stays under it (a sheet in the Settings window
  would cover its tabs); while it is open its Save is the page's one primary.
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
Rust, never a hand-drawn canvas. **One chart system** (`src/ui/plots/`: its constants and the one frame, legend,
tooltip and axes in `system.tsx`, and a file per family of charts — `traffic`, `latency`, `errors`, `where`,
`runs`, `operation` — behind one index; Capacity's three charts in `src/ui/charts.tsx` are drawn with its
constants): every constant a chart is drawn with is exported from `system.tsx` and defined nowhere else, and
`scripts/lint-charts.py` counts them.

- **One question per chart**: its title is the question it answers, in both languages ("Where did the time
  go?", "时间花在了哪里？"); its sentence and `accessibilityLabel` the answer ("Most of the waiting: PutObject,
  65%"). A terminal gets the sentence and a table.
- **One reading order** on the Time pane: four titled groups, **Traffic, Latency, Errors, Where**, every chart
  shown (none behind "more"); Latency opens with the table by operation. Two abreast where there is room, a last
  one alone takes the row.
- **One selection**: the stretch picked on the timeline. Charts over time zoom to it; every other chart is
  reckoned again from the requests that started in it (`sessions.stretch`) and its sentence says which
  ("in +200 ms – +400 ms"); a shared page, which cannot, says "of the whole session".
- **One frame** (`Chart`), used by every chart — the Time pane's, Capacity's, Compare's, the trends, the
  waterfall, Run's live panel, the shared page: a card with the question as its title, a sentence that answers it
  (it wraps, never cut), how it is cut at the title's end (`action`), the plot, its `legend`, and `empty` said in
  the plot's room. A card is as high as its content: a chart of two bars is not stretched to its neighbour's
  height. The Time pane is a list of chart descriptors drawn by one loop. `CHARTS` names the chart
  components (a `Plot` anywhere else fails the lint, as does a chart used outside a frame); `FRAMED` those that
  draw their own frame (`RunTrend`); `MARKS` what is not a chart and stands in none: a session's `Sparkline` in its
  row and the shape strip under a session's header (`SessionShape strip`).
- **One left edge**: rows' names in one gutter (`GUTTER`, 176 pt, `AXIS_LABEL`), wide enough for
  "GetObject, not found" whole; a legend or a note under a chart of rows starts at the plot's edge (`gutter`). Flows
  have the same gutter either side.
- **One row pitch**: 28 (`PITCH`, `rowsHeight(n)`) for every chart of rows (time spent, chains, the waterfall, the
  heat map, the dumbbells, the kinds over time, Capacity's bars). Charts that are not rows have a height of their
  own, each named once: a map of areas (`TREEMAP`, one height for all three), requests against time or size
  (`PLOT`), the timeline (`SHAPE`) and the strip under a session's header (`SHAPE_STRIP`), lanes (as many as ran
  at once), flows (`FLOWS`), the thin strips (`STRIP`: the trends, Run's live panel, a trial over its clean run),
  the stack (`STACK`) and a mark (`MARK`). **A bar's thickness** is one of `BARS` (rows, pairs, slices, buckets)
  or `CELLS`, **its tip** one of `TIP` (across, up, down, a span, a cell, an area, a mark): no literal padding,
  radius or height in a chart, and none on a chart in a page.
- **One legend** (`Legend`): a round swatch, one gap, in a row that wraps, with what else is to be said under the
  chart after it (`ChartNote`). **One tooltip** (`ChartTip`): the popup surface, a title and a line or two of
  figures, beside the pointer at one offset — every chart has one, Flows, the trends and the distribution
  comparison included. **One fade** (`INACTIVE`, 0.4) for what the pointer is not on, **one outline** (`HOVER`) for
  what it is on, one band or line under it (`CROSS_BAND`, `CROSS_LINE`). A mark a few points high (`Sparkline`)
  stands on a baseline and is drawn at full strength, so it reads in dark.
- **Colors are roles.** Outcomes in their status tokens (`bar` for ok, `status-miss`, `status-client`,
  `status-server`; `status-injected` for what a fault scenario answered, whatever it looked like); a quantity in
  `chart-1`, a second in `chart-4`; tails and percentiles in a quiet
  `muted-foreground` (dashed for a line), never an error's color; retries in `info`; before is
  `muted-foreground/30`, after its outcome (`status-ok` fewer, `status-server` more). What a selection leaves out
  turns `bar-track`, it does not disappear.
- **One time axis** (`TimeAxis`), under every chart over time — the timeline and the strip on Operations, lanes,
  bytes, latency's bands and its heat map, the scatter, the kinds, errors and their rate, chains, the prefix heat
  map, a trial over its clean run (`OVER_TIME` names them): the same round moments (`timeTicks`: 1, 2 or 5 of a
  power of ten apart, three or four of them) over the same stretch (`sessionSpan`, or the zoom), each said from
  the session's start (`+200 ms`, `sinceTick`). A chart of slices puts a tick on the slice its moment falls in.
  A length of time on an axis (the waterfall, from its operation's first request) has the same ticks, said as
  lengths. lint-charts fails a `PlotAxis` that writes a moment anywhere else.
- **One value scale** (`ValueAxis`) on every chart of counts (`COUNTS`: the timeline, the bands, the histogram,
  bytes, errors and their rate, sizes, a trial over its clean run): zero and the peak with its unit
  ("24 requests", "538 KiB/s"). The peak is on a chip inside the plot over its own line, in the headroom every such
  chart leaves above its peak (a fifth of it), so it stands on no mark; zero is outside the plot beside the
  baseline, in the frame's padding. Where every bar says its own count over itself (the histogram), the scale
  writes zero alone. A second quantity has its tick at the far edge (in
  flight on the timeline, the share done on the histogram). Latencies on a log scale are drawn as logs, the
  ticks saying times.
- **A budget on its chart**: `sonde.toml`'s `max-p95`, `max-errors` (as a share of the run) and `max-waiting`
  (over the runs) are dashed lines with a chip ("budget 50 ms"); the sentence says how far the run is.
- **Out and kept**: a Time chart's rows go out as CSV (its title's button, to the exports folder); the stretch
  chosen goes into a `sonde://` link and a shared page's `#range=` (D-123: still two presses).
- **A whole legend**: every colour and every line a chart draws is named under it — the timeline's stack, its
  line of requests in flight, the findings' numbers and a trial's faults; the scatter's outcomes and its p95.
- **The histogram**: each bar says how many requests it holds; the share done by each time is a dashed line on
  a scale of its own, never a bar's label; its tail is named by the share it really holds. A bar of none beside
  one with a count says "0" on a sliver (`ZERO_SLIVER`): a bar of no value is not drawn, nor its label.
- **Latency's bands**: the median a line, the body (median to p95) and the tail (p95 to p99) two tones of their
  own (`latency-body`, `latency-tail`) that pass 3:1 against each other and against the card, in light and in
  dark (lint-charts computes it). Beside the bands, the same latencies as a grid (slices by buckets of
  latency, a cell's tone how many): what a band averages away.
- **Quiet frames**: hairline grids (`stroke-border`), no axis lines, small tabular labels in `text-caption`
  `muted-foreground`; bars' tips rounded, their bases square; labels at the bars' ends rather than a value axis.
- **A press acts on what the pointer is over** (a slice narrows the operations; a dot or a bar opens its request);
  the plot sits in a pressable view, as the plot takes no presses itself.
- **One moment across a pane's charts**: the moment the pointer rests on in one time chart is marked in the others
  of the same pane (`focusOf()`, the pane's own: two windows do not mark each other's).
- **Findings on the shape**: a number over a slice is the finding of that number, where it begins (several that
  begin together are said in order, "1, 2, 3"); the frame's legend says so, and the slice's tooltip names the
  findings that begin in it.
- **A trial against its clean run**: on a trial's session the Time pane opens its Traffic with the trial drawn
  over the clean run on one axis (requests a second, then the median latency), and a rule in the injected tone
  at every moment sonde answered in the store's place — on that chart, on the timeline and on the strip.
- **By operation**: Latency opens with a table — count, error share, median, p95, p99 — sortable,
  a percentile of fewer than twenty requests marked `~`. It stands on the Time pane, not over the operations:
  the Operations pane lists what the program did in order, and this is a reading of the same requests as
  figures, beside the charts it summarises.
- **Errors**: the counts by class, and beside them the rate (the share of each slice's requests that failed);
  every answer in a table with headings, none left out (it scrolls), a fault scenario's in the injected tone.
- **Two runs**: five tiles say each fact once (requests, errors, latency, waiting, cost), each with both runs'
  lines; equal figures are said once with no change beside them; between two different programs nothing is
  better or worse, and nothing is green or red. A dumbbell's figures stand on its own line, over an axis.
- **Motion**: an appear of 420 ms ease-out per new data (`appearGeneration`), updates gliding in 260 ms; nothing
  moves with reduced motion.
- **Lanes**: every request on the first lane free at its start, a run of cells from start to end in its
  outcome's color; requests of a lane alternate two shades. What ran at once is stacked, what waited is in a row.
- **Fewer is the ok tone, more the error's**, in every chart that compares; a throttle is `warn`, a failure
  `status-server`, a retry `info`; the slow tail is `chart-tail`, never an error's color.
- **Flows and grids**: a Sankey shows few nodes (top operations and prefixes, the rest "other"), outcomes in their
  status colors, ribbons a gradient from source to target; a heat map ramps from `heat-low` to `heat-high`
  (`HEAT_RAMP`, tones of its own that lint-charts holds apart from the card, so one request reads in dark too) by
  the square root of its counts.
- **Annotations over legends**: a chart names its notable point in place (`PlotLabel`), a baseline is a dashed line.
- Times read from the session's start (`+230 ms`, `2.35 s`); costs to two significant digits under a cent.

## Pictures

The screenshot tour (`scripts/shots.sh`: wide, least, dark, Chinese) gates a merge: CI compares it with the last
green tour, and a screen that changed or is new must match its approved picture in `app/shots.approved.json`.
Approving is looking at the difference images and committing that file (`scripts/shots-diff.py approve`).

## Motion

Short, and only to explain a change: overview cards rise in, operations slide into
the live list, sheets come from their edge. Nothing moves with reduced motion.

## Words

Sentence case, no final period on labels, `…` for commands that ask for more (and only those: see Actions).
A trial's scenario is said in words (`scenario.<name>`: "The store asks to slow down") with its id in mono beside
it, as `sonde run --trial` takes it, and what it does and how hard it presses in a line under them
(`scenario.about.<name>`; a project's own scenario keeps the line its `sonde.toml` gives). The trial's dialog
says the limit the program runs under and lets it be changed; **Stop** ends a running trial and is its own
word (nothing dismisses the dialog while it runs), the time it has taken counts beside its progress, and a
finished scenario opens its own session. A verdict that is not "survived" says where it turned (the request,
the program's line) and what to change, from Rust's phrase. Every literal a reader sees goes through `t()`, in both languages; a key
nothing uses fails `check-locales.py`. Nouns
for places (Run, Sessions, Buckets, Activity), verbs for commands. Chinese is written,
not translated: `桶`, `会话`, `未完成的上传`; a place, a tab, a button and a kind of thing each have a word of
their own (运行 the place, 启动 its button, 各次运行 and 历次运行 the runs in the list and on the tab; 操作 what
a program did, 动作 the palette's commands; 测试用桶 where probes write, 临时桶 what they may create; 未通过 a
broke trial, 失败 a failed request, job or budget). What sonde says about a session comes from
phrases (`src/phrases.ts`) in both languages: English from Rust's own templates, which `check-locales.py`
holds the app's to, letter for letter.
