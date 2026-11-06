#!/usr/bin/env python3
"""Writes a real Apache Iceberg table with pyiceberg (SQLite catalog, local
warehouse) for sonde's decoder tests and integration scenario.

    pip install "pyiceberg[sql-sqlite,pyarrow]==0.9.1"
    python3 tools/gen_iceberg.py <out-dir>

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

out = os.path.abspath(sys.argv[1])
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
data_dir = os.path.join(wh, "demo.db", "events", "data")
with open(os.path.join(data_dir, "00000-0-orphan-from-failed-write.parquet"), "wb") as f:
    f.write(b"PAR1" + b"\0" * 64 + b"PAR1")
os.remove(os.path.join(out, "catalog.db"))
print(f"table at {wh}/demo.db/events, {len(t.metadata.snapshots)} snapshots")
