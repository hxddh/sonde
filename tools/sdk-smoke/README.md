# SDK smoke tests for `sonde run`

One small program per AWS SDK, doing what applications usually do: create a
bucket, a small PUT, a multipart upload through the SDK's own uploader, a
ranged GET, HEAD, LIST, a copy, a batch delete and a presigned GET. Each uses
the SDK's defaults (checksums, chunked signing), configured only from the
environment `sonde run` sets. Each exits non-zero on the first failure.

    scripts/sdk-smoke.sh --url http://127.0.0.1:9100    # every SDK, through sonde

The bucket is `$SMOKE_BUCKET` (default `sonde-smoke`).
