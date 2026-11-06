//! Multipart upload health: every incomplete upload, its parts' bytes
//! (summed from ListParts), its age and what it costs, and whether a
//! lifecycle rule cleans such uploads up.

use std::sync::Arc;

use chrono::Utc;
use futures::stream::{self, StreamExt};
use s3raw::S3Error;
use s3raw::xml::{self, LifecycleConfiguration};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::Sonde;
use crate::guard::{Action, Plan, PlanItem};
use crate::jobs::Progress;
use crate::services::parse_time;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadRow {
    pub key: String,
    pub upload_id: String,
    pub initiated: Option<String>,
    pub age_days: Option<f64>,
    pub parts: u32,
    pub bytes: u64,
    pub storage_class: Option<String>,
    /// What the parts cost since the upload started.
    pub cost_so_far: f64,
    /// What they cost per month from now on.
    pub cost_per_month: f64,
    /// Older than the threshold.
    pub stale: bool,
    /// ListParts failed for this upload (the bytes are unknown).
    pub parts_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbortRule {
    pub id: Option<String>,
    pub prefix: Option<String>,
    pub days: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LifecycleStatus {
    /// An enabled rule covers the scanned prefix.
    Covered,
    /// Rules exist but none covers the whole scanned prefix.
    Partial,
    /// No AbortIncompleteMultipartUpload rule at all.
    Missing,
    /// The lifecycle could not be read.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleVerdict {
    pub status: LifecycleStatus,
    pub rules: Vec<AbortRule>,
    pub message: String,
    /// The rule sonde suggests adding.
    pub suggested_rule: String,
    pub error: Option<S3Error>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MpuReport {
    pub endpoint: String,
    pub bucket: String,
    pub prefix: String,
    pub threshold_days: u32,
    pub uploads: Vec<UploadRow>,
    pub total_uploads: usize,
    pub stale_uploads: usize,
    pub total_bytes: u64,
    pub stale_bytes: u64,
    pub cost_per_month: f64,
    pub cost_so_far: f64,
    pub currency: String,
    pub lifecycle: LifecycleVerdict,
    pub scanned_at: String,
}

/// Judges a lifecycle document against `prefix`.
pub fn judge_lifecycle(
    lc: Option<&LifecycleConfiguration>,
    prefix: &str,
    suggest_days: u32,
) -> LifecycleVerdict {
    let suggested_rule = xml::abort_rule_xml("sonde-abort-incomplete-mpu", prefix, suggest_days);
    let Some(lc) = lc else {
        return LifecycleVerdict {
            status: LifecycleStatus::Missing,
            rules: vec![],
            message: format!(
                "The bucket has no lifecycle configuration: incomplete uploads are kept (and billed) forever. Add a rule aborting them after {suggest_days} days."
            ),
            suggested_rule,
            error: None,
        };
    };
    let rules: Vec<AbortRule> = lc
        .rule
        .iter()
        .filter_map(|r| {
            let days = r
                .abort_incomplete_multipart_upload
                .as_ref()?
                .days_after_initiation?;
            Some(AbortRule {
                id: r.id.clone(),
                prefix: r.scope_prefix(),
                days,
                enabled: r.enabled(),
            })
        })
        .collect();
    let covering: Vec<&AbortRule> = rules
        .iter()
        .filter(|r| {
            r.enabled
                && r.prefix
                    .as_ref()
                    .is_some_and(|p| prefix.starts_with(p.as_str()))
        })
        .collect();
    let (status, message) = if let Some(best) = covering.iter().min_by_key(|r| r.days) {
        (
            LifecycleStatus::Covered,
            format!(
                "Covered: rule {} aborts incomplete uploads under {:?} after {} days.",
                best.id.clone().unwrap_or_default(),
                best.prefix.clone().unwrap_or_default(),
                best.days
            ),
        )
    } else if rules.is_empty() {
        (
            LifecycleStatus::Missing,
            format!(
                "The lifecycle has {} rule(s) but none with AbortIncompleteMultipartUpload: incomplete uploads are kept forever.",
                lc.rule.len()
            ),
        )
    } else {
        (
            LifecycleStatus::Partial,
            format!(
                "{} AbortIncompleteMultipartUpload rule(s) exist but none (enabled) covers {:?}.",
                rules.len(),
                prefix
            ),
        )
    };
    LifecycleVerdict {
        status,
        rules,
        message,
        suggested_rule,
        error: None,
    }
}

/// Scans a bucket's incomplete uploads as a job (`MpuReport` as result).
pub fn scan(s: &Arc<Sonde>, endpoint: &str, bucket: &str, prefix: &str) -> String {
    let (s2, ep, b, p) = (
        s.clone(),
        endpoint.to_string(),
        bucket.to_string(),
        prefix.to_string(),
    );
    let settings = s.settings();
    s.jobs().spawn(
        "mpu",
        &format!("Multipart scan {b}/{p}"),
        settings.concurrency,
        settings.rate_limit,
        move |ctx| async move {
            let report = scan_inner(&s2, &ep, &b, &p, &ctx).await?;
            Ok(serde_json::to_value(report).unwrap_or_default())
        },
    )
}

pub async fn scan_inner(
    s: &Sonde,
    ep: &str,
    b: &str,
    p: &str,
    ctx: &crate::jobs::JobCtx,
) -> Result<MpuReport, S3Error> {
    let settings = s.settings();
    let c = s.job_client(ep, ctx)?;
    // 1. list every upload
    let mut uploads = Vec::new();
    let (mut km, mut um): (Option<String>, Option<String>) = (None, None);
    loop {
        ctx.check()?;
        let page = c
            .list_multipart_uploads(b, Some(p), km.as_deref(), um.as_deref())
            .await?;
        uploads.extend(page.upload);
        ctx.progress(Progress {
            done: uploads.len() as u64,
            total: None,
            bytes: 0,
            message: format!("{} uploads listed", uploads.len()),
        });
        if !page.is_truncated
            || (page.next_key_marker.is_none() && page.next_upload_id_marker.is_none())
        {
            break;
        }
        km = page.next_key_marker;
        um = page.next_upload_id_marker;
    }
    let total = uploads.len() as u64;
    // 2. sum the parts of each, concurrently (the throttle bounds it)
    let now = Utc::now();
    let done = std::sync::atomic::AtomicU64::new(0);
    let rows: Vec<UploadRow> = stream::iter(uploads)
        .map(|u| {
            let c = c.clone();
            let b = b.to_string();
            let prices = settings.prices.clone();
            let done = &done;
            async move {
                let mut bytes = 0u64;
                let mut parts = 0u32;
                let mut marker: Option<String> = None;
                let mut parts_error = None;
                loop {
                    match c
                        .list_parts(&b, &u.key, &u.upload_id, marker.as_deref())
                        .await
                    {
                        Ok(lp) => {
                            parts += lp.part.len() as u32;
                            bytes += lp.part.iter().map(|p| p.size).sum::<u64>();
                            if !lp.is_truncated || lp.next_part_number_marker.is_none() {
                                break;
                            }
                            marker = lp.next_part_number_marker;
                        }
                        Err(e) => {
                            parts_error = Some(e.to_string());
                            break;
                        }
                    }
                }
                let age_days = u
                    .initiated
                    .as_deref()
                    .and_then(parse_time)
                    .map(|t| (now - t).num_seconds() as f64 / 86400.0);
                let gib = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                let price = prices.per_gib_month(u.storage_class.as_deref());
                let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                ctx.progress(Progress {
                    done: n,
                    total: Some(total),
                    bytes: 0,
                    message: format!("ListParts {n}/{total}"),
                });
                UploadRow {
                    stale: age_days.is_some_and(|a| a > settings.stale_upload_days as f64),
                    cost_so_far: gib * price * age_days.unwrap_or(0.0) / 30.0,
                    cost_per_month: gib * price,
                    key: u.key,
                    upload_id: u.upload_id,
                    initiated: u.initiated,
                    age_days,
                    parts,
                    bytes,
                    storage_class: u.storage_class,
                    parts_error,
                }
            }
        })
        .buffer_unordered(settings.concurrency.max(1))
        .collect()
        .await;
    ctx.check()?;
    let mut rows = rows;
    rows.sort_by(|a, b| {
        b.age_days
            .partial_cmp(&a.age_days)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // 3. lifecycle
    let lifecycle = match c.lifecycle_raw(b).await {
        Ok(None) => judge_lifecycle(None, p, settings.stale_upload_days),
        Ok(Some(text)) => match xml::lifecycle(&text) {
            Ok(lc) => judge_lifecycle(Some(&lc), p, settings.stale_upload_days),
            Err(e) => LifecycleVerdict {
                status: LifecycleStatus::Unknown,
                rules: vec![],
                message: format!("The lifecycle document could not be read: {e}"),
                suggested_rule: xml::abort_rule_xml(
                    "sonde-abort-incomplete-mpu",
                    p,
                    settings.stale_upload_days,
                ),
                error: Some(e),
            },
        },
        Err(e) => LifecycleVerdict {
            status: LifecycleStatus::Unknown,
            rules: vec![],
            message: format!("GetBucketLifecycleConfiguration failed: {e}"),
            suggested_rule: xml::abort_rule_xml(
                "sonde-abort-incomplete-mpu",
                p,
                settings.stale_upload_days,
            ),
            error: Some(e),
        },
    };
    let stale: Vec<&UploadRow> = rows.iter().filter(|r| r.stale).collect();
    Ok(MpuReport {
        endpoint: ep.into(),
        bucket: b.into(),
        prefix: p.into(),
        threshold_days: settings.stale_upload_days,
        total_uploads: rows.len(),
        stale_uploads: stale.len(),
        total_bytes: rows.iter().map(|r| r.bytes).sum(),
        stale_bytes: stale.iter().map(|r| r.bytes).sum(),
        cost_per_month: rows.iter().map(|r| r.cost_per_month).sum(),
        cost_so_far: rows.iter().map(|r| r.cost_so_far).sum(),
        currency: settings.prices.currency.clone(),
        uploads: rows,
        lifecycle,
        scanned_at: now.to_rfc3339(),
    })
}

/// A dry run of aborting the given uploads.
pub fn plan_abort(s: &Sonde, endpoint: &str, bucket: &str, uploads: &[UploadRow]) -> Plan {
    let items = uploads
        .iter()
        .map(|u| PlanItem {
            key: u.key.clone(),
            id: Some(u.upload_id.clone()),
            bytes: Some(u.bytes),
            detail: format!(
                "{} parts, started {}",
                u.parts,
                u.initiated.clone().unwrap_or_default()
            ),
        })
        .collect();
    s.guard()
        .plan(Action::AbortUploads, endpoint, bucket, items, None, None)
}

/// A dry run of adding an abort rule: the merged document, and the current one.
pub async fn plan_lifecycle_rule(
    s: &Sonde,
    endpoint: &str,
    bucket: &str,
    prefix: &str,
    days: u32,
) -> Result<Plan, S3Error> {
    let c = s.client(endpoint)?;
    let current = c.lifecycle_raw(bucket).await?;
    let rule = xml::abort_rule_xml("sonde-abort-incomplete-mpu", prefix, days);
    let doc = xml::add_rule(current.as_deref(), &rule);
    xml::lifecycle(&doc)
        .map_err(|e| S3Error::decode(format!("the merged document does not parse: {e}")))?;
    Ok(s.guard().plan(
        Action::PutLifecycle,
        endpoint,
        bucket,
        vec![PlanItem {
            key: prefix.into(),
            id: None,
            bytes: None,
            detail: format!("abort incomplete uploads after {days} days"),
        }],
        Some(doc),
        current,
    ))
}

pub fn summary_json(r: &MpuReport) -> serde_json::Value {
    json!({"uploads": r.total_uploads, "stale": r.stale_uploads, "bytes": r.total_bytes, "lifecycle": r.lifecycle.status})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts() {
        assert_eq!(
            judge_lifecycle(None, "", 7).status,
            LifecycleStatus::Missing
        );
        let lc = xml::lifecycle("<LifecycleConfiguration><Rule><ID>logs</ID><Filter><Prefix>logs/</Prefix></Filter><Status>Enabled</Status><AbortIncompleteMultipartUpload><DaysAfterInitiation>3</DaysAfterInitiation></AbortIncompleteMultipartUpload></Rule><Rule><ID>exp</ID><Filter/><Status>Enabled</Status><Expiration><Days>30</Days></Expiration></Rule></LifecycleConfiguration>").unwrap();
        let v = judge_lifecycle(Some(&lc), "logs/2024/", 7);
        assert_eq!(v.status, LifecycleStatus::Covered);
        assert!(v.message.contains("after 3 days"));
        assert_eq!(
            judge_lifecycle(Some(&lc), "", 7).status,
            LifecycleStatus::Partial
        );
        let none = xml::lifecycle("<LifecycleConfiguration><Rule><ID>exp</ID><Filter/><Status>Enabled</Status><Expiration><Days>30</Days></Expiration></Rule></LifecycleConfiguration>").unwrap();
        assert_eq!(
            judge_lifecycle(Some(&none), "", 7).status,
            LifecycleStatus::Missing
        );
        let disabled = xml::lifecycle("<LifecycleConfiguration><Rule><ID>a</ID><Filter><Prefix></Prefix></Filter><Status>Disabled</Status><AbortIncompleteMultipartUpload><DaysAfterInitiation>1</DaysAfterInitiation></AbortIncompleteMultipartUpload></Rule></LifecycleConfiguration>").unwrap();
        assert_eq!(
            judge_lifecycle(Some(&disabled), "", 7).status,
            LifecycleStatus::Partial
        );
    }
}
