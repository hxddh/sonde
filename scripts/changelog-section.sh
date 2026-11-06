#!/usr/bin/env sh
# Prints the CHANGELOG.md section of version $1.
set -eu
awk -v v="$1" '
  /^## \[/ { p = index($0, "[" v "]") > 0; next }
  p { print }
' "$(dirname "$0")/../CHANGELOG.md"
