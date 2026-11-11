#!/usr/bin/env python3
"""Chart lint: one chart system (app/src/ui/plots.tsx), counted.

14.0 counted 24 one-off charts: five row pitches, three left edges, three
treemap heights, four legend gaps, two swatch shapes, two tooltips, two
fades, two hover outlines, and a frame only the Time pane used. This counts
them across app/src and viewer and exits 1 when a count is not one, or when
a chart bypasses the system:

- frame: every `<Plot` is inside a function named in `CHARTS` (or `MARKS`)
  in plots.tsx, and every use of a chart component is inside a `<Chart`
  frame or a chart descriptor's `body:` (the Time pane's list), unless it
  is named in `FRAMED` and draws its own;
- pitch: a chart of rows is `rowsHeight(n)` high — no `length * N`;
- edge: `ml-/pl-/mx-` gutters only in the `GUTTER*` constants, one width;
- treemap: every `<Treemap` plot is `TREEMAP` high;
- legend: swatches only in `Legend`; tooltip: every interactive `<Plot` has
  `<ChartTip`, placed by the pointer at one offset; fade: `INACTIVE` only;
  outline: `HOVER` only.
Run by lint-kit.py, so CI's lint step covers it.
"""
import re
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
app = root / "app" / "src"
plots = app / "ui" / "plots.tsx"
SYSTEM = [plots, app / "ui" / "charts.tsx"]
files = sorted(app.rglob("*.tsx")) + sorted((root / "viewer").glob("*.tsx"))
failed = False


def rel(p):
    return p.relative_to(root).as_posix()


def say(p, n, why):
    global failed
    print(f"{rel(p)}:{n}: [chart] {why}")
    failed = True


src = plots.read_text()
names = lambda const: re.findall(r'"([\w ]+)"', re.search(rf"export const {const} = \[(.*?)\]", src, re.S).group(1))
CHARTS = names("CHARTS")
MARKS = [m.split()[0] for m in names("MARKS")]
FRAMED = names("FRAMED")
for name in FRAMED:
    body = re.search(rf"^export function {name}\(.*?^}}", src, re.S | re.M)
    if not body or "<Chart" not in body.group(0):
        print(f"app/src/ui/plots.tsx:1: [chart] {name} is named in FRAMED and draws no <Chart> frame")
        failed = True


def functions(text):
    """(name, first line, last line) of each top-level function."""
    starts = [(m.group(1), text[: m.start()].count("\n") + 1) for m in re.finditer(r"^(?:export )?function (\w+)\(", text, re.M)]
    total = text.count("\n") + 1
    return [(name, a, (starts[i + 1][1] - 1) if i + 1 < len(starts) else total) for i, (name, a) in enumerate(starts)]


counts = {"frames": 0, "plots": 0, "legends": 0, "tooltips": 0}
pitches, edges = set(), set()
for p in files:
    text = p.read_text()
    lines = text.splitlines()
    fns = functions(text)
    inside = lambda n: next((name for name, a, b in fns if a <= n <= b), None)
    # the spans a frame covers: <Chart …> … </Chart>, and a descriptor's body
    framed = set()
    for m in re.finditer(r"<Chart\b.*?</Chart>", text, re.S):
        framed.update(range(text[: m.start()].count("\n") + 1, text[: m.end()].count("\n") + 2))
    counts["frames"] += len(re.findall(r"<Chart\b", text)) if p != plots else 0
    for n, line in enumerate(lines, 1):
        if "<Plot" in line and re.search(r"<Plot\b", line):
            counts["plots"] += 1
            fn = inside(n)
            if p not in SYSTEM:
                say(p, n, "a Plot outside the chart system: draw it in plots.tsx, in a chart named in CHARTS")
            elif fn not in CHARTS and fn not in MARKS:
                say(p, n, f"a Plot in {fn}(), which CHARTS does not name")
            # the plot's own attributes, to its closing bracket
            block = "\n".join(lines[n - 1 : n + 30])
            block = block[: block.index("\n        >") if "\n        >" in block else len(block)]
            head = re.split(r"\n\s*>\n|\s>\n", "\n".join(lines[n - 1 : n + 30]), maxsplit=1)[0]
            if "interactive={false}" not in head:
                if "<ChartTip" not in head:
                    say(p, n, "a chart with no tooltip: tooltipContent={<ChartTip tip={…} />}")
                else:
                    counts["tooltips"] += 1
                if 'tooltipPlacement="pointer"' not in head or "tooltipOffset={10}" not in head:
                    say(p, n, 'the tooltip placed its own way: tooltipPlacement="pointer" tooltipOffset={10}')
            if "<Treemap" in "\n".join(lines[n - 1 : n + 30]) and "TREEMAP" not in head:
                say(p, n, "a treemap of its own height: class={TREEMAP}")
        for name in CHARTS:
            if re.search(rf"<{name}\b", line) and p not in SYSTEM and name not in FRAMED:
                strip = name == "SessionShape" and " strip" in line
                if n not in framed and "body:" not in line and not strip and not re.search(r"body: \(\) =>", "\n".join(lines[max(0, n - 4) : n])):
                    say(p, n, f"<{name}> outside the frame: put it in a <Chart> (or a chart descriptor's body)")
        if p in SYSTEM or "Plot" in text:
            for m in re.finditer(r"\.length \* (\d+)", line):
                if p in SYSTEM:
                    pitches.add(m.group(1))
                    say(p, n, f"a row pitch of its own ({m.group(1)}): rowsHeight(n)")
        if p in SYSTEM:
            for m in re.finditer(r"(?<![\w-])(?:ml|pl|mx)-(\d\d)\b", line):
                if re.match(r"^(?:export )?const GUTTER\w* = ", line):
                    edges.add(m.group(1))
                else:
                    say(p, n, f"a left edge of its own ({m.group(0)}): GUTTER")
            if re.search(r"inactiveOpacity=\{(?!INACTIVE\})|hoverDim=\{(?!1 - INACTIVE\})", line):
                say(p, n, "a fade of its own: INACTIVE")
            if re.search(r"hover(?:Cell)?Class=\"", line):
                say(p, n, "a hover outline of its own: HOVER")
        if re.search(r"size-2 rounded-(?:full|sm)\b.*swatch|swatch.*size-2 rounded", line) and inside(n) != "Legend":
            say(p, n, "a legend swatch by hand: Legend")
        if re.search(r"<Legend\b", line):
            counts["legends"] += 1
        if re.search(r"\bfunction (?:Tip|Legend|ChartTip|Chart)\b", line) and p != plots and not (p.name == "kit.tsx" and "function Tip(" in line):
            say(p, n, "a second frame, legend or tooltip: plots.tsx has the one")

pitch = re.findall(r"^export const PITCH = (\d+);", src, re.M)
if pitch != ["28"]:
    say(plots, 1, f"the row pitch is 28, once: found {pitch}")
if len(edges) != 1:
    say(plots, 1, f"one left edge: found {sorted(edges)}")
for const in ("TREEMAP", "INACTIVE", "HOVER", "GUTTER", "PITCH"):
    for p in files:
        if p != plots and re.search(rf"^(?:export )?const {const} = ", p.read_text(), re.M):
            say(p, 1, f"{const} mirrored: import it from plots.tsx")
print(f"charts: {counts['plots']} plots in {len(CHARTS)} charts and {len(MARKS)} marks; {counts['frames']} frames used; row pitch {'/'.join(pitch)}; left edge {'/'.join(sorted(edges))}; {counts['legends']} uses of the one legend; {counts['tooltips']} of the one tooltip: {'failed' if failed else 'ok'}")
sys.exit(1 if failed else 0)
