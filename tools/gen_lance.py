#!/usr/bin/env python3
"""Writes and reads a real Lance dataset with the `pylance` package: three
appends, a delete, a scalar index, a compaction, then a filtered scan and a
few rows taken by id.

    pip install pylance
    python3 tools/gen_lance.py <dir>
    sonde run -- python3 tools/gen_lance.py s3://bucket/prefix/docs.lance    # through S3, recorded
"""
import os
import sys

import lance
import pyarrow as pa

target = sys.argv[1]
opts = None
if target.startswith("s3://"):
    opts = {
        "aws_endpoint": os.environ["AWS_ENDPOINT_URL"],
        "aws_access_key_id": os.environ["AWS_ACCESS_KEY_ID"],
        "aws_secret_access_key": os.environ["AWS_SECRET_ACCESS_KEY"],
        "aws_region": os.environ.get("AWS_REGION", "us-east-1"),
        "allow_http": "true",
        "aws_virtual_hosted_style_request": "false",
    }


def batch(start, n, kind):
    return pa.table(
        {
            "id": list(range(start, start + n)),
            "kind": [kind] * n,
            "text": [f"row {i} " * 20 for i in range(start, start + n)],
        }
    )


lance.write_dataset(batch(0, 2000, "a"), target, mode="create", storage_options=opts)
lance.write_dataset(batch(2000, 2000, "b"), target, mode="append", storage_options=opts)
lance.write_dataset(batch(4000, 2000, "c"), target, mode="append", storage_options=opts)

ds = lance.dataset(target, storage_options=opts)
ds.delete("id < 100")
ds.create_scalar_index("id", index_type="BTREE")
ds.optimize.compact_files()

ds = lance.dataset(target, storage_options=opts)
print("version", ds.version, "rows", ds.count_rows())
print("kind b:", ds.to_table(filter="kind = 'b'", columns=["id"]).num_rows)
print("taken:", ds.take([5, 2500, 5800], columns=["id", "kind"]).num_rows)
