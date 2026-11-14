#!/usr/bin/env sh
# Screenshots of the app on a real window: tests/shots.test.tsx on this
# machine's desktop target (GTK on Linux, under a virtual display when there
# is none; AppKit on macOS; Win32 on Windows). Fresh config and data; PNGs in
# app/shots (or $SONDE_SHOTS). SHOTS_FILTER="shots wide" or "part 1": those
# variants only (CI runs them side by side).
set -eu
cd "$(dirname "$0")/../app"
tmp="$(mktemp -d)"
cleanup() {
  [ -n "${wm:-}" ] && kill "$wm" 2>/dev/null || true
  [ -n "${xvfb:-}" ] && kill "$xvfb" 2>/dev/null || true
  # SONDE_SHOTS_KEEP=DIR: the tour's sessions kept (tests/fixtures/make.py makes the fixtures of them)
  if [ -n "${SONDE_SHOTS_KEEP:-}" ]; then mkdir -p "$SONDE_SHOTS_KEEP" && cp -R "$tmp/data/sessions/." "$SONDE_SHOTS_KEEP/" || true; fi
  rm -rf "$tmp"
  # (the file the tour was told this machine by goes back to its committed form)
  printf '{ "max": "" }\n' > tests/shots.env.json
}
trap cleanup EXIT
export SONDE_CONFIG_DIR="$tmp/config" SONDE_DATA_DIR="$tmp/data"
# committed sessions, for the screens that draw measured figures (times,
# latencies, a trial's run over its clean run, two runs compared): the same
# picture each time, where the tour's own runs would measure anew
mkdir -p "$SONDE_DATA_DIR/sessions"
cp -R tests/fixtures/sessions/. "$SONDE_DATA_DIR/sessions/"
unset AWS_PROFILE AWS_SESSION_TOKEN AWS_ROLE_ARN AWS_WEB_IDENTITY_TOKEN_FILE
export AWS_ACCESS_KEY_ID=AKIDSONDEUITEST AWS_SECRET_ACCESS_KEY=sonde-ui-test-secret \
  AWS_CONFIG_FILE="$tmp/aws-config" AWS_SHARED_CREDENTIALS_FILE="$tmp/aws-credentials" \
  AWS_EC2_METADATA_DISABLED=true
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}" CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
case "$(uname -s)" in
  Darwin)
    target=macos
    # the window must fit the screen (a CI runner's is 1024×768, a menu bar above)
    if [ -z "${SONDE_SHOT_MAX:-}" ]; then
      res="$(system_profiler SPDisplaysDataType 2>/dev/null | awk '/Resolution/ {print $2"x"$4; exit}')"
      w="${res%x*}"; h="${res#*x}"
      case "$w$h" in *[!0-9]* | "") ;; *) export SONDE_SHOT_MAX="$w"x"$((h - 80))" ;; esac
    fi
    echo "windows at most ${SONDE_SHOT_MAX:-as large as asked}" ;;
  MINGW* | MSYS* | CYGWIN*) target=windows ;;
  *) target=linux ;;
esac
if [ "$target" = linux ] && [ -z "${DISPLAY:-}" ]; then
  export DISPLAY=:99
  Xvfb "$DISPLAY" -screen 0 1600x1000x24 >/dev/null 2>&1 &
  xvfb=$!
  sleep 1
  # a window manager, so windows take the size they ask for
  if command -v openbox >/dev/null; then openbox >/dev/null 2>&1 & wm=$!; fi
  sleep 1
fi
# what the tour is told of this machine: a file, as its VM has no environment
printf '{ "max": "%s" }\n' "${SONDE_SHOT_MAX:-}" > tests/shots.env.json
mkdir -p shots
nsh test tests/shots.test.tsx --target "$target" ${SHOTS_FILTER:+--filter "$SHOTS_FILTER"}
if [ -n "${SONDE_SHOTS:-}" ]; then mkdir -p "$SONDE_SHOTS" && cp shots/*.png "$SONDE_SHOTS/"; fi
ls "${SONDE_SHOTS:-shots}"
# SONDE_README=1: the README's pictures (docs/images) from this tour, so they are of this version's screens
if [ -n "${SONDE_README:-}" ]; then
  from="${SONDE_SHOTS:-shots}"
  to=../docs/images
  while read -r shot name; do
    [ -f "$from/$shot.png" ] && cp "$from/$shot.png" "$to/$name.png" && echo "docs/images/$name.png <- $shot"
  done <<'MAP'
wide-02b-ran run
wide-02e-trial-plan trial-plan
wide-02g-trial-done trial
wide-02j-trial-time trial-over-clean
wide-03-session session
min-03-session session-min
zh-03-session session-zh
wide-03b-operation operation
wide-03d-filtered finding-advice
wide-03f-slice slice
wide-03g-time time
dark-03g-time time-dark
wide-03i-distribution spread
wide-03l-flows flows
dark-03l-flows flows-dark
wide-03m-latency latency
wide-03h-runs runs
wide-04b-objects store
wide-04d-uploads uploads
wide-05-activity activity
wide-05c-endpoints endpoints
wide-06-palette palette
MAP
fi
