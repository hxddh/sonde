#!/usr/bin/env python3
"""Kit lint: the look lives in app/src/ui/kit.tsx, not in the pages.

A page that writes a card's classes by hand, a DataTable's classes, a fixed
chart gutter or a spacing off the 4/8/12/16/24 scale drifts from the rest
(11.0 found 32 cards in 7 paddings). Exit 1 when one does; kit.tsx itself
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
]
OWN = {"ui/kit.tsx"}
failed = False
for path in sorted(app.rglob("*.tsx")):
    rel = path.relative_to(app).as_posix()
    if rel in OWN:
        continue
    for n, line in enumerate(path.read_text().splitlines(), 1):
        for pattern, why in RULES:
            if re.search(pattern, line):
                print(f"app/src/{rel}:{n}: {why}")
                failed = True
sys.exit(1 if failed else 0)
