//! Where a session's time went and what its requests cost: the time a
//! program spent waiting on its store, by kind of request; how many requests
//! were in flight; the rate of requests over the session by outcome; an
//! estimate of the requests' price. And two runs of one program compared.
//!
//! Times are wall-clock: a kind's time is the union of its requests'
//! intervals (ten requests at once for 50 ms are 50 ms of waiting, not 500).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use reqlog::Record;
use serde::Serialize;

use crate::episode::Observation;
use crate::summary::is_miss;

/// Request prices, per thousand (S3's classes: writes and listings, reads).
/// Deletes are free.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPrices {
    pub per_1000_writes: f64,
    pub per_1000_reads: f64,
    pub currency: String,
    /// Lists, when they cost other than a write (B2's class C).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_1000_lists: Option<f64>,
}

impl Default for RequestPrices {
    /// S3 Standard in us-east-1.
    fn default() -> Self {
        RequestPrices {
            per_1000_writes: 0.005,
            per_1000_reads: 0.0004,
            currency: "USD".into(),
            per_1000_lists: None,
        }
    }
}

/// S3's price class of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// PUT, COPY, POST, LIST.
    Write,
    /// GET, HEAD and the rest.
    Read,
    /// DELETE.
    Free,
}

pub fn class(r: &Record) -> Class {
    let op = r.operation.as_str();
    if r.method == "DELETE" || op == "DeleteObjects" {
        Class::Free
    } else if r.method == "PUT" || r.method == "POST" || op.starts_with("List") {
        Class::Write
    } else {
        Class::Read
    }
}

/// One kind of request: an S3 operation, its lookups that found nothing apart.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    #[serde(rename = "kind")]
    pub operation: String,
    /// GET or HEAD answered 404.
    pub missed: bool,
    pub requests: usize,
    /// Wall-clock time with one of them in flight.
    pub waiting_ms: f64,
    /// Their latencies added up.
    pub sum_ms: f64,
    /// The program's waiting that was theirs: each moment's waiting split
    /// evenly among the requests then in flight, so the kinds add up to the
    /// session's waiting.
    pub waited_ms: f64,
    /// `waited_ms` over the session's waiting (the kinds add up to 1).
    pub share: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    pub errors: usize,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub cost: f64,
}

/// Requests started in one slice of the session, by outcome.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bin {
    /// The slice's start, from the session's start.
    pub t_ms: f64,
    pub ok: usize,
    pub miss: usize,
    pub client: usize,
    pub server: usize,
    /// How long the slice's requests took: median, p95 and p99 (0 without any).
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    /// Of them, the store asked to slow down (503 SlowDown, 429).
    pub throttled: usize,
    /// Of them, sent more than once (retried by the SDK).
    pub retried: usize,
    /// Bytes sent and received by them.
    pub bytes_up: u64,
    pub bytes_down: u64,
}

/// How many requests took up to `le_ms` (and more than the bucket before):
/// the latency's distribution, on fixed buckets that grow by about 2.5×.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyBucket {
    pub le_ms: f64,
    pub n: usize,
}

/// The buckets' upper edges, in ms; the last holds everything slower.
pub const LATENCY_EDGES: [f64; 15] = [
    0.5,
    1.0,
    2.0,
    5.0,
    10.0,
    20.0,
    50.0,
    100.0,
    200.0,
    500.0,
    1000.0,
    2000.0,
    5000.0,
    10000.0,
    f64::INFINITY,
];

/// The store asked the program to slow down.
fn throttled(r: &Record) -> bool {
    matches!(r.status, Some(503 | 429))
        || r.error_body
            .as_deref()
            .is_some_and(|b| b.contains("<Code>SlowDown</Code>"))
}

/// Requests in flight from `t_ms` on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub t_ms: f64,
    pub n: usize,
}

/// One request, for a plot of latencies.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub id: u64,
    /// Its group, by index.
    pub group: usize,
    pub ms: f64,
    pub t_ms: f64,
    /// The answer's status (none: no answer).
    pub status: Option<u16>,
    /// Bytes sent and received.
    pub bytes: u64,
}

/// One kind of request's latency in one slice of the session: how many,
/// and their median and p95 (the busiest kinds only).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupBin {
    /// The group, by index into `groups`.
    pub group: usize,
    /// The slice, by index into `rate`.
    pub bin: usize,
    pub n: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
}

/// The requests one place in the program's code sent (`x-sonde-caller`):
/// their part of the waiting, failures and cost.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallerStat {
    pub place: String,
    pub requests: usize,
    pub waited_ms: f64,
    pub errors: usize,
    pub cost: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub writes: usize,
    pub reads: usize,
    pub free: usize,
    pub amount: f64,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// From the first request's start to the last one's end.
    pub wall_ms: f64,
    /// Time with a request in flight.
    pub waiting_ms: f64,
    pub peak_in_flight: usize,
    pub peak_at_ms: f64,
    /// Requests in flight on average while any was.
    pub mean_in_flight: f64,
    /// Where the time went: most first.
    pub groups: Vec<Group>,
    pub bin_ms: f64,
    pub rate: Vec<Bin>,
    pub in_flight: Vec<Step>,
    /// The slowest requests, at most 400, slowest first.
    pub samples: Vec<Sample>,
    pub cost: Cost,
    /// The prices the cost is reckoned at.
    pub prices: RequestPrices,
    /// The busiest prefixes, most requests first (at most 8, then "other").
    pub prefixes: Vec<PrefixStat>,
    /// Requests per prefix (by index) and slice of the session (by `rate`'s index).
    pub heat: Vec<HeatCell>,
    /// Where requests went: operation → prefix → outcome, each a flow of requests.
    pub flows: Vec<Flow>,
    /// Runs of requests made one after another (each waiting for the one
    /// before), longest first: where requests at once would save time.
    pub chains: Vec<Chain>,
    /// How long requests took, all of them: a count per bucket
    /// ([`LATENCY_EDGES`]), and the median, p95 and p99.
    pub latency: Vec<LatencyBucket>,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    /// The busiest kinds' latency over the session, slice by slice.
    pub group_rate: Vec<GroupBin>,
    /// The places in the program's code that sent requests, most waiting first.
    pub callers: Vec<CallerStat>,
    /// Whether the requests' times are known: a CloudTrail file has none, and
    /// what took how long is not said of it.
    pub timed: bool,
}

/// Requests made one at a time, each starting once the one before ended
/// (with no other request in flight): the program waited for each in turn.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chain {
    pub start_ms: f64,
    pub end_ms: f64,
    pub requests: usize,
    /// The record ids, in order.
    pub ids: Vec<u64>,
    /// What it would take with [`CHAIN_CONCURRENCY`] at once, about.
    pub at_once_ms: f64,
    /// The time spent waiting on the requests themselves (the gaps between
    /// them are the program's own work, which running them at once keeps).
    pub waiting_ms: f64,
}

/// The requests at once a chain is weighed with: what SDKs default to, or near.
pub const CHAIN_CONCURRENCY: usize = 8;
/// The fewest requests a chain is told of.
pub const CHAIN_MIN: usize = 10;
/// The longest wait between two requests of a chain (the program's own time).
const CHAIN_GAP_US: i64 = 10_000;

/// The chains of `records`: see [`Chain`].
pub fn chains(records: &[Record]) -> Vec<Chain> {
    let mut sorted: Vec<&Record> = records.iter().collect();
    sorted.sort_by_key(|r| (start_us(r), r.id));
    let Some(t0) = sorted.first().map(|r| start_us(r)) else {
        return Vec::new();
    };
    let ms = |us: i64| (us - t0) as f64 / 1000.0;
    let mut out: Vec<Chain> = Vec::new();
    let mut cur: Vec<&Record> = Vec::new();
    let mut end = i64::MIN;
    let close = |cur: &mut Vec<&Record>, out: &mut Vec<Chain>| {
        if cur.len() >= CHAIN_MIN {
            let (a, b) = (
                start_us(cur[0]),
                cur.iter().map(|r| end_us(r)).max().unwrap_or(0),
            );
            // the same requests at once: the waves of the longest ones
            let mut lat: Vec<i64> = cur.iter().map(|r| end_us(r) - start_us(r)).collect();
            lat.sort_unstable_by(|x, y| y.cmp(x));
            let at_once: i64 = lat.iter().step_by(CHAIN_CONCURRENCY).sum();
            out.push(Chain {
                start_ms: ms(a),
                end_ms: ms(b),
                requests: cur.len(),
                ids: cur.iter().map(|r| r.id).collect(),
                at_once_ms: at_once as f64 / 1000.0,
                waiting_ms: lat.iter().sum::<i64>() as f64 / 1000.0,
            });
        }
        cur.clear();
    };
    for r in sorted {
        let s = start_us(r);
        if !cur.is_empty() && (s < end || s - end > CHAIN_GAP_US) {
            // an overlap (requests at once already) or the program's own pause
            let overlapped = s < end;
            close(&mut cur, &mut out);
            if overlapped {
                end = end.max(end_us(r));
                continue;
            }
        }
        if cur.is_empty() && s < end {
            end = end.max(end_us(r));
            continue;
        }
        cur.push(r);
        end = end_us(r);
    }
    close(&mut cur, &mut out);
    out.sort_by(|a, b| {
        (b.end_ms - b.start_ms - b.at_once_ms).total_cmp(&(a.end_ms - a.start_ms - a.at_once_ms))
    });
    out
}

/// A prefix (a key's folder, three levels at most): S3 scales its request
/// rate per prefix.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixStat {
    pub prefix: String,
    /// The folders under it shown with it (0: this folder alone). A prefix
    /// that stands for deeper folders says so wherever it is named.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub folders: usize,
    pub requests: usize,
    pub reads: usize,
    pub writes: usize,
    pub errors: usize,
    /// The most requests started in any one second.
    pub peak_per_second: usize,
    /// Of reads and of writes, in any one second.
    pub peak_reads: usize,
    pub peak_writes: usize,
    /// What its requests cost: where the money went, by part of the key space.
    pub cost: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatCell {
    pub prefix: usize,
    pub bin: usize,
    pub n: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Flow {
    pub source: String,
    pub target: String,
    pub value: usize,
}

/// S3's request rate per prefix and second: reads (GET, HEAD) and writes
/// (PUT, COPY, POST, DELETE).
pub const READS_PER_PREFIX: usize = 5_500;
pub const WRITES_PER_PREFIX: usize = 3_500;

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// A key's prefix for S3's rate limits: its bucket and folder, three levels
/// at most ("b/logs/2026/"). What a session is *shown* by is [`prefix_groups`].
pub fn prefix_of(r: &Record) -> String {
    let (b, short) = prefix_parts(r);
    prefix_named(b, short)
}

/// [`prefix_of`] as the record's own text: its bucket, and its key's
/// folder cut to three levels. For counting a million requests by prefix
/// without writing a million names.
pub fn prefix_parts(r: &Record) -> (&str, &str) {
    let b = r.bucket.as_deref().unwrap_or("");
    let k = r.key.as_deref().unwrap_or("");
    let dir = match k.rfind('/') {
        Some(i) => &k[..=i],
        None => "",
    };
    let short = match dir.match_indices('/').nth(2) {
        Some((i, _)) => &dir[..=i],
        None => dir,
    };
    (b, short)
}

/// The name [`prefix_of`] gives the parts of [`prefix_parts`].
pub fn prefix_named(bucket: &str, short: &str) -> String {
    match (bucket.is_empty(), short.is_empty()) {
        (true, true) => "/".into(),
        (true, false) => short.to_string(),
        _ => format!("{bucket}/{short}"),
    }
}

/// A key's whole folder with its bucket ("b/warehouse/db/events/data/"),
/// however deep: what the prefixes shown are chosen from.
pub fn folder_of(r: &Record) -> String {
    let b = r.bucket.as_deref().unwrap_or("");
    let k = r.key.as_deref().unwrap_or("");
    let dir = match k.rfind('/') {
        Some(i) => &k[..=i],
        None => "",
    };
    match (b.is_empty(), dir.is_empty()) {
        (true, true) => "/".into(),
        (true, false) => dir.to_string(),
        _ => format!("{b}/{dir}"),
    }
}

/// The folder above `folder` ("a/b/c/" → "a/b/"), if it has one.
fn parent_of(folder: &str) -> Option<&str> {
    let inner = folder.strip_suffix('/')?;
    inner.rfind('/').map(|i| &folder[..=i])
}

/// The prefixes a session is shown by: its folders at the depth that tells
/// them apart, not cut at a fixed level. With more folders than `max`, the
/// quietest of the deepest is folded into the nearest folder above it that
/// it shares with another, again and again, so what stays apart is what
/// carried the requests. Each name with how many of the session's folders
/// were folded into it (0: that folder alone).
pub fn prefix_groups<'a>(
    folders: impl Iterator<Item = &'a str>,
    max: usize,
) -> Vec<(String, usize)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for f in folders {
        *counts.entry(f).or_default() += 1;
    }
    fold_folders(counts.into_iter(), max)
}

/// [`prefix_groups`] of folders each named once, with its requests.
fn fold_folders<'a>(
    counts: impl Iterator<Item = (&'a str, usize)>,
    max: usize,
) -> Vec<(String, usize)> {
    use std::cmp::Reverse;
    /// A group: its requests, whether a key lies right in it, and the
    /// folders of the session folded into it.
    #[derive(Default)]
    struct G {
        requests: usize,
        own: bool,
        folded: usize,
    }
    let mut groups: BTreeMap<String, G> = counts
        .map(|(f, requests)| {
            let g = G {
                requests,
                own: true,
                folded: 0,
            };
            (f.to_string(), g)
        })
        .collect();
    if groups.len() > max {
        let depth = |f: &str| f.matches('/').count();
        // the deepest first, of them the quietest; an entry whose requests
        // are no longer the group's is an old one
        let mut heap: std::collections::BinaryHeap<(usize, Reverse<usize>, Reverse<String>)> =
            groups
                .iter()
                .map(|(f, g)| (depth(f), Reverse(g.requests), Reverse(f.clone())))
                .collect();
        while groups.len() > max {
            // nothing left to fold (buckets, keys with no folder): the caller keeps the busiest
            let Some((_, Reverse(n), Reverse(f))) = heap.pop() else {
                break;
            };
            if groups.get(&f).is_none_or(|g| g.requests != n) {
                continue;
            }
            // a folder alone under its ancestors goes straight to where it
            // meets another: walked up a level at a time it would be counted
            // once per level. One that meets nothing stays (and never will:
            // folding only moves folders up within what they already share)
            let Some(to) = meeting(&groups, &f) else {
                continue;
            };
            let Some(g) = groups.remove(&f) else { continue };
            let p = groups.entry(to.clone()).or_default();
            p.requests += g.requests;
            p.folded += g.folded + usize::from(g.own);
            heap.push((depth(&to), Reverse(p.requests), Reverse(to)));
        }
    }
    let mut out: Vec<(String, usize, usize)> = groups
        .into_iter()
        .map(|(f, g)| (f, g.requests, g.folded))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.into_iter().map(|(f, _, under)| (f, under)).collect()
}

/// The nearest folder above `f` that is a group itself or holds another
/// group beside `f`: where folding `f` joins it with something.
fn meeting<G>(groups: &BTreeMap<String, G>, f: &str) -> Option<String> {
    use std::ops::Bound::{Excluded, Included, Unbounded};
    // the first name past everything under `f` ('0' follows '/')
    let past = format!("{}0", f.strip_suffix('/')?);
    let mut above = parent_of(f);
    while let Some(a) = above {
        // names from `a` up to `f` all start with `a` (and `a` itself is one)
        let before = groups
            .range::<str, _>((Included(a), Excluded(f)))
            .next()
            .is_some();
        let after = || {
            groups
                .range::<str, _>((Included(past.as_str()), Unbounded))
                .next()
                .is_some_and(|(k, _)| k.starts_with(a))
        };
        if before || after() {
            return Some(a.to_string());
        }
        above = parent_of(a);
    }
    None
}

/// Which of `groups` a folder belongs to: the longest that holds it.
pub fn prefix_for<'g>(groups: &'g [(String, usize)], folder: &str) -> Option<&'g str> {
    groups
        .iter()
        .map(|(g, _)| g.as_str())
        .filter(|g| folder.starts_with(g))
        .max_by_key(|g| g.len())
}

pub fn is_read(r: &Record) -> bool {
    matches!(r.method.as_str(), "GET" | "HEAD")
}

/// The most of `times` (µs, sorted) within any one second.
pub fn peak_second(times: &[i64]) -> usize {
    let (mut best, mut lo) = (0, 0);
    for hi in 0..times.len() {
        while times[hi] - times[lo] >= 1_000_000 {
            lo += 1;
        }
        best = best.max(hi - lo + 1);
    }
    best
}

const MAX_PREFIXES: usize = 8;

const MAX_BINS: f64 = 90.0;
const MAX_STEPS: usize = 480;
/// Requests drawn one by one: every request of a session this size or
/// smaller; of a larger one, the slowest and an even spread of the rest.
const MAX_SAMPLES: usize = 4000;
const SLOWEST: usize = 400;
/// The kinds of request whose latency is drawn over time.
const GROUP_RATES: usize = 5;

pub(crate) fn start_us(r: &Record) -> i64 {
    r.time.timestamp_micros()
}

pub(crate) fn end_us(r: &Record) -> i64 {
    start_us(r) + r.latency_us()
}

/// The time with a request in flight, in ms.
pub fn waiting_ms(records: &[Record]) -> f64 {
    waiting_ms_of(records.iter())
}

/// [`waiting_ms`] of any records, wherever they are kept.
pub fn waiting_ms_of<'a>(records: impl Iterator<Item = &'a Record>) -> f64 {
    union_us(records.map(|r| (start_us(r), end_us(r))).collect()) as f64 / 1000.0
}

/// Each kind's part of the waiting, in microseconds: every stretch of time
/// split evenly among the requests in flight then, so that requests at once
/// count once and the kinds add up to the time with a request in flight.
pub(crate) fn waited_us<K: Ord + Clone>(by: &BTreeMap<K, Vec<&Record>>) -> BTreeMap<K, f64> {
    let keys: Vec<&K> = by.keys().collect();
    let mut events: Vec<(i64, i32, usize)> = Vec::new();
    for (k, rs) in by.values().enumerate() {
        for r in rs {
            let (s, e) = (start_us(r), end_us(r));
            if e > s {
                events.push((s, 1, k));
                events.push((e, -1, k));
            }
        }
    }
    // ends before starts at the same moment
    events.sort_unstable();
    let mut active = vec![0i64; keys.len()];
    let mut total = 0i64;
    let mut out = vec![0f64; keys.len()];
    let mut last = events.first().map(|e| e.0).unwrap_or(0);
    for (t, d, k) in events {
        if total > 0 && t > last {
            let dt = (t - last) as f64;
            for (i, n) in active.iter().enumerate() {
                if *n > 0 {
                    out[i] += dt * *n as f64 / total as f64;
                }
            }
        }
        last = t;
        active[k] += d as i64;
        total += d as i64;
    }
    keys.into_iter().cloned().zip(out).collect()
}

/// The length of the union of intervals, in microseconds.
fn union_us(mut iv: Vec<(i64, i64)>) -> i64 {
    iv.sort_unstable();
    let mut total = 0;
    let mut cur: Option<(i64, i64)> = None;
    for (s, e) in iv {
        match &mut cur {
            Some((_, ce)) if s <= *ce => *ce = (*ce).max(e),
            _ => {
                if let Some((cs, ce)) = cur {
                    total += ce - cs;
                }
                cur = Some((s, e));
            }
        }
    }
    if let Some((cs, ce)) = cur {
        total += ce - cs;
    }
    total
}

/// 1, 2 or 5 times a power of ten: the slice of the rate plot.
fn nice(ms: f64) -> f64 {
    let ms = ms.max(1.0);
    let p = 10f64.powf(ms.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * p >= ms {
            return m * p;
        }
    }
    10.0 * p
}

/// A session's time axis: its first instant, its length, and the slices
/// every chart over the session shares.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeline {
    /// The first request's start, in µs.
    pub t0_us: i64,
    /// From there to the last request's end (1 at least).
    pub wall_us: i64,
    pub bin_ms: f64,
    pub bins: usize,
}

impl Timeline {
    /// None without a request.
    pub fn of(records: &[Record]) -> Option<Timeline> {
        let t0_us = records.iter().map(start_us).min()?;
        let t1 = records.iter().map(end_us).max().unwrap_or(t0_us);
        let wall_us = (t1 - t0_us).max(1);
        let wall_ms = wall_us as f64 / 1000.0;
        let bin_ms = nice(wall_ms / MAX_BINS);
        Some(Timeline {
            t0_us,
            wall_us,
            bin_ms,
            bins: ((wall_ms / bin_ms).ceil() as usize).max(1),
        })
    }

    /// The slice an instant (µs) falls in.
    pub fn bin_at(&self, us: i64) -> usize {
        (((us - self.t0_us) as f64 / 1000.0 / self.bin_ms) as usize).min(self.bins - 1)
    }

    /// The slice a request started in.
    pub fn bin_of(&self, r: &Record) -> usize {
        self.bin_at(start_us(r))
    }
}

/// The `q` quantile of sorted values by nearest rank (the smallest value
/// with at least `q` of them at or under it; 0 without any). The one
/// definition every figure uses: a session's, a kind's, a slice's.
pub fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() as f64 * q).ceil() as usize).clamp(1, sorted.len()) - 1]
}

/// Whether a request reached the store: one sonde refused, one it answered
/// in the store's place (a trial's fault), or one that got no answer, is not
/// on the store's bill. A fault done to the store's own answer (a lost
/// answer, a body cut short, a race the store had let the write win) is.
pub fn reached_store(r: &Record) -> bool {
    let has = |name: &str| {
        r.response_headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(name))
    };
    r.status.is_some()
        && !has("x-sonde-proxy")
        && !(has("x-sonde-fault") && !has("x-sonde-store-status"))
}

/// What one request costs: nothing when it never reached the store.
pub fn price(r: &Record, prices: &RequestPrices) -> f64 {
    match class(r) {
        _ if !reached_store(r) => 0.0,
        Class::Write if r.operation.starts_with("List") => {
            prices.per_1000_lists.unwrap_or(prices.per_1000_writes) / 1000.0
        }
        Class::Write => prices.per_1000_writes / 1000.0,
        Class::Read => prices.per_1000_reads / 1000.0,
        Class::Free => 0.0,
    }
}

fn tone(r: &Record) -> u8 {
    match r.status {
        _ if is_miss(r) => 1,
        Some(s) if (200..400).contains(&s) => 0,
        Some(s) if (400..500).contains(&s) => 2,
        _ => 3,
    }
}

/// A session's prefixes ([`prefix_groups`], the busiest [`MAX_PREFIXES`] and
/// then "other"), each with its requests and its busiest second; and the
/// groups they were counted by.
pub fn prefix_stats(
    records: &[Record],
    prices: &RequestPrices,
) -> (Vec<(String, usize)>, Vec<PrefixStat>) {
    // each folder named once, however many requests went to it: a session
    // has far fewer folders than requests
    let mut ids: HashMap<(&str, &str), usize> = HashMap::new();
    let mut names: Vec<(String, usize)> = Vec::new();
    let mut last: Option<(&str, &str, usize)> = None;
    let of: Vec<usize> = records
        .iter()
        .map(|r| {
            let b = r.bucket.as_deref().unwrap_or("");
            let k = r.key.as_deref().unwrap_or("");
            let dir = k.rfind('/').map_or("", |i| &k[..=i]);
            // a run of requests to one folder is looked up once
            let id = match last {
                Some((lb, ld, id)) if lb == b && ld == dir => id,
                _ => *ids.entry((b, dir)).or_insert_with(|| {
                    names.push((folder_of(r), 0));
                    names.len() - 1
                }),
            };
            last = Some((b, dir, id));
            names[id].1 += 1;
            id
        })
        .collect();
    // two (bucket, folder) pairs can spell one name ("" and "a/", "a" and ""):
    // counted together
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (f, n) in &names {
        *counts.entry(f.as_str()).or_default() += n;
    }
    // at the depth that tells the folders apart, the quiet ones folded into the folder above
    let shown = fold_folders(counts.iter().map(|(f, n)| (*f, *n)), MAX_PREFIXES);
    // the groups by their requests: the busiest each a prefix, the rest "other"
    let mut requests: BTreeMap<&str, usize> = BTreeMap::new();
    let group: Vec<&str> = names
        .iter()
        .map(|(f, n)| {
            let g = prefix_for(&shown, f).unwrap_or("/");
            *requests.entry(g).or_default() += n;
            g
        })
        .collect();
    let mut ranked: Vec<(&str, usize)> = requests.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let slots = ranked.len().min(MAX_PREFIXES + 1);
    let slot: Vec<usize> = group
        .iter()
        .map(|g| {
            let at = ranked.iter().position(|(name, _)| name == g).unwrap_or(0);
            at.min(MAX_PREFIXES)
        })
        .collect();
    let under: HashMap<&str, usize> = shown.iter().map(|(g, n)| (g.as_str(), *n)).collect();
    let mut prefixes: Vec<PrefixStat> = (0..slots)
        .map(|i| {
            let prefix = if i < MAX_PREFIXES {
                ranked[i].0
            } else {
                "other"
            };
            PrefixStat {
                folders: under.get(prefix).copied().unwrap_or(0),
                prefix: prefix.to_string(),
                requests: 0,
                reads: 0,
                writes: 0,
                errors: 0,
                peak_per_second: 0,
                peak_reads: 0,
                peak_writes: 0,
                cost: 0.0,
            }
        })
        .collect();
    // their requests, and when: for the busiest second of reads and of writes
    let mut all: Vec<Vec<i64>> = vec![Vec::new(); slots];
    let mut reads: Vec<Vec<i64>> = vec![Vec::new(); slots];
    let mut writes: Vec<Vec<i64>> = vec![Vec::new(); slots];
    for (r, id) in records.iter().zip(of) {
        let i = slot[id];
        let p = &mut prefixes[i];
        p.requests += 1;
        p.errors += usize::from(r.is_error() && !is_miss(r));
        p.cost += price(r, prices);
        let t = start_us(r);
        all[i].push(t);
        if is_read(r) {
            reads[i].push(t);
        } else {
            writes[i].push(t);
        }
    }
    // records are mostly in the order they started: sorted only when not
    let peak = |times: &mut Vec<i64>| {
        if !times.is_sorted() {
            times.sort_unstable();
        }
        peak_second(times)
    };
    for (i, p) in prefixes.iter_mut().enumerate() {
        (p.reads, p.writes) = (reads[i].len(), writes[i].len());
        p.peak_per_second = peak(&mut all[i]);
        p.peak_reads = peak(&mut reads[i]);
        p.peak_writes = peak(&mut writes[i]);
    }
    (shown, prefixes)
}

pub fn profile(records: &[Record], prices: &RequestPrices) -> Profile {
    let empty = Profile {
        wall_ms: 0.0,
        waiting_ms: 0.0,
        peak_in_flight: 0,
        peak_at_ms: 0.0,
        mean_in_flight: 0.0,
        groups: Vec::new(),
        bin_ms: 1.0,
        rate: Vec::new(),
        in_flight: Vec::new(),
        samples: Vec::new(),
        cost: Cost {
            writes: 0,
            reads: 0,
            free: 0,
            amount: 0.0,
            currency: prices.currency.clone(),
        },
        prices: prices.clone(),
        prefixes: Vec::new(),
        heat: Vec::new(),
        flows: Vec::new(),
        chains: Vec::new(),
        latency: Vec::new(),
        p50_ms: 0.0,
        p95_ms: 0.0,
        p99_ms: 0.0,
        group_rate: Vec::new(),
        callers: Vec::new(),
        timed: false,
    };
    let Some(tl) = Timeline::of(records) else {
        return empty;
    };
    let (t0, wall_us) = (tl.t0_us, tl.wall_us);
    let busy_us = union_us(records.iter().map(|r| (start_us(r), end_us(r))).collect());
    let ms = |us: i64| us as f64 / 1000.0;
    let price = |r: &Record| price(r, prices);

    // groups
    // (named by the records' own text: a kind is not copied for each request)
    let mut by: BTreeMap<(&str, bool), Vec<&Record>> = BTreeMap::new();
    for r in records {
        by.entry((r.operation.as_str(), is_miss(r)))
            .or_default()
            .push(r);
    }
    let waited = waited_us(&by);
    let mut groups: Vec<(Group, Vec<&Record>)> = by
        .into_iter()
        .map(|(key, rs)| {
            let waited_us = waited.get(&key).copied().unwrap_or(0.0);
            let (operation, missed) = (key.0.to_string(), key.1);
            let mut lat: Vec<f64> = rs.iter().map(|r| r.latency_ms.max(0.0)).collect();
            lat.sort_by(f64::total_cmp);
            let busy = union_us(rs.iter().map(|r| (start_us(r), end_us(r))).collect());
            let g = Group {
                operation,
                missed,
                requests: rs.len(),
                waiting_ms: ms(busy),
                sum_ms: lat.iter().sum(),
                waited_ms: waited_us / 1000.0,
                share: waited_us / busy_us.max(1) as f64,
                p50_ms: percentile(&lat, 0.5),
                p95_ms: percentile(&lat, 0.95),
                max_ms: lat.last().copied().unwrap_or(0.0),
                errors: rs.iter().filter(|r| r.is_error() && !is_miss(r)).count(),
                bytes_up: rs.iter().map(|r| r.request_bytes.unwrap_or(0)).sum(),
                bytes_down: rs.iter().map(|r| r.response_bytes.unwrap_or(0)).sum(),
                cost: rs.iter().map(|r| price(r)).sum(),
            };
            (g, rs)
        })
        .collect();
    groups.sort_by(|a, b| {
        b.0.waited_ms
            .total_cmp(&a.0.waited_ms)
            .then(b.0.requests.cmp(&a.0.requests))
    });
    let group_of: HashMap<u64, usize> = groups
        .iter()
        .enumerate()
        .flat_map(|(i, (_, rs))| rs.iter().map(move |r| (r.id, i)))
        .collect();

    // rate, by outcome
    let (bin, bins) = (tl.bin_ms, tl.bins);
    let mut rate: Vec<Bin> = (0..bins)
        .map(|i| Bin {
            t_ms: i as f64 * bin,
            ok: 0,
            miss: 0,
            client: 0,
            server: 0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
            throttled: 0,
            retried: 0,
            bytes_up: 0,
            bytes_down: 0,
        })
        .collect();
    let mut took: Vec<Vec<f64>> = vec![Vec::new(); bins];
    for r in records {
        let i = tl.bin_of(r);
        took[i].push(r.latency_ms);
        let b = &mut rate[i];
        b.throttled += usize::from(throttled(r));
        b.retried += usize::from(r.retries > 0);
        b.bytes_up += r.request_bytes.unwrap_or(0);
        b.bytes_down += r.response_bytes.unwrap_or(0);
        match tone(r) {
            0 => b.ok += 1,
            1 => b.miss += 1,
            2 => b.client += 1,
            _ => b.server += 1,
        }
    }

    // how long requests took, slice by slice: the band the live view draws
    for (b, mut l) in rate.iter_mut().zip(took) {
        if l.is_empty() {
            continue;
        }
        l.sort_by(f64::total_cmp);
        let at = |q: f64| percentile(&l, q);
        (b.p50_ms, b.p95_ms, b.p99_ms) = (at(0.5), at(0.95), at(0.99));
    }

    // in flight: a step at each start and end, the busiest kept per slice
    // a request that took no time (refused, failed before it left) is never in flight
    let mut events: Vec<(i64, i32)> = records
        .iter()
        .filter(|r| end_us(r) > start_us(r))
        .flat_map(|r| [(start_us(r), 1), (end_us(r), -1)])
        .collect();
    // ends before starts at one instant: a request answered as the next leaves
    events.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut n: i64 = 0;
    let mut steps: Vec<Step> = Vec::new();
    let (mut peak, mut peak_at) = (0usize, 0.0);
    for (t, d) in events {
        n = (n + d as i64).max(0);
        let at = ms(t - t0);
        if n as usize > peak {
            peak = n as usize;
            peak_at = at;
        }
        match steps.last_mut() {
            Some(s) if s.t_ms == at => s.n = n as usize,
            _ => steps.push(Step {
                t_ms: at,
                n: n as usize,
            }),
        }
    }
    if steps.len() > MAX_STEPS {
        // per slice its busiest moment, then where it ended: a burst's peak
        // does not run on through the idle time after it
        let slice = ms(wall_us) / MAX_STEPS as f64;
        let mut per: Vec<(f64, usize, usize)> = Vec::with_capacity(MAX_STEPS + 1);
        for s in steps {
            let at = (s.t_ms / slice).floor() * slice;
            match per.last_mut() {
                Some(k) if k.0 == at => {
                    k.1 = k.1.max(s.n);
                    k.2 = s.n;
                }
                _ => per.push((at, s.n, s.n)),
            }
        }
        steps = Vec::with_capacity(per.len() * 2);
        for (at, max, last) in per {
            steps.push(Step { t_ms: at, n: max });
            if last != max {
                steps.push(Step {
                    t_ms: at + slice * 0.5,
                    n: last,
                });
            }
        }
    }

    // the requests one by one: all of them, or the slowest and an even
    // spread of the rest (in time order), so the picture is not only the tail
    let sample = |r: &Record| Sample {
        id: r.id,
        group: group_of[&r.id],
        ms: r.latency_ms.max(0.0),
        t_ms: ms(start_us(r) - t0),
        status: r.status,
        bytes: r.request_bytes.unwrap_or(0) + r.response_bytes.unwrap_or(0),
    };
    let mut samples: Vec<Sample> = if records.len() <= MAX_SAMPLES {
        records.iter().map(sample).collect()
    } else {
        let mut by_ms: Vec<&Record> = records.iter().collect();
        by_ms.sort_by(|a, b| b.latency_ms.total_cmp(&a.latency_ms));
        let slow: std::collections::HashSet<u64> = by_ms[..SLOWEST].iter().map(|r| r.id).collect();
        let rest: Vec<&Record> = records.iter().filter(|r| !slow.contains(&r.id)).collect();
        let want = MAX_SAMPLES - SLOWEST;
        let step = rest.len() as f64 / want as f64;
        by_ms[..SLOWEST]
            .iter()
            .copied()
            .chain((0..want).map(|i| rest[((i as f64 * step) as usize).min(rest.len() - 1)]))
            .map(sample)
            .collect()
    };
    samples.sort_by(|a, b| a.t_ms.total_cmp(&b.t_ms));

    // the busiest kinds' latency, slice by slice
    let mut group_rate = Vec::new();
    for (gi, (_, rs)) in groups.iter().enumerate().take(GROUP_RATES) {
        let mut per: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
        for r in rs {
            let i = tl.bin_of(r);
            per.entry(i).or_default().push(r.latency_ms.max(0.0));
        }
        for (i, mut lat) in per {
            lat.sort_by(f64::total_cmp);
            group_rate.push(GroupBin {
                group: gi,
                bin: i,
                n: lat.len(),
                p50_ms: percentile(&lat, 0.5),
                p95_ms: percentile(&lat, 0.95),
            });
        }
    }

    // the places in the program's code that sent them, when it says
    let mut by_place: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for r in records {
        if let Some(p) = crate::callers::caller(r) {
            by_place.entry(p).or_default().push(r);
        }
    }
    let place_waited = waited_us(&by_place);
    let mut callers: Vec<CallerStat> = by_place
        .into_iter()
        .map(|(place, rs)| CallerStat {
            waited_ms: place_waited.get(&place).copied().unwrap_or(0.0) / 1000.0,
            requests: rs.len(),
            errors: rs.iter().filter(|r| r.is_error() && !is_miss(r)).count(),
            cost: rs.iter().map(|r| price(r)).sum(),
            place,
        })
        .collect();
    callers.sort_by(|a, b| {
        b.waited_ms
            .total_cmp(&a.waited_ms)
            .then(b.requests.cmp(&a.requests))
    });
    callers.truncate(12);

    let count = |c: Class| {
        records
            .iter()
            .filter(|r| reached_store(r) && class(r) == c)
            .count()
    };
    let cost = Cost {
        writes: count(Class::Write),
        reads: count(Class::Read),
        free: count(Class::Free),
        amount: records.iter().map(&price).sum(),
        currency: prices.currency.clone(),
    };
    let (shown, prefixes) = prefix_stats(records, prices);
    let prefix_index = |r: &Record| {
        let f = folder_of(r);
        let p = prefix_for(&shown, &f).unwrap_or("/");
        prefixes
            .iter()
            .take(MAX_PREFIXES)
            .position(|s| s.prefix == p)
            .unwrap_or(prefixes.len().saturating_sub(1))
    };
    let mut cells: HashMap<(usize, usize), usize> = HashMap::new();
    for r in records {
        let b = tl.bin_of(r);
        *cells.entry((prefix_index(r), b)).or_default() += 1;
    }
    let mut heat: Vec<HeatCell> = cells
        .into_iter()
        .map(|((prefix, bin), n)| HeatCell { prefix, bin, n })
        .collect();
    heat.sort_by_key(|c| (c.prefix, c.bin));

    // flows: operation → prefix → outcome
    let outcome = |r: &Record| match tone(r) {
        0 => "ok",
        1 => "not found",
        2 => "client error",
        _ => "server error",
    };
    let top_ops: Vec<String> =
        groups
            .iter()
            .map(|(g, _)| g.operation.clone())
            .fold(Vec::new(), |mut v, o| {
                if !v.contains(&o) {
                    v.push(o);
                }
                v
            });
    let op_name = |r: &Record| {
        if top_ops.iter().take(6).any(|o| *o == r.operation) {
            r.operation.to_string()
        } else {
            "other operations".into()
        }
    };
    let mut flow: BTreeMap<(String, String), usize> = BTreeMap::new();
    for r in records {
        let p = &prefixes[prefix_index(r)].prefix;
        let p = if p == "other" {
            "other prefixes".to_string()
        } else {
            p.clone()
        };
        *flow.entry((op_name(r), p.clone())).or_default() += 1;
        *flow.entry((p, outcome(r).to_string())).or_default() += 1;
    }
    let flows: Vec<Flow> = flow
        .into_iter()
        .map(|((source, target), value)| Flow {
            source,
            target,
            value,
        })
        .collect();

    let sum_ms: f64 = records.iter().map(|r| r.latency_ms.max(0.0)).sum();
    // the distribution, every request in it (not a sample)
    let mut all: Vec<f64> = records.iter().map(|r| r.latency_ms.max(0.0)).collect();
    all.sort_by(f64::total_cmp);
    let q = |p: f64| percentile(&all, p);
    let mut latency: Vec<LatencyBucket> = LATENCY_EDGES
        .iter()
        .map(|&le_ms| LatencyBucket { le_ms, n: 0 })
        .collect();
    for v in &all {
        let i = LATENCY_EDGES
            .iter()
            .position(|e| v <= e)
            .unwrap_or(LATENCY_EDGES.len() - 1);
        latency[i].n += 1;
    }
    // from the first bucket with a request to the last: no empty tails
    let first = latency.iter().position(|b| b.n > 0).unwrap_or(0);
    let last = latency.iter().rposition(|b| b.n > 0).unwrap_or(0);
    let latency: Vec<LatencyBucket> = latency[first..=last].to_vec();
    // a log without times (CloudTrail) says what was asked and when, not how
    // long it took: no chains, no distribution
    let timed = records.iter().any(|r| r.latency_ms > 0.0);
    Profile {
        wall_ms: ms(wall_us),
        waiting_ms: ms(busy_us),
        peak_in_flight: peak,
        peak_at_ms: peak_at,
        mean_in_flight: if busy_us > 0 {
            sum_ms / ms(busy_us)
        } else {
            0.0
        },
        groups: groups.into_iter().map(|(g, _)| g).collect(),
        bin_ms: bin,
        rate,
        in_flight: steps,
        samples,
        group_rate,
        callers,
        cost,
        prices: prices.clone(),
        prefixes,
        heat,
        flows,
        chains: if timed { chains(records) } else { Vec::new() },
        latency: if timed { latency } else { Vec::new() },
        p50_ms: q(0.5),
        p95_ms: q(0.95),
        p99_ms: q(0.99),
        timed,
    }
}

/// One kind of request on two stores (a session and its replay): how long it
/// took on each.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyChange {
    #[serde(rename = "kind")]
    pub operation: String,
    pub missed: bool,
    pub requests: (usize, usize),
    pub p50_ms: (f64, f64),
    pub p95_ms: (f64, f64),
    pub waiting_ms: (f64, f64),
}

/// The kinds of request two sessions share, by how much slower (p95) the
/// second is, slowest first: a workload measured on two stores.
pub fn latency_changes(a: &Profile, b: &Profile) -> Vec<LatencyChange> {
    let mut out: Vec<LatencyChange> = a
        .groups
        .iter()
        .filter_map(|ga| {
            let gb = b
                .groups
                .iter()
                .find(|g| g.operation == ga.operation && g.missed == ga.missed)?;
            Some(LatencyChange {
                operation: ga.operation.clone(),
                missed: ga.missed,
                requests: (ga.requests, gb.requests),
                p50_ms: (ga.p50_ms, gb.p50_ms),
                p95_ms: (ga.p95_ms, gb.p95_ms),
                waiting_ms: (ga.waiting_ms, gb.waiting_ms),
            })
        })
        .collect();
    let ratio = |c: &LatencyChange| c.p95_ms.1.max(0.01) / c.p95_ms.0.max(0.01);
    out.sort_by(|x, y| ratio(y).total_cmp(&ratio(x)));
    out
}

/// A group's change from one run to the other.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupChange {
    #[serde(rename = "kind")]
    pub operation: String,
    pub missed: bool,
    pub requests: (usize, usize),
    /// The run's waiting that was this kind's in each ([`Group::waited_ms`]:
    /// what `show`'s "waiting on" says), not the time one was in flight.
    pub waiting_ms: (f64, f64),
}

/// Two runs of a program, before and after a change.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunChange {
    pub requests: (usize, usize),
    pub wall_ms: (f64, f64),
    pub waiting_ms: (f64, f64),
    pub errors: (usize, usize),
    pub cost: (f64, f64),
    pub currency: String,
    pub peak_in_flight: (usize, usize),
    /// Kinds of request, the largest change first.
    pub groups: Vec<GroupChange>,
    /// Findings of the first run the second no longer has at any
    /// threshold (fixed).
    pub resolved: Vec<FindingText>,
    /// Findings only the second run has.
    pub appeared: Vec<FindingText>,
    /// Each finding of either run, where it stands: fixed (with what it
    /// saved, measured, and what was predicted), less, new, back or open.
    pub findings: Vec<crate::fixloop::FindingChange>,
    /// Both are runs of one program. Two programs' figures still compare,
    /// but their findings do not: one's are not "gone" because the other
    /// never had them.
    pub same_program: bool,
    /// The exit code of a run compared that failed (the later's, when both
    /// did): its findings are not compared, a crash is no fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<i32>,
    /// Both runs' latency, bucket by bucket on the same edges: their two
    /// distributions side by side.
    pub latency: (Vec<LatencyBucket>, Vec<LatencyBucket>),
    /// The busiest prefixes of either run, each run's requests to them: where
    /// the change happened.
    pub prefixes: Vec<PrefixChange>,
}

/// One prefix in two runs: its requests in each.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixChange {
    pub prefix: String,
    pub requests: (usize, usize),
}

impl RunChange {
    /// Says whether the two runs are of one program (`command`s by
    /// [`crate::program::program_key`]); across programs the findings
    /// gone and new are left out.
    pub fn of_programs(mut self, a: Option<&str>, b: Option<&str>) -> Self {
        let (ka, kb) = (
            crate::program::program_key(a),
            crate::program::program_key(b),
        );
        self.same_program = ka.is_some() && ka == kb;
        if !self.same_program {
            self.resolved.clear();
            self.appeared.clear();
            self.findings.clear();
        }
        self
    }

    /// Says whether a run compared failed (`exits`, as kept: none when not
    /// known); then their findings are not compared.
    pub fn of_exits(mut self, exits: (Option<i32>, Option<i32>)) -> Self {
        let failed = |x: Option<i32>| x.filter(|x| *x != 0);
        self.failed = failed(exits.1).or(failed(exits.0));
        if self.failed.is_some() {
            self.resolved.clear();
            self.appeared.clear();
            self.findings.clear();
        }
        self
    }
}

/// A finding as it reads: in English, and as phrases for other languages.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingText {
    pub text: String,
    pub phrases: Vec<crate::phrase::Phrase>,
}

/// A finding's identity across runs: what it is and what it is about, not
/// its counts, nor what changes from one run to the next in the keys it
/// names ([`stable`]). Set on a finding when it is made ([`name_findings`]).
pub fn finding_identity(o: &Observation) -> String {
    if o.identity.is_empty() {
        identity(o)
    } else {
        o.identity.clone()
    }
}

/// [`finding_identity`] worked out, before one is set.
pub(crate) fn identity(o: &Observation) -> String {
    identity_as(o, false)
}

/// A finding's identity, what it is about made [`stable`] or as it was
/// (`literal`).
fn identity_as(o: &Observation, literal: bool) -> String {
    let Some(p) = o.phrases.first() else {
        return o.text.clone();
    };
    // one finding whatever its form: one or many, under a threshold or
    // over. Reads and writes near a prefix's limit are two
    let key = p.key.trim_end_matches(".rate").trim_end_matches(".one");
    let about: Vec<String> = ["key", "prefix", "format"]
        .iter()
        .filter_map(|k| {
            p.args.get(k).map(|v| {
                if literal {
                    v.to_string()
                } else {
                    stable(v.as_ref())
                }
            })
        })
        .collect();
    format!("{key}|{}", about.join("|"))
}

/// Names each of a run's findings ([`Observation::identity`]) by what it is
/// about made [`stable`]; but where two findings of the run (of `findings`
/// and `among`, its findings at any threshold) read the same so, each keeps
/// what it is about as it was: `t00/` and `t01/` listed again and again are
/// two findings, not one. Two that read the same even so are told apart by
/// rank (`#2`).
pub fn name_findings(findings: &mut [Observation], among: &[Observation]) {
    use std::collections::HashSet;
    let mut by: HashMap<String, HashSet<String>> = HashMap::new();
    for o in findings.iter().chain(among) {
        by.entry(identity_as(o, false))
            .or_default()
            .insert(identity_as(o, true));
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    for o in findings.iter_mut() {
        let id = identity_as(o, false);
        let id = if by.get(&id).is_some_and(|l| l.len() > 1) {
            identity_as(o, true)
        } else {
            id
        };
        let n = seen.entry(id.clone()).or_default();
        *n += 1;
        o.identity = if *n == 1 { id } else { format!("{id}#{n}") };
    }
}

/// The kinds of finding, as an identity begins (`obs.hot` of `obs.hot|b/k`):
/// what `[check] expect` may name.
pub const FINDING_KINDS: &[&str] = &[
    "obs.unfinished",
    "obs.orphans",
    "obs.small",
    "obs.hot",
    "obs.listed",
    "obs.headget",
    "obs.rename",
    "obs.markers",
    "obs.throttled",
    "obs.prefixrate.reads",
    "obs.prefixrate.writes",
    "obs.serial",
];

/// The version of the rules a finding's identity is made by ([`stable`]
/// and [`identity`]): kept with a program's findings (`findings.json`), so
/// findings kept under older rules are made again by these, not read as one
/// fixed and another new. 1 was 19.0's; 2 takes every number out.
pub const IDENTITY_RULES: u32 = 2;

/// A key or prefix with what changes from one run of a program to the next
/// taken out: the value of a `name=value` folder `name={v}`, a UUID
/// `{uuid}`, a date `{date}`, a date and time `{time}`, a word of six or
/// more hex letters and digits (a commit, a short run id) or a long mixed
/// id `{id}`, and every other run of digits `{n}` (`part-{n}`, `run-{n}`,
/// `v{n}`). Two findings of one run that read the same once so are told
/// apart by their keys as they were ([`Run::with`]).
pub fn stable(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        // a folder or file `name=value` (Hive's partitions): its value
        if i == 0 || b[i - 1] == b'/' {
            let seg = s[i..].find('/').map_or(&s[i..], |e| &s[i..i + e]);
            if let Some((name, value)) = seg.split_once('=')
                && !name.is_empty()
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
            {
                out.push_str(name);
                out.push('=');
                if !value.is_empty() {
                    out.push_str("{v}");
                }
                i += seg.len();
                continue;
            }
        }
        if !b[i].is_ascii_alphanumeric() {
            // a whole character: keys are UTF-8
            let ch = s[i..].chars().next().unwrap_or('?');
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        if let Some(end) = uuid_at(b, i) {
            out.push_str("{uuid}");
            i = end;
            continue;
        }
        if let Some((end, time)) = date_at(b, i) {
            out.push_str(if time { "{time}" } else { "{date}" });
            i = end;
            continue;
        }
        let end = word_end(b, i);
        let w = &s[i..end];
        let has = |f: fn(&u8) -> bool| w.as_bytes().iter().any(f);
        let hex = !has(|c| !c.is_ascii_hexdigit());
        if (w.len() >= 6 && hex && has(u8::is_ascii_digit) && has(u8::is_ascii_alphabetic))
            || (w.len() >= 20 && has(u8::is_ascii_digit) && has(u8::is_ascii_alphabetic))
        {
            out.push_str("{id}");
        } else {
            // each run of digits
            let mut digits = false;
            for c in w.chars() {
                if c.is_ascii_digit() {
                    if !digits {
                        out.push_str("{n}");
                    }
                    digits = true;
                } else {
                    out.push(c);
                    digits = false;
                }
            }
        }
        i = end;
    }
    out
}

/// The end of the word (letters and digits) at `i`.
fn word_end(b: &[u8], i: usize) -> usize {
    i + b[i..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count()
}

/// The end of a UUID at `i` (8-4-4-4-12 hex digits), followed by no letter
/// or digit.
fn uuid_at(b: &[u8], i: usize) -> Option<usize> {
    let mut at = i;
    for (n, len) in [8, 4, 4, 4, 12].into_iter().enumerate() {
        if n > 0 {
            (b.get(at) == Some(&b'-')).then_some(())?;
            at += 1;
        }
        b.get(at..at + len)?
            .iter()
            .all(u8::is_ascii_hexdigit)
            .then_some(())?;
        at += len;
    }
    (!b.get(at).is_some_and(u8::is_ascii_alphanumeric)).then_some(at)
}

/// A date at `i` (`2026-11-13`, `2026_11_13`, `2026/11/13`, `20261113`),
/// and the time of day after it when there is one (`T10:00:00.123Z`,
/// `T100000Z`, `_10-00`): where it ends, and whether it had a time.
fn date_at(b: &[u8], i: usize) -> Option<(usize, bool)> {
    let num = |at: usize, below: u32| -> Option<usize> {
        let run = b.get(at..at + 2)?;
        let n = std::str::from_utf8(run).ok()?.parse::<u32>().ok()?;
        (run.iter().all(u8::is_ascii_digit) && n < below).then_some(at + 2)
    };
    let year: u32 = std::str::from_utf8(b.get(i..i + 4)?).ok()?.parse().ok()?;
    (1970..2100).contains(&year).then_some(())?;
    let sep = b
        .get(i + 4)
        .copied()
        .filter(|c| matches!(c, b'-' | b'_' | b'/'));
    let mut at = i + 4 + usize::from(sep.is_some());
    let month = num(at, 13).filter(|_| b[at..at + 2] != *b"00")?;
    at = month;
    if let Some(s) = sep {
        (b.get(at) == Some(&s)).then_some(())?;
        at += 1;
    }
    at = num(at, 32).filter(|_| b[at..at + 2] != *b"00")?;
    let ends = |at: usize| !b.get(at).is_some_and(u8::is_ascii_alphanumeric);
    // the time of day: hours and minutes, perhaps seconds and a fraction
    if matches!(b.get(at), Some(b'T' | b' ' | b'_' | b'-')) {
        let sep = |t: usize| usize::from(matches!(b.get(t), Some(b':' | b'-' | b'_')));
        if let Some(mut t) = num(at + 1, 24)
            && let Some(m) = num(t + sep(t), 60)
        {
            t = m;
            if let Some(s) = num(t + sep(t), 60) {
                t = s;
            }
            if b.get(t) == Some(&b'.') && b.get(t + 1).is_some_and(u8::is_ascii_digit) {
                t += 1 + b[t + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            }
            t += usize::from(b.get(t) == Some(&b'Z'));
            if ends(t) {
                return Some((t, true));
            }
        }
    }
    ends(at).then_some((at, false))
}

/// What failed a request, as two runs' errors are compared: its status
/// and S3's code (`500 InternalError`), `no answer`, or its status `cut
/// short` (an answer that began and broke off).
pub fn error_class(r: &Record) -> String {
    match (r.status, crate::episode::code_of(r)) {
        (None, _) => "no answer".to_string(),
        (Some(s), _) if s < 400 => format!("{s} cut short"),
        (Some(s), Some(c)) => format!("{s} {c}"),
        (Some(s), None) => s.to_string(),
    }
}

/// A run's failed requests by [`error_class`]: throttling and lookups that
/// found nothing are not counted.
fn error_classes(records: &[Record]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for r in records
        .iter()
        .filter(|r| r.is_error() && !is_miss(r) && !throttled(r))
    {
        *out.entry(error_class(r)).or_default() += 1;
    }
    out
}

/// One run as [`compare_runs`] holds it: what it amounts to, its records
/// not kept. Two runs are compared one read after the other, so comparing
/// costs what reading the larger costs, not both at once.
#[derive(Debug, Clone)]
pub struct Run {
    pub requests: usize,
    /// Its failed requests (lookups that found nothing are no failure).
    pub errors: usize,
    pub profile: Profile,
    pub observations: Vec<Observation>,
    /// Its requests by folder ([`folder_of`]): what the prefixes both runs
    /// are shown by are chosen from.
    pub folders: HashMap<String, usize>,
    /// Its findings at any threshold, by identity: a finding of the run
    /// before that is only among these is less, not fixed.
    pub traces: BTreeMap<String, Observation>,
    /// What its findings would save, by what their requests asked.
    pub asks: crate::fixloop::Asks,
    /// Its failed requests by what failed them ([`error_class`]), throttling
    /// (a finding of its own) and lookups that found nothing left out.
    pub error_classes: BTreeMap<String, usize>,
    /// How often it asked what the run before's findings would save
    /// ([`Run::measure`]), and whether that was counted.
    pub counts: HashMap<String, usize>,
    pub measured: bool,
}

impl Run {
    /// A run of `records`, its profile and findings reckoned already.
    pub fn of(records: &[Record], profile: Profile, observations: Vec<Observation>) -> Run {
        let traces = crate::episode::traces(records, &crate::episode::episodes(records));
        Run::with(records, profile, observations, traces)
    }

    /// [`Run::of`] a session's records, read with its header as its
    /// findings were ([`crate::episode::traces_of`]), its operations made
    /// already.
    pub fn of_session(
        header: &crate::session::SessionHeader,
        records: &[Record],
        episodes: &[crate::episode::Episode],
        profile: Profile,
        observations: Vec<Observation>,
    ) -> Run {
        let traces = crate::episode::traces_of(header, records, episodes);
        Run::with(records, profile, observations, traces)
    }

    /// Counts what `records` (this run's) asked of what `before`'s findings
    /// would save: what a fixed finding saved is measured by it.
    pub fn measure(&mut self, records: &[Record], before: &Run) {
        let wanted: std::collections::HashSet<&str> = before
            .asks
            .values()
            .flatten()
            .map(|(a, _, _)| a.as_str())
            .collect();
        self.counts = crate::fixloop::count(records, &wanted);
        self.measured = true;
    }

    /// A run of `records`, its profile, findings and findings at any
    /// threshold ([`crate::episode::traces`]) reckoned already.
    pub fn with(
        records: &[Record],
        profile: Profile,
        observations: Vec<Observation>,
        traces: Vec<Observation>,
    ) -> Run {
        let mut folders: HashMap<String, usize> = HashMap::new();
        let mut name = String::new();
        for r in records {
            // the folder named in a buffer: a string is kept once per folder
            name.clear();
            let b = r.bucket.as_deref().unwrap_or("");
            let k = r.key.as_deref().unwrap_or("");
            let dir = k.rfind('/').map_or("", |i| &k[..=i]);
            match (b.is_empty(), dir.is_empty()) {
                (true, true) => name.push('/'),
                (true, false) => name.push_str(dir),
                _ => {
                    name.push_str(b);
                    name.push('/');
                    name.push_str(dir);
                }
            }
            match folders.get_mut(name.as_str()) {
                Some(n) => *n += 1,
                None => {
                    folders.insert(name.clone(), 1);
                }
            }
        }
        // a run's findings are told apart among all of them, at any
        // threshold: as a finding and as a trace, one name
        let (mut observations, mut traces) = (observations, traces);
        name_findings(&mut traces, &observations);
        name_findings(&mut observations, &traces);
        let asks = crate::fixloop::asks_of(records, &observations);
        let error_classes = error_classes(records);
        let traces = traces
            .into_iter()
            .map(|o| (finding_identity(&o), o))
            .collect();
        Run {
            requests: records.len(),
            errors: records
                .iter()
                .filter(|r| r.is_error() && !is_miss(r))
                .count(),
            profile,
            observations,
            folders,
            traces,
            asks,
            error_classes,
            counts: HashMap::new(),
            measured: false,
        }
    }

    /// A run of `records` reckoned here: where its time went on one thread
    /// while its operations and findings are made on this one.
    pub fn analysed(
        header: &crate::session::SessionHeader,
        records: &[Record],
        prices: &RequestPrices,
    ) -> Run {
        let (profile, observations, traces) = std::thread::scope(|scope| {
            let p = scope.spawn(|| profile(records, prices));
            let episodes = crate::episode::episodes(records);
            let o = crate::episode::observations_of(header, records, &episodes);
            let t = crate::episode::traces_of(header, records, &episodes);
            drop(episodes);
            match p.join() {
                Ok(p) => (p, o, t),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        });
        Run::with(records, profile, observations, traces)
    }
}

pub fn compare_runs(a: &Run, b: &Run) -> RunChange {
    let keys: BTreeSet<(String, bool)> = a
        .profile
        .groups
        .iter()
        .chain(b.profile.groups.iter())
        .map(|g| (g.operation.clone(), g.missed))
        .collect();
    // a kind's waiting is its share of the run's, as `show` says it
    // ([`Group::waited_ms`]), not the time one of them was in flight
    let find = |p: &Profile, k: &(String, bool)| {
        p.groups
            .iter()
            .find(|g| g.operation == k.0 && g.missed == k.1)
            .map_or((0, 0.0), |g| (g.requests, g.waited_ms))
    };
    let mut groups: Vec<GroupChange> = keys
        .into_iter()
        .map(|k| {
            let (ra, ba) = find(&a.profile, &k);
            let (rb, bb) = find(&b.profile, &k);
            GroupChange {
                operation: k.0,
                missed: k.1,
                requests: (ra, rb),
                waiting_ms: (ba, bb),
            }
        })
        .collect();
    groups.sort_by(|x, y| {
        let d = |g: &GroupChange| (g.waiting_ms.1 - g.waiting_ms.0).abs();
        d(y).total_cmp(&d(x)).then(
            (y.requests.1 as i64 - y.requests.0 as i64)
                .abs()
                .cmp(&(x.requests.1 as i64 - x.requests.0 as i64).abs()),
        )
    });
    let findings = crate::fixloop::changes(a, b);
    use crate::fixloop::{State, said};
    // each prefix either run counted (its busiest, not the rest's "other")
    // one set of prefixes for both runs, chosen from the folders of both:
    // each run's own choice would name the same folder at two depths
    let mut both: HashMap<&str, usize> = HashMap::new();
    for (f, n) in a.folders.iter().chain(b.folders.iter()) {
        *both.entry(f.as_str()).or_default() += n;
    }
    let shared = fold_folders(both.into_iter(), MAX_PREFIXES * 2);
    let count = |fs: &HashMap<String, usize>, name: &str| -> usize {
        fs.iter()
            .filter(|(f, _)| prefix_for(&shared, f) == Some(name))
            .map(|(_, n)| n)
            .sum()
    };
    let mut prefixes: Vec<PrefixChange> = shared
        .iter()
        .map(|(name, _)| PrefixChange {
            prefix: name.clone(),
            requests: (count(&a.folders, name), count(&b.folders, name)),
        })
        .collect();
    prefixes.sort_by_key(|x| std::cmp::Reverse(x.requests.0.max(x.requests.1)));
    let (pa, pb) = (&a.profile, &b.profile);
    RunChange {
        prefixes,
        requests: (a.requests, b.requests),
        wall_ms: (pa.wall_ms, pb.wall_ms),
        waiting_ms: (pa.waiting_ms, pb.waiting_ms),
        errors: (a.errors, b.errors),
        cost: (pa.cost.amount, pb.cost.amount),
        currency: pb.cost.currency.clone(),
        peak_in_flight: (pa.peak_in_flight, pb.peak_in_flight),
        groups,
        latency: (pa.latency.clone(), pb.latency.clone()),
        resolved: said(&findings, &[State::Fixed]),
        appeared: said(&findings, &[State::New, State::Back]),
        findings,
        same_program: true,
        failed: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    fn rec(id: u64, at_ms: i64, op: &str, status: u16, latency: f64) -> Record {
        let method = match op {
            "PutObject" => "PUT",
            "HeadObject" => "HEAD",
            "DeleteObject" => "DELETE",
            _ => "GET",
        };
        Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
            method: method.into(),
            operation: op.into(),
            status: Some(status),
            latency_ms: latency,
            ..Default::default()
        }
    }

    #[test]
    fn what_changes_between_runs_is_taken_out_of_an_identity() {
        for (key, said) in [
            ("b/jobs/1794558464595/state.json", "b/jobs/{n}/state.json"),
            (
                "b/runs/0f8fad5b-d9cb-469f-a165-70867728950e/out",
                "b/runs/{uuid}/out",
            ),
            // Hive's partitions, and a date and hour as one value
            (
                "b/in/year=2026/month=11/day=13/hour=10/conf.json",
                "b/in/year={v}/month={v}/day={v}/hour={v}/conf.json",
            ),
            ("b/ev/dt=2026-11-13-21/x", "b/ev/dt={v}/x"),
            ("b/at=2026-11-13T10:00:00.123Z/x", "b/at={v}/x"),
            // Delta's log, Iceberg's metadata and snapshots
            (
                "b/t/_delta_log/00000000000000000012.json",
                "b/t/_delta_log/{n}.json",
            ),
            (
                "b/t/metadata/00003-0f8fad5b-d9cb-469f-a165-70867728950e.metadata.json",
                "b/t/metadata/{n}-{uuid}.metadata.json",
            ),
            (
                "b/t/metadata/v12.metadata.json",
                "b/t/metadata/v{n}.metadata.json",
            ),
            (
                "b/t/metadata/snap-6894157311395042398-1-0f8fad5b-d9cb-469f-a165-70867728950e.avro",
                "b/t/metadata/snap-{n}-{n}-{uuid}.avro",
            ),
            // Spark's part files
            (
                "b/o/part-00000-0f8fad5b-d9cb-469f-a165-70867728950e-c000.snappy.parquet",
                "b/o/part-{n}-{uuid}-c{n}.snappy.parquet",
            ),
            (
                "b/o/_temporary/0/_temporary/attempt_202611131000_0001_m_000001_1/part-00000",
                "b/o/_temporary/{n}/_temporary/attempt_{n}_{n}_m_{n}_{n}/part-{n}",
            ),
            // a short run id, a counter, a time folder, a date
            ("b/job-3fa2b1c9/out", "b/job-{id}/out"),
            ("b/run-42/out", "b/run-{n}/out"),
            ("b/logs/22-46-01/x", "b/logs/{n}-{n}-{n}/x"),
            ("b/2026/11/13/x", "b/{date}/x"),
            ("b/run_20261113/x", "b/run_{date}/x"),
            ("b/c/3f786850e387550fdab836ed7e6dc881de23001b", "b/c/{id}"),
            ("b/u/01HF8X3KQZ7Y4M2N5P6R8T9VWX/x", "b/u/{id}/x"),
            // words stay
            ("b/static/config.json", "b/static/config.json"),
            ("b/decade/x", "b/decade/x"),
            ("b/über/schlüssel", "b/über/schlüssel"),
        ] {
            assert_eq!(stable(key), said, "{key}");
        }
    }

    /// Two findings of one run that read the same once their numbers are
    /// out (`t00/`, `t01/`) keep their keys; one alone is said stable.
    #[test]
    fn two_findings_of_a_run_that_read_the_same_keep_their_keys() {
        let listed = |prefix: &str| Observation {
            level: "info",
            text: String::new(),
            phrases: vec![crate::phrase::p(
                "obs.listed",
                &[("prefix", prefix.into()), ("n", "9".into())],
            )],
            episodes: Vec::new(),
            impact: 9,
            saving: None,
            saving_text: None,
            saves_requests: 0,
            saves_ms: 0.0,
            saved_ids: Vec::new(),
            timed_ids: Vec::new(),
            fixes: Vec::new(),
            callers: Vec::new(),
            sdk: None,
            identity: String::new(),
        };
        let mut two = vec![listed("b/t00/"), listed("b/t01/")];
        name_findings(&mut two, &[]);
        assert_eq!(two[0].identity, "obs.listed|b/t00/");
        assert_eq!(two[1].identity, "obs.listed|b/t01/");
        // one over its threshold, the other under it: still two
        let mut one = vec![listed("b/t00/")];
        name_findings(&mut one, &[listed("b/t01/")]);
        assert_eq!(one[0].identity, "obs.listed|b/t00/");
        let mut alone = vec![listed("b/t00/")];
        name_findings(&mut alone, &[]);
        assert_eq!(alone[0].identity, "obs.listed|b/t{n}/");
        // the same key twice is told apart by rank
        let mut same = vec![listed("b/t00/"), listed("b/t00/")];
        name_findings(&mut same, &[]);
        assert_eq!(same[1].identity, "obs.listed|b/t{n}/#2");
    }

    #[test]
    fn reads_and_writes_near_a_prefixs_limit_are_two_findings() {
        let of = |key: &'static str| Observation {
            level: "warn",
            text: String::new(),
            phrases: vec![crate::phrase::p(
                key,
                &[("prefix", "b/t/".into()), ("n", "4000".into())],
            )],
            episodes: Vec::new(),
            impact: 0,
            saving: None,
            saving_text: None,
            saves_requests: 0,
            saves_ms: 0.0,
            saved_ids: Vec::new(),
            timed_ids: Vec::new(),
            fixes: Vec::new(),
            callers: Vec::new(),
            sdk: None,
            identity: String::new(),
        };
        let (r, w) = (
            finding_identity(&of("obs.prefixrate.reads")),
            finding_identity(&of("obs.prefixrate.writes")),
        );
        assert_eq!(r, "obs.prefixrate.reads|b/t/");
        assert_eq!(w, "obs.prefixrate.writes|b/t/");
        // one finding whatever its form
        assert_eq!(
            finding_identity(&of("obs.throttled.rate")),
            finding_identity(&of("obs.throttled"))
        );
    }

    #[test]
    fn a_kinds_waiting_in_a_comparison_is_its_share_as_show_says_it() {
        // a long write and short reads beside it all the while: the reads
        // were in flight nearly the whole run, but waited on for half of it
        let run = |reads: u64| {
            let mut rs = vec![rec(1, 0, "PutObject", 200, 1_000.0)];
            for i in 0..reads {
                let mut r = rec(i + 2, i as i64 * 100, "GetObject", 200, 100.0);
                r.bucket = Some("b".into());
                r.key = Some(format!("t/{}/f", i % 3).into());
                rs.push(r);
            }
            let p = profile(&rs, &RequestPrices::default());
            (Run::of(&rs, p.clone(), Vec::new()), p)
        };
        let ((a, pa), (b, _)) = (run(10), run(4));
        let c = compare_runs(&a, &b);
        let get = c
            .groups
            .iter()
            .find(|g| g.operation == "GetObject")
            .unwrap();
        let shown = pa
            .groups
            .iter()
            .find(|g| g.operation == "GetObject")
            .unwrap();
        assert_eq!(get.waiting_ms.0, shown.waited_ms);
        assert!(
            (shown.waited_ms - 500.0).abs() < 1e-6,
            "{}",
            shown.waited_ms
        );
        assert!(shown.waiting_ms > 900.0);
        // the kinds add up to the run's waiting, as they do in `show`
        let sum: f64 = c.groups.iter().map(|g| g.waiting_ms.0).sum();
        assert!((sum - c.waiting_ms.0).abs() < 1e-6);
        // and the prefixes are counted from each run's folders
        assert_eq!((c.requests, c.errors), ((11, 5), (0, 0)));
        let t0 = c.prefixes.iter().find(|p| p.prefix == "b/t/0/").unwrap();
        assert_eq!(t0.requests, (4, 2));
        assert_eq!(a.folders.values().sum::<usize>(), 11);
    }

    #[test]
    fn time_is_wall_clock_not_summed() {
        // ten reads at once for 50 ms, then a write of 100 ms
        let mut rs: Vec<Record> = (0..10)
            .map(|i| rec(i + 1, 0, "GetObject", 200, 50.0))
            .collect();
        rs.push(rec(11, 200, "PutObject", 200, 100.0));
        let p = profile(&rs, &RequestPrices::default());
        assert_eq!(p.wall_ms, 300.0);
        assert_eq!(p.waiting_ms, 150.0);
        assert_eq!(p.peak_in_flight, 10);
        let get = p
            .groups
            .iter()
            .find(|g| g.operation == "GetObject")
            .unwrap();
        assert_eq!(get.waiting_ms, 50.0);
        assert_eq!(get.sum_ms, 500.0);
        assert_eq!(p.groups[0].operation, "PutObject", "most time first");
        assert!((p.groups[0].share - 100.0 / 150.0).abs() < 1e-9);
        assert_eq!(p.rate.iter().map(|b| b.ok).sum::<usize>(), 11);
    }

    #[test]
    fn every_request_is_in_the_distribution_and_a_slow_down_in_its_slice() {
        let mut rs: Vec<Record> = (0..20)
            .map(|i| rec(i + 1, i as i64, "GetObject", 200, 3.0 + i as f64))
            .collect();
        let mut slow = rec(21, 30, "PutObject", 503, 400.0);
        slow.error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
        slow.retries = 2;
        rs.push(slow);
        let p = profile(&rs, &RequestPrices::default());
        assert_eq!(p.latency.iter().map(|b| b.n).sum::<usize>(), 21);
        assert_eq!(
            p.latency.first().map(|b| b.le_ms),
            Some(5.0),
            "no empty buckets before the first request"
        );
        assert_eq!(p.rate.iter().map(|b| b.throttled).sum::<usize>(), 1);
        assert_eq!(p.rate.iter().map(|b| b.retried).sum::<usize>(), 1);
        assert!(p.p50_ms >= 12.0 && p.p50_ms <= 14.0, "{}", p.p50_ms);
        assert_eq!(p.p99_ms, 400.0);
    }

    #[test]
    fn overlapping_kinds_share_the_waiting_and_add_up_to_it() {
        // a read of 100 ms; a write over its second half and 50 ms more
        let rs = [
            rec(1, 0, "GetObject", 200, 100.0),
            rec(2, 50, "PutObject", 200, 100.0),
        ];
        let p = profile(&rs, &RequestPrices::default());
        assert_eq!(p.waiting_ms, 150.0);
        let by = |op: &str| p.groups.iter().find(|g| g.operation == op).unwrap();
        // the 50 ms both were in flight count once, half each
        assert!((by("GetObject").waited_ms - 75.0).abs() < 1e-6);
        assert!((by("PutObject").waited_ms - 75.0).abs() < 1e-6);
        let total: f64 = p.groups.iter().map(|g| g.share).sum();
        assert!((total - 1.0).abs() < 1e-9, "shares add up to {total}");
    }

    #[test]
    fn lookups_that_found_nothing_are_their_own_kind_and_requests_are_priced() {
        let rs = [
            rec(1, 0, "HeadObject", 404, 5.0),
            rec(2, 10, "HeadObject", 200, 5.0),
            rec(3, 20, "PutObject", 200, 5.0),
            rec(4, 30, "DeleteObject", 204, 5.0),
        ];
        let p = profile(&rs, &RequestPrices::default());
        assert!(
            p.groups
                .iter()
                .any(|g| g.operation == "HeadObject" && g.missed)
        );
        assert!(
            p.groups
                .iter()
                .any(|g| g.operation == "HeadObject" && !g.missed)
        );
        assert_eq!((p.cost.writes, p.cost.reads, p.cost.free), (1, 2, 1));
        assert!((p.cost.amount - (0.005 + 2.0 * 0.0004) / 1000.0).abs() < 1e-12);
        assert_eq!(p.rate.iter().map(|b| b.miss).sum::<usize>(), 1);
    }

    #[test]
    fn an_empty_session_has_an_empty_profile() {
        let p = profile(&[], &RequestPrices::default());
        assert!(p.groups.is_empty() && p.rate.is_empty());
    }

    #[test]
    fn many_steps_are_thinned_keeping_the_peaks() {
        let rs: Vec<Record> = (0..5000)
            .map(|i| {
                rec(
                    i + 1,
                    i as i64 * 2,
                    "GetObject",
                    200,
                    if i == 2500 { 0.5 } else { 1.0 },
                )
            })
            .collect();
        let p = profile(&rs, &RequestPrices::default());
        assert!(
            p.in_flight.len() <= 2 * MAX_STEPS + 2,
            "{}",
            p.in_flight.len()
        );
        assert_eq!(p.in_flight.iter().map(|s| s.n).max(), Some(1));
        assert!(p.rate.len() <= MAX_BINS as usize + 1);
        assert_eq!(p.samples.len(), rs.len().min(MAX_SAMPLES));
    }

    #[test]
    fn requests_by_the_code_that_sent_them_and_kinds_over_time() {
        use reqlog::Header;
        let rs: Vec<Record> = (0..6)
            .map(|i| Record {
                id: i + 1,
                time: chrono::DateTime::from_timestamp_millis(1_700_000_000_000 + i as i64 * 20)
                    .unwrap(),
                method: "GET".into(),
                operation: if i < 4 {
                    "GetObject".into()
                } else {
                    "HeadObject".into()
                },
                status: Some(200),
                latency_ms: 10.0,
                request_headers: vec![Header::new(
                    crate::callers::CALLER_HEADER,
                    if i < 4 {
                        "etl/load.py:12 read"
                    } else {
                        "etl/load.py:30 check"
                    },
                )],
                ..Default::default()
            })
            .collect();
        let p = profile(&rs, &RequestPrices::default());
        assert_eq!(p.callers.len(), 2);
        assert_eq!(p.callers[0].place, "etl/load.py:12 read");
        assert_eq!(p.callers[0].requests, 4);
        assert!((p.callers.iter().map(|c| c.waited_ms).sum::<f64>() - p.waiting_ms).abs() < 1e-6);
        // each sample says its answer; every request is drawn in a session this small
        assert_eq!(p.samples.len(), 6);
        assert!(p.samples.iter().all(|s| s.status == Some(200)));
        // the kinds' latency over time: both kinds, each in the slices it ran
        assert!(p.group_rate.iter().any(|g| g.group == 0));
        assert!(p.group_rate.iter().any(|g| g.group == 1));
        assert_eq!(p.group_rate.iter().map(|g| g.n).sum::<usize>(), 6);
    }

    #[test]
    fn two_runs_compared_by_kind_with_findings_resolved() {
        use crate::episode::{episodes, observations};
        // before: a key read 12 times; after: once
        let before: Vec<Record> = (0..12)
            .map(|i| {
                let mut r = rec(i + 1, i as i64 * 10, "GetObject", 200, 5.0);
                r.bucket = Some("b".into());
                r.key = Some("hot".into());
                r
            })
            .collect();
        let after = vec![before[0].clone()];
        let prices = RequestPrices::default();
        let (pa, pb) = (profile(&before, &prices), profile(&after, &prices));
        let (oa, ob) = (
            observations(&before, &episodes(&before)),
            observations(&after, &episodes(&after)),
        );
        let c = compare_runs(&Run::of(&before, pa, oa), &Run::of(&after, pb, ob));
        assert_eq!(c.requests, (12, 1));
        assert_eq!(c.groups[0].operation, "GetObject");
        assert_eq!(c.groups[0].requests, (12, 1));
        assert_eq!(
            c.resolved[0].text,
            "b/hot was read 12 times (100% of all requests)"
        );
        assert!(c.appeared.is_empty());
    }

    #[test]
    fn a_request_that_took_no_time_is_never_in_flight() {
        let rs = [
            rec(1, 0, "DeleteObject", 403, 0.0),
            rec(2, 1, "DeleteObject", 403, 0.0),
            rec(3, 10, "GetObject", 200, 5.0),
            rec(4, 20, "GetObject", 200, 5.0),
        ];
        let p = profile(&rs, &RequestPrices::default());
        assert_eq!(p.peak_in_flight, 1);
        assert_eq!(p.in_flight.last().map(|s| s.n), Some(0));
    }

    #[test]
    fn a_bursts_peak_ends_with_the_burst() {
        let mut rs: Vec<Record> = (0..600)
            .map(|i| rec(i + 1, i as i64, "GetObject", 200, 3.0))
            .collect();
        rs.push(rec(601, 100_000, "GetObject", 200, 5.0));
        let p = profile(&rs, &RequestPrices::default());
        let at_50s = p
            .in_flight
            .iter()
            .rev()
            .find(|s| s.t_ms <= 50_000.0)
            .unwrap();
        assert_eq!(
            at_50s.n,
            0,
            "{:?}",
            &p.in_flight[p.in_flight.len().saturating_sub(5)..]
        );
    }

    #[test]
    fn a_percentile_is_the_same_number_wherever_it_is_shown() {
        assert_eq!(percentile(&[], 0.5), 0.0);
        assert_eq!(percentile(&[10.0, 30.0], 0.5), 10.0);
        assert_eq!(percentile(&[10.0, 30.0], 0.95), 30.0);
        // two requests of one kind: the session's median is the kind's, the
        // slice's and the summary's
        let rs = [
            rec(1, 0, "GetObject", 200, 10.0),
            rec(2, 1, "GetObject", 200, 30.0),
        ];
        let p = profile(&rs, &RequestPrices::default());
        assert_eq!(p.p50_ms, 10.0);
        assert_eq!(p.groups[0].p50_ms, 10.0);
        assert_eq!(p.rate[0].p50_ms, 10.0);
        assert_eq!(p.group_rate[0].p50_ms, 10.0);
        let s = crate::summary::summarize(&rs);
        assert_eq!(s.operations[0].p50_ms, 10.0);
    }

    #[test]
    fn a_request_that_never_reached_the_store_is_not_on_its_bill() {
        let mut refused = rec(2, 10, "PutObject", 403, 0.0);
        refused.response_headers = vec![reqlog::Header::new("x-sonde-proxy", "refused")];
        let mut lost = rec(3, 20, "PutObject", 200, 5.0);
        lost.status = None;
        lost.transport_error = Some("connection refused".into());
        let rs = [rec(1, 0, "PutObject", 200, 5.0), refused, lost];
        let p = profile(&rs, &RequestPrices::default());
        assert_eq!((p.cost.writes, p.cost.reads, p.cost.free), (1, 0, 0));
        assert!((p.cost.amount - 0.005 / 1000.0).abs() < 1e-12);
        let by_kind: f64 = p.groups.iter().map(|g| g.cost).sum();
        assert!((by_kind - p.cost.amount).abs() < 1e-12);
        // the store's own refusal is a request it answered
        let p = profile(
            &[rec(1, 0, "PutObject", 403, 5.0)],
            &RequestPrices::default(),
        );
        assert_eq!(p.cost.writes, 1);
    }

    fn at(id: u64, key: &str) -> Record {
        Record {
            bucket: Some("lake".into()),
            key: Some(key.into()),
            ..rec(id, id as i64, "GetObject", 200, 1.0)
        }
    }

    #[test]
    fn folders_below_the_third_level_are_told_apart() {
        // a table's data and its metadata: one prefix when cut at three levels
        let records = vec![
            at(1, "warehouse/db/events/metadata/v1.json"),
            at(2, "warehouse/db/events/data/dt=2026-11-01/a.parquet"),
            at(3, "warehouse/db/events/data/dt=2026-11-01/b.parquet"),
            at(4, "warehouse/db/events/data/dt=2026-11-02/a.parquet"),
        ];
        let p = profile(&records, &RequestPrices::default());
        let names: Vec<&str> = p.prefixes.iter().map(|x| x.prefix.as_str()).collect();
        assert_eq!(
            names,
            [
                "lake/warehouse/db/events/data/dt=2026-11-01/",
                "lake/warehouse/db/events/data/dt=2026-11-02/",
                "lake/warehouse/db/events/metadata/",
            ]
        );
        assert!(p.prefixes.iter().all(|x| x.folders == 0));
        // S3's rate limit still goes by three levels
        assert_eq!(prefix_of(&records[1]), "lake/warehouse/db/events/");
    }

    #[test]
    fn the_quiet_folders_fold_into_the_one_above_and_it_says_how_many() {
        // thirty partitions, the later the busier, and the table's metadata
        let mut records = Vec::new();
        let mut id = 0;
        for day in 1..=30u64 {
            for _ in 0..day {
                id += 1;
                records.push(at(id, &format!("t/data/dt={day:02}/x.parquet")));
            }
        }
        for _ in 0..40 {
            id += 1;
            records.push(at(id, "t/metadata/v.json"));
        }
        let p = profile(&records, &RequestPrices::default());
        assert!(p.prefixes.len() <= MAX_PREFIXES, "{:?}", p.prefixes.len());
        // nothing is "other": every request is under a prefix that names where it went
        assert!(p.prefixes.iter().all(|x| x.prefix != "other"));
        assert_eq!(
            p.prefixes.iter().map(|x| x.requests).sum::<usize>(),
            records.len()
        );
        // the busiest partitions stay apart, the metadata too
        assert!(
            p.prefixes
                .iter()
                .any(|x| x.prefix == "lake/t/data/dt=30/" && x.folders == 0)
        );
        assert!(
            p.prefixes
                .iter()
                .any(|x| x.prefix == "lake/t/metadata/" && x.requests == 40)
        );
        // the quiet ones are in the folder above, which says how many it stands for
        let folded = p
            .prefixes
            .iter()
            .find(|x| x.prefix == "lake/t/data/")
            .unwrap();
        assert_eq!(folded.folders, 30 - (MAX_PREFIXES - 2));
    }

    #[test]
    fn a_lone_deep_folder_folds_once_however_far_up_it_goes() {
        // eight busy folders at the top and one quiet folder five levels down:
        // it joins the others at the bucket in one step, and is one folder
        let mut records = Vec::new();
        let mut id = 0;
        for p in 0..8u64 {
            for _ in 0..10 + p {
                id += 1;
                records.push(at(id, &format!("p{p}/k")));
            }
        }
        id += 1;
        records.push(at(id, "x/a/b/c/d/k"));
        let folders: Vec<String> = records.iter().map(folder_of).collect();
        let groups = prefix_groups(folders.iter().map(String::as_str), MAX_PREFIXES);
        assert_eq!(groups.len(), MAX_PREFIXES, "{groups:?}");
        // the deep folder and the quietest top one, and no folder between
        assert!(groups.contains(&("lake/".to_string(), 2)), "{groups:?}");
        assert!(
            groups.iter().all(|(g, _)| !g.starts_with("lake/x/")),
            "{groups:?}"
        );
        let p = profile(&records, &RequestPrices::default());
        assert_eq!(
            p.prefixes.iter().map(|x| x.requests).sum::<usize>(),
            records.len()
        );
        assert_eq!(p.prefixes.iter().map(|x| x.folders).sum::<usize>(), 2);
    }

    #[test]
    fn a_folder_that_shares_nothing_keeps_its_name() {
        // ten buckets, a deep folder in each: none meets another, none is
        // cut back to its bucket
        let records: Vec<Record> = (0..10u64)
            .map(|i| Record {
                bucket: Some(format!("b{i}").into()),
                ..at(i + 1, "a/b/c/k")
            })
            .collect();
        let folders: Vec<String> = records.iter().map(folder_of).collect();
        let groups = prefix_groups(folders.iter().map(String::as_str), MAX_PREFIXES);
        assert_eq!(groups.len(), 10);
        assert!(
            groups
                .iter()
                .all(|(g, n)| g.ends_with("/a/b/c/") && *n == 0),
            "{groups:?}"
        );
    }

    #[test]
    fn two_runs_are_compared_by_one_set_of_prefixes() {
        // the second run reads one partition more: both are named at the same depth
        let before = vec![at(1, "t/data/dt=01/a"), at(2, "t/metadata/v")];
        let after = vec![
            at(1, "t/data/dt=01/a"),
            at(2, "t/data/dt=02/a"),
            at(3, "t/metadata/v"),
        ];
        let prices = RequestPrices::default();
        let (pa, pb) = (profile(&before, &prices), profile(&after, &prices));
        let c = compare_runs(
            &Run::of(&before, pa.clone(), Vec::new()),
            &Run::of(&after, pb.clone(), Vec::new()),
        );
        let of = |name: &str| {
            c.prefixes
                .iter()
                .find(|x| x.prefix == name)
                .map(|x| x.requests)
        };
        assert_eq!(of("lake/t/data/dt=01/"), Some((1, 1)));
        assert_eq!(of("lake/t/data/dt=02/"), Some((0, 1)));
        assert_eq!(of("lake/t/metadata/"), Some((1, 1)));
    }

    #[test]
    fn a_prefix_one_run_counts_under_other_is_still_counted_in_the_comparison() {
        let at = |id: u64, key: String| Record {
            bucket: Some("b".into()),
            key: Some(key.into()),
            ..rec(id, id as i64, "GetObject", 200, 1.0)
        };
        // before: ten prefixes, p0/ the quietest of them (so under "other")
        let mut before = Vec::new();
        let mut id = 0;
        for p in 0..10u64 {
            for _ in 0..=p {
                id += 1;
                before.push(at(id, format!("p{p}/k")));
            }
        }
        // after: p0/ alone
        let after: Vec<Record> = (1..=5).map(|i| at(i, "p0/k".into())).collect();
        let prices = RequestPrices::default();
        let (pa, pb) = (profile(&before, &prices), profile(&after, &prices));
        let name = folder_of(&after[0]);
        assert!(
            !pa.prefixes.iter().any(|x| x.prefix == name),
            "the test needs {name} outside the first run's busiest"
        );
        let c = compare_runs(
            &Run::of(&before, pa.clone(), Vec::new()),
            &Run::of(&after, pb.clone(), Vec::new()),
        );
        let change = c.prefixes.iter().find(|x| x.prefix == name).unwrap();
        assert_eq!(change.requests, (1, 5));
    }
}

#[cfg(test)]
mod injected {
    use super::*;
    use reqlog::Header;

    #[test]
    fn a_request_sonde_answered_itself_is_not_priced() {
        let prices = RequestPrices::default();
        let put = Record {
            method: "PUT".into(),
            operation: "PutObject".into(),
            bucket: Some("b".into()),
            key: Some("k".into()),
            status: Some(200),
            ..Default::default()
        };
        assert!(price(&put, &prices) > 0.0);
        // a trial's 503: the store never saw it
        let mut fault = put.clone();
        fault.status = Some(503);
        fault.injected = Some("slowdown".into());
        fault
            .response_headers
            .push(Header::new("x-sonde-fault", "slowdown"));
        assert!(!reached_store(&fault));
        assert_eq!(price(&fault, &prices), 0.0);
        // an answer lost on purpose: the store made the write, and bills it
        let mut lost = fault.clone();
        lost.response_headers
            .push(Header::new("x-sonde-store-status", "200"));
        assert_eq!(price(&lost, &prices), price(&put, &prices));
        let p = profile(&[put, fault, lost], &prices);
        assert_eq!(p.cost.writes, 2);
    }
}
