//! A run read by its four signals — rate, errors, latency, bytes — and how
//! many requests were in flight: each a figure with its line over the
//! session, on the slices the profile's charts use ([`Timeline`]). With them,
//! what the figures alone do not say: the time before an answer's first byte
//! against the rest, the sizes of bodies and how fast they moved, every
//! status the store gave and when, what each operation cost, and each
//! prefix's busiest second against S3's limit.
//!
//! One pass over the records and a few sorts: a million requests are read as
//! readily as a hundred.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use reqlog::Record;
use serde::Serialize;

use crate::profile::{
    Class, READS_PER_PREFIX, RequestPrices, Timeline, WRITES_PER_PREFIX, class, end_us,
    prefix_stats, price, reached_store, start_us,
};
use crate::summary::is_failure;

/// Requests a second.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rate {
    /// Over the whole run (none when the run took no time).
    pub value: Option<f64>,
    pub unit: &'static str,
    /// Slice by slice, in the same unit.
    pub series: Vec<f64>,
    pub requests: usize,
}

/// Failures of each kind: a lookup that found nothing is none of them.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorCounts {
    /// 4xx.
    pub client: usize,
    /// 5xx.
    pub server: usize,
    /// No answer, or one cut short.
    pub transport: usize,
    /// A fault scenario's doing, whatever it looked like: not the store's.
    pub injected: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Errors {
    /// The failed requests' share of all (0 to 1; none without a request).
    pub value: Option<f64>,
    pub unit: &'static str,
    /// Failed requests, slice by slice (the four kinds added up).
    pub series: Vec<f64>,
    pub count: usize,
    pub counts: ErrorCounts,
    /// Each kind, slice by slice.
    pub client: Vec<f64>,
    pub server: Vec<f64>,
    pub transport: Vec<f64>,
    pub injected: Vec<f64>,
    /// Times a request was sent again, slice by slice, and in all.
    pub retries: Vec<f64>,
    pub retries_total: u64,
}

/// How long requests took. None of it is said of a log without times.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Latency {
    /// The median.
    pub value: Option<f64>,
    pub unit: &'static str,
    /// The median, slice by slice (0 in a slice without a request).
    pub series: Vec<f64>,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    /// The bands over `series`.
    pub p95: Vec<f64>,
    pub p99: Vec<f64>,
}

/// Bytes of bodies a second, sent (up) and received (down).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bytes {
    /// Both ways, over the whole run.
    pub value: Option<f64>,
    pub unit: &'static str,
    /// Both ways, slice by slice.
    pub series: Vec<f64>,
    pub up_per_sec: Option<f64>,
    pub down_per_sec: Option<f64>,
    pub up: Vec<f64>,
    pub down: Vec<f64>,
    pub up_bytes: u64,
    pub down_bytes: u64,
}

/// Requests in flight: saturation.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InFlight {
    /// The most at once (the profile's `peakInFlight`).
    pub value: Option<f64>,
    pub unit: &'static str,
    /// The most at once within each slice.
    pub series: Vec<f64>,
    pub peak: usize,
    pub peak_at_ms: f64,
}

/// One kind of request: the wait for its answer's first byte, and the rest.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstByteRow {
    pub operation: String,
    /// The requests whose first byte was timed.
    pub count: usize,
    /// Medians.
    pub first_byte_ms: f64,
    pub rest_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstByte {
    /// Of the timed requests' time, the part before the first byte (0 to 1).
    pub share: f64,
    pub count: usize,
    /// Most requests first.
    pub kinds: Vec<FirstByteRow>,
}

/// Bodies of up to `le_bytes` (and more than the bucket before: each is
/// twice the last; 0 is the empty body).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeBucket {
    pub le_bytes: u64,
    pub count: usize,
    pub bytes: u64,
}

/// How fast one operation's bodies of one size moved.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Throughput {
    pub operation: String,
    /// The size bucket, by its upper bound in bytes.
    pub bucket: u64,
    pub count: usize,
    /// The median.
    pub mib_per_sec: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sizes {
    /// Bodies sent and received, smallest first, from the first bucket with
    /// one to the last. A request whose size is not known is in neither.
    pub request: Vec<SizeBucket>,
    pub response: Vec<SizeBucket>,
    /// Time against size: transfers of [`TRANSFER_MIN`] and more, by
    /// operation and then size.
    pub throughput: Vec<Throughput>,
    /// All of them: the median (none without a timed transfer).
    pub mib_per_sec: Option<f64>,
}

/// One answer the session got, and when.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusRow {
    /// None: no answer.
    pub status: Option<u16>,
    /// S3's error code, where the answer's body gave one.
    pub code: Option<String>,
    pub count: usize,
    /// Of them, a fault scenario's.
    pub injected: usize,
    /// The first and the last, from the session's start.
    pub first_ms: f64,
    pub last_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostRow {
    pub operation: String,
    /// Its price class: "write", "list", "read" or "free".
    pub class: &'static str,
    pub requests: usize,
    /// Of them, the ones the store bills (it answered them).
    pub billed: usize,
    pub cost: f64,
    /// Of the session's cost (0 when it cost nothing).
    pub share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostBy {
    /// The profile's `cost.amount`.
    pub amount: f64,
    pub currency: String,
    /// Dearest first.
    pub operations: Vec<CostRow>,
}

/// A prefix's busiest second against what S3 serves a prefix.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixRate {
    pub prefix: String,
    /// The folders shown with it (the profile's).
    pub folders: usize,
    pub requests: usize,
    pub peak_reads: usize,
    pub peak_writes: usize,
    /// Of 5,500 reads and of 3,500 writes a second (1 is the limit).
    pub read_share: f64,
    pub write_share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signals {
    /// Whether the requests' times are known (the profile's rule).
    pub timed: bool,
    pub wall_ms: f64,
    /// The run took under a second ([`SHORT_MS`]): it has totals and no
    /// rates (`rate.value`, `bytes.value` and the two ways are none). Nine
    /// requests in a third of a second are not "27 a second".
    pub short: bool,
    /// The slices every series is over: the profile's `binMs` and `rate`.
    pub bin_ms: f64,
    pub bins: usize,
    pub rate: Rate,
    pub errors: Errors,
    pub latency: Latency,
    pub bytes: Bytes,
    pub in_flight: InFlight,
    /// None when no request's first byte was timed.
    pub first_byte: Option<FirstByte>,
    pub sizes: Sizes,
    /// Most frequent first.
    pub statuses: Vec<StatusRow>,
    pub cost: CostBy,
    /// The profile's prefixes, in its order.
    pub prefixes: Vec<PrefixRate>,
}

/// A run under this long has no rate: there was no second to have one in.
pub const SHORT_MS: f64 = 1000.0;

/// Under this many requests a run has no shape over its time to draw: a
/// sparkline of it is a few stray ticks (the app's `FEW`).
pub const FEW: usize = 20;

/// How long after a failure its retry may start: the bound the episodes
/// group a request's attempts by.
pub const RETRY_WINDOW_US: i64 = 60_000_000;

/// Under this a steady wait is an SDK's fixed backoff, not a program's poll.
const POLL_MIN_US: i64 = 200_000;

/// A request's time as it is taken everywhere here: never negative, never
/// past a day ([`reqlog::MAX_LATENCY_MS`]), as its span on the timeline is.
fn took_ms(r: &Record) -> f64 {
    if r.latency_ms.is_finite() {
        r.latency_ms.clamp(0.0, reqlog::MAX_LATENCY_MS)
    } else {
        0.0
    }
}

/// Which records are retries: the same request sent again after a failure
/// that asks for it.
///
/// The rule. A request is named by [`crate::fault::identity_of`] and its
/// `Range` (two ranges of one object are two requests). After a retryable
/// failure ([`crate::episode::retryable`]: no answer, a 408, 429, 500, 502,
/// 503 or 504, SlowDown), its retry is the next request of that name, in
/// the order of their times and not of the file, that starts once the
/// failure was answered and within [`RETRY_WINDOW_US`] of it. A request is
/// the retry of one failure at most. And a request that fails three times
/// or more in a row at a steady pace of [`POLL_MIN_US`] or more (waits
/// within a tenth of their median) is a program polling a key that keeps
/// failing, not an SDK backing off: none of those is a retry. (An SDK set
/// to wait a fixed fifth of a second or more between three attempts reads
/// as that poll; the SDKs' own backoff grows or is jittered.)
fn retries_of(records: &[Record]) -> Vec<bool> {
    let mut again = vec![false; records.len()];
    // only the places a failure was at are named: naming every request of a
    // large session would cost more than the rest of this pass
    let places: std::collections::HashSet<(Option<&str>, Option<&str>)> = records
        .iter()
        .filter(|r| crate::episode::retryable(r))
        .map(|r| (r.bucket.as_deref(), r.key.as_deref()))
        .collect();
    if places.is_empty() {
        return again;
    }
    let mut groups: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, r) in records.iter().enumerate() {
        if !places.contains(&(r.bucket.as_deref(), r.key.as_deref())) {
            continue;
        }
        let range = r
            .request_headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("range"))
            .map_or("", |h| h.value.trim());
        groups
            .entry(format!("{} {range}", crate::fault::identity_of(r)))
            .or_default()
            .push(i);
    }
    for group in groups.values_mut() {
        if !group
            .iter()
            .any(|&i| crate::episode::retryable(&records[i]))
        {
            continue;
        }
        group.sort_by_key(|&i| (start_us(&records[i]), records[i].id));
        // (failure, its retry, the wait between them), in time order
        let mut links: Vec<(usize, usize, i64)> = Vec::new();
        let mut free = 0;
        for at in 0..group.len() {
            let r = &records[group[at]];
            if !crate::episode::retryable(r) {
                continue;
            }
            let told = end_us(r);
            free = free.max(at + 1);
            while free < group.len() && start_us(&records[group[free]]) < told {
                free += 1;
            }
            if let Some(&next) = group.get(free) {
                let wait = start_us(&records[next]) - told;
                if wait <= RETRY_WINDOW_US {
                    links.push((at, free, wait));
                    free += 1;
                }
            }
        }
        // chains of failures each followed by the next: a steady one is a poll
        let mut i = 0;
        while i < links.len() {
            let mut j = i + 1;
            while j < links.len() && links[j].0 == links[j - 1].1 {
                j += 1;
            }
            let chain = &links[i..j];
            let mut waits: Vec<i64> = chain.iter().map(|l| l.2).collect();
            waits.sort_unstable();
            let mid = waits[waits.len() / 2];
            let poll = chain.len() >= 3
                && waits[0] >= POLL_MIN_US
                && (waits[waits.len() - 1] - waits[0]) * 10 <= mid;
            if !poll {
                for l in chain {
                    again[group[l.1]] = true;
                }
            }
            i = j;
        }
    }
    again
}

/// The smallest body whose transfer is timed against its size: under it the
/// time is the request's, not the bytes'.
pub const TRANSFER_MIN: u64 = 64 * 1024;

const MIB: f64 = 1024.0 * 1024.0;

/// S3's code in an error body: the XML's `<Code>`, or the word before the
/// colon of "Code: message".
fn s3_code(body: &str) -> Option<&str> {
    if let Some(i) = body.find("<Code>") {
        let rest = &body[i + 6..];
        return rest.find("</Code>").map(|e| &rest[..e]);
    }
    let code = body.split_once(':')?.0.trim();
    (!code.is_empty()
        && code.len() <= 64
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_'))
    .then_some(code)
}

/// A size's bucket: 0 for the empty body, then one per power of two.
fn size_bucket(n: u64) -> usize {
    if n == 0 {
        0
    } else {
        1 + (64 - (n - 1).leading_zeros() as usize)
    }
}

fn bucket_le(i: usize) -> u64 {
    if i == 0 { 0 } else { 1u64 << (i - 1).min(63) }
}

struct SizeHist {
    count: [usize; 66],
    bytes: [u64; 66],
}

impl SizeHist {
    fn new() -> Self {
        SizeHist {
            count: [0; 66],
            bytes: [0; 66],
        }
    }

    fn add(&mut self, n: u64) {
        let i = size_bucket(n);
        self.count[i] += 1;
        self.bytes[i] += n;
    }

    fn buckets(&self) -> Vec<SizeBucket> {
        let Some(first) = self.count.iter().position(|n| *n > 0) else {
            return Vec::new();
        };
        let last = self.count.iter().rposition(|n| *n > 0).unwrap_or(first);
        (first..=last)
            // the empty body is its own bar, not the low end of a run of empty ones
            .filter(|i| *i == first || self.count[*i] > 0 || *i > self.first_sized())
            .map(|i| SizeBucket {
                le_bytes: bucket_le(i),
                count: self.count[i],
                bytes: self.bytes[i],
            })
            .collect()
    }

    /// The first bucket with a body that has bytes.
    fn first_sized(&self) -> usize {
        self.count
            .iter()
            .skip(1)
            .position(|n| *n > 0)
            .map_or(usize::MAX, |i| i + 1)
    }
}

/// What a kind of request adds up to.
struct Op {
    class: &'static str,
    requests: usize,
    billed: usize,
    cost: f64,
    first_byte: Vec<f64>,
    rest: Vec<f64>,
}

fn class_name(r: &Record) -> &'static str {
    match class(r) {
        Class::Free => "free",
        Class::Write if r.operation.starts_with("List") => "list",
        Class::Write => "write",
        Class::Read => "read",
    }
}

/// [`percentile`] of values in any order: the same figure, without sorting
/// them all (they are left partly ordered).
fn quantile(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let i = ((v.len() as f64 * q).ceil() as usize).clamp(1, v.len()) - 1;
    *v.select_nth_unstable_by(i, f64::total_cmp).1
}

/// The median, p95 and p99 of values in any order: the highest first, and
/// each of the others among what is left under it.
fn bands(v: &mut [f64]) -> (f64, f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let at = |q: f64| ((v.len() as f64 * q).ceil() as usize).clamp(1, v.len()) - 1;
    let (i50, i95, i99) = (at(0.5), at(0.95), at(0.99));
    let p99 = *v.select_nth_unstable_by(i99, f64::total_cmp).1;
    let v = &mut v[..=i99];
    let p95 = *v.select_nth_unstable_by(i95, f64::total_cmp).1;
    let v = &mut v[..=i95];
    (*v.select_nth_unstable_by(i50, f64::total_cmp).1, p95, p99)
}

fn median(v: &mut [f64]) -> f64 {
    quantile(v, 0.5)
}

pub fn signals(records: &[Record], prices: &RequestPrices) -> Signals {
    let tl = Timeline::of(records);
    let bins = tl.map_or(0, |t| t.bins);
    let bin_ms = tl.map_or(1.0, |t| t.bin_ms);
    let bin_s = bin_ms / 1000.0;
    let t0 = tl.map_or(0, |t| t.t0_us);
    let wall_ms = tl.map_or(0.0, |t| t.wall_us as f64 / 1000.0);
    // a log without times (CloudTrail) says what was asked and when, not how long it took
    let timed = records.iter().any(|r| r.latency_ms > 0.0);

    let zeros = || vec![0f64; bins];
    let (mut n, mut up, mut down) = (zeros(), zeros(), zeros());
    let (mut client, mut server, mut transport, mut injected) =
        (zeros(), zeros(), zeros(), zeros());
    let mut retries = zeros();
    let mut took: Vec<Vec<f64>> = vec![Vec::new(); bins];
    let mut all: Vec<f64> = Vec::with_capacity(records.len());
    let mut counts = ErrorCounts::default();
    let (mut up_bytes, mut down_bytes, mut retries_total) = (0u64, 0u64, 0u64);
    let mut ops: BTreeMap<&str, Op> = BTreeMap::new();
    let (mut fb_sum, mut fb_of) = (0f64, 0f64);
    let (mut sent, mut received) = (SizeHist::new(), SizeHist::new());
    let mut moved: BTreeMap<(&str, usize), Vec<f64>> = BTreeMap::new();
    let mut moved_all: Vec<f64> = Vec::new();
    let mut statuses: BTreeMap<(Option<u16>, Option<&str>), StatusRow> = BTreeMap::new();
    let mut amount = 0f64;
    let mut spans: Vec<(i64, i64)> = Vec::new();
    let sent_again = retries_of(records);

    for (at, r) in records.iter().enumerate() {
        let Some(tl) = &tl else { break };
        let i = tl.bin_of(r);
        let s = start_us(r);
        let latency_ms = took_ms(r);
        n[i] += 1.0;
        took[i].push(latency_ms);
        all.push(latency_ms);

        let (u, d) = (r.request_bytes.unwrap_or(0), r.response_bytes.unwrap_or(0));
        up[i] += u as f64;
        down[i] += d as f64;
        up_bytes += u;
        down_bytes += d;
        if let Some(b) = r.request_bytes {
            sent.add(b);
        }
        if let Some(b) = r.response_bytes {
            received.add(b);
        }

        // a retry is counted where it is known: inside a record (sonde's own
        // client says how many it took), or as a record of its own, which is
        // how a program's SDK retries through the proxy: the same request
        // again after an answer that asks for it ([`retries_of`] has the rule)
        let again = u32::from(sent_again[at]);
        retries[i] += (r.retries + again) as f64;
        retries_total += (r.retries + again) as u64;
        if is_failure(r) {
            // what sonde did in the store's place is not the store's failure
            let (series, count) = if r.injected.is_some() {
                (&mut injected, &mut counts.injected)
            } else if r.status.is_none() || r.transport_error.is_some() {
                (&mut transport, &mut counts.transport)
            } else if r.status.is_some_and(|s| s >= 500) {
                (&mut server, &mut counts.server)
            } else {
                (&mut client, &mut counts.client)
            };
            series[i] += 1.0;
            *count += 1;
        }

        let cost = price(r, prices);
        // in the records' order, as the profile adds it up: the same figure
        amount += cost;
        let op = ops.entry(r.operation.as_str()).or_insert_with(|| Op {
            class: class_name(r),
            requests: 0,
            billed: 0,
            cost: 0.0,
            first_byte: Vec::new(),
            rest: Vec::new(),
        });
        op.requests += 1;
        op.billed += usize::from(reached_store(r));
        op.cost += cost;
        if let Some(fb) = r.first_byte_ms.filter(|_| timed) {
            let fb = if fb.is_finite() {
                fb.clamp(0.0, reqlog::MAX_LATENCY_MS)
            } else {
                0.0
            };
            let whole = latency_ms.max(fb);
            op.first_byte.push(fb);
            op.rest.push(whole - fb);
            fb_sum += fb;
            fb_of += whole;
        }

        // a transfer: the body the operation is about, when it arrived whole
        let body = match r.operation.as_str() {
            "GetObject" => r.response_bytes,
            "PutObject" | "UploadPart" => r.request_bytes,
            _ => None,
        };
        if let Some(b) = body.filter(|b| *b >= TRANSFER_MIN)
            && latency_ms > 0.0
            && !r.is_error()
        {
            let rate = b as f64 / MIB / (latency_ms / 1000.0);
            moved
                .entry((r.operation.as_str(), size_bucket(b)))
                .or_default()
                .push(rate);
            moved_all.push(rate);
        }

        let at = (s - t0) as f64 / 1000.0;
        let code = r.error_body.as_deref().and_then(s3_code);
        let row = statuses
            .entry((r.status, code))
            .or_insert_with(|| StatusRow {
                status: r.status,
                code: code.map(str::to_string),
                count: 0,
                injected: 0,
                first_ms: at,
                last_ms: at,
            });
        row.count += 1;
        row.injected += usize::from(r.injected.is_some());
        row.first_ms = row.first_ms.min(at);
        row.last_ms = row.last_ms.max(at);

        // a request that took no time is never in flight (the profile's rule)
        let e = end_us(r);
        if e > s {
            spans.push((s, e));
        }
    }

    // latency: the whole run's and each slice's, by the profile's definition
    let whole = bands(&mut all);
    let (whole50, whole95, whole99) = (
        timed.then_some(whole.0),
        timed.then_some(whole.1),
        timed.then_some(whole.2),
    );
    let (mut p50, mut p95, mut p99) = (zeros(), zeros(), zeros());
    for (i, l) in took.iter_mut().enumerate() {
        if l.is_empty() {
            continue;
        }
        (p50[i], p95[i], p99[i]) = bands(l);
    }

    // in flight: the requests in the order they started, the ends of those
    // still running kept by time (as many as are in flight, never all of
    // them); an end before a start at one instant. Each slice keeps the most
    // it saw, what ran into it included
    if !spans.is_sorted() {
        spans.sort_unstable();
    }
    let mut in_flight = zeros();
    let (mut level, mut peak, mut peak_at) = (0usize, 0usize, 0f64);
    // a slice is a whole number of ms: its edges are counted, not divided for
    let bin_us = (bin_ms * 1000.0).round() as i64;
    let mut cur = 0usize;
    let mut running: BinaryHeap<Reverse<i64>> = BinaryHeap::new();
    let mut reach = |t: i64, level: usize, in_flight: &mut Vec<f64>| {
        while cur + 1 < bins && t >= t0 + (cur as i64 + 1) * bin_us {
            cur += 1;
            in_flight[cur] = level as f64;
        }
        cur
    };
    for (s, e) in spans {
        while let Some(Reverse(end)) = running.peek().copied().filter(|end| end.0 <= s) {
            running.pop();
            reach(end, level, &mut in_flight);
            level -= 1;
        }
        let at = reach(s, level, &mut in_flight);
        level += 1;
        if level > peak {
            peak = level;
            peak_at = (s - t0) as f64 / 1000.0;
        }
        in_flight[at] = in_flight[at].max(level as f64);
        running.push(Reverse(e));
    }
    while let Some(Reverse(end)) = running.pop() {
        reach(end, level, &mut in_flight);
        level -= 1;
    }

    // a run too short to have a second in has totals, not rates
    let short = wall_ms < SHORT_MS;
    let seconds = (!short).then_some(wall_ms / 1000.0);
    let per_bin = |v: &[f64]| v.iter().map(|x| x / bin_s).collect::<Vec<f64>>();
    let failed = counts.client + counts.server + counts.transport + counts.injected;

    let fb_count: usize = ops.values().map(|o| o.first_byte.len()).sum();
    let mut cost_rows = Vec::with_capacity(ops.len());
    let mut kinds = Vec::new();
    for (name, mut o) in ops {
        cost_rows.push(CostRow {
            operation: name.to_string(),
            class: o.class,
            requests: o.requests,
            billed: o.billed,
            cost: o.cost,
            share: if amount > 0.0 { o.cost / amount } else { 0.0 },
        });
        if !o.first_byte.is_empty() {
            kinds.push(FirstByteRow {
                operation: name.to_string(),
                count: o.first_byte.len(),
                first_byte_ms: median(&mut o.first_byte),
                rest_ms: median(&mut o.rest),
            });
        }
    }
    cost_rows.sort_by(|a, b| {
        b.cost
            .total_cmp(&a.cost)
            .then(b.requests.cmp(&a.requests))
            .then(a.operation.cmp(&b.operation))
    });
    kinds.sort_by(|a, b| b.count.cmp(&a.count).then(a.operation.cmp(&b.operation)));

    let mut statuses: Vec<StatusRow> = statuses.into_values().collect();
    statuses.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.status.cmp(&b.status))
            .then(a.code.cmp(&b.code))
    });

    let prefixes = if records.is_empty() {
        Vec::new()
    } else {
        prefix_stats(records, prices)
            .1
            .into_iter()
            .map(|p| PrefixRate {
                read_share: p.peak_reads as f64 / READS_PER_PREFIX as f64,
                write_share: p.peak_writes as f64 / WRITES_PER_PREFIX as f64,
                prefix: p.prefix,
                folders: p.folders,
                requests: p.requests,
                peak_reads: p.peak_reads,
                peak_writes: p.peak_writes,
            })
            .collect()
    };

    Signals {
        timed,
        wall_ms,
        short,
        bin_ms,
        bins,
        rate: Rate {
            value: seconds.map(|s| records.len() as f64 / s),
            unit: "req/s",
            series: per_bin(&n),
            requests: records.len(),
        },
        errors: Errors {
            value: (!records.is_empty()).then(|| failed as f64 / records.len() as f64),
            unit: "share",
            series: (0..bins)
                .map(|i| client[i] + server[i] + transport[i] + injected[i])
                .collect(),
            count: failed,
            counts,
            client,
            server,
            transport,
            injected,
            retries,
            retries_total,
        },
        latency: Latency {
            value: whole50,
            unit: "ms",
            series: if timed { p50 } else { Vec::new() },
            p50_ms: whole50,
            p95_ms: whole95,
            p99_ms: whole99,
            p95: if timed { p95 } else { Vec::new() },
            p99: if timed { p99 } else { Vec::new() },
        },
        bytes: Bytes {
            value: seconds.map(|s| (up_bytes + down_bytes) as f64 / s),
            unit: "B/s",
            series: (0..bins).map(|i| (up[i] + down[i]) / bin_s).collect(),
            up_per_sec: seconds.map(|s| up_bytes as f64 / s),
            down_per_sec: seconds.map(|s| down_bytes as f64 / s),
            up: per_bin(&up),
            down: per_bin(&down),
            up_bytes,
            down_bytes,
        },
        in_flight: InFlight {
            value: timed.then_some(peak as f64),
            unit: "requests",
            series: in_flight,
            peak,
            peak_at_ms: peak_at,
        },
        first_byte: (fb_count > 0).then(|| FirstByte {
            share: if fb_of > 0.0 { fb_sum / fb_of } else { 0.0 },
            count: fb_count,
            kinds,
        }),
        sizes: Sizes {
            request: sent.buckets(),
            response: received.buckets(),
            throughput: moved
                .into_iter()
                .map(|((operation, bucket), mut v)| Throughput {
                    operation: operation.to_string(),
                    bucket: bucket_le(bucket),
                    count: v.len(),
                    mib_per_sec: median(&mut v),
                })
                .collect(),
            mib_per_sec: (!moved_all.is_empty()).then(|| median(&mut moved_all)),
        },
        statuses,
        cost: CostBy {
            amount,
            currency: prices.currency.clone(),
            operations: cost_rows,
        },
        prefixes,
    }
}

const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// The width of a signal's line in the terminal.
pub const SPARK_COLUMNS: usize = 32;

/// What a column of a sparkline is of the slices it stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    /// Their mean: a rate.
    Mean,
    /// The most of them: a burst or a tail is not averaged away.
    Max,
}

/// A series as bars, [`Column::Max`] of the values each stands for.
pub fn sparkline(series: &[f64], columns: usize) -> String {
    sparkline_of(series, columns, Column::Max)
}

/// A series as `columns` bars at most: a series of fewer values is as wide
/// as it is (a bar for each, none stretched), a longer one has `how` of the
/// values each column stands for. Nothing is the lowest bar, anything at
/// all is above it.
pub fn sparkline_of(series: &[f64], columns: usize, how: Column) -> String {
    let len = series.len();
    if len == 0 || columns == 0 {
        return String::new();
    }
    let columns = columns.min(len);
    let drawn: Vec<f64> = (0..columns)
        .map(|c| {
            let a = c * len / columns;
            let b = ((c + 1) * len).div_ceil(columns).clamp(a + 1, len);
            let part = &series[a..b];
            match how {
                Column::Max => part.iter().copied().fold(0.0, f64::max),
                Column::Mean => part.iter().sum::<f64>() / part.len() as f64,
            }
        })
        .collect();
    let max = drawn.iter().copied().fold(0.0, f64::max);
    drawn
        .iter()
        .map(|v| {
            if *v <= 0.0 || max <= 0.0 {
                BARS[0]
            } else {
                BARS[1 + ((v / max * 7.0) as usize).min(6)]
            }
        })
        .collect()
}

fn paint(s: &str, code: &str, color: bool) -> String {
    if color {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

/// A length of time as sonde writes it: "0.4 ms", "12 ms", "1.5 s",
/// "2m 26s".
pub fn ms_text(v: f64) -> String {
    if v >= 60_000.0 {
        format!(
            "{}m {:02}s",
            (v / 60_000.0) as u64,
            (v % 60_000.0 / 1000.0) as u64
        )
    } else if v >= 1000.0 {
        format!("{:.1} s", v / 1000.0)
    } else if v >= 0.95 {
        format!("{v:.0} ms")
    } else {
        format!("{v:.1} ms")
    }
}

/// A count of bytes as sonde writes it, on the one ladder: "0 B",
/// "1.5 KiB", up to EiB (what a count of bytes can reach).
pub fn bytes_text(b: f64) -> String {
    // the one ladder: as far as a count of bytes goes
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let (mut v, mut i) = (b.max(0.0), 0);
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{v:.0} B")
    } else {
        format!("{v:.1} {}", units[i])
    }
}

/// 1234567 as "1,234,567".
pub fn count_text(n: usize) -> String {
    let d = n.to_string();
    let mut out = String::with_capacity(d.len() + d.len() / 3);
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "1 request", "1,234 requests".
pub fn requests_text(n: usize) -> String {
    format!("{} request{}", count_text(n), if n == 1 { "" } else { "s" })
}

fn rate_text(v: f64) -> String {
    if v >= 100.0 {
        count_text(v.round() as usize)
    } else if v >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

fn percent(share: f64) -> String {
    let p = share * 100.0;
    if p > 0.0 && p < 0.1 {
        "<0.1%".into()
    } else if p < 10.0 && p.fract() != 0.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

/// The rows of each table `text` prints at most.
const TABLE_ROWS: usize = 4;
const PREFIX_ROWS: usize = 3;

/// The signals for a terminal: a line each for rate, errors, latency and
/// bytes with its series as a sparkline, then what is there to say of the
/// first byte, the cost by operation and the prefixes past half of S3's limit.
///
/// It follows a head line that has the requests, the time they took, the
/// failures and the lookups that found nothing: none of those is said again
/// here. This is the one place for the bytes, the most in flight and the
/// cost.
pub fn text(s: &Signals, color: bool) -> String {
    let mut out = String::new();
    if s.rate.requests == 0 {
        return out;
    }
    // a run under a second, or of a handful of requests, has its figures
    // and no line over its time
    let drawn = !s.short && s.rate.requests >= FEW;
    // the figures in one column, the lines under one another
    let mut line = |label: &str,
                    figure: String,
                    tone: Option<&str>,
                    series: &[f64],
                    how: Column,
                    rest: Vec<String>| {
        // (a long figure still stands apart from what follows it)
        let pad = " ".repeat(30usize.saturating_sub(figure.chars().count()).max(2));
        let figure = match tone {
            Some(t) => paint(&figure, t, color),
            None => figure,
        };
        // as wide as its slices, and the lines after it still in a column
        let (spark, gap) = if drawn {
            let spark = sparkline_of(series, SPARK_COLUMNS, how);
            let gap = " ".repeat(SPARK_COLUMNS - spark.chars().count());
            (spark, gap)
        } else {
            (String::new(), String::new())
        };
        let row = format!(
            "{}{figure}{pad}{spark}{gap}  {}",
            paint(&format!("{label:<9}"), "2", color),
            rest.join(" · ")
        );
        out.push_str(row.trim_end());
        out.push('\n');
    };
    // what a bar is, said when it stands for more than one slice
    let wide = s.bins > SPARK_COLUMNS;
    let bar = |what: &str| (drawn && wide).then(|| format!("a bar is {what}"));

    let mut rest: Vec<String> = Vec::new();
    if s.in_flight.peak > 1 {
        rest.push(format!("{} at once at most", count_text(s.in_flight.peak)));
    }
    rest.extend(bar("the mean of its slices"));
    line(
        "rate",
        match s.rate.value {
            Some(v) => format!("{} req/s", rate_text(v)),
            // (the head line has the requests and the time)
            None => "no rate: under a second".into(),
        },
        None,
        &s.rate.series,
        Column::Mean,
        rest,
    );

    let c = &s.errors.counts;
    let mut kinds: Vec<String> = [
        (c.client, "client"),
        (c.server, "server"),
        (c.transport, "no answer"),
        (c.injected, "injected"),
    ]
    .iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, what)| format!("{} {what}", count_text(*n)))
    .collect();
    if s.errors.retries_total > 0 {
        kinds.push(format!(
            "{} {}",
            count_text(s.errors.retries_total as usize),
            if s.errors.retries_total == 1 {
                "retry"
            } else {
                "retries"
            }
        ));
    }
    let mut rest = vec![kinds.join(", ")];
    rest.retain(|k| !k.is_empty());
    if s.errors.count > 0 {
        rest.extend(bar("the most in its slices"));
    }
    line(
        "errors",
        if s.errors.count == 0 {
            "none".into()
        } else {
            format!(
                "{} ({})",
                percent(s.errors.value.unwrap_or(0.0)),
                count_text(s.errors.count)
            )
        },
        (s.errors.count > 0).then_some("31"),
        // no failure, no line of none
        if s.errors.count == 0 {
            &[]
        } else {
            &s.errors.series
        },
        Column::Max,
        rest,
    );

    match (s.latency.p50_ms, s.latency.p95_ms, s.latency.p99_ms) {
        (Some(p50), Some(p95), Some(p99)) => line(
            "latency",
            format!("p50 {}", ms_text(p50)),
            None,
            // the tail is what moves
            &s.latency.p95,
            Column::Max,
            [format!("p95 {}, p99 {}", ms_text(p95), ms_text(p99))]
                .into_iter()
                .chain(drawn.then(|| {
                    if wide {
                        "a bar is the highest p95 of its slices".into()
                    } else {
                        "a bar is a slice's p95".into()
                    }
                }))
                .collect(),
        ),
        // a log without times
        _ => line(
            "latency",
            "not recorded".into(),
            None,
            &[],
            Column::Max,
            Vec::new(),
        ),
    }

    // the one place for the bytes: every body, an error's answer too
    let totals = format!(
        "{} sent, {} received (error bodies too)",
        bytes_text(s.bytes.up_bytes as f64),
        bytes_text(s.bytes.down_bytes as f64)
    );
    let mut rest = Vec::new();
    let figure = match (s.bytes.up_per_sec, s.bytes.down_per_sec) {
        (Some(u), Some(d)) => {
            rest.push(totals);
            format!("↑ {}/s ↓ {}/s", bytes_text(u), bytes_text(d))
        }
        _ => {
            rest.push("error bodies too".into());
            format!(
                "{} sent, {} received",
                bytes_text(s.bytes.up_bytes as f64),
                bytes_text(s.bytes.down_bytes as f64)
            )
        }
    };
    rest.extend(bar("the mean of its slices"));
    line("bytes", figure, None, &s.bytes.series, Column::Mean, rest);

    let heading = |t: String| format!("{}\n", paint(&t, "1", color));
    let width = |names: &mut dyn Iterator<Item = &str>| {
        names
            .map(|n| n.chars().count())
            .max()
            .unwrap_or(0)
            .clamp("operation".len(), 40)
    };
    let more = |left: usize| {
        if left == 0 {
            String::new()
        } else {
            format!("  … and {} more\n", count_text(left))
        }
    };

    if let Some(fb) = &s.first_byte {
        let rows = &fb.kinds[..fb.kinds.len().min(TABLE_ROWS)];
        let w = width(&mut rows.iter().map(|k| k.operation.as_str()));
        out.push_str(&heading(format!(
            "first byte  {} of the time is the wait for it",
            percent(fb.share)
        )));
        out.push_str(&paint(
            &format!(
                "  {:<w$}  {:>9}  {:>10}  {}\n",
                "operation", "requests", "first byte", "then the rest"
            ),
            "2",
            color,
        ));
        for k in rows {
            out.push_str(&format!(
                "  {:<w$}  {:>9}  {:>10}  {}\n",
                k.operation,
                count_text(k.count),
                ms_text(k.first_byte_ms),
                ms_text(k.rest_ms),
            ));
        }
        out.push_str(&more(fb.kinds.len() - rows.len()));
    }

    let all_paid = s.cost.operations.iter().filter(|r| r.cost > 0.0).count();
    let paid: Vec<&CostRow> = s
        .cost
        .operations
        .iter()
        .filter(|r| r.cost > 0.0)
        .take(TABLE_ROWS)
        .collect();
    if !paid.is_empty() {
        let w = width(&mut paid.iter().map(|r| r.operation.as_str()));
        out.push_str(&heading(format!(
            "cost  {} for its requests",
            crate::program::money_text(s.cost.amount, &s.cost.currency)
        )));
        out.push_str(&paint(
            &format!(
                "  {:<w$}  {:>9}  {:>5}  {}\n",
                "operation", "requests", "share", "cost"
            ),
            "2",
            color,
        ));
        for r in &paid {
            out.push_str(&format!(
                "  {:<w$}  {:>9}  {:>5}  {}\n",
                r.operation,
                count_text(r.requests),
                percent(r.share),
                crate::program::money_text(r.cost, &s.cost.currency),
            ));
        }
        out.push_str(&more(all_paid - paid.len()));
    }

    let mut near: Vec<&PrefixRate> = s
        .prefixes
        .iter()
        .filter(|p| p.read_share >= 0.5 || p.write_share >= 0.5)
        .collect();
    near.sort_by(|a, b| {
        b.read_share
            .max(b.write_share)
            .total_cmp(&a.read_share.max(a.write_share))
    });
    near.truncate(PREFIX_ROWS);
    if !near.is_empty() {
        let w = width(&mut near.iter().map(|p| p.prefix.as_str()));
        out.push_str(&heading(format!(
            "prefixes past half of S3's limit ({} reads, {} writes a second)",
            count_text(READS_PER_PREFIX),
            count_text(WRITES_PER_PREFIX)
        )));
        for p in near {
            out.push_str(&format!(
                "  {:<w$}  {:>6} reads/s {:>5}  {:>6} writes/s {:>5}\n",
                p.prefix,
                count_text(p.peak_reads),
                percent(p.read_share),
                count_text(p.peak_writes),
                percent(p.write_share),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use reqlog::Header;

    fn get(id: u64, at_ms: i64, key: &str, status: u16) -> Record {
        Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
            method: "GET".into(),
            operation: "GetObject".into(),
            bucket: Some("b".into()),
            key: Some(key.into()),
            status: Some(status),
            latency_ms: 5.0,
            ..Default::default()
        }
    }

    fn retries(records: &[Record]) -> u64 {
        signals(records, &RequestPrices::default())
            .errors
            .retries_total
    }

    #[test]
    fn a_retry_follows_its_failure_within_the_window_and_in_time_order() {
        // a 503, and the same read an hour later: not its retry
        assert_eq!(
            retries(&[get(1, 0, "k", 503), get(2, 3_600_000, "k", 200)]),
            0
        );
        // within the window it is
        assert_eq!(retries(&[get(1, 0, "k", 503), get(2, 59_000, "k", 200)]), 1);
        // a 408 asks for a retry as a 503 does
        assert_eq!(retries(&[get(1, 0, "k", 408), get(2, 100, "k", 200)]), 1);
        // a 501 does not
        assert_eq!(retries(&[get(1, 0, "k", 501), get(2, 100, "k", 200)]), 0);
        // the file's order is not the order of time: the read listed first
        // came after the failure
        assert_eq!(retries(&[get(2, 200, "k", 200), get(1, 0, "k", 503)]), 1);
        // and one that came before it is not its retry
        assert_eq!(retries(&[get(2, 200, "k", 503), get(1, 0, "k", 200)]), 0);
        // a request in flight when the failure was answered is another sender's
        let mut slow = get(2, 2, "k", 200);
        slow.latency_ms = 50.0;
        assert_eq!(retries(&[get(1, 0, "k", 503), slow]), 0);
    }

    #[test]
    fn another_range_of_the_object_is_another_request() {
        let ranged = |id, at, range: &str, status| {
            let mut r = get(id, at, "k", status);
            r.request_headers.push(Header::new("range", range));
            r
        };
        assert_eq!(
            retries(&[
                ranged(1, 0, "bytes=0-9", 503),
                ranged(2, 10, "bytes=10-19", 206),
                ranged(3, 20, "bytes=20-29", 206),
            ]),
            0
        );
        assert_eq!(
            retries(&[
                ranged(1, 0, "bytes=0-9", 503),
                ranged(2, 10, "bytes=10-19", 206),
                ranged(3, 20, "bytes=0-9", 206),
            ]),
            1
        );
    }

    #[test]
    fn a_poll_of_a_key_that_keeps_failing_is_not_a_run_of_retries() {
        // five polls a second apart, each answered 503
        let poll: Vec<Record> = (0..5)
            .map(|i| get(i + 1, i as i64 * 1000, "job", 503))
            .collect();
        assert_eq!(retries(&poll), 0);
        // an SDK backing off: the waits grow
        let backoff = [
            get(1, 0, "job", 503),
            get(2, 300, "job", 503),
            get(3, 900, "job", 503),
            get(4, 2100, "job", 503),
            get(5, 4500, "job", 200),
        ];
        assert_eq!(retries(&backoff), 4);
        // two failures and the answer: too few to call a poll
        assert_eq!(
            retries(&[
                get(1, 0, "job", 503),
                get(2, 1000, "job", 503),
                get(3, 2000, "job", 200)
            ]),
            2
        );
        // attempts in quick step are a fixed backoff, not a poll
        let quick: Vec<Record> = (0..5)
            .map(|i| get(i + 1, i as i64 * 20, "job", 503))
            .collect();
        assert_eq!(retries(&quick), 4);
    }

    #[test]
    fn a_run_under_a_second_has_totals_and_no_rate() {
        let mut one = get(1, 0, "k", 200);
        one.latency_ms = 2.0;
        one.response_bytes = Some(10);
        let s = signals(std::slice::from_ref(&one), &RequestPrices::default());
        assert!(s.short);
        assert_eq!(
            (s.rate.value, s.bytes.value, s.bytes.down_per_sec),
            (None, None, None)
        );
        assert_eq!(s.rate.requests, 1);
        let said = text(&s, false);
        let rate = said.lines().next().unwrap();
        // the head line has the requests and the time: not said again
        assert_eq!(rate, "rate     no rate: under a second", "{said}");
        assert!(
            !said.contains("req/s") && !said.contains("1 request"),
            "{said}"
        );
        assert!(said.contains("0 B sent, 10 B received"), "{said}");
        let spark = |said: &str| said.chars().any(|c| ('▁'..='█').contains(&c));
        // under a second: no line over its time
        assert!(!spark(&said) && !said.contains("a bar is"), "{said}");
        // a run of a second and more has its rate
        let long = [get(1, 0, "k", 200), get(2, 1995, "k", 200)];
        let s = signals(&long, &RequestPrices::default());
        assert!(!s.short);
        assert_eq!(s.rate.value, Some(1.0));
        let said = text(&s, false);
        assert!(said.starts_with("rate     1.00 req/s"), "{said}");
        assert!(
            !said.contains("2 requests") && !said.contains("2.0 s"),
            "{said}"
        );
        // no failure: no line drawn for none
        let errors = said.lines().nth(1).unwrap();
        assert_eq!(errors, "errors   none");
        // two requests: no shape over the time to draw either
        assert!(!spark(&said) && !said.contains("a bar is"), "{said}");
        // twenty over two seconds have one
        let many: Vec<Record> = (0..FEW as u64)
            .map(|i| get(i + 1, i as i64 * 100, "k", 200))
            .collect();
        let said = text(&signals(&many, &RequestPrices::default()), false);
        assert!(spark(&said), "{said}");
        assert!(
            said.contains("a bar is the highest p95 of its slices"),
            "{said}"
        );
        let few = &many[..FEW - 1];
        let said = text(&signals(few, &RequestPrices::default()), false);
        assert!(!spark(&said), "{said}");
    }

    #[test]
    fn a_latency_no_request_can_have_is_clamped_everywhere_alike() {
        let mut neg = get(1, 0, "k", 200);
        neg.latency_ms = -5.0;
        let mut absurd = get(2, 10, "k", 200);
        absurd.latency_ms = 1e12;
        let ok = get(3, 20, "k", 200);
        let s = signals(&[neg, absurd, ok], &RequestPrices::default());
        // a day at most, in the figures as on the timeline
        assert_eq!(s.latency.p99_ms, Some(reqlog::MAX_LATENCY_MS));
        assert_eq!(s.latency.p50_ms, Some(5.0));
        let slice_max = s.latency.p99.iter().copied().fold(0.0, f64::max);
        assert_eq!(slice_max, reqlog::MAX_LATENCY_MS);
        assert!(s.latency.series.iter().all(|v| *v >= 0.0));
    }

    #[test]
    fn a_sparkline_is_as_wide_as_its_slices_and_a_bar_is_their_mean_or_their_most() {
        // fewer values than columns: a bar each, none stretched
        assert_eq!(sparkline_of(&[0.0, 8.0], 4, Column::Mean), "▁█");
        assert_eq!(sparkline(&[0.0, 8.0, 4.0], 32), "▁█▅");
        // more: the mean for a rate, the most for a tail
        let series = [0.0, 8.0, 4.0, 4.0];
        assert_eq!(sparkline_of(&series, 2, Column::Mean), "██");
        assert_eq!(sparkline_of(&series, 2, Column::Max), "█▅");
    }

    #[test]
    fn the_tables_have_headings_and_say_what_they_left_out() {
        let ops = [
            "GetObject",
            "PutObject",
            "HeadObject",
            "DeleteObject",
            "CopyObject",
            "ListObjectsV2",
        ];
        let records: Vec<Record> = ops
            .iter()
            .enumerate()
            .map(|(i, op)| {
                let mut r = get(i as u64, i as i64 * 400, "k", 200);
                r.operation = (*op).into();
                r.method = if *op == "GetObject" { "GET" } else { "PUT" }.into();
                r.first_byte_ms = Some(2.0);
                r
            })
            .collect();
        let said = text(&signals(&records, &RequestPrices::default()), false);
        assert!(
            said.contains("  operation      requests  first byte  then the rest\n"),
            "{said}"
        );
        assert!(said.contains("  … and 2 more\n"), "{said}");
        assert!(said.contains("requests  share  cost\n"), "{said}");
    }

    #[test]
    fn bytes_are_written_on_one_ladder() {
        assert_eq!(crate::episode::human_bytes(2048), "2.0 KiB");
        assert_eq!(crate::episode::human_bytes(5 << 50), "5.0 PiB");
        assert_eq!(bytes_text(0.0), "0 B");
        assert_eq!(bytes_text(1536.0), "1.5 KiB");
        assert_eq!(bytes_text(600_000.0 * 1024f64.powi(4)), "585.9 PiB");
        assert_eq!(bytes_text(3.0 * 1024f64.powi(6)), "3.0 EiB");
    }
}
