# Compatibility probes

What `sonde endpoints test ENDPOINT --probes BUCKET` runs ([cli.md](cli.md#buckets)). Generated from the probe
catalog by `scripts/gen-probe-docs.py`.

| Probe | Category | Checks | AWS behavior | Needs |
|---|---|---|---|---|
| `cond.put-if-none-match` | conditionalWrites | PUT If-None-Match: * | First PUT 200; second PUT 412 PreconditionFailed (409 ConditionalRequestConflict while a concurrent write is in flight). The object keeps its first body. |  |
| `cond.put-if-match` | conditionalWrites | PUT If-Match | PUT with a stale ETag: 412 PreconditionFailed; with the current ETag: 200. A missing key with If-Match: 404 NoSuchKey. |  |
| `cond.copy-if-match` | conditionalWrites | Conditional CopyObject | Wrong source ETag: 412; destination exists with If-None-Match: *: 412; otherwise 200. |  |
| `cond.copy-dest-if-match` | conditionalWrites | Conditional CopyObject destination | Stale ETag: 412 PreconditionFailed, the destination unchanged; current ETag: 200; missing destination: 404. |  |
| `cond.complete-if-none-match` | conditionalWrites | Conditional CompleteMultipartUpload | 412 PreconditionFailed; the existing object is kept. The upload can still be aborted. |  |
| `cond.delete-if-match` | conditionalWrites | Conditional delete | Stale ETag: 412 PreconditionFailed, the object kept; current ETag: 204, the object gone. DeleteObjects: 200 with a PreconditionFailed error for the object, which is kept. |  |
| `consistency.list-after-write` | consistency | List after write | Strong consistency (since December 2020): every acknowledged PUT appears in the next listing. |  |
| `consistency.read-after-overwrite` | consistency | Read after overwrite | Strong read-after-write: every GET returns the latest body. |  |
| `consistency.read-after-delete` | consistency | Read after delete | GET and HEAD answer 404 NoSuchKey right after DeleteObject; the key leaves listings. |  |
| `checksum.crc32.single` | checksums | CRC32 on PutObject | Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored. |  |
| `checksum.crc32.mpu-composite` | checksums | CRC32 multipart, composite | CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE. |  |
| `checksum.crc32.mpu-full-object` | checksums | CRC32 multipart, full-object | CreateMultipartUpload with x-amz-checksum-type: FULL_OBJECT; HEAD returns the CRC of the whole object (no -N suffix), type FULL_OBJECT. |  |
| `checksum.crc32c.single` | checksums | CRC32C on PutObject | Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored. |  |
| `checksum.crc32c.mpu-composite` | checksums | CRC32C multipart, composite | CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE. |  |
| `checksum.crc32c.mpu-full-object` | checksums | CRC32C multipart, full-object | CreateMultipartUpload with x-amz-checksum-type: FULL_OBJECT; HEAD returns the CRC of the whole object (no -N suffix), type FULL_OBJECT. |  |
| `checksum.crc64nvme.single` | checksums | CRC64NVME on PutObject | Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored. |  |
| `checksum.crc64nvme.mpu-composite` | checksums | CRC64NVME multipart, composite | CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE. |  |
| `checksum.crc64nvme.mpu-full-object` | checksums | CRC64NVME multipart, full-object | CreateMultipartUpload with x-amz-checksum-type: FULL_OBJECT; HEAD returns the CRC of the whole object (no -N suffix), type FULL_OBJECT. |  |
| `checksum.sha1.single` | checksums | SHA1 on PutObject | Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored. |  |
| `checksum.sha1.mpu-composite` | checksums | SHA1 multipart, composite | CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE. |  |
| `checksum.sha256.single` | checksums | SHA256 on PutObject | Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored. |  |
| `checksum.sha256.mpu-composite` | checksums | SHA256 multipart, composite | CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE. |  |
| `checksum.sha512.single` | checksums | SHA512 on PutObject | Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored. |  |
| `checksum.sha512.mpu-composite` | checksums | SHA512 multipart, composite | CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE. |  |
| `checksum.md5.single` | checksums | MD5 on PutObject | Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored. |  |
| `checksum.md5.mpu-composite` | checksums | MD5 multipart, composite | CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE. |  |
| `checksum.xxhash64.single` | checksums | XXHASH64 on PutObject | Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored. |  |
| `checksum.xxhash64.mpu-composite` | checksums | XXHASH64 multipart, composite | CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE. |  |
| `checksum.xxhash3.single` | checksums | XXHASH3 on PutObject | Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored. |  |
| `checksum.xxhash3.mpu-composite` | checksums | XXHASH3 multipart, composite | CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE. |  |
| `checksum.xxhash128.single` | checksums | XXHASH128 on PutObject | Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored. |  |
| `checksum.xxhash128.mpu-composite` | checksums | XXHASH128 multipart, composite | CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE. |  |
| `checksum.trailing` | checksums | Trailing checksum (aws-chunked) | 200; the trailer's CRC32 is verified, stored and returned by HEAD; the stored body is the decoded payload (no chunk framing). |  |
| `mpu.min-part-size` | multipart | Minimum part size | 400 EntityTooSmall at CompleteMultipartUpload; a 5 MiB first part completes. |  |
| `mpu.out-of-order` | multipart | Parts out of order | Parts may be uploaded in any order. A Complete listing parts out of ascending order: 400 InvalidPartOrder. Ascending: 200, the body in part-number order. |  |
| `mpu.duplicate-complete` | multipart | Complete twice | The retry answers 200 with the same ETag (S3 treats a repeated Complete of a just-completed upload as success); afterwards the upload id is gone (NoSuchUpload). |  |
| `mpu.concurrent-same-key` | multipart | Concurrent uploads of one key | Two distinct upload ids; both complete; the last Complete wins (the key holds B's body). |  |
| `mpu.upload-part-copy` | multipart | UploadPartCopy | 200 with <CopyPartResult>; the completed object holds the copied range followed by the uploaded part. |  |
| `range.bounds` | range | Range boundaries | 206 Partial Content for each, with Content-Range bytes a-b/10; a range past the end is cut to the object (bytes 5-9/10). |  |
| `range.multi` | range | Multiple ranges | S3 does not serve multiple ranges: it ignores such a header and answers 200 with the whole object. |  |
| `range.unsatisfiable` | range | Range past the end | 416 InvalidRange with Content-Range: bytes */10; on an empty object 416 as well. |  |
| `keys.special` | keys | Special characters and Unicode in keys | Keys are opaque UTF-8: every key round-trips through PUT, GET and ListObjectsV2 (with encoding-type=url) exactly. |  |
| `list.token-stability` | listing | Continuation token stability | A continuation token is a position: later pages continue after the last key returned, include keys written after it, never repeat keys, and the same token can be used again. |  |
| `list.start-after` | listing | ListObjectsV2 StartAfter | Keys strictly after StartAfter, in UTF-8 binary order; StartAfter is echoed in the response. |  |
| `delete.batch-limit` | delete | DeleteObjects batch limit | 1000 keys: 200, each key reported deleted (deleting a missing key succeeds). 1001 keys: 400 MalformedXML. |  |
| `presign.expiry` | presign | Presigned URL expiry | Fresh URL: 200. Expired: 403 AccessDenied ("Request has expired"). X-Amz-Expires above 604800: 400 AuthorizationQueryParametersError. | waits in real time |
| `presign.clock-skew` | presign | Clock skew tolerance | 5 minutes: accepted. Beyond 15 minutes: 403 RequestTimeTooSkewed, whose body gives the server's time. |  |
| `meta.size-limit` | metadata | User metadata size limit | Up to 2 KiB of user metadata: 200, returned verbatim by HEAD. Over 2 KiB: 400 MetadataTooLarge. |  |
| `version.id-format` | versioning | Version ID format | Opaque, unique, URL-ready ids of at most 1,024 bytes (today 32 characters of letters, digits, `.`, `_` and `-`); a store's own shape (a UUID, a number) is as good. `null` only for writes made while versioning was off or suspended. | a bucket of its own (skipped) |
| `lock.retention` | objectLock | Object Lock retention | Deleting a locked version: 403 AccessDenied; GetObjectRetention returns the mode and date; a plain DELETE adds a delete marker. | a bucket of its own (skipped) |
| `sse-c` | encryption | SSE-C | PUT with the key: 200. GET without it: 400 InvalidRequest; with a wrong key: 403 AccessDenied; with the right key: 200. Over plain HTTP S3 refuses SSE-C altogether (400). | HTTPS |
| `express.append` | express | S3 Express append (x-amz-write-offset-bytes) | Directory buckets (S3 Express One Zone): the append succeeds and a wrong offset gives 400 InvalidWriteOffset. General purpose buckets: 501 NotImplemented. |  |
