#!/usr/bin/env python3
"""What changed on screen: this run's screenshots against an earlier run's.

    scripts/shots-diff.py BEFORE_DIR AFTER_DIR OUT_DIR

For every screenshot in both, the share of pixels that changed; those past
0.5% get a before | after | difference image in OUT_DIR. Prints a Markdown
table (CI appends it to the job summary), new and missing screens included.
Exits 0: a change is for a person to look at, not a failure.
"""
import sys
from pathlib import Path

from PIL import Image, ImageChops

before, after, out = (Path(a) for a in sys.argv[1:4])
out.mkdir(parents=True, exist_ok=True)
a_names = {p.name for p in before.glob("*.png")}
b_names = {p.name for p in after.glob("*.png")}
rows = []
for name in sorted(a_names & b_names):
    a = Image.open(before / name).convert("RGB")
    b = Image.open(after / name).convert("RGB")
    if a.size != b.size:
        rows.append((100.0, name, f"size {a.size} → {b.size}"))
        continue
    diff = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 16 else 0)
    changed = sum(diff.histogram()[255:]) / (a.size[0] * a.size[1]) * 100
    if changed > 0.5:
        w, h = a.size
        sheet = Image.new("RGB", (w * 3, h), "white")
        sheet.paste(a, (0, 0))
        sheet.paste(b, (w, 0))
        sheet.paste(Image.composite(Image.new("RGB", a.size, (230, 30, 60)), b.point(lambda v: v // 3), diff), (w * 2, 0))
        sheet.save(out / name)
    rows.append((changed, name, ""))
print("### Screens against the last green run\n")
print(f"{len(a_names & b_names)} compared · {len(b_names - a_names)} new · {len(a_names - b_names)} gone\n")
print("| screen | changed | |\n|---|---:|---|")
for changed, name, note in sorted(rows, reverse=True):
    if changed > 0.5 or note:
        print(f"| {name} | {changed:.1f}% | {note} |")
for name in sorted(b_names - a_names):
    print(f"| {name} | new | |")
for name in sorted(a_names - b_names):
    print(f"| {name} | gone | |")
