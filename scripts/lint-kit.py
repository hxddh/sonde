#!/usr/bin/env python3
"""Kit lint: the look lives in app/src/ui/kit.tsx, not in the pages.

One of each, and a rule here so the count stays (app/DESIGN.md names each
rule by the name printed with it). A page that writes a card's classes by
hand, a heading, a row's states, a figure, a spacing off the 4/8/12/16/24
scale, a third control height, a fourth icon size, a layout threshold of its
own, a second way to write a duration or a number drifts from the rest
(14.0 counted five figures, thirteen hand headings, six pressable rows, a
hundred off-scale spacings). Exit 1 when one does. kit.tsx itself and the
files that define the chart language are where those live; the charts have a
lint of their own (lint-charts.py), run from here so CI's one command covers it.
"""
import re
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
app = root / "app" / "src"
viewer = root / "viewer"

SPACE = r"(?<![\w./\[-])-?(?:gap|gap-x|gap-y|space-x|space-y|p|px|py|pt|pb|pl|pr|ps|pe|m|mx|my|mt|mb|ml|mr|ms|me)-(?!(?:0|1|2|3|4|6|px|auto)(?![\w.]))[\w.]+(?![\w-])"

# (name, pattern, what to do instead): everywhere but the kit
RULES = [
    ("card", r"rounded-\(--radius-card\) border border-border bg-card", "a card by hand: use Card or CARD"),
    ("gutter", r"\bml-52\b|\bpl-52\b|\bml-32\b|\bml-36\b|\bpl-36\b|\bmx-28\b", "a fixed chart gutter: use GUTTER"),
    ("spacing", SPACE, "spacing off the scale (4/8/12/16/24: gap-1 2 3 4 6)"),
    ("table", r'headerCell: "pl-2', "a table's classes by hand: use TABLE"),
    ("motion", r"reduceMotion\(\) \? \"\"", "motion checked by hand: use motion-safe:"),
    ("row-height", r"rowHeight\(\d", "a row height by hand: use ROW"),
    ("search", r'<Icon name="search"', "a search field by hand: use SearchField"),
    ("control", r"(?<![\w:-])h-(?:7|8)\b[^\"`]*\bborder-input\b", "a control by hand: use the kit's control or CONTROL"),
    ("form-width", r"max-w-\[(?:720|860|560)px\]", "a page's width by hand: use FORM or READING"),
    # 15.0: one of each
    ("figure", r"\bfunction (?:Figure|Stat)\b", "a figure of its own: the kit has the two (Stat, Figure)"),
    ("figure", r"(?:text-(?:title|heading|display)\b[^\"`]*\btabular-nums\b|\btabular-nums\b[^\"`]*\btext-(?:title|heading|display)\b)", "a large number drawn by hand: use Figure or Stat"),
    ("heading", r'role="header"', "a heading by hand: use Title, Heading or Section"),
    ("heading", r"\bfont-(?:semibold|bold)\b", "heavy text by hand: a heading is Title or Heading; other words are Strong (or EMPHASIS with an `emphasis:` comment)"),
    ("row", r"\bROW_STATE\b", "a row's states by hand: use ListRow (or ROWS for a control that takes classes)"),
    ("row", r"i\(\) > 0 \? \"border-t border-border", "a hairline row by hand: use Row or ListRow divided"),
    ("threshold", r"(?:\.[wh]\b|[wW]idth\(\)|[hH]eight\(\))\s*[<>]=?\s*\d{3,4}\b", "a layout threshold of its own: use a BREAK key"),
    ("duration", r"(?:export )?const \w+ = duration;|duration as \w+|\bms as \w+", "another name for the one duration: call duration()"),
    ("duration", r"toFixed\(\d?\)\}? ?(?:ms|s)\b|\$\{[^}]*\} ms`", "milliseconds written by hand: use duration()"),
    ("control-height", r"<(?:BaseButton|Input|Button|IconButton|Field|SearchField|NumberField|Choice)\b[^>]*\b(?:h|size)-(?!7\b|8\b|full\b|px\b)\d[\d.]*\b", "a third control height: h-8 and h-7 only (in a field: IconButton inField)"),
    ("icon-size", r"<Icon\b[^>]*\bsize=\{(?!12\}|16\}|20\}|ICON\.)", "an icon size of its own: ICON.small 12, default 16, large 20"),
    ("icon-tile", r"size-(?:8|9|10|11|12) (?:shrink-0 )?items-center justify-center rounded-\S+ bg-muted", "an icon tile by hand: use IconTile"),
    ("empty", r"text-(?:small|body) text-muted-foreground\">\{t\(\"[\w.]*(?:\.none|\.empty|noMatch|\.nothing|None|Empty)\"\)\}</Text>", "nothing-here by hand: use Empty (compact inside a list or a chart)"),
]
# in the pages and the shared pieces only: what the reader sees of a number
UI_RULES = [
    ("number", r"(?<![\w.])String\(", "a number as text by hand: count(), percent(), duration() (an id: a template)"),
    ("number", r"\.toFixed\(", "a number rounded by hand: count(), percent(), factor(), rate()"),
]
OWN = {"ui/kit.tsx"}
# where a rule's own exception lives, each for the reason beside it
EXEMPT = {
    # the palette is the search field's popup, and the title bar's stand-in for it opens that popup
    ("ui/palette.tsx", "search"), ("ui/shell.tsx", "search"),
    # the chart system's own constants: the one gutter is defined there
    ("ui/plots.tsx", "gutter"),
    # the format's own file is where milliseconds and decimals are written
    ("format.ts", "duration"), ("format.ts", "number"),
    # sentences of the engine, in Chinese: their `{ms} ms` are the engine's templates
    ("phrases.ts", "duration"), ("corephrases.ts", "duration"),
    # the lanes' cells stand a slice wide, not on the spacing scale's 4-point grid
}
# spacing the chart system names once (its gutter), on the lines that define it
GUTTER_LINE = re.compile(r"^(?:export )?const GUTTER\w* = ")
failed = False


def say(where, n, name, why):
    global failed
    print(f"{where}:{n}: [{name}] {why}")
    failed = True


files = [(p, f"app/src/{p.relative_to(app).as_posix()}", p.relative_to(app).as_posix()) for p in sorted(app.rglob("*.ts*"))]
files += [(p, f"viewer/{p.name}", f"viewer/{p.name}") for p in sorted(viewer.glob("*.tsx"))]
for path, where, rel in files:
    lines = path.read_text().splitlines()
    ui = rel.startswith(("ui/", "screens/", "viewer/"))
    for n, line in enumerate(lines, 1):
        near = " ".join(lines[max(0, n - 3) : n])
        if rel in OWN:
            # the kit's own controls may stand off the scale where a comment says why
            if re.search(SPACE, line) and "off-scale:" not in near:
                say(where, n, "spacing", "spacing off the scale in the kit without an `off-scale:` comment saying why")
            continue
        if rel == "ui/plots.tsx" and GUTTER_LINE.match(line):
            continue
        for name, pattern, why in RULES + (UI_RULES if ui else []):
            if (rel, name) not in EXEMPT and re.search(pattern, line):
                say(where, n, name, why)
        # weight outside a heading: allowed through the kit's constant, with its reason
        if "EMPHASIS" in line and not line.lstrip().startswith("import") and "emphasis:" not in near:
            say(where, n, "heading", "EMPHASIS without an `emphasis:` comment saying why it is not a heading")

# every threshold BREAK names is used, and by its name
kit = (app / "ui" / "kit.tsx").read_text()
m = re.search(r"export const BREAK = \{([^}]*)\}", kit)
everything = "\n".join(p.read_text() for p, _, rel in files if rel not in OWN)
for key in re.findall(r"(\w+):", m.group(1) if m else ""):
    if f"BREAK.{key}" not in everything:
        say("app/src/ui/kit.tsx", kit[: m.start()].count("\n") + 1, "threshold", f"BREAK.{key} is used nowhere: delete it")

# one function writes milliseconds: nothing else exported from format.ts does
fmt = (app / "format.ts").read_text()
for name, body in re.findall(r"export function (\w+)\([^)]*\): string \{(.*?)\n\}", fmt, re.S):
    if name != "duration" and re.search(r"\}\s?(?:ms|s|min|h)\b[ `]", body):
        say("app/src/format.ts", fmt[: fmt.index(f"export function {name}(")].count("\n") + 1, "duration", f"{name}() writes a duration: only duration() does")

charts = subprocess.run([sys.executable, str(Path(__file__).with_name("lint-charts.py"))])
sys.exit(1 if failed or charts.returncode else 0)
