# Recording

The program talks to a local endpoint with keys made for the run; sonde checks them, re-signs with the real
credentials, forwards the request, streams the answer back and appends a record to the run's file. The real
credentials never reach the program.

```sh
sonde run -- python etl.py                  # record a command
sonde run --policy no-delete -- ./job      # refuse its deletes
sonde run --listen                          # a standing endpoint (127.0.0.1:9000) for a program started elsewhere
```

The endpoint is `-e <id>`, `--url <url>`, or the only one configured; credentials as for every command
([credentials](credentials.md)).

**In the app**, Runs' command bar (⌘N, or Runs pressed again) does the same: a command, an endpoint (the folder
behind its `⋯`), Return. It runs through your shell in a pseudo-terminal (ConPTY on Windows); its run is the
newest in the list, on its Output tab, its terminal beside its operations; Stop ends it, a notification says how. Output is kept in the run with
arrival times (`{"log": …}`), escape codes removed, `\r`-redrawn lines at their last state; choosing an operation
highlights its lines and back. Output is kept as printed: do not print secrets. With a folder chosen and the field
empty, the bar proposes what the folder runs: a Makefile's targets, `package.json`'s scripts, `pyproject.toml`'s entry
points, Python programs (`spark-submit` for a Spark job), `go run .`, `cargo run`.

## What the program is given

| Variable | Value |
|---|---|
| `AWS_ENDPOINT_URL`, `AWS_ENDPOINT_URL_S3`, `S3_ENDPOINT_URL` | `http://127.0.0.1:<port>` |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | made for this recording (`SONDE…` / 64 hex digits) |
| `AWS_REGION`, `AWS_DEFAULT_REGION` | the endpoint's region (`us-east-1` when none) |
| `AWS_ALLOW_HTTP`, `AWS_S3_FORCE_PATH_STYLE` | `true` |
| `AWS_VIRTUAL_HOSTED_STYLE_REQUEST` | `false` |
| `AWS_EC2_METADATA_DISABLED` | `true` |
| `RCLONE_S3_ENDPOINT`, `RCLONE_S3_ACCESS_KEY_ID`, `RCLONE_S3_SECRET_ACCESS_KEY`, `RCLONE_S3_REGION`, `RCLONE_S3_FORCE_PATH_STYLE`, `RCLONE_S3_ENV_AUTH` | the same, for rclone's S3 remotes (they win over its config file) |

- Removed: `AWS_SESSION_TOKEN`, `AWS_SECURITY_TOKEN`, `AWS_PROFILE`, `AWS_DEFAULT_PROFILE`, `AWS_ROLE_ARN`,
  `AWS_WEB_IDENTITY_TOKEN_FILE`, the container-credential variables.
- Tested in CI (`scripts/sdk-smoke.sh`, against RustFS): boto3, AWS SDKs for JavaScript v3, Go v2, Java v2, the AWS
  CLI (also in its container image, through `run -- docker run`), polars (object_store), and boto3 under `--tls`.
- An error that names a `SONDE…` key came from a store sonde was not in front of: the client did not take the
  endpoint. When nothing reaches sonde, `run` says how the program's own client takes it, that client first:

| Client | How it reaches sonde |
|---|---|
| AWS CLI v1.29+ / v2.13+, s5cmd (`S3_ENDPOINT_URL`), pandas (s3fs), polars, pyarrow `FileSystem.from_uri`, rclone (`RCLONE_S3_*`, every S3 remote), Spark (below) | by itself |
| the DuckDB CLI | by itself under `sonde run -- duckdb …`: it is given `-init` with `SET s3_*` (after your `~/.duckdbrc`; a command with its own `-init` keeps it) |
| pyarrow `S3FileSystem()` | `S3FileSystem(endpoint_override=os.environ["AWS_ENDPOINT_URL"])` |
| DuckDB in a program | `SET s3_endpoint` to the endpoint's host and port, `s3_url_style='path'`, `s3_use_ssl=false` (read it from `AWS_ENDPOINT_URL` in the program) |
| a container | `sonde run -- docker run …` (or `podman run`) is given `--network host` (unless the command names a network) and `-e NAME` for each variable above, by name only; `--tls`'s certificate is mounted where it is. A network of its own cannot reach 127.0.0.1. Docker Desktop needs host networking turned on |

Production that runs elsewhere (Kubernetes, serverless) is read from its logs: [`sonde import`](#production-traffic-access-logs-cloudtrail-har).

### TLS

`--tls` (or `[record] tls = true`) also serves TLS on the same port (by the first bytes of a connection);
`AWS_ENDPOINT_URL` becomes `https://`, plus:

| Variable | Value |
|---|---|
| `AWS_CA_BUNDLE`, `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE`, `CURL_CA_BUNDLE` | `<data>/tls/bundle.pem`: system roots plus sonde's authority |
| `NODE_EXTRA_CA_CERTS` | `<data>/tls/ca.pem`: sonde's authority alone |

The first run makes an authority, issues one certificate (`localhost`, `*.localhost`, `127.0.0.1`, `::1`) and
discards the authority's key. The certificate's key is `tls/key.pem` (0600); both renew after about two years,
by one sonde at a time (`tls/make.lock`). `bundle.pem` is rewritten only when it changes. HTTP/1.1 only.

### Spark, Flink, Trino and Hadoop s3a

`sonde run` adds the endpoint to known engines' command lines, before the command's own arguments (which win; sonde
warns when they name another endpoint):

| Command | sonde adds |
|---|---|
| `spark-submit`, `spark-shell`, `pyspark`, `spark-sql`, `sparkR` | `--conf spark.hadoop.fs.s3a.endpoint=http://127.0.0.1:<port>`, `…endpoint.region`, `…path.style.access=true`, `…connection.ssl.enabled=false`, `…aws.credentials.provider=com.amazonaws.auth.EnvironmentVariableCredentialsProvider` |
| `flink run`, `flink run-application` | the same as `-Ds3.endpoint=…`, `-Ds3.path.style.access=true`, … |
| any command | `PYSPARK_SUBMIT_ARGS` with the same `--conf`s, before its value |

Keys never go on a command line. Engines get plain HTTP even with `--tls`. Only a driver on this machine reaches
127.0.0.1 (Spark `local[*]` or client mode, a local Flink cluster).

By hand, with what `sonde run --listen` prints (its address fixes the port; keys are new each time):

- **Spark**: the `spark.hadoop.fs.s3a.*` settings above; `s3://` paths also need
  `spark.hadoop.fs.s3.impl=org.apache.hadoop.fs.s3a.S3AFileSystem`; `fs.s3a.bucket.<name>.endpoint` overrides the
  global one. Iceberg `S3FileIO` reads `AWS_ENDPOINT_URL` (Java SDK 2.21+) and needs
  `spark.sql.catalog.<name>.s3.path-style-access=true`.
- **Hadoop** (`core-site.xml`: Hive, `hadoop fs`, Flink's hadoop file system):

  ```xml
  <property><name>fs.s3a.endpoint</name><value>http://127.0.0.1:9000</value></property>
  <property><name>fs.s3a.path.style.access</name><value>true</value></property>
  <property><name>fs.s3a.connection.ssl.enabled</name><value>false</value></property>
  <property><name>fs.s3a.aws.credentials.provider</name>
            <value>com.amazonaws.auth.EnvironmentVariableCredentialsProvider</value></property>
  ```
- **Flink** (`flink-conf.yaml` / `config.yaml`): `s3.endpoint`, `s3.path.style.access: true`, `s3.access-key`,
  `s3.secret-key` (or the environment).
- **Trino**: the server reads S3, from its catalogs' properties files, which no command line or `-D` reaches; sonde
  cannot write them for it. Said once in the catalog, with Trino's `${ENV:…}`, every `sonde run -- bin/launcher run`
  then goes through sonde (its keys come from the environment, new each run):

  ```properties
  fs.native-s3.enabled=true
  s3.endpoint=${ENV:AWS_ENDPOINT_URL}
  s3.region=${ENV:AWS_REGION}
  s3.path-style-access=true
  ```
  The legacy file system: `hive.s3.endpoint`, `hive.s3.path-style-access=true`, `hive.s3.ssl.enabled=false`.

Findings for s3a: FileOutputCommitter renames out of `_temporary/` (copy plus delete) get a warning with the S3A
committer settings; empty `…/` directory markers are reported with their delete cost.

## How a request is handled

1. Bucket and key from the path (`/bucket/key`), each `%XX` its byte (`a%2Fb` is the key `a/b`; bytes that are not
   UTF-8 are recorded as `%XX`); the query decoded as is (`+` stays).
2. Its kind named from method, path and query (`UploadPart`, `ListObjectsV2`, …).
3. It must be for sonde: a loopback Host (`127.0.0.1`, `localhost`, `*.localhost`, `::1`) unless `--allow-remote`,
   and a SigV4 (header or presigned) or SigV2 signature with this recording's keys, over header values in UTF-8
   (`x-amz-meta-name: café`, as sent). Else `403 SignatureDoesNotMatch`, recorded as refused, never forwarded.
4. The [policy](#what-a-recorded-program-may-do) may refuse it: `403 AccessDenied` naming the policy, recorded as
   refused. A path starting `//`, or a bucket `.` or `..`, gets `400 InvalidURI`.
5. The key goes on as the program wrote it, byte for byte: an encoded slash (`a%2Fb`), `%25`, `+`, bytes that are
   not UTF-8 (`%FF`), empty segments (`a//b`) and `.` and `..` segments (`a/../b`, a key of its own in S3) reach the
   store as sent, path-style or virtual-hosted. Re-signed (SigV4) for the real endpoint, over the path as a store
   reads it (decoded, then encoded once), in the bucket's region: a redirect that names another
   (`x-amz-bucket-region`, as AWS answers for a bucket elsewhere) is kept for the bucket, a request with no body is
   sent again signed for it (to that region's host, on AWS), and the bucket's next requests are signed for it. Bodies stream through:
   signed `aws-chunked` (`STREAMING-AWS4-HMAC-SHA256-PAYLOAD[-TRAILER]`) decoded as it arrives and sent unsigned (its
   trailing checksum carried in an unsigned `aws-chunked` body; with a trailer sonde does not know, or as a control
   document, read whole, up to 1 GiB, and sent with its SHA-256); `STREAMING-UNSIGNED-PAYLOAD-TRAILER` unchanged; a
   body without a length is buffered (S3 needs `Content-Length`).
6. A body is held to what its headers said. An `aws-chunked` payload shorter than `x-amz-decoded-content-length`, a
   body cut before its last chunk or its `Content-Length`, or a program gone part way gets `400 IncompleteBody`; a
   payload longer than said, or framing that does not read, `400 InvalidRequest`; a body that stops for the idle
   limit (60 s), `400 RequestTimeout`. The store is not sent the body's last bytes before it has ended as said, so
   it keeps no object of such an upload; the record says why (`transportError`).
7. A presigned URL with the recording's key is re-signed with headers (`x-sonde-note` on the record).
8. The response streams back unchanged; an error body's first 64 KiB is kept on the record.

A record holds the program's headers (signatures redacted), the URL sonde sent to, status, bytes each way, request
ids, the error body, **latency** (the whole exchange, until the body is through or the program stops reading) and
**first byte** (until response headers). Overhead: +0.2 ms at p50 for a 1 KiB GET ([performance](dev/performance.md)).

## What a recorded program may do

For `sonde run` (`--listen` too) and the app's command bar (and its trials). It is the only answer to "what may a program do";
sonde's own destructive acts are each a dry run, then `--yes` ([security](security.md#one-safety-model)).

| Policy | Allows | Refuses |
|---|---|---|
| `allow` (default) | everything: the program does with its data what it would without sonde | nothing |
| `no-delete` (a trial's default) | reads, listings, every write (over an object or not); multipart uploads; tags; a retention set or lengthened, a legal hold turned on; turning versioning on; every bucket setting not at right; removing what this recording made, and its ACL (below) | every other delete (`DELETE`, `POST ?delete`) and `AbortMultipartUpload`, `RenameObject`; what removes data later or lifts what guards it: `PUT ?lifecycle`, `PUT`/`DELETE ?policy`, `PUT ?acl` (a bucket's, or an object's the recording did not make), `PUT`/`DELETE ?publicAccessBlock`, `PUT ?ownershipControls`, suspending versioning and turning a legal hold off (a document that does not read as plainly `Enabled` / `ON`), any request with `x-amz-bypass-governance-retention` (a delete or a shorter retention, of its own objects too); any write sonde cannot name |
| `read-only` | GET and HEAD | everything else |

A program may remove what it made in the same recording: abort an upload it started, delete an object it made or
a version it wrote (during a trial, also one under a `{trial}` place of its command, and what the clean run made), a
`DeleteObjects` whose every object is such, a bucket it created. A write over an object goes through: with versioning
on, the store keeps the old version; without it, use `read-only` to keep what is there.

**An object is the recording's when its key held nothing before the write**, as the store's own answers show:

| The store answered | The key is the recording's to delete |
|---|---|
| 2xx to a write sent with `If-None-Match: *` | yes: it was created |
| 404 to a GET or HEAD of the key, then 2xx to a write | yes: nothing was there |
| 2xx with `x-amz-version-id` (the bucket keeps versions) | yes: a delete adds a marker, and only the version it wrote may be deleted by id |
| 2xx to any other write | no: on a bucket without versions the key may have stood before the run, and a write over it must not make it deletable |

sonde asks the store nothing of its own to find out (recording adds no request and no wait), so a program that writes
a temporary key without looking and deletes it is refused the delete under `no-delete`: give the trial its `{trial}`
place, or `--policy allow`. A store that ignores `If-None-Match` answers 2xx to a create over an object
([probe](probes.md) `cond.put-if-none-match` tells).

Which applies, first match: the flag (`--policy allow|no-delete|read-only`); `[record] policy` in `sonde.toml`;
`allow`, or `no-delete` for `--trial` (sonde's own faults could push a program down a path it would
not take). The app's Run and Run again take the same (`allow`, or the project's); a trial and Record… (no program) ask.

A request sonde refuses is said with its reason, a line each: a policy's names the flag that lifts it; S3 Express
and a path that names no bucket are no policy's.

## A run's file

- JSON lines: a header, then a record per completed request ([format](dev/run-format.md)), redacted. A crash
  or a full disk loses at most a torn last line, which is skipped.
- In `<data>/runs/` ([`sonde runs prune`](cli.md#read-a-run)), or `-o FILE`. Gzipped in place when finished (about
  a tenth; still `.sonde`); a live one stays plain (the app reads only what was appended).

```
172 requests (34 operations) from object_store 0.14 to adhoc in 467 ms, waiting 299 ms · 116 not found
```

GET and HEAD answered 404 are **not found**, not errors. Errors: other 4xx/5xx, transport failures, refused requests.

## Production traffic: access logs, CloudTrail, HAR

```sh
sonde import access.log      # S3 server access log
sonde import trail.json      # CloudTrail data events ({"Records": …} or one per line)
sonde import browser.har     # a HAR; gzip read directly
sonde import s3://logs/s3/  # where a store writes its logs: what arrived since the last import
```

Each becomes a run, read like a recording; the app opens them too (Open, or drop the file).

- Access-log lines that are not a program's request (lifecycle expiry, a batch delete's objects) and unreadable lines
  are skipped and counted.
- CloudTrail keeps upload id, part, listing page and version; HAR keeps headers (secrets masked).
- A 404 `NoSuchBucket` is an error; a missing key is a lookup.

| Source | Times | Who sent it | SDK |
|---|---|---|---|
| Access log | total and first byte | requester | yes |
| CloudTrail | none (no latency charts, nor findings of requests one after another) | IAM identity | yes |
| HAR | total | — | yes |

## Operations, not requests

`sonde show` groups requests into operations; `--requests` prints one line per request.

```
01:09:10.149  Iceberg  commit: table metadata v2 · 3 files (7.6 KiB)   20 reqs · 112 ms
00:22:25.074  SlateDB  looked for the newest WAL: probed ids 1–128        8 reqs · 5 ms
```

| Operation | Made of |
|---|---|
| multipart upload | Create, its parts, Complete or Abort (by upload id) |
| retried request | the same request repeated after a 5xx, 429, SlowDown or no response |
| lookups that found nothing | a burst of GET/HEAD answered 404 |
| listing in N pages | ListObjects calls continuing one another |
| format step | a commit point and the requests that led to it |

| Format | Commit point |
|---|---|
| SlateDB | a manifest, WAL, SST or compaction-state file |
| Iceberg | `*.metadata.json` |
| Delta | `_delta_log/<n>.json` |
| Lance | `_versions/<n>.manifest` |
| Hudi | a completed instant (`.hoodie/<instant>.commit`, `.deltacommit`, `.replacecommit`, …, or 1.x `.hoodie/timeline/`); `.requested`, `.inflight`, `.hoodie/.temp/` fold in; base and log files counted |
| Paimon | `snapshot/snapshot-<n>`, `LATEST`/`EARLIEST`, `schema/schema-<n>`; manifests and `bucket-<n>/` files fold in |

- A format's other requests fold into the next commit point; a completed multipart upload is one write.
- Lookups across numbered files (SlateDB's newest WAL) are their own step.
- Files with no commit within 2 s are reported as written without one.

**Findings** follow ([the kinds](cli.md#findings)). Notes common to nearly every step of a format are said once among
the operations; a retry, a lost race (412) and a slow request are in the operation and the charts, not findings.

Fixtures (`crates/analysis/tests/fixtures`): SlateDB 0.18 (172 requests → 34 operations), pyiceberg 0.9.1 (116 → 6),
delta-rs 1.6 (49 → 5), boto3, the Java SDK.

## Replay on another endpoint

Will the program work on store X, and what will it cost there?

```sh
sonde replay run.sonde -e ceph                             # the plan: what it would send, where
sonde replay run.sonde -e r2 --bucket data=scratch --yes   # replay, clean up, compare
```

Re-enacted (a run holds no object data):

- What the run read and had not written (its input) is written first under the prefix: a stand-in of the recorded
  size for each object it read, copied from or wrote on a condition it met, and empty `sonde-seed-N` keys where a
  listing found more. Reads of existing data then find it, and nothing differs falsely.
- Payloads: deterministic bytes of the recorded length. Control documents (DeleteObjects, tagging, …): as recorded,
  keys moved under the prefix, with the `Content-MD5` of what is sent.
- Upload ids, part ETags, ETags in `If-Match`/`If-None-Match` and continuation tokens map to the replay endpoint's;
  `CompleteMultipartUpload` is rebuilt from the replayed parts.
- Every key and listing prefix under `replay-<time>/` (or `--prefix`). Creating or deleting a bucket, bucket-level
  changes (lifecycle, policy) and requests naming a version id or a version or upload page marker are left out,
  and the plan says so.
- One request at a time, in order, no retries.
- A dry run first; `--yes` replays. It deletes what it wrote afterwards, always.
- Each bucket must be there, its prefix empty and listable, else nothing is sent.
- Never on the recorded keys: an empty prefix is refused.
- Compared by status and S3 error code, and for an answer that succeeded, what it carried: a listing's keys
  (counted on each record, `x-sonde-keys`), a read's bytes. 404 on both sides is the same. Exit 2 when anything differs.

Then what the program relies on: only the [probes](probes.md) that match features its requests used (checksum
algorithms and types, conditional headers, ranges, multipart, DeleteObjects size, SSE-C, presigned URLs, paged
listings), run against that store under `sonde-probe/` of the first bucket (removed after, audited), each with its
verdict. Last, each store's request cost at its vendor's prices. `endpoints test --probes` runs the whole catalogue.

```
✗ PutObject   recorded 412 PreconditionFailed → replay 200   ×1   e.g. data/log/0001.json  (if-none-match: *)

cost  0.00026 USD on rfs → 0.00023 USD on r2 (requests, at each store's prices)

What etl relies on, and how r2 answers
  ✓ pass  checksum.crc32.mpu-composite  CreateMultipartUpload ×1
  ✓ pass  cond.put-if-none-match        PutObject ×1
```

Tested: the SlateDB, pyiceberg, delta-rs and boto3 runs replay with no difference on RustFS and Ceph RGW 16.2.5, and
so does a boto3 ETL that reads 40 objects it did not write (RustFS).

In the app it is asked from a run: the run's ⋯ › On another store… shows the plan, runs it and opens the comparison:
grouped differences, what the program relies on, the cost on each store, and both runs as transcripts
(`Op bucket/key [conditions] → outcome`).

## Limits

- HTTP/1.1 between program and sonde; TLS only with `--tls`.
- S3 Express One Zone (directory buckets, `<name>--<zone>--x-s3`): `CreateSession` and every request to one get
  `501 NotImplemented`, never forwarded, whatever the policy.
- One target endpoint per run.
- No overall time limit per exchange; 60 s without a byte cuts it (a request body that stops is answered
  `400 RequestTimeout`, an answer that stops `502`). Connecting gives up after 10 s.
- `run` waits up to 5 s at exit for requests in flight.
- Replay is sequential: concurrency and timing are not reproduced.
- Length-less bodies, and signed `aws-chunked` ones with a trailer sonde does not know, are read whole: up to 1 GiB
  each, 2 GiB at once (more waits).
