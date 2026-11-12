use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::Record;

/// The ring buffer's default capacity.
pub const DEFAULT_CAPACITY: usize = 5000;

/// Status filter: an exact code, a class, or "any error".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum StatusFilter {
    Exact(u16),
    /// `2` for 2xx, `4` for 4xx, `5` for 5xx.
    Class(u16),
    /// Any status ≥ 400 or a transport error.
    Errors,
    /// A transport error only (no response).
    Transport,
}

/// What a listing of the log keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Filter {
    pub status: Option<StatusFilter>,
    /// Operation name, matched case-insensitively.
    pub operation: Option<String>,
    pub endpoint: Option<String>,
    pub job: Option<String>,
    /// Matched against the bucket, the key and the request id.
    pub text: Option<String>,
    /// Records retried at least once.
    pub retried: bool,
}

impl Filter {
    pub fn matches(&self, r: &Record) -> bool {
        if let Some(s) = &self.status {
            let ok = match s {
                StatusFilter::Exact(c) => r.status == Some(*c),
                StatusFilter::Class(c) => r.status.is_some_and(|s| s / 100 == *c),
                StatusFilter::Errors => r.is_error(),
                StatusFilter::Transport => r.status.is_none(),
            };
            if !ok {
                return false;
            }
        }
        if let Some(op) = &self.operation
            && !r.operation.eq_ignore_ascii_case(op)
        {
            return false;
        }
        if let Some(e) = &self.endpoint
            && &r.endpoint != e
        {
            return false;
        }
        if let Some(j) = &self.job
            && r.job.as_ref() != Some(j)
        {
            return false;
        }
        if self.retried && r.retries == 0 {
            return false;
        }
        if let Some(t) = &self.text {
            let t = t.to_lowercase();
            let hit = [
                r.bucket.as_deref(),
                r.key.as_deref(),
                r.request_id.as_deref(),
            ]
            .iter()
            .any(|f| f.is_some_and(|v| v.to_lowercase().contains(&t)));
            if !hit {
                return false;
            }
        }
        true
    }
}

/// Where records go besides the ring (the SQLite file).
pub trait Sink: Send + Sync {
    fn write(&self, record: &Record);
}

struct Inner {
    records: VecDeque<Record>,
    capacity: usize,
}

/// The shared request log: a bounded ring of redacted records.
#[derive(Clone)]
pub struct RequestLog {
    inner: Arc<RwLock<Inner>>,
    next: Arc<AtomicU64>,
    sink: Arc<RwLock<Option<Arc<dyn Sink>>>>,
    tx: broadcast::Sender<u64>,
}

impl Default for RequestLog {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl std::fmt::Debug for RequestLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestLog")
            .field("len", &self.len())
            .finish()
    }
}

impl RequestLog {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(1024);
        RequestLog {
            inner: Arc::new(RwLock::new(Inner {
                records: VecDeque::with_capacity(capacity.min(DEFAULT_CAPACITY)),
                capacity: capacity.max(1),
            })),
            next: Arc::new(AtomicU64::new(1)),
            sink: Arc::new(RwLock::new(None)),
            tx,
        }
    }

    /// Adds persistence; records already in the ring are not replayed.
    pub fn set_sink(&self, sink: Option<Arc<dyn Sink>>) {
        *self.sink.write() = sink;
    }

    pub fn set_capacity(&self, capacity: usize) {
        let mut inner = self.inner.write();
        inner.capacity = capacity.max(1);
        while inner.records.len() > inner.capacity {
            inner.records.pop_front();
        }
    }

    pub fn capacity(&self) -> usize {
        self.inner.read().capacity
    }

    /// Redacts `record`, gives it an id and stores it. Returns the id.
    pub fn push(&self, record: Record) -> u64 {
        let mut record = record.redacted();
        // the id is taken under the lock, so the ring stays in id order for
        // `get`'s binary search when requests finish concurrently
        let id = {
            let mut inner = self.inner.write();
            let id = self.next.fetch_add(1, Ordering::Relaxed);
            record.id = id;
            if inner.records.len() >= inner.capacity {
                inner.records.pop_front();
            }
            inner.records.push_back(record.clone());
            id
        };
        if let Some(sink) = self.sink.read().as_ref() {
            sink.write(&record);
        }
        let _ = self.tx.send(id);
        id
    }

    /// Ids of records as they are pushed.
    pub fn subscribe(&self) -> broadcast::Receiver<u64> {
        self.tx.subscribe()
    }

    pub fn len(&self) -> usize {
        self.inner.read().records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, id: u64) -> Option<Record> {
        let inner = self.inner.read();
        // ids are increasing: binary search
        let idx = inner.records.binary_search_by_key(&id, |r| r.id).ok()?;
        inner.records.get(idx).cloned()
    }

    /// Finds the newest record carrying `request_id` (for "jump to request").
    pub fn find_request_id(&self, request_id: &str) -> Option<Record> {
        self.inner
            .read()
            .records
            .iter()
            .rev()
            .find(|r| r.request_id.as_deref() == Some(request_id))
            .cloned()
    }

    /// Matching records, newest first, at most `limit`.
    pub fn list(&self, filter: &Filter, limit: usize) -> Vec<Record> {
        self.inner
            .read()
            .records
            .iter()
            .rev()
            .filter(|r| filter.matches(r))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Every record, oldest first.
    pub fn snapshot(&self) -> Vec<Record> {
        self.inner.read().records.iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.inner.write().records.clear();
    }

    /// Distinct operation names in the log, for filter menus.
    pub fn operations(&self) -> Vec<String> {
        let mut ops: Vec<String> = self
            .inner
            .read()
            .records
            .iter()
            .map(|r| r.operation.to_string())
            .collect();
        ops.sort();
        ops.dedup();
        ops
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Header;

    fn rec(op: &str, status: Option<u16>) -> Record {
        Record {
            operation: op.into(),
            method: "GET".into(),
            endpoint: "e1".into(),
            bucket: Some("b".into()),
            key: Some("k".into()),
            status,
            request_headers: vec![Header::new(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/x, SignedHeaders=host, Signature=abc",
            )],
            ..Default::default()
        }
    }

    #[test]
    fn ring_bounds_and_ids() {
        let log = RequestLog::new(3);
        for i in 0..5 {
            log.push(rec(&format!("Op{i}"), Some(200)));
        }
        assert_eq!(log.len(), 3);
        let all = log.snapshot();
        assert_eq!(all.iter().map(|r| r.id).collect::<Vec<_>>(), vec![3, 4, 5]);
        assert!(log.get(1).is_none());
        assert_eq!(log.get(4).unwrap().operation, "Op3");
        log.set_capacity(1);
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn redacts_on_push() {
        let log = RequestLog::new(10);
        let id = log.push(rec("GetObject", Some(200)));
        let r = log.get(id).unwrap();
        assert!(!r.request_headers[0].value.contains("Signature=abc"));
        assert!(!r.request_headers[0].value.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn filters() {
        let log = RequestLog::new(10);
        log.push(rec("GetObject", Some(200)));
        log.push(rec("GetObject", Some(404)));
        log.push(rec("PutObject", Some(503)));
        log.push(rec("PutObject", None));
        let f = |s| Filter {
            status: Some(s),
            ..Default::default()
        };
        assert_eq!(log.list(&f(StatusFilter::Errors), 10).len(), 3);
        assert_eq!(log.list(&f(StatusFilter::Class(5)), 10).len(), 1);
        assert_eq!(log.list(&f(StatusFilter::Exact(404)), 10).len(), 1);
        assert_eq!(log.list(&f(StatusFilter::Transport), 10).len(), 1);
        let op = Filter {
            operation: Some("putobject".into()),
            ..Default::default()
        };
        assert_eq!(log.list(&op, 10).len(), 2);
        let ep = Filter {
            endpoint: Some("other".into()),
            ..Default::default()
        };
        assert!(log.list(&ep, 10).is_empty());
        // newest first
        assert_eq!(log.list(&Filter::default(), 1)[0].id, 4);
        assert_eq!(log.operations(), vec!["GetObject", "PutObject"]);
    }

    #[test]
    fn find_by_request_id() {
        let log = RequestLog::new(10);
        let mut r = rec("HeadObject", Some(403));
        r.request_id = Some("REQ123".into());
        let id = log.push(r);
        assert_eq!(log.find_request_id("REQ123").unwrap().id, id);
        assert!(log.find_request_id("nope").is_none());
    }
}

#[cfg(test)]
mod concurrency {
    use super::*;

    #[test]
    fn ids_stay_in_order_when_pushed_from_many_threads() {
        let log = RequestLog::new(100_000);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    for _ in 0..2_000 {
                        log.push(Record::default());
                    }
                });
            }
        });
        let ids: Vec<u64> = log.inner.read().records.iter().map(|r| r.id).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]));
        assert!(log.get(ids[ids.len() / 2]).is_some());
    }
}
