#!/usr/bin/env sh
# Screenshots of the app on a real window: tests/shots.test.tsx on the
# Linux (GTK) target, under a virtual display when there is none. Fresh
# config and data; PNGs in app/shots (or $SONDE_SHOTS).
set -eu
cd "$(dirname "$0")/../app"
tmp="$(mktemp -d)"
cleanup() {
  [ -n "${wm:-}" ] && kill "$wm" 2>/dev/null || true
  [ -n "${xvfb:-}" ] && kill "$xvfb" 2>/dev/null || true
  rm -rf "$tmp"
}
trap cleanup EXIT
export SONDE_CONFIG_DIR="$tmp/config" SONDE_DATA_DIR="$tmp/data"
unset AWS_PROFILE AWS_SESSION_TOKEN AWS_ROLE_ARN AWS_WEB_IDENTITY_TOKEN_FILE
export AWS_ACCESS_KEY_ID=AKIDSONDEUITEST AWS_SECRET_ACCESS_KEY=sonde-ui-test-secret \
  AWS_CONFIG_FILE="$tmp/aws-config" AWS_SHARED_CREDENTIALS_FILE="$tmp/aws-credentials" \
  AWS_EC2_METADATA_DISABLED=true
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}" CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
if [ -z "${DISPLAY:-}" ]; then
  export DISPLAY=:99
  Xvfb "$DISPLAY" -screen 0 1600x1000x24 >/dev/null 2>&1 &
  xvfb=$!
  sleep 1
  # a window manager, so windows take the size they ask for
  if command -v openbox >/dev/null; then openbox >/dev/null 2>&1 & wm=$!; fi
  sleep 1
fi
mkdir -p "${SONDE_SHOTS:-shots}"
nsh test tests/shots.test.tsx --target linux
ls "${SONDE_SHOTS:-shots}"
