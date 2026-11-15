#!/usr/bin/env bash
# The AWS CLI (v1 or v2) through `sonde run`: configured only from the environment.
set -euo pipefail
bucket="${SMOKE_BUCKET:-sonde-smoke}"
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
aws s3api head-bucket --bucket "$bucket" 2>/dev/null || aws s3 mb "s3://$bucket"
head -c $((12 * 1024 * 1024)) /dev/urandom > "$dir/up.bin"
# up in parts (past the CLI's 8 MiB threshold), down in ranged parts
aws s3 cp --only-show-errors "$dir/up.bin" "s3://$bucket/aws/big.bin"
aws s3 cp --only-show-errors "s3://$bucket/aws/big.bin" "$dir/down.bin"
cmp "$dir/up.bin" "$dir/down.bin"
aws s3 rm --only-show-errors "s3://$bucket/aws/big.bin"
echo "aws cli ok"
