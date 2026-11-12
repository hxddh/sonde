# Recording

sonde sits between a program and its object store and records every S3 request. The program talks to a local
endpoint with keys made for the run; sonde checks them, signs again with the real credentials, forwards the request,
streams the answer back and appends a record to a session file. The real credentials never reach the program.

```sh
sonde run -- python etl.py                  # run a command, record its S3 traffic
sonde run --live -- ./my-service            # print requests as they happen
sonde run --allow-writes -- python etl.py   # let it delete and write over what it finds
sonde proxy --listen 127.0.0.1:9000         # a standing endpoint for a program started elsewhere
sonde watch                                 # a proxy for hours: status line, new warnings, rotated files
sonde show <session>.sonde --errors
```

The endpoint is `-e <id>`, `--url <url>`, or the only one configured. Credentials are resolved as for every command
([credentials](credentials.md)).

## Run, in the app

The app's home does what `sonde run` does:

- Type a command (and a working directory if not your home), choose the endpoint and a policy, press Return. The
  policy starts at Settings › Safety (`no-delete`).
- The command runs through your shell in a pseudo-terminal (ConPTY on Windows): colours, progress bars and input
  work. Requests appear beside it as operations. Stop ends it; a notification says how it ended and the session opens.
- The program's output is kept in the session with its arrival times (`{"log": …}` lines). Choosing an operation
  highlights the lines printed while it ran, and the reverse. Escape codes are removed; a line redrawn with `\r`
  keeps its last state.
- Output is kept as printed: do not run commands that print secrets.

## What the program is given

| Variable | Value |
|---|---|
| `AWS_ENDPOINT_URL`, `AWS_ENDPOINT_URL_S3` | `http://127.0.0.1:<port>` |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | keys made for this recording (`SONDE…` / 64 hex digits); only a holder can use the endpoint |
| `AWS_REGION`, `AWS_DEFAULT_REGION` | the endpoint's region (`us-east-1` when none) |
| `AWS_ALLOW_HTTP`, `AWS_S3_FORCE_PATH_STYLE` | `true` |
| `AWS_VIRTUAL_HOSTED_STYLE_REQUEST` | `false` |
| `AWS_EC2_METADATA_DISABLED` | `true` |

- Removed: `AWS_SESSION_TOKEN`, `AWS_PROFILE`, `AWS_ROLE_ARN`, `AWS_WEB_IDENTITY_TOKEN_FILE` and the
  container-credential variables.
- A client that ignores these variables needs the endpoint set by hand (`sonde proxy` prints it), with path-style
  addressing or a `<bucket>.localhost` host.
- Tested in CI (`scripts/sdk-smoke.sh`): boto3, AWS SDKs for JavaScript v3, Go v2, Java v2, and object_store (Rust).

### TLS

`--tls` (or `[record] tls = true` in `sonde.toml`) also serves TLS on the same port; a connection that opens with a
TLS handshake gets TLS, any other plain HTTP. `AWS_ENDPOINT_URL` becomes `https://127.0.0.1:<port>`, and:

| Variable | Value |
|---|---|
| `AWS_CA_BUNDLE`, `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE`, `CURL_CA_BUNDLE` | `<data>/tls/bundle.pem`: the system roots plus sonde's authority |
| `NODE_EXTRA_CA_CERTS` | `<data>/tls/ca.pem`: sonde's authority alone |

- The first run makes an authority, issues one certificate (`localhost`, `*.localhost`, `127.0.0.1`, `::1`) and
  discards the authority's key, so nothing else can be issued.
- The certificate's key is `tls/key.pem` (0600); both are renewed after about two years.
- HTTP/1.1 only.

### Spark, Flink, Trino and Hadoop s3a

JVM engines read the endpoint from their own configuration, not `AWS_ENDPOINT_URL`. `sonde run` adds it to the
command line of the engines it knows, before the command's own arguments (which win; sonde warns when they name
another endpoint):

| Command | sonde adds |
|---|---|
| `spark-submit`, `spark-shell`, `pyspark`, `spark-sql`, `sparkR` | `--conf spark.hadoop.fs.s3a.endpoint=http://127.0.0.1:<port>`, `…endpoint.region`, `…path.style.access=true`, `…connection.ssl.enabled=false`, `…aws.credentials.provider=com.amazonaws.auth.EnvironmentVariableCredentialsProvider` |
| `flink run`, `flink run-application` | the same as `-Ds3.endpoint=…`, `-Ds3.path.style.access=true`, … |
| any command | `PYSPARK_SUBMIT_ARGS` with the same `--conf`s, before what it already held |

- Keys are never put on a command line; the provider reads them from the environment.
- Engines get the plain HTTP endpoint even with `--tls`.
- Only a driver on this machine reaches 127.0.0.1: Spark `local[*]` or client mode, a local Flink cluster.

By hand (a cluster, a server, an app embedding Spark), with the port and keys `sonde proxy` prints
(`--listen 127.0.0.1:9000` fixes the port; keys are new each time):

- **Spark**: the `spark.hadoop.fs.s3a.*` settings above. `s3://` paths also need
  `spark.hadoop.fs.s3.impl=org.apache.hadoop.fs.s3a.S3AFileSystem`. A bucket's `fs.s3a.bucket.<name>.endpoint`
  overrides the global one. Iceberg `S3FileIO` reads `AWS_ENDPOINT_URL` (Java SDK 2.21+) but needs
  `spark.sql.catalog.<name>.s3.path-style-access=true`.
- **Hadoop** (`core-site.xml`: Hive, `hadoop fs`, Flink's hadoop file system):

  ```xml
  <property><name>fs.s3a.endpoint</name><value>http://127.0.0.1:9000</value></property>
  <property><name>fs.s3a.path.style.access</name><value>true</value></property>
  <property><name>fs.s3a.connection.ssl.enabled</name><value>false</value></property>
  <property><name>fs.s3a.aws.credentials.provider</name>
            <value>com.amazonaws.auth.EnvironmentVariableCredentialsProvider</value></property>
  ```
- **Flink** (`flink-conf.yaml` / `config.yaml`): `s3.endpoint: http://127.0.0.1:9000`, `s3.path.style.access: true`,
  `s3.access-key`, `s3.secret-key` (or the environment).
- **Trino** (the server reads S3; start it after `sonde proxy`). Native S3 (`fs.native-s3.enabled=true`):
  `s3.endpoint`, `s3.path-style-access=true`, `s3.region`, `s3.aws-access-key`, `s3.aws-secret-key`. Legacy:
  `hive.s3.endpoint`, `hive.s3.path-style-access=true`, `hive.s3.ssl.enabled=false`, `hive.s3.aws-access-key`,
  `hive.s3.aws-secret-key`.

s3a's habits are reported: a rename is a copy plus a delete, so Hadoop's FileOutputCommitter copying out of
`_temporary/` gets a warning with the S3A committer settings; empty `…/` objects are directory markers, with their
delete cost.

## How a request is handled

1. Bucket and key come from the path (`/bucket/key`) or a `<bucket>.localhost` Host; the query is decoded as is
   (`+` stays a plus).
2. The operation is named from method, path and query (`UploadPart`, `ListObjectsV2`, …).
3. The request must be for sonde: a loopback Host (`127.0.0.1`, `localhost`, `*.localhost`, `::1`) unless
   `--allow-remote`, and a SigV4 (header or presigned) or SigV2 signature made with this recording's keys.
   Otherwise: `403 SignatureDoesNotMatch`, recorded as refused, never forwarded.
4. The policy may refuse it ([below](#what-a-recorded-program-may-do)): `403 AccessDenied` naming the policy,
   recorded as refused. A path starting with an empty segment (`//bucket/key`) gets `400 InvalidURI`.
5. The client's signature is dropped and the request signed again (SigV4) for the real endpoint. Bodies stream
   through, except:
   - signed `aws-chunked` (`STREAMING-AWS4-HMAC-SHA256-PAYLOAD[-TRAILER]`): decoded (up to 1 GiB), sent with its
     SHA-256, trailing checksums turned into headers;
   - a body without a length (chunked transfer): buffered, as S3 needs `Content-Length`;
   - `STREAMING-UNSIGNED-PAYLOAD-TRAILER`: passed through unchanged (not yet covered by the SDK tests).
6. A presigned URL made with the recording's key is re-signed with headers; the record notes it (`x-sonde-note`).
7. The response streams back unchanged. Error bodies are read whole and kept.

`sonde proxy` listens on loopback only; `--allow-remote` opens it to other machines, for whoever holds the printed keys.

A record holds the headers the program sent (signatures redacted), the URL sonde sent to, status, bytes each way,
request ids, the error body, and two times: **latency** (the whole exchange, until the body is through or the
program stops reading) and **first byte** (until response headers). Overhead against a local RustFS: +0.2 ms at
p50 for a 1 KiB GET ([performance](performance.md)).

## What a recorded program may do

The policy applies to `sonde run`, `sonde proxy`, `sonde watch` and the app's Run.

| Policy | Allows | Refuses |
|---|---|---|
| `no-delete` (default) | reads, listings, new objects, writes over objects this recording wrote; multipart uploads; tags, ACLs, retention; turning versioning on | every delete (`DELETE`, `POST ?delete`), `AbortMultipartUpload`, `PUT ?lifecycle`, `PUT`/`DELETE ?policy`, suspending versioning, a write (`PutObject`, `CompleteMultipartUpload`, `CopyObject`) over an object that existed before the recording, a write with `If-Match`, any write sonde cannot name |
| `read-only` | GET and HEAD | everything else |
| `allow` | everything | nothing |

How `no-delete` finds an existing object:

- `PutObject` and `CompleteMultipartUpload` without a condition are sent with `If-None-Match: *` (not recorded);
  the store's `412` becomes sonde's `403` naming the key.
- `CopyObject`'s target is checked first with a `HEAD` of sonde's own (not recorded); an answer other than found or
  not found refuses the copy.
- A store that ignores `If-None-Match` (older MinIO and Ceph, some gateways) overwrites anyway: use `read-only`.
- A recording may rewrite what it wrote itself, including a trial's runs after its clean run. A program run again
  over its previous run's output is refused: use `--allow-writes`.

Which policy applies, first match:

1. the flag: `--policy allow|no-delete|read-only`, `--allow-writes`, `--no-delete`, `--read-only`;
2. `[record] policy` in the project's `sonde.toml` (sonde warns on stderr when it allows deletes);
3. `no-delete`.

In the app, Run's choice starts at the `record-policy` setting; `[record] policy` may narrow it, never widen it.
The `read-only` *setting* governs sonde's own actions (aborts, deletes, lifecycle, probes, replays, app trials), not
a recorded program.

## Session files

- JSON lines: a header, then one record per completed request ([session-format](session-format.md)). A crash loses
  at most a torn last line, which is skipped.
- Records are redacted before writing; no secrets.
- Default location: `<data>/sessions/`; `-o FILE` elsewhere.
- A finished session is gzipped in place (about a tenth of the size; still `.sonde`). A live one stays plain; the app
  reads only what was appended since its last look.
- `sonde sessions` lists them; `sonde sessions prune --keep N --yes` keeps the newest N and every baseline.
- `sonde watch` writes a new file every `--rotate` minutes.
- `sonde show` prints the timeline and summary; `--format json` gives header, summary and every record.

Summary line:

```
172 requests to rustfs in 0.5 s · 116 not found · ↑36.5 KiB ↓22.7 KiB
```

GET and HEAD answered 404 count as **not found**, not errors (formats probe for keys that don't exist yet). Errors
are other 4xx/5xx, transport failures and refused requests.

## Production traffic: access logs, CloudTrail, HAR

```sh
sonde import access.log      # S3 server access log
sonde import trail.json      # CloudTrail data events ({"Records": …} or one event per line)
sonde import browser.har     # a HAR (a browser's, or sonde's --log-out)
sonde import trail.json.gz   # gzip is read directly
```

Each becomes a session (`<data>/sessions/` or `-o FILE`), read like a recording; the app opens them too (Open, or
drop the file).

- Access-log lines that are not a program's request (lifecycle expiry, a batch delete's objects) are left out;
  unreadable lines are skipped. `sonde import` reports both counts.
- CloudTrail keeps upload id, part, listing page and version; HAR keeps headers (secrets masked).
- A 404 `NoSuchBucket` is an error; a missing key is a lookup.

| Source | Times | Who sent it | SDK |
|---|---|---|---|
| Access log | total and first byte | requester | yes |
| CloudTrail | none (no latency charts or findings) | IAM identity | yes |
| HAR | total | — | yes |

## Operations, not requests

`sonde show` groups requests into operations; `--requests` prints one line per request.

```
01:09:10.149  Iceberg  commit: table metadata v2 · 3 files (7.6 KiB)   20 reqs · 112 ms
01:09:10.597  Iceberg  1 file written, no commit followed (72 B)         3 reqs · 18 ms
00:22:25.074  SlateDB  looked for the newest WAL: probed ids 1…128        8 reqs · 5 ms
00:22:25.085  SlateDB  WAL 1 written empty (fencing marker)              2 reqs · 9 ms
```

| Operation | Made of |
|---|---|
| multipart upload | Create, its parts, Complete or Abort (by upload id) |
| retried request | the same request repeated after a 5xx, 429, SlowDown or no response |
| lookups that found nothing | a burst of GET/HEAD answered 404 |
| listing in N pages | ListObjects calls continuing one another |
| format step | a commit point and the requests that led to it |

Commit points, recognized from keys:

| Format | Commit point |
|---|---|
| SlateDB | a manifest, WAL, SST or compaction-state file |
| Iceberg | `*.metadata.json` |
| Delta | `_delta_log/<n>.json` |
| Lance | `_versions/<n>.manifest` |
| Hudi | a completed instant (`.hoodie/<instant>.commit`, `.deltacommit`, `.replacecommit`, …, or 1.x `.hoodie/timeline/`); `.requested`, `.inflight` and `.hoodie/.temp/` markers fold in; base and log files are counted |
| Paimon | `snapshot/snapshot-<n>`, `LATEST`/`EARLIEST`, `schema/schema-<n>`; manifests and `bucket-<n>/` files fold in |

- A format's other requests (reads, listings, data files, Iceberg manifests) fold into the next commit point; a
  completed multipart upload counts as one write.
- Lookups across numbered files (SlateDB finding its newest WAL) are a step of their own.
- Files with no commit within 2 s are reported as written without one.

**Observations** follow the operations: uploads never completed, files written without a commit, retries,
conditional writes that lost a race (412), keys read repeatedly, prefixes listed repeatedly, small files sent as
multipart, and notes that apply to nearly every step of a format (said once).

Tested sessions (`crates/proxy/tests/fixtures`): SlateDB 0.18 (172 requests → 34 operations), pyiceberg 0.9.1
(116 → 6), delta-rs 1.6 (49 → 5), boto3, the Java SDK.

## Replay on another endpoint

```sh
sonde replay session.sonde -e ceph                         # plan, confirm, replay, clean up, compare
sonde replay session.sonde -e r2 --bucket data=scratch --yes
sonde diff session.sonde replay.sonde                      # compare two files later
```

```
Replay 116 requests from iceberg.sonde on ceph, under replay-20261107-015056/ in lake.
  63 writes and 0 deletes, all under that prefix: nothing outside it is touched.
116 compared: 116 the same
```

How requests are re-enacted (a session holds no object data):

- Payloads are deterministic bytes of the recorded length.
- Control documents (DeleteObjects, tagging, …) are the recorded ones, keys moved under the prefix.
- Upload ids, part ETags, ETags in `If-Match`/`If-None-Match` and continuation tokens map to the replay endpoint's;
  `CompleteMultipartUpload` is rebuilt from the replayed parts.
- Every key and listing prefix goes under `replay-<time>/` (or `--prefix`). Bucket-level changes (lifecycle,
  policy, deleting the bucket) and requests naming a version id are left out.
- One request at a time, in recorded order, no retries.

Options and rules:

- It asks before sending (`--yes` in scripts) and deletes what it wrote afterwards (`--keep` keeps it).
- The prefix must be empty and listable (or its bucket absent); otherwise the replay stops before sending anything.
- `--in-place` replays keys as recorded, bucket changes included: scratch buckets only.
- Requests compare by status and S3 error code. 404 lookups on both sides are the same. `CreateBucket` answered
  "already owned by you" is counted apart.
- Exit 2 when anything differs, else 0.

Differences are grouped, with the original request's conditions:

```
✗ PutObject   recorded 412 PreconditionFailed → replay 200   ×1   e.g. data/log/0001.json  (if-none-match: *)
```

(This store ignores conditional creates: two writers could both think they won.)

Tested: the SlateDB, pyiceberg, delta-rs and boto3 sessions recorded on RustFS replay with no difference on RustFS
and Ceph RGW 16.2.5.

In the app, Replay shows the plan in a dialog (refused while read-only), runs it and opens the comparison: grouped
differences and both sessions side by side as transcripts (`Op bucket/key [conditions] → outcome`, prefix removed).
Compare does the same for any two sessions.

## Share a session

```sh
sonde share                              # newest session → ./<session>-share/
sonde share run.sonde -o /tmp/site       # to a folder
sonde share --serve                      # http://127.0.0.1:<port>/ until Ctrl-C
sonde share --json -o session.json       # only the computed document
sonde share --html --redact -o run.html  # one HTML file, names replaced
```

- A share is a static site: sonde's viewer (embedded in the binary) and a `session.json` with header, summary,
  operations, observations, records and output, all precomputed. It opens in any browser; nothing runs server-side.
- It holds what the session file holds (redacted records, program output): share it as you would the file.
- `--redact` replaces bucket and key names with tokens (same name, same token; `_delta_log`, `metadata`, numbered
  files and extensions kept, so findings still read) and drops headers, bodies, URLs, request ids, output and arguments.
- `--html` is one self-contained file: figures, findings (linked to operations), where the time went, a timeline and
  every operation at `#op-N`, light and dark.

## Limits

- HTTP/1.1 between program and sonde; TLS only with `--tls`.
- S3 Express One Zone (directory buckets, `<name>--<zone>--x-s3`): `CreateSession` and every request to a directory
  bucket get `501 NotImplemented` and never reach the store.
- One target endpoint per proxy.
- No overall time limit per exchange; one that stalls for 60 s is cut. Connecting gives up after 10 s.
- `run`, `proxy` and `watch` wait up to 5 s at exit for requests in flight.
- Replay is sequential: original concurrency and timing are not reproduced.
- Signed `aws-chunked` and length-less bodies are read whole: up to 1 GiB each, 2 GiB at once (more waits).
