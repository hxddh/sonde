#!/usr/bin/env bash
# Runs the SDK smoke programs (tools/sdk-smoke) through `sonde run` and checks
# each run: the program must succeed and sonde must have recorded requests,
# none of them failed (lookups answered 404 are fine).
#
#   scripts/sdk-smoke.sh [sonde options, e.g. --url http://127.0.0.1:9100] [-- sdk...]
#
# SDKs: python node go java aws polars, tls (boto3 again, through `run
# --tls`: https and the unsigned streaming trailer) and docker (the AWS CLI in
# a container: `run -- docker run …` gives it the host's network and the
# variables). Default: every one whose toolchain is installed. Runs go to $SMOKE_SESSIONS (default: a temporary
# directory).
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
sonde="${SONDE:-$root/target/debug/sonde}"
opts=()
while [ $# -gt 0 ] && [ "$1" != "--" ]; do opts+=("$1"); shift; done
[ "${1:-}" = "--" ] && shift
sdks=("$@")
[ ${#sdks[@]} -eq 0 ] && sdks=(python node go java aws polars tls docker)
out="${SMOKE_SESSIONS:-$(mktemp -d)}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"   # the builds below change directory
smoke="$root/tools/sdk-smoke"
fail=0
py="${SMOKE_PYTHON:-python3}"
for sdk in "${sdks[@]}"; do
  run=(run --policy allow)
  case "$sdk" in
    python|tls)
      "$py" -c "import boto3" 2>/dev/null || { echo "skip $sdk (no boto3 for $py)"; continue; }
      [ "$sdk" = tls ] && run+=(--tls)
      cmd=("$py" "$smoke/python/smoke.py") ;;
    polars)
      "$py" -c "import boto3, polars" 2>/dev/null || { echo "skip polars (no polars for $py)"; continue; }
      cmd=("$py" "$smoke/polars/smoke.py") ;;
    aws)
      command -v aws >/dev/null || { echo "skip aws"; continue; }
      cmd=(bash "$smoke/aws/smoke.sh") ;;
    docker)
      docker info >/dev/null 2>&1 || { echo "skip docker"; continue; }
      cmd=(docker run --rm amazon/aws-cli:2.40.3 s3 ls) ;;
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
  if ! "$sonde" "${opts[@]}" "${run[@]}" -o "$session" -- "${cmd[@]}"; then
    echo "!! $sdk: the program failed (session: $session)"; fail=1; continue
  fi
  read -r requests errors < <("$sonde" show "$session" --format json | python3 -c 'import json,sys; s=json.load(sys.stdin)["summary"]; print(s["requests"], s["errors"])')
  if [ "${requests:-0}" = "0" ]; then
    echo "!! $sdk: nothing reached sonde"; fail=1
  elif [ "$errors" != "0" ]; then
    echo "!! $sdk: $errors failed requests"; "$sonde" show "$session" --errors; fail=1
  fi
done
exit $fail
