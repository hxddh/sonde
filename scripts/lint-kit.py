#!/usr/bin/env python3
"""Kit lint: the look lives in app/src/ui/kit.tsx, not in the pages.

A page that writes a card's classes by hand, a DataTable's classes, a fixed
chart gutter or a spacing off the 4/8/12/16/24 scale drifts from the rest
(11.0 found 32 cards in 7 paddings), as does a control, a row height or a
search field drawn by hand (12.0 found five heights and six search fields). Exit 1 when one does; kit.tsx itself
and the files that define the chart language are where those live.
"""
import re
import sys
from pathlib import Path

app = Path(__file__).resolve().parent.parent / "app" / "src"
RULES = [
    (r"rounded-\(--radius-card\) border border-border bg-card", "a card by hand: use Card or CARD"),
    (r"\bml-52\b|\bpl-52\b|\bml-32\b", "a fixed chart gutter: use GUTTER"),
    (r"(?<![\w.:-])(?:gap|px|py|p)-(?:2|3)\.5(?![\w.-])", "spacing off the scale (4/8/12/16/24)"),
    (r"(?<![\w.:-])gap-7(?![\w.-])", "spacing off the scale (4/8/12/16/24)"),
    (r'headerCell: "pl-2', "a table's classes by hand: use TABLE"),
    (r"reduceMotion\(\) \? \"\"", "motion checked by hand: use motion-safe:"),
    # 12.0: two control heights, three row heights, one search field
    (r"rowHeight\(\d", "a row height by hand: use ROW"),
    (r'<Icon name="search"', "a search field by hand: use SearchField"),
    (r"(?<![\w:-])h-(?:7|8)\b[^\"`]*\bborder-input\b", "a control by hand: use the kit's control or CONTROL"),
    (r"max-w-\[(?:720|860)px\]", "a settings page's width by hand: use FORM"),
]
OWN = {"ui/kit.tsx"}
# where a rule's own exception lives: the palette is the search field's popup
# and the title bar's stand-in for it opens that popup
EXEMPT = {("ui/palette.tsx", "a search field by hand: use SearchField"), ("ui/shell.tsx", "a search field by hand: use SearchField")}
failed = False
for path in sorted(app.rglob("*.tsx")):
    rel = path.relative_to(app).as_posix()
    if rel in OWN:
        continue
    for n, line in enumerate(path.read_text().splitlines(), 1):
        for pattern, why in RULES:
            if (rel, why) not in EXEMPT and re.search(pattern, line):
                print(f"app/src/{rel}:{n}: {why}")
                failed = True
sys.exit(1 if failed else 0)
