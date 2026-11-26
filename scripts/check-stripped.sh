#!/usr/bin/env bash
# The ELF programs and libraries under each path given carry no symbol table
# and no debug sections: a packaged app is as small as the terminal binary
# (whose `strip = "symbols"` is Cargo.toml's; the app's build is Native
# Shell's workspace, held to it by CARGO_PROFILE_RELEASE_STRIP in build.yml).
# Fails naming each file that does, and when a path holds no ELF at all.
set -euo pipefail

[ $# -gt 0 ] || { echo "usage: $0 PATH..." >&2; exit 64; }
seen=0
failed=0
while IFS= read -r -d '' f; do
  # (an ELF by its first four bytes: no `file` on a runner needed)
  [ "$(head -c 4 "$f" | od -An -c | tr -d ' ')" = '177ELF' ] || continue
  seen=$((seen + 1))
  sections="$(readelf -S -W "$f" 2>/dev/null | awk '{ for (i = 1; i <= NF; i++) if ($i ~ /^\.(symtab|strtab|debug_|zdebug_)/) print $i }' | sort -u | paste -sd' ' -)"
  if [ -n "$sections" ]; then
    echo "not stripped: $f ($sections)" >&2
    failed=1
  fi
done < <(find "$@" -type f -print0)
[ "$seen" -gt 0 ] || { echo "no ELF file under: $*" >&2; exit 1; }
[ "$failed" = 0 ] || exit 1
echo "stripped: $seen ELF file(s)"
