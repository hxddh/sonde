#!/usr/bin/env python3
"""Every word the app says, in every language, and no word it does not say:
the keys of app/locales/en.json must all be in the other locales, with the
same %{placeholders}, and each must be used — named whole in the app's or the
viewer's source, or a member of a family a template builds its keys from
(t(`trial.${standing}`)). A family is listed here with its members, key by
key: a prefix alone passes nothing (15.0 had 87 keys that passed only by
one), a member with no words fails, and so does a template whose family is
not listed. A key nothing uses is a sentence nobody keeps true (14.0 had 48)."""
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

# The families of keys a template builds, each with its members: what the
# value in the template can be. A new member is added here with its words.
FAMILIES = {
    "audit.action.": "abortUploads deleteVersions restoreObject deleteObjects putLifecycle runProbes",
    "board.": "traffic latency errors where",
    "byGroup.": "prefix size caller sdk status",
    "costBy.": "write list read free",
    "failures.": "server client transport injected",
    "flows.": "ok notfound clienterror servererror",
    "jobs.state.": "running finished failed cancelled",
    "layouts.kind.": "commit fence gap manifest object seal segment snapshot wal writes",
    "layouts.severity.": "error warning info",
    "mpu.lifecycle.": "covered partial missing unknown",
    "palette.": "light dark system",
    "prefixUse.limit.": "reads writes",
    "scenario.": "slowdown error lost-answer fail-complete cut-body lost-race skew complete-500 slow-first-byte stale-list slow s3-limits",
    "scenario.about.": "slowdown error lost-answer fail-complete cut-body lost-race skew complete-500 slow-first-byte stale-list slow s3-limits",
    "settings.path.": "config exports downloads audit",
    "split.by.": "none kind prefix size caller sdk status",
    "trial.": "survived degraded broke untested refused",
    "verdict.": "pass fail differs unsupported error skipped requests waiting cost errors warnings p95",
    "why.": "obs.unfinished obs.orphans obs.small obs.hot obs.listed obs.headget obs.throttled obs.throttled.rate obs.serial obs.prefixrate",
}

# used: the key's own text in the source, or a member of a listed family
sources = [p for d in ("app/src", "viewer") for p in (top / d).rglob("*.ts*") if ".nsh" not in p.parts] + [top / "app" / "App.tsx"]
text = "\n".join(p.read_text() for p in sources if p.exists())
templates = set(re.findall(r"`([a-zA-Z][\w-]*(?:\.[\w-]+)*\.)\$\{", text))
members = set()
for prefix, names in FAMILIES.items():
    if prefix not in templates:
        print(f"check-locales: the family {prefix}* is listed and no template builds it: delete it")
        bad += 1
    for name in names.split():
        members.add(prefix + name)
        if prefix + name not in en:
            print(f"en.json: {prefix}{name} is a member of {prefix}* and has no words")
            bad += 1
for prefix in sorted(templates - FAMILIES.keys()):
    if any(k.startswith(prefix) for k in en):
        print(f"check-locales: a template builds {prefix}* keys and the family is not listed: name its members in FAMILIES")
        bad += 1
families = 0
for k in en:
    base = k[: -len(".one")] if k.endswith(".one") else k
    if f'"{base}"' in text or f"'{base}'" in text or f"`{base}`" in text:
        continue
    if base in members:
        families += 1
        continue
    print(f"en.json: {k} is used nowhere")
    bad += 1
# what sonde says of a session (app/src/phrases.ts): English is Rust's own templates, letter for
# letter (crates/proxy/src/phrase.rs), and Chinese has a template for every one of them
phrases = (top / "app" / "src" / "phrases.ts").read_text()
rust = (top / "crates" / "proxy" / "src" / "phrase.rs").read_text()
rust = rust[rust.index("fn template(key: &str)") :]
rust = rust[: rust.index("\n}\n")]
theirs = {m.group(1): json.loads('"' + m.group(2) + '"') for m in re.finditer(r'"([\w.]+)" => (?:\{\s*)?"((?:[^"\\]|\\.)*)"', rust)}


def table(name):
    body = phrases[phrases.index(f"const {name}: Record<string, string> = {{") :]
    body = body[: body.index("\n};")]
    return {m.group(1) or m.group(2): json.loads('"' + m.group(3) + '"') for m in re.finditer(r'^  (?:"([\w.]+)"|(\w+)): "((?:[^"\\]|\\.)*)",$', body, re.M)}


ours, zh = table("en"), table("zh")
for k in sorted(theirs.keys() | ours.keys()):
    if theirs.get(k) != ours.get(k):
        print(f"phrases.ts: en[{k!r}] is not Rust's template ({'missing in phrases.ts' if k not in ours else 'missing in phrase.rs' if k not in theirs else 'they differ'})")
        bad += 1
for k in sorted(theirs.keys() - zh.keys()):
    print(f"phrases.ts: zh has no template for {k!r}")
    bad += 1
print(f"locales: {len(theirs)} phrases said from the templates of phrase.rs (en, zh); {len(en)} keys, {families} of them by {len(FAMILIES)} listed families: " + ("ok" if not bad else f"{bad} problems"))
sys.exit(1 if bad else 0)
