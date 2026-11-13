//! Long work as cancellable jobs: big listings, ListParts sums, capacity
//! scans, probe runs. A job runs on the Tokio runtime with its own
//! [`Throttle`] (concurrency and rate), reports progress as a stream of
//! [`JobEvent`]s, and stops for real when cancelled: its future is dropped,
//! which aborts the requests in flight. A job that must clean up after
//! itself (probe runs, downloads) is spawned with
//! [`JobManager::spawn_cooperative`] instead: it watches its token, cleans
//! up, and ends by itself.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use decoders::Phrase;
use parking_lot::Mutex;
use s3raw::{S3Error, Throttle};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Running,
    Finished,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: u64,
    pub total: Option<u64>,
    pub bytes: u64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_text: Vec<Phrase>,
}

impl Progress {
    /// Progress with its message as phrases (and their English).
    pub fn say(done: u64, total: Option<u64>, bytes: u64, text: impl Into<Vec<Phrase>>) -> Self {
        let message_text = text.into();
        Progress {
            done,
            total,
            bytes,
            message: crate::phrase::en(&message_text),
            message_text,
        }
    }
}

/// A job's title: phrases, or words given as they are.
pub struct Title(pub Vec<Phrase>);

impl From<Phrase> for Title {
    fn from(p: Phrase) -> Self {
        Title(vec![p])
    }
}
impl From<Vec<Phrase>> for Title {
    fn from(p: Vec<Phrase>) -> Self {
        Title(p)
    }
}
impl From<&str> for Title {
    fn from(text: &str) -> Self {
        Title(vec![crate::phrase::raw(text)])
    }
}
impl From<&String> for Title {
    fn from(text: &String) -> Self {
        Title(vec![crate::phrase::raw(text.as_str())])
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum JobEvent {
    Progress {
        job: String,
        progress: Progress,
    },
    /// A partial result (rows found so far), for UIs that show them live.
    Partial {
        job: String,
        data: Value,
    },
    Finished {
        job: String,
        result: Value,
    },
    Failed {
        job: String,
        error: S3Error,
    },
    Cancelled {
        job: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub title_text: Vec<Phrase>,
    pub state: JobState,
    pub progress: Progress,
    pub started: DateTime<Utc>,
    pub ended: Option<DateTime<Utc>>,
    pub concurrency: usize,
    pub rate_limit: Option<f64>,
    pub error: Option<S3Error>,
}

struct Entry {
    info: JobInfo,
    cancel: CancellationToken,
    events: broadcast::Sender<JobEvent>,
    result: Option<Value>,
}

/// What a running job can use.
#[derive(Clone)]
pub struct JobCtx {
    pub id: String,
    pub throttle: Arc<Throttle>,
    pub cancel: CancellationToken,
    jobs: JobManager,
}

impl JobCtx {
    pub fn progress(&self, p: Progress) {
        self.jobs.update(&self.id, p);
    }

    pub fn partial(&self, data: Value) {
        if let Some(e) = self.jobs.inner.lock().get(&self.id) {
            let _ = e.events.send(JobEvent::Partial {
                job: self.id.clone(),
                data,
            });
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// `Err(cancelled)` once the job was cancelled: `?` it in loops.
    pub fn check(&self) -> Result<(), S3Error> {
        if self.cancel.is_cancelled() {
            Err(S3Error::cancelled())
        } else {
            Ok(())
        }
    }
}

/// Jobs over that are kept (with their results) for views to read, of each
/// kind: a view reads its job's result by id long after it ended (a
/// layout's time slider), and a hundred downloads since must not take it.
pub const KEEP_FINISHED: usize = 100;

/// All jobs of the process.
#[derive(Clone)]
pub struct JobManager {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
    /// Bumped as any job starts, moves on or ends: a view of the jobs
    /// follows it instead of asking again and again.
    changes: Arc<tokio::sync::watch::Sender<u64>>,
}

impl Default for JobManager {
    fn default() -> Self {
        Self {
            inner: Arc::default(),
            changes: Arc::new(tokio::sync::watch::channel(0).0),
        }
    }
}

impl JobManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// A receiver that wakes as the jobs change (one started, moved on, ended).
    pub fn changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changes.subscribe()
    }

    fn changed(&self) {
        self.changes.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// Starts `work` as a job. Returns its id at once. Cancelling drops
    /// the future where it stands.
    pub fn spawn<F, Fut>(
        &self,
        kind: &str,
        title: impl Into<Title>,
        concurrency: usize,
        rate: Option<f64>,
        work: F,
    ) -> String
    where
        F: FnOnce(JobCtx) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, S3Error>> + Send + 'static,
    {
        self.start(kind, title, concurrency, rate, false, work)
    }

    /// As [`Self::spawn`], for work that has something to undo: cancelling
    /// does not drop the future. The work watches `ctx.cancel` (or
    /// [`JobCtx::check`]), cleans up, and returns `Err(cancelled)`; the job
    /// is running until it does.
    pub fn spawn_cooperative<F, Fut>(
        &self,
        kind: &str,
        title: impl Into<Title>,
        concurrency: usize,
        rate: Option<f64>,
        work: F,
    ) -> String
    where
        F: FnOnce(JobCtx) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, S3Error>> + Send + 'static,
    {
        self.start(kind, title, concurrency, rate, true, work)
    }

    fn start<F, Fut>(
        &self,
        kind: &str,
        title: impl Into<Title>,
        concurrency: usize,
        rate: Option<f64>,
        cooperative: bool,
        work: F,
    ) -> String
    where
        F: FnOnce(JobCtx) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, S3Error>> + Send + 'static,
    {
        let title_text = title.into().0;
        let id = format!(
            "{kind}-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        );
        let (tx, _) = broadcast::channel(256);
        let cancel = CancellationToken::new();
        let throttle = Arc::new(Throttle::new(concurrency, rate));
        let info = JobInfo {
            id: id.clone(),
            kind: kind.into(),
            title: crate::phrase::en(&title_text),
            title_text,
            state: JobState::Running,
            progress: Progress::default(),
            started: Utc::now(),
            ended: None,
            concurrency: throttle.concurrency(),
            rate_limit: throttle.rate(),
            error: None,
        };
        self.inner.lock().insert(
            id.clone(),
            Entry {
                info,
                cancel: cancel.clone(),
                events: tx,
                result: None,
            },
        );
        self.changed();
        let ctx = JobCtx {
            id: id.clone(),
            throttle,
            cancel: cancel.clone(),
            jobs: self.clone(),
        };
        let jobs = self.clone();
        let job_id = id.clone();
        tokio::spawn(async move {
            // a panic ends the job as failed: it must not stay running forever
            let guarded = std::panic::AssertUnwindSafe(async move { work(ctx).await });
            let work = futures::FutureExt::catch_unwind(guarded);
            let caught = if cooperative {
                work.await
            } else {
                tokio::select! {
                    r = work => r,
                    _ = cancel.cancelled() => Ok(Err(S3Error::cancelled())),
                }
            };
            let outcome = caught.unwrap_or_else(|p| {
                let what = p
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown".into());
                Err(S3Error::config(format!("the job panicked: {what}")))
            });
            jobs.finish(&job_id, outcome);
            // results are big (reports, layouts, probe runs): keep the newest of each kind
            jobs.prune(KEEP_FINISHED);
        });
        id
    }

    fn update(&self, id: &str, p: Progress) {
        let mut g = self.inner.lock();
        if let Some(e) = g.get_mut(id) {
            e.info.progress = p.clone();
            let _ = e.events.send(JobEvent::Progress {
                job: id.into(),
                progress: p,
            });
        }
        drop(g);
        self.changed();
    }

    fn finish(&self, id: &str, outcome: Result<Value, S3Error>) {
        let mut g = self.inner.lock();
        let Some(e) = g.get_mut(id) else { return };
        e.info.ended = Some(Utc::now());
        let event = match outcome {
            Ok(v) => {
                e.info.state = JobState::Finished;
                e.result = Some(v.clone());
                JobEvent::Finished {
                    job: id.into(),
                    result: v,
                }
            }
            Err(err) if err.kind == s3raw::ErrorKind::Cancelled => {
                e.info.state = JobState::Cancelled;
                JobEvent::Cancelled { job: id.into() }
            }
            Err(err) => {
                e.info.state = JobState::Failed;
                e.info.error = Some(err.clone());
                JobEvent::Failed {
                    job: id.into(),
                    error: err,
                }
            }
        };
        let _ = e.events.send(event);
        drop(g);
        self.changed();
    }

    /// Cancels a job; `false` if it is unknown or already over.
    pub fn cancel(&self, id: &str) -> bool {
        let g = self.inner.lock();
        match g.get(id) {
            Some(e) if e.info.state == JobState::Running => {
                e.cancel.cancel();
                true
            }
            _ => false,
        }
    }

    pub fn info(&self, id: &str) -> Option<JobInfo> {
        self.inner.lock().get(id).map(|e| e.info.clone())
    }

    pub fn list(&self) -> Vec<JobInfo> {
        let mut v: Vec<JobInfo> = self.inner.lock().values().map(|e| e.info.clone()).collect();
        v.sort_by_key(|j| std::cmp::Reverse(j.started));
        v
    }

    pub fn result(&self, id: &str) -> Option<Value> {
        self.inner.lock().get(id).and_then(|e| e.result.clone())
    }

    /// Events from now on; check [`Self::info`] first for a job that may
    /// already be over.
    pub fn subscribe(&self, id: &str) -> Option<broadcast::Receiver<JobEvent>> {
        self.inner.lock().get(id).map(|e| e.events.subscribe())
    }

    /// Waits for the job's end; its result or error.
    pub async fn wait(&self, id: &str) -> Result<Value, S3Error> {
        let mut rx = self
            .subscribe(id)
            .ok_or_else(|| S3Error::config(format!("no job {id}")))?;
        loop {
            if let Some(info) = self.info(id) {
                match info.state {
                    JobState::Finished => return Ok(self.result(id).unwrap_or(Value::Null)),
                    JobState::Failed => {
                        return Err(info.error.unwrap_or_else(|| S3Error::config("failed")));
                    }
                    JobState::Cancelled => return Err(S3Error::cancelled()),
                    JobState::Running => {}
                }
            }
            match rx.recv().await {
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(S3Error::config("job vanished"));
                }
            }
        }
    }

    /// Drops finished jobs but, of each kind, the `keep` that ended last:
    /// many jobs of one kind (downloads) never push out another kind's
    /// result (a layout a screen still reads by its job's id).
    pub fn prune(&self, keep: usize) {
        let mut g = self.inner.lock();
        let mut done: HashMap<String, Vec<(String, DateTime<Utc>)>> = HashMap::new();
        for e in g.values().filter(|e| e.info.state != JobState::Running) {
            done.entry(e.info.kind.clone())
                .or_default()
                .push((e.info.id.clone(), e.info.ended.unwrap_or(e.info.started)));
        }
        for mut of_kind in done.into_values() {
            of_kind.sort_by_key(|d| std::cmp::Reverse(d.1));
            for (id, _) in of_kind.into_iter().skip(keep) {
                g.remove(&id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn a_title_and_a_message_are_phrases_beside_their_english() {
        use crate::phrase::{en, ph};
        let jobs = JobManager::new();
        let id = jobs.spawn(
            "du",
            ph!("job.du", bucket = "b", prefix = "p/"),
            1,
            None,
            |ctx| async move {
                ctx.progress(Progress::say(3, None, 0, ph!("progress.objects", n = 3)));
                Ok(Value::Null)
            },
        );
        jobs.wait(&id).await.unwrap();
        let info = jobs.info(&id).unwrap();
        assert_eq!(info.title, "Size of b/p/");
        assert_eq!(en(&info.title_text), info.title);
        assert_eq!(info.progress.message, "3 objects");
        assert_eq!(en(&info.progress.message_text), info.progress.message);
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(v["titleText"][0]["key"], "job.du");
        assert_eq!(v["progress"]["messageText"][0]["args"]["n"], "3");
        // words given as they are stay words, and old JSON still reads
        let plain = jobs.spawn("t", "plain", 1, None, |_| async { Ok(Value::Null) });
        assert_eq!(jobs.info(&plain).unwrap().title, "plain");
        let old: Progress =
            serde_json::from_str(r#"{"done":1,"total":null,"bytes":0,"message":"x"}"#).unwrap();
        assert!(old.message_text.is_empty());
    }

    #[tokio::test]
    async fn runs_and_reports() {
        let jobs = JobManager::new();
        let id = jobs.spawn("t", "test", 2, None, |ctx| async move {
            for i in 0..3 {
                ctx.progress(Progress {
                    done: i,
                    ..Default::default()
                });
            }
            Ok(serde_json::json!({"ok": true}))
        });
        assert_eq!(jobs.wait(&id).await.unwrap()["ok"], true);
        assert_eq!(jobs.info(&id).unwrap().state, JobState::Finished);
    }

    #[tokio::test]
    async fn a_view_of_the_jobs_hears_them_change() {
        let jobs = JobManager::new();
        let mut changes = jobs.changes();
        changes.borrow_and_update();
        let id = jobs.spawn("t", "test", 1, None, |_| async { Ok(serde_json::json!(1)) });
        // the start, then the end: each wakes the view
        tokio::time::timeout(Duration::from_secs(5), changes.changed())
            .await
            .unwrap()
            .unwrap();
        jobs.wait(&id).await.unwrap();
        changes.borrow_and_update();
        assert!(!changes.has_changed().unwrap());
        jobs.spawn("t", "again", 1, None, |_| async {
            Ok(serde_json::json!(2))
        });
        assert!(changes.has_changed().unwrap());
    }

    #[tokio::test]
    async fn cancel_stops_the_work() {
        let jobs = JobManager::new();
        let reached = Arc::new(AtomicBool::new(false));
        let r = reached.clone();
        let id = jobs.spawn("t", "slow", 1, None, move |_ctx| async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            r.store(true, Ordering::SeqCst);
            Ok(Value::Null)
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(jobs.cancel(&id));
        assert!(jobs.wait(&id).await.unwrap_err().kind == s3raw::ErrorKind::Cancelled);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!reached.load(Ordering::SeqCst), "the future was dropped");
        assert!(!jobs.cancel(&id), "already over");
    }

    #[tokio::test]
    async fn a_cooperative_job_cleans_up_before_it_ends_cancelled() {
        let jobs = JobManager::new();
        let cleaned = Arc::new(AtomicBool::new(false));
        let c = cleaned.clone();
        let id = jobs.spawn_cooperative("t", "tidy", 1, None, move |ctx| async move {
            ctx.cancel.cancelled().await;
            tokio::time::sleep(Duration::from_millis(30)).await;
            c.store(true, Ordering::SeqCst);
            ctx.check()?;
            Ok(Value::Null)
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(jobs.cancel(&id));
        assert!(jobs.wait(&id).await.unwrap_err().kind == s3raw::ErrorKind::Cancelled);
        assert!(cleaned.load(Ordering::SeqCst), "the cleanup ran first");
    }

    #[tokio::test]
    async fn a_job_that_panics_ends_failed() {
        let jobs = JobManager::new();
        let id = jobs.spawn("t", "boom", 1, None, |_| async {
            if true {
                panic!("byte index 12 is not a char boundary");
            }
            Ok(Value::Null)
        });
        let e = tokio::time::timeout(Duration::from_secs(5), jobs.wait(&id))
            .await
            .expect("the job ended")
            .unwrap_err();
        assert!(e.message.contains("panicked"), "{e}");
        assert_eq!(jobs.info(&id).unwrap().state, JobState::Failed);
    }

    #[tokio::test]
    async fn finished_jobs_beyond_the_newest_are_dropped() {
        let jobs = JobManager::new();
        let mut ids = Vec::new();
        for _ in 0..KEEP_FINISHED + 5 {
            let id = jobs.spawn("t", "quick", 1, None, |_| async { Ok(Value::Null) });
            jobs.wait(&id).await.unwrap();
            ids.push(id);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(jobs.list().len(), KEEP_FINISHED);
        assert!(jobs.info(&ids[0]).is_none());
        assert!(jobs.info(ids.last().unwrap()).is_some());
    }

    #[tokio::test]
    async fn many_jobs_of_one_kind_do_not_drop_a_result_of_another() {
        let jobs = JobManager::new();
        // a layout a screen keeps reading by its job's id
        let layout = jobs.spawn("layout", "decode", 1, None, |_| async {
            Ok(serde_json::json!({"snapshots": 3}))
        });
        jobs.wait(&layout).await.unwrap();
        let mut downloads = Vec::new();
        for _ in 0..KEEP_FINISHED + 5 {
            let id = jobs.spawn("download", "get", 1, None, |_| async { Ok(Value::Null) });
            jobs.wait(&id).await.unwrap();
            downloads.push(id);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(jobs.result(&layout).unwrap()["snapshots"], 3);
        // the downloads themselves are still bounded
        assert!(jobs.info(&downloads[0]).is_none());
        assert_eq!(jobs.list().len(), KEEP_FINISHED + 1);
    }

    #[tokio::test]
    async fn failures_and_events() {
        let jobs = JobManager::new();
        let id = jobs.spawn("t", "fail", 1, Some(5.0), |ctx| async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            ctx.progress(Progress {
                done: 1,
                total: Some(2),
                ..Default::default()
            });
            Err(S3Error::config("boom"))
        });
        let mut rx = jobs.subscribe(&id).unwrap();
        let mut saw_progress = false;
        loop {
            match rx.recv().await.unwrap() {
                JobEvent::Progress { .. } => saw_progress = true,
                JobEvent::Failed { error, .. } => {
                    assert_eq!(error.message, "boom");
                    break;
                }
                e => panic!("{e:?}"),
            }
        }
        assert!(saw_progress);
        assert_eq!(jobs.info(&id).unwrap().rate_limit, Some(5.0));
        jobs.prune(0);
        assert!(jobs.info(&id).is_none());
    }
}
