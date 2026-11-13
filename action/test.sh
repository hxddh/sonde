#!/usr/bin/env bash
# The action's steps (sonde-action.sh) against a stand-in gh and sonde: which
# baseline is fetched, which comment is edited, what each step outputs.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
t="$(mktemp -d)"
trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin"

cat >"$t/bin/gh" <<'EOF'
#!/usr/bin/env bash
echo "gh $*" >>"$FAKE/gh.log"
case "$1 $2" in
  "run list") for id in ${FAKE_RUNS:-}; do echo "$id"; done ;;
  "run download")
    [ "$3" = "${FAKE_HAS:-}" ] || exit 1
    while [ $# -gt 0 ]; do [ "$1" = -D ] && dir="$2"; shift; done
    mkdir -p "$dir" && touch "$dir/run.sonde" ;;
  "api --paginate") [ -z "${FAKE_COMMENT:-}" ] || echo "$FAKE_COMMENT" ;;
  "api -X")
    for a in "$@"; do case "$a" in body=*) printf '%s' "${a#body=}" >"$FAKE/body" ;; esac; done
    echo "https://github.test/$4" ;;
esac
EOF
cat >"$t/bin/sonde" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$FAKE/sonde.$1"
case "$1" in
  run) while [ $# -gt 0 ]; do [ "$1" = -o ] && touch "$2"; shift; done; exit "${FAKE_RUN_EXIT:-0}" ;;
  check) echo "### sonde: within budget ✅"; exit "${FAKE_CHECK:-0}" ;;
  share) while [ $# -gt 0 ]; do [ "$1" = -o ] && echo page >"$2"; shift; done ;;
esac
EOF
chmod +x "$t/bin/gh" "$t/bin/sonde"

export PATH="$t/bin:$PATH" FAKE="$t" RUNNER_TEMP="$t/runner" GITHUB_OUTPUT="$t/out"
export GITHUB_REPOSITORY=o/r GITHUB_WORKFLOW_REF="o/r/.github/workflows/ci.yml@refs/pull/7/merge"
export GITHUB_STEP_SUMMARY="$t/summary"
step() { : >"$GITHUB_OUTPUT"; bash "$here/sonde-action.sh" "$@"; }
output() { sed -n "s/^$1=//p" "$GITHUB_OUTPUT"; }
failed=0
is() {
  if [ "$2" != "$3" ]; then
    echo "FAIL $1: got [$2], want [$3]" >&2
    failed=1
  else echo "ok   $1"; fi
}

# a pull request: the newest of the base branch's runs that kept the artifact
export GITHUB_BASE_REF=main
FAKE_RUNS="11 22" FAKE_HAS=22 step baseline >"$t/said"
is "baseline from the base branch's run that has it" "$(output baseline)" "$t/runner/sonde/baseline/run.sonde"
is "it says which run" "$(grep -c 'actions/runs/22' "$t/said")" 1
is "of this workflow, on the base branch" "$(grep -c -- '--workflow ci.yml --branch main' "$t/gh.log")" 1
FAKE_RUNS="11" step baseline >"$t/said"
is "none kept: auto" "$(output baseline)" auto
is "none kept: a warning" "$(grep -c '^::warning title=sonde::no baseline' "$t/said")" 1
SONDE_BASELINE=none step baseline
is "none asked: none" "$(output baseline)" ""
SONDE_BASELINE=https://x/b.sonde step baseline
is "a URL is passed on" "$(output baseline)" https://x/b.sonde
# a push: this branch's earlier run, never its own
unset GITHUB_BASE_REF
: >"$t/gh.log"
GITHUB_REF_NAME=main GITHUB_RUN_ID=11 FAKE_RUNS="11 22" FAKE_HAS=22 step baseline >/dev/null
is "a push takes the run before it" "$(output baseline)" "$t/runner/sonde/baseline/run.sonde"
is "and never downloads its own" "$(grep -c 'download 11' "$t/gh.log")" 0

# the program's command as typed; its status kept for the verdict
SONDE_RUN="python3 -c 'print(1)'" SONDE_RUN_ARGS="--url https://s3.test --callers" SONDE_NAME=etl FAKE_RUN_EXIT=3 step record
is "record keeps the program's status" "$(output exit)" 3
is "record writes a session" "$(output session)" "$t/runner/sonde/run.sonde"
is "the command's words as typed" "$(tail -n 3 "$t/sonde.run" | paste -sd'|')" "python3|-c|print(1)"
is "run's flags and the name before --" "$(grep -c -e '^--callers$' -e '^etl$' "$t/sonde.run")" 2

# check: passed or not; anything else is the step's failure
export SONDE_SESSION="$t/runner/sonde/run.sonde" SONDE_BASELINE=auto
SONDE_CHECK_ARGS="--tolerance 5" step check >/dev/null
is "a check that passed" "$(output passed)" true
is "check's flags" "$(paste -sd' ' "$t/sonde.check")" "check $SONDE_SESSION --baseline auto --tolerance 5 -f github"
FAKE_CHECK=1 step check >/dev/null
is "a check that failed" "$(output passed)" false
if FAKE_CHECK=64 step check >/dev/null; then is "a check that could not run fails the step" 0 1; fi

# the report: against a baseline file, never `auto`
step report >/dev/null
is "a report" "$(output file)" "$t/runner/sonde/report/sonde-report.html"
is "auto is not passed to share" "$(grep -c baseline "$t/sonde.share")" 0

# one comment, found by its marker and edited; else written
export SONDE_PR=7 SONDE_ARTIFACT=etl SONDE_REPORT_URL=https://github.test/artifact/1
step comment
is "no comment yet: one is written" "$(output url)" "https://github.test/repos/o/r/issues/7/comments"
is "the comment starts with its marker" "$(head -n 1 "$t/body")" "<!-- sonde: etl -->"
is "the comment links the report" "$(grep -c -- '--report-url' "$t/sonde.check")" 1
is "no status: none passed" "$(grep -c -- '--program-exit' "$t/sonde.check")" 0
is "the summary has the comment" "$(grep -c 'within budget' "$t/summary")" 1
SONDE_EXIT=1 step comment
is "the comment says the program's status" "$(paste -sd' ' "$t/sonde.check" | grep -c -- '--program-exit 1')" 1
FAKE_COMMENT=99 step comment
is "a comment there: it is edited" "$(output url)" "https://github.test/repos/o/r/issues/comments/99"
SONDE_COMMENT=false FAKE_COMMENT=99 step comment
is "comment: false writes none" "$(output url)" ""

# the verdict: the program's status first, then the check's
SONDE_EXIT=0 SONDE_PASSED=true step verdict
if SONDE_EXIT=3 step verdict >/dev/null; then is "a program that failed fails it" 0 1; fi
if SONDE_EXIT=0 SONDE_PASSED=false step verdict >/dev/null; then is "a check that failed fails it" 0 1; fi
SONDE_EXIT=0 SONDE_PASSED=false SONDE_FAIL=false step verdict >/dev/null
echo "ok   fail: false lets a failed check pass"

exit "$failed"
