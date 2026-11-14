#!/usr/bin/env bash
# scripts/release-wait.sh against a stand-in gh that answers, call by call,
# with the CI runs of a commit: what the release waits for, takes or refuses.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
t="$(mktemp -d)"
trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin"
# each call answers with the next line of $FAKE/answers (the last again when
# they run out): runs as "id status conclusion", ";" between runs; "-" none;
# "!" the API failing
cat >"$t/bin/gh" <<'EOF'
#!/usr/bin/env bash
n=$(cat "$FAKE/n" 2>/dev/null || echo 0)
echo $((n + 1)) >"$FAKE/n"
line=$(sed -n "$((n + 1))p" "$FAKE/answers")
[ -n "$line" ] || line=$(tail -n 1 "$FAKE/answers")
case "$line" in
  "!") exit 1 ;;
  -) exit 0 ;;
  *) tr ';' '\n' <<<"$line" ;;
esac
EOF
chmod +x "$t/bin/gh"

failed=0
# waits, with the answers given, for at most a second of polls; "exit, calls, run"
waits() {
  rm -f "$t/n" "$t/out"
  printf '%s\n' "$@" >"$t/answers"
  local status=0
  FAKE="$t" PATH="$t/bin:$PATH" REPO=o/r SHA=abc RELEASE_POLL_S=0 RELEASE_START_MIN="${START:-0}" \
    RELEASE_WAIT_MIN="${WAIT:-120}" GITHUB_OUTPUT="$t/out" GITHUB_STEP_SUMMARY=/dev/null \
    timeout 10 bash "$here/release-wait.sh" >"$t/said" 2>&1 || status=$?
  echo "$status $(cat "$t/n") $(sed -n 's/^run=//p' "$t/out" 2>/dev/null)"
}
is() {
  if [ "$2" != "$3" ]; then
    echo "FAIL $1: got [$2], want [$3]" >&2
    sed 's/^/     /' "$t/said" >&2
    failed=1
  else echo "ok   $1"; fi
}

is "a green run is taken at once" "$(waits "5 completed success")" "0 1 5"
is "a running one is waited for, through an API that fails once" \
  "$(waits "5 queued null" "5 in_progress null" "!" "5 completed success")" "0 4 5"
is "a green run among failed ones is taken" "$(waits "5 completed failure;4 completed success")" "0 1 4"
is "every run finished, none green: refused" "$(waits "5 completed failure;4 completed cancelled")" "1 1 "
is "it says which" "$(grep -c 'actions/runs/5 failure' "$t/said")" 1
is "no run of the commit: refused" "$(waits -)" "1 1 "
is "it says how to make one" "$(grep -c 'run CI on it by hand' "$t/said")" 1
is "a run that starts late is waited for" "$(START=1 waits - - "6 completed success")" "0 3 6"
is "a run past the bound: refused" "$(WAIT=0 waits "5 in_progress null")" "1 1 "

exit "$failed"
