#!/usr/bin/env bash
# Runs the SDK smoke programs (tools/sdk-smoke) through `sonde run` and checks
# each session: the program must succeed and sonde must have recorded no
# failed request (lookups answered 404 are fine).
#
#   scripts/sdk-smoke.sh [sonde options, e.g. --url http://127.0.0.1:9100] [-- sdk...]
#
# SDKs: python node go java (default: every one whose toolchain is installed).
# Sessions go to $SMOKE_SESSIONS (default: a temporary directory).
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
sonde="${SONDE:-$root/target/debug/sonde}"
opts=()
while [ $# -gt 0 ] && [ "$1" != "--" ]; do opts+=("$1"); shift; done
[ "${1:-}" = "--" ] && shift
sdks=("$@")
[ ${#sdks[@]} -eq 0 ] && sdks=(python node go java)
out="${SMOKE_SESSIONS:-$(mktemp -d)}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"   # the builds below change directory
smoke="$root/tools/sdk-smoke"
fail=0
for sdk in "${sdks[@]}"; do
  case "$sdk" in
    python)
      py="${SMOKE_PYTHON:-python3}"
      "$py" -c "import boto3" 2>/dev/null || { echo "skip python (no boto3 for $py)"; continue; }
      cmd=("$py" "$smoke/python/smoke.py") ;;
    node)
      [ -d "$smoke/node/node_modules" ] || (cd "$smoke/node" && npm install --silent --no-audit --no-fund) || { echo "skip node"; continue; }
      cmd=(node "$smoke/node/smoke.mjs") ;;
    go)
      command -v go >/dev/null || { echo "skip go"; continue; }
      (cd "$smoke/go" && go build -o "$out/go-smoke" .) || { echo "go build failed"; fail=1; continue; }
      cmd=("$out/go-smoke") ;;
    java)
      command -v mvn >/dev/null || { echo "skip java"; continue; }
      (cd "$smoke/java" && mvn -q -B compile) || { echo "java build failed"; fail=1; continue; }
      cmd=(mvn -q -B -f "$smoke/java/pom.xml" exec:java) ;;
    *) echo "unknown SDK $sdk"; exit 64 ;;
  esac
  session="$out/$sdk.sonde"
  echo "== $sdk"
  # (each program removes what it wrote: deletes are asked for)
  if ! "$sonde" "${opts[@]}" run --allow-writes -o "$session" -- "${cmd[@]}"; then
    echo "!! $sdk: the program failed (session: $session)"; fail=1; continue
  fi
  errors="$("$sonde" show "$session" --format json | python3 -c 'import json,sys; print(json.load(sys.stdin)["summary"]["errors"])')"
  if [ "$errors" != "0" ]; then
    echo "!! $sdk: $errors failed requests"; "$sonde" show "$session" --errors; fail=1
  fi
done
exit $fail
