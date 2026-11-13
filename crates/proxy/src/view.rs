//! A session as a window shows it: analysed once, then looked at in pages.
//! The view carries the totals, the signals, the profile, the findings and
//! the first page of operations; the rest are asked for in ranges, sorted
//! and narrowed here. What is sent does not grow with the session.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::episode::{
    Episode, Kind, Observation, Outcome, episodes, observations_of, shared_notes,
};
use crate::fixes::WhatIf;
use crate::profile::{Profile, RequestPrices};
use crate::session::Session;
use crate::signals::Signals;
use crate::summary::Summary;

/// Operations in a page when none is asked for, and at most.
pub const PAGE: usize = 500;
pub const PAGE_MAX: usize = 5000;
/// Operations a finding lists in the view (the rest: [`Analysis::finding_episodes`]).
pub const FINDING_EPISODES: usize = 200;
/// Request ids an operation's row carries (all of them: [`Analysis::episode`]).
pub const ROW_RECORDS: usize = 64;
/// Operations [`Analysis::episodes_at`] names at most.
const AT_MOST: usize = 200;

/// What the table sorts by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sort {
    #[default]
    Time,
    Duration,
    Requests,
}

/// A range of a session's operations, as the table asks for it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    pub offset: usize,
    pub limit: Option<usize>,
    pub sort: Sort,
    pub descending: bool,
    /// Only the operations of this finding (its place in the findings).
    pub finding: Option<usize>,
    /// Only the operations running in this window, in ms from the first
    /// operation's start.
    pub slice: Option<(f64, f64)>,
    /// Only the operations whose title or notes hold this, whatever its case.
    pub text: Option<String>,
    /// Only the operations that did not end well.
    pub failed: bool,
    /// Only the operations that sent a request of this kind (`GetObject`).
    pub kind: Option<String>,
}

impl Query {
    /// What decides which operations and in what order: all but the range.
    fn order_key(&self) -> Query {
        Query {
            offset: 0,
            limit: None,
            text: self
                .text
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_lowercase),
            ..self.clone()
        }
    }
}

/// A session, analysed.
pub struct Analysis {
    pub episodes: Vec<Episode>,
    pub observations: Vec<Observation>,
    pub shared_notes: Vec<String>,
    pub summary: Summary,
    pub profile: Profile,
    pub signals: Signals,
    pub what_if: Option<WhatIf>,
    /// The SDK that sent most of its requests (`boto3 1.44`).
    pub sdk: Option<String>,
    /// The requests a fault scenario answered, by id.
    pub injected: Vec<u64>,
    /// Which operation each request belongs to, by request id (sorted: a
    /// map's buckets would cost twice as much a request).
    owner: Vec<(u64, u32)>,
    /// The kinds of request each operation sent, as bits of `kind_names`
    /// (the 64th holds every kind past the 63rd): what a query by kind reads.
    kinds: Vec<u64>,
    kind_names: Vec<String>,
    /// The first operation's start and the last end.
    span: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// The order last asked for: the next page of it does not sort again.
    order: Mutex<Option<(Query, Arc<Vec<u32>>)>>,
}

fn millis(a: DateTime<Utc>, b: DateTime<Utc>) -> f64 {
    (b - a).num_microseconds().unwrap_or(0) as f64 / 1000.0
}

/// What a thread made, or its panic carried on.
fn joined<T>(h: std::thread::ScopedJoinHandle<'_, T>) -> T {
    match h.join() {
        Ok(v) => v,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

impl Analysis {
    /// `path` (a run, its `header` and `records`, analysed as this) against
    /// its program's run before it ([`crate::fixloop::since_last`]). Its
    /// operations are not made again, and its findings at any threshold
    /// only when there is a run before to judge: 18.0 made both for every
    /// `show`, a third more time and memory at a million requests.
    pub fn since_last(
        &self,
        path: &std::path::Path,
        header: &crate::session::SessionHeader,
        records: &[reqlog::Record],
        prices: &dyn Fn(&str) -> RequestPrices,
    ) -> Option<crate::fixloop::SinceLast> {
        let key = crate::program::key_of(header)?;
        if !header.is_recording() {
            return None;
        }
        let dir = crate::fixloop::folder_of(path);
        let traces = match crate::fixloop::previous_run(&dir, &key, header.created, path) {
            Some(_) => crate::episode::traces_of(header, records, &self.episodes),
            None => Vec::new(),
        };
        let run = crate::profile::Run::with(
            records,
            self.profile.clone(),
            self.observations.clone(),
            traces,
        );
        crate::fixloop::since_last(&dir, path, header, records, run, prices)
    }

    pub fn of(s: &Session, prices: &RequestPrices) -> Analysis {
        Analysis::of_records(&s.header, &s.records, prices)
    }

    /// Everything a session amounts to, worked out once: its operations and
    /// findings on this thread while its counts, its signals and where its
    /// time went are reckoned on others (each reads every record, none
    /// needs another's answer).
    pub fn of_records(
        header: &crate::session::SessionHeader,
        records: &[reqlog::Record],
        prices: &RequestPrices,
    ) -> Analysis {
        let (summary, signals, profile, episodes, observations) = std::thread::scope(|scope| {
            let summary = scope.spawn(|| crate::summary::summarize(records));
            let signals = scope.spawn(|| crate::signals::signals(records, prices));
            let profile = scope.spawn(|| crate::profile::profile(records, prices));
            let episodes = episodes(records);
            let observations = observations_of(header, records, &episodes);
            (
                joined(summary),
                joined(signals),
                joined(profile),
                episodes,
                observations,
            )
        });
        let shared_notes = shared_notes(&episodes)
            .into_iter()
            .map(|(_, n, _, _)| n)
            .collect();
        let what_if = crate::fixes::what_if(records, &profile, &observations);
        let mut owner = Vec::with_capacity(records.len());
        for (i, e) in episodes.iter().enumerate() {
            for id in &e.records {
                owner.push((*id, i as u32));
            }
        }
        // the kinds each operation sent, a bit a kind
        let mut kind_names: Vec<String> = Vec::new();
        let mut kinds = vec![0u64; episodes.len()];
        {
            let mut by_id: Vec<(u64, u32)> = owner.clone();
            by_id.sort_unstable_by_key(|(id, _)| *id);
            for r in records {
                let mut k = by_id.partition_point(|(id, _)| *id < r.id);
                while k < by_id.len() && by_id[k].0 == r.id {
                    let name: &str = &r.operation;
                    let bit = match kind_names.iter().position(|n| n == name) {
                        Some(b) => b,
                        None if kind_names.len() < 63 => {
                            kind_names.push(name.to_string());
                            kind_names.len() - 1
                        }
                        None => 63,
                    };
                    kinds[by_id[k].1 as usize] |= 1 << bit;
                    k += 1;
                }
            }
        }
        // (a request two operations claim is the later's)
        owner.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        owner.dedup_by_key(|(id, _)| *id);
        let span = episodes.iter().map(|e| e.start).min().and_then(|start| {
            let end = episodes.iter().map(|e| e.end).max()?;
            Some((start, end))
        });
        Analysis {
            sdk: crate::callers::sdk_of(records),
            summary,
            signals,
            injected: records
                .iter()
                .filter(|r| r.injected.is_some())
                .map(|r| r.id)
                .collect(),
            episodes,
            observations,
            shared_notes,
            profile,
            what_if,
            owner,
            kinds,
            kind_names,
            span,
            order: Mutex::new(None),
        }
    }

    /// An operation as a row: what the table and the inspector read. Its
    /// title is its phrases (the English is sent only where there are
    /// none), its requests' ids are cut to `records` at most.
    fn row(&self, index: usize, records: usize) -> Option<Value> {
        let e = self.episodes.get(index)?;
        let mut v = serde_json::to_value(e).ok()?;
        let o = v.as_object_mut()?;
        if !e.title_text.is_empty() {
            o.remove("title");
        }
        if e.records.len() > records {
            o.insert("records".into(), json!(&e.records[..records]));
        }
        o.insert("index".into(), json!(index));
        o.insert("requests".into(), json!(e.records.len()));
        if e.kind == Kind::Probe || e.title().contains("found nothing") {
            o.insert("miss".into(), json!(true));
        }
        Some(v)
    }

    /// One operation, with every request's id.
    pub fn episode(&self, index: usize) -> Option<Value> {
        self.row(index, usize::MAX)
    }

    /// The operation a request belongs to.
    pub fn episode_of(&self, record: u64) -> Option<usize> {
        let at = self
            .owner
            .binary_search_by_key(&record, |(id, _)| *id)
            .ok()?;
        Some(self.owner[at].1 as usize)
    }

    /// The operations a query shows, in its order.
    fn order(&self, q: &Query) -> Arc<Vec<u32>> {
        let key = q.order_key();
        if let Some((k, order)) = &*self.order.lock()
            && *k == key
        {
            return order.clone();
        }
        let eps = &self.episodes;
        let t0 = self.span.map(|s| s.0);
        // a kind no request had narrows to nothing; one past the 63rd to every one past it
        let kind_bit = key.kind.as_deref().map(|k| match self.kind_names.iter().position(|n| n == k) {
            Some(b) => 1u64 << b,
            None if self.kind_names.len() >= 63 => 1u64 << 63,
            None => 0,
        });
        let keep = |i: &u32| {
            let e = &eps[*i as usize];
            if key.failed && e.outcome == Outcome::Ok {
                return false;
            }
            if let Some(bit) = kind_bit
                && self.kinds[*i as usize] & bit == 0
            {
                return false;
            }
            if let (Some((a, b)), Some(t0)) = (key.slice, t0)
                && !(millis(t0, e.start) < b && millis(t0, e.end) >= a)
            {
                return false;
            }
            if let Some(text) = &key.text {
                let holds = |t: &str| t.to_lowercase().contains(text.as_str());
                if !holds(&e.title()) && !e.notes.iter().any(|n| holds(n)) {
                    return false;
                }
            }
            true
        };
        let mut order: Vec<u32> = match key.finding {
            Some(f) => self
                .observations
                .get(f)
                .map(|o| o.episodes.iter().map(|i| *i as u32).collect())
                .unwrap_or_default(),
            None => (0..eps.len() as u32).collect(),
        };
        order.retain(keep);
        // (already in time order: sorted only for another key, or backwards)
        if key.sort != Sort::Time || key.descending {
            let by = |i: u32| -> i64 {
                let e = &eps[i as usize];
                match key.sort {
                    Sort::Time => e.start.timestamp_micros(),
                    Sort::Duration => (e.end - e.start).num_microseconds().unwrap_or(i64::MAX),
                    Sort::Requests => e.records.len() as i64,
                }
            };
            // equal ones in the session's order, whichever way the rest go
            order.sort_by(|a, b| {
                let (x, y) = (by(*a), by(*b));
                let c = if key.descending { y.cmp(&x) } else { x.cmp(&y) };
                c.then(a.cmp(b))
            });
        }
        let order = Arc::new(order);
        *self.order.lock() = Some((key, order.clone()));
        order
    }

    /// A page of operations: `{ total, offset, rows }`.
    pub fn page(&self, q: &Query) -> Value {
        let order = self.order(q);
        let limit = q.limit.unwrap_or(PAGE).min(PAGE_MAX);
        let rows: Vec<Value> = order
            .iter()
            .skip(q.offset)
            .take(limit)
            .filter_map(|i| self.row(*i as usize, ROW_RECORDS))
            .collect();
        json!({ "total": order.len(), "offset": q.offset, "rows": rows })
    }

    /// The row a query shows an operation at; `None` when it leaves it out.
    pub fn row_of(&self, q: &Query, index: usize) -> Option<usize> {
        self.order(q).iter().position(|i| *i as usize == index)
    }

    /// The operations running at `at` (the first [`AT_MOST`]), or else the
    /// nearest one: output is read a moment after it is written, so a line
    /// can come in just after the request it announced.
    pub fn episodes_at(&self, at: DateTime<Utc>) -> Vec<usize> {
        let running: Vec<usize> = self
            .episodes
            .iter()
            .enumerate()
            .filter(|(_, e)| e.start <= at && at <= e.end)
            .map(|(i, _)| i)
            .take(AT_MOST)
            .collect();
        if !running.is_empty() {
            return running;
        }
        self.episodes
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| {
                if at < e.start {
                    e.start - at
                } else {
                    at - e.end
                }
            })
            .map(|(i, _)| vec![i])
            .unwrap_or_default()
    }

    /// A range of the operations a finding is about.
    pub fn finding_episodes(&self, finding: usize, offset: usize, limit: usize) -> Vec<usize> {
        self.observations
            .get(finding)
            .map(|o| {
                o.episodes
                    .iter()
                    .skip(offset)
                    .take(limit)
                    .copied()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// What a window opens with: everything but the operations past the
    /// first page, and the findings' operations past the first
    /// [`FINDING_EPISODES`].
    pub fn view(&self, path: &str, header: &crate::session::SessionHeader) -> Map<String, Value> {
        let mut formats: std::collections::BTreeMap<&str, usize> = Default::default();
        let mut kinds: std::collections::BTreeMap<String, usize> = Default::default();
        for e in &self.episodes {
            if let Some(f) = e.format {
                *formats.entry(f.name()).or_default() += 1;
            }
            let kind = serde_json::to_value(e.kind)
                .ok()
                .and_then(|k| k.as_str().map(String::from))
                .unwrap_or_default();
            *kinds.entry(kind).or_default() += 1;
        }
        let findings: Vec<Value> = self
            .observations
            .iter()
            .map(|o| {
                let mut v = to(o);
                if let Some(m) = v.as_object_mut() {
                    if o.episodes.len() > FINDING_EPISODES {
                        m.insert("operations".into(), json!(&o.episodes[..FINDING_EPISODES]));
                    }
                    m.insert("operationsTotal".into(), json!(o.episodes.len()));
                    if let Some(e) = o.episodes.first().and_then(|i| self.episodes.get(*i)) {
                        m.insert("firstStart".into(), json!(e.start));
                    }
                }
                v
            })
            .collect();
        let first: Vec<Value> = (0..self.episodes.len().min(PAGE))
            .filter_map(|i| self.row(i, ROW_RECORDS))
            .collect();
        let mut v = Map::new();
        v.insert("path".into(), json!(path));
        v.insert("header".into(), to(header));
        v.insert("summary".into(), to(&self.summary));
        v.insert("profile".into(), to(&self.profile));
        v.insert("signals".into(), to(&self.signals));
        v.insert("whatIf".into(), to(&self.what_if));
        v.insert("injected".into(), json!(self.injected));
        v.insert("sdk".into(), json!(self.sdk));
        v.insert("operations".into(), Value::Array(first));
        v.insert("operationsTotal".into(), json!(self.episodes.len()));
        v.insert(
            "span".into(),
            match self.span {
                Some((start, end)) => json!({ "start": start, "end": end }),
                None => Value::Null,
            },
        );
        v.insert(
            "families".into(),
            json!({ "formats": formats, "kinds": kinds }),
        );
        v.insert("observations".into(), Value::Array(findings));
        v.insert("sharedNotes".into(), json!(self.shared_notes));
        v
    }
}

/// Anything that serializes, as a JSON value (null when it cannot be).
fn to<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqlog::Record;

    fn session(n: u64) -> Session {
        let records = (0..n)
            .map(|i| Record {
                id: i + 1,
                time: DateTime::from_timestamp_millis(1_700_000_000_000 + i as i64 * 100).unwrap(),
                method: if i % 7 == 0 { "PUT" } else { "GET" }.into(),
                operation: if i % 7 == 0 { "PutObject" } else { "GetObject" }.into(),
                bucket: Some("b".into()),
                key: Some(format!("data/k{i:05}").into()),
                status: Some(if i % 50 == 3 { 500 } else { 200 }),
                // the later, the quicker: sorting by time taken is not the order they came in
                latency_ms: 1.0 + (n - i) as f64 / 10.0,
                ..Default::default()
            })
            .collect();
        Session {
            header: crate::session::SessionHeader::new("e", "https://e", "r", None),
            records,
            logs: Vec::new(),
        }
    }

    #[test]
    fn the_view_carries_the_first_page_and_the_count_of_the_rest() {
        let s = session(1800);
        let a = Analysis::of(&s, &RequestPrices::default());
        assert_eq!(a.episodes.len(), 1800);
        let v = a.view("p.sonde", &s.header);
        assert_eq!(v["operations"].as_array().unwrap().len(), PAGE);
        assert_eq!(v["operationsTotal"], 1800);
        let row = &v["operations"][3];
        assert_eq!(row["index"], 3);
        assert_eq!(row["requests"], 1);
        // said once: by its phrases
        assert!(row.get("title").is_none() && row["titleText"][0]["key"] == "request");
        assert_eq!(v["span"]["start"], json!(a.episodes[0].start));
        assert_eq!(v["families"]["kinds"]["request"], 1800);
        // the view of 1800 operations is no larger than that of 500
        let small = Analysis::of(&session(500), &RequestPrices::default());
        let (big, little) = (
            serde_json::to_string(&v["operations"]).unwrap().len(),
            serde_json::to_string(&small.view("p", &s.header)["operations"])
                .unwrap()
                .len(),
        );
        assert_eq!(big, little);
    }

    #[test]
    fn pages_follow_the_tables_sorting_and_narrowing() {
        let s = session(1800);
        let a = Analysis::of(&s, &RequestPrices::default());
        let q = |json: Value| -> Query { serde_json::from_value(json).unwrap() };
        // the next page of the session's own order
        let p = a.page(&q(json!({ "offset": 500, "limit": 3 })));
        assert_eq!(p["total"], 1800);
        let idx = |p: &Value| -> Vec<u64> {
            p["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["index"].as_u64().unwrap())
                .collect()
        };
        assert_eq!(idx(&p), [500, 501, 502]);
        // the slowest first: the earliest took longest
        let slow = a.page(&q(
            json!({ "sort": "duration", "descending": true, "limit": 2 }),
        ));
        assert_eq!(idx(&slow), [0, 1]);
        // and where the table shows an operation under that order
        let by_duration = q(json!({ "sort": "duration" }));
        assert_eq!(a.row_of(&by_duration, 1799), Some(0));
        assert_eq!(a.row_of(&by_duration, 0), Some(1799));
        // those that failed, by a word of their title
        let failed = a.page(&q(json!({ "failed": true })));
        assert_eq!(failed["total"], 36);
        assert_eq!(idx(&failed)[..2], [3, 53]);
        // those that sent a kind of request
        let puts = a.page(&q(json!({ "kind": "PutObject" })));
        assert_eq!(puts["total"], 258);
        assert_eq!(idx(&puts)[..2], [0, 7]);
        assert_eq!(a.page(&q(json!({ "kind": "DeleteObject" })))["total"], 0);
        let named = a.page(&q(json!({ "text": "K00042" })));
        assert_eq!(idx(&named), [42]);
        assert_eq!(a.row_of(&q(json!({ "text": "k00042" })), 7), None);
        // a slice of time: the operations running in it (100 ms apart, the
        // first taking 181 ms)
        let slice = a.page(&q(json!({ "slice": [1000.0, 1200.0] })));
        assert!(
            idx(&slice).contains(&10) && idx(&slice).contains(&11) && !idx(&slice).contains(&12)
        );
        // a request's operation, and the operations at a moment
        assert_eq!(a.episode_of(43), Some(42));
        assert_eq!(
            a.episodes_at(s.records[1799].time + chrono::Duration::hours(1)),
            [1799]
        );
        assert_eq!(a.episode(42).unwrap()["records"], json!([43]));
    }

    #[test]
    fn a_finding_lists_some_of_its_operations_and_counts_them_all() {
        // one key read 600 times: a finding about 600 operations
        let mut s = session(600);
        for r in &mut s.records {
            r.key = Some("conf.json".into());
            r.method = "GET".into();
            r.operation = "GetObject".into();
            r.status = Some(200);
        }
        let a = Analysis::of(&s, &RequestPrices::default());
        let v = a.view("p", &s.header);
        let hot = v["observations"]
            .as_array()
            .unwrap()
            .iter()
            .position(|o| o["phrases"][0]["key"] == "obs.hot")
            .expect("a hot key");
        let o = &v["observations"][hot];
        assert_eq!(o["operations"].as_array().unwrap().len(), FINDING_EPISODES);
        assert_eq!(o["operationsTotal"], 600);
        assert_eq!(o["firstStart"], json!(a.episodes[0].start));
        assert_eq!(a.finding_episodes(hot, 598, 10), [598, 599]);
        let q: Query = serde_json::from_value(json!({ "finding": hot, "offset": 590 })).unwrap();
        assert_eq!(a.page(&q)["rows"].as_array().unwrap().len(), 10);
    }
}
