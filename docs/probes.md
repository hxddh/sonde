# Compatibility probes

Generated from the probe catalog (`sonde probe list --format json`) by `scripts/gen-probe-docs.py`.

Each probe runs in a scratch prefix `sonde-probe/<run>/<probe>/` of the bucket you give (`--bucket`), and, with
`--temp-buckets`, in temporary buckets of its own. Everything it wrote is removed afterwards: every version,
delete marker and incomplete upload under the prefix, and the temporary buckets (Object Lock versions with
`x-amz-bypass-governance-retention`). A cancelled run cleans up after the probe it stopped in before it ends.
What could not be removed (a locked version without `s3:BypassGovernanceRetention`, a bucket that is not empty)
is named in the probe's observations (`left behind: …`), in the job's last message and in the audit log.

Verdicts: **pass** (as AWS), **fail** (a guarantee broke: data written that should not be, wrong data read),
**differs** (works, differently from AWS), **unsupported** (not implemented), **error** (could not run: the
probe timed out, or a request it only prepares with failed, as in a scratch bucket that cannot be written),
**skipped** (needs a temporary bucket or real-time waits that were not allowed).

The judge of every probe is a pure function of the recorded responses, unit-tested on fixtures recorded from
Ceph RGW and RustFS and on AWS-reference fixtures (`crates/probes/fixtures`).

| Probe | Category | Title |
|---|---|---|
| [`cond.put-if-none-match`](#condput-if-none-match) | conditionalWrites | PUT If-None-Match: * |
| [`cond.put-if-match`](#condput-if-match) | conditionalWrites | PUT If-Match |
| [`cond.copy-if-match`](#condcopy-if-match) | conditionalWrites | Conditional CopyObject |
| [`cond.complete-if-none-match`](#condcomplete-if-none-match) | conditionalWrites | Conditional CompleteMultipartUpload |
| [`consistency.list-after-write`](#consistencylist-after-write) | consistency | List after write |
| [`consistency.read-after-overwrite`](#consistencyread-after-overwrite) | consistency | Read after overwrite |
| [`consistency.read-after-delete`](#consistencyread-after-delete) | consistency | Read after delete |
| [`checksum.crc32.single`](#checksumcrc32single) | checksums | CRC32 on PutObject |
| [`checksum.crc32.mpu-composite`](#checksumcrc32mpu-composite) | checksums | CRC32 multipart, composite |
| [`checksum.crc32.mpu-full-object`](#checksumcrc32mpu-full-object) | checksums | CRC32 multipart, full-object |
| [`checksum.crc32c.single`](#checksumcrc32csingle) | checksums | CRC32C on PutObject |
| [`checksum.crc32c.mpu-composite`](#checksumcrc32cmpu-composite) | checksums | CRC32C multipart, composite |
| [`checksum.crc32c.mpu-full-object`](#checksumcrc32cmpu-full-object) | checksums | CRC32C multipart, full-object |
| [`checksum.crc64nvme.single`](#checksumcrc64nvmesingle) | checksums | CRC64NVME on PutObject |
| [`checksum.crc64nvme.mpu-composite`](#checksumcrc64nvmempu-composite) | checksums | CRC64NVME multipart, composite |
| [`checksum.crc64nvme.mpu-full-object`](#checksumcrc64nvmempu-full-object) | checksums | CRC64NVME multipart, full-object |
| [`checksum.sha1.single`](#checksumsha1single) | checksums | SHA1 on PutObject |
| [`checksum.sha1.mpu-composite`](#checksumsha1mpu-composite) | checksums | SHA1 multipart, composite |
| [`checksum.sha256.single`](#checksumsha256single) | checksums | SHA256 on PutObject |
| [`checksum.sha256.mpu-composite`](#checksumsha256mpu-composite) | checksums | SHA256 multipart, composite |
| [`checksum.trailing`](#checksumtrailing) | checksums | Trailing checksum (aws-chunked) |
| [`mpu.min-part-size`](#mpumin-part-size) | multipart | Minimum part size |
| [`mpu.out-of-order`](#mpuout-of-order) | multipart | Parts out of order |
| [`mpu.duplicate-complete`](#mpuduplicate-complete) | multipart | Complete twice |
| [`mpu.concurrent-same-key`](#mpuconcurrent-same-key) | multipart | Concurrent uploads of one key |
| [`mpu.upload-part-copy`](#mpuupload-part-copy) | multipart | UploadPartCopy |
| [`range.bounds`](#rangebounds) | range | Range boundaries |
| [`range.multi`](#rangemulti) | range | Multiple ranges |
| [`range.unsatisfiable`](#rangeunsatisfiable) | range | Range past the end |
| [`keys.special`](#keysspecial) | keys | Special characters and Unicode in keys |
| [`list.token-stability`](#listtoken-stability) | listing | Continuation token stability |
| [`list.start-after`](#liststart-after) | listing | ListObjectsV2 StartAfter |
| [`delete.batch-limit`](#deletebatch-limit) | delete | DeleteObjects batch limit |
| [`presign.expiry`](#presignexpiry) | presign | Presigned URL expiry |
| [`presign.clock-skew`](#presignclock-skew) | presign | Clock skew tolerance |
| [`meta.size-limit`](#metasize-limit) | metadata | User metadata size limit |
| [`version.id-format`](#versionid-format) | versioning | Version ID format |
| [`lock.retention`](#lockretention) | objectLock | Object Lock retention |
| [`sse-c`](#sse-c) | encryption | SSE-C |
| [`express.append`](#expressappend) | express | S3 Express append (x-amz-write-offset-bytes) |

## conditionalWrites

### `cond.put-if-none-match`

**PUT If-None-Match: *.** Create-only writes: a PUT with If-None-Match: * succeeds on a new key and is refused when the key exists.

**AWS behavior:** First PUT 200; second PUT 412 PreconditionFailed (409 ConditionalRequestConflict while a concurrent write is in flight). The object keeps its first body.

### `cond.put-if-match`

**PUT If-Match.** Compare-and-swap writes: a PUT with If-Match succeeds only while the object still has that ETag.

**AWS behavior:** PUT with a stale ETag: 412 PreconditionFailed; with the current ETag: 200. A missing key with If-Match: 404 NoSuchKey.

### `cond.copy-if-match`

**Conditional CopyObject.** CopyObject with x-amz-copy-source-if-match (on the source) and If-None-Match: * (on the destination).

**AWS behavior:** Wrong source ETag: 412; destination exists with If-None-Match: *: 412; otherwise 200.

### `cond.complete-if-none-match`

**Conditional CompleteMultipartUpload.** CompleteMultipartUpload with If-None-Match: * when the key was created in the meantime.

**AWS behavior:** 412 PreconditionFailed; the existing object is kept. The upload can still be aborted.

## consistency

### `consistency.list-after-write`

**List after write.** Writes 20 keys, then lists them at once.

**AWS behavior:** Strong consistency (since December 2020): every acknowledged PUT appears in the next listing.

### `consistency.read-after-overwrite`

**Read after overwrite.** Overwrites a key and reads it at once, five times.

**AWS behavior:** Strong read-after-write: every GET returns the latest body.

### `consistency.read-after-delete`

**Read after delete.** Deletes a key and reads it at once.

**AWS behavior:** GET and HEAD answer 404 NoSuchKey right after DeleteObject; the key leaves listings.

## checksums

### `checksum.crc32.single`

**CRC32 on PutObject.** PUT with x-amz-checksum-crc32: the right value is stored and returned by HEAD (checksum mode); a wrong value is refused.

**AWS behavior:** Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored.

### `checksum.crc32.mpu-composite`

**CRC32 multipart, composite.** A two-part upload with CRC32 part checksums, completed in composite mode.

**AWS behavior:** CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE.

### `checksum.crc32.mpu-full-object`

**CRC32 multipart, full-object.** A two-part upload with CRC32 part checksums, completed in full-object mode.

**AWS behavior:** CreateMultipartUpload with x-amz-checksum-type: FULL_OBJECT; HEAD returns the CRC of the whole object (no -N suffix), type FULL_OBJECT.

### `checksum.crc32c.single`

**CRC32C on PutObject.** PUT with x-amz-checksum-crc32c: the right value is stored and returned by HEAD (checksum mode); a wrong value is refused.

**AWS behavior:** Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored.

### `checksum.crc32c.mpu-composite`

**CRC32C multipart, composite.** A two-part upload with CRC32C part checksums, completed in composite mode.

**AWS behavior:** CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE.

### `checksum.crc32c.mpu-full-object`

**CRC32C multipart, full-object.** A two-part upload with CRC32C part checksums, completed in full-object mode.

**AWS behavior:** CreateMultipartUpload with x-amz-checksum-type: FULL_OBJECT; HEAD returns the CRC of the whole object (no -N suffix), type FULL_OBJECT.

### `checksum.crc64nvme.single`

**CRC64NVME on PutObject.** PUT with x-amz-checksum-crc64nvme: the right value is stored and returned by HEAD (checksum mode); a wrong value is refused.

**AWS behavior:** Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored.

### `checksum.crc64nvme.mpu-composite`

**CRC64NVME multipart, composite.** A two-part upload with CRC64NVME part checksums, completed in composite mode.

**AWS behavior:** CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE.

### `checksum.crc64nvme.mpu-full-object`

**CRC64NVME multipart, full-object.** A two-part upload with CRC64NVME part checksums, completed in full-object mode.

**AWS behavior:** CreateMultipartUpload with x-amz-checksum-type: FULL_OBJECT; HEAD returns the CRC of the whole object (no -N suffix), type FULL_OBJECT.

### `checksum.sha1.single`

**SHA1 on PutObject.** PUT with x-amz-checksum-sha1: the right value is stored and returned by HEAD (checksum mode); a wrong value is refused.

**AWS behavior:** Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored.

### `checksum.sha1.mpu-composite`

**SHA1 multipart, composite.** A two-part upload with SHA1 part checksums, completed in composite mode.

**AWS behavior:** CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE.

### `checksum.sha256.single`

**SHA256 on PutObject.** PUT with x-amz-checksum-sha256: the right value is stored and returned by HEAD (checksum mode); a wrong value is refused.

**AWS behavior:** Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored.

### `checksum.sha256.mpu-composite`

**SHA256 multipart, composite.** A two-part upload with SHA256 part checksums, completed in composite mode.

**AWS behavior:** CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE.

### `checksum.trailing`

**Trailing checksum (aws-chunked).** PUT with Content-Encoding: aws-chunked, STREAMING-UNSIGNED-PAYLOAD-TRAILER and an x-amz-checksum-crc32 trailer: what current AWS SDKs send by default.

**AWS behavior:** 200; the trailer's CRC32 is verified, stored and returned by HEAD; the stored body is the decoded payload (no chunk framing).

## multipart

### `mpu.min-part-size`

**Minimum part size.** Completes an upload whose first part is 1 MiB (every part but the last must be at least 5 MiB).

**AWS behavior:** 400 EntityTooSmall at CompleteMultipartUpload; a 5 MiB first part completes.

### `mpu.out-of-order`

**Parts out of order.** Uploads part 2 before part 1, completes with the parts listed in descending order, then in ascending order.

**AWS behavior:** Parts may be uploaded in any order. A Complete listing parts out of ascending order: 400 InvalidPartOrder. Ascending: 200, the body in part-number order.

### `mpu.duplicate-complete`

**Complete twice.** Sends the same CompleteMultipartUpload twice, as a client retrying after a lost response would.

**AWS behavior:** The retry answers 200 with the same ETag (S3 treats a repeated Complete of a just-completed upload as success); afterwards the upload id is gone (NoSuchUpload).

### `mpu.concurrent-same-key`

**Concurrent uploads of one key.** Starts two uploads of the same key, completes A, then B.

**AWS behavior:** Two distinct upload ids; both complete; the last Complete wins (the key holds B's body).

### `mpu.upload-part-copy`

**UploadPartCopy.** Builds an object from a byte range of another with UploadPartCopy, plus an uploaded part.

**AWS behavior:** 200 with <CopyPartResult>; the completed object holds the copied range followed by the uploaded part.

## range

### `range.bounds`

**Range boundaries.** GETs bytes=0-0, bytes=9-9, bytes=-3 (suffix), bytes=7- (open) and bytes=5-100 (past the end) of a 10-byte object.

**AWS behavior:** 206 Partial Content for each, with Content-Range bytes a-b/10; a range past the end is cut to the object (bytes 5-9/10).

### `range.multi`

**Multiple ranges.** GET with Range: bytes=0-1,4-5.

**AWS behavior:** S3 does not serve multiple ranges: it ignores such a header and answers 200 with the whole object.

### `range.unsatisfiable`

**Range past the end.** GET bytes=100-200 of a 10-byte object, and bytes=0- of an empty object.

**AWS behavior:** 416 InvalidRange with Content-Range: bytes */10; on an empty object 416 as well.

## keys

### `keys.special`

**Special characters and Unicode in keys.** PUTs, GETs and lists 14 keys with spaces, +, %, reserved characters, dot segments, // and non-ASCII; the verdict is of the keys sent (sonde cannot send the two with a dot segment).

**AWS behavior:** Keys are opaque UTF-8: every key round-trips through PUT, GET and ListObjectsV2 (with encoding-type=url) exactly.

## listing

### `list.token-stability`

**Continuation token stability.** Lists 30 keys 10 at a time; between pages, adds keys before and after the position, then reuses the first token.

**AWS behavior:** A continuation token is a position: later pages continue after the last key returned, include keys written after it, never repeat keys, and the same token can be used again.

### `list.start-after`

**ListObjectsV2 StartAfter.** Lists with StartAfter set to an existing key, to a key between two keys, and to a key outside the prefix.

**AWS behavior:** Keys strictly after StartAfter, in UTF-8 binary order; StartAfter is echoed in the response.

## delete

### `delete.batch-limit`

**DeleteObjects batch limit.** DeleteObjects with 1000 keys, then with 1001 (keys that do not exist: nothing is deleted for real).

**AWS behavior:** 1000 keys: 200, each key reported deleted (deleting a missing key succeeds). 1001 keys: 400 MalformedXML.

## presign

### `presign.expiry`

**Presigned URL expiry.** Presigns a GET valid for a few seconds (allowing for the difference between this clock and the store's), uses it at once and again once it has expired; presigns with X-Amz-Expires over 7 days. Needs: slow.

**AWS behavior:** Fresh URL: 200. Expired: 403 AccessDenied ("Request has expired"). X-Amz-Expires above 604800: 400 AuthorizationQueryParametersError.

### `presign.clock-skew`

**Clock skew tolerance.** Signs requests with the clock 5 minutes ahead, then 20 minutes ahead.

**AWS behavior:** 5 minutes: accepted. Beyond 15 minutes: 403 RequestTimeTooSkewed, whose body gives the server's time.

## metadata

### `meta.size-limit`

**User metadata size limit.** PUTs with 1.9 KiB and 2.1 KiB of x-amz-meta-* (names and values).

**AWS behavior:** Up to 2 KiB of user metadata: 200, returned verbatim by HEAD. Over 2 KiB: 400 MetadataTooLarge.

## versioning

### `version.id-format`

**Version ID format.** Enables versioning on a temporary bucket, writes a key twice and inspects the version ids. Needs: tempBucket.

**AWS behavior:** Opaque, unique, URL-safe ids (32 characters today); `null` only for writes made while versioning was off or suspended.

## objectLock

### `lock.retention`

**Object Lock retention.** Creates a bucket with Object Lock, writes an object in GOVERNANCE mode for one day, then tries to delete that version. Needs: tempBucket.

**AWS behavior:** Deleting a locked version: 403 AccessDenied; GetObjectRetention returns the mode and date; a plain DELETE adds a delete marker.

## encryption

### `sse-c`

**SSE-C.** Writes an object with a customer key, reads it without the key, with a wrong key and with the right key. Needs: https.

**AWS behavior:** PUT with the key: 200. GET without it: 400 InvalidRequest; with a wrong key: 403 AccessDenied; with the right key: 200. Over plain HTTP S3 refuses SSE-C altogether (400).

## express

### `express.append`

**S3 Express append (x-amz-write-offset-bytes).** Appends to an object with PUT x-amz-write-offset-bytes at its size, then at a wrong offset.

**AWS behavior:** Directory buckets (S3 Express One Zone): the append succeeds and a wrong offset gives 400 InvalidWriteOffset. General purpose buckets: 501 NotImplemented.
