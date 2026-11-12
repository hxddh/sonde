#!/usr/bin/env python3
"""What changed on screen, and whether it was approved.

    scripts/shots-diff.py BEFORE_DIR AFTER_DIR OUT_DIR [--approved FILE --platform NAME]
    scripts/shots-diff.py approve SHOTS_DIR FILE PLATFORM

The first form: for every screenshot in both runs, the share of pixels that
changed (a pixel counts past a difference of 16 of 255, so font smoothing
does not); those past 0.5% get a before | after | difference image in
OUT_DIR. Prints a Markdown table (CI appends it to the job summary), new and
missing screens included. With `--approved`, a screen that changed past 0.5%,
or is new, must match the picture approved for it in FILE (the committed
baseline, `app/shots.approved.json`): else it is an unapproved change and the
exit is 1. A platform FILE has no pictures for is reported, never failed.

The second form approves a tour: it writes each screenshot's fingerprint
into FILE under PLATFORM (the pictures of a CI run's `sonde-shots-<platform>`
artifact, downloaded), so the change is approved by committing FILE.

A fingerprint is the picture's mean grey in a 40 × 25 grid, each cell one of
16 levels, compressed: small enough to commit, coarse enough that font
smoothing and a pixel's shift do not change it, fine enough that a change
past 0.5% of the pixels does. Two match when no more than `TOLERANCE` cells
differ by more than one level.
"""
from __future__ import annotations

import base64
import json
import re
import sys
import zlib
from pathlib import Path

from PIL import Image, ImageChops

GRID = (40, 25)
LEVELS = 16
# cells that may differ by more than a level and still be the same picture
TOLERANCE = 2
# screens that draw what the tour's run measured (latencies, its timeline, a request's timings): on the same code
# they move from run to run by more than any tolerance that would still catch a change, so they are reported with
# their difference images for a person to look at, and not held
LIVE = re.compile(r"-(02c-errors|02d-first-byte|02j-trial-time|03c-request|03g-time|03p-programs)\.png$")


def is_held(name: str) -> bool:
    return not LIVE.search(name)

# the share of pixels past which a screen changed (and the difference that makes a pixel count)
CHANGED_PCT = 0.5
PIXEL = 16


def fingerprint(path: Path) -> str:
    """A picture's grid of mean greys, a level each, compressed."""
    im = Image.open(path).convert("L").resize(GRID, Image.Resampling.BOX)
    cells = bytes(v * LEVELS // 256 for v in im.tobytes())
    return base64.b64encode(zlib.compress(cells, 9)).decode()


def cells(fp: str) -> bytes:
    return zlib.decompress(base64.b64decode(fp))


def distance(a: str, b: str) -> int:
    """How many cells of two fingerprints differ by more than a level (every one, for grids that differ)."""
    x, y = cells(a), cells(b)
    if len(x) != len(y):
        return len(x) or 1
    return sum(1 for p, q in zip(x, y) if abs(p - q) > 1)


def approve(shots: Path, file: Path, platform: str) -> None:
    data = json.loads(file.read_text()) if file.exists() else {}
    data.setdefault("about", "The approved screenshot tour, per platform: scripts/shots-diff.py approve writes it; CI fails a changed screen that does not match it")
    data["pictures"] = data.get("pictures", {})
    data["pictures"][platform] = {p.name: fingerprint(p) for p in sorted(shots.glob("*.png"))}
    file.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n")
    print(f"{file}: {len(data['pictures'][platform])} pictures approved for {platform}")


def diff(before: Path, after: Path, out: Path, approved: Path | None, platform: str | None) -> int:
    out.mkdir(parents=True, exist_ok=True)
    a_names = {p.name for p in before.glob("*.png")} if before.is_dir() else set()
    b_names = {p.name for p in after.glob("*.png")}
    rows = []
    for name in sorted(a_names & b_names):
        a = Image.open(before / name).convert("RGB")
        b = Image.open(after / name).convert("RGB")
        if a.size != b.size:
            rows.append((100.0, name, f"size {a.size} → {b.size}"))
            continue
        d = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > PIXEL else 0)
        changed = sum(d.histogram()[255:]) / (a.size[0] * a.size[1]) * 100
        if changed > CHANGED_PCT:
            w, h = a.size
            sheet = Image.new("RGB", (w * 3, h), "white")
            sheet.paste(a, (0, 0))
            sheet.paste(b, (w, 0))
            sheet.paste(Image.composite(Image.new("RGB", a.size, (230, 30, 60)), b.point(lambda v: v // 3), d), (w * 2, 0))
            sheet.save(out / name)
        rows.append((changed, name, ""))
    # the screens to hold to the approved pictures: those that changed, and the new ones
    held = [name for changed, name, note in rows if changed > CHANGED_PCT or note] + sorted(b_names - a_names)
    pictures = None
    if approved and platform:
        pictures = (json.loads(approved.read_text()).get("pictures", {}) if approved.exists() else {}).get(platform)
    verdict = {}
    if pictures is not None:
        for name in held:
            if not is_held(name):
                verdict[name] = "live: look"
                continue
            fp = pictures.get(name)
            verdict[name] = "approved" if fp and distance(fp, fingerprint(after / name)) <= TOLERANCE else "**not approved**"
    print("### Screens against the last green run\n")
    print(f"{len(a_names & b_names)} compared · {len(b_names - a_names)} new · {len(a_names - b_names)} gone\n")
    print("| screen | changed | |\n|---|---:|---|")
    for changed, name, note in sorted(rows, reverse=True):
        if changed > CHANGED_PCT or note:
            print(f"| {name} | {changed:.1f}% | {' · '.join(x for x in (note, verdict.get(name, '')) if x)} |")
    for name in sorted(b_names - a_names):
        print(f"| {name} | new | {verdict.get(name, '')} |")
    for name in sorted(a_names - b_names):
        print(f"| {name} | gone | |")
    unapproved = sorted(n for n, v in verdict.items() if v not in ("approved", "live: look"))
    print()
    if pictures is None:
        print(f"No approved pictures for {platform or 'this platform'}: reported, not held." if approved else "")
    elif unapproved:
        print(f"**{len(unapproved)} changed screens are not approved.** Look at the before | after | difference images (the `sonde-shots-diff-{platform}` artifact); to approve them, download this run's `sonde-shots-{platform}` artifact and run `scripts/shots-diff.py approve DIR app/shots.approved.json {platform}`, then commit the file.")
    else:
        print(f"Every changed screen is approved ({len(verdict)}).")
    return 1 if unapproved else 0


def main(argv: list[str]) -> int:
    if argv[:1] == ["approve"]:
        approve(Path(argv[1]), Path(argv[2]), argv[3])
        return 0
    approved = platform = None
    if "--approved" in argv:
        i = argv.index("--approved")
        approved = Path(argv[i + 1])
        del argv[i : i + 2]
    if "--platform" in argv:
        i = argv.index("--platform")
        platform = argv[i + 1]
        del argv[i : i + 2]
    before, after, out = (Path(a) for a in argv[:3])
    return diff(before, after, out, approved, platform)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
