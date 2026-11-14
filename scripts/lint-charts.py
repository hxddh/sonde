#!/usr/bin/env python3
"""Chart lint: one chart system (app/src/ui/plots/, its constants in system.tsx), counted.

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
- edge: `ml-/pl-/mx-` gutters only in the `GUTTER*` constants: one width for
  the names of rows (`GUTTER`, `GUTTER_PAD`, `GUTTER_BOTH`), one for a scale's
  words outside its plot (`GUTTER_SCALE`);
- treemap: every `<Treemap` plot is `TREEMAP` high;
- legend: swatches only in `Legend`; tooltip: every interactive `<Plot` has
  `<ChartTip`, placed by the pointer at one offset; fade: `INACTIVE` only;
  outline: `HOVER` only;
- time axis: one (`TimeAxis`, its ticks from `timeTicks`): a `PlotAxis` that
  writes a moment (`sinceTick`, a row's `tMs`) anywhere else fails, as does
  a chart named in `OVER_TIME` that draws no `<TimeAxis`;
- value scale: every chart named in `COUNTS` has a `<ValueAxis` (zero and
  the peak, with its unit); where every bar says its own count (each `<Bar`
  has a `label=`) the scale is `marked` (zero alone), and a `marked` scale
  stands only under bars that all say theirs (16.0's lint demanded a peak
  where DESIGN says there is none);
- time's ticks: `LABEL.time` only in `TimeAxis`; a bottom axis of buckets or
  measures is `LABEL.bucket`;
- words: a chart's text is a `LABEL` role (the kit's `TEXT` underneath, so
  Chinese is set at 12 pt): no `text-caption` in a chart file;
- strokes: a line's width is a `STROKE` name and a dash a `DASH` one: no
  `stroke-[…]`, `stroke-N` or dash pattern of its own outside system.tsx;
- bars: thickness only from `BARS`/`CELLS`, tips only from `TIP`, heights
  only the named ones (`PLOT`, `SHAPE`, `STRIP`, `STACK`, `SHAPE_STRIP`, `MARK`,
  `TREEMAP`, `FLOWS`): a literal `paddingInner:`, `cornerRadii={…}` or `h-N`
  outside system.tsx fails, in a chart file and on a chart in a page;
- latency's two tones (`latency-body`, `latency-tail` in theme.json) pass a
  contrast check of 3:1 against each other and against the card, light and dark;
  a heat map's ramp (`heat-low` to `heat-high`) has its busiest end 3:1 from the
  card and its quietest end seen on it (1.4:1), light and dark.
Run by lint-kit.py, so CI's lint step covers it.
"""
import json
import math
import re
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
app = root / "app" / "src"
plots = app / "ui" / "plots" / "system.tsx"
FAMILIES = sorted((app / "ui" / "plots").glob("*.tsx"))
SYSTEM = FAMILIES + [app / "ui" / "charts.tsx"]
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
everything = "\n".join(p.read_text() for p in SYSTEM)


def body_of(name):
    m = re.search(rf"^export function {name}\(.*?^}}", everything, re.S | re.M)
    return m.group(0) if m else ""


for name in FRAMED:
    if "<Chart" not in body_of(name):
        print(f"{rel(plots)}:1: [chart] {name} is named in FRAMED and draws no <Chart> frame")
        failed = True
OVER_TIME = names("OVER_TIME")
COUNTS = names("COUNTS")
for name in OVER_TIME:
    if "<TimeAxis" not in body_of(name):
        say(plots, 1, f"{name} is a chart over time and draws no <TimeAxis>: the one time axis")
for name in COUNTS:
    if "<ValueAxis" not in body_of(name):
        say(plots, 1, f"{name} is a chart of counts and has no <ValueAxis>: zero and the peak, with its unit")
for name in COUNTS:
    body = body_of(name)
    bars = re.findall(r"<Bar\b.*?/>", body)
    marked = re.search(r"<ValueAxis\b[^\n]*?\bmarked\b", body)
    if bars and all("label=" in b for b in bars) and not marked:
        say(plots, 1, f"{name}: every bar says its own count, and its scale draws a peak too: <ValueAxis marked …/> (zero alone)")
    if marked and not (bars and all("label=" in b for b in bars)):
        say(plots, 1, f"{name}: a scale of zero alone (marked) over bars that do not all say their count")
for name in OVER_TIME + COUNTS:
    if name not in CHARTS and name not in MARKS:
        say(plots, 1, f"{name} is named in OVER_TIME or COUNTS and is no chart")


def functions(text):
    """(name, first line, last line) of each top-level function."""
    starts = [(m.group(1), text[: m.start()].count("\n") + 1) for m in re.finditer(r"^(?:export )?function (\w+)\(", text, re.M)]
    total = text.count("\n") + 1
    return [(name, a, (starts[i + 1][1] - 1) if i + 1 < len(starts) else total) for i, (name, a) in enumerate(starts)]


counts = {"frames": 0, "plots": 0, "legends": 0, "tooltips": 0, "time": 0, "value": 0}
HEIGHT = re.compile(r"(?<![\w-])h-(?!full\b|px\b)[\w.\[\]]+")
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
    counts["time"] += len(re.findall(r"<TimeAxis\b", text))
    counts["value"] += len(re.findall(r"<ValueAxis\b", text))
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
        # time's ticks are TimeAxis's: another bottom axis has its own name
        if re.search(r"labelClass=\{LABEL\.time\}", line) and inside(n) != "TimeAxis":
            say(p, n, "time's ticks on an axis that is not time: LABEL.bucket")
        if p in SYSTEM and re.search(r"(?<![\w-])text-caption(?![\w-])", line):
            say(p, n, "the caption by hand in a chart: a LABEL role (Chinese takes 12 pt there)")
        if p in SYSTEM and p != plots and re.search(r"(?<![\w-])stroke-(?:\[[\d.]+\]|\d+\b)|strokeDasharray: \"", line):
            say(p, n, "a stroke width or dash of its own: STROKE or DASH")
        # one time axis: a moment is written on an axis by TimeAxis only
        if "<PlotAxis" in line and re.search(r"sinceTick|tMs\b", line) and inside(n) != "TimeAxis":
            say(p, n, "a time axis of its own: <TimeAxis domain={…} /> (slices={n} on a band scale)")
        # one of each measure: thickness, tip and height are the system's
        if p != plots:
            if re.search(r"paddingInner: [\d.]|paddingOuter: [\d.]", line):
                say(p, n, "a bar thickness of its own: BARS or CELLS")
            if re.search(r"(?:cornerRadii|nodeCornerRadius|paddingInner)=\{[\d{]", line):
                say(p, n, "a tip radius of its own: TIP")
            chart_here = p in SYSTEM or any(re.search(rf"<{name}\b", line) for name in CHARTS + MARKS)
            if chart_here and HEIGHT.search(re.sub(r"//.*|\{/\*.*?\*/\}", "", line)) and "stroke-" not in line:
                say(p, n, f"a chart height of its own ({HEIGHT.search(line).group(0)}): PLOT, SHAPE, STRIP, STACK, SHAPE_STRIP, MARK, TREEMAP or FLOWS")
        if p in SYSTEM or "Plot" in text:
            for m in re.finditer(r"\.length \* (\d+)", line):
                if p in SYSTEM:
                    pitches.add(m.group(1))
                    say(p, n, f"a row pitch of its own ({m.group(1)}): rowsHeight(n)")
        if p in SYSTEM:
            for m in re.finditer(r"(?<![\w-])(?:ml|pl|mx)-(\d\d)\b", line):
                if re.match(r"^(?:export )?const GUTTER(?:_PAD|_BOTH)? = ", line):
                    edges.add(m.group(1))
                elif re.match(r"^(?:export )?const GUTTER_SCALE(?:_END)? = ", line):
                    pass
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
        if re.search(r"\bfunction (?:Tip|Legend|ChartTip|Chart|TimeAxis|ValueAxis)\b", line) and p != plots and not (p.name == "kit.tsx" and "function Tip(" in line):
            say(p, n, "a second frame, legend, tooltip or axis: plots/system.tsx has the one")

pitch = re.findall(r"^export const PITCH = (\d+);", src, re.M)
if pitch != ["28"]:
    say(plots, 1, f"the row pitch is 28, once: found {pitch}")
if len(edges) != 1:
    say(plots, 1, f"one left edge: found {sorted(edges)}")
for const in ("TREEMAP", "INACTIVE", "HOVER", "GUTTER", "PITCH", "PLOT", "SHAPE", "STRIP", "STACK", "BARS", "CELLS", "TIP", "MARK"):
    for p in files:
        if p != plots and re.search(rf"^(?:export )?const {const} = ", p.read_text(), re.M):
            say(p, 1, f"{const} mirrored: import it from plots")
    if len(re.findall(rf"^export const {const} = ", src, re.M)) != 1:
        say(plots, 1, f"{const} is defined once in system.tsx")


# latency's two tones: told apart from each other and from the card, in light and in dark
def luminance(oklch):
    L, C, H = [float(x) for x in re.match(r"oklch\(([\d.]+) ([\d.]+) ([\d.]+)", oklch).groups()]
    a, b = C * math.cos(math.radians(H)), C * math.sin(math.radians(H))
    l, m, s = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3, (L - 0.1055613458 * a - 0.0638541728 * b) ** 3, (L - 0.0894841775 * a - 1.2914855480 * b) ** 3
    rgb = (4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s, -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s)
    r, g, bl = [min(1.0, max(0.0, x)) for x in rgb]
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl


def contrast(x, y):
    hi, lo = sorted([luminance(x), luminance(y)], reverse=True)
    return (hi + 0.05) / (lo + 0.05)


theme = json.loads((root / "app" / "theme.json").read_text())["cssVars"]
tones = []
for mode in ("light", "dark"):
    t = theme[mode]
    pairs = {"body/card": contrast(t["latency-body"], t["card"]), "tail/card": contrast(t["latency-tail"], t["card"]), "body/tail": contrast(t["latency-body"], t["latency-tail"])}
    tones.append(f"{mode} " + " ".join(f"{k} {v:.1f}" for k, v in pairs.items()))
    for k, v in pairs.items():
        if v < 3:
            say(root / "app" / "theme.json", 1, f"latency's tones, {mode}: {k} is {v:.2f}:1, under 3:1")
    heat = {"heat-high/card": (contrast(t["heat-high"], t["card"]), 3), "heat-low/card": (contrast(t["heat-low"], t["card"]), 1.4)}
    tones.append(f"{mode} heat " + " ".join(f"{k} {v:.1f}" for k, (v, _) in heat.items()))
    for k, (v, least) in heat.items():
        if v < least:
            say(root / "app" / "theme.json", 1, f"the heat ramp, {mode}: {k} is {v:.2f}:1, under {least}:1")
print(f"charts: {counts['plots']} plots in {len(CHARTS)} charts and {len(MARKS)} marks; {counts['frames']} frames used; row pitch {'/'.join(pitch)}; left edge {'/'.join(sorted(edges))}; {counts['legends']} uses of the one legend; {counts['tooltips']} of the one tooltip; {counts['time']} uses of the one time axis over {len(OVER_TIME)} charts; {counts['value']} of the one value scale over {len(COUNTS)}; latency's tones {'; '.join(tones)}: {'failed' if failed else 'ok'}")
sys.exit(1 if failed else 0)
