#!/usr/bin/env bash
# An output's value, read from a GITHUB_OUTPUT file as GitHub reads it:
# `name<<delimiter`, the value's lines, the delimiter (what sonde-action.sh
# writes), or `name=value` on a line. The last one written, when written twice.
#
#   read-output.sh NAME FILE
set -euo pipefail
[ $# -eq 2 ] || { echo "usage: $0 NAME FILE" >&2; exit 64; }
awk -v name="$1" '
  delim != "" {
    if ($0 == delim) { delim = ""; if (cur == name) { value = held; found = 1 } }
    else if (cur == name) held = held (n++ ? "\n" : "") $0
    next
  }
  {
    i = index($0, "<<"); e = index($0, "=")
    if (i && (!e || i < e)) { cur = substr($0, 1, i - 1); delim = substr($0, i + 2); held = ""; n = 0; next }
    if (e && substr($0, 1, e - 1) == name) { value = substr($0, e + 1); found = 1 }
  }
  END { if (found) print value }
' "$2"
