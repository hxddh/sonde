#!/usr/bin/env python3
"""The sessions the screenshot tour shows where it draws measured figures.

    python3 app/tests/fixtures/make.py SESSIONS_DIR

SESSIONS_DIR is a tour's own sessions folder (`$SONDE_DATA_DIR/sessions` of a
`scripts/shots.sh` run, kept with `SONDE_SHOTS_KEEP=DIR`): its first run of the
tour's program (the config read ten times), the run after the fix, and that
run's slowdown trial. They are written here under `sessions/`, as one program
(`./nightly-report.sh`) run on two mornings, with the trial's verdict in the
folder's index: what scripts/shots.sh copies into the tour's data folder, so
those screens are the same from run to run.
"""
from __future__ import annotations

import gzip
import json
import re
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent / "sessions"
COMMAND = "./nightly-report.sh"
# the two mornings the runs are put on
MORNINGS = [datetime(2026, 11, 2, 7, 14, 5, tzinfo=timezone.utc), datetime(2026, 11, 3, 7, 14, 5, tzinfo=timezone.utc)]
STAMP = re.compile(r'"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z)"')


def parse(s: str) -> datetime:
    s = s.rstrip("Z")
    whole, _, frac = s.partition(".")
    return datetime.fromisoformat(whole).replace(tzinfo=timezone.utc) + timedelta(microseconds=int((frac + "000000")[:6]) if frac else 0)


def text(path: Path) -> list[str]:
    return gzip.decompress(path.read_bytes()).decode().splitlines() if path.read_bytes()[:2] == b"\x1f\x8b" else path.read_text().splitlines()


def moved(lines: list[str], by: timedelta, header: dict) -> list[str]:
    """Every time in the session moved by `by`, the header made the fixture's; in the session format 2
    (docs/session-format.md: a record's S3 call is `kind`, `operation` in format 1)."""
    header = {k: v for k, v in header.items() if k != "sonde"}
    header["version"] = 2
    out = [json.dumps(header, separators=(",", ":"))]
    for line in lines[1:]:
        line = STAMP.sub(lambda m: '"' + (parse(m.group(1)) + by).strftime("%Y-%m-%dT%H:%M:%S.%fZ") + '"', line)
        record = json.loads(line)
        if "operation" in record and "log" not in record:
            record = {("kind" if k == "operation" else k): v for k, v in record.items()}
            line = json.dumps(record, separators=(",", ":"), ensure_ascii=False)
        out.append(line)
    return out


def write(path: Path, lines: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    # finished sessions are kept compressed; mtime 0, so the bytes are the same each time
    path.write_bytes(gzip.compress(("\n".join(lines) + "\n").encode(), mtime=0))


def main(src: Path) -> None:
    runs = sorted(p for p in src.glob("*-curl.sonde"))
    if len(runs) < 2:
        sys.exit(f"{src}: no two runs of the tour's program")
    first, fixed = runs[0], runs[1]
    names = []
    by_run = {}
    for run, morning in zip((first, fixed), MORNINGS):
        lines = text(run)
        h = json.loads(lines[0])
        by = morning - parse(h["created"])
        by_run[run.name] = by
        name = morning.strftime("%Y%m%d-%H%M%S") + "-nightly-report.sonde"
        names.append(name)
        h.update(created=morning.strftime("%Y-%m-%dT%H:%M:%S.%fZ"), command=COMMAND, cwd="/home/me/report")
        write(HERE / name, moved(lines, by, h))
    # the fixed run's slowdown trial, and its verdict as the folder's index keeps it
    trial = src / "trials" / f"{fixed.stem}.slowdown.sonde"
    lines = text(trial)
    h = json.loads(lines[0])
    by = by_run[fixed.name]
    h.update(created=(parse(h["created"]) + by).strftime("%Y-%m-%dT%H:%M:%S.%fZ"), command=COMMAND, cwd="/home/me/report",
             trialOf=names[1])
    trial_name = f"{Path(names[1]).stem}.slowdown.sonde"
    write(HERE / "trials" / trial_name, moved(lines, by, h))
    verdict = None
    for line in (src / ".index.jsonl").read_text().splitlines():
        e = json.loads(line)
        if e["name"] == fixed.name and e.get("set") == "trials":
            verdict = [t for t in e["value"] if t["scenario"] == "slowdown"]
    if not verdict:
        sys.exit(f"{src}: no slowdown verdict for {fixed.name} in its index")
    for t in verdict:
        t["session"] = trial_name
        if isinstance(t.get("where"), dict) and "operation" in t["where"]:
            t["where"] = {("kind" if k == "operation" else k): v for k, v in t["where"].items()}
        for k, v in list(t.items()):
            if isinstance(v, str) and STAMP.fullmatch(f'"{v}"'):
                t[k] = (parse(v) + by).strftime("%Y-%m-%dT%H:%M:%S.%fZ")
    index = [{"name": n, "set": "exit", "value": 0} for n in names] + [{"name": names[1], "set": "trials", "value": verdict}]
    (HERE / ".index.jsonl").write_text("".join(json.dumps(x, separators=(",", ":")) + "\n" for x in index))
    for p in sorted(HERE.rglob("*")):
        if p.is_file():
            print(p.relative_to(HERE.parent), p.stat().st_size)


if __name__ == "__main__":
    main(Path(sys.argv[1]))
