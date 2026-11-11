//! Versions and delete markers of one key, and the bucket's versioning.

use s3raw::S3Error;
use serde::{Deserialize, Serialize};

use crate::Sonde;
use crate::guard::{Action, Plan, PlanItem};
use crate::phrase::ph;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionEntry {
    pub version_id: String,
    pub is_latest: bool,
    pub is_delete_marker: bool,
    pub last_modified: Option<String>,
    pub size: u64,
    pub etag: Option<String>,
    pub storage_class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    pub bucket: String,
    pub key: String,
    /// Newest first.
    pub versions: Vec<VersionEntry>,
    /// The key reads as deleted: its latest version is a delete marker.
    pub deleted: bool,
    pub noncurrent_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersioningStatus {
    /// `Enabled`, `Suspended`, or `Off` (never enabled).
    pub status: String,
    /// `Enabled`, `Disabled`, or `None` when the service does not say.
    pub mfa_delete: Option<String>,
    pub object_lock: Option<String>,
}

pub async fn versioning(
    s: &Sonde,
    endpoint: &str,
    bucket: &str,
) -> Result<VersioningStatus, S3Error> {
    let c = s.client(endpoint)?;
    let v = c.bucket_versioning(bucket).await?;
    let lock = c
        .object_lock_configuration(bucket)
        .await
        .ok()
        .flatten()
        .map(|l| {
            let mut s = l.object_lock_enabled.unwrap_or_else(|| "Unknown".into());
            if let Some(r) = l.rule.and_then(|r| r.default_retention) {
                s.push_str(&format!(
                    " (default {} {} {})",
                    r.mode.unwrap_or_default(),
                    r.days.or(r.years).unwrap_or(0),
                    if r.days.is_some() { "days" } else { "years" }
                ));
            }
            s
        });
    Ok(VersioningStatus {
        status: v.status.unwrap_or_else(|| "Off".into()),
        mfa_delete: v.mfa_delete,
        object_lock: lock,
    })
}

/// Sorts newest first; equal times keep the service's order (newest first).
pub fn build_timeline(bucket: &str, key: &str, versions: Vec<VersionEntry>) -> Timeline {
    let mut versions = versions;
    versions.sort_by(|a, b| {
        let ta = a
            .last_modified
            .as_deref()
            .and_then(crate::services::parse_time);
        let tb = b
            .last_modified
            .as_deref()
            .and_then(crate::services::parse_time);
        tb.cmp(&ta).then(b.is_latest.cmp(&a.is_latest))
    });
    let deleted = versions
        .iter()
        .find(|v| v.is_latest)
        .is_some_and(|v| v.is_delete_marker);
    let noncurrent_bytes = versions
        .iter()
        .filter(|v| !v.is_latest && !v.is_delete_marker)
        .map(|v| v.size)
        .sum();
    Timeline {
        bucket: bucket.into(),
        key: key.into(),
        versions,
        deleted,
        noncurrent_bytes,
    }
}

pub async fn timeline(
    s: &Sonde,
    endpoint: &str,
    bucket: &str,
    key: &str,
) -> Result<Timeline, S3Error> {
    let c = s.client(endpoint)?;
    let mut out = Vec::new();
    let (mut km, mut vm): (Option<String>, Option<String>) = (None, None);
    loop {
        let page = c
            .list_object_versions(bucket, Some(key), km.as_deref(), vm.as_deref(), None)
            .await?;
        let mut past = false;
        for v in page.version {
            if v.key == key {
                out.push(VersionEntry {
                    version_id: v.version_id.unwrap_or_else(|| "null".into()),
                    is_latest: v.is_latest,
                    is_delete_marker: false,
                    last_modified: v.last_modified,
                    size: v.size,
                    etag: v.etag,
                    storage_class: v.storage_class,
                });
            } else if v.key.as_str() > key {
                past = true;
            }
        }
        for d in page.delete_marker {
            if d.key == key {
                out.push(VersionEntry {
                    version_id: d.version_id.unwrap_or_else(|| "null".into()),
                    is_latest: d.is_latest,
                    is_delete_marker: true,
                    last_modified: d.last_modified,
                    size: 0,
                    etag: None,
                    storage_class: None,
                });
            } else if d.key.as_str() > key {
                past = true;
            }
        }
        if past || !page.is_truncated || page.next_key_marker.as_deref().is_some_and(|k| k > key) {
            break;
        }
        km = page.next_key_marker;
        vm = page.next_version_id_marker;
        if km.is_none() {
            break;
        }
    }
    Ok(build_timeline(bucket, key, out))
}

/// A dry run of restoring a deleted key: removing its latest delete marker.
pub async fn plan_restore(
    s: &Sonde,
    endpoint: &str,
    bucket: &str,
    key: &str,
) -> Result<Plan, S3Error> {
    let t = timeline(s, endpoint, bucket, key).await?;
    let latest = t
        .versions
        .iter()
        .find(|v| v.is_latest)
        .ok_or_else(|| S3Error::refused(format!("{key} has no versions")))?;
    if !latest.is_delete_marker {
        return Err(S3Error::refused(format!(
            "{key} is not deleted: its latest version is not a delete marker"
        )));
    }
    let restores = t
        .versions
        .iter()
        .find(|v| !v.is_delete_marker)
        .map(|v| v.version_id.clone());
    let detail = match restores {
        Some(v) => ph!("plan.item.restore", marker = latest.version_id, version = v),
        None => ph!("plan.item.restore.nothing", marker = latest.version_id),
    };
    Ok(s.guard().plan(
        Action::RestoreObject,
        endpoint,
        bucket,
        vec![PlanItem::new(
            key,
            Some(latest.version_id.clone()),
            None,
            detail,
        )],
        None,
        None,
    ))
}

/// A dry run of permanently deleting versions (or delete markers) of keys.
pub fn plan_delete_versions(
    s: &Sonde,
    endpoint: &str,
    bucket: &str,
    targets: &[(String, String, Option<u64>)],
) -> Plan {
    let items = targets
        .iter()
        .map(|(k, v, b)| {
            PlanItem::new(
                k.clone(),
                Some(v.clone()),
                *b,
                ph!("plan.item.version", v = v),
            )
        })
        .collect();
    s.guard()
        .plan(Action::DeleteVersions, endpoint, bucket, items, None, None)
}

/// A dry run of deleting objects (current versions; a versioned bucket keeps them as noncurrent).
pub fn plan_delete_objects(
    s: &Sonde,
    endpoint: &str,
    bucket: &str,
    keys: &[(String, Option<u64>)],
) -> Plan {
    let items = keys
        .iter()
        .map(|(k, b)| PlanItem::new(k.clone(), None, *b, ph!("plan.item.delete")))
        .collect();
    s.guard()
        .plan(Action::DeleteObjects, endpoint, bucket, items, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(id: &str, t: &str, latest: bool, dm: bool, size: u64) -> VersionEntry {
        VersionEntry {
            version_id: id.into(),
            is_latest: latest,
            is_delete_marker: dm,
            last_modified: Some(t.into()),
            size,
            etag: None,
            storage_class: None,
        }
    }

    #[test]
    fn timeline_order_and_state() {
        let t = build_timeline(
            "b",
            "k",
            vec![
                v("1", "2024-01-01T00:00:00Z", false, false, 10),
                v("3", "2024-01-03T00:00:00Z", true, true, 0),
                v("2", "2024-01-02T00:00:00Z", false, false, 20),
            ],
        );
        assert_eq!(
            t.versions
                .iter()
                .map(|v| v.version_id.as_str())
                .collect::<Vec<_>>(),
            vec!["3", "2", "1"]
        );
        assert!(t.deleted);
        assert_eq!(t.noncurrent_bytes, 30);
        let t = build_timeline(
            "b",
            "k",
            vec![v("1", "2024-01-01T00:00:00Z", true, false, 10)],
        );
        assert!(!t.deleted);
    }
}
