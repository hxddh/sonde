//! The layout lens over an endpoint: an S3 [`decoders::Source`], detection
//! and analysis as jobs.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use decoders::{Format, ObjectMeta, Source, SourceError};
use s3raw::ops::ListV2;
use s3raw::{S3Client, S3Error};
use serde_json::json;

use crate::Sonde;
use crate::jobs::{JobCtx, Progress};
use crate::phrase::ph;

/// A bucket as a decoder source: read-only (ListObjectsV2 and GET).
pub struct S3Source {
    pub client: S3Client,
    pub bucket: String,
    /// Listing stops here (protects the UI from unbounded scans).
    pub max_objects: usize,
    pub ctx: Option<JobCtx>,
}

#[async_trait]
impl Source for S3Source {
    async fn list(&self, prefix: &str) -> Result<Vec<ObjectMeta>, SourceError> {
        let mut out = vec![];
        let mut token = None;
        loop {
            if let Some(c) = &self.ctx {
                c.check().map_err(|e| SourceError(e.to_string()))?;
            }
            let (page, _) = self
                .client
                .list_objects_v2(
                    &self.bucket,
                    &ListV2 {
                        prefix: Some(prefix.into()),
                        continuation_token: token,
                        url_encoding: true,
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| SourceError(e.to_string()))?;
            out.extend(page.contents.into_iter().map(|o| {
                ObjectMeta {
                    last_modified: o
                        .last_modified
                        .as_deref()
                        .and_then(crate::services::parse_time),
                    key: o.key,
                    size: o.size,
                }
            }));
            if let Some(c) = &self.ctx {
                c.progress(Progress::say(
                    out.len() as u64,
                    None,
                    0,
                    ph!("progress.listed", n = out.len()),
                ));
            }
            if out.len() >= self.max_objects
                || !page.is_truncated
                || page.next_continuation_token.is_none()
            {
                break;
            }
            token = page.next_continuation_token;
        }
        Ok(out)
    }

    async fn get(&self, key: &str, range: Option<(u64, u64)>) -> Result<Bytes, SourceError> {
        let r = range.map(|(s, e)| format!("bytes={s}-{e}"));
        self.client
            .get_object(&self.bucket, key, r.as_deref())
            .await
            .map(|r| r.body)
            .map_err(|e| SourceError(e.to_string()))
    }
}

pub const DEFAULT_MAX_OBJECTS: usize = 200_000;

/// Detects layouts under `prefix` (a job; result: `[Detection]`).
pub fn detect(s: &Arc<Sonde>, endpoint: &str, bucket: &str, prefix: &str) -> String {
    let (s2, ep, b, p) = (
        s.clone(),
        endpoint.to_string(),
        bucket.to_string(),
        prefix.to_string(),
    );
    let settings = s.settings();
    s.jobs().spawn(
        "layout-detect",
        ph!("job.layoutDetect", bucket = b, prefix = p),
        settings.concurrency,
        settings.rate_limit,
        move |ctx| async move {
            let src = S3Source {
                client: s2.job_client(&ep, &ctx)?,
                bucket: b,
                max_objects: DEFAULT_MAX_OBJECTS,
                ctx: Some(ctx.clone()),
            };
            let found = decoders::detect(&src, &p)
                .await
                .map_err(|e| S3Error::decode(e.0))?;
            Ok(json!(found))
        },
    )
}

/// Analyzes the layout `format` at `root` (a job; result: `Layout`).
pub fn analyze(s: &Arc<Sonde>, endpoint: &str, bucket: &str, format: Format, root: &str) -> String {
    let (s2, ep, b, r) = (
        s.clone(),
        endpoint.to_string(),
        bucket.to_string(),
        root.to_string(),
    );
    let settings = s.settings();
    s.jobs().spawn(
        "layout",
        ph!("job.layout", format = format.label(), bucket = b, root = r),
        settings.concurrency,
        settings.rate_limit,
        move |ctx| async move {
            let src = S3Source {
                client: s2.job_client(&ep, &ctx)?,
                bucket: b,
                max_objects: DEFAULT_MAX_OBJECTS,
                ctx: Some(ctx.clone()),
            };
            let layout = decoders::analyze(&src, format, &r)
                .await
                .map_err(|e| S3Error::decode(e.0))?;
            Ok(serde_json::to_value(layout).unwrap_or_default())
        },
    )
}
