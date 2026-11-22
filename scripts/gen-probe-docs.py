#!/usr/bin/env python3
"""Writes docs/probes.md from the probe catalog (`cargo run -q -p probes --example catalog`,
each probe's title and AWS reference: the source of truth).
`--check` writes nothing and fails when docs/probes.md is not what it would write.

    python3 scripts/gen-probe-docs.py [--check]"""
import difflib, json, subprocess, sys
check = "--check" in sys.argv[1:]
catalog = json.loads(subprocess.check_output(["cargo", "run", "-q", "-p", "probes", "--example", "catalog"]))
needs = {"tempBucket": "a bucket of its own (skipped)", "https": "HTTPS", "slow": "waits in real time"}
cell = lambda s: s.replace("|", "\\|").replace("\n", " ")
out = ["# Compatibility probes", "",
       "What `sonde endpoints test ENDPOINT --probes BUCKET` runs ([cli.md](cli.md#buckets)). Generated from the probe",
       "catalog by `scripts/gen-probe-docs.py`.", "",
       "| Probe | Category | Checks | AWS behavior | Needs |", "|---|---|---|---|---|"]
out += [f"| `{m['id']}` | {m['category']} | {cell(m['title'])} | {cell(m['awsBehavior'])} | {', '.join(needs.get(n, n) for n in m['needs'])} |"
        for m in catalog]
text = "\n".join(out) + "\n"
if check:
    with open("docs/probes.md") as f:
        now = f.read()
    if now != text:
        sys.stdout.writelines(difflib.unified_diff(now.splitlines(True), text.splitlines(True), "docs/probes.md", "generated", n=1))
        print(f"probes: docs/probes.md is not what the catalog of {len(catalog)} makes (run scripts/gen-probe-docs.py, commit it): failed")
        sys.exit(1)
    print(f"probes: docs/probes.md is what the catalog of {len(catalog)} makes: ok")
else:
    with open("docs/probes.md", "w") as f:
        f.write(text)
    print(f"{len(catalog)} probes")
