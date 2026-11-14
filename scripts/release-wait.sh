#!/usr/bin/env bash
# release.yml: waits for CI's run of the commit being released (ci.yml, on
# main: a push, the nightly or one by hand; never a pull request's) and
# writes its id as `run=` to $GITHUB_OUTPUT once it is green. Fails when every
# such run finished and none is green, when none started within
# RELEASE_START_MIN minutes, or after RELEASE_WAIT_MIN minutes.
#
#   REPO=o/r SHA=<commit> GH_TOKEN=... scripts/release-wait.sh
set -euo pipefail

: "${REPO:?}" "${SHA:?}"
wait_min="${RELEASE_WAIT_MIN:-120}"
start_min="${RELEASE_START_MIN:-15}"
every="${RELEASE_POLL_S:-60}"
url="https://github.com/$REPO/actions/workflows/ci.yml"
started=$SECONDS

while :; do
  # one line per run: id, status, conclusion
  # (a failed call is asked again, as a run not yet started is)
  runs="$(gh api "repos/$REPO/actions/workflows/ci.yml/runs?head_sha=$SHA&per_page=50" --jq \
    '.workflow_runs[] | select((.event == "push" or .event == "schedule" or .event == "workflow_dispatch") and .head_repository.full_name == env.REPO) | "\(.id) \(.status) \(.conclusion)"')" ||
    { echo "::warning::GitHub's API did not answer; asking again"; runs="?"; }
  green="$(awk '$2 == "completed" && $3 == "success" { print $1; exit }' <<<"$runs")"
  if [ -n "$green" ]; then
    echo "CI's run of $SHA is green: https://github.com/$REPO/actions/runs/$green"
    echo "run=$green" >>"${GITHUB_OUTPUT:-/dev/null}"
    echo "- built and checked by https://github.com/$REPO/actions/runs/$green" >>"${GITHUB_STEP_SUMMARY:-/dev/null}"
    exit 0
  fi
  waited=$(((SECONDS - started) / 60))
  if [ "$runs" = "?" ]; then
    :
  elif [ -z "$runs" ]; then
    if [ "$waited" -ge "$start_min" ]; then
      echo "::error title=No CI run::no CI run of $SHA on main after $start_min minutes: release a commit on main, or run CI on it by hand ($url)"
      exit 1
    fi
    echo "no CI run of $SHA yet ($waited min)"
  elif ! awk '$2 != "completed" { found = 1 } END { exit !found }' <<<"$runs"; then
    echo "::error title=CI failed::CI's run of $SHA finished and is not green; nothing is released (fix it, or re-run it and release again):"
    awk -v r="https://github.com/$REPO/actions/runs/" '{ print "  " r $1 " " $3 }' <<<"$runs"
    exit 1
  else
    echo "CI's run of $SHA is running ($waited min): $(awk '$2 != "completed" { print $1 }' <<<"$runs" | paste -sd' ')"
  fi
  if [ "$waited" -ge "$wait_min" ]; then
    echo "::error title=CI too slow::CI's run of $SHA did not finish within $wait_min minutes; release again once it is green"
    exit 1
  fi
  sleep "$every"
done
