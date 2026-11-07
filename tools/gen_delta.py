#!/usr/bin/env python3
"""Writes a real Delta Lake table with delta-rs (the `deltalake` package):
three appends and an overwrite, so `_delta_log` holds commits 0–3.

    pip install deltalake
    python3 tools/gen_delta.py <dir>
    sonde run -- python3 tools/gen_delta.py s3://bucket/prefix    # through S3, recorded

On S3 the commits are conditional creates (`AWS_CONDITIONAL_PUT=etag`), the
safe way for concurrent writers without a lock table.
"""
import os
import sys

import pyarrow as pa
from deltalake import write_deltalake

target = sys.argv[1]
opts = None
if target.startswith("s3://"):
    opts = {
        "AWS_ENDPOINT_URL": os.environ["AWS_ENDPOINT_URL"],
        "AWS_ACCESS_KEY_ID": os.environ["AWS_ACCESS_KEY_ID"],
        "AWS_SECRET_ACCESS_KEY": os.environ["AWS_SECRET_ACCESS_KEY"],
        "AWS_REGION": os.environ.get("AWS_REGION", "us-east-1"),
        "AWS_ALLOW_HTTP": "true",
        "AWS_CONDITIONAL_PUT": "etag",
    }
def batch(start, n, kind):
    return pa.table({"id": list(range(start, start + n)), "kind": [kind] * n})
write_deltalake(target, batch(0, 100, "click"), storage_options=opts)
write_deltalake(target, batch(100, 50, "view"), mode="append", storage_options=opts)
write_deltalake(target, batch(150, 25, "view"), mode="append", storage_options=opts)
write_deltalake(target, batch(0, 30, "click"), mode="overwrite", storage_options=opts)
print(f"delta table at {target}: 4 commits")
