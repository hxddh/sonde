//! Browsing: buckets, prefix / delimiter pages, object details,
//! downloads, presigned URLs and prefix search.

use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use s3raw::S3Error;
use s3raw::checksum::{self, StoredChecksum};
use s3raw::etag::{self, EtagInfo};
use s3raw::ops::ListV2;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::AsyncWriteExt;

use crate::Sonde;
use crate::jobs::Progress;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketEntry {
    pub name: String,
    pub created: Option<String>,
    pub region: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The full key or prefix.
    pub key: String,
    /// The part after the listed prefix.
    pub name: String,
    pub is_prefix: bool,
    pub size: u64,
    pub last_modified: Option<String>,
    pub etag: Option<String>,
    pub storage_class: Option<String>,
    /// Parts in a multipart ETag.
    pub parts: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub entries: Vec<Entry>,
    pub next_token: Option<String>,
    pub is_truncated: bool,
    pub key_count: usize,
    pub record_id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Sse {
    pub algorithm: Option<String>,
    pub kms_key_id: Option<String>,
    pub bucket_key_enabled: Option<bool>,
    pub customer_algorithm: Option<String>,
    pub customer_key_md5: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectDetail {
    pub bucket: String,
    pub key: String,
    pub size: Option<u64>,
    pub content_type: Option<String>,
    pub last_modified: Option<String>,
    pub version_id: Option<String>,
    pub storage_class: String,
    pub etag: Option<EtagInfo>,
    /// Part sizes consistent with a multipart ETag.
    pub part_size_guesses: Vec<u64>,
    pub checksums: Vec<StoredChecksum>,
    pub sse: Sse,
    pub metadata: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub restore: Option<String>,
    pub object_lock: Option<String>,
    pub record_id: u64,
}

pub async fn list_buckets(s: &Sonde, endpoint: &str) -> Result<Vec<BucketEntry>, S3Error> {
    let c = s.client(endpoint)?;
    match c.list_buckets().await {
        Ok(r) => Ok(r
            .buckets
            .bucket
            .into_iter()
            .map(|b| BucketEntry {
                name: b.name,
                created: b.creation_date,
                region: b.bucket_region,
            })
            .collect()),
        Err(e) => {
            // credentials scoped to named buckets: show those
            let configured = s
                .endpoint_config(endpoint)
                .map(|e| e.buckets)
                .unwrap_or_default();
            if configured.is_empty() {
                Err(e)
            } else {
                Ok(configured
                    .into_iter()
                    .map(|name| BucketEntry {
                        name,
                        created: None,
                        region: None,
                    })
                    .collect())
            }
        }
    }
}

/// One page under `prefix`, split by `delimiter` (`/` usually; `None` flat).
pub async fn list_page(
    s: &Sonde,
    endpoint: &str,
    bucket: &str,
    prefix: &str,
    delimiter: Option<&str>,
    token: Option<String>,
    max_keys: Option<u32>,
) -> Result<Page, S3Error> {
    let c = s.client(endpoint)?;
    let (r, resp) = c
        .list_objects_v2(
            bucket,
            &ListV2 {
                prefix: Some(prefix.into()),
                delimiter: delimiter.map(String::from),
                continuation_token: token,
                max_keys,
                url_encoding: true,
                ..Default::default()
            },
        )
        .await?;
    let strip = |k: &str| k.strip_prefix(prefix).unwrap_or(k).to_string();
    let mut entries: Vec<Entry> = r
        .common_prefixes
        .iter()
        .map(|p| Entry {
            name: strip(&p.prefix),
            key: p.prefix.clone(),
            is_prefix: true,
            size: 0,
            last_modified: None,
            etag: None,
            storage_class: None,
            parts: None,
        })
        .collect();
    entries.extend(r.contents.iter().map(|o| Entry {
        name: strip(&o.key),
        key: o.key.clone(),
        is_prefix: false,
        size: o.size,
        last_modified: o.last_modified.clone(),
        parts: o.etag.as_deref().and_then(|e| etag::parse(e).parts()),
        etag: o.etag.clone(),
        storage_class: o.storage_class.clone(),
    }));
    Ok(Page {
        key_count: entries.len(),
        entries,
        next_token: r.next_continuation_token,
        is_truncated: r.is_truncated,
        record_id: resp.record_id,
    })
}

pub async fn object_detail(
    s: &Sonde,
    endpoint: &str,
    bucket: &str,
    key: &str,
    version: Option<&str>,
) -> Result<ObjectDetail, S3Error> {
    let c = s.client(endpoint)?;
    let h = c.head_object(bucket, key, version).await?;
    let get = |n: &str| {
        h.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n))
            .map(|(_, v)| v.clone())
    };
    let et = h.etag.as_deref().map(etag::parse);
    let guesses = match (&et, h.size) {
        (Some(e), Some(size)) => e
            .parts()
            .map(|n| etag::guess_part_sizes(size, n))
            .unwrap_or_default(),
        _ => vec![],
    };
    Ok(ObjectDetail {
        bucket: bucket.into(),
        key: key.into(),
        size: h.size,
        content_type: get("content-type"),
        last_modified: get("last-modified"),
        version_id: get("x-amz-version-id"),
        storage_class: get("x-amz-storage-class").unwrap_or_else(|| "STANDARD".into()),
        etag: et,
        part_size_guesses: guesses,
        checksums: checksum::from_headers(&h.headers),
        sse: Sse {
            algorithm: get("x-amz-server-side-encryption"),
            kms_key_id: get("x-amz-server-side-encryption-aws-kms-key-id"),
            bucket_key_enabled: get("x-amz-server-side-encryption-bucket-key-enabled")
                .map(|v| v == "true"),
            customer_algorithm: get("x-amz-server-side-encryption-customer-algorithm"),
            customer_key_md5: get("x-amz-server-side-encryption-customer-key-md5"),
        },
        restore: get("x-amz-restore"),
        object_lock: get("x-amz-object-lock-mode").map(|m| {
            format!(
                "{m} until {}",
                get("x-amz-object-lock-retain-until-date").unwrap_or_default()
            )
        }),
        metadata: h.metadata,
        headers: h.headers,
        record_id: h.record_id,
    })
}

pub async fn presign(
    s: &Sonde,
    endpoint: &str,
    bucket: &str,
    key: &str,
    expires_secs: u64,
    version: Option<&str>,
) -> Result<String, S3Error> {
    let c = s.client(endpoint)?;
    let q: Vec<(String, String)> = version
        .map(|v| vec![("versionId".to_string(), v.to_string())])
        .unwrap_or_default();
    c.presign("GET", bucket, key, expires_secs.clamp(1, 604_800), &q)
        .await
}

/// A file name for the object `key`: its last segment, without what a
/// file system refuses (separators, `* ? " < > |`, control characters,
/// names of dots, Windows' device names).
pub fn download_name(key: &str) -> String {
    let last = key.rsplit('/').find(|p| !p.is_empty()).unwrap_or("");
    let name: String = last
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let name = name.trim_end_matches(['.', ' ']).trim_start();
    if name.is_empty() {
        return "object".into();
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(|c: char| c.is_ascii_digit() && c != '0'));
    if device {
        format!("_{name}")
    } else {
        name.to_string()
    }
}

/// `path` with `suffix` after its whole name (`a.csv` → `a.csv.sonde-partial`).
fn with_suffix(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut n = path.file_name().unwrap_or_default().to_os_string();
    n.push(suffix);
    path.with_file_name(n)
}

const PARTIAL: &str = ".sonde-partial";

/// A destination no file or download holds: `dest`, else `name (2).ext`,
/// `name (3).ext`… Its partial file is created here, which reserves it.
async fn reserve(dest: &std::path::Path) -> Result<(PathBuf, PathBuf, tokio::fs::File), S3Error> {
    let stem = dest
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let ext = dest
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 1..10_000 {
        let to = if n == 1 {
            dest.to_path_buf()
        } else {
            dest.with_file_name(format!("{stem} ({n}){ext}"))
        };
        if tokio::fs::try_exists(&to).await.unwrap_or(true) {
            continue;
        }
        let part = with_suffix(&to, PARTIAL);
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
            .await
        {
            Ok(f) => return Ok((to, part, f)),
            // another download is writing this name
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(S3Error::config(format!("{}: {e}", part.display()))),
        }
    }
    Err(S3Error::config(format!(
        "{}: no free name beside it",
        dest.display()
    )))
}

/// Streams an object to `dest` as a job, or beside it under a free name
/// (`name (2).ext`) when `dest` exists: a download never replaces a file.
/// A cancelled or failed download removes its partial file.
pub fn download(s: &Arc<Sonde>, endpoint: &str, bucket: &str, key: &str, dest: PathBuf) -> String {
    let (s2, ep, b, k) = (
        s.clone(),
        endpoint.to_string(),
        bucket.to_string(),
        key.to_string(),
    );
    let settings = s.settings();
    // cooperative: a cancelled download still gets to remove its partial file
    s.jobs().spawn_cooperative(
        "download",
        &format!("Download {b}/{k}"),
        1,
        settings.rate_limit,
        move |ctx| async move {
            let mut part = None;
            let r = tokio::select! {
                r = fetch(&s2, &ep, &b, &k, &dest, &ctx, &mut part) => r,
                _ = ctx.cancel.cancelled() => Err(S3Error::cancelled()),
            };
            if r.is_err()
                && let Some(p) = part
                && let Err(e) = tokio::fs::remove_file(&p).await
                && e.kind() != std::io::ErrorKind::NotFound
            {
                // said where the job shows: the file is the user's to remove
                tracing::warn!("partial download {} left behind: {e}", p.display());
                ctx.progress(Progress {
                    message: format!("left behind: {} ({e})", p.display()),
                    ..Default::default()
                });
            }
            r
        },
    )
}

async fn fetch(
    s: &Sonde,
    endpoint: &str,
    bucket: &str,
    key: &str,
    dest: &std::path::Path,
    ctx: &crate::jobs::JobCtx,
    partial: &mut Option<PathBuf>,
) -> Result<serde_json::Value, S3Error> {
    let c = s.job_client(endpoint, ctx)?;
    let (resp, record_id) = c
        .send_stream(
            s3raw::S3Request::new("GetObject", http::Method::GET)
                .bucket(bucket)
                .key(key),
        )
        .await?;
    let total = resp.content_length();
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| S3Error::config(format!("{}: {e}", dir.display())))?;
    }
    let (dest, part, mut file) = reserve(dest).await?;
    *partial = Some(part.clone());
    let mut stream = resp.bytes_stream();
    let mut written = 0u64;
    while let Some(chunk) = stream.next().await {
        ctx.check()?;
        let chunk = chunk.map_err(|e| S3Error::new(s3raw::ErrorKind::Transport, e.to_string()))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| S3Error::config(e.to_string()))?;
        written += chunk.len() as u64;
        ctx.progress(Progress {
            done: written,
            total,
            bytes: written,
            message: format!("{written} bytes"),
        });
    }
    file.flush()
        .await
        .map_err(|e| S3Error::config(e.to_string()))?;
    drop(file);
    tokio::fs::rename(&part, &dest)
        .await
        .map_err(|e| S3Error::config(e.to_string()))?;
    Ok(json!({"path": dest, "bytes": written, "recordId": record_id}))
}

/// Finds keys under `prefix` whose name contains `needle` (case-insensitive),
/// as a job that streams matches; stops at `limit` matches.
pub fn search(
    s: &Arc<Sonde>,
    endpoint: &str,
    bucket: &str,
    prefix: &str,
    needle: &str,
    limit: usize,
) -> String {
    let (s2, ep, b, p, n) = (
        s.clone(),
        endpoint.to_string(),
        bucket.to_string(),
        prefix.to_string(),
        needle.to_lowercase(),
    );
    let settings = s.settings();
    s.jobs().spawn(
        "search",
        &format!("Search {b}/{p}*{needle}*"),
        1,
        settings.rate_limit,
        move |ctx| async move {
            let c = s2.job_client(&ep, &ctx)?;
            let mut token = None;
            let mut scanned = 0u64;
            let mut found: Vec<Entry> = Vec::new();
            loop {
                ctx.check()?;
                let (page, _) = c
                    .list_objects_v2(
                        &b,
                        &ListV2 {
                            prefix: Some(p.clone()),
                            continuation_token: token.clone(),
                            url_encoding: true,
                            ..Default::default()
                        },
                    )
                    .await?;
                scanned += page.contents.len() as u64;
                let hits: Vec<Entry> = page
                    .contents
                    .iter()
                    .filter(|o| o.key.to_lowercase().contains(&n))
                    .map(|o| Entry {
                        name: o.key.clone(),
                        key: o.key.clone(),
                        is_prefix: false,
                        size: o.size,
                        last_modified: o.last_modified.clone(),
                        etag: o.etag.clone(),
                        storage_class: o.storage_class.clone(),
                        parts: o.etag.as_deref().and_then(|e| etag::parse(e).parts()),
                    })
                    .collect();
                if !hits.is_empty() {
                    ctx.partial(json!(hits));
                }
                found.extend(hits);
                ctx.progress(Progress {
                    done: scanned,
                    total: None,
                    bytes: 0,
                    message: format!("{scanned} keys scanned, {} matches", found.len()),
                });
                if found.len() >= limit
                    || !page.is_truncated
                    || page.next_continuation_token.is_none()
                {
                    break;
                }
                token = page.next_continuation_token;
            }
            found.truncate(limit);
            Ok(json!({"scanned": scanned, "matches": found}))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_download_name_is_one_a_file_system_takes() {
        assert_eq!(download_name("a/b/report.csv"), "report.csv");
        assert_eq!(download_name("dir/"), "dir");
        assert_eq!(download_name("x/."), "object");
        assert_eq!(download_name("x/.."), "object");
        assert_eq!(download_name(""), "object");
        assert_eq!(download_name("a/w*h?a\"t<>|:\\.txt"), "w_h_a_t_____.txt");
        assert_eq!(download_name("a/con.txt"), "_con.txt");
        assert_eq!(download_name("a/COM1"), "_COM1");
        assert_eq!(download_name("a/common.txt"), "common.txt");
    }

    #[tokio::test]
    async fn downloads_of_one_name_do_not_replace_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        crate::demo::install(&s).await.unwrap();
        let dest = dir.path().join("dl").join("README.md");
        let mut paths = Vec::new();
        for _ in 0..2 {
            let job = download(&s, "demo", "demo", "README.md", dest.clone());
            let r = s.jobs().wait(&job).await.unwrap();
            paths.push(PathBuf::from(r["path"].as_str().unwrap()));
        }
        assert_eq!(paths[0], dest);
        assert_eq!(paths[1], dir.path().join("dl").join("README (2).md"));
        assert!(paths.iter().all(|p| p.exists()));
        // the partial is the whole name and a suffix: data.csv and data.json differ
        assert_eq!(
            with_suffix(std::path::Path::new("d/data.csv"), PARTIAL),
            PathBuf::from("d/data.csv.sonde-partial")
        );
    }

    #[tokio::test]
    async fn a_cancelled_download_removes_its_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        // a server that sends the head and half the body, then waits
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let (mut c, _) = l.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = c.read(&mut buf).await;
            let _ = c
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\nhalf-")
                .await;
            let _ = c.flush().await;
            std::future::pending::<()>().await
        });
        let mut e =
            crate::EndpointConfig::new("slow", "slow", &url, "us-east-1", crate::Vendor::Other);
        e.path_style = true;
        e.credentials = crate::CredentialSource::Anonymous;
        s.add_transient_endpoint(e).unwrap();
        let dest = dir.path().join("dl").join("big.bin");
        let part = with_suffix(&dest, PARTIAL);
        let job = download(&s, "slow", "b", "big.bin", dest.clone());
        for _ in 0..200 {
            if part.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(part.exists(), "the download began");
        assert!(s.jobs().cancel(&job));
        assert!(s.jobs().wait(&job).await.is_err());
        assert!(!part.exists(), "the partial file is gone");
        assert!(!dest.exists());
    }
}
