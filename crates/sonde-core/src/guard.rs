//! The guard of sonde's own destructive acts: dry-run plans, the typed
//! confirmation, and the audit log.
//!
//! A destructive operation (abort uploads, delete versions or delete
//! markers, delete objects, change lifecycle) is never executed directly.
//! It is first *planned*: the plan lists exactly what would change (the
//! dry run) and names its confirmation: the number of items, or the bucket
//! name. It is typed only for more than [`TYPED_ABOVE`] items or a bucket's
//! lifecycle; a smaller plan is a plain confirm. [`Guard::take`] hands the
//! plan back for execution only when the plan is fresh and, where it is
//! typed, the confirmation matches. Every execution is appended to the audit log.
//!
//! The app's plans live in its process. A terminal's dry run and its
//! `--yes` are two processes, so the dry run's plan is kept in a file
//! ([`Guard::save`]: `<data>/plans/`, 0600, one per command) and `--yes`
//! takes that plan ([`Guard::take_saved`]): fresh, once, never one built
//! anew from what the store holds by then.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use s3raw::S3Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::phrase::{self, Phrase, ph};

/// Plans expire after this many seconds.
pub const PLAN_TTL_SECS: i64 = 600;

/// A plan of more items than this asks for its count typed; one of this
/// many or fewer, a plain confirm.
pub const TYPED_ABOVE: usize = 100;

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
    /// What confirms it.
    pub confirm: String,
    /// Whether [`Plan::confirm`] must be typed (more than [`TYPED_ABOVE`]
    /// items, a bucket's lifecycle); otherwise a plain confirm runs it.
    pub typed: bool,
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

/// The command a kept plan is of: what `--yes` must name again, exactly,
/// to run it. Two commands that differ in any of these never share a plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub action: Action,
    /// The endpoint's id and its URL (`--url` is `adhoc` whatever it names).
    pub endpoint: String,
    pub url: String,
    pub bucket: String,
    /// The command's own arguments, as it reads them (`prefix=tmp/ days=7`).
    pub arguments: String,
}

impl Scope {
    /// The name of the file its plan is kept in.
    fn file(&self) -> String {
        use sha1::Digest;
        let said = serde_json::to_vec(self).unwrap_or_default();
        format!("{}.json", hex::encode(sha1::Sha1::digest(said)))
    }
}

/// A plan as kept between a dry run and its `--yes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    scope: Scope,
    plan: Plan,
}

/// The time, as a [`Guard`] reads it.
type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

#[derive(Clone)]
pub struct Guard {
    plans: Arc<Mutex<HashMap<String, Plan>>>,
    audit_path: PathBuf,
    /// Where the plans of a terminal's dry runs are kept.
    plans_dir: PathBuf,
    clock: Clock,
}

impl Guard {
    /// A guard writing its audit log at `audit_path`, its kept plans in
    /// `plans/` beside it.
    pub fn new(audit_path: PathBuf) -> Self {
        Guard {
            plans: Arc::new(Mutex::new(HashMap::new())),
            plans_dir: audit_path.with_file_name("plans"),
            audit_path,
            clock: Arc::new(Utc::now),
        }
    }

    /// The same guard reading the time from `clock` (tests: a plan's age
    /// without waiting for it).
    pub fn with_clock(mut self, clock: impl Fn() -> DateTime<Utc> + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }

    /// When a plan stops being one that may run.
    pub fn expires(plan: &Plan) -> DateTime<Utc> {
        plan.created + chrono::Duration::seconds(PLAN_TTL_SECS)
    }

    /// Whether `plan` is too old to run (or from a time yet to come: a
    /// clock set back is no reason to run it).
    fn stale(&self, plan: &Plan) -> bool {
        let age = (self.now() - plan.created).num_seconds();
        !(-60..=PLAN_TTL_SECS).contains(&age)
    }

    /// Records a plan. Items-based plans confirm with their count; plans of
    /// one item or of the whole bucket with the bucket name. Typed only
    /// past [`TYPED_ABOVE`] items, and for a lifecycle.
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
        let typed = action == Action::PutLifecycle || items.len() > TYPED_ABOVE;
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
            typed,
            confirm_hint,
            confirm_hint_text,
            created: self.now(),
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
    pub fn take(&self, id: &str, confirmation: &str) -> Result<Plan, S3Error> {
        let mut plans = self.plans.lock();
        let plan = plans
            .get(id)
            .ok_or_else(|| S3Error::refused("no such plan: run the dry run again"))?;
        if self.stale(plan) {
            plans.remove(id);
            return Err(S3Error::refused(
                "the dry run is older than 10 minutes: run it again",
            ));
        }
        if plan.typed && confirmation.trim() != plan.confirm {
            return Err(S3Error::refused(format!(
                "confirmation does not match: type {}",
                plan.confirm_hint
            )));
        }
        Ok(plans.remove(id).unwrap())
    }

    /// Keeps `plan` as the dry run of the command `scope` says, in place of
    /// an earlier one of that command: the plan its `--yes` will run. The
    /// file is made 0600; plans too old to run are removed on the way.
    pub fn save(&self, scope: &Scope, plan: &Plan) -> Result<PathBuf, String> {
        std::fs::create_dir_all(&self.plans_dir)
            .map_err(|e| format!("{}: {e}", self.plans_dir.display()))?;
        self.sweep();
        let path = self.plans_dir.join(scope.file());
        let saved = Saved {
            scope: scope.clone(),
            plan: plan.clone(),
        };
        let bytes = serde_json::to_vec(&saved).map_err(|e| e.to_string())?;
        reqlog::private::replace(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    /// Removes what is left in the plans' folder past its time: plans never
    /// confirmed, and what a run cut short left behind.
    fn sweep(&self) {
        let Ok(dir) = std::fs::read_dir(&self.plans_dir) else {
            return;
        };
        for e in dir.flatten() {
            let path = e.path();
            let old = match read_saved(&path) {
                Some(s) => self.stale(&s.plan),
                // not a plan (half of one, a file claimed by a run that
                // died): gone once it is older than any plan may be
                None => e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|d| d.as_secs() > PLAN_TTL_SECS as u64),
            };
            if old {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// The plan the dry run of `scope`'s command kept, if it may run now:
    /// there, of that very command, fresh, and (`id` given) the one the dry
    /// run named. It is taken out of its folder before it is read (a rename:
    /// of two `--yes` at once one gets it, the other is refused), so a plan
    /// runs once, and nothing is sent before it is gone.
    pub fn take_saved(&self, scope: &Scope, id: Option<&str>) -> Result<Plan, S3Error> {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let none = || {
            S3Error::refused(
                "no dry run of this command to confirm: run it without --yes first (same endpoint, bucket and options), then again with --yes",
            )
        };
        let path = self.plans_dir.join(scope.file());
        // a plan asked for by id is left for its own `--yes` when another is there
        if let Some(id) = id {
            let there = read_saved(&path).ok_or_else(none)?;
            if there.plan.id != id {
                return Err(S3Error::refused(format!(
                    "plan {id} is not this command's latest dry run ({}): nothing was done",
                    there.plan.id
                )));
            }
        }
        let mine = self.plans_dir.join(format!(
            ".{}.{}.{}.taken",
            scope.file(),
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::rename(&path, &mine).map_err(|_| none())?;
        let saved = read_saved(&mine);
        let _ = std::fs::remove_file(&mine);
        let saved = saved.ok_or_else(none)?;
        if &saved.scope != scope {
            return Err(none());
        }
        if id.is_some_and(|id| id != saved.plan.id) {
            return Err(S3Error::refused(
                "another dry run replaced the plan: nothing was done; run the dry run again",
            ));
        }
        if self.stale(&saved.plan) {
            return Err(S3Error::refused(
                "the dry run is older than 10 minutes: run it again (without --yes), then --yes",
            ));
        }
        Ok(saved.plan)
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

fn read_saved(path: &Path) -> Option<Saved> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
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
    /// Why the outcome is not in the audit log (its `started` line stands
    /// alone there), when it could not be written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_error: Option<String>,
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
        // a plan without phrases still reads
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
    fn a_plan_runs_once_fresh_and_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let g = Guard::new(dir.path().join("audit.jsonl"));
        let p = g.plan(Action::AbortUploads, "e", "bucket", items(101), None, None);
        assert_eq!(p.confirm, "101");
        assert!(p.typed);
        assert_eq!(
            p.summary,
            "Abort multipart uploads 101 item(s) in bucket (101 KiB)"
        );
        // wrong confirmation refuses and keeps the plan
        assert!(
            g.take(&p.id, "2")
                .unwrap_err()
                .message
                .contains("number of items")
        );
        // right one passes once
        assert_eq!(g.take(&p.id, " 101 ").unwrap().items.len(), 101);
        assert!(g.take(&p.id, "101").is_err());
        // a hundred or fewer: a plain confirm, nothing typed
        let few = g.plan(Action::DeleteObjects, "e", "bucket", items(100), None, None);
        assert!(!few.typed);
        assert_eq!(g.take(&few.id, "").unwrap().items.len(), 100);
        let one = g.plan(Action::DeleteVersions, "e", "bucket", items(1), None, None);
        assert_eq!(one.confirm, "bucket");
        assert!(!one.typed);
        let lc = g.plan(
            Action::PutLifecycle,
            "e",
            "bucket",
            vec![],
            Some("<x/>".into()),
            None,
        );
        assert_eq!(lc.confirm_hint, "the bucket name");
        assert!(lc.typed);
    }

    fn scope(arguments: &str) -> Scope {
        Scope {
            action: Action::AbortUploads,
            endpoint: "e".into(),
            url: "http://store".into(),
            bucket: "bucket".into(),
            arguments: arguments.into(),
        }
    }

    /// A guard whose time is `at`'s, to move by hand.
    fn at(dir: &Path, at: &Arc<Mutex<DateTime<Utc>>>) -> Guard {
        let now = at.clone();
        Guard::new(dir.join("audit.jsonl")).with_clock(move || *now.lock())
    }

    /// 32.0's `--yes` made its own plan from what the bucket held by then:
    /// the dry run's is kept, for its own command, and runs once.
    #[test]
    fn a_kept_plan_is_its_commands_alone_and_runs_once() {
        let dir = tempfile::tempdir().unwrap();
        // another process: nothing shared but the folder
        let (dry, yes) = (
            Guard::new(dir.path().join("audit.jsonl")),
            Guard::new(dir.path().join("audit.jsonl")),
        );
        let none = yes.take_saved(&scope("p"), None).unwrap_err();
        assert_eq!(none.kind, s3raw::ErrorKind::Refused);
        assert!(none.message.contains("without --yes first"), "{none}");
        let p = dry.plan(Action::AbortUploads, "e", "bucket", items(2), None, None);
        let path = dry.save(&scope("p"), &p).unwrap();
        assert_eq!(path.parent().unwrap(), dir.path().join("plans"));
        assert_eq!(
            crate::Paths::rooted(dir.path()).plans(),
            Guard::new(crate::Paths::rooted(dir.path()).audit_log()).plans_dir
        );
        // what would be aborted is the user's alone, from the moment it is made
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // another command (its prefix, its option) has no plan
        assert!(yes.take_saved(&scope("q"), None).is_err());
        let mut other = scope("p");
        other.url = "http://another".into();
        assert!(yes.take_saved(&other, None).is_err());
        // a plan named that is not the one kept: refused, and the one kept stays
        let wrong = yes.take_saved(&scope("p"), Some("0000")).unwrap_err();
        assert!(
            wrong.message.contains("not this command's latest"),
            "{wrong}"
        );
        let taken = yes.take_saved(&scope("p"), Some(&p.id)).unwrap();
        assert_eq!(taken, p);
        let again = yes.take_saved(&scope("p"), None).unwrap_err();
        assert!(again.message.contains("without --yes first"), "{again}");
        assert_eq!(
            std::fs::read_dir(dir.path().join("plans")).unwrap().count(),
            0
        );
        // a dry run again replaces the plan: the newest is the one that runs
        let (a, b) = (
            dry.plan(Action::AbortUploads, "e", "bucket", items(1), None, None),
            dry.plan(Action::AbortUploads, "e", "bucket", items(3), None, None),
        );
        dry.save(&scope("p"), &a).unwrap();
        dry.save(&scope("p"), &b).unwrap();
        assert_eq!(yes.take_saved(&scope("p"), None).unwrap().id, b.id);
    }

    #[test]
    fn a_kept_plan_past_ten_minutes_is_refused_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let time = Arc::new(Mutex::new(Utc::now()));
        let g = at(dir.path(), &time);
        let p = g.plan(Action::AbortUploads, "e", "bucket", items(2), None, None);
        assert_eq!(p.created, *time.lock());
        assert_eq!(
            Guard::expires(&p),
            p.created + chrono::Duration::seconds(600)
        );
        g.save(&scope("p"), &p).unwrap();
        g.save(&scope("kept"), &p).unwrap();
        g.save(&scope("fresh"), &p).unwrap();
        // at ten minutes it still runs
        *time.lock() += chrono::Duration::seconds(PLAN_TTL_SECS);
        assert!(g.take_saved(&scope("fresh"), None).is_ok());
        *time.lock() += chrono::Duration::seconds(1);
        let late = g.take_saved(&scope("p"), None).unwrap_err();
        assert_eq!(late.kind, s3raw::ErrorKind::Refused);
        assert!(late.message.contains("older than 10 minutes"), "{late}");
        // and is gone: not there to run when the clock is set back
        *time.lock() -= chrono::Duration::seconds(300);
        assert!(g.take_saved(&scope("p"), None).is_err());
        // one from a time yet to come is no fresher
        *time.lock() -= chrono::Duration::seconds(3600);
        assert!(g.take_saved(&scope("kept"), None).is_err());
        // the app's plans, in its process, age by the same clock
        let p = g.plan(Action::DeleteObjects, "e", "bucket", items(2), None, None);
        *time.lock() += chrono::Duration::seconds(PLAN_TTL_SECS + 1);
        let late = g.take(&p.id, "").unwrap_err();
        assert!(late.message.contains("older than 10 minutes"), "{late}");
        // a plan never confirmed does not stay: the next dry run removes it
        let old = g.plan(Action::AbortUploads, "e", "bucket", items(1), None, None);
        g.save(&scope("old"), &old).unwrap();
        *time.lock() += chrono::Duration::seconds(PLAN_TTL_SECS + 1);
        let new = g.plan(Action::AbortUploads, "e", "bucket", items(1), None, None);
        g.save(&scope("new"), &new).unwrap();
        assert_eq!(
            std::fs::read_dir(dir.path().join("plans")).unwrap().count(),
            1
        );
    }

    /// Two `--yes` at once: the plan is taken by a rename, so one of them
    /// has it and every other is refused before it sends anything.
    #[test]
    fn of_many_confirmations_at_once_one_takes_the_plan() {
        let dir = tempfile::tempdir().unwrap();
        let g = Guard::new(dir.path().join("audit.jsonl"));
        for _ in 0..20 {
            let p = g.plan(Action::AbortUploads, "e", "bucket", items(2), None, None);
            g.save(&scope("p"), &p).unwrap();
            let start = Arc::new(std::sync::Barrier::new(8));
            let took: usize = (0..8)
                .map(|_| {
                    let (g, start) = (g.clone(), start.clone());
                    std::thread::spawn(move || {
                        start.wait();
                        g.take_saved(&scope("p"), None).is_ok() as usize
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|t| t.join().unwrap())
                .sum();
            assert_eq!(took, 1);
        }
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
