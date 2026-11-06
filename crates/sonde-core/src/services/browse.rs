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

/// Streams an object to `dest` as a job; a cancelled or failed download
/// removes its partial file.
pub fn download(s: &Arc<Sonde>, endpoint: &str, bucket: &str, key: &str, dest: PathBuf) -> String {
    let (s2, ep, b, k) = (
        s.clone(),
        endpoint.to_string(),
        bucket.to_string(),
        key.to_string(),
    );
    let settings = s.settings();
    s.jobs().spawn(
        "download",
        &format!("Download {b}/{k}"),
        1,
        settings.rate_limit,
        move |ctx| async move {
            let c = s2.job_client(&ep, &ctx)?;
            let (resp, record_id) = c
                .send_stream(
                    s3raw::S3Request::new("GetObject", http::Method::GET)
                        .bucket(&b)
                        .key(&k),
                )
                .await?;
            let total = resp.content_length();
            if let Some(dir) = dest.parent() {
                tokio::fs::create_dir_all(dir)
                    .await
                    .map_err(|e| S3Error::config(format!("{}: {e}", dir.display())))?;
            }
            let part = dest.with_extension("sonde-partial");
            let mut file = tokio::fs::File::create(&part)
                .await
                .map_err(|e| S3Error::config(format!("{}: {e}", part.display())))?;
            let mut stream = resp.bytes_stream();
            let mut written = 0u64;
            let result: Result<(), S3Error> = async {
                while let Some(chunk) = stream.next().await {
                    ctx.check()?;
                    let chunk = chunk
                        .map_err(|e| S3Error::new(s3raw::ErrorKind::Transport, e.to_string()))?;
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
                Ok(())
            }
            .await;
            drop(file);
            if let Err(e) = result {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(e);
            }
            tokio::fs::rename(&part, &dest)
                .await
                .map_err(|e| S3Error::config(e.to_string()))?;
            Ok(json!({"path": dest, "bytes": written, "recordId": record_id}))
        },
    )
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
