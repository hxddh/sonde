use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use md5::Digest as _;

use super::meta;
use crate::Probe;
use crate::cx::{ProbeCx, data};
use crate::model::{Category, Judgement, Needs, ProbeMeta, Steps, Verdict};

pub fn probes() -> Vec<Box<dyn Probe>> {
    vec![
        Box::new(PresignExpiry),
        Box::new(ClockSkew),
        Box::new(MetadataLimit),
        Box::new(VersionIdFormat),
        Box::new(ObjectLock),
        Box::new(SseC),
        Box::new(ExpressAppend),
    ]
}

pub struct PresignExpiry;

#[async_trait]
impl Probe for PresignExpiry {
    fn meta(&self) -> ProbeMeta {
        meta(
            "presign.expiry",
            Category::Presign,
            "Presigned URL expiry",
            "Presigns a GET valid for a few seconds (allowing for the difference between this clock and the store's), uses it at once and again once it has expired; presigns with X-Amz-Expires over 7 days.",
            "Fresh URL: 200. Expired: 403 AccessDenied (\"Request has expired\"). X-Amz-Expires above 604800: 400 AuthorizationQueryParametersError.",
            &[Needs::Slow],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("presign");
        let put = cx.put("put", &k, "presigned", &[]).await;
        // the store's clock, from its Date header: the URL expires by it
        let skew = put
            .as_ref()
            .and_then(|r| r.header("date"))
            .and_then(|d| chrono::DateTime::parse_from_rfc2822(d).ok())
            .map(|d| (d.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds());
        let Some((valid, wait)) = presign_timing(skew) else {
            cx.note(
                "clock",
                format!(
                    "the store's clock differs from this one by {} s",
                    skew.unwrap_or(0)
                ),
            );
            return;
        };
        let Ok(url) = cx.client.presign("GET", &cx.bucket, &k, valid, &[]).await else {
            cx.note("presign", "no credentials");
            return;
        };
        cx.record(
            "fresh",
            cx.client
                .send_url("GetObject (presigned)", http::Method::GET, &url)
                .await,
        );
        tokio::time::sleep(Duration::from_secs(wait)).await;
        cx.record(
            "expired",
            cx.client
                .send_url("GetObject (presigned)", http::Method::GET, &url)
                .await,
        );
        if let Ok(long) = cx.client.presign("GET", &cx.bucket, &k, 604_800, &[]).await {
            let long = long.replace("X-Amz-Expires=604800", "X-Amz-Expires=604801");
            cx.record(
                "too-long",
                cx.client
                    .send_url("GetObject (presigned)", http::Method::GET, &long)
                    .await,
            );
        }
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (fresh, expired, long) = (s.at("fresh"), s.at("expired"), s.at("too-long"));
        if let Some(j) = super::setup_failed(s, &["put"]) {
            return j;
        }
        if let Some(clock) = s.get("clock") {
            return Judgement::new(Verdict::Error, format!("could not run: {}", clock.body), s);
        }
        if !fresh.ok() {
            return Judgement::new(
                Verdict::Fail,
                format!("a fresh presigned URL was refused: {}", fresh.describe()),
                s,
            );
        }
        if expired.ok() {
            return Judgement::new(Verdict::Fail, "an expired presigned URL still works", s);
        }
        if expired.is(403) && (long.is(400) || long.is(403)) {
            Judgement::new(Verdict::Pass, "expiry enforced; 7-day maximum enforced", s)
        } else if expired.is(403) {
            Judgement::new(
                Verdict::Differs,
                format!(
                    "expiry enforced, but X-Amz-Expires=604801 answered {:?}",
                    long.status
                ),
                s,
            )
        } else {
            Judgement::new(
                Verdict::Differs,
                format!("an expired URL answered {:?}", expired.status),
                s,
            )
        }
    }
}

/// How long the presigned URL is valid and how long to wait before it
/// must have expired, in seconds, given how far the store's clock is ahead
/// of this one (`None`: unknown). The URL is signed with this clock and
/// expires by the store's, so a store `d` seconds ahead sees it `d` seconds
/// old at once: the validity grows by `d`, the wait by `-d`. Either way
/// about 8 seconds of real validity (a rate limit may delay the first use)
/// and 3 of margin past the expiry. `None` when the clocks are too far
/// apart to time anything.
fn presign_timing(store_ahead: Option<i64>) -> Option<(u64, u64)> {
    const LIVE: i64 = 8;
    const MARGIN: i64 = 3;
    match store_ahead {
        // unknown: a longer validity and wait absorb a few seconds either way
        None => Some((10, 15)),
        Some(d) if d.abs() > 45 => None,
        Some(d) => Some(((LIVE + d.max(0)) as u64, (LIVE + MARGIN - d.min(0)) as u64)),
    }
}

pub struct ClockSkew;

#[async_trait]
impl Probe for ClockSkew {
    fn meta(&self) -> ProbeMeta {
        meta(
            "presign.clock-skew",
            Category::Presign,
            "Clock skew tolerance",
            "Signs requests with the clock 5 minutes ahead, then 20 minutes ahead.",
            "5 minutes: accepted. Beyond 15 minutes: 403 RequestTimeTooSkewed, whose body gives the server's time.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("skew");
        cx.put("put", &k, "x", &[]).await;
        for (label, secs) in [
            ("ahead-5m", 300),
            ("ahead-20m", 1200),
            ("behind-20m", -1200),
        ] {
            let c = cx.client.with_clock_skew(secs);
            let r = c
                .send_raw(
                    s3raw::S3Request::new("HeadObject", http::Method::HEAD)
                        .bucket(&cx.bucket)
                        .key(&k)
                        .no_retry(),
                )
                .await;
            cx.record(label, r);
        }
        let c = cx.client.with_clock_skew(1200);
        let r = c
            .send_raw(
                s3raw::S3Request::new("GetObject", http::Method::GET)
                    .bucket(&cx.bucket)
                    .key(&k)
                    .no_retry(),
            )
            .await;
        cx.record("ahead-20m-get", r);
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (small, big, behind, get) = (
            s.at("ahead-5m"),
            s.at("ahead-20m"),
            s.at("behind-20m"),
            s.at("ahead-20m-get"),
        );
        if !small.ok() {
            return Judgement::new(
                Verdict::Differs,
                format!("5 minutes of skew refused: {}", small.describe()),
                s,
            );
        }
        if big.ok() || behind.ok() {
            return Judgement::new(
                Verdict::Differs,
                "20 minutes of skew accepted (AWS allows 15)",
                s,
            );
        }
        if get.code.as_deref() == Some("RequestTimeTooSkewed") {
            Judgement::new(Verdict::Pass, "15-minute window, RequestTimeTooSkewed", s)
        } else {
            Judgement::new(
                Verdict::Differs,
                format!("skew refused with {:?} {:?}", get.status, get.code),
                s,
            )
        }
    }
}

pub struct MetadataLimit;

#[async_trait]
impl Probe for MetadataLimit {
    fn meta(&self) -> ProbeMeta {
        meta(
            "meta.size-limit",
            Category::Metadata,
            "User metadata size limit",
            "PUTs with 1.9 KiB and 2.1 KiB of x-amz-meta-* (names and values).",
            "Up to 2 KiB of user metadata: 200, returned verbatim by HEAD. Over 2 KiB: 400 MetadataTooLarge.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let small = "v".repeat(1900);
        let big = "v".repeat(2150);
        cx.put(
            "under",
            &cx.key("meta-under"),
            "x",
            &[("x-amz-meta-m", small.as_str())],
        )
        .await;
        cx.head("head", &cx.key("meta-under"), &[]).await;
        cx.put(
            "over",
            &cx.key("meta-over"),
            "x",
            &[("x-amz-meta-m", big.as_str())],
        )
        .await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (under, head, over) = (s.at("under"), s.at("head"), s.at("over"));
        if !under.ok() {
            return Judgement::new(
                Verdict::Differs,
                format!("1.9 KiB of metadata refused: {}", under.describe()),
                s,
            );
        }
        if head.header("x-amz-meta-m").map(|v| v.len()) != Some(1900) {
            return Judgement::new(Verdict::Fail, "metadata not returned verbatim", s);
        }
        if over.ok() {
            Judgement::new(
                Verdict::Differs,
                "2.1 KiB of metadata accepted (AWS limit: 2 KiB)",
                s,
            )
        } else if over.code.as_deref() == Some("MetadataTooLarge") {
            Judgement::new(Verdict::Pass, "2 KiB limit with MetadataTooLarge", s)
        } else {
            Judgement::new(
                Verdict::Differs,
                format!("over the limit: {:?} {:?}", over.status, over.code),
                s,
            )
        }
    }
}

pub struct VersionIdFormat;

/// What a version id looks like.
pub fn version_id_shape(id: &str) -> &'static str {
    if id == "null" {
        "null"
    } else if id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
    {
        "aws-like (32 URL-safe characters)"
    } else if id.len() == 36 && id.chars().filter(|c| *c == '-').count() == 4 {
        "uuid"
    } else if id.bytes().all(|b| b.is_ascii_digit()) {
        "numeric"
    } else {
        "other"
    }
}

#[async_trait]
impl Probe for VersionIdFormat {
    fn meta(&self) -> ProbeMeta {
        meta(
            "version.id-format",
            Category::Versioning,
            "Version ID format",
            "Enables versioning on a temporary bucket, writes a key twice and inspects the version ids.",
            "Opaque, unique, URL-safe ids (32 characters today); `null` only for writes made while versioning was off or suspended.",
            &[Needs::TempBucket],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let Some(b) = cx.temp_bucket("create-bucket", false).await else {
            return;
        };
        cx.record(
            "enable",
            cx.client.put_bucket_versioning(&b, "Enabled").await,
        );
        let mut ids = vec![];
        for i in 0..2 {
            let r = cx
                .client
                .put_object(&b, "k", bytes::Bytes::from(format!("{i}")), &[])
                .await;
            if let Some(r) = cx.record(&format!("put-{i}"), r) {
                ids.push(r.header("x-amz-version-id").unwrap_or("").to_string());
            }
        }
        cx.note("ids", ids.join(" "));
    }
    fn judge(&self, s: &Steps) -> Judgement {
        if s.get("create-bucket").is_none() {
            return Judgement::new(
                Verdict::Skipped,
                "needs a temporary bucket (allow bucket creation)",
                s,
            );
        }
        if !s.at("create-bucket").ok() {
            return Judgement::new(Verdict::Error, "could not create a temporary bucket", s);
        }
        if !s.at("enable").ok() {
            return Judgement::new(Verdict::Unsupported, "versioning could not be enabled", s);
        }
        let ids: Vec<String> = s
            .at("ids")
            .body
            .split(' ')
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect();
        if ids.len() < 2 || ids.iter().any(|i| i.is_empty()) {
            return Judgement::new(
                Verdict::Fail,
                "PUT on a versioned bucket returned no x-amz-version-id",
                s,
            );
        }
        if ids[0] == ids[1] {
            return Judgement::new(Verdict::Fail, "two writes got the same version id", s);
        }
        let shape = version_id_shape(&ids[0]);
        let mut j = if shape.starts_with("aws-like") {
            Judgement::new(Verdict::Pass, format!("unique ids, {shape}"), s)
        } else {
            Judgement::new(
                Verdict::Differs,
                format!(
                    "unique ids, but {shape} ({}): code assuming AWS's shape may break",
                    ids[0]
                ),
                s,
            )
        };
        j.observations.push(format!("ids: {}", ids.join(", ")));
        j
    }
}

pub struct ObjectLock;

#[async_trait]
impl Probe for ObjectLock {
    fn meta(&self) -> ProbeMeta {
        meta(
            "lock.retention",
            Category::ObjectLock,
            "Object Lock retention",
            "Creates a bucket with Object Lock, writes an object in GOVERNANCE mode for one day, then tries to delete that version.",
            "Deleting a locked version: 403 AccessDenied; GetObjectRetention returns the mode and date; a plain DELETE adds a delete marker.",
            &[Needs::TempBucket],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let Some(b) = cx.temp_bucket("create-bucket", true).await else {
            return;
        };
        let until = (chrono::Utc::now() + chrono::Duration::days(1))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let body = bytes::Bytes::from_static(b"locked");
        let md5 = base64::engine::general_purpose::STANDARD.encode(md5::Md5::digest(&body));
        let r = cx
            .client
            .put_object(
                &b,
                "locked",
                body,
                &[
                    ("x-amz-object-lock-mode", "GOVERNANCE"),
                    ("x-amz-object-lock-retain-until-date", until.as_str()),
                    ("content-md5", md5.as_str()),
                ],
            )
            .await;
        let version = cx
            .record("put-locked", r)
            .and_then(|r| r.header("x-amz-version-id").map(String::from));
        cx.send(
            "get-retention",
            s3raw::S3Request::new("GetObjectRetention", http::Method::GET)
                .bucket(&b)
                .key("locked")
                .query("retention", ""),
        )
        .await;
        if let Some(v) = version {
            cx.send(
                "delete-version",
                s3raw::S3Request::new("DeleteObject", http::Method::DELETE)
                    .bucket(&b)
                    .key("locked")
                    .query("versionId", v.as_str()),
            )
            .await;
            cx.note("version", v);
        }
    }
    fn judge(&self, s: &Steps) -> Judgement {
        if s.get("create-bucket").is_none() {
            return Judgement::new(
                Verdict::Skipped,
                "needs a temporary bucket (allow bucket creation)",
                s,
            );
        }
        let (create, put, ret, del) = (
            s.at("create-bucket"),
            s.at("put-locked"),
            s.at("get-retention"),
            s.at("delete-version"),
        );
        if !create.ok() {
            return Judgement::new(
                Verdict::Unsupported,
                format!(
                    "a bucket with Object Lock could not be created: {}",
                    create.describe()
                ),
                s,
            );
        }
        if !put.ok() {
            return Judgement::new(
                Verdict::Unsupported,
                format!("a PUT with retention was refused: {}", put.describe()),
                s,
            );
        }
        if del.ok() {
            return Judgement::new(
                Verdict::Fail,
                "a version under GOVERNANCE retention was deleted without bypass",
                s,
            );
        }
        if del.is(403) && ret.ok() && ret.body.contains("GOVERNANCE") {
            Judgement::new(Verdict::Pass, "retention enforced and reported", s)
        } else {
            Judgement::new(
                Verdict::Differs,
                format!(
                    "delete refused with {:?}; retention read {:?}",
                    del.status, ret.status
                ),
                s,
            )
        }
    }
}

pub struct SseC;

#[async_trait]
impl Probe for SseC {
    fn meta(&self) -> ProbeMeta {
        meta(
            "sse-c",
            Category::Encryption,
            "SSE-C",
            "Writes an object with a customer key, reads it without the key, with a wrong key and with the right key.",
            "PUT with the key: 200. GET without it: 400 InvalidRequest; with a wrong key: 403 AccessDenied; with the right key: 200. Over plain HTTP S3 refuses SSE-C altogether (400).",
            &[Needs::Https],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let key = [7u8; 32];
        let wrong = [8u8; 32];
        let b64 = |k: &[u8]| base64::engine::general_purpose::STANDARD.encode(k);
        let md5 = |k: &[u8]| base64::engine::general_purpose::STANDARD.encode(md5::Md5::digest(k));
        let (k64, kmd5, w64, wmd5) = (b64(&key), md5(&key), b64(&wrong), md5(&wrong));
        let hs = |k: &str, m: &str| {
            vec![
                (
                    "x-amz-server-side-encryption-customer-algorithm".to_string(),
                    "AES256".to_string(),
                ),
                (
                    "x-amz-server-side-encryption-customer-key".to_string(),
                    k.to_string(),
                ),
                (
                    "x-amz-server-side-encryption-customer-key-md5".to_string(),
                    m.to_string(),
                ),
            ]
        };
        let k = cx.key("ssec");
        let right = hs(&k64, &kmd5);
        let right_ref: Vec<(&str, &str)> = right
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        cx.note("https", cx.https.to_string());
        cx.put("put", &k, "secret", &right_ref).await;
        cx.get("no-key", &k, &[]).await;
        let w = hs(&w64, &wmd5);
        let w_ref: Vec<(&str, &str)> = w.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        cx.get("wrong-key", &k, &w_ref).await;
        cx.get("right-key", &k, &right_ref).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let https = s.at("https").body == "true";
        let (put, none, wrong, right) = (
            s.at("put"),
            s.at("no-key"),
            s.at("wrong-key"),
            s.at("right-key"),
        );
        if !put.ok() {
            if !https && put.is(400) {
                return Judgement::new(
                    Verdict::Pass,
                    "refused over HTTP, as AWS does (run over HTTPS to probe further)",
                    s,
                );
            }
            return Judgement::new(
                Verdict::Unsupported,
                format!("SSE-C refused: {}", put.describe()),
                s,
            );
        }
        if none.ok() || wrong.ok() {
            return Judgement::new(Verdict::Fail, "the object was readable without its key", s);
        }
        if !right.ok() || right.body != "secret" {
            return Judgement::new(
                Verdict::Fail,
                "the right key did not read the object back",
                s,
            );
        }
        let mut j = if none.is(400) && wrong.is(403) {
            Judgement::new(Verdict::Pass, "key required and checked", s)
        } else {
            Judgement::new(
                Verdict::Differs,
                format!("no key → {:?}, wrong key → {:?}", none.status, wrong.status),
                s,
            )
        };
        if !https {
            j.verdict = Verdict::Differs;
            j.summary = format!(
                "SSE-C accepted over plain HTTP (AWS refuses); {}",
                j.summary
            );
        }
        j
    }
}

pub struct ExpressAppend;

#[async_trait]
impl Probe for ExpressAppend {
    fn meta(&self) -> ProbeMeta {
        meta(
            "express.append",
            Category::Express,
            "S3 Express append (x-amz-write-offset-bytes)",
            "Appends to an object with PUT x-amz-write-offset-bytes at its size, then at a wrong offset.",
            "Directory buckets (S3 Express One Zone): the append succeeds and a wrong offset gives 400 InvalidWriteOffset. General purpose buckets: 501 NotImplemented.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("append");
        cx.put("create", &k, data(5, 1), &[]).await;
        cx.put(
            "append",
            &k,
            data(3, 2),
            &[("x-amz-write-offset-bytes", "5")],
        )
        .await;
        cx.put(
            "wrong-offset",
            &k,
            data(3, 2),
            &[("x-amz-write-offset-bytes", "2")],
        )
        .await;
        cx.head("head", &k, &[]).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (append, wrong, head) = (s.at("append"), s.at("wrong-offset"), s.at("head"));
        if append.not_implemented() || (!append.ok() && head.header("content-length") == Some("5"))
        {
            return Judgement::new(
                Verdict::Unsupported,
                format!("no appends: {}", append.describe()),
                s,
            );
        }
        let len = head.header("content-length").unwrap_or("?");
        match len {
            "8" if !wrong.ok() => {
                Judgement::new(Verdict::Pass, "appended; a wrong offset refused", s)
            }
            "8" => Judgement::new(Verdict::Fail, "a wrong offset was accepted", s),
            "3" => Judgement::new(
                Verdict::Fail,
                "the header was ignored: the append replaced the object",
                s,
            ),
            other => Judgement::new(
                Verdict::Differs,
                format!("object length {other} after the appends"),
                s,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::presign_timing;

    #[test]
    fn the_presigned_url_outlives_a_few_seconds_of_clock_difference() {
        // by the store's clock the URL is fresh at once and expired after the wait
        for d in [-45i64, -3, -2, 0, 2, 3, 45] {
            let (valid, wait) = presign_timing(Some(d)).unwrap();
            let (valid, wait) = (valid as i64, wait as i64);
            assert!(valid - d >= 8, "skew {d}: fresh for {} s", valid - d);
            assert!(wait + d >= valid + 3, "skew {d}: expired at the second use");
        }
        // unknown clocks: 4 s either way is absorbed
        let (valid, wait) = presign_timing(None).unwrap();
        assert!(valid >= 10 && wait >= valid + 5);
        assert_eq!(presign_timing(Some(600)), None);
    }
}
