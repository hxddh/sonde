//! "Why is this bucket so big?": current and noncurrent bytes, delete
//! markers, incomplete multipart uploads, and their distribution by
//! storage class, top-level prefix and object size.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use s3raw::S3Error;
use s3raw::ops::ListV2;
use serde::{Deserialize, Serialize};

use crate::Sonde;
use crate::guard::human_bytes;
use crate::jobs::{JobCtx, Progress};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agg {
    pub objects: u64,
    pub bytes: u64,
}

impl Agg {
    fn add(&mut self, bytes: u64) {
        self.objects += 1;
        self.bytes += bytes;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slice {
    pub name: String,
    pub current: Agg,
    pub noncurrent: Agg,
    pub delete_markers: u64,
}

impl Slice {
    pub fn total_bytes(&self) -> u64 {
        self.current.bytes + self.noncurrent.bytes
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeBand {
    pub label: String,
    pub min: u64,
    /// Exclusive; `None` for the last band.
    pub max: Option<u64>,
    pub objects: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuReport {
    pub endpoint: String,
    pub bucket: String,
    pub prefix: String,
    /// `ListObjectVersions`, or `ListObjectsV2` where versions cannot be listed.
    pub source: String,
    pub current: Agg,
    pub noncurrent: Agg,
    pub delete_markers: u64,
    pub multipart: Agg,
    pub total_bytes: u64,
    pub by_class: Vec<Slice>,
    pub by_prefix: Vec<Slice>,
    pub by_size: Vec<SizeBand>,
    /// Findings, the largest contributor first.
    pub findings: Vec<String>,
    pub cost_per_month: f64,
    pub currency: String,
    pub scanned_at: String,
    pub requests: u64,
}

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const GIB: u64 = 1024 * MIB;

pub fn size_bands() -> Vec<SizeBand> {
    let edges: [(u64, &str); 8] = [
        (0, "< 1 KiB"),
        (KIB, "1–128 KiB"),
        (128 * KIB, "128 KiB–1 MiB"),
        (MIB, "1–16 MiB"),
        (16 * MIB, "16–128 MiB"),
        (128 * MIB, "128 MiB–1 GiB"),
        (GIB, "1–5 GiB"),
        (5 * GIB, "≥ 5 GiB"),
    ];
    edges
        .iter()
        .enumerate()
        .map(|(i, (lo, label))| SizeBand {
            label: label.to_string(),
            min: *lo,
            max: edges.get(i + 1).map(|e| e.0),
            objects: 0,
            bytes: 0,
        })
        .collect()
}

/// Folds listings into a report.
#[derive(Debug, Default)]
pub struct Accumulator {
    pub prefix: String,
    pub current: Agg,
    pub noncurrent: Agg,
    pub delete_markers: u64,
    pub classes: BTreeMap<String, Slice>,
    pub prefixes: BTreeMap<String, Slice>,
    pub bands: Vec<SizeBand>,
}

impl Accumulator {
    pub fn new(prefix: &str) -> Self {
        Accumulator {
            prefix: prefix.into(),
            bands: size_bands(),
            ..Default::default()
        }
    }

    fn top(&self, key: &str) -> String {
        let rest = key.strip_prefix(&self.prefix).unwrap_or(key);
        match rest.find('/') {
            Some(i) => format!("{}{}", self.prefix, &rest[..=i]),
            None => "(objects at this level)".to_string(),
        }
    }

    pub fn object(&mut self, key: &str, size: u64, class: Option<&str>, current: bool) {
        let class = class.unwrap_or("STANDARD").to_string();
        let top = self.top(key);
        let c = self.classes.entry(class.clone()).or_insert_with(|| Slice {
            name: class,
            ..Default::default()
        });
        let p = self.prefixes.entry(top.clone()).or_insert_with(|| Slice {
            name: top,
            ..Default::default()
        });
        if current {
            self.current.add(size);
            c.current.add(size);
            p.current.add(size);
        } else {
            self.noncurrent.add(size);
            c.noncurrent.add(size);
            p.noncurrent.add(size);
        }
        if let Some(b) = self
            .bands
            .iter_mut()
            .find(|b| size >= b.min && b.max.is_none_or(|m| size < m))
        {
            b.objects += 1;
            b.bytes += size;
        }
    }

    pub fn delete_marker(&mut self, key: &str) {
        self.delete_markers += 1;
        let top = self.top(key);
        self.prefixes
            .entry(top.clone())
            .or_insert_with(|| Slice {
                name: top,
                ..Default::default()
            })
            .delete_markers += 1;
    }

    pub fn finish(
        self,
        s: &Sonde,
        endpoint: &str,
        bucket: &str,
        source: &str,
        multipart: Agg,
        requests: u64,
    ) -> DuReport {
        let prices = s.settings().prices;
        let mut by_class: Vec<Slice> = self.classes.into_values().collect();
        by_class.sort_by_key(|c| std::cmp::Reverse(c.total_bytes()));
        let mut by_prefix: Vec<Slice> = self.prefixes.into_values().collect();
        by_prefix.sort_by_key(|c| std::cmp::Reverse(c.total_bytes()));
        let cost_per_month = by_class
            .iter()
            .map(|c| c.total_bytes() as f64 / GIB as f64 * prices.per_gib_month(Some(&c.name)))
            .sum::<f64>()
            + multipart.bytes as f64 / GIB as f64 * prices.default_per_gib_month;
        let total_bytes = self.current.bytes + self.noncurrent.bytes + multipart.bytes;
        let mut r = DuReport {
            endpoint: endpoint.into(),
            bucket: bucket.into(),
            prefix: self.prefix,
            source: source.into(),
            current: self.current,
            noncurrent: self.noncurrent,
            delete_markers: self.delete_markers,
            multipart,
            total_bytes,
            by_class,
            by_prefix,
            by_size: self.bands,
            findings: vec![],
            cost_per_month,
            currency: prices.currency,
            scanned_at: Utc::now().to_rfc3339(),
            requests,
        };
        r.findings = findings(&r);
        r
    }
}

fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 * 100.0 / whole as f64
    }
}

/// The report's findings, biggest first.
pub fn findings(r: &DuReport) -> Vec<String> {
    let mut f: Vec<(u64, String)> = Vec::new();
    let t = r.total_bytes;
    if t == 0 {
        return vec![format!(
            "{} holds no bytes{}.",
            if r.prefix.is_empty() {
                r.bucket.clone()
            } else {
                format!("{}/{}", r.bucket, r.prefix)
            },
            if r.delete_markers > 0 {
                format!(" but {} delete markers", r.delete_markers)
            } else {
                String::new()
            }
        )];
    }
    f.push((
        r.current.bytes,
        format!(
            "Current versions: {} in {} objects ({:.1}%).",
            human_bytes(r.current.bytes),
            r.current.objects,
            pct(r.current.bytes, t)
        ),
    ));
    if r.noncurrent.bytes > 0 {
        let mut s = format!(
            "Noncurrent versions: {} in {} versions ({:.1}%), invisible to plain listings.",
            human_bytes(r.noncurrent.bytes),
            r.noncurrent.objects,
            pct(r.noncurrent.bytes, t)
        );
        if pct(r.noncurrent.bytes, t) > 20.0 {
            s.push_str(" A NoncurrentVersionExpiration lifecycle rule would reclaim them.");
        }
        f.push((r.noncurrent.bytes, s));
    }
    if r.multipart.bytes > 0 {
        f.push((r.multipart.bytes, format!("Incomplete multipart uploads: {} in {} uploads ({:.1}%), billed but not listed as objects. See Multipart.", human_bytes(r.multipart.bytes), r.multipart.objects, pct(r.multipart.bytes, t))));
    }
    if r.delete_markers > 0 {
        let s = if r.delete_markers > r.current.objects {
            format!(
                "{} delete markers, more than the {} current objects: listings slow down; ExpiredObjectDeleteMarker cleans up the ones with no versions left.",
                r.delete_markers, r.current.objects
            )
        } else {
            format!("{} delete markers.", r.delete_markers)
        };
        f.push((0, s));
    }
    if let Some(p) = r.by_prefix.first()
        && r.by_prefix.len() > 1
        && pct(p.total_bytes(), t) >= 50.0
    {
        f.push((
            p.total_bytes(),
            format!(
                "Prefix {} holds {:.1}% of the bytes ({}).",
                p.name,
                pct(p.total_bytes(), t),
                human_bytes(p.total_bytes())
            ),
        ));
    }
    if let Some(c) = r.by_class.first()
        && r.by_class.len() > 1
    {
        f.push((
            c.total_bytes() / 2,
            format!(
                "Storage class {} holds {:.1}% of the bytes.",
                c.name,
                pct(c.total_bytes(), t)
            ),
        ));
    }
    let small: u64 = r
        .by_size
        .iter()
        .filter(|b| b.max.is_some_and(|m| m <= 128 * KIB))
        .map(|b| b.objects)
        .sum();
    let objects = r.current.objects + r.noncurrent.objects;
    if objects > 1000 && pct(small, objects) > 50.0 {
        f.push((0, format!("{:.0}% of objects are under 128 KiB: request costs dominate, and IA classes bill each as 128 KiB.", pct(small, objects))));
    }
    f.sort_by_key(|x| std::cmp::Reverse(x.0));
    f.into_iter().map(|x| x.1).collect()
}

/// Scans as a job; the result is a [`DuReport`], also cached on disk.
pub fn scan(
    s: &Arc<Sonde>,
    endpoint: &str,
    bucket: &str,
    prefix: &str,
    include_multipart: bool,
) -> String {
    let (s2, ep, b, p) = (
        s.clone(),
        endpoint.to_string(),
        bucket.to_string(),
        prefix.to_string(),
    );
    let settings = s.settings();
    s.jobs().spawn(
        "du",
        &format!("Capacity scan {b}/{p}"),
        settings.concurrency,
        settings.rate_limit,
        move |ctx| async move {
            let r = scan_inner(&s2, &ep, &b, &p, include_multipart, &ctx).await?;
            let _ = save_cache(&s2, &r);
            Ok(serde_json::to_value(r).unwrap_or_default())
        },
    )
}

pub async fn scan_inner(
    s: &Sonde,
    ep: &str,
    b: &str,
    p: &str,
    include_multipart: bool,
    ctx: &JobCtx,
) -> Result<DuReport, S3Error> {
    let c = s.job_client(ep, ctx)?;
    let mut acc = Accumulator::new(p);
    let mut requests = 0u64;
    let mut source = "ListObjectVersions";
    let (mut km, mut vm): (Option<String>, Option<String>) = (None, None);
    loop {
        ctx.check()?;
        let page = match c
            .list_object_versions(b, Some(p), km.as_deref(), vm.as_deref(), None)
            .await
        {
            Ok(page) => page,
            // 403: credentials that may list objects but not versions
            Err(e)
                if requests == 0
                    && matches!(e.status, Some(400) | Some(403) | Some(405) | Some(501)) =>
            {
                source = "ListObjectsV2";
                break;
            }
            Err(e) => return Err(e),
        };
        requests += 1;
        for v in &page.version {
            acc.object(&v.key, v.size, v.storage_class.as_deref(), v.is_latest);
        }
        for d in &page.delete_marker {
            acc.delete_marker(&d.key);
        }
        ctx.progress(Progress {
            done: acc.current.objects + acc.noncurrent.objects,
            total: None,
            bytes: acc.current.bytes + acc.noncurrent.bytes,
            message: format!(
                "{} versions, {} delete markers",
                acc.current.objects + acc.noncurrent.objects,
                acc.delete_markers
            ),
        });
        if !page.is_truncated || page.next_key_marker.is_none() {
            break;
        }
        km = page.next_key_marker;
        vm = page.next_version_id_marker;
    }
    if source == "ListObjectsV2" {
        let mut token = None;
        loop {
            ctx.check()?;
            let (page, _) = c
                .list_objects_v2(
                    b,
                    &ListV2 {
                        prefix: Some(p.into()),
                        continuation_token: token,
                        url_encoding: true,
                        ..Default::default()
                    },
                )
                .await?;
            requests += 1;
            for o in &page.contents {
                acc.object(&o.key, o.size, o.storage_class.as_deref(), true);
            }
            ctx.progress(Progress {
                done: acc.current.objects,
                total: None,
                bytes: acc.current.bytes,
                message: format!("{} objects", acc.current.objects),
            });
            if !page.is_truncated || page.next_continuation_token.is_none() {
                break;
            }
            token = page.next_continuation_token;
        }
    }
    let mut uncounted = None;
    let multipart = if include_multipart {
        match crate::services::mpu::scan_inner(s, ep, b, p, ctx).await {
            Ok(m) => Agg {
                objects: m.total_uploads as u64,
                bytes: m.total_bytes,
            },
            Err(e) if e.kind == s3raw::ErrorKind::Cancelled => return Err(e),
            Err(e) => {
                uncounted = Some(e);
                Agg::default()
            }
        }
    } else {
        Agg::default()
    };
    let mut report = acc.finish(s, ep, b, source, multipart, requests);
    if let Some(e) = uncounted {
        // zero here is "unknown", and the report must say so
        report.findings.insert(
            0,
            format!("Incomplete multipart uploads were not counted: {e}"),
        );
    }
    Ok(report)
}

fn cache_file(s: &Sonde, ep: &str, b: &str, p: &str) -> PathBuf {
    use sha1::Digest;
    let h = hex::encode(&sha1::Sha1::digest(p.as_bytes())[..8]);
    s.paths()
        .scan_cache()
        .join(ep)
        .join(b)
        .join(format!("{h}.json"))
}

pub fn save_cache(s: &Sonde, r: &DuReport) -> Result<PathBuf, String> {
    let f = cache_file(s, &r.endpoint, &r.bucket, &r.prefix);
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&f, serde_json::to_vec_pretty(r).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(f)
}

pub fn load_cache(s: &Sonde, ep: &str, b: &str, p: &str) -> Option<DuReport> {
    serde_json::from_slice(&std::fs::read(cache_file(s, ep, b, p)).ok()?).ok()
}

/// The report as CSV: one row per top-level prefix.
pub fn to_csv(r: &DuReport) -> String {
    let mut out = String::from(
        "prefix,current_objects,current_bytes,noncurrent_objects,noncurrent_bytes,delete_markers\n",
    );
    for p in &r.by_prefix {
        let name = if p.name.contains([',', '"', '\n']) {
            format!("\"{}\"", p.name.replace('"', "\"\""))
        } else {
            p.name.clone()
        };
        out.push_str(&format!(
            "{name},{},{},{},{},{}\n",
            p.current.objects,
            p.current.bytes,
            p.noncurrent.objects,
            p.noncurrent.bytes,
            p.delete_markers
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates() {
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        let mut a = Accumulator::new("");
        a.object("logs/a", 10 * MIB, Some("STANDARD"), true);
        a.object("logs/a", 30 * MIB, Some("STANDARD"), false);
        a.object("img/x", 100, Some("STANDARD_IA"), true);
        a.object("top", 5, None, true);
        a.delete_marker("logs/b");
        let r = a.finish(
            &s,
            "e",
            "b",
            "ListObjectVersions",
            Agg {
                objects: 1,
                bytes: 60 * MIB,
            },
            1,
        );
        assert_eq!(r.current.objects, 3);
        assert_eq!(r.noncurrent.bytes, 30 * MIB);
        assert_eq!(r.by_prefix[0].name, "logs/");
        assert_eq!(r.by_prefix[0].delete_markers, 1);
        assert_eq!(r.by_class[0].name, "STANDARD");
        assert_eq!(
            r.by_size
                .iter()
                .find(|b| b.label == "< 1 KiB")
                .unwrap()
                .objects,
            2
        );
        assert_eq!(
            r.by_size
                .iter()
                .find(|b| b.label == "16–128 MiB")
                .unwrap()
                .objects,
            1
        );
        assert!(
            r.findings[0].starts_with("Incomplete multipart uploads"),
            "{:?}",
            r.findings
        );
        assert!(
            r.findings
                .iter()
                .any(|f| f.contains("NoncurrentVersionExpiration"))
        );
        let path = save_cache(&s, &r).unwrap();
        assert!(path.exists());
        assert_eq!(
            load_cache(&s, "e", "b", "").unwrap().total_bytes,
            r.total_bytes
        );
        assert!(to_csv(&r).lines().count() == 4);
    }

    #[test]
    fn empty_report() {
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        let r = Accumulator::new("x/").finish(&s, "e", "b", "ListObjectsV2", Agg::default(), 1);
        assert_eq!(r.findings, vec!["b/x/ holds no bytes."]);
    }

    #[tokio::test]
    async fn a_scan_that_may_not_list_versions_or_uploads_says_so() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // a store that refuses ListObjectVersions and ListMultipartUploads
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut c, _)) = l.accept().await {
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    loop {
                        let mut chunk = [0u8; 4096];
                        match c.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                        let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                            continue;
                        };
                        let head = String::from_utf8_lossy(&buf[..end]).to_string();
                        buf.drain(..end + 4);
                        let target = head.split(' ').nth(1).unwrap_or("");
                        let (status, body) = if target.contains("versions")
                            || target.contains("uploads")
                        {
                            (
                                403,
                                "<Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>",
                            )
                        } else {
                            (
                                200,
                                "<ListBucketResult><Contents><Key>a</Key><Size>7</Size></Contents></ListBucketResult>",
                            )
                        };
                        let reply = format!(
                            "HTTP/1.1 {status} X\r\ncontent-length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        if c.write_all(reply.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        let mut e = crate::EndpointConfig::new("e", "e", &url, "us-east-1", crate::Vendor::Other);
        e.path_style = true;
        e.credentials = crate::CredentialSource::Anonymous;
        s.add_transient_endpoint(e).unwrap();
        let job = scan(&s, "e", "b", "", true);
        let r: DuReport = serde_json::from_value(s.jobs().wait(&job).await.unwrap()).unwrap();
        assert_eq!(r.source, "ListObjectsV2", "403 on versions falls back");
        assert_eq!(r.current.bytes, 7);
        assert!(
            r.findings[0].starts_with("Incomplete multipart uploads were not counted:"),
            "{:?}",
            r.findings
        );
    }
}
