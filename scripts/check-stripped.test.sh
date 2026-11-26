#!/usr/bin/env bash
# scripts/check-stripped.sh against two builds of one small program: as
# rustc leaves it (symbols, debug sections) and stripped.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ "$(uname -s)" = Linux ] || { echo "ok   (ELF only: skipped on $(uname -s))"; exit 0; }
t="$(mktemp -d)"
trap 'rm -rf "$t"' EXIT
failed=0
is() {
  if [ "$2" != "$3" ]; then
    echo "FAIL $1: got [$2], want [$3]" >&2
    failed=1
  else echo "ok   $1"; fi
}

mkdir -p "$t/plain" "$t/stripped" "$t/none"
echo 'fn main() { println!("sonde"); }' >"$t/main.rs"
rustc -g -o "$t/plain/app" "$t/main.rs"
rustc -C strip=symbols -o "$t/stripped/app" "$t/main.rs"
echo text >"$t/stripped/readme.txt"
echo text >"$t/none/readme.txt"

status=0 && bash "$here/check-stripped.sh" "$t/plain" >/dev/null 2>"$t/said" || status=$?
is "a build with its symbols fails" "$status" 1
is "it names the file and what it carries" "$(grep -c "not stripped: $t/plain/app (.*\.symtab" "$t/said")" 1
status=0 && bash "$here/check-stripped.sh" "$t/stripped" >"$t/said" 2>&1 || status=$?
is "a stripped build passes, other files beside it" "$status $(cat "$t/said")" "0 stripped: 1 ELF file(s)"
status=0 && bash "$here/check-stripped.sh" "$t/stripped" "$t/plain" >/dev/null 2>&1 || status=$?
is "one file with symbols among several fails" "$status" 1
status=0 && bash "$here/check-stripped.sh" "$t/none" >/dev/null 2>&1 || status=$?
is "nothing to check is a failure, not a pass" "$status" 1

exit "$failed"
