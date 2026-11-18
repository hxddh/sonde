#!/usr/bin/env bash
# scripts/changelog-section.sh against a changelog of its own: each version's
# English notes, and its Chinese ones where the section has them.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
t="$(mktemp -d)"
trap 'rm -rf "$t"' EXIT
cat >"$t/CHANGELOG.md" <<'MD'
# Changelog

## [2.0.0] - 2026-11-18

### Added

- A thing.

### 中文

### 新增

- 一件事。

## [1.0.0] - 2026-11-01

### Fixed

- Another.
MD
failed=0
check() {
  local want="$1" got
  shift
  got="$(CHANGELOG="$t/CHANGELOG.md" sh "$here/changelog-section.sh" "$@")"
  if [ "$got" != "$want" ]; then
    printf 'changelog-section.sh %s:\n--- want\n%s\n--- got\n%s\n' "$*" "$want" "$got"
    failed=1
  fi
}
check $'\n### Added\n\n- A thing.' 2.0.0
check $'\n### 新增\n\n- 一件事。' 2.0.0 zh
check $'\n### Fixed\n\n- Another.' 1.0.0
# no Chinese written: nothing, and the updater shows the English
check '' 1.0.0 zh
if [ "$failed" = 0 ]; then echo ok; else echo failed; exit 1; fi
