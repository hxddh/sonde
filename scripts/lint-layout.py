#!/usr/bin/env python3
"""Layout lint: a table shows its titles and its cells whole.

Four checks, for every DataTable whose columns have literal widths or the
kit's named ones (`COLUMN.x`; app/src, App.tsx and viewer), in English and
in Chinese:

- a column's title fits its width (with the cell's padding, and the sort
  mark where the column sorts): 15.0 shipped "Bytes h…";
- the widest text a column can hold fits it, where what it holds is written
  by one of the formatters (a day, a date, a clock time, a size, a count, an
  age, a cost): 15.0 shipped "2026年8月1…" in a column 104 pt wide, and the
  old lint, which only added widths up, said ok;
- the bucket's details (Incomplete uploads, Versions, Capacity, Layouts)
  open in the 760 pt side panel: their tables' columns add up to no more
  than the panel holds (8.0's incomplete uploads lost Bytes and Cost);
- a column of keys, paths, targets or names is cut in its middle or at its
  start, never at its end (16.0 shipped "backups/db-2026-08-12.t…"): where
  the table draws such a cell itself, it says `ellipsis-middle` or
  `ellipsis-start`, or cuts it with `middle()` / `Path`.

Widths are estimated from the text (a Latin letter, a digit, a CJK
character, at the table's type size): a little wide, so what passes fits.
Exit 1 when a title or a cell would be cut, or a table is too wide.
"""
import json
import re
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
app = root / "app" / "src"
PANEL = 760
# the panel's padding and the table's border
FITS = PANEL - 2 * 24 - 4
# a cell's inset either side (TABLE.cell: px-2), and the sort mark beside a title that sorts
PAD = 16
SORT = 22
TYPE = {"caption": 11, "small": 12, "body": 13}
locales = {name: json.loads((root / "app" / "locales" / f"{name}.json").read_text()) for name in ("en", "zh")}


def text_width(text, points, mono=False):
    """About how wide `text` is drawn, in points."""
    if mono:
        return sum(1.0 if ord(ch) > 0x2E7F else 0.61 for ch in text) * points
    w = 0.0
    for ch in text:
        if ord(ch) > 0x2E7F:
            w += 1.0
        elif ch.isdigit():
            w += 0.62
        elif ch in " .,:;'|!ijl":
            w += 0.29
        elif ch in "mwMW%":
            w += 0.88
        elif ch.isupper():
            w += 0.68
        else:
            w += 0.55
    return w * points


# the widest a formatter writes, by language: what a column of them must hold
WIDEST = {
    "day": {"en": "Sep 30, 2026", "zh": "2026年12月30日"},
    "date": {"en": "Sep 30, 2026, 12:59 PM", "zh": "2026年12月30日 23:59"},
    "clock": {"en": "23:59:59.999", "zh": "23:59:59.999"},
    "bytes": {"en": "1023.9 MiB", "zh": "1023.9 MiB"},
    "count": {"en": "99,999", "zh": "99,999"},
    "age": {"en": "9,999 d", "zh": "9,999 天"},
    "duration": {"en": "59 min 59 s", "zh": "59 min 59 s"},
    "money": {"en": "99999.99 USD", "zh": "99999.99 USD"},
}


def kind_of(expr):
    """Which formatter writes a cell, from the expression that fills it."""
    if re.search(r'duration\([^)]*"age"\)', expr):
        return "age"
    for name in ("day", "date", "clock", "bytes", "count", "duration", "money"):
        if re.search(rf"\b{name}\(", expr):
            return name
    return None


# the kit's named column widths: `COLUMN.status` reads as 52
kit_src = (app / "ui" / "kit.tsx").read_text()
KIT = {}
m = re.search(r"export const COLUMN = \{(.*?)\} as const;", kit_src, re.S)
for k, v in re.findall(r"^\s*(\w+): (\d+),", m.group(1) if m else "", re.M):
    KIT[f"COLUMN.{k}"] = int(v)
# what a column of paths is keyed by
PATHS = {"key", "target", "name", "path", "prefix", "url"}
COLUMN = re.compile(r'\{\s*key(?:: "(?P<key>[\w-]+)")?, title: (?P<title>t\("[\w.]+"\)|"[^"]*"), width: (?P<width>[^,}]+)(?P<rest>[^}]*)\}')
failed = False
checked = {"titles": 0, "cells": 0, "tables": 0, "paths": 0}


def say(where, why):
    global failed
    print(f"{where}: {why}")
    failed = True


files = sorted(app.rglob("*.tsx")) + [root / "app" / "App.tsx"] + sorted((root / "viewer").glob("*.tsx"))
for path in files:
    src = path.read_text()
    if "DataTable" not in src:
        continue
    rel = path.relative_to(root).as_posix()
    lines = src.splitlines()
    consts = {m.group(1): int(m.group(2)) for m in re.finditer(r"^(?:export )?const (\w+) = (\d+);", src, re.M)}
    consts.update(KIT)
    # a record of widths by column (`const OP_WIDTH = { status: 26, … }`), read as `x.widths.status`
    for m in re.finditer(r"const \w*WIDTH\w*(?:: [^=]+)? = \{([^}]*)\}", src):
        for k, v in re.findall(r"(\w+): (\d+)", m.group(1)):
            consts[f"widths.{k}"] = int(v)
    for n, line in enumerate(lines, 1):
        for m in COLUMN.finditer(line):
            width = m.group("width").strip()
            if width.isdigit():
                w = int(width)
            elif width in consts:
                w = consts[width]
            elif re.search(r"\.widths\.(\w+)$", width) and f"widths.{width.rsplit('.', 1)[1]}" in consts:
                w = consts[f"widths.{width.rsplit('.', 1)[1]}"]
            else:
                # a width worked out as the table is laid out: the column takes the room there is
                continue
            key = m.group("key")
            if key is None:
                # `case "duration": return { key, … }`: the key is the case's
                back = "\n".join(lines[max(0, n - 3) : n])
                found = re.findall(r'case "([\w-]+)":', back)
                key = found[-1] if found else ""
            title = m.group("title")
            sortable = "sortable: true" in m.group("rest")
            # the table's type size: the nearest `fontSize={TYPE.x}` after the columns
            after = "\n".join(lines[n : n + 40])
            size = re.search(r"fontSize=\{TYPE\.(\w+)\}", after)
            points = TYPE.get(size.group(1), 13) if size else 13
            mono = bool(re.search(r"fontFamily=\{[^}]*font-mono", after)) and "terminal() ? undefined" not in after
            for lang, words in locales.items():
                if title.startswith("t("):
                    said = words.get(title[3:-2], "")
                else:
                    said = title.strip('"')
                need = text_width(said, TYPE["caption"]) + PAD + (SORT if sortable else 0)
                checked["titles"] += 1
                if said and need > w:
                    say(f"{rel}:{n}", f"[title] {key or said}: “{said}” ({lang}) needs {need:.0f} pt of {w} pt")
            # what fills the column, where a formatter writes it
            expr = re.search(rf"\b{re.escape(key)}: ([^\n]*?)(?:, \w+: |\s*\}}\)|$)", "\n".join(l for l in lines if re.search(rf"\b{re.escape(key)}: ", l) and "title:" not in l)) if key else None
            kind = kind_of(expr.group(1)) if expr else None
            if kind:
                for lang in locales:
                    widest = WIDEST[kind][lang]
                    need = text_width(widest, TYPE["caption"] if kind == "clock" else points, mono or kind == "clock") + PAD
                    checked["cells"] += 1
                    if need > w:
                        say(f"{rel}:{n}", f"[cell] {key}: a {kind} as wide as “{widest}” ({lang}) needs {need:.0f} pt of {w} pt")

    # a column of paths drawn by the table's own cell: cut in its middle or at its start
    for m in re.finditer(r'c\.key === "([\w-]+)"', src):
        key = m.group(1)
        if key not in PATHS:
            continue
        n = src[: m.start()].count("\n") + 1
        near = "\n".join(lines[n - 1 : n + 8])
        checked["paths"] += 1
        if "line-clamp-1" in near and not re.search(r"ellipsis-(?:middle|start)|\bmiddle\(|<Path\b", near):
            say(f"{rel}:{n}", f"[path] {key}: cut at its end (line-clamp-1); a path is cut in its middle (`textOverflow: \"ellipsis-middle\"`) or at its start")

# the bucket's sheets: their tables fit the panel
store = (app / "screens" / "StoreArea.tsx").read_text()
detail = set(re.findall(r"return <(\w+) />", store)) - {"Browse"}
for name in sorted(detail):
    src = (app / "screens" / f"{name}.tsx").read_text()
    for m in re.finditer(r"<DataTable\b.*?columns=\{\[(.*?)\]\}", src, re.S):
        widths = [int(w) if w.isdigit() else KIT.get(w, 0) for w in re.findall(r"\bwidth:\s*(\d+|COLUMN\.\w+)", m.group(1))]
        # a column that takes the room the others leave: its least
        widths += [KIT.get(w, 0) for w in re.findall(r"width: \w+\(\), minWidth: (COLUMN\.\w+)", m.group(1))]
        if not widths:
            continue
        total = sum(widths)
        checked["tables"] += 1
        if total > FITS:
            say(f"app/src/screens/{name}.tsx:{src[: m.start()].count(chr(10)) + 1}", f"[sheet] columns {total} pt of {FITS} pt: too wide")
print(f"layout: {checked['titles']} titles and {checked['cells']} widest cells against their columns' widths (en, zh); {checked['tables']} sheet tables against the panel's {FITS} pt; {checked['paths']} columns of paths cut in their middle: {'failed' if failed else 'ok'}")
sys.exit(1 if failed else 0)
