#!/usr/bin/env python3
"""Every word the app says, in every language, and no word it does not say:
the keys of app/locales/en.json must all be in the other locales, with the
same %{placeholders}, and each must be used — named whole in the app's or the
viewer's source, or built there from a prefix (t(`trial.${standing}`)). A key
nothing uses is a sentence nobody keeps true (14.0 had 48 of them)."""
import json, pathlib, re, sys

top = pathlib.Path(__file__).resolve().parent.parent
root = top / "app" / "locales"
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

# used: the key's own text in the source, or a prefix a template builds keys from
sources = [p for d in ("app/src", "viewer") for p in (top / d).rglob("*.ts*") if ".nsh" not in p.parts] + [top / "app" / "App.tsx"]
text = "\n".join(p.read_text() for p in sources if p.exists())
prefixes = set(re.findall(r"`([a-zA-Z][\w-]*(?:\.[\w-]+)*\.)\$\{", text))
for k in en:
    base = k[: -len(".one")] if k.endswith(".one") else k
    if f'"{base}"' in text or f"'{base}'" in text or f"`{base}`" in text or any(base.startswith(p) for p in prefixes):
        continue
    print(f"en.json: {k} is used nowhere")
    bad += 1
print("locales: ok" if not bad else f"locales: {bad} problems")
sys.exit(1 if bad else 0)
