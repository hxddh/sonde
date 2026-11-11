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
use crate::phrase::{self, Phrase, ph};
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_text: Vec<Phrase>,
    /// The rule sonde suggests adding.
    pub suggested_rule: String,
    pub error: Option<S3Error>,
}

impl LifecycleVerdict {
    /// A verdict with its message as phrases (and their English).
    pub fn say(
        status: LifecycleStatus,
        rules: Vec<AbortRule>,
        message: impl Into<Vec<Phrase>>,
        suggested_rule: String,
        error: Option<S3Error>,
    ) -> Self {
        let message_text = message.into();
        LifecycleVerdict {
            status,
            rules,
            message: phrase::en(&message_text),
            message_text,
            suggested_rule,
            error,
        }
    }
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
        return LifecycleVerdict::say(
            LifecycleStatus::Missing,
            vec![],
            ph!("lifecycle.none", days = suggest_days),
            suggested_rule,
            None,
        );
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
            ph!(
                "lifecycle.covered",
                rule = best.id.clone().unwrap_or_default(),
                prefix = format!("{:?}", best.prefix.clone().unwrap_or_default()),
                days = best.days
            ),
        )
    } else if rules.is_empty() {
        (
            LifecycleStatus::Missing,
            ph!("lifecycle.noAbort", n = lc.rule.len()),
        )
    } else {
        (
            LifecycleStatus::Partial,
            ph!(
                "lifecycle.partial",
                n = rules.len(),
                prefix = format!("{prefix:?}")
            ),
        )
    };
    LifecycleVerdict::say(status, rules, message, suggested_rule, None)
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
        ph!("job.mpu", bucket = b, prefix = p),
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
        ctx.progress(Progress::say(
            uploads.len() as u64,
            None,
            0,
            ph!("progress.uploads", n = uploads.len()),
        ));
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
                ctx.progress(Progress::say(
                    n,
                    Some(total),
                    0,
                    ph!("progress.listParts", n = n, total = total),
                ));
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
            Err(e) => LifecycleVerdict::say(
                LifecycleStatus::Unknown,
                vec![],
                ph!("lifecycle.unreadable", error = e),
                xml::abort_rule_xml("sonde-abort-incomplete-mpu", p, settings.stale_upload_days),
                Some(e),
            ),
        },
        Err(e) => LifecycleVerdict::say(
            LifecycleStatus::Unknown,
            vec![],
            ph!("lifecycle.failed", error = e),
            xml::abort_rule_xml("sonde-abort-incomplete-mpu", p, settings.stale_upload_days),
            Some(e),
        ),
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
        .map(|u| {
            PlanItem::new(
                u.key.clone(),
                Some(u.upload_id.clone()),
                Some(u.bytes),
                ph!(
                    "plan.item.upload",
                    parts = u.parts,
                    started = u.initiated.clone().unwrap_or_default()
                ),
            )
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
    let doc = merged(current.as_deref(), prefix, days)?;
    Ok(s.guard().plan(
        Action::PutLifecycle,
        endpoint,
        bucket,
        vec![PlanItem::new(
            prefix,
            None,
            None,
            ph!("plan.item.abortRule", days = days),
        )],
        Some(doc),
        current,
    ))
}

/// The bucket's lifecycle document with sonde's abort rule for `prefix`
/// added (or replaced). An error when the rule cannot be put into the
/// document as fetched with every other rule kept: a PUT replaces the whole
/// configuration, so a document sonde cannot splice is never planned over.
fn merged(current: Option<&str>, prefix: &str, days: u32) -> Result<String, S3Error> {
    // one rule per prefix, replaced when planned again: two rules with one
    // ID are refused by the service
    let id = abort_rule_id(prefix);
    let rule = xml::abort_rule_xml(&id, prefix, days);
    let refuse = |why: String| {
        S3Error::refused(format!(
            "the bucket's lifecycle configuration {why}: adding a rule would replace the rules it has. Nothing was planned; add the rule by hand:\n{rule}"
        ))
    };
    let doc = match current.filter(|c| !c.trim().is_empty()) {
        None => xml::add_rule(None, &rule),
        Some(c) => {
            let before =
                xml::lifecycle(c).map_err(|e| refuse(format!("could not be read ({e})")))?;
            let kept = before
                .rule
                .iter()
                .filter(|r| r.id.as_deref() != Some(id.as_str()))
                .count();
            let rest = without_rule(c, &id);
            if !rest.contains("</LifecycleConfiguration>") {
                // no place to add to; only a document without rules may be replaced
                if kept > 0 || rest.to_ascii_lowercase().contains("rule") {
                    return Err(refuse(
                        "is not written the way sonde can add to (no </LifecycleConfiguration>)"
                            .into(),
                    ));
                }
                xml::add_rule(None, &rule)
            } else {
                let doc = xml::add_rule(Some(&rest), &rule);
                // every rule that was there is there still, and ours once
                let after = xml::lifecycle(&doc).map_err(|e| {
                    S3Error::decode(format!("the merged document does not parse: {e}"))
                })?;
                if after.rule.len() != kept + 1 {
                    return Err(refuse(format!(
                        "has {kept} other rules and the merged document would have {}",
                        after.rule.len().saturating_sub(1)
                    )));
                }
                doc
            }
        }
    };
    xml::lifecycle(&doc)
        .map_err(|e| S3Error::decode(format!("the merged document does not parse: {e}")))?;
    Ok(doc)
}

/// The ID of sonde's abort rule for `prefix`: the same for the same prefix,
/// another for another (IDs are unique in a lifecycle configuration).
fn abort_rule_id(prefix: &str) -> String {
    use sha1::Digest;
    const ID: &str = "sonde-abort-incomplete-mpu";
    if prefix.is_empty() {
        return ID.into();
    }
    let h = hex::encode(&sha1::Sha1::digest(prefix.as_bytes())[..4]);
    format!("{ID}-{h}")
}

/// `doc` without its rules whose ID is `id` (the rest verbatim).
fn without_rule(doc: &str, id: &str) -> String {
    let needle = format!("<ID>{id}</ID>");
    let mut out = String::with_capacity(doc.len());
    let mut rest = doc;
    while let Some(start) = rest.find("<Rule>") {
        let Some(len) = rest[start..].find("</Rule>") else {
            break;
        };
        let end = start + len + "</Rule>".len();
        out.push_str(&rest[..start]);
        if !rest[start..end].contains(&needle) {
            out.push_str(&rest[start..end]);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
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
            v.message,
            "Covered: rule logs aborts incomplete uploads under \"logs/\" after 3 days."
        );
        // every verdict's message is phrases that say its English
        for v in [
            v,
            judge_lifecycle(None, "", 7),
            judge_lifecycle(Some(&lc), "", 7),
        ] {
            assert!(!v.message.is_empty());
            assert_eq!(phrase::en(&v.message_text), v.message);
            let json = serde_json::to_value(&v).unwrap();
            assert!(json["messageText"][0]["key"].as_str().unwrap().starts_with("lifecycle."));
        }
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

    #[test]
    fn planning_the_abort_rule_again_never_repeats_a_rule_id() {
        let ids = |doc: &str| -> Vec<String> {
            xml::lifecycle(doc)
                .unwrap()
                .rule
                .iter()
                .filter_map(|r| r.id.clone())
                .collect()
        };
        let add =
            |current: Option<&str>, prefix: &str, days: u32| merged(current, prefix, days).unwrap();
        let user = "<LifecycleConfiguration><Rule><ID>exp</ID><Filter/><Status>Enabled</Status><Expiration><Days>30</Days></Expiration></Rule></LifecycleConfiguration>";
        let a = add(Some(user), "a/", 7);
        let b = add(Some(&a), "b/", 7);
        // again for a/, with other days: replaced, not added
        let again = add(Some(&b), "a/", 3);
        let got = ids(&again);
        assert_eq!(got.len(), 3, "{got:?}");
        let unique: std::collections::BTreeSet<&String> = got.iter().collect();
        assert_eq!(unique.len(), 3, "{got:?}");
        assert!(got.contains(&"exp".to_string()), "the user's rule is kept");
        assert!(again.contains("<DaysAfterInitiation>3</DaysAfterInitiation>"));
        assert_eq!(
            again.matches("<DaysAfterInitiation>7<").count(),
            1,
            "b/ only"
        );
        assert_eq!(abort_rule_id(""), "sonde-abort-incomplete-mpu");
    }

    #[test]
    fn a_lifecycle_document_with_no_place_to_add_to_is_never_planned_over() {
        // rules, but no closing tag as sonde looks for it: a namespace
        // prefix, another case, a document cut short
        for doc in [
            "<s3:LifecycleConfiguration xmlns:s3=\"http://s3.amazonaws.com/doc/2006-03-01/\"><s3:Rule><s3:ID>exp</s3:ID><s3:Filter/><s3:Status>Enabled</s3:Status><s3:Expiration><s3:Days>30</s3:Days></s3:Expiration></s3:Rule></s3:LifecycleConfiguration>",
            "<LifecycleConfiguration><Rule><ID>exp</ID><Filter/><Status>Enabled</Status><Expiration><Days>30</Days></Expiration></Rule></LifecycleConfiguration >",
            "<LifecycleConfiguration><Rule><ID>exp</ID><Filter/><Status>Enabled</Status>",
            "{\"Rules\": [{\"ID\": \"exp\"}]}",
        ] {
            let e = merged(Some(doc), "", 7).unwrap_err();
            assert_eq!(e.kind, s3raw::ErrorKind::Refused, "{doc}: {e}");
            assert!(e.message.contains("would replace the rules it has"), "{e}");
            assert!(
                e.message.contains("<AbortIncompleteMultipartUpload>"),
                "{e}"
            );
        }
        // no document, an empty one, one without rules: a new one loses nothing
        for doc in [
            None,
            Some(""),
            Some("<LifecycleConfiguration/>"),
            Some("<LifecycleConfiguration></LifecycleConfiguration>"),
        ] {
            let got = merged(doc, "", 7).unwrap();
            assert_eq!(xml::lifecycle(&got).unwrap().rule.len(), 1, "{doc:?}");
        }
        // and the usual document keeps its rule
        let user = "<LifecycleConfiguration><Rule><ID>exp</ID><Filter/><Status>Enabled</Status><Expiration><Days>30</Days></Expiration></Rule></LifecycleConfiguration>";
        let got = merged(Some(user), "", 7).unwrap();
        assert!(got.contains("<ID>exp</ID>"));
        assert_eq!(xml::lifecycle(&got).unwrap().rule.len(), 2);
    }

    #[tokio::test]
    async fn a_plan_is_refused_when_the_stores_document_cannot_be_added_to() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut c, _)) = l.accept().await {
                let mut buf = [0u8; 4096];
                let _ = c.read(&mut buf).await;
                let body = "<s3:LifecycleConfiguration xmlns:s3=\"http://s3.amazonaws.com/doc/2006-03-01/\"><s3:Rule><s3:ID>exp</s3:ID><s3:Status>Enabled</s3:Status></s3:Rule></s3:LifecycleConfiguration>";
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = c.write_all(reply.as_bytes()).await;
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        let mut e = crate::EndpointConfig::new("e", "e", &url, "us-east-1", crate::Vendor::Other);
        e.path_style = true;
        e.credentials = crate::CredentialSource::Anonymous;
        s.add_transient_endpoint(e).unwrap();
        let err = plan_lifecycle_rule(&s, "e", "b", "", 7).await.unwrap_err();
        assert_eq!(err.kind, s3raw::ErrorKind::Refused, "{err}");
    }
}
