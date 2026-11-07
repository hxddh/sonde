# Recording: `sonde run`, `sonde proxy`, `sonde show`

sonde can sit between an application and its object store and record every S3
request the application makes. The application talks to a local endpoint with
throwaway credentials; sonde re-signs each request with the real endpoint's
credentials, forwards it, streams the response back, and writes a session file.

```sh
sonde run -- python etl.py                     # run a command with its S3 traffic recorded
sonde run --live --no-delete -- ./my-service   # print requests as they happen; refuse deletes
sonde proxy --listen 127.0.0.1:9000            # a standing endpoint for an app started elsewhere
sonde show ~/…/sessions/20261107-002225-etl.py.sonde --errors
```

The endpoint is chosen as for every command: `-e <id>`, `--url <url>`, or the
only one configured. Credentials are resolved as usual (profiles, SSO, IMDS,
IRSA, …) and stay in sonde; the application never receives them.

## What `sonde run` sets for the command

| Variable | Value |
|---|---|
| `AWS_ENDPOINT_URL`, `AWS_ENDPOINT_URL_S3` | `http://127.0.0.1:<port>` |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | `sonde` / a fixed dummy secret |
| `AWS_REGION`, `AWS_DEFAULT_REGION` | the endpoint's region (`us-east-1` when it has none) |
| `AWS_ALLOW_HTTP`, `AWS_S3_FORCE_PATH_STYLE` | `true` (object_store, some SDKs) |
| `AWS_VIRTUAL_HOSTED_STYLE_REQUEST` | `false` |
| `AWS_EC2_METADATA_DISABLED` | `true` |

It removes `AWS_SESSION_TOKEN`, `AWS_PROFILE`, `AWS_ROLE_ARN`,
`AWS_WEB_IDENTITY_TOKEN_FILE` and the container-credential variables, which
would point the SDK elsewhere. Clients that ignore these variables need the
endpoint set by hand (`sonde proxy` prints it); most SDKs also need path-style
addressing, or a `<bucket>.localhost` host for virtual-hosted style.

Checked with boto3, the AWS SDKs for JavaScript v3, Go v2 and Java v2, and
object_store (Rust) — `scripts/sdk-smoke.sh` runs them in CI.

## How a request is handled

1. The bucket and key come from the path (`/bucket/key`) or from a
   `<bucket>.localhost` Host header; the query is decoded as is (`+` stays a plus).
2. The operation is named from method, path and query (`UploadPart`,
   `ListObjectsV2`, …).
3. The policy may refuse it: `--no-delete` refuses deletes, aborts and
   lifecycle changes; `--read-only` lets only GET and HEAD through. A refused
   request gets an S3 `AccessDenied` error and never leaves the machine.
4. The client's signature headers are dropped and the request is signed again
   for the real endpoint (SigV4, the same code as every other sonde request).
   Bodies stream through, with these exceptions:
   - **signed `aws-chunked`** (`STREAMING-AWS4-HMAC-SHA256-PAYLOAD[-TRAILER]`,
     the Java SDK over HTTP): the chunk signatures chain from the client's key,
     so sonde decodes the body (up to 1 GiB), sends the payload with its SHA-256,
     and turns trailing checksums into headers;
   - a body without a length (chunked transfer) is buffered, since S3 wants a
     `Content-Length`;
   - `STREAMING-UNSIGNED-PAYLOAD-TRAILER` passes through unchanged (SDKs use it
     over HTTPS only; not yet exercised by the SDK tests).
5. **Presigned URLs** made with the dummy key are re-signed with headers; the
   record says so (`x-sonde-note`).
6. The response streams back unchanged. Error bodies are read whole and kept in
   the record.

What a record holds: the headers **the application sent** (signatures
redacted, as everywhere in sonde), the URL sonde sent to, status, latency to the
response headers, bytes each way, request ids and the error body.

The overhead measured against a local RustFS: +0.2 ms at p50 for a 1 KiB GET
(`cargo run --release -p sonde-proxy --example overhead`; docs/performance.md).

## Session files

`.sonde` files are JSON lines: a header (`format: "sonde-session"`, version,
creation time, endpoint id and URL, region, the command) and then one record
per line, appended as requests complete — a session survives a crash, and a
torn last line is skipped on reading. They hold no secrets: records are
redacted before they are written. Without `-o`, sessions go to the data
directory's `sessions/`.

`sonde show` prints the timeline and a summary; `--format json` gives the
header, the summary and every record.

## Operations, not requests

`sonde show` groups a session's requests into operations; `--requests` prints
one line per request instead.

```
01:09:10.149  Iceberg  commit: table metadata v2 · 3 files (7.6 KiB)   20 reqs · 112 ms
01:09:10.597  Iceberg  1 file written, no commit followed (72 B)         3 reqs · 18 ms
00:22:25.074  SlateDB  looked for the newest WAL: probed ids 1…128        8 reqs · 5 ms
00:22:25.085  SlateDB  WAL 1 written empty (fencing marker)              2 reqs · 9 ms
```

| Operation | Made of |
|---|---|
| multipart upload | CreateMultipartUpload, its parts, Complete or Abort (by upload id) |
| retried request | the same request repeated after a 5xx, 429, SlowDown or no response |
| lookups that found nothing | a burst of GET/HEAD answered 404 |
| listing in N pages | ListObjects calls continuing one another |
| format step | a commit point and what led to it (below) |

**Storage formats** are recognized from keys. Some writes are *commit points*:
a SlateDB manifest, WAL, SST or compaction-state file; an Iceberg
`*.metadata.json`; a Delta `_delta_log/<n>.json`. The format's other requests —
reads, listings, data files, Iceberg manifests — are folded into the next
commit point (a completed multipart upload counts as one write). Lookups
across several numbered files, like SlateDB searching for its newest WAL, are
a step of their own. Files with no commit within two seconds are reported as
written without one.

After the operations come **observations**: uploads never completed, files
written without a commit, retries, conditional writes that lost a race (412),
keys read again and again, prefixes listed again and again, small files sent as
multipart uploads, and notes true of nearly every step of a format (said once
rather than on every line).

Checked on real runs: SlateDB 0.18 (172 requests → 34 operations), pyiceberg
0.9.1 (116 → 6), delta-rs 1.6 (49 → 5), boto3 and the Java SDK — the sessions
are in `crates/proxy/tests/fixtures`.

## Reading a summary

```
172 requests to rustfs in 0.5 s · 116 not found · ↑36.5 KiB ↓22.7 KiB
```

GET and HEAD answered 404 are counted as **not found**, not as errors: stores
such as SlateDB find their newest manifest or WAL file by probing for keys
that do not exist yet. Errors are the other 4xx and 5xx answers, transport
failures, and requests sonde refused.

## Limits

- HTTP/1.1 between the application and sonde; plain HTTP on the loopback
  interface (no TLS listener yet).
- One target endpoint per proxy.
- Signed `aws-chunked` bodies are decoded in memory, up to 1 GiB each.
