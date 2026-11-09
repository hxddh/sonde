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
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPrices {
    pub per_1000_writes: f64,
    pub per_1000_reads: f64,
    pub currency: String,
}

impl Default for RequestPrices {
    /// S3 Standard in us-east-1.
    fn default() -> Self {
        RequestPrices {
            per_1000_writes: 0.005,
            per_1000_reads: 0.0004,
            currency: "USD".into(),
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
    pub operation: String,
    /// GET or HEAD answered 404.
    pub missed: bool,
    pub requests: usize,
    /// Wall-clock time with one of them in flight.
    pub busy_ms: f64,
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
    pub busy_ms: f64,
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

/// A key's prefix: its bucket and folder, three levels at most ("b/logs/2026/").
pub fn prefix_of(r: &Record) -> String {
    let b = r.bucket.as_deref().unwrap_or("");
    let k = r.key.as_deref().unwrap_or("");
    let dir = match k.rfind('/') {
        Some(i) => &k[..=i],
        None => "",
    };
    let short: String = dir.split_inclusive('/').take(3).collect();
    match (b.is_empty(), short.is_empty()) {
        (true, true) => "/".into(),
        (true, false) => short,
        _ => format!("{b}/{short}"),
    }
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
const MAX_SAMPLES: usize = 400;

fn start_us(r: &Record) -> i64 {
    r.time.timestamp_micros()
}

fn end_us(r: &Record) -> i64 {
    start_us(r) + r.latency_us()
}

/// The time with a request in flight, in ms.
pub fn busy_ms(records: &[Record]) -> f64 {
    union_us(records.iter().map(|r| (start_us(r), end_us(r))).collect()) as f64 / 1000.0
}

/// Each kind's part of the waiting, in microseconds: every stretch of time
/// split evenly among the requests in flight then, so that requests at once
/// count once and the kinds add up to the time with a request in flight.
fn waited_us<K: Ord + Clone>(by: &BTreeMap<K, Vec<&Record>>) -> BTreeMap<K, f64> {
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

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

fn tone(r: &Record) -> u8 {
    match r.status {
        _ if is_miss(r) => 1,
        Some(s) if (200..400).contains(&s) => 0,
        Some(s) if (400..500).contains(&s) => 2,
        _ => 3,
    }
}

pub fn profile(records: &[Record], prices: &RequestPrices) -> Profile {
    let empty = Profile {
        wall_ms: 0.0,
        busy_ms: 0.0,
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
    };
    let Some(t0) = records.iter().map(start_us).min() else {
        return empty;
    };
    let t1 = records.iter().map(end_us).max().unwrap_or(t0);
    let wall_us = (t1 - t0).max(1);
    let busy_us = union_us(records.iter().map(|r| (start_us(r), end_us(r))).collect());
    let ms = |us: i64| us as f64 / 1000.0;
    let price = |c: Class| match c {
        Class::Write => prices.per_1000_writes / 1000.0,
        Class::Read => prices.per_1000_reads / 1000.0,
        Class::Free => 0.0,
    };

    // groups
    let mut by: BTreeMap<(String, bool), Vec<&Record>> = BTreeMap::new();
    for r in records {
        by.entry((r.operation.clone(), is_miss(r)))
            .or_default()
            .push(r);
    }
    let waited = waited_us(&by);
    let mut groups: Vec<(Group, Vec<&Record>)> = by
        .into_iter()
        .map(|(key, rs)| {
            let waited_us = waited.get(&key).copied().unwrap_or(0.0);
            let (operation, missed) = key;
            let mut lat: Vec<f64> = rs.iter().map(|r| r.latency_ms.max(0.0)).collect();
            lat.sort_by(f64::total_cmp);
            let busy = union_us(rs.iter().map(|r| (start_us(r), end_us(r))).collect());
            let g = Group {
                operation,
                missed,
                requests: rs.len(),
                busy_ms: ms(busy),
                sum_ms: lat.iter().sum(),
                waited_ms: waited_us / 1000.0,
                share: waited_us / busy_us.max(1) as f64,
                p50_ms: percentile(&lat, 0.5),
                p95_ms: percentile(&lat, 0.95),
                max_ms: lat.last().copied().unwrap_or(0.0),
                errors: rs.iter().filter(|r| r.is_error() && !is_miss(r)).count(),
                bytes_up: rs.iter().map(|r| r.request_bytes.unwrap_or(0)).sum(),
                bytes_down: rs.iter().map(|r| r.response_bytes.unwrap_or(0)).sum(),
                cost: rs.iter().map(|r| price(class(r))).sum(),
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
    let bin = nice(ms(wall_us) / MAX_BINS);
    let bins = ((ms(wall_us) / bin).ceil() as usize).max(1);
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
        let i = ((ms(start_us(r) - t0) / bin) as usize).min(bins - 1);
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
        let at = |q: f64| l[((l.len() as f64 * q).ceil() as usize).clamp(1, l.len()) - 1];
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

    // the slowest requests
    let mut samples: Vec<Sample> = records
        .iter()
        .map(|r| Sample {
            id: r.id,
            group: group_of[&r.id],
            ms: r.latency_ms.max(0.0),
            t_ms: ms(start_us(r) - t0),
        })
        .collect();
    samples.sort_by(|a, b| b.ms.total_cmp(&a.ms));
    samples.truncate(MAX_SAMPLES);

    let count = |c: Class| records.iter().filter(|r| class(r) == c).count();
    let cost = Cost {
        writes: count(Class::Write),
        reads: count(Class::Read),
        free: count(Class::Free),
        amount: records.iter().map(|r| price(class(r))).sum(),
        currency: prices.currency.clone(),
    };
    // prefixes: their requests, their busiest second, and when
    let mut per: HashMap<String, Vec<&Record>> = HashMap::new();
    for r in records {
        per.entry(prefix_of(r)).or_default().push(r);
    }
    let mut ranked: Vec<(String, Vec<&Record>)> = per.into_iter().collect();
    ranked.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    let stat = |prefix: String, rs: &[&Record]| {
        let mut all: Vec<i64> = rs.iter().map(|r| start_us(r)).collect();
        let mut reads: Vec<i64> = rs
            .iter()
            .filter(|r| is_read(r))
            .map(|r| start_us(r))
            .collect();
        let mut writes: Vec<i64> = rs
            .iter()
            .filter(|r| !is_read(r))
            .map(|r| start_us(r))
            .collect();
        all.sort_unstable();
        reads.sort_unstable();
        writes.sort_unstable();
        PrefixStat {
            prefix,
            requests: rs.len(),
            reads: reads.len(),
            writes: writes.len(),
            errors: rs.iter().filter(|r| r.is_error() && !is_miss(r)).count(),
            peak_per_second: peak_second(&all),
            peak_reads: peak_second(&reads),
            peak_writes: peak_second(&writes),
            cost: rs.iter().map(|r| price(class(r))).sum(),
        }
    };
    let mut prefixes: Vec<PrefixStat> = ranked
        .iter()
        .take(MAX_PREFIXES)
        .map(|(p, rs)| stat(p.clone(), rs))
        .collect();
    let rest: Vec<&Record> = ranked
        .iter()
        .skip(MAX_PREFIXES)
        .flat_map(|(_, rs)| rs.iter().copied())
        .collect();
    if !rest.is_empty() {
        prefixes.push(stat("other".into(), &rest));
    }
    let prefix_index = |r: &Record| {
        let p = prefix_of(r);
        prefixes
            .iter()
            .take(MAX_PREFIXES)
            .position(|s| s.prefix == p)
            .unwrap_or(prefixes.len().saturating_sub(1))
    };
    let mut cells: HashMap<(usize, usize), usize> = HashMap::new();
    for r in records {
        let b = ((ms(start_us(r) - t0) / bin) as usize).min(bins - 1);
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
            r.operation.clone()
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
    let q = |p: f64| all[((all.len() as f64 * p).ceil() as usize).clamp(1, all.len()) - 1];
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
    Profile {
        wall_ms: ms(wall_us),
        busy_ms: ms(busy_us),
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
        cost,
        prices: prices.clone(),
        prefixes,
        heat,
        flows,
        chains: chains(records),
        latency,
        p50_ms: q(0.5),
        p95_ms: q(0.95),
        p99_ms: q(0.99),
    }
}

/// One kind of request on two stores (a session and its replay): how long it
/// took on each.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyChange {
    pub operation: String,
    pub missed: bool,
    pub requests: (usize, usize),
    pub p50_ms: (f64, f64),
    pub p95_ms: (f64, f64),
    pub busy_ms: (f64, f64),
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
                busy_ms: (ga.busy_ms, gb.busy_ms),
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
    pub operation: String,
    pub missed: bool,
    pub requests: (usize, usize),
    pub busy_ms: (f64, f64),
}

/// Two runs of a program, before and after a change.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunChange {
    pub requests: (usize, usize),
    pub wall_ms: (f64, f64),
    pub busy_ms: (f64, f64),
    pub errors: (usize, usize),
    pub cost: (f64, f64),
    pub currency: String,
    pub peak_in_flight: (usize, usize),
    /// Kinds of request, the largest change first.
    pub groups: Vec<GroupChange>,
    /// Findings of the first run the second no longer has.
    pub resolved: Vec<FindingText>,
    /// Findings only the second run has.
    pub appeared: Vec<FindingText>,
    /// Both are runs of one program. Two programs' figures still compare,
    /// but their findings do not: one's are not "gone" because the other
    /// never had them.
    pub same_program: bool,
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
/// its counts.
/// A finding's identity across runs (see [`identity`]).
pub fn finding_identity(o: &Observation) -> String {
    if o.phrases.is_empty() {
        o.text.clone()
    } else {
        identity(o)
    }
}

fn identity(o: &Observation) -> String {
    let p = &o.phrases[0];
    // one finding whatever its form: one or many, under a threshold or over
    let key = p
        .key
        .trim_end_matches(".rate")
        .trim_end_matches(".one")
        .trim_end_matches(".reads")
        .trim_end_matches(".writes");
    let mut about: Vec<String> = match key {
        // which request was the slowest is a detail, not another finding
        "obs.slow" => Vec::new(),
        _ => ["key", "prefix", "format"]
            .iter()
            .filter_map(|k| p.args.get(*k).cloned())
            .collect(),
    };
    // a format's shared habit is the note it shares
    if key == "obs.shared"
        && let Some(note) = o.phrases.get(1)
    {
        about.push(note.key.to_string());
        about.extend(note.args.values().cloned());
    }
    format!("{key}|{}", about.join("|"))
}

pub fn compare_runs(
    a: (&[Record], &Profile, &[Observation]),
    b: (&[Record], &Profile, &[Observation]),
) -> RunChange {
    let errors = |rs: &[Record]| rs.iter().filter(|r| r.is_error() && !is_miss(r)).count();
    let keys: BTreeSet<(String, bool)> =
        a.1.groups
            .iter()
            .chain(b.1.groups.iter())
            .map(|g| (g.operation.clone(), g.missed))
            .collect();
    let find = |p: &Profile, k: &(String, bool)| {
        p.groups
            .iter()
            .find(|g| g.operation == k.0 && g.missed == k.1)
            .map_or((0, 0.0), |g| (g.requests, g.busy_ms))
    };
    let mut groups: Vec<GroupChange> = keys
        .into_iter()
        .map(|k| {
            let (ra, ba) = find(a.1, &k);
            let (rb, bb) = find(b.1, &k);
            GroupChange {
                operation: k.0,
                missed: k.1,
                requests: (ra, rb),
                busy_ms: (ba, bb),
            }
        })
        .collect();
    groups.sort_by(|x, y| {
        let d = |g: &GroupChange| (g.busy_ms.1 - g.busy_ms.0).abs();
        d(y).total_cmp(&d(x)).then(
            (y.requests.1 as i64 - y.requests.0 as i64)
                .abs()
                .cmp(&(x.requests.1 as i64 - x.requests.0 as i64).abs()),
        )
    });
    let ids = |o: &[Observation]| -> BTreeMap<String, FindingText> {
        o.iter()
            .filter(|o| !o.phrases.is_empty())
            .map(|o| {
                (
                    identity(o),
                    FindingText {
                        text: o.text.clone(),
                        phrases: o.phrases.clone(),
                    },
                )
            })
            .collect()
    };
    let (ia, ib) = (ids(a.2), ids(b.2));
    RunChange {
        requests: (a.0.len(), b.0.len()),
        wall_ms: (a.1.wall_ms, b.1.wall_ms),
        busy_ms: (a.1.busy_ms, b.1.busy_ms),
        errors: (errors(a.0), errors(b.0)),
        cost: (a.1.cost.amount, b.1.cost.amount),
        currency: b.1.cost.currency.clone(),
        peak_in_flight: (a.1.peak_in_flight, b.1.peak_in_flight),
        groups,
        resolved: ia
            .iter()
            .filter(|(k, _)| !ib.contains_key(*k))
            .map(|(_, t)| t.clone())
            .collect(),
        appeared: ib
            .iter()
            .filter(|(k, _)| !ia.contains_key(*k))
            .map(|(_, t)| t.clone())
            .collect(),
        same_program: true,
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
    fn time_is_wall_clock_not_summed() {
        // ten reads at once for 50 ms, then a write of 100 ms
        let mut rs: Vec<Record> = (0..10)
            .map(|i| rec(i + 1, 0, "GetObject", 200, 50.0))
            .collect();
        rs.push(rec(11, 200, "PutObject", 200, 100.0));
        let p = profile(&rs, &RequestPrices::default());
        assert_eq!(p.wall_ms, 300.0);
        assert_eq!(p.busy_ms, 150.0);
        assert_eq!(p.peak_in_flight, 10);
        let get = p
            .groups
            .iter()
            .find(|g| g.operation == "GetObject")
            .unwrap();
        assert_eq!(get.busy_ms, 50.0);
        assert_eq!(get.sum_ms, 500.0);
        assert_eq!(p.groups[0].operation, "PutObject", "most time first");
        assert!((p.groups[0].share - 100.0 / 150.0).abs() < 1e-9);
        assert_eq!(p.rate.iter().map(|b| b.ok).sum::<usize>(), 11);
    }

    #[test]
    fn every_request_is_in_the_distribution_and_a_slow_down_in_its_slice() {
        let mut rs: Vec<Record> = (0..20).map(|i| rec(i + 1, i as i64, "GetObject", 200, 3.0 + i as f64)).collect();
        let mut slow = rec(21, 30, "PutObject", 503, 400.0);
        slow.error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
        slow.retries = 2;
        rs.push(slow);
        let p = profile(&rs, &RequestPrices::default());
        assert_eq!(p.latency.iter().map(|b| b.n).sum::<usize>(), 21);
        assert_eq!(p.latency.first().map(|b| b.le_ms), Some(5.0), "no empty buckets before the first request");
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
        assert_eq!(p.busy_ms, 150.0);
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
        assert_eq!(p.samples.len(), MAX_SAMPLES);
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
        let c = compare_runs((&before, &pa, &oa), (&after, &pb, &ob));
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
}
