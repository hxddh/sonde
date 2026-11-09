#!/usr/bin/env python3
"""Every word the app says, in every language: the keys of app/locales/en.json
must all be in the other locales, with the same %{placeholders}."""
import json, pathlib, re, sys

root = pathlib.Path(__file__).resolve().parent.parent / "app" / "locales"
en = json.loads((root / "en.json").read_text())
holes = lambda s: sorted(set(re.findall(r"%\{(\w+)\}", s)))
bad = 0
for f in sorted(root.glob("*.json")):
    if f.name == "en.json":
        continue
    other = json.loads(f.read_text())
    for k, v in en.items():
        if k not in other:
            print(f"{f.name}: missing {k}")
            bad += 1
        # a count of one may write its "1" out (English) or keep %{n} (Chinese)
        elif [h for h in holes(v) if not (k.endswith(".one") and h == "n")] != [h for h in holes(other[k]) if not (k.endswith(".one") and h == "n")]:
            print(f"{f.name}: {k} has {holes(other[k])}, English has {holes(v)}")
            bad += 1
    for k in other.keys() - en.keys():
        print(f"{f.name}: {k} is not in en.json")
        bad += 1
print("locales: ok" if not bad else f"locales: {bad} problems")
sys.exit(1 if bad else 0)
