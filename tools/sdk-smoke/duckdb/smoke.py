"""DuckDB (httpfs) through `sonde run`: a Parquet file written, then queried
twice, each query reading the file's footer and the row groups it needs by
range (what `obs.rangeagain` finds when the footer is read again). DuckDB
reads no AWS_ENDPOINT_URL of its own: the endpoint is given from it, as
docs/recording.md says. The bucket is made, and the file removed, with
boto3."""
import os
from urllib.parse import urlparse

import boto3
import duckdb

bucket = os.environ.get("SMOKE_BUCKET", "sonde-smoke")
s3 = boto3.client("s3")
try:
    s3.create_bucket(Bucket=bucket)
except (s3.exceptions.BucketAlreadyOwnedByYou, s3.exceptions.BucketAlreadyExists):
    pass
endpoint = urlparse(os.environ["AWS_ENDPOINT_URL"])
db = duckdb.connect()
db.execute("INSTALL httpfs; LOAD httpfs;")
db.execute(
    "CREATE SECRET smoke (TYPE s3, KEY_ID ?, SECRET ?, REGION ?, ENDPOINT ?, URL_STYLE 'path', USE_SSL ?)",
    [
        os.environ["AWS_ACCESS_KEY_ID"],
        os.environ["AWS_SECRET_ACCESS_KEY"],
        os.environ.get("AWS_REGION", "us-east-1"),
        endpoint.netloc,
        endpoint.scheme == "https",
    ],
)
url = f"s3://{bucket}/duckdb/events.parquet"
db.execute(
    f"COPY (SELECT range AS id, range % 7 AS day, range * 0.5 AS v FROM range(300000)) TO '{url}' (FORMAT parquet, ROW_GROUP_SIZE 50000)"
)
# two queries: each reads the footer, then only the columns and row groups it needs
assert db.execute(f"SELECT count(*) FROM '{url}' WHERE day = 3").fetchone()[0] == 42857, "count"
assert db.execute(f"SELECT max(v) FROM '{url}' WHERE id < 1000").fetchone()[0] == 499.5, "max"
s3.delete_object(Bucket=bucket, Key="duckdb/events.parquet")
print("duckdb ok")
