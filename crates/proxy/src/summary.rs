//! What a session amounts to: counts, bytes, time, and per operation.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use reqlog::Record;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpStats {
    pub operation: String,
    pub count: u64,
    pub errors: u64,
    pub misses: u64,
    /// The share of its requests that failed, 0 to 1.
    pub error_share: f64,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    /// Fewer than [`FEW`] requests: its percentiles are of too few to tell
    /// apart, and are shown as such.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub few: bool,
}

/// Requests below which an operation's percentiles say little.
pub const FEW: u64 = 20;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub requests: u64,
    /// Failed requests: 4xx and 5xx except misses, and transport errors.
    pub errors: u64,
    /// GET and HEAD answered 404: lookups that found nothing. Stores probe
    /// for keys this way, so they are not failures.
    pub misses: u64,
    /// Refused by the proxy's policy (also counted in `errors`).
    pub refused: u64,
    /// From the first request to the end of the last one, in seconds.
    pub seconds: f64,
    pub bytes_up: u64,
    pub bytes_down: u64,
    /// Busiest first.
    pub operations: Vec<OpStats>,
    /// Error responses by status and S3 code, most frequent first.
    pub error_codes: Vec<(String, u64)>,
}

fn s3_code(body: &str) -> Option<&str> {
    let start = body.find("<Code>")? + 6;
    let end = body[start..].find("</Code>")? + start;
    Some(&body[start..end])
}

/// Whether a record is a lookup that found nothing (GET or HEAD → 404).
pub fn is_miss(r: &Record) -> bool {
    r.status == Some(404)
        && (r.method == "GET" || r.method == "HEAD")
        // a bucket that is not there is a mistake, not a lookup: a key is
        // looked for, a bucket is named
        && !r
            .error_body
            .as_deref()
            .is_some_and(|b| b.contains("<Code>NoSuchBucket</Code>"))
}

/// Whether a record failed (a miss is not a failure).
pub fn is_failure(r: &Record) -> bool {
    r.is_error() && !is_miss(r)
}

/// Summarizes records in any order.
pub fn summarize(records: &[Record]) -> Summary {
    let mut s = Summary::default();
    let mut ops: BTreeMap<&str, (OpStats, Vec<f64>)> = BTreeMap::new();
    let mut codes: BTreeMap<String, u64> = BTreeMap::new();
    let mut first: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut last: Option<chrono::DateTime<chrono::Utc>> = None;
    for r in records {
        s.requests += 1;
        let up = r.request_bytes.unwrap_or(0);
        let down = if r.is_error() {
            0
        } else {
            r.response_bytes.unwrap_or(0)
        };
        s.bytes_up += up;
        s.bytes_down += down;
        let refused = r
            .response_headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("x-sonde-proxy"));
        if is_miss(r) {
            s.misses += 1;
        } else if r.is_error() {
            s.errors += 1;
            if refused {
                s.refused += 1;
            }
            let what = match (r.status, r.error_body.as_deref().and_then(s3_code)) {
                _ if refused => "refused by sonde".to_string(),
                (Some(st), Some(code)) => format!("{st} {code}"),
                (Some(st), None) => st.to_string(),
                (None, _) => "no response".to_string(),
            };
            *codes.entry(what).or_default() += 1;
        }
        let end = r.time + chrono::Duration::microseconds(r.latency_us());
        first = Some(first.map_or(r.time, |f| f.min(r.time)));
        last = Some(last.map_or(end, |l| l.max(end)));
        let e = ops.entry(r.operation.as_str()).or_insert_with(|| {
            (
                OpStats {
                    operation: r.operation.clone(),
                    ..Default::default()
                },
                Vec::new(),
            )
        });
        e.0.count += 1;
        e.0.errors += u64::from(is_failure(r));
        e.0.misses += u64::from(is_miss(r));
        e.0.bytes_up += up;
        e.0.bytes_down += down;
        e.0.max_ms = e.0.max_ms.max(r.latency_ms);
        e.1.push(r.latency_ms);
    }
    if let (Some(f), Some(l)) = (first, last) {
        s.seconds = (l - f).num_microseconds().unwrap_or(0) as f64 / 1e6;
    }
    s.operations = ops
        .into_values()
        .map(|(mut o, mut lat)| {
            lat.sort_by(|a, b| a.total_cmp(b));
            o.p50_ms = crate::profile::percentile(&lat, 0.5);
            o.p95_ms = crate::profile::percentile(&lat, 0.95);
            o.p99_ms = crate::profile::percentile(&lat, 0.99);
            o.error_share = o.errors as f64 / o.count.max(1) as f64;
            o.few = o.count < FEW;
            o
        })
        .collect();
    s.operations
        .sort_by(|a, b| b.count.cmp(&a.count).then(a.operation.cmp(&b.operation)));
    s.error_codes = codes.into_iter().collect();
    s.error_codes
        .sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    s
}

/// What a session file holds, counted once and kept beside it
/// (`<name>.summary.json`): listing sessions, and holding a run to an earlier
/// one, read this and never the session whole. It is of one version of the
/// file (its length and the time it was written): another is counted again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counted {
    pub version: u32,
    pub len: u64,
    pub modified_ms: u64,
    pub created: DateTime<Utc>,
    pub endpoint: String,
    #[serde(default)]
    pub command: Option<String>,
    /// The program's name, when it has one, and the key its runs go by.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub program: Option<String>,
    /// Recorded from a program's own requests (not a replay, an import, a trial).
    pub recording: bool,
    #[serde(default)]
    pub git: Option<crate::session::GitStamp>,
    pub requests: u64,
    pub errors: u64,
    pub misses: u64,
    pub seconds: f64,
    pub bytes_up: u64,
    pub bytes_down: u64,
    /// Its requests over its time, in [`SHAPE_BINS`] slices.
    pub shape: Vec<u32>,
    /// The figures a baseline and a program's history need, once the
    /// session has been analysed ([`figures`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub figures: Option<Figures>,
}

/// A run's figures, with the prices its cost was reckoned at (other prices:
/// reckoned again) and the warnings it had, as [`crate::profile::finding_identity`] names them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Figures {
    pub prices: crate::profile::RequestPrices,
    pub point: crate::program::RunPoint,
    pub warnings: Vec<String>,
}

pub const COUNTED_VERSION: u32 = 1;
pub const SHAPE_BINS: usize = 24;

/// Where a session's counts are kept: `<name>.summary.json` beside it.
pub fn counted_file(session: &std::path::Path) -> std::path::PathBuf {
    let name = session
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = name.strip_suffix(".sonde").unwrap_or(&name);
    session.with_file_name(format!("{stem}.summary.json"))
}

/// Removes a session's counts: when the session is removed.
pub fn forget(session: &std::path::Path) {
    let _ = std::fs::remove_file(counted_file(session));
}

/// What of a record's line the counts read: the rest of the line is skipped,
/// not parsed.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CountedLine<'a> {
    #[serde(default)]
    time: Option<DateTime<Utc>>,
    #[serde(borrow, default)]
    method: std::borrow::Cow<'a, str>,
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    latency_ms: f64,
    #[serde(default)]
    request_bytes: Option<u64>,
    #[serde(default)]
    response_bytes: Option<u64>,
    #[serde(default)]
    error_body: Option<String>,
    #[serde(default)]
    transport_error: Option<serde::de::IgnoredAny>,
}

/// A session's counts, taken a line at a time: the same figures as
/// [`summarize`] of its records.
#[derive(Default)]
pub struct Counter {
    requests: u64,
    errors: u64,
    misses: u64,
    bytes_up: u64,
    bytes_down: u64,
    first: Option<i64>,
    last: Option<i64>,
    starts: Vec<i64>,
}

impl Counter {
    /// A line of a session file after its header: a record, the program's
    /// output, or a line cut short (not counted).
    pub fn line(&mut self, line: &[u8]) {
        if !line.starts_with(b"{") || line.starts_with(b"{\"log\"") {
            return;
        }
        let Ok(r) = serde_json::from_slice::<CountedLine>(line) else {
            return;
        };
        let error = r.status.is_none_or(|s| s >= 400) || r.transport_error.is_some();
        let miss = r.status == Some(404)
            && (r.method == "GET" || r.method == "HEAD")
            && !r
                .error_body
                .as_deref()
                .is_some_and(|b| b.contains("<Code>NoSuchBucket</Code>"));
        let latency_us = (r.latency_ms.clamp(0.0, reqlog::MAX_LATENCY_MS) * 1000.0) as i64;
        self.add(
            r.time.map(|t| t.timestamp_micros()),
            latency_us,
            error,
            miss,
            r.request_bytes.unwrap_or(0),
            r.response_bytes.unwrap_or(0),
        );
    }

    pub fn record(&mut self, r: &Record) {
        self.add(
            Some(r.time.timestamp_micros()),
            r.latency_us(),
            r.is_error(),
            is_miss(r),
            r.request_bytes.unwrap_or(0),
            r.response_bytes.unwrap_or(0),
        );
    }

    fn add(
        &mut self,
        start: Option<i64>,
        latency_us: i64,
        error: bool,
        miss: bool,
        up: u64,
        down: u64,
    ) {
        self.requests += 1;
        self.misses += u64::from(miss);
        self.errors += u64::from(error && !miss);
        self.bytes_up += up;
        self.bytes_down += if error { 0 } else { down };
        // (a line another tool wrote may have no time: counted, not placed)
        let Some(start) = start else { return };
        let end = start + latency_us;
        self.first = Some(self.first.map_or(start, |f| f.min(start)));
        self.last = Some(self.last.map_or(end, |l| l.max(end)));
        self.starts.push(start);
    }

    /// The counts, of the file as it is now.
    pub fn finish(
        self,
        session: &std::path::Path,
        header: &crate::session::SessionHeader,
    ) -> Counted {
        let (len, modified_ms) = stamp(session).unwrap_or((0, 0));
        let mut shape = vec![0u32; SHAPE_BINS];
        if let (Some(a), Some(b)) = (self.starts.iter().min(), self.starts.iter().max()) {
            let span = (b - a).max(1) as f64;
            for t in &self.starts {
                let i = (((t - a) as f64 / span) * SHAPE_BINS as f64) as usize;
                shape[i.min(SHAPE_BINS - 1)] += 1;
            }
        }
        Counted {
            version: COUNTED_VERSION,
            len,
            modified_ms,
            created: header.created,
            endpoint: header.endpoint.clone(),
            command: header.command.clone(),
            name: header.name.clone(),
            program: crate::program::key_of(header),
            recording: header.is_recording(),
            git: header.git.clone(),
            requests: self.requests,
            errors: self.errors,
            misses: self.misses,
            seconds: match (self.first, self.last) {
                (Some(f), Some(l)) => (l - f) as f64 / 1e6,
                _ => 0.0,
            },
            bytes_up: self.bytes_up,
            bytes_down: self.bytes_down,
            shape,
            figures: None,
        }
    }
}

/// A file's length and when it was last written, in ms.
fn stamp(path: &std::path::Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let at = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64);
    Some((m.len(), at))
}

/// The counts kept beside a session, when they are of the file as it is now.
pub fn kept(session: &std::path::Path) -> Option<Counted> {
    let c: Counted = serde_json::from_slice(&std::fs::read(counted_file(session)).ok()?).ok()?;
    (c.version == COUNTED_VERSION && Some((c.len, c.modified_ms)) == stamp(session)).then_some(c)
}

/// Keeps a session's counts beside it. A failure to write is no error: they
/// are counted again next time.
pub fn keep(session: &std::path::Path, c: &Counted) {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let file = counted_file(session);
    // a name of its own: the app and the CLI may write at the same time
    let tmp = file.with_extension(format!(
        "json.{}.{}.tmp",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let written = serde_json::to_vec(c)
        .ok()
        .and_then(|b| std::fs::write(&tmp, b).ok())
        .and_then(|_| std::fs::rename(&tmp, &file).ok());
    if written.is_none() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// A session's counts: those kept beside it, or counted now in one pass over
/// its lines (no record is built) and, for a finished session, kept. `None`
/// when the file is not a session.
pub fn counted(session: &std::path::Path) -> Option<Counted> {
    if let Some(c) = kept(session) {
        return Some(c);
    }
    let mut lines = crate::session::Lines::open(session).ok()?;
    let header: crate::session::SessionHeader = serde_json::from_slice(lines.line().ok()??).ok()?;
    let mut n = Counter::default();
    // what reads is counted: a file cut short still lists
    while let Ok(Some(l)) = lines.line() {
        n.line(l);
    }
    let c = n.finish(session, &header);
    // a live recording is still growing: its counts would be stale at once
    if crate::session::is_compressed(session) {
        keep(session, &c);
    }
    Some(c)
}

/// A run's figures at `prices`: those kept beside it, or the session read
/// and analysed now and its figures kept. With them, its counts.
pub fn figures(
    session: &std::path::Path,
    prices: &crate::profile::RequestPrices,
) -> Option<(Counted, Figures)> {
    let path = session.to_string_lossy().into_owned();
    if let Some(c) = kept(session)
        && let Some(f) = c.figures.clone().filter(|f| f.prices == *prices)
    {
        let mut f = f;
        // as it is named now: the folder may have moved
        f.point.path = path;
        return Some((c, f));
    }
    let s = crate::session::Session::open_lean(session).ok()?;
    let f = figures_of(&path, &s, prices);
    let mut n = Counter::default();
    for r in &s.records {
        n.record(r);
    }
    let mut c = n.finish(session, &s.header);
    c.figures = Some(f.clone());
    if crate::session::is_compressed(session) {
        keep(session, &c);
    }
    Some((c, f))
}

/// The figures of a session already read.
pub fn figures_of(
    path: &str,
    s: &crate::session::Session,
    prices: &crate::profile::RequestPrices,
) -> Figures {
    let profile = crate::profile::profile(&s.records, prices);
    let findings = crate::episode::observations(&s.records, &crate::episode::episodes(&s.records));
    Figures {
        prices: prices.clone(),
        point: crate::program::run_point(
            path,
            &s.header.created.to_rfc3339(),
            &s.records,
            &profile,
            &findings,
        ),
        warnings: findings
            .iter()
            .filter(|o| o.level == "warn")
            .map(crate::profile::finding_identity)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqlog::Header;

    fn rec(op: &str, status: u16, ms: f64, at_ms: i64) -> Record {
        Record {
            operation: op.into(),
            status: Some(status),
            latency_ms: ms,
            time: chrono::DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
            request_bytes: Some(10),
            response_bytes: Some(100),
            ..Default::default()
        }
    }

    #[test]
    fn a_missing_bucket_is_an_error_and_a_missing_key_a_lookup() {
        let mut key = rec("GetObject", 404, 1.0, 0);
        key.method = "GET".into();
        key.error_body = Some("<Error><Code>NoSuchKey</Code></Error>".into());
        let mut bucket = key.clone();
        bucket.error_body = Some("<Error><Code>NoSuchBucket</Code></Error>".into());
        assert!(is_miss(&key) && !is_failure(&key));
        assert!(!is_miss(&bucket) && is_failure(&bucket));
    }

    #[test]
    fn counts() {
        let mut slow = rec("GetObject", 503, 80.0, 100);
        slow.error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
        let mut refused = rec("DeleteObject", 403, 0.0, 150);
        refused.response_headers = vec![Header::new("x-sonde-proxy", "refused")];
        let s = summarize(&[
            rec("GetObject", 200, 10.0, 0),
            Record {
                method: "HEAD".into(),
                ..rec("HeadObject", 404, 1.0, 10)
            },
            rec("GetObject", 200, 30.0, 50),
            slow,
            rec("PutObject", 200, 20.0, 120),
            refused,
        ]);
        assert_eq!(s.requests, 6);
        assert_eq!(s.errors, 2);
        assert_eq!(s.misses, 1);
        assert_eq!(s.refused, 1);
        assert_eq!(s.bytes_up, 60);
        assert_eq!(s.bytes_down, 300);
        assert!(!s.error_codes.iter().any(|(c, _)| c.starts_with("404")));
        assert!((s.seconds - 0.18).abs() < 1e-9);
        assert_eq!(s.operations[0].operation, "GetObject");
        assert_eq!(s.operations[0].count, 3);
        assert_eq!(s.operations[0].p50_ms, 30.0);
        assert_eq!(s.operations[0].max_ms, 80.0);
        assert!(s.error_codes.contains(&("503 SlowDown".into(), 1)));
        assert!(s.error_codes.contains(&("refused by sonde".into(), 1)));
        assert_eq!(summarize(&[]), Summary::default());
    }
}
