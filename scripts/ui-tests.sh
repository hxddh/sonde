#!/usr/bin/env sh
# UI tests, headless: the desktop form (nsh test) and the terminal form
# (nsh test --target tui, an in-memory terminal). Each run uses fresh
# config and data directories. `ui-tests.sh perf` runs the performance test
# alone (100,000 rows loaded and scrolled; CI's nightly run).
set -eu
cd "$(dirname "$0")/../app"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export SONDE_CONFIG_DIR="$tmp/config" SONDE_DATA_DIR="$tmp/data"
# the same credential chain everywhere: dummy keys from the environment, no
# profiles, no instance metadata (a CI runner may answer IMDS)
unset AWS_PROFILE AWS_SESSION_TOKEN AWS_ROLE_ARN AWS_WEB_IDENTITY_TOKEN_FILE \
  AWS_CONTAINER_CREDENTIALS_FULL_URI AWS_CONTAINER_CREDENTIALS_RELATIVE_URI
export AWS_ACCESS_KEY_ID=AKIDSONDEUITEST AWS_SECRET_ACCESS_KEY=sonde-ui-test-secret \
  AWS_CONFIG_FILE="$tmp/aws-config" AWS_SHARED_CREDENTIALS_FILE="$tmp/aws-credentials" \
  AWS_EC2_METADATA_DISABLED=true
# the generated runner workspace keeps its own profiles: no debug info keeps it small
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
if [ "${1:-}" = perf ]; then
  nsh test tests/perf.test.tsx --target tui --trace
  exit
fi
nsh test tests/desktop.test.tsx
nsh test tests/screens.test.tsx
nsh test tests/run.test.tsx
nsh test tests/terminal.test.tsx --target tui
(cd ../viewer && nsh test tests/viewer.test.tsx)
