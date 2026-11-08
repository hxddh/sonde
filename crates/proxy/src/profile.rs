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
    /// `busy_ms` over the session's busy time.
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
    let mut groups: Vec<(Group, Vec<&Record>)> = by
        .into_iter()
        .map(|((operation, missed), rs)| {
            let mut lat: Vec<f64> = rs.iter().map(|r| r.latency_ms.max(0.0)).collect();
            lat.sort_by(f64::total_cmp);
            let busy = union_us(rs.iter().map(|r| (start_us(r), end_us(r))).collect());
            let g = Group {
                operation,
                missed,
                requests: rs.len(),
                busy_ms: ms(busy),
                sum_ms: lat.iter().sum(),
                share: busy as f64 / busy_us.max(1) as f64,
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
        b.0.busy_ms
            .total_cmp(&a.0.busy_ms)
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
        })
        .collect();
    for r in records {
        let i = ((ms(start_us(r) - t0) / bin) as usize).min(bins - 1);
        let b = &mut rate[i];
        match tone(r) {
            0 => b.ok += 1,
            1 => b.miss += 1,
            2 => b.client += 1,
            _ => b.server += 1,
        }
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
