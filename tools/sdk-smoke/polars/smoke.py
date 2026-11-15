"""polars (Rust's object_store) through `sonde run`: configured only from the
environment. The bucket is made, and the table removed, with boto3."""
import os
import boto3
import polars as pl

bucket = os.environ.get("SMOKE_BUCKET", "sonde-smoke")
s3 = boto3.client("s3")
try:
    s3.create_bucket(Bucket=bucket)
except (s3.exceptions.BucketAlreadyOwnedByYou, s3.exceptions.BucketAlreadyExists):
    pass
url = f"s3://{bucket}/polars/table.parquet"
df = pl.DataFrame({"id": range(200_000), "v": [i * 0.5 for i in range(200_000)]})
df.write_parquet(url)
assert pl.read_parquet(url).equals(df), "read back"
# a lazy scan reads the footer, then only the row groups it needs
assert pl.scan_parquet(url).filter(pl.col("id") < 10).collect().height == 10, "scan"
s3.delete_object(Bucket=bucket, Key="polars/table.parquet")
print("polars ok")
