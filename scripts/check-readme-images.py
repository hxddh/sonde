#!/usr/bin/env python3
"""The README's pictures are this version's screens.

    scripts/check-readme-images.py

Each picture `scripts/shots.sh` copies into docs/images (its MAP, with
SONDE_README=1) must be the same picture as its tour shot approved for linux
in app/shots.approved.json, by the fingerprint `scripts/shots-diff.py`
compares screens with. A picture that differs is an old screen: take the
tour (`SONDE_README=1 sh scripts/shots.sh`) once its pictures are approved.
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


def main() -> int:
    pairs = readme_map((root / "scripts" / "shots.sh").read_text())
    approved = json.loads((root / "app" / "shots.approved.json").read_text()).get("pictures", {}).get(PLATFORM, {})
    failed = not pairs
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
        elif (d := shots_diff.distance(shots_diff.fingerprint(picture), want)) > shots_diff.TOLERANCE:
            print(f"docs/images/{name}.png: not the tour's approved {shot} ({d} cells differ)")
            failed = True
        else:
            same += 1
    print(f"readme images: {same} of {len(pairs)} are the tour's approved {PLATFORM} pictures (SONDE_README=1 sh scripts/shots.sh takes them): {'failed' if failed else 'ok'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
