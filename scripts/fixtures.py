#!/usr/bin/env python3
"""Rewrites every session fixture in the session format this sonde writes.

    python3 scripts/fixtures.py          # rewrite them
    python3 scripts/fixtures.py --check  # exit 1 when one is not current

sonde reads its own format only (docs/dev/session-format.md), so when the format
changes, the fixtures change with it: set RENAMES to what the new version
renamed, run this, and commit the fixtures. A compressed fixture stays
compressed (mtime 0: the same bytes each time); a plain one stays plain.
"""
from __future__ import annotations

import gzip
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = [ROOT / "crates/proxy/tests/fixtures", ROOT / "app/tests/fixtures/sessions"]
# a record's fields the current version renamed: {old: new}
RENAMES = {"operation": "kind"}


def version() -> int:
    src = (ROOT / "crates/proxy/src/session.rs").read_text()
    return int(re.search(r"pub const VERSION: u32 = (\d+);", src).group(1))


def current(lines: list[str], want: int) -> list[str]:
    header = json.loads(lines[0])
    if header.get("format") != "sonde-session":
        raise SystemExit(f"not a session: {lines[0][:80]}")
    header["version"] = want
    out = [json.dumps(header, separators=(",", ":"), ensure_ascii=False)]
    for line in lines[1:]:
        if line.startswith('{"log":') or not line:
            out.append(line)
            continue
        record = json.loads(line)
        if any(k in record for k in RENAMES):
            record = {RENAMES.get(k, k): v for k, v in record.items()}
            line = json.dumps(record, separators=(",", ":"), ensure_ascii=False)
        out.append(line)
    return out


def main() -> None:
    check = "--check" in sys.argv[1:]
    want = version()
    stale = []
    for folder in FIXTURES:
        for path in sorted(folder.rglob("*.sonde")):
            raw = path.read_bytes()
            packed = raw[:2] == b"\x1f\x8b"
            text = gzip.decompress(raw).decode() if packed else raw.decode()
            lines = text.splitlines()
            fresh = "\n".join(current(lines, want)) + "\n"
            if fresh == text:
                continue
            stale.append(path.relative_to(ROOT))
            if not check:
                body = fresh.encode()
                path.write_bytes(gzip.compress(body, mtime=0) if packed else body)
    for p in stale:
        print(("not current: " if check else "rewrote ") + str(p))
    if check and stale:
        sys.exit(1)


if __name__ == "__main__":
    main()
