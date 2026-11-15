#!/usr/bin/env python3
"""Every word sonde says, in every language, and no word it does not say:
the keys of app/locales/en.json must all be in the other locales, with the
same %{placeholders}, and each must be used — named whole in the app's or the
viewer's source, a phrase Rust says (`phrase.<key>`, `core.<key>`: the key
named in crates/, a fact's label, a probe's id), or a member of a family a
template builds its keys from
(t(`trial.${standing}`)). A family is listed here with its members, key by
key: a prefix alone passes nothing (15.0 had 87 keys that passed only by
one), a member with no words fails, and so does a template whose family is
not listed. A key nothing uses is a sentence nobody keeps true (14.0 had 48).

And what the app stopped saying is said: every key en.json had at the last
release tag (vX.Y.Z) and has no more is named — its key, or its English
words — in CHANGELOG.md's newest Removed or Changed section, or listed in
`GONE` below with the reason it is no loss (reworded under another key). 23.0
dropped the sidebar's request count and nobody's list said so."""
import json, pathlib, re, subprocess, sys

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
        # a phrase's other language may leave out what English needs (its plural %{s}, an "it")
        elif k.startswith(("phrase.", "core.")):
            if not set(holes(other[k])) <= set(holes(v)):
                print(f"{f.name}: {k} has {holes(other[k])}, English has only {holes(v)}")
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
    "byGroup.": "prefix size caller sdk status key",
    "costBy.": "write list read free",
    "failures.": "throttled server client transport injected",
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
    "split.by.": "none kind prefix size caller sdk status key",
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
# what Rust says: the phrase's key named in its source (a fact by its label, a probe's catalog by its id)
rust = "\n".join(p.read_text() for p in (top / "crates").rglob("*.rs"))


def said_by_rust(k):
    key = k.split(".", 1)[1]
    if key.startswith("fact."):
        return f'"{key[5:]}"' in rust
    probe = re.fullmatch(r"probe\.(.+)\.(title|description|aws)", key)
    if probe:
        return f'"{probe.group(1)}"' in rust or probe.group(1).startswith("checksum.")
    return f'"{key}"' in rust


families = 0
said = 0
for k in en:
    if k.startswith(("phrase.", "core.")):
        if said_by_rust(k):
            said += 1
        else:
            print(f"en.json: {k} is a phrase Rust does not say")
            bad += 1
        continue
    base = k[: -len(".one")] if k.endswith(".one") else k
    if f'"{base}"' in text or f"'{base}'" in text or f"`{base}`" in text:
        continue
    if base in members:
        families += 1
        continue
    print(f"en.json: {k} is used nowhere")
    bad += 1

# Keys gone since the last release, each named in the changelog or here with why it is no loss. Emptied at each
# release (a release's changelog says what went); a key here is one whose words still reach the reader.
GONE = {
}


def git(*args):
    return subprocess.run(["git", "-C", str(top), *args], capture_output=True, text=True)


def last_release():
    """The newest vX.Y.Z tag here, else fetched (a shallow CI checkout has none): its en.json, or None."""
    semver = lambda tags: sorted((t for t in tags if re.fullmatch(r"v\d+\.\d+\.\d+", t)), key=lambda t: [int(x) for x in t[1:].split(".")])
    tags = semver(git("tag", "--list", "v*").stdout.split())
    if not tags:
        remote = semver(re.findall(r"refs/tags/(v[\d.]+)$", git("ls-remote", "--tags", "origin").stdout, re.M))
        if remote and git("fetch", "--quiet", "--depth=1", "origin", "tag", remote[-1], "--no-tags").returncode == 0:
            tags = remote
    if not tags:
        return None, None
    shown = git("show", f"{tags[-1]}:app/locales/en.json")
    return (tags[-1], json.loads(shown.stdout)) if shown.returncode == 0 else (tags[-1], None)


def newest_sections():
    """The text of the changelog's newest release's Removed and Changed sections."""
    log = (top / "CHANGELOG.md").read_text()
    m = re.search(r"^## \[.*?(?=^## \[|\Z)", log, re.S | re.M)
    newest = m.group(0) if m else ""
    return "\n".join(re.findall(r"^### (?:Removed|Changed)\n(.*?)(?=^### |\Z)", newest, re.S | re.M))


tag, was = last_release()
if was is None:
    print(f"check-locales: no release's en.json to hold the changelog to ({tag or 'no vX.Y.Z tag'}): skipped")
else:
    told = newest_sections()
    words = lambda v: re.sub(r"\s+", " ", re.sub(r"%\{\w+\}", "", v)).strip().lower()
    for k in sorted(set(was) - set(en)):
        if k.endswith(".one") and k[: -len(".one")] in en:
            continue
        if k in GONE:
            continue
        w = words(was[k])
        if k in told or (len(w) >= 8 and w in told.lower()):
            continue
        print(f"en.json: {k} (\"{was[k]}\") was said at {tag} and is gone: name it in CHANGELOG.md's newest Removed or Changed section, or in GONE with why it is no loss")
        bad += 1
    for k in sorted(GONE):
        if k in en:
            print(f"check-locales: GONE lists {k}, which en.json still has: delete it there")
            bad += 1
        elif k not in was:
            print(f"check-locales: GONE lists {k}, which {tag} did not have: delete it there")
            bad += 1
print(f"locales: {len(en)} keys, {said} of them phrases Rust says, {families} by {len(FAMILIES)} listed families: " + ("ok" if not bad else f"{bad} problems"))
sys.exit(1 if bad else 0)
