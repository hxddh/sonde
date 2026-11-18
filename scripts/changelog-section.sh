#!/usr/bin/env sh
# Prints the CHANGELOG.md section of version $1: its English notes, or with
# $2 = zh its Chinese ones, the part under the section's `### 中文` heading
# (nothing when the section has none: the updater then shows the English).
set -eu
awk -v v="$1" -v lang="${2:-en}" '
  /^## \[/ { p = index($0, "[" v "]") > 0; zh = 0; next }
  p && /^### 中文[[:space:]]*$/ { zh = 1; next }
  p && (lang == "zh") == zh { print }
' "${CHANGELOG:-$(dirname "$0")/../CHANGELOG.md}"
