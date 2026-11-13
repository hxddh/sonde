//! What a probe says, as data: its catalog entry (`probe.<id>.title`,
//! `.description`, `.aws`), its verdict's summary (`judge.<id>.<verdict>`)
//! and its observations are phrases beside their English, so the app says
//! them in the reader's language (`app/src/corephrases.ts`).
//!
//! The type and the filling are `sonde_core::phrase`'s; the table here is
//! this crate's, and the catalog's own words are its templates.

pub use sonde_core::phrase::{Phrase, ph, plural, raw, untranslated};

/// The catalog's entries as templates: three a probe.
fn catalog_templates() -> Vec<(String, String)> {
    crate::catalog()
        .into_iter()
        .flat_map(|m| {
            [
                (format!("probe.{}.title", m.id), m.title),
                (format!("probe.{}.description", m.id), m.description),
                (format!("probe.{}.aws", m.id), m.aws_behavior),
            ]
        })
        .collect()
}

/// This crate's phrase (or sonde-core's, or a decoder's) in English.
pub fn say(p: &Phrase) -> String {
    if p.key.starts_with("probe.") {
        return catalog_templates()
            .into_iter()
            .find(|(k, _)| *k == p.key)
            .map(|(_, t)| t)
            .unwrap_or_default();
    }
    match template(&p.key) {
        Some(t) => p.fill(t),
        None => sonde_core::phrase::say(p),
    }
}

/// A text in English: its phrases one after the other.
pub fn en(text: &[Phrase]) -> String {
    text.iter().map(say).collect()
}

pub fn template(key: &str) -> Option<&'static str> {
    TEMPLATES.iter().find(|(k, _)| *k == key).map(|(_, t)| *t)
}

/// Every key this crate says and its English: the table and the catalog.
pub fn templates() -> Vec<(String, String)> {
    TEMPLATES
        .iter()
        .map(|(k, t)| (k.to_string(), t.to_string()))
        .chain(catalog_templates())
        .collect()
}

/// The English templates: `{name}` takes the argument of that name.
pub const TEMPLATES: &[(&str, &str)] = &[
    // TEMPLATES-BEGIN
    ("judge.checksum.unsupported", "{p1} checksums are refused"),
    ("judge.checksum.error", "the PUT failed"),
    (
        "judge.checksum.fail",
        "a wrong checksum was accepted: the value is not verified",
    ),
    ("judge.checksum.pass", "verified, stored and returned"),
    (
        "judge.checksum.differs",
        "a wrong value is refused with {p1} {p2} (AWS: 400 BadDigest)",
    ),
    (
        "judge.checksum.fail.2",
        "HEAD returned {v}, the PUT sent {expected}",
    ),
    (
        "judge.checksum.differs.2",
        "verified on upload but not returned by HEAD",
    ),
    ("judge.checksum.unsupported.2", "refused: {p1}"),
    ("judge.checksum.error.2", "failed: {p1}"),
    (
        "judge.checksum.pass.2",
        "{wanttype} checksum as AWS computes it",
    ),
    ("judge.checksum.differs.3", "value right, but type {t}"),
    (
        "judge.checksum.differs.4",
        "value right, x-amz-checksum-type missing",
    ),
    (
        "judge.checksum.differs.5",
        "HEAD returned {v} ({p1}), expected {expected}",
    ),
    (
        "judge.checksum.differs.6",
        "the object carries no checksum after Complete",
    ),
    (
        "judge.checksum.trailing.unsupported",
        "aws-chunked trailers refused: {p1}",
    ),
    (
        "judge.checksum.trailing.fail",
        "stored length {p1}: the chunk framing was kept",
    ),
    (
        "judge.checksum.trailing.pass",
        "trailer verified and stored",
    ),
    (
        "judge.checksum.trailing.fail.2",
        "stored CRC32 {v}, trailer said {expected}",
    ),
    (
        "judge.checksum.trailing.differs",
        "accepted, but the trailer's checksum is not stored",
    ),
    ("judge.cond.notImplemented", "not implemented ({p1})"),
    (
        "judge.cond.put-if-none-match.fail",
        "a create-only PUT of a new key was refused",
    ),
    (
        "judge.cond.put-if-none-match.fail.2",
        "If-None-Match: * was ignored: the existing object was overwritten",
    ),
    (
        "judge.cond.put-if-none-match.pass",
        "refused with 412 as AWS does",
    ),
    (
        "judge.cond.put-if-none-match.differs",
        "refused with 409 instead of 412",
    ),
    (
        "judge.cond.put-if-none-match.differs.2",
        "refused with {p1} {p2}",
    ),
    (
        "judge.cond.put-if-none-match.fail.3",
        "refused, yet the body changed",
    ),
    (
        "judge.cond.put-if-match.fail",
        "If-Match was ignored: a stale ETag overwrote the object",
    ),
    (
        "judge.cond.put-if-match.fail.2",
        "the current ETag was refused",
    ),
    (
        "judge.cond.put-if-match.fail.3",
        "If-Match was ignored on a missing key: the object was created",
    ),
    (
        "judge.cond.put-if-match.pass",
        "412 on a stale ETag, 404 on a missing key",
    ),
    (
        "judge.cond.put-if-match.differs",
        "missing key answered {p1} instead of 404",
    ),
    (
        "judge.cond.put-if-match.differs.2",
        "stale ETag answered {p1} instead of 412",
    ),
    (
        "judge.cond.copy-if-match.fail",
        "copy-source-if-match was ignored",
    ),
    (
        "judge.cond.copy-if-match.fail.2",
        "a copy with the right source ETag failed",
    ),
    (
        "judge.cond.copy-if-match.differs",
        "If-None-Match: * on the copy destination is ignored (AWS refuses with 412)",
    ),
    (
        "judge.cond.copy-if-match.pass",
        "both conditions enforced with 412",
    ),
    (
        "judge.cond.copy-if-match.differs.2",
        "conditions enforced with {p1} / {p2}",
    ),
    (
        "judge.cond.complete-if-none-match.fail",
        "If-None-Match: * was ignored: the upload replaced the existing object",
    ),
    (
        "judge.cond.complete-if-none-match.pass",
        "refused with 412; the existing object kept",
    ),
    (
        "judge.cond.complete-if-none-match.fail.2",
        "refused, but the existing object changed",
    ),
    (
        "judge.cond.complete-if-none-match.differs",
        "refused with {p1} {p2}",
    ),
    (
        "judge.consistency.list-after-write.error",
        "the listing failed",
    ),
    (
        "judge.consistency.list-after-write.error.2",
        "could not run: no write was acknowledged ({p1})",
    ),
    (
        "judge.consistency.list-after-write.pass",
        "all {puts} writes listed at once",
    ),
    (
        "judge.consistency.list-after-write.fail",
        "only {listed} of {puts} acknowledged writes listed: listings are eventually consistent",
    ),
    (
        "judge.consistency.read-after-overwrite.pass",
        "every read saw the latest write",
    ),
    (
        "judge.consistency.read-after-overwrite.fail",
        "{p1} of 5 reads returned an older body",
    ),
    (
        "judge.consistency.read-after-delete.fail",
        "the deleted object was still readable",
    ),
    ("judge.consistency.read-after-delete.pass", "404 at once"),
    (
        "judge.consistency.read-after-delete.differs",
        "answered {p1} / {p2} instead of 404",
    ),
    (
        "judge.keys.special.error",
        "could not run: no key could be written ({p1})",
    ),
    ("judge.keys.special.pass", "all {p1} keys round-trip"),
    (
        "judge.keys.special.said",
        "{p1} of {sent} keys do not round-trip: {p2}",
    ),
    (
        "judge.list.token-stability.error",
        "could not run: the listing failed",
    ),
    ("judge.list.token-stability.fail", "pages repeated keys"),
    ("judge.list.token-stability.fail.2", "pages out of order"),
    (
        "judge.list.token-stability.pass",
        "positional tokens: later pages continue after the position and the token can be reused",
    ),
    (
        "judge.list.token-stability.differs",
        "a key written after the position was not returned: the listing is a snapshot",
    ),
    (
        "judge.list.token-stability.differs.2",
        "a key before the position appeared in a later page",
    ),
    (
        "judge.list.token-stability.differs.3",
        "reusing the first token answered {p1} starting at {p2}",
    ),
    (
        "judge.list.start-after.pass",
        "keys strictly after StartAfter",
    ),
    ("judge.list.start-after.unsupported", "StartAfter refused"),
    (
        "judge.list.start-after.fail",
        "existing → {p1}, between → {p2}, before prefix → {p3}",
    ),
    (
        "judge.delete.batch-limit.unsupported",
        "DeleteObjects is not implemented",
    ),
    ("judge.delete.batch-limit.fail", "1000 keys refused: {p1}"),
    (
        "judge.delete.batch-limit.differs",
        "1001 keys accepted (no 1000-key limit)",
    ),
    (
        "judge.delete.batch-limit.pass",
        "1000 accepted, 1001 MalformedXML",
    ),
    (
        "judge.delete.batch-limit.differs.2",
        "1001 keys refused with {p1} {p2}; {deleted} of 1000 reported deleted",
    ),
    ("judge.presign.expiry.error", "could not run: {p1}"),
    (
        "judge.presign.expiry.fail",
        "a fresh presigned URL was refused: {p1}",
    ),
    (
        "judge.presign.expiry.fail.2",
        "an expired presigned URL still works",
    ),
    (
        "judge.presign.expiry.pass",
        "expiry enforced; 7-day maximum enforced",
    ),
    (
        "judge.presign.expiry.differs",
        "expiry enforced, but X-Amz-Expires=604801 answered {p1}",
    ),
    (
        "judge.presign.expiry.differs.2",
        "an expired URL answered {p1}",
    ),
    (
        "judge.presign.clock-skew.differs",
        "5 minutes of skew refused: {p1}",
    ),
    (
        "judge.presign.clock-skew.differs.2",
        "20 minutes of skew accepted (AWS allows 15)",
    ),
    (
        "judge.presign.clock-skew.pass",
        "15-minute window, RequestTimeTooSkewed",
    ),
    (
        "judge.presign.clock-skew.differs.3",
        "skew refused with {p1} {p2}",
    ),
    (
        "judge.meta.size-limit.differs",
        "1.9 KiB of metadata refused: {p1}",
    ),
    (
        "judge.meta.size-limit.fail",
        "metadata not returned verbatim",
    ),
    (
        "judge.meta.size-limit.differs.2",
        "2.1 KiB of metadata accepted (AWS limit: 2 KiB)",
    ),
    (
        "judge.meta.size-limit.pass",
        "2 KiB limit with MetadataTooLarge",
    ),
    (
        "judge.meta.size-limit.differs.3",
        "over the limit: {p1} {p2}",
    ),
    (
        "judge.version.id-format.skipped",
        "needs a temporary bucket (allow bucket creation)",
    ),
    (
        "judge.version.id-format.error",
        "could not create a temporary bucket",
    ),
    (
        "judge.version.id-format.unsupported",
        "versioning could not be enabled",
    ),
    (
        "judge.version.id-format.fail",
        "PUT on a versioned bucket returned no x-amz-version-id",
    ),
    (
        "judge.version.id-format.fail.2",
        "two writes got the same version id",
    ),
    ("judge.version.id-format.pass", "unique ids, {shape}"),
    (
        "judge.version.id-format.differs",
        "unique ids, but {shape} ({p1}): code assuming AWS's shape may break",
    ),
    (
        "judge.lock.retention.skipped",
        "needs a temporary bucket (allow bucket creation)",
    ),
    (
        "judge.lock.retention.unsupported",
        "a bucket with Object Lock could not be created: {p1}",
    ),
    (
        "judge.lock.retention.unsupported.2",
        "a PUT with retention was refused: {p1}",
    ),
    (
        "judge.lock.retention.fail",
        "a version under GOVERNANCE retention was deleted without bypass",
    ),
    (
        "judge.lock.retention.pass",
        "retention enforced and reported",
    ),
    (
        "judge.lock.retention.differs",
        "delete refused with {p1}; retention read {p2}",
    ),
    (
        "judge.sse-c.pass",
        "refused over HTTP, as AWS does (run over HTTPS to probe further)",
    ),
    ("judge.sse-c.unsupported", "SSE-C refused: {p1}"),
    (
        "judge.sse-c.fail",
        "the object was readable without its key",
    ),
    (
        "judge.sse-c.fail.2",
        "the right key did not read the object back",
    ),
    ("judge.sse-c.pass.2", "key required and checked"),
    ("judge.sse-c.differs", "no key → {p1}, wrong key → {p2}"),
    ("judge.express.append.unsupported", "no appends: {p1}"),
    (
        "judge.express.append.pass",
        "appended; a wrong offset refused",
    ),
    ("judge.express.append.fail", "a wrong offset was accepted"),
    (
        "judge.express.append.fail.2",
        "the header was ignored: the append replaced the object",
    ),
    (
        "judge.express.append.differs",
        "object length {other} after the appends",
    ),
    ("judge.setup.failed", "could not run: setup failed ({p1})"),
    (
        "judge.setup.refused",
        "could not run: the write was not allowed ({p1})",
    ),
    (
        "judge.mpu.min-part-size.differs",
        "a 1 MiB non-final part was accepted (no 5 MiB minimum)",
    ),
    ("judge.mpu.min-part-size.pass", "400 EntityTooSmall"),
    (
        "judge.mpu.min-part-size.differs.2",
        "refused with {p1} {p2} instead of EntityTooSmall",
    ),
    (
        "judge.mpu.out-of-order.fail",
        "parts uploaded out of order could not be completed",
    ),
    (
        "judge.mpu.out-of-order.fail.2",
        "the completed body is not in part-number order",
    ),
    (
        "judge.mpu.out-of-order.differs",
        "a descending part list was accepted (AWS: InvalidPartOrder)",
    ),
    (
        "judge.mpu.out-of-order.pass",
        "out-of-order uploads complete; a descending list is InvalidPartOrder",
    ),
    (
        "judge.mpu.out-of-order.differs.2",
        "a descending list is refused with {p1} {p2}",
    ),
    (
        "judge.mpu.duplicate-complete.error",
        "the first Complete failed",
    ),
    (
        "judge.mpu.duplicate-complete.pass",
        "the retry succeeded with the same ETag",
    ),
    (
        "judge.mpu.duplicate-complete.fail",
        "the retry succeeded with another ETag",
    ),
    (
        "judge.mpu.duplicate-complete.differs",
        "the retry fails with NoSuchUpload: clients retrying a lost response see an error for a write that succeeded",
    ),
    (
        "judge.mpu.duplicate-complete.differs.2",
        "the retry answered {p1} {p2}",
    ),
    (
        "judge.mpu.concurrent-same-key.fail",
        "both uploads got the same upload id",
    ),
    (
        "judge.mpu.concurrent-same-key.pass",
        "independent uploads; the last Complete wins",
    ),
    (
        "judge.mpu.concurrent-same-key.fail.2",
        "both completed but the key reads {p1}",
    ),
    (
        "judge.mpu.concurrent-same-key.differs",
        "the second upload could not complete after the first",
    ),
    (
        "judge.mpu.upload-part-copy.unsupported",
        "UploadPartCopy is not implemented",
    ),
    (
        "judge.mpu.upload-part-copy.fail",
        "UploadPartCopy failed: {p1}",
    ),
    (
        "judge.mpu.upload-part-copy.pass",
        "copied range and uploaded part combined",
    ),
    (
        "judge.mpu.upload-part-copy.fail.2",
        "completed object length {p1}, expected {want}",
    ),
    ("judge.range.bounds.fail", "wrong bytes for {p1}"),
    (
        "judge.range.bounds.pass",
        "every boundary as AWS answers it",
    ),
    (
        "judge.range.multi.pass",
        "ignored: 200 with the whole object",
    ),
    (
        "judge.range.multi.differs",
        "served as multipart/byteranges (AWS returns the whole object)",
    ),
    (
        "judge.range.multi.differs.2",
        "206 with Content-Range {p1}: only one range served",
    ),
    ("judge.range.multi.differs.3", "answered {p1} {p2}"),
    (
        "judge.range.unsatisfiable.differs",
        "a range past the end answered {p1}",
    ),
    ("judge.range.unsatisfiable.pass", "416 InvalidRange"),
    (
        "judge.range.unsatisfiable.differs.2",
        "past the end {p1} {p2}; empty object {p3}",
    ),
    ("judge.timedOut", "timed out after {secs} s; "),
    ("judge.skipped.slow", "slow probes are off"),
    (
        "judge.sse-c.http",
        "SSE-C accepted over plain HTTP (AWS refuses); ",
    ),
    (
        "obs.keys.notTested",
        "{keys} not tested: sonde cannot send a key with a . or .. segment (its HTTP library collapses them), which says nothing about the store",
    ),
    ("obs.keys.listed", "listed: {listed}"),
    ("obs.ids", "ids: {ids}"),
    ("obs.leftBehind", "left behind: {what}"),
    // TEMPLATES-END
];
