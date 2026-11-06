#!/usr/bin/env sh
# Integration tests against real S3 implementations.
#
#   scripts/integration.sh [minio ceph rustfs]     (default: all three)
#
# Starts them with docker compose, waits until they answer, writes the
# scenarios (incomplete uploads, versions, delete markers, a real SlateDB
# database and Iceberg table), and runs the live tests of sonde-core
# (browse, multipart, versions, capacity, the guard), probes (the whole
# suite, with cleanup) and the layout lens through S3.
set -eu
cd "$(dirname "$0")/.."
services="${*:-minio ceph rustfs}"
compose="docker compose -f tests/integration/docker-compose.yml"
for s in $services; do $compose --profile "$s" up -d "$s"; done
targets=""
wait_for() { # name url
  i=0
  until curl -s -o /dev/null -m 3 "$2"; do
    i=$((i + 1)); [ $i -gt 120 ] && { echo "$1 did not start"; $compose logs "$1" | tail -50; exit 1; }
    sleep 2
  done
  # RGW answers before its user exists: wait for a signed request to pass
  sleep 5
}
for s in $services; do
  case "$s" in
    minio) wait_for minio http://127.0.0.1:9000/minio/health/live; targets="$targets;minio=http://127.0.0.1:9000,sondeminio,sondeminio-secret,minio" ;;
    ceph) wait_for ceph http://127.0.0.1:7480; targets="$targets;ceph=http://127.0.0.1:7480,sondeaccess,sondesecret,ceph-rgw" ;;
    rustfs) wait_for rustfs http://127.0.0.1:9100; targets="$targets;rustfs=http://127.0.0.1:9100,rustfsadmin,rustfsadmin,other" ;;
  esac
done
export SONDE_IT_TARGETS="${targets#;}"
# fresh layouts written by the real systems
layouts="$(mktemp -d)"
trap 'rm -rf "$layouts"' EXIT
if [ "${SONDE_IT_GENERATE:-1}" = 1 ]; then
  (cd tools/gen-slatedb && cargo run --release -q -- "$layouts/slatedb")
  python3 tools/gen_iceberg.py "$layouts/iceberg"
  export SONDE_IT_LAYOUTS="$layouts"
fi
cargo test -p sonde-core --test live --test live_layouts -- --nocapture
cargo test -p probes --test live -- --nocapture
echo "integration: ok ($SONDE_IT_TARGETS)"
