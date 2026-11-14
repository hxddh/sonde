# SDK smoke tests for `sonde run`

One small program per client, each configured only from the environment `sonde run` sets (no endpoint, path-style
or region in the code), each exiting non-zero on the first failure:

- boto3, the AWS SDKs for JavaScript v3, Go v2 and Java v2, with the SDK's defaults (checksums, chunked signing):
  create a bucket, a small PUT, a multipart upload through the SDK's own uploader, a ranged GET, HEAD, LIST, a
  copy, a batch delete and a presigned GET;
- the AWS CLI: `aws s3 cp` up and down (in parts);
- polars (Rust's object_store): a Parquet table written, read back and scanned;
- `tls`: boto3 again through `run --tls` (https, and the unsigned streaming trailer);
- `docker`: the AWS CLI's image listing the buckets, run as `sonde run -- docker run …` with nothing added.

    scripts/sdk-smoke.sh --url http://127.0.0.1:9100    # every client, through sonde

The bucket is `$SMOKE_BUCKET` (default `sonde-smoke`). CI runs them against RustFS (`record` in ci.yml).
