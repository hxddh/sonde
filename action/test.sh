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
  run)
    # where it ran, and the code there
    pwd >"$FAKE/sonde.pwd"
    cat etl.py >"$FAKE/sonde.code" 2>/dev/null || true
    ls >"$FAKE/sonde.ls"
    # where it wrote, and its data folder; with FAKE_EXEC, the program, under it
    printf '%s\n' "${SONDE_DATA_DIR:-}" >"$FAKE/sonde.data"
    while [ $# -gt 0 ] && [ "$1" != -- ]; do [ "$1" = -o ] && touch "$2"; shift; done
    [ -z "${FAKE_EXEC:-}" ] || { shift; SONDE_UNDER=1 "$@" || true; }
    exit "${FAKE_RUN_EXIT:-0}" ;;
  check) echo "### sonde: within budget ✅"; exit "${FAKE_CHECK:-0}" ;;
  show) exit "${FAKE_SHOW:-0}" ;;
  share) while [ $# -gt 0 ]; do [ "$1" = -o ] && echo page >"$2"; shift; done ;;
esac
EOF
chmod +x "$t/bin/gh" "$t/bin/sonde"

# (none of the runner's own: CI runs this in a workflow of its own)
unset GITHUB_EVENT_NAME GITHUB_BASE_REF GITHUB_REF_NAME GITHUB_RUN_ID GITHUB_ACTION_REF SONDE_VERSION SONDE_DATA_DIR
export PATH="$t/bin:$PATH" FAKE="$t" RUNNER_TEMP="$t/runner" GITHUB_OUTPUT="$t/out"
export GITHUB_REPOSITORY=o/r GITHUB_WORKFLOW_REF="o/r/.github/workflows/ci.yml@refs/pull/7/merge"
export GITHUB_STEP_SUMMARY="$t/summary"
step() { : >"$GITHUB_OUTPUT"; bash "$here/sonde-action.sh" "$@"; }
# (out of any git checkout: a pull request's merge-base is recorded only in one, below)
mkdir -p "$t/nogit" && cd "$t/nogit"
output() { sed -n "s/^$1=//p" "$GITHUB_OUTPUT"; }
failed=0
is() {
  if [ "$2" != "$3" ]; then
    echo "FAIL $1: got [$2], want [$3]" >&2
    failed=1
  else echo "ok   $1"; fi
}

# install: a major's tag is the newest release of that major, a release's tag
# that release (a stand-in curl serves the releases and refuses downloads)
mkdir -p "$t/net"
cat >"$t/net/curl" <<'EOF'
#!/usr/bin/env bash
for a in "$@"; do case "$a" in https://*) url="$a" ;; esac; done
case "$url" in
  https://api.github.com/*)
    printf '  "tag_name": "%s",\n' v20.0.0 v19.3.0-rc.1 v19.2.10 v19.2.9 v19.0.0 v18.4.0 ;;
  *) echo "curl $url" >>"$FAKE/curl.log"; exit 22 ;;
esac
EOF
chmod +x "$t/net/curl"
# the action as checked out at a ref: its scripts beside a Cargo.toml of its version
mkdir -p "$t/at/action"
cp "$here/sonde-action.sh" "$t/at/action/"
cp "$here/../install.sh" "$t/at/"
# what was downloaded (the release's path) for the action at REF with VERSION;
# what it said in $t/said
fetched() {
  : >"$t/curl.log"
  printf '[workspace.package]\nversion = "%s"\n' "${OWN:-19.1.0}" >"$t/at/Cargo.toml"
  PATH="$t/net:$PATH" GITHUB_ACTION_REF="$1" SONDE_VERSION="${2:-}" bash "$t/at/action/sonde-action.sh" install >"$t/said" 2>&1 || true
  sed -n 's|.*/releases/\(.*\)/sonde-.*|\1|p' "$t/curl.log" | head -n 1
}
is "install @v19: the newest 19.x" "$(fetched v19)" download/v19.2.10
is "install @v18: the newest 18.x" "$(fetched v18)" download/v18.4.0
is "install @v19.0.0: that release" "$(fetched v19.0.0)" download/v19.0.0
is "install @v19 with a version: that version" "$(fetched v19 19.0.0)" download/v19.0.0
is "install @v19 with latest: the latest" "$(fetched v19 latest)" latest/download
is "install @v19.2: the newest 19.2.x" "$(fetched v19.2)" download/v19.2.10
is "install @v19 with 19.0: the newest 19.0.x" "$(fetched v19 19.0)" download/v19.0.0
is "install @v19.3 (a prerelease alone) fails before downloading" "$(fetched v19.3)" ""
is "it says so" "$(grep -c 'no release of sonde 19.3 found' "$t/said")" 1
is "install with a version that is none fails before downloading" "$(fetched v19 19.x)" ""
is "it says what a version is" "$(grep -c 'SONDE_VERSION=19.x: a version (26.0.0), a major (26)' "$t/said")" 1
is "install of a major with no release fails before downloading" "$(fetched v7)" ""
# a branch, a commit, a local checkout: the newest release of its own major, never `latest`
is "install @main of 19.1.0: the newest 19.x" "$(fetched main)" download/v19.2.10
is "it says why" "$(grep -c 'is sonde 19.1.0.s: installing the newest 19.x' "$t/said")" 1
is "install @<sha> of 18.0.0: the newest 18.x" "$(OWN=18.0.0 fetched 0123abc)" download/v18.4.0
is "install from ./ of 19.1.0: the newest 19.x" "$(fetched "")" download/v19.2.10
is "install @main of an unreleased 21.0.0 fails before downloading" "$(OWN=21.0.0 fetched main)" ""
is "it says so" "$(grep -c '^::error title=sonde::sonde 21 could not be installed' "$t/said")" 1
is "install @main with latest is refused" "$(fetched main latest)" ""
is "it says what to set" "$(grep -c 'set version: 19, or use the action at a release.s tag (@v19)' "$t/said")" 1
is "install @a-branch-like-v2-fix is a branch" "$(fetched v2-fix)" download/v19.2.10

# install.sh unpacks a release's archive: the binary, and its manual pages
# beside a bin folder (a stand-in curl serves one archive for any platform)
mkdir -p "$t/rel/man/man1" "$t/serve"
printf '#!/bin/sh\necho "sonde 99.0.0"\n' >"$t/rel/sonde" && chmod +x "$t/rel/sonde"
echo ".TH sonde 1" >"$t/rel/man/man1/sonde.1"
tar -C "$t/rel" -czf "$t/serve/a.tar.gz" sonde man
(cd "$t/serve" && { sha256sum a.tar.gz 2>/dev/null || shasum -a 256 a.tar.gz; }) >"$t/serve/a.tar.gz.sha256"
cat >"$t/serve/curl" <<'EOF'
#!/usr/bin/env bash
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift ;; https://*) url="$1" ;; esac; shift; done
case "$url" in
  *.tar.gz) cp "$SERVE/a.tar.gz" "$out" ;;
  *.tar.gz.sha256) cp "$SERVE/a.tar.gz.sha256" "$out" ;;
  *) exit 22 ;;
esac
EOF
chmod +x "$t/serve/curl"
installs() { # into DIR, what install.sh says to $t/said
  PATH="$t/serve:$PATH" SERVE="$t/serve" SONDE_VERSION=99.0.0 SONDE_VERIFY=0 SONDE_INSTALL_DIR="$1" sh "$here/../install.sh" >"$t/said" 2>&1
}
installs "$t/home/bin"
is "install: the binary" "$("$t/home/bin/sonde" --version)" "sonde 99.0.0"
is "install: its manual pages beside bin" "$(cat "$t/home/share/man/man1/sonde.1")" ".TH sonde 1"
installs "$t/elsewhere"
is "install: none beside a folder that is no bin" "$(find "$t" -maxdepth 1 -name share | wc -l | tr -d ' ')" 0

# a pull request: the newest of the base branch's runs that kept the artifact
export GITHUB_BASE_REF=main
FAKE_RUNS="11 22" FAKE_HAS=22 step baseline >"$t/said"
is "baseline from the base branch's run that has it" "$(output baseline)" "$t/runner/sonde/baseline/run.sonde"
is "it says which run" "$(grep -c 'actions/runs/22' "$t/said")" 1
is "of this workflow, on the base branch" "$(grep -c -- '--workflow ci.yml --branch main' "$t/gh.log")" 1
FAKE_SHOW=65 FAKE_RUNS="11 22" FAKE_HAS=22 step baseline >"$t/said"
is "a run an earlier sonde wrote is not a baseline" "$(output baseline)" auto
is "it says why" "$(grep -c 'not one this sonde reads' "$t/said")" 1
FAKE_RUNS="11" step baseline >"$t/said"
is "none kept: auto" "$(output baseline)" auto
is "none kept: a warning" "$(grep -c '^::warning title=sonde::no baseline' "$t/said")" 1
is "it says why no merge-base was recorded" "$(grep -c 'its merge-base could not be recorded (not a git checkout)' "$t/said")" 1
is "and nothing was run" "$([ -f "$t/sonde.pwd" ] && echo ran || echo none)" none

# none kept, in a checkout: the pull request's merge-base, recorded in this job
# (`git merge-base origin/BASE HEAD`), first of a shallow clone of the pull
# request's branch (actions/checkout with `ref: head_ref`): where it left the base
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$t"
git init -q -b main "$t/src"
(
  cd "$t/src"
  mkdir job && echo base >job/etl.py && git add . && git commit -qm base
  git checkout -qb pr && echo pr >job/etl.py && git commit -qam pr
  git checkout -q main && echo later >job/later && git add . && git commit -qm later
)
git clone -q --bare "$t/src" "$t/origin.git"
git clone -q --depth 1 --branch pr "file://$t/origin.git" "$t/repo"
cd "$t/repo/job"
rm -f "$t/sonde.pwd"
SONDE_RUN="python3 etl.py" SONDE_NAME=etl FAKE_RUNS="11" step baseline >"$t/said"
is "the merge-base is the baseline" "$(output baseline)" "$t/runner/sonde/merge-base-run/merge-base.sonde"
is "it says which commit" "$(grep -c '^::notice title=sonde::baseline: the merge-base [0-9a-f]\{7\} of main, recorded in this job' "$t/said")" 1
is "of a branch: recorded from where it left the base" "$(cat "$t/sonde.code")" base
is "not from the base branch's newest" "$(grep -c '^later$' "$t/sonde.ls")" 0
is "in the working directory of its checkout" "$(basename "$(cat "$t/sonde.pwd")")/$(basename "$(dirname "$(cat "$t/sonde.pwd")")")" job/merge-base
is "the same command and name" "$(tail -n 4 "$t/sonde.run" | paste -sd'|')" "etl|--|python3|etl.py"
is "its checkout is gone after" "$(git worktree list | wc -l | tr -d ' ')" 1
is "in a data folder of its own, not the runner's" "$(cat "$t/sonde.data")" "$t/runner/sonde/merge-base-data"
is "in a folder of its own, not the run's (never its run before)" "$(dirname "$(sed -n '/^-o$/{n;p;}' "$t/sonde.run")")" "$t/runner/sonde/merge-base-run"
# shellcheck disable=SC2016 # (expanded by setup, in its folder)
SONDE_RUN="python3 etl.py" SONDE_SETUP='echo "$PWD" >"$FAKE/setup.pwd"' FAKE_RUNS="" step baseline >/dev/null
is "setup runs first, in the same folder" "$(cat "$t/setup.pwd")" "$(cat "$t/sonde.pwd")"
SONDE_RUN="python3 etl.py" SONDE_SETUP="false" step baseline >"$t/said"
is "setup failed: held to its budget" "$(output baseline)" auto
is "it says why" "$(grep -c 'could not be recorded (setup failed in the merge-base.s checkout)' "$t/said")" 1
SONDE_RUN="python3 etl.py" FAKE_RUN_EXIT=3 step baseline >"$t/said"
is "its program failed: held to its budget" "$(output baseline)" auto
is "it says why, and what to set" "$(grep -c "the merge-base's program exited 3: .*the setup input" "$t/said")" 1
# a push is no pull request: no merge-base
GITHUB_BASE_REF="" GITHUB_REF_NAME=main SONDE_RUN="python3 etl.py" step baseline >"$t/said"
is "a push with none kept: no merge-base" "$(grep -c 'merge-base' "$t/said")" 0
# actions/checkout's default: the pull request merged into its base
# (refs/pull/7/merge, depth 1), whose merge-base is the base branch's commit
# it was merged into, its newest when the merge was made
(
  cd "$t/src"
  git checkout -q --detach main && git merge -q --no-ff pr -m merge
  git push -q "$t/origin.git" HEAD:refs/pull/7/merge && git checkout -q main
)
git init -q "$t/merged"
cd "$t/merged"
git remote add origin "file://$t/origin.git"
git fetch -q --no-tags --depth=1 origin +refs/pull/7/merge:refs/remotes/pull/7/merge
git checkout -q --detach refs/remotes/pull/7/merge
cd job
SONDE_RUN="python3 etl.py" FAKE_RUNS="" step baseline >"$t/said"
is "of the merge: the merge-base is the baseline" "$(output baseline)" "$t/runner/sonde/merge-base-run/merge-base.sonde"
is "recorded from the base branch's commit it was merged into" "$(cat "$t/sonde.code")/$(grep -c '^later$' "$t/sonde.ls")" base/1
is "which is the merge's first parent" "$(grep -c "^::notice title=sonde::baseline: the merge-base $(git rev-parse --short=7 HEAD^1) of main" "$t/said")" 1
cd "$t/nogit"
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
is "of the pushes" "$(grep -c -- '--branch main --event push' "$t/gh.log")" 1
# a scheduled run: the schedule's run before it
: >"$t/gh.log"
GITHUB_EVENT_NAME=schedule GITHUB_REF_NAME=main GITHUB_RUN_ID=11 FAKE_RUNS="11 22" FAKE_HAS=22 step baseline >/dev/null
is "a scheduled run takes the schedule's run before it" "$(output baseline)" "$t/runner/sonde/baseline/run.sonde"
is "of the schedule's runs" "$(grep -c -- '--branch main --event schedule' "$t/gh.log")" 1

# the program's command as typed, bash's; its status kept for the verdict
SONDE_RUN="python3 -c 'print(1)'" SONDE_RUN_ARGS="--url https://s3.test --policy allow" SONDE_NAME=etl FAKE_RUN_EXIT=3 step record
is "record keeps the program's status" "$(output exit)" 3
is "record writes a run" "$(output file)" "$t/runner/sonde/run.sonde"
is "in the runner's data folder" "$(cat "$t/sonde.data")" ""
is "the command as typed, run by bash" "$(tail -n 3 "$t/sonde.run" | paste -sd'|')" "bash|-c|python3 -c 'print(1)'"
is "run's flags and the name before --" "$(grep -c -e '^--policy$' -e '^allow$' -e '^etl$' "$t/sonde.run")" 3
# a plain command is run itself, so sonde sets up what it knows (a container's network, a JVM engine's endpoint)
SONDE_RUN="docker run --rm 'amazon/aws-cli' s3 ls" step record >/dev/null
is "a plain command is run itself, its words as typed" "$(sed -n '/^--$/,$p' "$t/sonde.run" | tail -n +2 | paste -sd'|')" "docker|run|--rm|amazon/aws-cli|s3|ls"
# each shell operator as typed, all of it under sonde, none of it before
# shellcheck disable=SC2016 # (expanded by the program's bash, under sonde)
{
  said() { : >"$t/ran"; SONDE_RUN="$1" FAKE_EXEC=1 step record >/dev/null; cat "$t/ran"; }
  mkdir -p sub
  is "cd x && runs both under sonde" "$(said 'cd sub && echo "$SONDE_UNDER ${PWD##*/}" >>"$FAKE/ran"')" "1 sub"
  is "&& runs both under sonde" "$(said 'echo "a$SONDE_UNDER" >>"$FAKE/ran" && echo "b$SONDE_UNDER" >>"$FAKE/ran"' | paste -sd' ')" "a1 b1"
  is "| pipes under sonde" "$(said 'echo "x$SONDE_UNDER" | tr x y >>"$FAKE/ran"')" y1
  is "| is not sonde's command" "$(grep -c 'run.sonde$' "$t/sonde.run")" 1
  is "> redirects the program's output" "$(said 'echo "out$SONDE_UNDER" >"$FAKE/ran"')" out1
  is "; runs both under sonde" "$(said 'echo "a$SONDE_UNDER" >>"$FAKE/ran"; echo "b$SONDE_UNDER" >>"$FAKE/ran"' | paste -sd' ')" "a1 b1"
  : >"$t/ran"
  SONDE_RUN='true && echo outside >"$FAKE/ran"' step record >/dev/null || true
  is "nothing runs outside sonde" "$(cat "$t/ran")" ""
}

# check: passed or not; anything else is the step's failure
export SONDE_FILE="$t/runner/sonde/run.sonde" SONDE_BASELINE=auto
SONDE_CHECK_ARGS="--tolerance 5" step check >/dev/null
is "a check that passed" "$(output passed)" true
is "check's flags" "$(paste -sd' ' "$t/sonde.check")" "check $SONDE_FILE --baseline auto --tolerance 5 -f github"
FAKE_CHECK=1 step check >/dev/null
is "a check that failed" "$(output passed)" false
if FAKE_CHECK=64 step check >/dev/null; then is "a check that could not run fails the step" 0 1; fi

# every step given a path by the inputs (a baseline: baseline/main.sonde)
# runs where the command does
for s in record baseline check report comment; do
  is "the $s step runs in working-directory" "$(awk -v s="$s" '/^    - name:/ { wd = 0 }
    /working-directory: \$\{\{ inputs.working-directory \}\}/ { wd = 1 }
    $0 ~ ("sonde-action.sh\" " s "$") { print wd }' "$here/../action.yml")" 1
done

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
is "the summary has the comment" "$(grep -c 'within budget' "$t/summary")" 1
SONDE_EXIT=1 step comment
is "the program's status is the run's: no flag carries it" "$(grep -c -- '--program-exit' "$t/sonde.check")" 0
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
