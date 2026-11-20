#!/usr/bin/env python3
"""The README is this version's: its pictures are this version's screens,
and it and the docs name the action at this version's major.

    scripts/check-readme-images.py

Each picture `scripts/shots.sh` copies into docs/images (its MAP, with
SONDE_README=1) must be the same picture as its tour shot approved for linux
in app/shots.approved.json, by the fingerprint `scripts/shots-diff.py`
compares screens with. A picture that differs is an old screen: take the
tour (`SONDE_README=1 sh scripts/shots.sh`) once its pictures are approved.
Every `hxddh/sonde@vN` in the README, docs (not their history), action.yml,
install.sh and the workflows must be Cargo.toml's major.
The last line says `ok` or `failed`.
"""
from __future__ import annotations

import importlib.util
import json
import re
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("shots_diff", root / "scripts" / "shots-diff.py")
assert spec and spec.loader
shots_diff = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shots_diff)

PLATFORM = "linux"


def readme_map(shots_sh: str) -> list[tuple[str, str]]:
    """The (tour shot, docs/images name) pairs of shots.sh's MAP."""
    m = re.search(r"<<'MAP'\n(.*?)\nMAP\n", shots_sh, re.S)
    if not m:
        return []
    return [tuple(line.split()[:2]) for line in m.group(1).splitlines() if len(line.split()) >= 2]  # type: ignore[misc]


def stale_majors() -> list[str]:
    """Each `hxddh/sonde@vN` whose N is not Cargo.toml's major, where it is."""
    major = re.search(r'^version = "(\d+)\.', (root / "Cargo.toml").read_text(), re.M).group(1)  # type: ignore[union-attr]
    files = [root / "README.md", root / "action.yml", root / "install.sh"]
    files += [p for p in (root / "docs").rglob("*.md") if "history" not in p.parts]
    files += sorted((root / ".github" / "workflows").glob("*.yml"))
    return [
        f"{p.relative_to(root)}:{n}: hxddh/sonde@v{m.group(1)}, not v{major} (Cargo.toml's major)"
        for p in files
        for n, line in enumerate(p.read_text().splitlines(), 1)
        for m in re.finditer(r"hxddh/sonde@v(\d+)", line)
        if m.group(1) != major
    ]


def main() -> int:
    stale = stale_majors()
    for line in stale:
        print(line)
    pairs = readme_map((root / "scripts" / "shots.sh").read_text())
    approved = json.loads((root / "app" / "shots.approved.json").read_text()).get("pictures", {}).get(PLATFORM, {})
    failed = not pairs or bool(stale)
    if not pairs:
        print("scripts/shots.sh: no README map (<<'MAP' … MAP)")
    same = 0
    for shot, name in pairs:
        picture = root / "docs" / "images" / f"{name}.png"
        want = approved.get(f"{shot}.png")
        if want is None:
            print(f"docs/images/{name}.png: the tour has no approved {PLATFORM} shot {shot}")
            failed = True
        elif not picture.exists():
            print(f"docs/images/{name}.png: missing (the tour's {shot})")
            failed = True
        elif (d := shots_diff.distance(shots_diff.fingerprint(picture), want)) > shots_diff.tolerance(shot):
            print(f"docs/images/{name}.png: not the tour's approved {shot} ({d} cells differ)")
            failed = True
        else:
            same += 1
    print(f"readme images: {same} of {len(pairs)} are the tour's approved {PLATFORM} pictures (SONDE_README=1 sh scripts/shots.sh takes them); {len(stale)} names of the action at another major: {'failed' if failed else 'ok'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
