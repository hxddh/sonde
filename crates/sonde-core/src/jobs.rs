//! Long work as cancellable jobs: big listings, ListParts sums, capacity
//! scans, probe runs. A job runs on the Tokio runtime with its own
//! [`Throttle`] (concurrency and rate), reports progress as a stream of
//! [`JobEvent`]s, and stops for real when cancelled: its future is dropped,
//! which aborts the requests in flight.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
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

/// All jobs of the process.
#[derive(Clone, Default)]
pub struct JobManager {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
}

impl JobManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts `work` as a job. Returns its id at once.
    pub fn spawn<F, Fut>(
        &self,
        kind: &str,
        title: &str,
        concurrency: usize,
        rate: Option<f64>,
        work: F,
    ) -> String
    where
        F: FnOnce(JobCtx) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, S3Error>> + Send + 'static,
    {
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
            title: title.into(),
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
        let ctx = JobCtx {
            id: id.clone(),
            throttle,
            cancel: cancel.clone(),
            jobs: self.clone(),
        };
        let jobs = self.clone();
        let job_id = id.clone();
        tokio::spawn(async move {
            let outcome = tokio::select! {
                r = work(ctx) => r,
                _ = cancel.cancelled() => Err(S3Error::cancelled()),
            };
            jobs.finish(&job_id, outcome);
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

    /// Drops finished jobs older than the newest `keep`.
    pub fn prune(&self, keep: usize) {
        let mut g = self.inner.lock();
        let mut done: Vec<(String, DateTime<Utc>)> = g
            .values()
            .filter(|e| e.info.state != JobState::Running)
            .map(|e| (e.info.id.clone(), e.info.started))
            .collect();
        done.sort_by_key(|d| std::cmp::Reverse(d.1));
        for (id, _) in done.into_iter().skip(keep) {
            g.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

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
