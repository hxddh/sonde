#!/usr/bin/env python3
"""The app's icon per platform, from the one drawing (app/assets/icon.png).

macOS draws an icon inset on a transparent canvas: the artwork 824 of the
canvas's 1024 pixels, centred (Native Shell's packaging guide, "Icons").
Windows and Linux fill the canvas edge to edge. nsh.json names one file each
(`icon` as an object); run this after the drawing changes:

    python3 scripts/icons.py          # writes app/assets/icon-macos.png and icon-fill.png
    python3 scripts/icons.py --check  # fails when either is not what the drawing gives
"""
import sys
from pathlib import Path

from PIL import Image

ASSETS = Path(__file__).resolve().parent.parent / "app" / "assets"
CANVAS = 1024
MACOS_ART = 824


def artwork(src: Image.Image) -> Image.Image:
    """The drawing without the transparent margin around it."""
    box = src.getchannel("A").getbbox()
    if box is None:
        raise SystemExit("icons: the drawing is empty")
    art = src.crop(box)
    if abs(art.width - art.height) > 2:
        raise SystemExit(f"icons: the artwork is not square ({art.width}x{art.height})")
    return art


def placed(art: Image.Image, size: int) -> Image.Image:
    """The artwork `size` pixels wide, centred on a transparent canvas."""
    out = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    scaled = art.resize((size, size), Image.LANCZOS)
    at = (CANVAS - size) // 2
    out.paste(scaled, (at, at), scaled)
    return out


def main() -> int:
    src = Image.open(ASSETS / "icon.png").convert("RGBA")
    art = artwork(src)
    made = {"icon-macos.png": placed(art, MACOS_ART), "icon-fill.png": placed(art, CANVAS)}
    if "--check" in sys.argv:
        bad = [n for n, im in made.items() if not (ASSETS / n).is_file() or Image.open(ASSETS / n).convert("RGBA").tobytes() != im.tobytes()]
        for n in bad:
            print(f"icons: app/assets/{n} is not what icon.png gives: run scripts/icons.py")
        print(f"icons: macOS artwork {MACOS_ART} of {CANVAS}, the others edge to edge: {'failed' if bad else 'ok'}")
        return 1 if bad else 0
    for n, im in made.items():
        im.save(ASSETS / n, optimize=True)
        print(f"icons: wrote app/assets/{n}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
