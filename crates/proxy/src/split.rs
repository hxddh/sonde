//! A session's requests split by one dimension (`sonde show --by`, the
//! app's Group by): the operations table's figures for each prefix, size,
//! caller, SDK, status or operation, and one sentence saying where the slow
//! and the costly requests concentrate.

use std::collections::{BTreeMap, HashMap};

use reqlog::Record;
use serde::Serialize;

use crate::phrase::{Phrase, p};
use crate::profile::{RequestPrices, folder_of, percentile, prefix_for, prefix_groups, price};
use crate::summary::is_miss;

/// What the requests are split by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dimension {
    Operation,
    Prefix,
    Size,
    Caller,
    Sdk,
    Status,
}

impl Dimension {
    pub const ALL: [Dimension; 6] = [
        Dimension::Operation,
        Dimension::Prefix,
        Dimension::Size,
        Dimension::Caller,
        Dimension::Sdk,
        Dimension::Status,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Dimension::Operation => "operation",
            Dimension::Prefix => "prefix",
            Dimension::Size => "size",
            Dimension::Caller => "caller",
            Dimension::Sdk => "sdk",
            Dimension::Status => "status",
        }
    }

    pub fn parse(text: &str) -> Option<Dimension> {
        Dimension::ALL
            .into_iter()
            .find(|d| d.name().eq_ignore_ascii_case(text.trim()))
    }
}

/// One group of requests: the operations table's figures for it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// The prefix, size band, caller, SDK, status or operation.
    pub key: String,
    /// The groups past the table's rows, folded into one.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub other: bool,
    pub requests: usize,
    /// Failed requests (a lookup that found nothing is no failure).
    pub errors: usize,
    pub error_share: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    /// The program's waiting that was theirs ([`crate::profile::Group::waited_ms`]).
    pub waited_ms: f64,
    /// `waited_ms` over the session's waiting (the rows add up to 1).
    pub share: f64,
    pub cost: f64,
    pub cost_share: f64,
    /// Of them, slow requests (the session's slowest tenth).
    pub slow: usize,
}

/// A session split by one dimension.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Split {
    pub by: Dimension,
    /// The SDK that sent most requests (`boto3 1.44`), from their user agent.
    pub sdk: Option<String>,
    /// A request this slow or slower is slow: the session's p90 (0 when
    /// there are too few to say).
    pub slow_ms: f64,
    pub slow: usize,
    pub cost: f64,
    pub currency: String,
    /// Most waiting first; the rest folded into one `other` row last.
    pub rows: Vec<Row>,
    /// Where the slow and the costly requests concentrate, in a sentence
    /// (empty when nothing can be said).
    pub text: String,
    pub phrases: Vec<Phrase>,
}

/// Rows shown at most; past them the rest is one `other` row.
pub const ROWS: usize = 12;

/// Fewer slow requests than this, and where they are is not said.
const FEW_SLOW: usize = 3;

/// The size band of a request: the larger of what it sent and received.
const BANDS: [(u64, &str); 5] = [
    (0, "0 B"),
    (64 << 10, "< 64 KiB"),
    (1 << 20, "64 KiB-1 MiB"),
    (8 << 20, "1-8 MiB"),
    (u64::MAX, "8 MiB+"),
];

fn band(r: &Record) -> usize {
    let n = r
        .request_bytes
        .unwrap_or(0)
        .max(r.response_bytes.unwrap_or(0));
    if n == 0 {
        return 0;
    }
    BANDS
        .iter()
        .skip(1)
        .position(|(hi, _)| n < *hi)
        .unwrap_or(3)
        + 1
}

/// Keys of one dimension for each request.
struct Keyer {
    by: Dimension,
    prefixes: Vec<(String, usize)>,
}

impl Keyer {
    fn new(by: Dimension, records: &[Record]) -> Keyer {
        let prefixes = if by == Dimension::Prefix {
            let folders: Vec<String> = records.iter().map(folder_of).collect();
            prefix_groups(folders.iter().map(String::as_str), ROWS)
        } else {
            Vec::new()
        };
        Keyer { by, prefixes }
    }

    fn key(&self, r: &Record) -> String {
        match self.by {
            Dimension::Operation => r.operation.to_string(),
            Dimension::Prefix => {
                let f = folder_of(r);
                prefix_for(&self.prefixes, &f).unwrap_or(&f).to_string()
            }
            Dimension::Size => BANDS[band(r)].1.to_string(),
            Dimension::Caller => crate::callers::caller(r).unwrap_or_else(|| "(not named)".into()),
            Dimension::Sdk => crate::callers::sdk(r).unwrap_or_else(|| "(unknown)".into()),
            Dimension::Status => r
                .status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "no answer".into()),
        }
    }
}

/// The SDK most of `records` were sent by.
pub fn sdk_of(records: &[Record]) -> Option<String> {
    let mut n: HashMap<String, usize> = HashMap::new();
    // one in a hundred is plenty to say which SDK it is
    let step = (records.len() / 2000).max(1);
    for r in records.iter().step_by(step) {
        if let Some(s) = crate::callers::sdk(r) {
            *n.entry(s).or_default() += 1;
        }
    }
    n.into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(s, _)| s)
}

/// The requests at or over the session's p90: the slowest tenth (none with
/// fewer than [`FEW_SLOW`] of them, or no times).
fn slow_ms(records: &[Record]) -> f64 {
    let mut lat: Vec<f64> = records.iter().map(|r| r.latency_ms.max(0.0)).collect();
    lat.sort_by(f64::total_cmp);
    let at = percentile(&lat, 0.9);
    let n = lat.iter().filter(|l| **l >= at).count();
    if at <= 0.0 || n < FEW_SLOW { 0.0 } else { at }
}

/// `records` split by `by`.
pub fn split(records: &[Record], by: Dimension, prices: &RequestPrices) -> Split {
    let keyer = Keyer::new(by, records);
    let keys: Vec<String> = records.iter().map(|r| keyer.key(r)).collect();
    let slow_at = slow_ms(records);
    let is_slow = |r: &Record| slow_at > 0.0 && r.latency_ms >= slow_at;

    // the groups, the largest (by time taken, then requests) first; past
    // ROWS - 1 of them the rest is one
    let mut by_key: HashMap<&str, Vec<&Record>> = HashMap::new();
    for (r, k) in records.iter().zip(&keys) {
        by_key.entry(k.as_str()).or_default().push(r);
    }
    let mut ranked: Vec<(&str, Vec<&Record>)> = by_key.into_iter().collect();
    let took = |rs: &[&Record]| rs.iter().map(|r| r.latency_ms.max(0.0)).sum::<f64>();
    ranked.sort_by(|a, b| {
        took(&b.1)
            .total_cmp(&took(&a.1))
            .then(b.1.len().cmp(&a.1.len()))
            .then(a.0.cmp(b.0))
    });
    // (whether it is the rest folded, and its name)
    let fold = ranked.len() > ROWS;
    let mut groups: BTreeMap<(bool, String), Vec<&Record>> = BTreeMap::new();
    for (i, (k, rs)) in ranked.into_iter().enumerate() {
        let folded = fold && i >= ROWS - 1;
        let name = if folded {
            "other".to_string()
        } else {
            k.to_string()
        };
        groups.entry((folded, name)).or_default().extend(rs);
    }
    let waited = crate::profile::waited_us(&groups);
    let busy_us: f64 = waited.values().sum();
    let total_cost: f64 = records.iter().map(|r| price(r, prices)).sum();
    let mut rows: Vec<Row> = groups
        .iter()
        .map(|((other, key), rs)| {
            let mut lat: Vec<f64> = rs.iter().map(|r| r.latency_ms.max(0.0)).collect();
            lat.sort_by(f64::total_cmp);
            let errors = rs.iter().filter(|r| r.is_error() && !is_miss(r)).count();
            let w = waited.get(&(*other, key.clone())).copied().unwrap_or(0.0);
            let cost: f64 = rs.iter().map(|r| price(r, prices)).sum();
            Row {
                key: key.clone(),
                other: *other,
                requests: rs.len(),
                errors,
                error_share: errors as f64 / rs.len().max(1) as f64,
                p50_ms: percentile(&lat, 0.5),
                p95_ms: percentile(&lat, 0.95),
                p99_ms: percentile(&lat, 0.99),
                waited_ms: w / 1000.0,
                share: if busy_us > 0.0 { w / busy_us } else { 0.0 },
                cost,
                cost_share: if total_cost > 0.0 {
                    cost / total_cost
                } else {
                    0.0
                },
                slow: rs.iter().filter(|r| is_slow(r)).count(),
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        a.other
            .cmp(&b.other)
            .then(b.waited_ms.total_cmp(&a.waited_ms))
            .then(b.requests.cmp(&a.requests))
            .then(a.key.cmp(&b.key))
    });
    let slow = records.iter().filter(|r| is_slow(r)).count();
    let phrases = sentence(records, &keys, &rows, by, slow_at, total_cost);
    Split {
        by,
        sdk: sdk_of(records),
        slow_ms: slow_at,
        slow,
        cost: total_cost,
        currency: prices.currency.clone(),
        rows,
        text: crate::phrase::en(&phrases),
        phrases,
    }
}

/// A group named as a sentence says it: `under b/wal/`, `over 8 MiB`,
/// `from etl.py:19 load`.
fn place(by: Dimension, key: &str) -> Phrase {
    let v = |k: &'static str| p(k, &[("v", key.to_string())]);
    match by {
        Dimension::Operation => v("split.operation"),
        Dimension::Prefix => v("split.prefix"),
        Dimension::Caller => v("split.caller"),
        Dimension::Sdk => v("split.sdk"),
        Dimension::Status if key == "no answer" => p("split.status.none", &[]),
        Dimension::Status => v("split.status"),
        Dimension::Size => match BANDS.iter().position(|(_, n)| *n == key) {
            Some(0) => p("split.size.none", &[]),
            Some(1) => p("split.size.under", &[("hi", "64 KiB".into())]),
            Some(2) => p(
                "split.size.between",
                &[("lo", "64 KiB".into()), ("hi", "1 MiB".into())],
            ),
            Some(3) => p(
                "split.size.between",
                &[("lo", "1 MiB".into()), ("hi", "8 MiB".into())],
            ),
            _ => p("split.size.over", &[("lo", "8 MiB".into())]),
        },
    }
}

fn pct(n: f64) -> String {
    format!("{:.0}", (n * 100.0).floor())
}

/// Where the slow requests concentrate (in the group of `by` that holds
/// most of them, and, within it, a second dimension's group when one holds
/// most of those) and where the cost does; nothing when they do not.
fn sentence(
    records: &[Record],
    keys: &[String],
    rows: &[Row],
    by: Dimension,
    slow_at: f64,
    total_cost: f64,
) -> Vec<Phrase> {
    let named: Vec<&Row> = rows.iter().filter(|r| !r.other).collect();
    let mut out = Vec::new();
    let total = records.len().max(1) as f64;
    let slow_total: usize = rows.iter().map(|r| r.slow).sum();
    if slow_at > 0.0 && slow_total >= FEW_SLOW && rows.len() >= 2 {
        let top = named
            .iter()
            .max_by(|a, b| a.slow.cmp(&b.slow).then(b.key.cmp(&a.key)))
            .copied();
        let concentrated = top.filter(|t| {
            let share = t.slow as f64 / slow_total as f64;
            share >= 0.5 && share >= t.requests as f64 / total * 1.5
        });
        match concentrated {
            Some(t) => {
                let in_top = |i: usize| keys[i] == t.key;
                let slow_in_top: Vec<usize> = (0..records.len())
                    .filter(|&i| in_top(i) && records[i].latency_ms >= slow_at)
                    .collect();
                let mut said = vec![place(by, &t.key)];
                let mut n = slow_in_top.len();
                // a second dimension that narrows them further
                let mut best: Option<(usize, Phrase)> = None;
                for d in [Dimension::Prefix, Dimension::Size, Dimension::Operation] {
                    if d == by {
                        continue;
                    }
                    let k2 = Keyer::new(d, records);
                    let mut slow_n: HashMap<String, usize> = HashMap::new();
                    for &i in &slow_in_top {
                        *slow_n.entry(k2.key(&records[i])).or_default() += 1;
                    }
                    let Some((v, m)) = slow_n
                        .into_iter()
                        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
                    else {
                        continue;
                    };
                    // most of them, and not merely all of the group's requests
                    let of_group = (0..records.len())
                        .filter(|&i| in_top(i) && k2.key(&records[i]) == v)
                        .count() as f64
                        / t.requests.max(1) as f64;
                    let narrows = m as f64 / n.max(1) as f64 >= 0.6
                        && m as f64 / slow_total as f64 >= 0.5
                        && of_group < 0.9;
                    if narrows && best.as_ref().is_none_or(|(b, _)| m > *b) {
                        best = Some((m, place(d, &v)));
                    }
                }
                if let Some((m, phrase)) = best {
                    said.push(p("split.and", &[]));
                    said.push(phrase);
                    n = m;
                }
                out.push(p(
                    "split.slow",
                    &[
                        ("pct", pct(n as f64 / slow_total as f64)),
                        ("ms", sonde_fmt::duration(slow_at)),
                    ],
                ));
                out.extend(said);
            }
            None => out.push(p("split.spread", &[])),
        }
    }
    if total_cost > 0.0 && rows.len() >= 2 {
        let top = named
            .iter()
            .max_by(|a, b| a.cost.total_cmp(&b.cost).then(b.key.cmp(&a.key)))
            .copied();
        // more of the cost than of the requests: else it says only that most requests are there
        if let Some(t) =
            top.filter(|t| t.cost_share >= 0.5 && t.cost_share >= t.requests as f64 / total * 1.5)
        {
            out.push(p(
                if out.is_empty() {
                    "split.cost.first"
                } else {
                    "split.cost"
                },
                &[("pct", pct(t.cost_share))],
            ));
            out.push(place(by, &t.key));
        }
    }
    if !out.is_empty() {
        out.push(p("split.end", &[]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use reqlog::Header;

    fn rec(id: u64, op: &str, key: &str, ms: f64, bytes: u64) -> Record {
        Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 1000).unwrap(),
            method: if op.starts_with("Put") { "PUT" } else { "GET" }.into(),
            operation: op.into(),
            bucket: Some("b".into()),
            key: Some(key.into()),
            status: Some(200),
            latency_ms: ms,
            response_bytes: Some(bytes),
            request_headers: vec![Header {
                name: "user-agent".into(),
                value: "Boto3/1.44.2 md/Botocore#1.44.2 Python/3.12".into(),
            }],
            ..Default::default()
        }
    }

    /// 40 small fast reads under data/, 10 large slow writes under wal/.
    fn session() -> Vec<Record> {
        let mut out = Vec::new();
        for i in 0..40 {
            out.push(rec(i, "GetObject", &format!("data/{i}"), 5.0, 1000));
        }
        for i in 40..50 {
            out.push(rec(i, "GetObject", &format!("wal/{i}"), 300.0, 20 << 20));
        }
        out
    }

    #[test]
    fn the_sentence_names_where_the_slow_requests_are_and_narrows_them() {
        let s = split(&session(), Dimension::Prefix, &RequestPrices::default());
        assert_eq!(s.sdk.as_deref(), Some("boto3 1.44"));
        assert_eq!(s.rows[0].key, "b/wal/");
        assert_eq!(s.rows[0].slow, 10);
        assert!((s.rows.iter().map(|r| r.share).sum::<f64>() - 1.0).abs() < 1e-9);
        assert_eq!(
            s.text,
            "100% of the slow requests (over 300 ms) are under b/wal/."
        );
        // by operation: one kind, nothing to tell apart
        let s = split(&session(), Dimension::Operation, &RequestPrices::default());
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.text, "");
        // by size, and narrowed by prefix: all of the slow requests are both
        let mut recs = session();
        // 10 more large reads under data/, fast
        for i in 50..60 {
            recs.push(rec(i, "GetObject", &format!("data/{i}"), 5.0, 20 << 20));
        }
        let s = split(&recs, Dimension::Size, &RequestPrices::default());
        assert_eq!(s.rows[0].key, "8 MiB+");
        assert_eq!(
            s.text,
            "100% of the slow requests (over 300 ms) are over 8 MiB, under b/wal/."
        );
    }

    #[test]
    fn the_cost_is_said_where_it_concentrates_and_spread_slowness_is_said_too() {
        let mut recs: Vec<Record> = (0..20)
            .map(|i| rec(i, "GetObject", &format!("in/{i}"), 10.0 + i as f64, 10))
            .collect();
        for i in 20..30 {
            recs.push(rec(
                i,
                "PutObject",
                &format!("out/{i}"),
                10.0 + i as f64,
                10,
            ));
        }
        let s = split(&recs, Dimension::Operation, &RequestPrices::default());
        // writes cost 12.5 times reads: 10 PUTs are 86% of the cost
        assert!(
            s.text.ends_with("86% of the cost is PutObject."),
            "{}",
            s.text
        );
        let even: Vec<Record> = (0..40)
            .map(|i| {
                rec(
                    i,
                    "GetObject",
                    &format!("{}/{i}", ["a", "b", "c", "d"][i as usize % 4]),
                    1.0 + i as f64,
                    10,
                )
            })
            .collect();
        let s = split(&even, Dimension::Prefix, &RequestPrices::default());
        assert!(
            s.text.starts_with("the slow requests are spread"),
            "{}",
            s.text
        );
    }

    #[test]
    fn past_the_rows_the_rest_is_one_group_and_errors_are_counted() {
        let mut recs: Vec<Record> = (0..30)
            .map(|i| rec(i, "GetObject", &format!("p{i}/k"), 5.0, 10))
            .collect();
        recs[0].status = Some(503);
        recs[1].status = Some(404);
        let s = split(&recs, Dimension::Prefix, &RequestPrices::default());
        assert!(s.rows.len() <= ROWS);
        assert_eq!(s.rows.iter().map(|r| r.requests).sum::<usize>(), 30);
        let s = split(&recs, Dimension::Status, &RequestPrices::default());
        let row = |k: &str| s.rows.iter().find(|r| r.key == k).unwrap();
        assert_eq!((row("503").errors, row("404").errors), (1, 0));
        assert_eq!(Dimension::parse("SDK"), Some(Dimension::Sdk));
        assert_eq!(Dimension::parse("bucket"), None);
    }
}
