# sonde's design standards

The app's own decisions on top of Native Shell's Design Guides
(`skills/nsh-design-guides` in the nsh checkout). Pages compose the components in
`src/ui/`; they do not restyle `nsh/base` controls or invent sizes of their own.

## The product

One task: see what a program does to S3. Three places — **Run** (start a program and
watch its requests become operations), **Sessions** (what a run did, what stands out,
compared and replayed), **Buckets** (what is in a store). Everything configured once
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

## Motion

Short, and only to explain a change: overview cards rise in, operations slide into
the live list, sheets come from their edge. Nothing moves with reduced motion.

## Words

Sentence case, no final period on labels, `…` for commands that ask for more. Nouns
for places (Run, Sessions, Buckets, Activity), verbs for commands. Chinese is written,
not translated: `桶`, `会话`, `未完成的上传`. What sonde says about a session comes from
phrases (`src/phrases.ts`) in both languages.
