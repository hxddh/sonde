#!/usr/bin/env python3
"""Writes a real Apache Iceberg table with pyiceberg (SQLite catalog, local
warehouse) for sonde's decoder tests and integration scenario.

    pip install "pyiceberg[sql-sqlite,pyarrow]==0.9.1"
    python3 tools/gen_iceberg.py <out-dir>
    sonde run -- python3 tools/gen_iceberg.py s3://bucket/prefix   # through S3, recorded

The table `demo.events` gets four snapshots (append, append, overwrite,
delete), an old snapshot made expirable by a short max-snapshot-age, and one
orphan data file (as a failed write leaves behind).
"""
import os
import shutil
import sys

import pyarrow as pa
from pyiceberg.catalog.sql import SqlCatalog
from pyiceberg.expressions import EqualTo

import tempfile

target = sys.argv[1]
if target.startswith("s3://"):
    # the catalog stays local; the warehouse is on S3, configured from the
    # environment `sonde run` sets
    out = tempfile.mkdtemp()
    wh = target.rstrip("/")
    cat = SqlCatalog("sonde", uri=f"sqlite:///{out}/catalog.db", warehouse=wh, **{
        "s3.endpoint": os.environ["AWS_ENDPOINT_URL"],
        "s3.access-key-id": os.environ["AWS_ACCESS_KEY_ID"],
        "s3.secret-access-key": os.environ["AWS_SECRET_ACCESS_KEY"],
        "s3.region": os.environ.get("AWS_REGION", "us-east-1"),
    })
else:
    out = os.path.abspath(target)
    wh = os.path.join(out, "warehouse")
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(wh)
    cat = SqlCatalog("sonde", uri=f"sqlite:///{out}/catalog.db", warehouse=f"file://{wh}")
cat.create_namespace("demo")
schema = pa.schema([("id", pa.int64()), ("kind", pa.string()), ("value", pa.float64())])
t = cat.create_table("demo.events", schema=schema, properties={"history.expire.max-snapshot-age-ms": "1"})
def batch(start, n, kind):
    return pa.table({"id": list(range(start, start + n)), "kind": [kind] * n, "value": [float(i) for i in range(n)]}, schema=schema)
t.append(batch(0, 100, "click"))
t.append(batch(100, 50, "view"))
t.overwrite(batch(0, 30, "click"))
t.delete(EqualTo("id", 5))
# an orphan: a data file no snapshot references
orphan = f"{t.location()}/data/00000-0-orphan-from-failed-write.parquet"
with t.io.new_output(orphan).create(overwrite=True) as f:
    f.write(b"PAR1" + b"\0" * 64 + b"PAR1")
os.remove(os.path.join(out, "catalog.db"))
print(f"table at {wh}/demo.db/events, {len(t.metadata.snapshots)} snapshots")
