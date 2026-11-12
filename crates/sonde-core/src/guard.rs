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

use crate::phrase::{self, Phrase, ph};

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
    /// A probe run: it writes and deletes under its scratch prefix (and
    /// buckets of its own). Never a plan; audited as a run.
    RunProbes,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::AbortUploads => "abort multipart uploads",
            Action::DeleteVersions => "permanently delete versions",
            Action::RestoreObject => "remove the latest delete marker",
            Action::DeleteObjects => "delete objects",
            Action::PutLifecycle => "replace the lifecycle configuration",
            Action::RunProbes => "run compatibility probes",
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_text: Vec<Phrase>,
}

impl PlanItem {
    /// An item with what would happen to it as phrases (and their English).
    pub fn new(
        key: impl Into<String>,
        id: Option<String>,
        bytes: Option<u64>,
        detail: impl Into<Vec<Phrase>>,
    ) -> PlanItem {
        let detail_text = detail.into();
        PlanItem {
            key: key.into(),
            id,
            bytes,
            detail: phrase::en(&detail_text),
            detail_text,
        }
    }
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_text: Vec<Phrase>,
    /// What the user must type to confirm.
    pub confirm: String,
    /// "the number of items" or "the bucket name", for the prompt.
    pub confirm_hint: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub confirm_hint_text: Vec<Phrase>,
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
    /// `started` on the entry written before the first request (a run
    /// that was cut short still shows); absent on the outcome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
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
        let (confirm, confirm_hint_text) = if by_name {
            (bucket.to_string(), vec![ph!("plan.confirm.bucket")])
        } else {
            (items.len().to_string(), vec![ph!("plan.confirm.count")])
        };
        let bytes: u64 = items.iter().filter_map(|i| i.bytes).sum();
        // a sentence an action, so another language orders its own words
        let key = match action {
            Action::PutLifecycle => "plan.putLifecycle",
            Action::AbortUploads => "plan.abortUploads",
            Action::DeleteVersions => "plan.deleteVersions",
            Action::RestoreObject => "plan.restoreObject",
            Action::DeleteObjects => "plan.deleteObjects",
            Action::RunProbes => "plan.runProbes",
        };
        let summary_text = vec![ph!(
            key,
            n = items.len(),
            bucket = bucket,
            size = human_bytes(bytes)
        )];
        let summary = phrase::en(&summary_text);
        let confirm_hint = phrase::en(&confirm_hint_text);
        let plan = Plan {
            id: uuid::Uuid::new_v4().simple().to_string(),
            action,
            endpoint: endpoint.into(),
            bucket: bucket.into(),
            items,
            document,
            previous_document,
            summary,
            summary_text,
            confirm,
            confirm_hint,
            confirm_hint_text,
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
        let mut f = reqlog::private::options()
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

#[cfg(test)]
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A size ([`sonde_fmt::bytes`]).
pub fn human_bytes(b: u64) -> String {
    sonde_fmt::bytes(b)
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
            .map(|i| PlanItem::new(format!("k{i}"), Some(format!("u{i}")), Some(1024), vec![]))
            .collect()
    }

    #[test]
    fn a_plan_says_its_summary_and_hint_as_phrases_for_every_action() {
        let dir = tempfile::tempdir().unwrap();
        let g = Guard::new(dir.path().join("audit.jsonl"));
        for action in [
            Action::AbortUploads,
            Action::DeleteVersions,
            Action::RestoreObject,
            Action::DeleteObjects,
            Action::RunProbes,
        ] {
            let p = g.plan(action.clone(), "e", "bucket", items(3), None, None);
            // the sentence is the action's label and its count, as before
            assert_eq!(
                p.summary,
                format!(
                    "{} 3 item(s) in bucket (3.0 KiB)",
                    capitalize(action.label())
                )
            );
            assert_eq!(phrase::en(&p.summary_text), p.summary);
            assert_eq!(phrase::en(&p.confirm_hint_text), p.confirm_hint);
        }
        let p = g.plan(Action::PutLifecycle, "e", "bucket", items(1), None, None);
        assert_eq!(p.summary, "Write a new lifecycle configuration to bucket");
        assert_eq!(p.confirm_hint, "the bucket name");
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["summaryText"][0]["key"], "plan.putLifecycle");
        assert_eq!(v["confirmHintText"][0]["key"], "plan.confirm.bucket");
        // a plan written before 15 still reads
        let mut old = v.clone();
        old.as_object_mut().unwrap().remove("summaryText");
        old.as_object_mut().unwrap().remove("confirmHintText");
        let back: Plan = serde_json::from_value(old).unwrap();
        assert!(back.summary_text.is_empty());
        let item = PlanItem::new("k", None, None, ph!("plan.item.version", v = "v1"));
        assert_eq!(item.detail, "version v1");
        assert_eq!(
            serde_json::to_value(&item).unwrap()["detailText"][0]["key"],
            "plan.item.version"
        );
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
            stage: None,
        };
        g.audit(&e).unwrap();
        g.audit(&e).unwrap();
        assert_eq!(g.audit_entries(10).len(), 2);
        // what was done to which keys is the user's alone
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(dir.path().join("a/audit.jsonl")).unwrap();
            assert_eq!(m.permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(5 * 1024 * 1024 * 1024), "5.0 GiB");
    }
}
