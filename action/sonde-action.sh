#!/usr/bin/env bash
# The steps of the sonde GitHub Action (action.yml), one per argument:
# install, record, baseline, check, report, comment, verdict. Each reads its
# inputs from SONDE_* variables and GitHub's own, and writes its outputs to
# $GITHUB_OUTPUT. docs/ci.md says what each does.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
work="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/sonde"
mkdir -p "$work"

out() { echo "$1=$2" >>"${GITHUB_OUTPUT:-/dev/null}"; }
note() { echo "::notice title=sonde::$*"; }
warn() { echo "::warning title=sonde::$*"; }
fail() {
  echo "::error title=sonde::$*"
  exit 1
}

# The release binary, its checksum checked and, where gh is signed in (it is
# with GH_TOKEN), its build provenance (install.sh).
install() {
  local version="${SONDE_VERSION:-}"
  if [ "$version" = path ]; then
    command -v sonde >/dev/null || fail "version: path, and no sonde on PATH"
    sonde --version
    return
  fi
  local ref="${GITHUB_ACTION_REF:-}"
  if [[ "$ref" =~ ^v[0-9]+(\.[0-9]+(\.[0-9]+(-[0-9A-Za-z.]+)?)?)?$ ]]; then
    # a release's tag (v19.0.1) is that release; a major's (v19) the newest of it
    [ -n "$version" ] || version="${ref#v}"
  else
    # a branch, a commit or a local checkout (`uses: ./`): its scripts are
    # of the version its Cargo.toml says, and run with a sonde of that
    # major, never with whatever release is the newest
    local own major
    own="$(sed -n 's/^version = "\([0-9][^"]*\)"$/\1/p' "$here/../Cargo.toml" 2>/dev/null | head -n 1)"
    [ -n "$own" ] || fail "no version in the action's Cargo.toml: set the version input (19, 19.0.0)"
    major="${own%%.*}"
    case "$version" in
      latest) fail "version: latest with the action at ${ref:-a local checkout} (sonde $own's scripts): latest may be of another major; set version: $major, or use the action at a release's tag (@v$major)" ;;
      "")
        version="$major"
        note "the action at ${ref:-a local checkout} is sonde $own's: installing the newest $major.x"
        ;;
    esac
  fi
  local dir="$work/bin"
  SONDE_VERSION="$version" SONDE_INSTALL_DIR="$dir" sh "$here/../install.sh" ||
    fail "sonde $version could not be installed (said above); a major with no release yet has nothing to install: set the version input, or use the action at a release's tag"
  echo "$dir" >>"${GITHUB_PATH:-/dev/null}"
}

# The program under `sonde run`, its session kept apart; its exit status is
# the verdict's, after the report and the comment.
record() {
  [ -n "${SONDE_RUN:-}" ] || fail "the run input is empty"
  local session="$work/run.sonde" args=() status=0
  eval "args=(${SONDE_RUN_ARGS:-})"
  [ -z "${SONDE_NAME:-}" ] || args+=(--name "$SONDE_NAME")
  eval "set -- $SONDE_RUN"
  rm -f "$session"
  sonde run -o "$session" ${args[@]+"${args[@]}"} -- "$@" || status=$?
  out exit "$status"
  [ -f "$session" ] || fail "sonde run wrote no session (exit $status)"
  out session "$session"
}

# The workflow file this run is of: `.github/workflows/ci.yml` of
# `owner/repo/.github/workflows/ci.yml@refs/heads/main`.
workflow_file() {
  local ref="${GITHUB_WORKFLOW_REF:-}"
  ref="${ref%@*}"
  echo "${ref##*/}"
}

# The session the latest earlier run of this workflow kept on the base
# branch (a pull request's) or on this branch (a push); `auto` (said as "no
# baseline yet" by every format of `check`) when there is none. Another input
# is passed on as it is.
baseline() {
  case "${SONDE_BASELINE:-auto}" in
    none | "") return ;;
    auto) ;;
    *)
      out baseline "$SONDE_BASELINE"
      return
      ;;
  esac
  local base="${GITHUB_BASE_REF:-}" artifact="${SONDE_ARTIFACT:-sonde}" event=push
  # a pull request against its base branch's pushes; any other run against
  # this branch's earlier runs of its own kind (a push's, a schedule's)
  if [ -z "$base" ]; then
    base="${GITHUB_REF_NAME:-}"
    event="${GITHUB_EVENT_NAME:-push}"
  fi
  [ -n "$base" ] || fail "no branch to take a baseline from (GITHUB_BASE_REF, GITHUB_REF_NAME)"
  local workflow ids id dir="$work/baseline"
  workflow="$(workflow_file)"
  ids="$(gh run list -R "$GITHUB_REPOSITORY" --workflow "$workflow" --branch "$base" \
    --event "$event" --status completed --limit 6 --json databaseId --jq '.[].databaseId' 2>/dev/null || true)"
  for id in $ids; do
    # (a push's own run is not its baseline)
    [ "$id" != "${GITHUB_RUN_ID:-}" ] || continue
    rm -rf "$dir"
    if gh run download "$id" -R "$GITHUB_REPOSITORY" -n "$artifact" -D "$dir" >/dev/null 2>&1; then
      local file
      file="$(find "$dir" -name '*.sonde' | head -n 1)"
      if [ -n "$file" ]; then
        note "baseline: the run of $base kept by ${GITHUB_SERVER_URL:-https://github.com}/$GITHUB_REPOSITORY/actions/runs/$id"
        out baseline "$file"
        return
      fi
    fi
  done
  warn "no baseline: no $event run of $workflow on $base kept the artifact $artifact (none yet, or it expired); this run is held to its budget alone"
  out baseline auto
}

# `sonde check`'s flags, as the step's inputs give them, in ARGS.
check_args() {
  ARGS=("$SONDE_SESSION")
  [ -z "${SONDE_BASELINE:-}" ] || ARGS+=(--baseline "$SONDE_BASELINE")
  [ -z "${SONDE_CONFIG:-}" ] || ARGS+=(--config "$SONDE_CONFIG")
  local more=()
  eval "more=(${SONDE_CHECK_ARGS:-})"
  ARGS+=(${more[@]+"${more[@]}"})
}

# Annotations on the run and the diff; whether it passed.
check() {
  local status=0
  check_args
  sonde check "${ARGS[@]}" -f github || status=$?
  case "$status" in
    0) out passed true ;;
    1) out passed false ;;
    *) fail "sonde check could not check the run (exit $status)" ;;
  esac
}

# The HTML report: the run against its baseline when there is one.
report() {
  local args=("$SONDE_SESSION") file="$work/report/sonde-report.html"
  mkdir -p "$(dirname "$file")"
  case "${SONDE_BASELINE:-}" in
    "" | auto) ;;
    *) args+=(--baseline "$SONDE_BASELINE") ;;
  esac
  [ "${SONDE_REDACT:-false}" != true ] || args+=(--redact)
  if sonde share --html "${args[@]}" -o "$file"; then
    out file "$file"
  else
    warn "no report: sonde share --html failed"
  fi
}

# The comment: on the job's summary, and one comment on the pull request,
# found by its marker and edited in place.
comment() {
  local md="$work/comment.md"
  check_args
  [ -z "${SONDE_REPORT_URL:-}" ] || ARGS+=(--report-url "$SONDE_REPORT_URL")
  [ -z "${SONDE_EXIT:-}" ] || ARGS+=(--program-exit "$SONDE_EXIT")
  sonde check "${ARGS[@]}" -f markdown >"$md" || [ $? -eq 1 ]
  cat "$md" >>"${GITHUB_STEP_SUMMARY:-/dev/null}"
  if [ -z "${SONDE_PR:-}" ] || [ "${SONDE_COMMENT:-true}" != true ]; then
    return
  fi
  local marker="<!-- sonde: ${SONDE_ARTIFACT:-sonde} -->" body id url
  body="$(printf '%s\n%s' "$marker" "$(cat "$md")")"
  id="$(SONDE_MARKER="$marker" gh api --paginate "repos/$GITHUB_REPOSITORY/issues/$SONDE_PR/comments" \
    --jq '.[] | select(.body | startswith(env.SONDE_MARKER)) | .id' 2>/dev/null | head -n 1 || true)"
  if [ -n "$id" ]; then
    url="$(gh api -X PATCH "repos/$GITHUB_REPOSITORY/issues/comments/$id" -f body="$body" --jq .html_url)" ||
      { warn "the comment could not be updated (the token needs pull-requests: write)"; return; }
  else
    url="$(gh api -X POST "repos/$GITHUB_REPOSITORY/issues/$SONDE_PR/comments" -f body="$body" --jq .html_url)" ||
      { warn "no comment: the token cannot write one (pull-requests: write; a fork's pull request has a read-only token)"; return; }
  fi
  out url "$url"
}

# The program's own failure first, then the check's.
verdict() {
  local status="${SONDE_EXIT:-0}"
  [ "$status" = 0 ] || fail "the program exited $status"
  if [ "${SONDE_PASSED:-true}" = false ] && [ "${SONDE_FAIL:-true}" = true ]; then
    fail "the check failed: see the annotations and the job's summary"
  fi
}

case "${1:-}" in
  install | record | baseline | check | report | comment | verdict) "$1" ;;
  *)
    echo "usage: $0 install|record|baseline|check|report|comment|verdict" >&2
    exit 64
    ;;
esac
