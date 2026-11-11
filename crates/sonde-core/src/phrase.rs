//! What sonde's services say, as data: findings, verdicts, plan summaries,
//! job titles and progress are phrases (a key and its arguments) beside
//! their English, so the app says them in the reader's language
//! (`app/src/corephrases.ts`) while the CLI prints the English.
//!
//! The type and the filling of a template are `decoders::phrase`'s (the one
//! crate everything that speaks depends on); the table here is this crate's.

pub use decoders::ph;
pub use decoders::phrase::{Phrase, plural, raw, untranslated};

/// This crate's phrase (or a decoder's) in English.
pub fn say(p: &Phrase) -> String {
    match template(&p.key) {
        Some(t) => p.fill(t),
        None => decoders::phrase::say(p),
    }
}

/// A text in English: its phrases one after the other.
pub fn en(text: &[Phrase]) -> String {
    text.iter().map(say).collect()
}

pub fn template(key: &str) -> Option<&'static str> {
    TEMPLATES.iter().find(|(k, _)| *k == key).map(|(_, t)| *t)
}

/// Every key this crate says and its English.
pub fn templates() -> Vec<(String, String)> {
    TEMPLATES
        .iter()
        .map(|(k, t)| (k.to_string(), t.to_string()))
        .collect()
}

/// The English templates: `{name}` takes the argument of that name.
pub const TEMPLATES: &[(&str, &str)] = &[
    // TEMPLATES-BEGIN
    ("du.versionsUnknown", "Noncurrent versions and delete markers were not counted: this key may not list versions ({why}). What they hold is unknown, and the total is of current objects only."),
    ("du.empty", "{place} holds no bytes"),
    ("du.empty.current", " in current objects"),
    ("du.empty.markers", " but {n} delete markers"),
    ("du.stop", "."),
    ("du.current", "Current versions: {size} in {n} objects ({pct}%)."),
    ("du.noncurrent", "Noncurrent versions: {size} in {n} versions ({pct}%), invisible to plain listings."),
    ("du.noncurrent.rule", " A NoncurrentVersionExpiration lifecycle rule would reclaim them."),
    ("du.multipart", "Incomplete multipart uploads: {size} in {n} uploads ({pct}%), billed but not listed as objects. See Multipart."),
    ("du.multipart.uncounted", "Incomplete multipart uploads were not counted: {error}"),
    ("du.markers.many", "{n} delete markers, more than the {objects} current objects: listings slow down; ExpiredObjectDeleteMarker cleans up the ones with no versions left."),
    ("du.markers", "{n} delete markers."),
    ("du.prefix", "Prefix {prefix} holds {pct}% of the bytes ({size})."),
    ("du.class", "Storage class {class} holds {pct}% of the bytes."),
    ("du.small", "{pct}% of objects are under 128 KiB: request costs dominate, and IA classes bill each as 128 KiB."),
    ("progress.versions", "{n} versions, {markers} delete markers"),
    ("progress.objects", "{n} objects"),
    ("job.du", "Capacity scan {bucket}/{prefix}"),
    ("plan.confirm.bucket", "the bucket name"),
    ("plan.confirm.count", "the number of items"),
    ("plan.putLifecycle", "Write a new lifecycle configuration to {bucket}"),
    ("plan.abortUploads", "Abort multipart uploads {n} item(s) in {bucket} ({size})"),
    ("plan.deleteVersions", "Permanently delete versions {n} item(s) in {bucket} ({size})"),
    ("plan.restoreObject", "Remove the latest delete marker {n} item(s) in {bucket} ({size})"),
    ("plan.deleteObjects", "Delete objects {n} item(s) in {bucket} ({size})"),
    ("plan.runProbes", "Run compatibility probes {n} item(s) in {bucket} ({size})"),
    ("plan.item.upload", "{parts} parts, started {started}"),
    ("plan.item.abortRule", "abort incomplete uploads after {days} days"),
    ("plan.item.restore", "delete marker {marker}; version {version} becomes current"),
    ("plan.item.restore.nothing", "delete marker {marker}; version nothing (no older version) becomes current"),
    ("plan.item.version", "version {v}"),
    ("plan.item.delete", "delete (adds a delete marker when versioning is on)"),
    ("lifecycle.none", "The bucket has no lifecycle configuration: incomplete uploads are kept (and billed) forever. Add a rule aborting them after {days} days."),
    ("lifecycle.covered", "Covered: rule {rule} aborts incomplete uploads under {prefix} after {days} days."),
    ("lifecycle.noAbort", "The lifecycle has {n} rule(s) but none with AbortIncompleteMultipartUpload: incomplete uploads are kept forever."),
    ("lifecycle.partial", "{n} AbortIncompleteMultipartUpload rule(s) exist but none (enabled) covers {prefix}."),
    ("lifecycle.unreadable", "The lifecycle document could not be read: {error}"),
    ("lifecycle.failed", "GetBucketLifecycleConfiguration failed: {error}"),
    ("job.mpu", "Multipart scan {bucket}/{prefix}"),
    ("job.download", "Download {bucket}/{key}"),
    ("job.search", "Search {bucket}/{prefix}*{needle}*"),
    ("job.layoutDetect", "Detect layouts in {bucket}/{prefix}"),
    ("job.layout", "{format} at {bucket}/{root}"),
    ("job.probes", "Probes on {endpoint}"),
    ("progress.uploads", "{n} uploads listed"),
    ("progress.listParts", "ListParts {n}/{total}"),
    ("progress.leftBehind", "left behind: {path} ({error})"),
    ("progress.bytes", "{n} bytes"),
    ("progress.search", "{n} keys scanned, {found} matches"),
    ("progress.listed", "listed {n} objects"),
    ("cred.default", "default chain"),
    ("cred.profile", "profile {name}"),
    ("cred.env", "env ${var}"),
    ("cred.imds", "instance role (IMDSv2)"),
    ("cred.webIdentity", "web identity {role}"),
    ("cred.process", "credential_process"),
    ("cred.static", "static keys (config file)"),
    ("cred.anonymous", "anonymous"),
    // TEMPLATES-END
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_key_is_listed_twice_or_is_a_decoders_key() {
        let all = templates();
        for (i, (k, _)) in all.iter().enumerate() {
            assert!(all[..i].iter().all(|(o, _)| o != k), "{k} is listed twice");
            assert!(
                decoders::phrase::template(k).is_none(),
                "{k} is a decoder's key"
            );
        }
    }

    #[test]
    fn every_key_has_a_chinese_template() {
        let ts = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../app/src/corephrases.ts");
        let ts = std::fs::read_to_string(ts).unwrap();
        assert_eq!(untranslated(&ts, &templates()), Vec::<String>::new());
    }
}
