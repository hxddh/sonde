#!/usr/bin/env bash
# The steps of the sonde GitHub Action (action.yml), one per argument:
# install, record, baseline, check, report, comment, verdict. Each reads its
# inputs from SONDE_* variables and GitHub's own, and writes its outputs to
# $GITHUB_OUTPUT. docs/ci.md says what each does.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
work="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/sonde"
mkdir -p "$work"

# An output, whatever its value holds: GitHub's `name<<delimiter` form, the
# delimiter one no value has (a `name=value` line ends at the value's first
# newline, and what follows it would be read as outputs of its own).
out() {
  local delim="sonde_${RANDOM}${RANDOM}${RANDOM}"
  while [[ "$2" == *"$delim"* ]]; do delim="sonde_${RANDOM}${RANDOM}${RANDOM}"; done
  printf '%s<<%s\n%s\n%s\n' "$1" "$delim" "$2" "$delim" >>"${GITHUB_OUTPUT:-/dev/null}"
}
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
    # a release's tag (v33.0.0) is that release; a major's (v33) the newest
    # of it, a major and minor (v33.0) the newest of that (install.sh)
    [ -n "$version" ] || version="${ref#v}"
  else
    # a branch, a commit or a local checkout (`uses: ./`): its scripts are
    # of the version its Cargo.toml says, and run with a sonde of that
    # major, never with whatever release is the newest
    local own major
    own="$(sed -n 's/^version = "\([0-9][^"]*\)"$/\1/p' "$here/../Cargo.toml" 2>/dev/null | head -n 1)"
    [ -n "$own" ] || fail "no version in the action's Cargo.toml: set the version input (33, 33.0.0)"
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

# The words of $1 in WORDS, split as a shell splits them, with no shell:
# blanks part words, '…' and "…" keep theirs (in "…", \ escapes " \ $ `),
# \ escapes one character; nothing is run or expanded. Fails on an open
# quote. For what a shell would have expanded: PATTERNS has each word as a
# glob pattern (its quoted and escaped characters escaped) and GLOBS whether
# it has an unquoted *, ? or [; GLOB is 1 when any has; SHELL_WORDS is 1
# when only a shell says what a word is (an unquoted {, a leading ~).
split_words() {
  local s="$1" c q="" w="" p="" g=0 have=0 i
  WORDS=()
  PATTERNS=()
  GLOBS=()
  GLOB=0
  SHELL_WORDS=0
  # a character kept as itself in the word's pattern
  lit() {
    case "$1" in '*' | '?' | '[' | ']' | "\\") p+="\\$1" ;; *) p+="$1" ;; esac
  }
  word() {
    WORDS+=("$w")
    PATTERNS+=("$p")
    GLOBS+=("$g")
    w="" && p="" && g=0 && have=0
  }
  for ((i = 0; i < ${#s}; i++)); do
    c="${s:i:1}"
    if [ "$q" = "'" ]; then
      if [ "$c" = "'" ]; then q=""; else w+="$c" && lit "$c"; fi
    elif [ "$q" = '"' ]; then
      case "$c" in
        '"') q="" ;;
        "\\")
          case "${s:i+1:1}" in
            '"' | "\\" | '$' | '`') w+="${s:i+1:1}" && lit "${s:i+1:1}" && i=$((i + 1)) ;;
            *) w+="$c" && lit "$c" ;;
          esac
          ;;
        *) w+="$c" && lit "$c" ;;
      esac
    else
      case "$c" in
        "'" | '"') q="$c" && have=1 ;;
        "\\") w+="${s:i+1:1}" && lit "${s:i+1:1}" && i=$((i + 1)) && have=1 ;;
        ' ' | $'\t' | $'\n') [ "$have" = 0 ] || word ;;
        *)
          case "$c" in
            '*' | '?' | '[') g=1 && GLOB=1 ;;
            '{') SHELL_WORDS=1 ;;
            '~') [ "$have" = 1 ] || SHELL_WORDS=1 ;;
          esac
          w+="$c" && p+="$c" && have=1
          ;;
      esac
    fi
  done
  [ -z "$q" ] || return 1
  [ "$have" = 0 ] || word
}

# WORDS with each unquoted glob expanded as a shell expands it (the files it
# names, sorted; the word as typed when it names none), in EXPANDED. No word
# is read as shell again: a file's name, or a word like `[0]=x`, is one
# argument whatever it holds.
expand_globs() {
  local i f
  EXPANDED=()
  for ((i = 0; i < ${#WORDS[@]}; i++)); do
    if [ "${GLOBS[i]}" = 1 ]; then
      local found=()
      while IFS= read -r f; do found+=("$f"); done < <(compgen -G "${PATTERNS[i]}" | LC_ALL=C sort)
      if [ "${#found[@]}" -gt 0 ]; then
        EXPANDED+=("${found[@]}")
        continue
      fi
    fi
    EXPANDED+=("${WORDS[i]}")
  done
}

# `sonde run -o FILE` of the program, as the inputs give it, in this
# folder: its exit status. A command with shell syntax (`cd job && make`,
# `etl | tee log`, `etl > out`) is bash's, as typed, all of it under sonde,
# the program named by what bash is given to run; a plain command (`docker
# run …`, `spark-submit …`) is run itself, so sonde sets up what it knows
# (a container's network, a JVM engine's endpoint).
run_to() {
  local file="$1" args=() cmd=() status=0
  split_words "${SONDE_RUN_ARGS:-}" || fail "run-args: a quote is not closed: ${SONDE_RUN_ARGS:-}"
  args=(${WORDS[@]+"${WORDS[@]}"})
  [ -z "${SONDE_NAME:-}" ] || args+=(--name "$SONDE_NAME")
  case "$SONDE_RUN" in
    *[\;\&\|\<\>\(\)\$\`]* | *$'\n'* | cd\ *) cmd=(bash -c "$SONDE_RUN") ;;
    *)
      split_words "$SONDE_RUN" || fail "run: a quote is not closed: $SONDE_RUN"
      if [ "$SHELL_WORDS" = 1 ]; then
        # braces and ~ are the shell's to expand: bash's, as typed
        cmd=(bash -c "$SONDE_RUN")
      elif [ "$GLOB" = 0 ]; then
        cmd=("${WORDS[@]}")
      else
        # a glob (data/*.csv) expands to the files it names, and the program
        # is still run itself; the command's text is never given to eval
        expand_globs
        cmd=("${EXPANDED[@]}")
      fi
      ;;
  esac
  rm -f "$file"
  sonde run -o "$file" ${args[@]+"${args[@]}"} -- "${cmd[@]}" || status=$?
  return "$status"
}

# The program under `sonde run`, its run kept apart; its exit status is
# the verdict's, after the report and the comment.
record() {
  [ -n "${SONDE_RUN:-}" ] || fail "the run input is empty"
  local file="$work/run.sonde" status=0
  run_to "$file" || status=$?
  out exit "$status"
  [ -f "$file" ] || fail "sonde run wrote no run (exit $status)"
  out file "$file"
}

# The pull request's merge-base, recorded here: the base branch's code at
# `git merge-base origin/BASE HEAD`, checked out beside this one (a git
# worktree). Of actions/checkout's default checkout, the pull request merged
# into its base (refs/pull/N/merge), that is the merge's first parent: the
# base branch's newest when GitHub made the merge; of the pull request's
# branch checked out, where it left the base. Then `setup` run in it, and
# the same command, on the same runner (so its times compare). Its run in $work/merge-base-run/, a
# folder of its own, as its data is: it is never "the run before" this
# run's program (`sonde run` says what changed since the run before it in
# the same folder), nor in the runner's history. On failure, why in WHY.
merge_base() {
  local base="$1" dir="$work/merge-base" file="$work/merge-base-run/merge-base.sonde" deep=() commit prefix status=0
  WHY=""
  git rev-parse --git-dir >/dev/null 2>&1 || { WHY="not a git checkout"; return 1; }
  # the base branch, and this checkout's history down to where it left it
  [ "$(git rev-parse --is-shallow-repository)" != true ] || deep=(--unshallow)
  git fetch --no-tags --quiet ${deep[@]+"${deep[@]}"} origin "+refs/heads/$base:refs/remotes/origin/$base" ||
    { WHY="git could not fetch $base"; return 1; }
  commit="$(git merge-base "origin/$base" HEAD)" || { WHY="no merge-base of $base and this commit"; return 1; }
  # the working directory, as far down the repository as here
  prefix="$(git rev-parse --show-prefix)"
  git worktree remove --force "$dir" >/dev/null 2>&1 || rm -rf "$dir"
  git worktree add --quiet --detach "$dir" "$commit" || { WHY="git could not check out $commit"; return 1; }
  echo "::group::sonde: the merge-base ${commit:0:7} of $base, recorded"
  local why="$work/merge-base.why"
  rm -f "$why"
  rm -rf "$work/merge-base-run" "$work/merge-base-data"
  mkdir -p "$work/merge-base-run"
  (
    export SONDE_DATA_DIR="$work/merge-base-data"
    cd "$dir/$prefix" 2>/dev/null || { echo "the working directory is not in the merge-base's checkout" >"$why"; exit 1; }
    if [ -n "${SONDE_SETUP:-}" ] && ! bash -ec "$SONDE_SETUP"; then
      echo "setup failed in the merge-base's checkout" >"$why"
      exit 1
    fi
    run_to "$file"
  ) || status=$?
  echo "::endgroup::"
  git worktree remove --force "$dir" >/dev/null 2>&1 || true
  if [ "$status" != 0 ] || [ ! -f "$file" ]; then
    if [ -f "$why" ]; then
      WHY="$(cat "$why")"
    else
      WHY="the merge-base's program exited $status: what its checkout needs (dependencies) goes in the setup input"
    fi
    return 1
  fi
  MERGE_BASE="$commit"
}

# The workflow file this run is of: `.github/workflows/ci.yml` of
# `owner/repo/.github/workflows/ci.yml@refs/heads/main`.
workflow_file() {
  local ref="${GITHUB_WORKFLOW_REF:-}"
  ref="${ref%@*}"
  echo "${ref##*/}"
}

# The run the latest earlier run of this workflow kept on the base
# branch (a pull request's) or on this branch (a push); for a pull request
# with none, its merge-base recorded here ([merge_base]); nothing when there
# is none (sonde.toml's `[check] baseline` then applies). `none` (no baseline
# at all, `check --baseline none`) and another input are passed on as they are.
baseline() {
  case "${SONDE_BASELINE:-auto}" in
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
  # only a run that succeeded: one whose program or check failed is no
  # measure of the branch (a broken main would hold every pull request to it)
  ids="$(gh run list -R "$GITHUB_REPOSITORY" --workflow "$workflow" --branch "$base" \
    --event "$event" --status success --limit 6 --json databaseId --jq '.[].databaseId' 2>/dev/null || true)"
  for id in $ids; do
    # (a push's own run is not its baseline)
    [ "$id" != "${GITHUB_RUN_ID:-}" ] || continue
    rm -rf "$dir"
    if gh run download "$id" -R "$GITHUB_REPOSITORY" -n "$artifact" -D "$dir" >/dev/null 2>&1; then
      local file
      file="$(find "$dir" -name '*.sonde' | head -n 1)"
      # (a run of a format this sonde cannot read is skipped: the next one
      # back, else none; one an earlier major wrote that it reads is a
      # baseline as any is)
      if [ -n "$file" ] && ! sonde show "$file" -f json >/dev/null 2>&1; then
        note "baseline: the run kept by run $id is not one this sonde reads (an older format's); looking further back"
        continue
      fi
      if [ -n "$file" ]; then
        note "baseline: the run of $base kept by ${GITHUB_SERVER_URL:-https://github.com}/$GITHUB_REPOSITORY/actions/runs/$id"
        out baseline "$file"
        return
      fi
    fi
  done
  # a pull request with none kept: its merge-base, recorded in this job
  if [ -n "${GITHUB_BASE_REF:-}" ]; then
    if merge_base "$base"; then
      note "baseline: the merge-base ${MERGE_BASE:0:7} of $base, recorded in this job (no run of $base kept the artifact $artifact)"
      out baseline "$work/merge-base-run/merge-base.sonde"
      return
    fi
    warn "no baseline: no $event run of $workflow on $base kept the artifact $artifact, and its merge-base could not be recorded ($WHY); this run is held to its budget (and to sonde.toml's baseline, if it names one)"
    return
  fi
  warn "no baseline: no $event run of $workflow on $base kept the artifact $artifact (none yet, or it expired); this run is held to its budget (and to sonde.toml's baseline, if it names one)"
}

# `sonde check`'s flags, as the step's inputs give them, in ARGS.
check_args() {
  ARGS=("$SONDE_FILE")
  [ -z "${SONDE_BASELINE:-}" ] || ARGS+=(--baseline "$SONDE_BASELINE")
  [ -z "${SONDE_CONFIG:-}" ] || ARGS+=(--config "$SONDE_CONFIG")
  split_words "${SONDE_CHECK_ARGS:-}" || fail "check-args: a quote is not closed: ${SONDE_CHECK_ARGS:-}"
  ARGS+=(${WORDS[@]+"${WORDS[@]}"})
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
  local args=("$SONDE_FILE") file="$work/report/sonde-report.html"
  mkdir -p "$(dirname "$file")"
  case "${SONDE_BASELINE:-}" in
    "" | none) ;;
    *) args+=(--baseline "$SONDE_BASELINE") ;;
  esac
  [ "${SONDE_REDACT:-false}" != true ] || args+=(--redact)
  if sonde share "${args[@]}" -o "$file" --force; then
    out file "$file"
  else
    warn "no report: sonde share failed"
  fi
}

# The comment: on the job's summary, and one comment on the pull request,
# found by its marker and edited in place.
comment() {
  local md="$work/comment.md"
  check_args
  # (sonde check reads SONDE_REPORT_URL, the link the comment ends with)
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
