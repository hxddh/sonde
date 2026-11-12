# Recording: Run, `sonde run`, `sonde proxy`, `sonde show`, `sonde share`

sonde can sit between an application and its object store and record every S3
request the application makes. The application talks to a local endpoint with
throwaway credentials; sonde re-signs each request with the real endpoint's
credentials, forwards it, streams the response back, and writes a session file.

```sh
sonde run -- python etl.py                     # run a command with its S3 traffic recorded
sonde run --live -- ./my-service               # print requests as they happen
sonde run --allow-writes -- python etl.py      # let it delete and write over what it finds
sonde proxy --listen 127.0.0.1:9000            # a standing endpoint for an app started elsewhere
sonde show ~/…/sessions/20261107-002225-etl.py.sonde --errors
```

The endpoint is chosen as for every command: `-e <id>`, `--url <url>`, or the
only one configured. Credentials are resolved as usual (profiles, SSO, IMDS,
IRSA, …) and stay in sonde; the application never receives them.

## Run, in the app

The app's home does what `sonde run` does, with the program in a terminal:
type a command (and a working directory if it is not your home), choose the
endpoint and a policy (Settings › Safety sets the one it starts with:
block deletes unless changed), press Return. The command runs
through your shell in a pseudo-terminal (ConPTY on Windows) — colours,
progress bars and prompts work, and you can type into it. Its requests appear
beside it as operations while it runs; Stop ends it. When it ends, a
notification says how, and the session opens.

The session keeps the program's output with the time each piece arrived
(`{"log": …}` lines in the file), so Sessions can line output and requests up:
choose an operation and the lines printed while it ran are highlighted; choose
a line and the operations running then are. Escape codes are removed and a
line redrawn with a carriage return keeps its last state. Output is in the
session as printed — do not run commands that print secrets.



| Variable | Value |
|---|---|
| `AWS_ENDPOINT_URL`, `AWS_ENDPOINT_URL_S3` | `http://127.0.0.1:<port>` |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | keys made for this recording (`SONDE…` / 64 random hex digits): only a program that has them can use the endpoint's credentials |
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
3. sonde checks it is meant for it: the Host must be a loopback name
   (`127.0.0.1`, `localhost`, `*.localhost`, `::1`) unless `--allow-remote`,
   and the request must carry a SigV4 signature (header or presigned query),
   or the SigV2 one older clients and boto3's presigned URLs use,
   made with this recording's keys. Anything else — an unsigned request, other
   keys, a web page rebinding a name to 127.0.0.1 — gets `403
   SignatureDoesNotMatch`, is recorded as refused, and never reaches the store.
   `sonde proxy` listens on loopback only; `--allow-remote` opens it to other
   machines, for whoever holds the keys it prints.
4. The recording's policy may refuse it ([below](#what-a-recorded-program-may-do)).
   A refused request gets an S3 `403 AccessDenied` error naming the policy,
   and is recorded as refused.
   Under every policy, a path that starts with an empty segment (`//bucket/key`)
   is answered `400 InvalidURI`: it names no bucket.
5. The client's signature headers are dropped and the request is signed again
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
6. **Presigned URLs** made with the recording's key are re-signed with headers; the
   record says so (`x-sonde-note`).
7. The response streams back unchanged. Error bodies are read whole and kept in
   the record.

What a record holds: the headers **the application sent** (signatures
redacted, as everywhere in sonde), the URL sonde sent to, status, bytes each
way, request ids, the error body, and two times:

- **latency**: the whole exchange, until the body has gone through (or the
  program stopped reading it);
- **first byte**: until the response headers arrived.

The overhead measured against a local RustFS: +0.2 ms at p50 for a 1 KiB GET
(`cargo run --release -p sonde-proxy --example overhead`; docs/performance.md).

## What a recorded program may do

The program reaches the store with sonde's credentials, so the recording
decides what it may do there. `sonde run`, `sonde proxy`, `sonde watch` and
the app's Run take one of three policies:

| Policy | Lets through | Refuses |
|---|---|---|
| `no-delete` (the default) | reads, listings, new objects, and writes over an object this recording wrote itself; uploads in parts; tags, ACLs, retention, versioning turned on | every delete (`DELETE`, `POST ?delete`, `DeleteObjects`), `AbortMultipartUpload`, lifecycle rules (`PUT ?lifecycle`), bucket policies (`PUT`/`DELETE ?policy`), versioning suspended, a write (`PutObject`, `CompleteMultipartUpload`, `CopyObject`) over an object that was there before the recording and that it did not write, a write on `If-Match`, and any write sonde cannot name |
| `read-only` | GET and HEAD | everything else |
| `allow` | everything | nothing |

An object already there is found by the store itself: a `PutObject` or a
`CompleteMultipartUpload` that says no condition is sent with
`If-None-Match: *` (not on the record: the program did not ask it), and the
store's `412` comes back as sonde's `403` naming the key. A `CopyObject`'s
target is looked for first with a `HEAD` of sonde's own (not recorded, not
timed); an answer other than found or not found refuses the copy. A store that
ignores `If-None-Match` (older MinIO and Ceph releases, some gateways) writes
over the object all the same: with such a store use `read-only`.

What the recording wrote, it may write again: a program that writes a key
twice, and a trial's runs after its clean run (the runs of `--trial` share
what they wrote; the app's trial takes what the clean run's session wrote).
A program run again writes over what its last run wrote: that object was
there before this recording, so it is refused under `no-delete` —
`--allow-writes` (or `[record] policy = "allow"`) is for a program that does.

The policy is, the first that says one: the flag (`--policy allow|no-delete|read-only`,
or `--allow-writes`, `--no-delete`, `--read-only`), then `[record] policy` in
the project's `sonde.toml`, then `no-delete`. sonde says on stderr when
`sonde.toml` lets a program delete. In the app the policy is Run's choice,
which starts at the `record-policy` setting (Settings › Safety); a project's
`[record] policy` may narrow it there, never widen it.

`read-only` mode (the setting) is about what sonde does itself — aborts,
deletes, lifecycle rules, probes, replays, trials from the app — not what a
recorded program does: that is the policy's.

## Session files

`.sonde` files are JSON lines: a header (`format: "sonde-session"`, version,
creation time, endpoint id and URL, region, the command) and then one record
per line, appended as requests complete ([session-format.md](session-format.md)
says each field, and what a reader may expect of a later version) — a session survives a crash, and a
torn last line is skipped on reading. They hold no secrets: records are
redacted before they are written. Without `-o`, sessions go to the data
directory's `sessions/`. A finished session is gzip-compressed in place (a
tenth of its size or less; still a `.sonde` file every command and the app
open); a live one stays plain while it is appended to, and the app reads only
what it appended since its last look. `sonde sessions` lists them,
`sonde sessions prune --keep N --yes` keeps the newest N (and every baseline).

`sonde watch` keeps a proxy running for hours: a status line for the last
minute, each new warning once, and a new session file every `--rotate` minutes.

`sonde show` prints the timeline and a summary; `--format json` gives the
header, the summary and every record.

## Production traffic: access logs, CloudTrail, HAR

```sh
sonde import access.log            # S3 server access log lines
sonde import trail.json            # CloudTrail data events ({"Records": …}, or one event a line)
sonde import browser.har           # a HAR (a browser's, or sonde's own --log-out)
sonde import trail.json.gz         # gzip is read as it is
```

Each becomes a session in the data directory (or `-o FILE`), read like a recording: operations, findings, where
the time went. The app opens them too (Open, or drop the file on the window).

An access log's lines that are no request of a program's (a lifecycle expiry, a batch delete's objects) are left out,
and a line that cannot be read (a file cut off) is skipped: `sonde import` says how many of each. CloudTrail keeps
which upload, part, listing page and version a request named; a HAR keeps the request's headers (secrets masked).

| Source | Times | Who sent it | SDK (user agent) |
|---|---|---|---|
| Access log | total and turnaround (first byte) | requester | yes |
| CloudTrail | none | IAM identity | yes |
| HAR | total | — | yes |

CloudTrail has no request times, so its sessions show no latency charts, waiting or latency findings. A 404 whose
code is `NoSuchBucket` counts as an error, a missing key as a lookup.

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
`*.metadata.json`; a Delta `_delta_log/<n>.json`; a Lance
`_versions/<n>.manifest`. The format's other requests —
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

## Replay on another endpoint

```sh
sonde replay session.sonde -e ceph          # plan, confirm, replay, clean up, compare
sonde replay session.sonde -e r2 --bucket data=scratch --yes
sonde diff session.sonde replay.sonde       # compare two files again later
```

```
Replay 116 requests from iceberg.sonde on ceph, under replay-20261107-015056/ in lake.
  63 writes and 0 deletes, all under that prefix: nothing outside it is touched.
116 compared: 116 the same
```

A session keeps no object data, and upload ids, ETags and continuation tokens
differ from one endpoint to another, so a replay re-enacts each request:

- payloads are deterministic bytes of the recorded length;
- control documents (DeleteObjects, tagging, …) are the recorded ones — sonde
  keeps them on records — with their keys moved under the replay prefix;
- upload ids, part ETags, ETags in `If-Match` / `If-None-Match` and
  continuation tokens are mapped to what the replay endpoint returned, and
  `CompleteMultipartUpload` is rebuilt from the replayed parts;
- every key and listing prefix is moved under `replay-<time>/` (or
  `--prefix`), so nothing that exists is touched; bucket-level changes
  (lifecycle, policy, deleting the bucket) are left out; requests naming a
  version id of the original endpoint are left out;
- requests go one at a time, in recorded order, without retries.

It asks before sending anything (`--yes` in scripts) and deletes what it wrote
afterwards (`--keep` keeps it). Because that deletes everything under the
prefix, the prefix must be empty first: a prefix that holds objects, or one
that cannot be listed (anything but a bucket that is not there yet), stops
the replay before it sends a request. `--in-place` replays keys as recorded, bucket
changes included — on a scratch bucket only.

Requests are compared by status and S3 error code; lookups that found nothing
on both sides are the same. A `CreateBucket` answered "already owned by you" is
counted apart (the bucket was there before). Differences are grouped, with the
conditions the original request carried:

```
✗ PutObject   recorded 412 PreconditionFailed → replay 200   ×1   e.g. data/log/0001.json  (if-none-match: *)
```

That line is a store ignoring conditional creates — two writers could both
believe they won. The exit status is 2 when anything differs, 0 otherwise.

Checked on real recordings: the SlateDB, pyiceberg, delta-rs and boto3
sessions recorded on RustFS replay with no difference on RustFS and on Ceph RGW
16.2.5.

In the app, Replay on a session shows the same plan in a dialog (refused while
sonde is read-only), runs it, and opens the comparison: the differences
grouped, and both sessions as transcripts side by side (`Op bucket/key
[conditions] → outcome`, the replay prefix taken off). Compare does the same
for any two sessions.

## Share a session

```sh
sonde share                              # the newest session → ./<session>-share/
sonde share run.sonde -o /tmp/site       # a session, to a folder
sonde share --serve                      # on http://127.0.0.1:<port>/ until Ctrl-C
sonde share --json -o session.json       # only the computed document
sonde share --html --redact -o run.html  # one HTML file, names replaced: safe to send outside
```

A shared session is a static web site: sonde's viewer (Native Shell's web
target, embedded in the binary) and a `session.json` with everything already
computed — header, summary, operations, observations, every record and the
output lines. Anyone with a browser reads it with the app's views; nothing
runs on a server, and nothing is installed. It holds what the session file
holds (redacted records, the program's output), so share it as you would the
file — or with `--redact`, which replaces bucket and key names with tokens
(the same name, the same token; `_delta_log`, `metadata`, numbered files and
file types kept, so the findings still read) and drops headers, bodies, URLs,
request ids, the program's output and its arguments.

`--html` is the session as one file with nothing to serve: its figures, its
findings (each linking to its operations), where the time went, a timeline
and every operation at its own `#op-N` link, in light and dark.



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
- No time limit on an exchange as a whole (a large body takes what it takes); one that stops moving for 60 s is cut,
  and connecting gives up after 10 s.
- `run`, `proxy` and `watch` wait up to 5 s at the end for requests still in flight, so they are in the session.
- Replay is sequential: concurrency and timing of the original are not reproduced.
- Signed `aws-chunked` bodies (and bodies sent chunked without a length) are read whole, up to 1 GiB each
  and 2 GiB at once: a request past that budget waits for the ones before it.
