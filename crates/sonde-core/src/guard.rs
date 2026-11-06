//! The read-only guard, dry-run plans and the audit log.
//!
//! A destructive operation (abort uploads, delete versions or delete
//! markers, delete objects, change lifecycle) is never executed directly.
//! It is first *planned*: the plan lists exactly what would change (the
//! dry run) and names the confirmation the user must type: the number of
//! items, or the bucket name. [`Guard::take`] hands the plan back for
//! execution only when read-only mode is off, the plan is fresh, and the
//! confirmation matches. Every execution is appended to the audit log.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use s3raw::S3Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Plans expire after this many seconds.
pub const PLAN_TTL_SECS: i64 = 600;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    AbortUploads,
    DeleteVersions,
    RestoreObject,
    DeleteObjects,
    PutLifecycle,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::AbortUploads => "abort multipart uploads",
            Action::DeleteVersions => "permanently delete versions",
            Action::RestoreObject => "remove the latest delete marker",
            Action::DeleteObjects => "delete objects",
            Action::PutLifecycle => "replace the lifecycle configuration",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub key: String,
    /// The version id or upload id the item names.
    pub id: Option<String>,
    pub bytes: Option<u64>,
    pub detail: String,
}

/// A dry run, waiting for confirmation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub action: Action,
    pub endpoint: String,
    pub bucket: String,
    pub items: Vec<PlanItem>,
    /// For lifecycle: the document that would be written, and the one now.
    pub document: Option<String>,
    pub previous_document: Option<String>,
    pub summary: String,
    /// What the user must type to confirm.
    pub confirm: String,
    /// "the number of items" or "the bucket name", for the prompt.
    pub confirm_hint: String,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub time: DateTime<Utc>,
    pub plan: String,
    pub action: Action,
    pub endpoint: String,
    pub bucket: String,
    pub items: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub errors: Vec<String>,
    pub request_ids: Vec<String>,
    pub targets: Vec<String>,
}

#[derive(Clone)]
pub struct Guard {
    plans: Arc<Mutex<HashMap<String, Plan>>>,
    audit_path: PathBuf,
}

impl Guard {
    pub fn new(audit_path: PathBuf) -> Self {
        Guard {
            plans: Arc::new(Mutex::new(HashMap::new())),
            audit_path,
        }
    }

    /// Records a plan. Items-based plans confirm with their count; plans of
    /// one item or of the whole bucket with the bucket name.
    pub fn plan(
        &self,
        action: Action,
        endpoint: &str,
        bucket: &str,
        items: Vec<PlanItem>,
        document: Option<String>,
        previous_document: Option<String>,
    ) -> Plan {
        let by_name =
            matches!(action, Action::PutLifecycle | Action::RestoreObject) || items.len() <= 1;
        let (confirm, confirm_hint) = if by_name {
            (bucket.to_string(), "the bucket name".to_string())
        } else {
            (items.len().to_string(), "the number of items".to_string())
        };
        let bytes: u64 = items.iter().filter_map(|i| i.bytes).sum();
        let summary = match action {
            Action::PutLifecycle => format!("Write a new lifecycle configuration to {bucket}"),
            _ => format!(
                "{} {} item(s) in {bucket} ({})",
                capitalize(action.label()),
                items.len(),
                human_bytes(bytes)
            ),
        };
        let plan = Plan {
            id: uuid::Uuid::new_v4().simple().to_string(),
            action,
            endpoint: endpoint.into(),
            bucket: bucket.into(),
            items,
            document,
            previous_document,
            summary,
            confirm,
            confirm_hint,
            created: Utc::now(),
        };
        self.plans.lock().insert(plan.id.clone(), plan.clone());
        plan
    }

    pub fn get(&self, id: &str) -> Option<Plan> {
        self.plans.lock().get(id).cloned()
    }

    pub fn discard(&self, id: &str) {
        self.plans.lock().remove(id);
    }

    /// The plan, if it may run now. Removes it (a plan runs once).
    pub fn take(&self, id: &str, read_only: bool, confirmation: &str) -> Result<Plan, S3Error> {
        if read_only {
            return Err(S3Error::refused(
                "read-only mode is on: turn it off in Settings to run destructive operations",
            ));
        }
        let mut plans = self.plans.lock();
        let plan = plans
            .get(id)
            .ok_or_else(|| S3Error::refused("no such plan: run the dry run again"))?;
        if (Utc::now() - plan.created).num_seconds() > PLAN_TTL_SECS {
            plans.remove(id);
            return Err(S3Error::refused(
                "the dry run is older than 10 minutes: run it again",
            ));
        }
        if confirmation.trim() != plan.confirm {
            return Err(S3Error::refused(format!(
                "confirmation does not match: type {}",
                plan.confirm_hint
            )));
        }
        Ok(plans.remove(id).unwrap())
    }

    pub fn audit(&self, entry: &AuditEntry) -> Result<(), String> {
        if let Some(dir) = self.audit_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit_path)
            .map_err(|e| e.to_string())?;
        writeln!(
            f,
            "{}",
            serde_json::to_string(entry).map_err(|e| e.to_string())?
        )
        .map_err(|e| e.to_string())
    }

    pub fn audit_entries(&self, limit: usize) -> Vec<AuditEntry> {
        let text = std::fs::read_to_string(&self.audit_path).unwrap_or_default();
        let mut v: Vec<AuditEntry> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        v.reverse();
        v.truncate(limit);
        v
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Outcome of executing a plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub succeeded: usize,
    pub failed: usize,
    pub errors: Vec<S3Error>,
    pub detail: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(n: usize) -> Vec<PlanItem> {
        (0..n)
            .map(|i| PlanItem {
                key: format!("k{i}"),
                id: Some(format!("u{i}")),
                bytes: Some(1024),
                detail: String::new(),
            })
            .collect()
    }

    #[test]
    fn three_conditions() {
        let dir = tempfile::tempdir().unwrap();
        let g = Guard::new(dir.path().join("audit.jsonl"));
        let p = g.plan(Action::AbortUploads, "e", "bucket", items(3), None, None);
        assert_eq!(p.confirm, "3");
        assert_eq!(
            p.summary,
            "Abort multipart uploads 3 item(s) in bucket (3.0 KiB)"
        );
        // read-only refuses
        assert!(
            g.take(&p.id, true, "3")
                .unwrap_err()
                .message
                .contains("read-only")
        );
        // wrong confirmation refuses and keeps the plan
        assert!(
            g.take(&p.id, false, "2")
                .unwrap_err()
                .message
                .contains("number of items")
        );
        // right one passes once
        assert_eq!(g.take(&p.id, false, " 3 ").unwrap().items.len(), 3);
        assert!(g.take(&p.id, false, "3").is_err());
        let one = g.plan(Action::DeleteVersions, "e", "bucket", items(1), None, None);
        assert_eq!(one.confirm, "bucket");
        let lc = g.plan(
            Action::PutLifecycle,
            "e",
            "bucket",
            vec![],
            Some("<x/>".into()),
            None,
        );
        assert_eq!(lc.confirm_hint, "the bucket name");
    }

    #[test]
    fn audit_log() {
        let dir = tempfile::tempdir().unwrap();
        let g = Guard::new(dir.path().join("a/audit.jsonl"));
        let e = AuditEntry {
            time: Utc::now(),
            plan: "p".into(),
            action: Action::DeleteVersions,
            endpoint: "e".into(),
            bucket: "b".into(),
            items: 2,
            succeeded: 1,
            failed: 1,
            errors: vec!["AccessDenied".into()],
            request_ids: vec!["R1".into()],
            targets: vec!["k@v".into()],
        };
        g.audit(&e).unwrap();
        g.audit(&e).unwrap();
        assert_eq!(g.audit_entries(10).len(), 2);
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(5 * 1024 * 1024 * 1024), "5.0 GiB");
    }
}
