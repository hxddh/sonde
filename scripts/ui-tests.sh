#!/usr/bin/env sh
# UI tests, headless: the desktop form (nsh test) and the terminal form
# (nsh test --target tui, an in-memory terminal). Each run uses fresh
# config and data directories.
set -eu
cd "$(dirname "$0")/../app"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export SONDE_CONFIG_DIR="$tmp/config" SONDE_DATA_DIR="$tmp/data"
# the generated runner workspace keeps its own profiles: no debug info keeps it small
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"
nsh test tests/desktop.test.tsx
nsh test tests/screens.test.tsx
nsh test tests/terminal.test.tsx --target tui
(cd ../plugins/delta-lens && nsh test)
