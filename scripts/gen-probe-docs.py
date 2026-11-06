#!/usr/bin/env python3
"""Writes docs/probes.md from `sonde probe list --format json` (the catalog
is the source of truth: each probe's description and AWS reference)."""
import json, subprocess, sys
sonde = sys.argv[1] if len(sys.argv) > 1 else "target/debug/sonde"
catalog = json.loads(subprocess.check_output([sonde, "probe", "list", "--format", "json"]))
out = ["# Compatibility probes", "",
       "Generated from the probe catalog (`sonde probe list --format json`) by `scripts/gen-probe-docs.py`.", "",
       "Each probe runs in a scratch prefix `sonde-probe/<run>/<probe>/` of the bucket you give (`--bucket`), and, with",
       "`--temp-buckets`, in temporary buckets of its own. Everything it wrote is removed afterwards: every version,",
       "delete marker and incomplete upload under the prefix, and the temporary buckets (Object Lock versions with",
       "`x-amz-bypass-governance-retention`).", "",
       "Verdicts: **pass** (as AWS), **fail** (a guarantee broke: data written that should not be, wrong data read),",
       "**differs** (works, differently from AWS), **unsupported** (not implemented), **error** (could not run),",
       "**skipped** (needs a temporary bucket or real-time waits that were not allowed).", "",
       "The judge of every probe is a pure function of the recorded responses, unit-tested on fixtures recorded from",
       "Ceph RGW and RustFS and on AWS-reference fixtures (`crates/probes/fixtures`).", ""]
cats = {}
for m in catalog: cats.setdefault(m["category"], []).append(m)
out += ["| Probe | Category | Title |", "|---|---|---|"]
out += [f"| [`{m['id']}`](#{m['id'].replace('.', '')}) | {m['category']} | {m['title']} |" for m in catalog]
for cat, ms in cats.items():
    out += ["", f"## {cat}"]
    for m in ms:
        needs = f" Needs: {', '.join(m['needs'])}." if m["needs"] else ""
        out += ["", f"### `{m['id']}`", "", f"**{m['title']}.** {m['description']}{needs}", "", f"**AWS behavior:** {m['awsBehavior']}"]
open("docs/probes.md", "w").write("\n".join(out) + "\n")
print(f"{len(catalog)} probes")
