#!/usr/bin/env python3
"""Layout lint: a table must fit where it is shown.

The bucket's details (Incomplete uploads, Versions, Capacity, Layouts,
Compatibility) open in the 760pt side panel; a DataTable there whose columns
add up past what the panel holds is cut off with no sign of it (8.0's
incomplete uploads lost Bytes and Cost). Exit 1 when one does.
"""
import re
import sys
from pathlib import Path

PANEL = 760
# the panel's padding and the table's border
FITS = PANEL - 2 * 24 - 4

app = Path(__file__).resolve().parent.parent / "app" / "src"
store = (app / "screens" / "StoreArea.tsx").read_text()
detail = set(re.findall(r'return <(\w+) />', store)) - {"Browse"}

failed = False
for name in sorted(detail):
    src = (app / "screens" / f"{name}.tsx").read_text()
    for m in re.finditer(r"<DataTable\b.*?columns=\{\[(.*?)\]\}", src, re.S):
        widths = [int(w) for w in re.findall(r"\bwidth:\s*(\d+)", m.group(1))]
        total = sum(widths)
        line = src[: m.start()].count("\n") + 1
        status = "ok" if total <= FITS else "too wide"
        print(f"{name}.tsx:{line}: columns {total}pt of {FITS}pt: {status}")
        failed |= total > FITS
sys.exit(1 if failed else 0)
