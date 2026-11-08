//! What a session amounts to: counts, bytes, time, and per operation.

use std::collections::BTreeMap;

use reqlog::Record;
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpStats {
    pub operation: String,
    pub count: u64,
    pub errors: u64,
    pub misses: u64,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub p50_ms: f64,
    pub max_ms: f64,
}

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
    r.status == Some(404) && (r.method == "GET" || r.method == "HEAD")
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
            o.p50_ms = lat[lat.len() / 2];
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
