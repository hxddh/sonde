//! Where a run's time went and what its requests cost: the time a
//! program spent waiting on its store, by kind of request; how many requests
//! were in flight; the rate of requests over the run by outcome; an
//! estimate of the requests' price. And two runs of one program compared.
//!
//! Times are wall-clock: a kind's time is the union of its requests'
//! intervals (ten requests at once for 50 ms are 50 ms of waiting, not 500).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use reqlog::Record;
use serde::Serialize;

use crate::episode::Observation;
use crate::summary::is_miss;

/// A run's prices: its requests, per thousand (S3's classes: writes and
/// listings, reads; deletes are free), its bytes out, per GiB, and the
/// storage it leaves, per GiB-month.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPrices {
    pub per_1000_writes: f64,
    pub per_1000_reads: f64,
    pub currency: String,
    /// Lists, when they cost other than a write (B2's class C).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_1000_lists: Option<f64>,
    /// Bytes sent out of the store, per GiB, when they leave its region.
    #[serde(default = "out_per_gib")]
    pub per_gib_out: f64,
    /// Whether they do: `Some(true)` read from outside the region (egress,
    /// charged), `Some(false)` read in it (free), `None` not known: not
    /// charged, and said as what it would be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress: Option<bool>,
    /// Storage of what a run leaves, per GiB-month.
    #[serde(default = "stored_per_gib_month")]
    pub per_gib_month: f64,
}

fn out_per_gib() -> f64 {
    0.09
}

fn stored_per_gib_month() -> f64 {
    0.023
}

impl Default for RequestPrices {
    /// S3 Standard in us-east-1, egress not known.
    fn default() -> Self {
        RequestPrices {
            per_1000_writes: 0.005,
            per_1000_reads: 0.0004,
            currency: "USD".into(),
            per_1000_lists: None,
            per_gib_out: out_per_gib(),
            egress: None,
            per_gib_month: stored_per_gib_month(),
        }
    }
}

impl RequestPrices {
    /// What is said of bytes out: [`Egress::Charged`] when they leave the
    /// region and cost something, [`Egress::Free`] when they do not (read
    /// in the region, or a store that charges none), else not known.
    pub fn egress(&self) -> Egress {
        match self.egress {
            _ if self.per_gib_out <= 0.0 => Egress::Free,
            Some(true) => Egress::Charged,
            Some(false) => Egress::Free,
            None => Egress::Unknown,
        }
    }

    /// The price of `bytes` out at egress's price, charged or not.
    pub fn out(&self, bytes: u64) -> f64 {
        bytes as f64 / GIB * self.per_gib_out
    }

    /// The price of `bytes` stored a month.
    pub fn stored(&self, bytes: u64) -> f64 {
        bytes as f64 / GIB * self.per_gib_month
    }
}

const GIB: f64 = (1u64 << 30) as f64;

/// Whether a run's bytes out are on its bill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Egress {
    /// Read from outside the store's region: charged.
    Charged,
    /// Read in its region, or a store with no egress fee.
    Free,
    /// Where it was read from is not known: not charged, said as "egress if
    /// read from outside the region".
    Unknown,
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
    let op = r.kind.as_str();
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
    pub kind: String,
    /// GET or HEAD answered 404.
    pub missed: bool,
    pub requests: usize,
    /// Wall-clock time with one of them in flight.
    pub waiting_ms: f64,
    /// Their latencies added up.
    pub sum_ms: f64,
    /// The program's waiting that was theirs: each moment's waiting split
    /// evenly among the requests then in flight, so the kinds add up to the
    /// run's waiting.
    pub waited_ms: f64,
    /// `waited_ms` over the run's waiting (the kinds add up to 1).
    pub share: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    pub errors: usize,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub cost: f64,
}

/// Requests started in one slice of the run, by outcome.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bin {
    /// The slice's start, from the run's start.
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
    /// Bytes sent and received by them.
    pub bytes_up: u64,
    pub bytes_down: u64,
    /// What they cost as requests, what their bytes out cost as egress
    /// (none when free; when not known, what it would be), and what the
    /// bytes they wrote and the run left stored cost a month.
    pub requests_cost: f64,
    pub transfer_cost: f64,
    pub storage_cost: f64,
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
pub(crate) fn throttled(r: &Record) -> bool {
    matches!(r.status, Some(503 | 429))
        || r.rare()
            .error_body
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

/// How many requests that started in one slice took about as long: a cell
/// of the latency's grid over time, counted over every request.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyCell {
    /// The slice, by index into `rate`.
    pub bin: usize,
    /// The latency's bucket on a log scale, [`HEAT_PER_DECADE`] to a decade:
    /// it holds the requests over `10^((bucket - 1) / 3)` ms and up to
    /// `10^(bucket / 3)` ms (bucket 0: up to 1 ms; the fastest, under 0.1 ms,
    /// are in bucket -3).
    pub bucket: i32,
    pub n: usize,
}

/// How many buckets of [`LatencyCell`] a decade of latency is cut in (each
/// about 2.15× the last).
pub const HEAT_PER_DECADE: f64 = 3.0;

/// The bucket of [`LatencyCell`] a latency falls in.
pub fn latency_bucket(ms: f64) -> i32 {
    (ms.max(0.1).log10() * HEAT_PER_DECADE - 1e-9).ceil() as i32
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

/// What a run cost: its requests, its bytes out (charged when they leave
/// the region) and, apart, what what it left stored costs a month.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub writes: usize,
    pub reads: usize,
    pub free: usize,
    /// The run's money: its requests, and its bytes out when charged
    /// ([`Transfer::egress`]). Storage is a month's, apart.
    pub amount: f64,
    pub currency: String,
    /// Of `amount`, the requests'.
    pub requests: f64,
    pub transfer: Transfer,
    pub storage: Storage,
}

/// The bytes a run read out of the store.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    /// Bytes the store sent (its answers' bodies).
    pub bytes: u64,
    /// Whether they are on the bill: `charged` (read from outside the
    /// region), `free` (read in it, or a store with no egress fee),
    /// `unknown` (not counted: `amount` is what it would be).
    pub egress: Egress,
    /// Their price as egress (in [`Cost::amount`] when `charged`).
    pub amount: f64,
    pub per_gib: f64,
}

/// What a run left stored, a month.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    /// Bytes it wrote and did not delete.
    pub bytes: u64,
    /// Their price a month (S3 Standard's class, or the settings').
    pub per_month: f64,
    /// A write's answer said the bucket keeps versions.
    pub versioned: bool,
    /// Versions its overwrites and deletes made noncurrent there …
    pub noncurrent: usize,
    /// … the bytes of those whose size it saw, and their price a month
    /// (until a lifecycle rule expires them).
    pub noncurrent_bytes: u64,
    pub noncurrent_per_month: f64,
}

impl Cost {
    /// What `show` and `run` say of the money past the requests: the bytes
    /// out (charged, or what they would be if read from outside the region)
    /// and what the run left stored, a month; none when neither comes to a
    /// cent and no version kept costs a cent a month.
    pub fn line(&self) -> Option<String> {
        use crate::program::{CENT, money_text};
        let money = |v: f64| money_text(v, &self.currency);
        let size = sonde_fmt::bytes;
        let mut parts = Vec::new();
        let t = &self.transfer;
        if t.bytes > 0 && t.amount >= CENT {
            parts.push(match t.egress {
                Egress::Charged => format!(
                    "requests ≈ {} · read {} ≈ {} egress",
                    money(self.requests),
                    size(t.bytes),
                    money(t.amount)
                ),
                _ => format!(
                    "read {} ≈ {} egress if read from outside the region (not counted: set the endpoint's `egress` to say)",
                    size(t.bytes),
                    money(t.amount)
                ),
            });
        }
        let st = &self.storage;
        if st.bytes > 0 && st.per_month >= CENT {
            parts.push(format!(
                "left {} stored ≈ {} a month",
                size(st.bytes),
                money(st.per_month)
            ));
        }
        // (versions of a known size under a cent a month: nothing to act on)
        let worth = st.noncurrent_bytes == 0 || st.noncurrent_per_month >= CENT;
        if st.versioned && st.noncurrent > 0 && worth {
            let s = if st.noncurrent == 1 { "" } else { "s" };
            parts.push(if st.noncurrent_bytes > 0 {
                format!(
                    "the bucket keeps versions: {} noncurrent version{s} of {} ≈ {} a month until a lifecycle rule expires them",
                    st.noncurrent,
                    size(st.noncurrent_bytes),
                    money(st.noncurrent_per_month)
                )
            } else {
                format!(
                    "the bucket keeps versions: {} noncurrent version{s} stay billed until a lifecycle rule expires them",
                    st.noncurrent
                )
            });
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    /// A run that cost nothing, at `prices`.
    pub fn none(prices: &RequestPrices) -> Cost {
        Cost {
            writes: 0,
            reads: 0,
            free: 0,
            amount: 0.0,
            currency: prices.currency.clone(),
            requests: 0.0,
            transfer: Transfer {
                bytes: 0,
                egress: prices.egress(),
                amount: 0.0,
                per_gib: prices.per_gib_out,
            },
            storage: Storage {
                bytes: 0,
                per_month: 0.0,
                versioned: false,
                noncurrent: 0,
                noncurrent_bytes: 0,
                noncurrent_per_month: 0.0,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Where its first slice begins, from the run's start: 0 for a
    /// run, a stretch's own start for a stretch of it. Every time below
    /// (`rate`, `inFlight`, `samples`, `chains`, `peakAtMs`) is from the
    /// run's start too, so a stretch's charts draw on the run's axis.
    pub start_ms: f64,
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
    /// Requests per prefix (by index) and slice of the run (by `rate`'s index).
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
    /// The latencies over time, every request counted (not a sample).
    pub latency_heat: Vec<LatencyCell>,
    /// The places in the program's code that sent requests, most waiting
    /// first: the busiest [`CALLERS`].
    pub callers: Vec<CallerStat>,
    /// Whether more places sent requests than `callers` keeps: then there
    /// were more than [`CALLERS`] (`12+`), else exactly as many as it has.
    pub callers_cut: bool,
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
    /// How many of `ids` each waits for, one after another (0: none; a
    /// read of a key one wrote, a write of a key one read, a listing of
    /// where one wrote waits for that one, one more than it): those of a
    /// level go at once, a level after the one before ([`at_once_ms`]).
    #[serde(skip)]
    pub levels: Vec<u32>,
}

/// The most places in the program's code a profile keeps ([`Profile::callers`]).
pub const CALLERS: usize = 12;

/// The requests at once a chain is weighed with: what SDKs default to, or near.
pub const CHAIN_CONCURRENCY: usize = 8;
/// The fewest requests a chain is told of.
pub const CHAIN_MIN: usize = 10;
/// The longest wait between two requests of a chain (the program's own
/// time): why 10 ms is D-277.
const CHAIN_GAP_US: i64 = 10_000;

/// What the requests of a chain touched, and the level each is at
/// ([`Chain::levels`]): whether the next one depends on one of them (a read
/// or a write of a key one of them wrote, a write of a key one read, a
/// listing of a prefix one wrote under, a write under a prefix one listed,
/// a listing's next page, an upload's parts and its end), and so waits for
/// it. Requests that depend on each other cannot be sent at once without
/// changing what the program does; those that depend on the same one can,
/// after it (a listing then uploads under it: the listing, then the uploads
/// at once; an upload's parts, after it began), and the others of the chain
/// around them (a manifest read and written back each time round a loop
/// holds back its own requests, not the loop's other reads).
#[derive(Default)]
struct Depends<'a> {
    /// Each key's levels: of its last write, of its last request (a copy's
    /// source by its name decoded).
    keys: std::collections::HashMap<(&'a str, Cow<'a, str>), (Option<u32>, u32)>,
    written: std::collections::BTreeMap<(&'a str, &'a str), u32>,
    listed: std::collections::HashMap<(&'a str, &'a str), u32>,
    /// The highest level of a write, as a listing past [`Depends::SCAN`]
    /// keys under its prefix waits for.
    wrote: Option<u32>,
    /// Each upload in parts, by its id: the level its parts are at.
    parts: std::collections::HashMap<&'a str, u32>,
}

impl<'a> Depends<'a> {
    /// The most keys under a prefix a listing's level is looked up in.
    const SCAN: usize = 1024;

    fn listing(r: &'a Record) -> Option<(&'a str, &'a str)> {
        r.kind.starts_with("ListObjects").then(|| {
            let prefix = crate::episode::query(r, "prefix").unwrap_or("");
            (r.bucket.as_deref().unwrap_or(""), prefix)
        })
    }

    /// Whether a listing is a page after the first: it carries where the
    /// page before ended (V2's token or start-after, ListObjects' marker).
    fn next_page(r: &Record) -> bool {
        ["continuation-token", "marker", "start-after"]
            .iter()
            .any(|q| crate::episode::query(r, q).is_some_and(|v| !v.is_empty()))
    }

    /// The upload a part is of: its id.
    fn part(r: &'a Record) -> Option<&'a str> {
        r.kind
            .starts_with("UploadPart")
            .then(|| crate::episode::query(r, "uploadId"))
            .flatten()
    }

    /// The key a copy reads (`x-amz-copy-source`), decoded.
    fn source(r: &'a Record) -> Option<(&'a str, Cow<'a, str>)> {
        if r.kind != "CopyObject" && r.kind != "UploadPartCopy" {
            return None;
        }
        let v = r
            .request_headers()
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("x-amz-copy-source"))?;
        let path = v.value.as_str().split('?').next()?.trim_start_matches('/');
        let (b, k) = path.split_once('/')?;
        Some((
            b,
            percent_encoding::percent_decode_str(k).decode_utf8_lossy(),
        ))
    }

    /// The level of the request `r` waits for, the highest: none when it
    /// waits for none.
    fn on(&self, r: &'a Record) -> Option<u32> {
        if let Some((b, prefix)) = Self::listing(r) {
            // the page before, of a listing that goes on
            let before = Self::next_page(r)
                .then(|| self.listed.get(&(b, prefix)).copied())
                .flatten();
            // the keys written under the prefix
            let mut under = self
                .written
                .range((b, prefix)..)
                .take_while(|((wb, k), _)| *wb == b && k.starts_with(prefix));
            let first: Vec<u32> = under.by_ref().take(Self::SCAN).map(|(_, l)| *l).collect();
            let written = match under.next() {
                Some(_) => self.wrote,
                None => first.into_iter().max(),
            };
            return written.max(before);
        }
        // the copy's source, as it was last written
        let source = Self::source(r)
            .and_then(|(b, k)| self.keys.get(&(b, k)))
            .and_then(|&(w, _)| w);
        let (Some(b), Some(k)) = (r.bucket.as_deref(), r.key.as_deref()) else {
            return source;
        };
        let write = !is_read(r);
        let key = self
            .keys
            .get(&(b, Cow::Borrowed(k)))
            .and_then(|&(w, any)| if write { Some(any) } else { w });
        // each of the key's prefixes, as a listing names it
        let listed = (write && !self.listed.is_empty())
            .then(|| {
                (0..=k.len())
                    .filter(|&i| k.is_char_boundary(i))
                    .filter_map(|i| self.listed.get(&(b, &k[..i])).copied())
                    .max()
            })
            .flatten();
        key.max(listed).max(source)
    }

    /// Adds `r`, and says its level.
    fn add(&mut self, r: &'a Record) -> u32 {
        // the parts of an upload all at the level of its first
        let part = Self::part(r);
        let level = match part.and_then(|id| self.parts.get(id)) {
            Some(&l) => l,
            None => self.on(r).map_or(0, |l| l + 1),
        };
        if let Some(id) = part {
            self.parts.insert(id, level);
        }
        if let Some(listed) = Self::listing(r) {
            let at = self.listed.entry(listed).or_default();
            *at = (*at).max(level);
            return level;
        }
        // a copy reads its source: its delete (a move) waits for it
        if let Some(source) = Self::source(r) {
            let at = self.keys.entry(source).or_insert((None, 0));
            at.1 = at.1.max(level);
        }
        if let (Some(b), Some(k)) = (r.bucket.as_deref(), r.key.as_deref()) {
            let write = !is_read(r);
            let at = self.keys.entry((b, Cow::Borrowed(k))).or_insert((None, 0));
            at.1 = at.1.max(level);
            if write {
                at.0 = Some(level);
                self.written.insert((b, k), level);
                self.wrote = self.wrote.max(Some(level));
            }
        }
        level
    }
}

/// What requests take sent at once, each level after the one before
/// ([`Chain::levels`]): of each level, the waves of [`CHAIN_CONCURRENCY`],
/// each as long as its longest. `of` is each request's level and time.
pub fn at_once_ms(mut of: Vec<(u32, f64)>) -> f64 {
    of.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
    of.chunk_by(|a, b| a.0 == b.0)
        .map(|level| {
            level
                .iter()
                .step_by(CHAIN_CONCURRENCY)
                .map(|(_, ms)| ms)
                .sum::<f64>()
        })
        .sum()
}

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
    let mut levels: Vec<u32> = Vec::new();
    let mut end = i64::MIN;
    let close = |cur: &mut Vec<&Record>, levels: &mut Vec<u32>, out: &mut Vec<Chain>| {
        if cur.len() >= CHAIN_MIN {
            let (a, b) = (
                start_us(cur[0]),
                cur.iter().map(|r| end_us(r)).max().unwrap_or(0),
            );
            let lat = |r: &&Record| (end_us(r) - start_us(r)) as f64 / 1000.0;
            // the same requests at once: each level after the one before
            let at_once = at_once_ms(levels.iter().copied().zip(cur.iter().map(lat)).collect());
            out.push(Chain {
                start_ms: ms(a),
                end_ms: ms(b),
                requests: cur.len(),
                ids: cur.iter().map(|r| r.id).collect(),
                at_once_ms: at_once,
                waiting_ms: cur.iter().map(lat).sum::<f64>(),
                levels: std::mem::take(levels),
            });
        }
        cur.clear();
        levels.clear();
    };
    let mut deps = Depends::default();
    for r in sorted {
        let s = start_us(r);
        if !cur.is_empty() && (s < end || s - end > CHAIN_GAP_US) {
            // an overlap (requests at once already) or the program's own pause
            let overlapped = s < end;
            close(&mut cur, &mut levels, &mut out);
            if overlapped {
                end = end.max(end_us(r));
                continue;
            }
        }
        if cur.is_empty() && s < end {
            end = end.max(end_us(r));
            continue;
        }
        if cur.is_empty() {
            deps = Depends::default();
        }
        // it needs what one before it did (reads what it wrote, lists where
        // it wrote): it goes after that one
        levels.push(deps.add(r));
        cur.push(r);
        end = end_us(r);
    }
    close(&mut cur, &mut levels, &mut out);
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
/// at most ("b/logs/2026/"). What a run is *shown* by is [`prefix_groups`].
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

/// The prefixes a run is shown by: its folders at the depth that tells
/// them apart, not cut at a fixed level. With more folders than `max`, the
/// quietest of the deepest is folded into the nearest folder above it that
/// it shares with another, again and again, so what stays apart is what
/// carried the requests. Each name with how many of the run's folders
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
    /// folders of the run folded into it.
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
/// Requests drawn one by one: every request of a run this size or
/// smaller; of a larger one, the slowest and an even spread of the rest.
const MAX_SAMPLES: usize = 4000;
const SLOWEST: usize = 400;

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

/// Each request's part of the waiting, in ms, by its place in `records`:
/// every moment with requests in flight split evenly among them. The parts
/// add up to [`waiting_ms`], and the parts of some requests are the time a
/// run saves without them: their summed time, divided by how many requests
/// were in flight beside them (docs/cli.md#findings). Alone (one at a time), a request's
/// part is its own time; one of eight at once, an eighth of it.
pub fn waits_ms(records: &[Record]) -> Vec<f64> {
    let n = records.len();
    let ends: Vec<i64> = records.iter().map(|r| end_us(r).max(start_us(r))).collect();
    // the places in order of their ends, and of their starts: records are
    // mostly in the order they started, sorted only when not (what is held
    // at once is what a million's peak memory is made of)
    let mut by_end: Vec<u32> = (0..n as u32).collect();
    if !ends.is_sorted() {
        by_end.sort_unstable_by_key(|&i| ends[i as usize]);
    }
    let by_start: Option<Vec<u32>> = (!records.is_sorted_by_key(start_us)).then(|| {
        let mut o: Vec<u32> = (0..n as u32).collect();
        o.sort_by_cached_key(|&i| start_us(&records[i as usize]));
        o
    });
    let start_at = |a: usize| by_start.as_ref().map_or(a, |o| o[a] as usize);
    // the time so far, each moment divided by the requests then in flight:
    // a request's part is this at its end less this at its start
    let mut out = vec![0f64; n];
    let (mut a, mut flying, mut so_far, mut last) = (0, 0i64, 0f64, i64::MIN);
    for &e in &by_end {
        let e = e as usize;
        // the starts before this end, then the end (an end before a start at the same moment)
        while a < n && start_us(&records[start_at(a)]) < ends[e] {
            let i = start_at(a);
            let t = start_us(&records[i]);
            if flying > 0 && t > last {
                so_far += (t - last) as f64 / flying as f64;
            }
            last = last.max(t);
            out[i] -= so_far;
            flying += 1;
            a += 1;
        }
        let t = ends[e];
        if flying > 0 && t > last {
            so_far += (t - last) as f64 / flying as f64;
        }
        last = last.max(t);
        out[e] += so_far;
        flying -= 1;
    }
    for w in &mut out {
        *w /= 1000.0;
    }
    out
}

/// Each kind's part of the waiting, in microseconds: every stretch of time
/// split evenly among the requests in flight then, so that requests at once
/// count once and the kinds add up to the time with a request in flight.
pub(crate) fn waited_us<K: Ord + Clone>(by: &BTreeMap<K, Vec<&Record>>) -> BTreeMap<K, f64> {
    let keys: Vec<&K> = by.keys().collect();
    let mut events: Vec<(i64, i32, u32)> =
        Vec::with_capacity(2 * by.values().map(Vec::len).sum::<usize>());
    for (k, rs) in by.values().enumerate() {
        for r in rs {
            let (s, e) = (start_us(r), end_us(r));
            if e > s {
                events.push((s, 1, k as u32));
                events.push((e, -1, k as u32));
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
        let k = k as usize;
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

/// A run's time axis: its first instant, its length, and the slices
/// every chart over the run shares.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeline {
    /// The first request's start, in µs.
    pub t0_us: i64,
    /// Where that is from the run's start, in ms (0 but for a stretch).
    pub start_ms: f64,
    /// From there to the last request's end (1 at least).
    pub wall_us: i64,
    pub bin_ms: f64,
    pub bins: usize,
}

impl Timeline {
    /// None without a request.
    pub fn of(records: &[Record]) -> Option<Timeline> {
        Timeline::from_origin(records, None)
    }

    /// [`Timeline::of`] for a stretch of a run whose first request
    /// started at `origin_us`: its slices are its own, its times the
    /// run's (`start_ms` on).
    pub fn from_origin(records: &[Record], origin_us: Option<i64>) -> Option<Timeline> {
        let t0_us = records.iter().map(start_us).min()?;
        let t1 = records.iter().map(end_us).max().unwrap_or(t0_us);
        let wall_us = (t1 - t0_us).max(1);
        let wall_ms = wall_us as f64 / 1000.0;
        let bin_ms = nice(wall_ms / MAX_BINS);
        Some(Timeline {
            t0_us,
            start_ms: origin_us.map_or(0.0, |o| (t0_us - o) as f64 / 1000.0),
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
/// definition every figure uses: a run's, a kind's, a slice's.
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
        r.response_headers()
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(name))
    };
    r.status.is_some()
        && !has("x-sonde-proxy")
        && !(has("x-sonde-fault") && !has("x-sonde-store-status"))
}

/// What one request costs: its price, and the bytes it read when they
/// are charged ([`Egress::Charged`]); nothing when it never reached the
/// store.
pub fn price(r: &Record, prices: &RequestPrices) -> f64 {
    let out = match prices.egress() {
        Egress::Charged if reached_store(r) => out_bytes(r),
        _ => 0,
    };
    request_price(r, prices) + prices.out(out)
}

/// The bytes a request read out of the store: its answer's body, when it
/// reached the store.
pub fn out_bytes(r: &Record) -> u64 {
    r.response_bytes.get().unwrap_or(0)
}

/// What one request costs as a request, without its bytes.
pub fn request_price(r: &Record, prices: &RequestPrices) -> f64 {
    match class(r) {
        _ if !reached_store(r) => 0.0,
        Class::Write if r.kind.starts_with("List") => {
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

/// A run's prefixes ([`prefix_groups`], the busiest [`MAX_PREFIXES`] and
/// then "other"), each with its requests and its busiest second; the
/// groups they were counted by; and each request's prefix, by its place.
pub fn prefix_stats(
    records: &[Record],
    prices: &RequestPrices,
) -> (Vec<(String, usize)>, Vec<PrefixStat>, Vec<u8>) {
    // each folder named once, however many requests went to it: a run
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
    let mut slots = Vec::with_capacity(records.len());
    for (r, id) in records.iter().zip(of) {
        let i = slot[id];
        slots.push(i as u8);
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
    (shown, prefixes, slots)
}

/// A run's [`Cost`]: its requests, its bytes out, and what it left
/// stored ([`crate::stored`]).
pub fn cost_of(records: &[Record], prices: &RequestPrices, stored: &crate::stored::Stored) -> Cost {
    let mut c = Cost::none(prices);
    let mut out = 0u64;
    for r in records.iter().filter(|r| reached_store(r)) {
        match class(r) {
            Class::Write => c.writes += 1,
            Class::Read => c.reads += 1,
            Class::Free => c.free += 1,
        }
        c.requests += request_price(r, prices);
        out += out_bytes(r);
    }
    c.transfer.bytes = out;
    c.transfer.amount = match c.transfer.egress {
        Egress::Free => 0.0,
        _ => prices.out(out),
    };
    c.amount = c.requests
        + match c.transfer.egress {
            Egress::Charged => c.transfer.amount,
            _ => 0.0,
        };
    c.storage.bytes = stored.bytes();
    c.storage.per_month = prices.stored(c.storage.bytes);
    c.storage.versioned = stored.versioned;
    c.storage.noncurrent = stored.noncurrent;
    c.storage.noncurrent_bytes = stored.noncurrent_bytes;
    c.storage.noncurrent_per_month = prices.stored(stored.noncurrent_bytes);
    c
}

pub fn profile(records: &[Record], prices: &RequestPrices) -> Profile {
    profile_from(records, prices, None)
}

/// [`profile`] of a stretch of a run whose first request started at
/// `origin_us`: its times on the run's axis ([`Profile::start_ms`]).
pub fn profile_from(records: &[Record], prices: &RequestPrices, origin_us: Option<i64>) -> Profile {
    let empty = Profile {
        start_ms: 0.0,
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
        cost: Cost::none(prices),
        prices: prices.clone(),
        prefixes: Vec::new(),
        heat: Vec::new(),
        flows: Vec::new(),
        chains: Vec::new(),
        latency: Vec::new(),
        p50_ms: 0.0,
        p95_ms: 0.0,
        p99_ms: 0.0,
        latency_heat: Vec::new(),
        callers: Vec::new(),
        callers_cut: false,
        timed: false,
    };
    let Some(tl) = Timeline::from_origin(records, origin_us) else {
        return empty;
    };
    let (t0, wall_us, start) = (tl.t0_us, tl.wall_us, tl.start_ms);
    let busy_us = union_us(records.iter().map(|r| (start_us(r), end_us(r))).collect());
    let ms = |us: i64| us as f64 / 1000.0;
    // an instant, from the run's start
    let at_ms = |us: i64| ms(us - t0) + start;
    let price = |r: &Record| price(r, prices);

    // groups
    // (named by the records' own text: a kind is not copied for each request)
    let mut by: BTreeMap<(&str, bool), Vec<&Record>> = BTreeMap::new();
    for r in records {
        by.entry((r.kind.as_str(), is_miss(r))).or_default().push(r);
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
                kind: operation,
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
                bytes_up: rs.iter().map(|r| r.request_bytes.get().unwrap_or(0)).sum(),
                bytes_down: rs.iter().map(|r| r.response_bytes.get().unwrap_or(0)).sum(),
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

    // rate, by outcome
    let (bin, bins) = (tl.bin_ms, tl.bins);
    let mut rate: Vec<Bin> = (0..bins)
        .map(|i| Bin {
            t_ms: start + i as f64 * bin,
            ok: 0,
            miss: 0,
            client: 0,
            server: 0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
            throttled: 0,
            bytes_up: 0,
            bytes_down: 0,
            requests_cost: 0.0,
            transfer_cost: 0.0,
            storage_cost: 0.0,
        })
        .collect();
    // each slice's latencies side by side in one list, made at its size:
    // the slices' requests counted first
    let mut slice_at = vec![0usize; bins + 1];
    for r in records {
        slice_at[tl.bin_of(r) + 1] += 1;
    }
    for i in 0..bins {
        slice_at[i + 1] += slice_at[i];
    }
    let mut took = vec![0f64; records.len()];
    let mut filled = slice_at.clone();
    // the latencies' grid: every request in its slice and bucket
    let mut grid: BTreeMap<(usize, i32), usize> = BTreeMap::new();
    let transfer_counts = prices.egress() != Egress::Free;
    for r in records {
        let i = tl.bin_of(r);
        took[filled[i]] = r.latency_ms;
        filled[i] += 1;
        *grid
            .entry((i, latency_bucket(r.latency_ms.max(0.0))))
            .or_default() += 1;
        let b = &mut rate[i];
        b.throttled += usize::from(throttled(r));
        b.bytes_up += r.request_bytes.get().unwrap_or(0);
        b.bytes_down += r.response_bytes.get().unwrap_or(0);
        if reached_store(r) {
            b.requests_cost += request_price(r, prices);
            if transfer_counts {
                b.transfer_cost += prices.out(out_bytes(r));
            }
        }
        match tone(r) {
            0 => b.ok += 1,
            1 => b.miss += 1,
            2 => b.client += 1,
            _ => b.server += 1,
        }
    }

    // what the run left stored, by the slice that wrote it
    let stored = crate::stored::stored(records);
    for (i, bytes) in &stored.left {
        rate[tl.bin_of(&records[*i])].storage_cost += prices.stored(*bytes);
    }

    // how long requests took, slice by slice: the band the live view draws
    for (i, b) in rate.iter_mut().enumerate() {
        let l = &mut took[slice_at[i]..slice_at[i + 1]];
        if l.is_empty() {
            continue;
        }
        l.sort_unstable_by(f64::total_cmp);
        let at = |q: f64| percentile(l, q);
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
    // per slice its busiest moment, then where it ended (a burst's peak
    // does not run on through the idle time after it), when every change
    // would be more than the pane draws
    let instants = events.chunk_by(|a, b| a.0 == b.0).count();
    let slice = (instants > MAX_STEPS).then(|| ms(wall_us) / MAX_STEPS as f64);
    let mut n: i64 = 0;
    let mut steps: Vec<Step> = Vec::new();
    let mut per: Vec<(f64, usize, usize)> = Vec::new();
    let (mut peak, mut peak_at) = (0usize, 0.0);
    for instant in events.chunk_by(|a, b| a.0 == b.0) {
        let at = at_ms(instant[0].0);
        for (_, d) in instant {
            n = (n + *d as i64).max(0);
            if n as usize > peak {
                peak = n as usize;
                peak_at = at;
            }
        }
        // where the instant ends
        let n = n as usize;
        match slice {
            None => match steps.last_mut() {
                Some(s) if s.t_ms == at => s.n = n,
                _ => steps.push(Step { t_ms: at, n }),
            },
            Some(slice) => {
                let at = start + ((at - start) / slice).floor() * slice;
                match per.last_mut() {
                    Some(k) if k.0 == at => {
                        k.1 = k.1.max(n);
                        k.2 = n;
                    }
                    _ => per.push((at, n, n)),
                }
            }
        }
    }
    if let Some(slice) = slice {
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
    // (the group of each request sampled: what a request's group is asked for)
    let group_of = |r: &Record| {
        let key = (r.kind.as_str(), is_miss(r));
        groups
            .iter()
            .position(|(g, _)| g.kind == key.0 && g.missed == key.1)
            .unwrap_or(0)
    };
    let sample = |r: &Record| Sample {
        id: r.id,
        group: group_of(r),
        ms: r.latency_ms.max(0.0),
        t_ms: at_ms(start_us(r)),
        status: r.status,
        bytes: r.request_bytes.get().unwrap_or(0) + r.response_bytes.get().unwrap_or(0),
    };
    let mut samples: Vec<Sample> = if records.len() <= MAX_SAMPLES {
        records.iter().map(sample).collect()
    } else {
        // the slowest first, the records' order among equals: picked, not
        // all sorted
        let slower = |a: &usize, b: &usize| {
            records[*b]
                .latency_ms
                .total_cmp(&records[*a].latency_ms)
                .then(a.cmp(b))
        };
        let mut by_ms: Vec<usize> = (0..records.len()).collect();
        by_ms.select_nth_unstable_by(SLOWEST - 1, slower);
        by_ms.truncate(SLOWEST);
        by_ms.sort_unstable_by(slower);
        let mut slow = vec![false; records.len()];
        for &i in &by_ms {
            slow[i] = true;
        }
        let rest: Vec<u32> = (0..records.len() as u32)
            .filter(|i| !slow[*i as usize])
            .collect();
        let want = MAX_SAMPLES - SLOWEST;
        let step = rest.len() as f64 / want as f64;
        by_ms
            .iter()
            .copied()
            .chain(
                (0..want).map(|i| rest[((i as f64 * step) as usize).min(rest.len() - 1)] as usize),
            )
            .map(|i| sample(&records[i]))
            .collect()
    };
    samples.sort_by(|a, b| a.t_ms.total_cmp(&b.t_ms));

    // the places in the program's code that sent them, when it says
    let mut by_place: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for r in records {
        if let Some(p) = sonde_proxy::callers::caller(r) {
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
    let callers_cut = callers.len() > CALLERS;
    callers.truncate(CALLERS);

    let cost = cost_of(records, prices, &stored);
    let (_, prefixes, slots) = prefix_stats(records, prices);
    let mut cells: HashMap<(usize, usize), usize> = HashMap::new();
    for (r, slot) in records.iter().zip(&slots) {
        *cells.entry((*slot as usize, tl.bin_of(r))).or_default() += 1;
    }
    let mut heat: Vec<HeatCell> = cells
        .into_iter()
        .map(|((prefix, bin), n)| HeatCell { prefix, bin, n })
        .collect();
    heat.sort_by_key(|c| (c.prefix, c.bin));

    // flows: operation → prefix → outcome (counted by number, named once)
    let mut top_ops: Vec<&str> = Vec::new();
    for (g, _) in &groups {
        if top_ops.len() < 6 && !top_ops.contains(&g.kind.as_str()) {
            top_ops.push(&g.kind);
        }
    }
    let mut to_prefix: HashMap<(usize, u8), usize> = HashMap::new();
    let mut to_outcome: HashMap<(u8, u8), usize> = HashMap::new();
    for (r, slot) in records.iter().zip(&slots) {
        let op = top_ops
            .iter()
            .position(|o| *o == r.kind)
            .unwrap_or(top_ops.len());
        *to_prefix.entry((op, *slot)).or_default() += 1;
        *to_outcome.entry((*slot, tone(r))).or_default() += 1;
    }
    let prefix_name = |slot: u8| match prefixes[slot as usize].prefix.as_str() {
        "other" => "other prefixes".to_string(),
        p => p.to_string(),
    };
    let mut flow: BTreeMap<(String, String), usize> = BTreeMap::new();
    for ((op, slot), n) in to_prefix {
        let op = top_ops.get(op).map_or("other operations", |o| o);
        *flow.entry((op.to_string(), prefix_name(slot))).or_default() += n;
    }
    for ((slot, tone), n) in to_outcome {
        let outcome = match tone {
            0 => "ok",
            1 => "not found",
            2 => "client error",
            _ => "server error",
        };
        *flow
            .entry((prefix_name(slot), outcome.to_string()))
            .or_default() += n;
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
    // (the slices' latencies are every request's)
    let mut all = took;
    for v in &mut all {
        *v = v.max(0.0);
    }
    all.sort_unstable_by(f64::total_cmp);
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
    let chains = if timed {
        let mut c = chains(records);
        for x in &mut c {
            (x.start_ms, x.end_ms) = (x.start_ms + start, x.end_ms + start);
        }
        c
    } else {
        Vec::new()
    };
    Profile {
        start_ms: start,
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
        latency_heat: if timed {
            grid.into_iter()
                .map(|((bin, bucket), n)| LatencyCell { bin, bucket, n })
                .collect()
        } else {
            Vec::new()
        },
        callers,
        callers_cut,
        cost,
        prices: prices.clone(),
        prefixes,
        heat,
        flows,
        chains,
        latency: if timed { latency } else { Vec::new() },
        p50_ms: q(0.5),
        p95_ms: q(0.95),
        p99_ms: q(0.99),
        timed,
    }
}

/// One kind of request on two stores (a run and its replay): how long it
/// took on each.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyChange {
    pub kind: String,
    pub missed: bool,
    pub requests: (usize, usize),
    pub p50_ms: (f64, f64),
    pub p95_ms: (f64, f64),
    pub waiting_ms: (f64, f64),
}

/// The kinds of request two runs share, by how much slower (p95) the
/// second is, slowest first: a workload measured on two stores.
pub fn latency_changes(a: &Profile, b: &Profile) -> Vec<LatencyChange> {
    let mut out: Vec<LatencyChange> = a
        .groups
        .iter()
        .filter_map(|ga| {
            let gb = b
                .groups
                .iter()
                .find(|g| g.kind == ga.kind && g.missed == ga.missed)?;
            Some(LatencyChange {
                kind: ga.kind.clone(),
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
    pub kind: String,
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
    /// The objects each run asked of (each key, each prefix listed, once):
    /// what its workload is judged by ([`workload_changed`]).
    pub objects: (usize, usize),
    /// The objects the first run's findings fixed did away with
    /// ([`crate::fixloop::freed`]: small files written as fewer, small
    /// reads packed into a shard, prefixes listed as one listing), not
    /// work left undone.
    #[serde(skip)]
    pub freed: usize,
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
    /// saved, measured, and what was predicted), less, new, more or open.
    pub findings: Vec<crate::fixloop::FindingChange>,
    /// Both are runs of one program. Two programs' figures still compare,
    /// but their findings do not: one's are not "gone" because the other
    /// never had them.
    pub same_program: bool,
    /// The exit code of a run compared that failed (the later's, when both
    /// did): its findings are not compared, a crash is no fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<i32>,
    /// How the command changed between two runs of one program (a named
    /// one's), `--limit 200 → 20`: then less, more and fixed are the
    /// workload's doing, and no saving is counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_changed: Option<String>,
    /// How much the workload changed between two runs of one program with
    /// the same command ([`workload_changed`], `objects 120 → 402, 3.4×`):
    /// a fix that does more work than before (all 400 files, not 30), or
    /// less (30, not 400), is not a fix to count, and no saving is counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload_changed: Option<String>,
    /// The two runs' endpoints, when they differ (`rfs`, `demo`): the same
    /// workload on two stores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<(String, String)>,
    /// How much the program's wall time varies run to run, in ms
    /// ([`crate::fixloop::noise_ms`]): read from its runs before the later
    /// one with no fix between them; none with fewer than two.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise_ms: Option<f64>,
    /// The run's wall time moved by less than `noise_ms`: what moved is
    /// not told from noise.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub within_noise: bool,
    /// Whether both runs' waiting is known: not when either's times are to
    /// the second (an imported log's), and then `waiting_ms` says nothing.
    pub waiting_known: bool,
    /// Both runs' latency, bucket by bucket on the same edges: their two
    /// distributions side by side.
    pub latency: (Vec<LatencyBucket>, Vec<LatencyBucket>),
    /// The busiest prefixes of either run, each run's requests to them: where
    /// the change happened.
    pub prefixes: Vec<PrefixChange>,
}

/// A workload that changed between two runs that asked of `(a, b)`
/// objects ([`RunChange::objects`]), said `objects 120 → 402, 3.4×`: grown
/// by half or more, or shrunk by a third or more below what a's findings
/// fixed or less leave (`saved`, the objects they did away with:
/// 120 small files written as 3 is a fix, not less work), by ten objects at
/// least either way. Objects, not requests: the same files read in fewer
/// requests is a fix whether or not a finding said so. None when it did
/// not change.
pub fn workload_changed((a, b): (usize, usize), saved: usize) -> Option<String> {
    let left = a.saturating_sub(saved);
    let grew = a > 0 && b >= a + 10 && b * 2 >= a * 3;
    let shrank = left >= b + 10 && b * 3 <= left * 2;
    (grew || shrank).then(|| {
        format!(
            "objects {} → {}, {:.1}×",
            sonde_fmt::count(a as u64),
            sonde_fmt::count(b as u64),
            b as f64 / a as f64
        )
    })
}

/// One prefix in two runs: its requests in each.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixChange {
    pub prefix: String,
    pub requests: (usize, usize),
}

impl RunChange {
    /// Says whether the two runs are of one program (their
    /// [`crate::program::key_of`]); across programs the findings gone and
    /// new are left out.
    pub fn of_programs(mut self, a: Option<&str>, b: Option<&str>) -> Self {
        self.same_program = a.is_some() && a == b;
        if !self.same_program {
            self.resolved.clear();
            self.appeared.clear();
            self.findings.clear();
        }
        self
    }

    /// Says how the command changed between two runs of one program
    /// ([`crate::program::command_change`]): a finding fixed, less or more
    /// then counts no saving.
    pub fn of_commands(mut self, a: Option<&str>, b: Option<&str>) -> Self {
        self.command_changed = crate::program::command_change(a, b);
        if self.command_changed.is_none() {
            self.workload_changed = workload_changed(self.objects, self.freed);
        }
        if self.command_changed.is_some() || self.workload_changed.is_some() {
            crate::fixloop::by_command(&mut self.findings);
        }
        self
    }

    /// Why what moved between the runs is not counted as a fix: the command
    /// changed, or the workload grew or shrank (`whole`: with how much, else
    /// in a few words, as each finding's line ends).
    pub fn uncounted(&self, whole: bool) -> Option<String> {
        if let Some(c) = &self.command_changed {
            return Some(format!("the command changed ({c})"));
        }
        self.workload_changed.as_ref().map(|w| {
            let how = if self.objects.1 > self.objects.0 {
                "the workload grew"
            } else {
                "the workload shrank"
            };
            if whole {
                format!("{how} ({w})")
            } else {
                how.to_string()
            }
        })
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

/// What a finding is about, as it was (no part made [`stable`]): what
/// tells one finding from another of its kind in one run.
pub fn literal_identity(o: &Observation) -> String {
    identity_as(o, true)
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
    // one finding whatever its form ([`kind_of_phrase`])
    let key = kind_of_phrase(p.key);
    // about several buckets: each of them, not the folder they share (none)
    if OF_BUCKET.contains(&key)
        && let Some(buckets) = p.args.get(BUCKETS)
    {
        return format!("{key}|{}", several(buckets, literal));
    }
    let about: Vec<String> = ["key", "prefix", "format"]
        .iter()
        .filter_map(|k| {
            p.args.get(k).map(|v| {
                let v = about_of(key, v.as_ref());
                if literal { v.to_string() } else { stable(v) }
            })
        })
        .collect();
    format!("{key}|{}", about.join("|"))
}

/// The kinds of finding that are about a bucket, not the key or folder
/// their text names: each is one finding a run, and what it names (the key
/// read and written back the most, the folder all its keys share) moves
/// with its counts. Fixed in part, one reads less, not fixed and new.
const OF_BUCKET: &[&str] = &[
    "obs.rmw",
    "obs.deletes",
    "obs.small",
    "obs.rangeagain",
    "obs.wholeread",
    "obs.smallreads",
    "obs.crawl",
];

/// The argument of a finding's phrase that names its buckets when its
/// objects are in several (`a,b`: sorted, a comma between), beside what
/// its words say. No bucket's name has a comma or a `/`.
pub const BUCKETS: &str = "buckets";

/// Several buckets (`a,b`) as an identity says them: each made [`stable`]
/// (or as it was, `literal`), sorted, each once. Buckets named by the day
/// (`logs-2026-11-25,logs-2026-11-26`) are one (`logs-{date}`).
fn several(buckets: &str, literal: bool) -> String {
    let all: BTreeSet<String> = buckets
        .split(',')
        .map(|b| if literal { b.to_string() } else { stable(b) })
        .collect();
    all.into_iter().collect::<Vec<_>>().join(",")
}

/// Whether two identities name one finding: the same, or of one kind that
/// is about its bucket ([`OF_BUCKET`], one finding a run) with a bucket in
/// common (`obs.small|a,b` and `obs.small|a`: small files still written to
/// `a` are the same finding, less of it, not one fixed and another new).
/// What a program's kept name, the run before's and an entry of `[check]`
/// name a finding by.
pub fn same_finding(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (Some((kind, a)), Some((k, b))) = (a.split_once('|'), b.split_once('|')) else {
        return false;
    };
    if kind != k || !OF_BUCKET.contains(&kind) || a.is_empty() || b.is_empty() {
        return false;
    }
    a.split(',').any(|x| b.split(',').any(|y| x == y))
}

/// Whether `id` is what rules 5 named a finding about several buckets
/// (`obs.small|/`: the folder its buckets share, none): it says no bucket,
/// so it is not made again by these rules.
pub fn of_no_bucket(id: &str) -> bool {
    id.split_once('|')
        .is_some_and(|(kind, about)| OF_BUCKET.contains(&kind) && about == "/")
}

/// What a finding of `kind` is about, of the key or prefix `v` it names:
/// its bucket for the kinds [`OF_BUCKET`] (`demo` of `demo/out/a/`; several
/// buckets' `a,b`, and rules 5's `/` for them, as they are), else `v`.
fn about_of<'a>(kind: &str, v: &'a str) -> &'a str {
    if !OF_BUCKET.contains(&kind) {
        return v;
    }
    match v.split_once('/') {
        Some((b, _)) if !b.is_empty() => b,
        _ => v,
    }
}

/// An identity made by older rules (or written in `[check] expect`) made
/// again by these ([`IDENTITY_RULES`]): its kind as one word, and what it is
/// about as [`identity`] makes it.
pub fn by_rules(id: &str) -> String {
    let mut parts = id.split('|');
    let kind = kind_of_phrase(parts.next().unwrap_or(""));
    let about: Vec<String> = parts
        .map(|v| match OF_BUCKET.contains(&kind) {
            true => several(about_of(kind, v), false),
            false => stable(about_of(kind, v)),
        })
        .collect();
    format!("{kind}|{}", about.join("|"))
}

/// Whether what a finding is about holds what changes between runs
/// ([`stable`] changes it): then another of the run's could read the same.
pub fn varies(o: &Observation) -> bool {
    identity_as(o, true) != identity_as(o, false)
}

/// The kinds of finding a run may have several of, each about one object
/// (a key read again and again, a key polled, a prefix listed again and
/// again, a prefix near its rate), with the fewest requests of an object
/// that make it a finding of the kind at any threshold
/// ([`crate::episode::traces`]): what such a finding is told apart from.
const ONE_OF_SEVERAL: &[(&str, usize)] = &[
    ("obs.hot", 2),
    ("obs.polled", 2),
    ("obs.listed", 2),
    ("obs.throttled", 1),
];

/// The object `r` is to a finding of `kind` ([`ONE_OF_SEVERAL`]), as its
/// bucket and the rest of its name: the key it reads (`obs.hot`) or does
/// not find (`obs.polled`), the prefix it lists from its first page
/// (`obs.listed`), the prefix it is at (`obs.throttled`). None when it is
/// no request of the kind.
fn object_for<'r>(kind: &str, r: &'r Record) -> Option<(&'r str, &'r str)> {
    let b = r.bucket.as_deref();
    match kind {
        "obs.hot" if is_read(r) => Some((b?, r.key.as_deref()?)),
        "obs.polled" if is_miss(r) => Some((b?, r.key.as_deref()?)),
        "obs.listed"
            if r.kind.starts_with("ListObjects")
                && !r.is_error()
                && crate::episode::query(r, "continuation-token").is_none()
                && crate::episode::query(r, "marker").is_none_or(str::is_empty) =>
        {
            Some((
                b.unwrap_or(""),
                crate::episode::query(r, "prefix").unwrap_or(""),
            ))
        }
        "obs.throttled" => Some(prefix_parts(r)).filter(|(b, _)| !b.is_empty()),
        _ => None,
    }
}

/// The objects of `records` that read the same as what one of `findings`
/// is about once what changes between runs is taken out ([`stable`]), and
/// are not it, each of them a finding of its kind at any threshold (`t/01/`
/// listed twice, never `t/01/` listed once, a key written, yesterday's
/// `_SUCCESS` looked at once): each as `(identity, literal identity)` of a
/// finding of that kind about it. One pass over the records, of the kinds
/// that have several findings a run ([`ONE_OF_SEVERAL`]), each object
/// matched part by part with what is the same in every run, and made
/// stable once.
pub fn alike(findings: &[Observation], records: &[Record]) -> Vec<(String, String)> {
    use std::hash::{BuildHasher, BuildHasherDefault};
    // the findings about one thing that varies, by their kind and what they
    // are about made stable (by part): the least that makes a trace of the
    // kind, and what each is about as it was
    let mut wanted: Vec<(&str, usize, Vec<String>, Vec<String>)> = Vec::new();
    for o in findings.iter().filter(|o| varies(o)) {
        let literal = identity_as(o, true);
        let Some((kind, about)) = literal.split_once('|') else {
            continue;
        };
        let Some(&(kind, least)) = ONE_OF_SEVERAL.iter().find(|(k, _)| *k == kind) else {
            continue;
        };
        if about.contains('|') {
            continue;
        }
        let parts: Vec<String> = stable(about).split('/').map(String::from).collect();
        match wanted.iter_mut().find(|w| w.0 == kind && w.2 == parts) {
            Some(w) => w.3.push(about.to_string()),
            None => wanted.push((kind, least, parts, vec![about.to_string()])),
        }
    }
    if wanted.is_empty() {
        return Vec::new();
    }
    let kinds: BTreeSet<&str> = wanted.iter().map(|w| w.0).collect();
    // whether `(b, rest)` is cut as `parts` are, with their parts that
    // never change the same
    let fits = |parts: &[String], (b, rest): (&str, &str)| {
        let mut theirs = std::iter::once(b).chain(rest.split('/'));
        parts.iter().all(|p| {
            theirs
                .next()
                .is_some_and(|t| p.contains('{') || t == p.as_str())
        }) && theirs.next().is_none()
    };
    let hash = |o: (&str, &str)| {
        BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default().hash_one(o)
    };
    // (which of `wanted`, its object as a hash, which record): sixteen
    // bytes an object that fits, sorted and counted, no name written
    let mut near: Vec<(u32, u64, u32)> = Vec::new();
    for (i, r) in records.iter().enumerate() {
        for &kind in &kinds {
            let Some(object) = object_for(kind, r) else {
                continue;
            };
            for (w, (k, _, parts, _)) in wanted.iter().enumerate() {
                if *k == kind && fits(parts, object) {
                    near.push((w as u32, hash(object), i as u32));
                }
            }
        }
    }
    near.sort_unstable();
    let mut found: BTreeSet<(String, String)> = BTreeSet::new();
    for same in near.chunk_by(|a, b| (a.0, a.1) == (b.0, b.1)) {
        let (kind, least, parts, abouts) = &wanted[same[0].0 as usize];
        if same.len() < *least {
            continue;
        }
        let Some((b, rest)) = object_for(kind, &records[same[0].2 as usize]) else {
            continue;
        };
        let theirs = format!("{b}/{rest}");
        if abouts.contains(&theirs) {
            continue;
        }
        let alike = parts
            .iter()
            .zip(theirs.split('/'))
            .all(|(p, t)| !p.contains('{') || stable(t) == *p);
        if alike {
            found.insert((
                format!("{kind}|{}", parts.join("/")),
                format!("{kind}|{theirs}"),
            ));
        }
    }
    found.into_iter().collect()
}

/// Names each of a run's findings ([`Observation::identity`]) by what it is
/// about made [`stable`]; but where two of them read the same so, or one
/// and one of `among` (the run's other objects read alike, [`alike`]: as
/// `(identity, literal identity)`), each keeps as they were the parts of
/// what it is about that tell them apart, the rest made stable: `t00/` and
/// `t01/` listed again and again are two findings (`runs/run-{n}/t00/`,
/// whatever the run's own folder), not one, though `t01/` is listed under
/// its threshold. Two that read the same even so are told apart by rank
/// (`#2`). A run's findings are named so once, where they are made, and
/// never again.
pub fn name_findings(findings: &mut [Observation], among: &[(String, String)]) {
    use std::collections::HashSet;
    let mut by: HashMap<String, HashSet<String>> = HashMap::new();
    for o in findings.iter() {
        by.entry(identity_as(o, false))
            .or_default()
            .insert(identity_as(o, true));
    }
    for (id, literal) in among {
        if let Some(g) = by.get_mut(id) {
            g.insert(literal.clone());
        }
    }
    // of each group read alike, the parts all its members share: once, not
    // once a member
    let shared: HashMap<&str, Option<Vec<bool>>> = by
        .iter()
        .filter(|(_, g)| g.len() > 1)
        .map(|(k, g)| (k.as_str(), shared_parts(g)))
        .collect();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for o in findings.iter_mut() {
        let id = identity_as(o, false);
        let id = match shared.get(id.as_str()) {
            Some(parts) => told_apart(&identity_as(o, true), parts.as_deref()),
            None => id,
        };
        let n = seen.entry(id.clone()).or_default();
        *n += 1;
        o.identity = if *n == 1 { id } else { format!("{id}#{n}") };
        o.kind = kind_of(&o.identity).to_string();
    }
}

/// Which parts (between `/`) every one of a group's literal identities
/// shares; none when they are not cut alike.
fn shared_parts(group: &std::collections::HashSet<String>) -> Option<Vec<bool>> {
    let mut members = group.iter().map(|g| g.split('/').collect::<Vec<&str>>());
    let first = members.next()?;
    let mut shared = vec![true; first.len()];
    for m in members {
        if m.len() != first.len() {
            return None;
        }
        for (i, p) in m.iter().enumerate() {
            shared[i] &= *p == first[i];
        }
    }
    Some(shared)
}

/// A literal identity among others that read the same once stable: each
/// part (between `/`) all of them share ([`shared_parts`]) made [`stable`],
/// the parts that differ kept as they are. As it is when they are not cut
/// alike.
fn told_apart(literal: &str, shared: Option<&[bool]>) -> String {
    let Some(shared) = shared else {
        return literal.to_string();
    };
    literal
        .split('/')
        .zip(shared)
        .map(|(p, &same)| if same { stable(p) } else { p.to_string() })
        .collect::<Vec<_>>()
        .join("/")
}

/// The findings of `b` that `a` named otherwise, by what each is about as
/// it was (a finding's literal identity, [`literal_identity`]), each as
/// `(b's name, a's name)`: `a`'s name for it, where neither run has a
/// finding of the other's name.
pub fn names_after(a: &Run, b: &Run) -> Vec<(String, String)> {
    let named = |r: &Run| -> std::collections::HashSet<String> {
        r.observations.iter().map(|o| o.identity.clone()).collect()
    };
    let (in_a, in_b) = (named(a), named(b));
    let mut by_literal: HashMap<String, &str> = HashMap::new();
    for o in a
        .observations
        .iter()
        .filter(|o| !in_b.contains(&o.identity))
    {
        by_literal
            .entry(literal_identity(o))
            .or_insert(o.identity.as_str());
    }
    b.observations
        .iter()
        .filter(|o| !o.identity.is_empty() && !in_a.contains(&o.identity))
        .filter_map(|o| {
            let was = match by_literal.remove(&literal_identity(o)) {
                Some(was) => was,
                // about its buckets, some of them the same ([`same_finding`])
                None => {
                    let at = by_literal
                        .iter()
                        .filter(|(_, was)| same_finding(was, &o.identity))
                        .map(|(literal, _)| literal.clone())
                        .min()?;
                    by_literal.remove(&at)?
                }
            };
            Some((o.identity.clone(), was.to_string()))
        })
        .collect()
}

/// Whether the identity `id` (a finding's, as a program keeps it) names
/// the finding whose literal identity is `literal`: of one kind, each part
/// of what it is about as the finding's, or that made [`stable`]; or, about
/// its buckets, with one in common ([`same_finding`]).
fn names(id: &str, literal: &str) -> bool {
    let (Some((kind, about)), Some((k, theirs))) = (id.split_once('|'), literal.split_once('|'))
    else {
        return false;
    };
    if OF_BUCKET.contains(&kind) && same_finding(id, &by_rules(literal)) {
        return true;
    }
    let mut parts = about.split('/');
    kind == k
        && theirs.split('/').all(|t| {
            parts
                .next()
                .is_some_and(|p| p == t || (p.contains('{') && stable(t) == p))
        })
        && parts.next().is_none()
}

/// Names a run's findings at any threshold (`traces`, named among
/// themselves as they are made) after its findings: a trace of a finding
/// (what it is about the same, as it was) takes the finding's identity, so
/// a finding has one identity, as a finding and as a trace.
pub fn name_traces(traces: &mut [Observation], findings: &[Observation]) {
    let of: HashMap<String, &str> = findings
        .iter()
        .map(|o| (identity_as(o, true), o.identity.as_str()))
        .collect();
    for t in traces.iter_mut() {
        if let Some(id) = of.get(&identity_as(t, true)) {
            t.identity = id.to_string();
            t.kind = kind_of(id).to_string();
        }
    }
}

/// A finding's kind, as its identity begins (`obs.hot` of `obs.hot|b/k`).
pub fn kind_of(identity: &str) -> &str {
    identity.split('|').next().unwrap_or("")
}

/// The kind of finding a phrase says, whatever its form: `obs.` and one
/// word (`obs.small.parts` is `obs.small`, `obs.throttled.rate` and a
/// prefix near its rate, `obs.prefixrate.reads`, are `obs.throttled`).
pub fn kind_of_phrase(key: &str) -> &str {
    if key.starts_with("obs.prefixrate") {
        return "obs.throttled";
    }
    match key.match_indices('.').nth(1) {
        Some((i, _)) => &key[..i],
        None => key,
    }
}

/// The kinds of finding, as an identity begins (`obs.hot` of `obs.hot|b/k`):
/// what `[check] expect` may name.
pub const FINDING_KINDS: &[&str] = &[
    "obs.unfinished",
    "obs.orphans",
    "obs.small",
    "obs.hot",
    "obs.rangeagain",
    "obs.wholeread",
    "obs.smallreads",
    "obs.listed",
    "obs.crawl",
    "obs.headget",
    "obs.deletes",
    "obs.rmw",
    "obs.polled",
    "obs.rename",
    "obs.markers",
    "obs.throttled",
    "obs.serial",
    // a fault scenario the program did not survive ([`crate::trial::findings`])
    "trial",
];

/// The version of the rules a finding's identity is made by ([`stable`]
/// and [`identity`]): kept with a program's findings (in `.index.jsonl`), so
/// findings kept under older rules are made again by these, not read as one
/// fixed and another new. 1 was 19.0's; 2 takes every number out; 3 keeps
/// a digit inside a word (`s3out`); 4 makes a prefix near its rate
/// (`obs.prefixrate.reads`, `.writes`) a throttling (`obs.throttled`); 5
/// makes a read written back, deletes one at a time and small writes about
/// their bucket ([`OF_BUCKET`]); 6 names each bucket of one about several
/// (`obs.small|a,b`, where 5 said `/`, or the bucket of the key a read
/// written back named), the same finding as one of its kind with a bucket
/// in common ([`same_finding`]).
pub const IDENTITY_RULES: u32 = 6;

/// A key or prefix with what changes from one run of a program to the next
/// taken out: the value of a `name=value` folder `name={v}`, a UUID
/// `{uuid}`, a date `{date}`, a date and time `{time}`, a word of six or
/// more hex letters and digits (a commit, a short run id) or a long mixed
/// id `{id}`, and every other run of digits `{n}` (`part-{n}`, `run-{n}`,
/// `v{n}`) but one with a letter on both sides (`s3out`, `h2o`). Two findings of one run that read the same once so are told
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
            // each run of digits, but one inside a word (`s3out`, `h2o`):
            // a letter on both sides of it is part of the name
            let wb = w.as_bytes();
            let mut j = 0;
            while j < wb.len() {
                if !wb[j].is_ascii_digit() {
                    out.push(wb[j] as char);
                    j += 1;
                    continue;
                }
                let run = wb[j..].iter().take_while(|c| c.is_ascii_digit()).count();
                let inside = j > 0 && j + run < wb.len();
                if inside {
                    out.push_str(&w[j..j + run]);
                } else {
                    out.push_str("{n}");
                }
                j += run;
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
    /// The objects it asked of: each key, and each prefix listed, once.
    pub objects: usize,
    /// Its failed requests (lookups that found nothing are no failure).
    pub errors: usize,
    pub profile: Profile,
    pub observations: Vec<Observation>,
    /// Its requests by folder ([`folder_of`]): what the prefixes both runs
    /// are shown by are chosen from.
    pub folders: HashMap<String, usize>,
    /// Its findings at any threshold, by identity: a finding of the run
    /// before that is only among these is less, not fixed. (Their counts
    /// and words: the operations, saved requests, fixes and callers each
    /// names are let go.)
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

    /// [`Run::of`] a run's records, read with its header as its
    /// findings were ([`crate::episode::traces_of`]), its operations made
    /// already.
    pub fn of_run(
        header: &crate::run::RunHeader,
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
            .map(|ask| ask.asked.as_str())
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
        Run::reckoned(records, profile, observations, traces, true)
    }

    /// [`Run::with`] for a run only ever the later of two compared: what
    /// its findings would save by what their requests asked ([`Run::asks`])
    /// is read of the earlier run alone, and is not reckoned.
    pub fn later(
        records: &[Record],
        profile: Profile,
        observations: Vec<Observation>,
        traces: Vec<Observation>,
    ) -> Run {
        Run::reckoned(records, profile, observations, traces, false)
    }

    fn reckoned(
        records: &[Record],
        profile: Profile,
        observations: Vec<Observation>,
        traces: Vec<Observation>,
        asked: bool,
    ) -> Run {
        let mut folders: HashMap<String, usize> = HashMap::new();
        let mut name = String::new();
        // each object (a listing by its prefix) as a hash of its name: eight
        // bytes a request, sorted and counted once, not a set of names (a
        // tenth of an outage's memory at its peak)
        let mut objects: Vec<u64> = Vec::with_capacity(records.len());
        for r in records {
            objects.push(object_of(r));
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
        // a finding keeps the identity it was made with; its trace takes it
        let mut traces = traces;
        name_traces(&mut traces, &observations);
        let asks = if asked {
            crate::fixloop::asks_of(records, &observations, &objects)
        } else {
            crate::fixloop::Asks::new()
        };
        let error_classes = error_classes(records);
        // (a trace is read for its counts and its words, not for the
        // operations and requests it names: a run of a million has tens of
        // thousands)
        let traces = traces
            .into_iter()
            .map(|mut o| {
                (o.episodes, o.saved_ids, o.kept_ids) = (Vec::new(), Vec::new(), Vec::new());
                (o.fixes, o.callers) = (Vec::new(), Vec::new());
                (finding_identity(&o), o)
            })
            .collect();
        objects.sort_unstable();
        objects.dedup();
        let objects = objects.len();
        Run {
            requests: records.len(),
            objects,
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

    /// Gives each of its findings the name the run before (`before`) gave
    /// it, by what it is about as it was, where the two runs named it
    /// otherwise: told apart from a sibling in one run (`t/00/`) and not in
    /// the other (its sibling gone, `t/{n}/`), it is one finding, not one
    /// fixed and another new ([`names_after`]).
    pub fn take_names_after(&mut self, before: &Run) {
        for (from, to) in names_after(before, self) {
            self.rename(&from, &to);
        }
    }

    /// Gives each of its findings the name its program keeps it by
    /// (`kept`), where it names it otherwise: a name that tells a finding
    /// apart (`t/00/`, its sibling gone since) names it while it is there,
    /// as it was or made [`stable`], each name once.
    pub fn take_kept_names<'k>(&mut self, kept: impl IntoIterator<Item = &'k String>) {
        let own: std::collections::HashSet<&str> = self
            .observations
            .iter()
            .map(|o| o.identity.as_str())
            .collect();
        let mut free: Vec<&String> = kept
            .into_iter()
            .filter(|k| !own.contains(k.as_str()) && !k.contains('#'))
            .collect();
        let mut renames = Vec::new();
        for o in &self.observations {
            if o.identity.is_empty() {
                continue;
            }
            let literal = literal_identity(o);
            if let Some(at) = free.iter().position(|k| names(k, &literal)) {
                renames.push((o.identity.clone(), free.remove(at).clone()));
            }
        }
        for (from, to) in renames {
            self.rename(&from, &to);
        }
    }

    /// Its finding `from` named `to`: as a finding, at any threshold, and
    /// what it would save.
    fn rename(&mut self, from: &str, to: &str) {
        for o in self.observations.iter_mut().filter(|o| o.identity == from) {
            o.identity = to.to_string();
        }
        if let Some(mut t) = self.traces.remove(from) {
            t.identity = to.to_string();
            self.traces.insert(to.to_string(), t);
        }
        if let Some(asks) = self.asks.remove(from) {
            self.asks.insert(to.to_string(), asks);
        }
    }

    /// A run of `records` reckoned: where its time went, its operations and
    /// its findings.
    pub fn analysed(
        header: &crate::run::RunHeader,
        records: &[Record],
        prices: &RequestPrices,
    ) -> Run {
        // (one after another, as [`crate::view::Analysis::of_records`])
        let profile = profile(records, prices);
        let episodes = crate::episode::episodes(records);
        let mut observations = crate::episode::observations_of(header, records, &episodes);
        // ranked as a plan ranks them: what each is predicted to save
        let reckoning = crate::fixes::Reckoning::of(records, &profile, !header.whole_seconds);
        crate::fixes::rank_by_saving(&mut observations, &reckoning);
        drop(reckoning);
        let traces = crate::episode::traces_of(header, records, &episodes);
        drop(episodes);
        Run::with(records, profile, observations, traces)
    }
}

/// A request's object, as a hash of its name: its bucket and key, a
/// listing's bucket and prefix (a key it is not).
pub fn object_of(r: &Record) -> u64 {
    use std::hash::{BuildHasher, BuildHasherDefault};
    let prefix = match r.key.as_deref() {
        Some(_) => "",
        None => r
            .query()
            .iter()
            .find(|(k, _)| k == "prefix")
            .map_or("", |(_, v)| v.as_ref()),
    };
    BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default().hash_one((
        r.bucket.as_deref().unwrap_or(""),
        r.key.as_deref(),
        prefix,
    ))
}

pub fn compare_runs(a: &Run, b: &Run) -> RunChange {
    let keys: BTreeSet<(String, bool)> = a
        .profile
        .groups
        .iter()
        .chain(b.profile.groups.iter())
        .map(|g| (g.kind.clone(), g.missed))
        .collect();
    // a kind's waiting is its share of the run's, as `show` says it
    // ([`Group::waited_ms`]), not the time one of them was in flight
    let find = |p: &Profile, k: &(String, bool)| {
        p.groups
            .iter()
            .find(|g| g.kind == k.0 && g.missed == k.1)
            .map_or((0, 0.0), |g| (g.requests, g.waited_ms))
    };
    let mut groups: Vec<GroupChange> = keys
        .into_iter()
        .map(|k| {
            let (ra, ba) = find(&a.profile, &k);
            let (rb, bb) = find(&b.profile, &k);
            GroupChange {
                kind: k.0,
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
    let freed = crate::fixloop::freed(&findings, a, b);
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
        objects: (a.objects, b.objects),
        freed,
        wall_ms: (pa.wall_ms, pb.wall_ms),
        waiting_ms: (pa.waiting_ms, pb.waiting_ms),
        errors: (a.errors, b.errors),
        cost: (pa.cost.amount, pb.cost.amount),
        currency: pb.cost.currency.clone(),
        peak_in_flight: (pa.peak_in_flight, pb.peak_in_flight),
        groups,
        latency: (pa.latency.clone(), pb.latency.clone()),
        resolved: said(&findings, &[State::Fixed]),
        appeared: said(&findings, &[State::New]),
        findings,
        same_program: true,
        failed: None,
        command_changed: None,
        workload_changed: None,
        endpoints: None,
        noise_ms: None,
        within_noise: false,
        waiting_known: true,
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
            kind: op.into(),
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
            // a digit inside a word is part of its name
            ("lake/s3out/", "lake/s3out/"),
            ("b/h2o/v2/x", "b/h2o/v{n}/x"),
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
            saves_bytes: 0,
            saved_ids: Vec::new(),
            kept_ids: Vec::new(),
            keeps_bytes: false,
            timed_ids: Vec::new(),
            fixes: Vec::new(),
            callers: Vec::new(),
            sdk: None,
            identity: String::new(),
            kind: String::new(),
            saves_cost: 0.0,
            expected: None,
        };
        let mut two = vec![listed("b/t00/"), listed("b/t01/")];
        name_findings(&mut two, &[]);
        assert_eq!(two[0].identity, "obs.listed|b/t00/");
        assert_eq!(two[1].identity, "obs.listed|b/t01/");
        // what tells them apart is kept, a run's own folder is not
        let mut runs = vec![listed("b/runs/run-17/t00/"), listed("b/runs/run-17/t01/")];
        name_findings(&mut runs, &[]);
        assert_eq!(runs[0].identity, "obs.listed|b/runs/run-{n}/t00/");
        assert_eq!(runs[1].identity, "obs.listed|b/runs/run-{n}/t01/");
        // one over its threshold, the other only listed: told apart among
        // the run's objects, whatever the other's count
        let list = |id: u64, prefix: &str| {
            let mut r = rec(id, 0, "ListObjectsV2", 200, 1.0);
            r.bucket = Some("b".into());
            r.lists_mut().query = vec![("prefix".into(), prefix.to_string().into())];
            r
        };
        let mut one = vec![listed("b/t00/")];
        let others = alike(
            &one,
            &[
                list(1, "t00/"),
                list(2, "t01/"),
                list(3, "t01/"),
                list(4, "t02/"),
                list(5, "x/"),
                list(6, "x/"),
            ],
        );
        // (`t02/` listed once is no finding at any threshold: not one to
        // tell `t00/` apart from)
        assert_eq!(
            others,
            [("obs.listed|b/t{n}/".into(), "obs.listed|b/t01/".into())]
        );
        name_findings(&mut one, &others);
        assert_eq!(one[0].identity, "obs.listed|b/t00/");
        let mut traces = vec![listed("b/t00/"), listed("b/t01/")];
        name_findings(&mut traces, &[]);
        name_traces(&mut traces, &one);
        assert_eq!(traces[0].identity, "obs.listed|b/t00/");
        assert_eq!(traces[1].identity, "obs.listed|b/t01/");
        // alone in the run, what changes between runs is taken out
        let mut alone = vec![listed("b/t00/")];
        let none = alike(&alone, &[list(1, "t00/")]);
        assert!(none.is_empty());
        name_findings(&mut alone, &none);
        assert_eq!(alone[0].identity, "obs.listed|b/t{n}/");
        // the same key twice is told apart by rank
        let mut same = vec![listed("b/t00/"), listed("b/t00/")];
        name_findings(&mut same, &[]);
        assert_eq!(same[1].identity, "obs.listed|b/t{n}/#2");
    }

    #[test]
    fn every_form_of_throttling_is_one_kind_of_finding() {
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
            saves_bytes: 0,
            saved_ids: Vec::new(),
            kept_ids: Vec::new(),
            keeps_bytes: false,
            timed_ids: Vec::new(),
            fixes: Vec::new(),
            callers: Vec::new(),
            sdk: None,
            identity: String::new(),
            kind: String::new(),
            saves_cost: 0.0,
            expected: None,
        };
        // a prefix near its rate, reads or writes, is a throttling of it
        // (a prefix is said near one of the two, the nearer)
        let (r, w) = (
            finding_identity(&of("obs.prefixrate.reads")),
            finding_identity(&of("obs.prefixrate.writes")),
        );
        assert_eq!(r, "obs.throttled|b/t/");
        assert_eq!(w, r);
        assert_eq!(kind_of(&r), "obs.throttled");
        // one finding whatever its form
        assert_eq!(
            finding_identity(&of("obs.throttled.rate")),
            finding_identity(&of("obs.throttled"))
        );
        assert_eq!(kind_of_phrase("obs.small.parts"), "obs.small");
        assert_eq!(kind_of_phrase("obs.rmw.log"), "obs.rmw");
        assert_eq!(kind_of_phrase("obs.hot"), "obs.hot");
        // the kinds `show -f json` says are these, in the order docs/cli.md lists them
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../docs/schemas/show.schema.json")).unwrap();
        let listed = &schema["$defs"]["observation"]["properties"]["kind"]["enum"];
        assert_eq!(
            listed.as_array().map(|a| a.len()),
            Some(FINDING_KINDS.len()),
            "{listed}"
        );
        assert!(
            FINDING_KINDS
                .iter()
                .all(|k| listed.as_array().unwrap().iter().any(|v| v == k))
        );
        // every kind is one of those `[check] expect` may name
        for k in [
            "obs.small",
            "obs.small.parts",
            "obs.deletes",
            "obs.rmw",
            "obs.rmw.log",
            "obs.polled",
            "obs.orphans",
            "obs.throttled.rate",
            "obs.prefixrate.writes",
        ] {
            assert!(FINDING_KINDS.contains(&kind_of_phrase(k)), "{k}");
        }
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
        let get = c.groups.iter().find(|g| g.kind == "GetObject").unwrap();
        let shown = pa.groups.iter().find(|g| g.kind == "GetObject").unwrap();
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
        let get = p.groups.iter().find(|g| g.kind == "GetObject").unwrap();
        assert_eq!(get.waiting_ms, 50.0);
        assert_eq!(get.sum_ms, 500.0);
        assert_eq!(p.groups[0].kind, "PutObject", "most time first");
        assert!((p.groups[0].share - 100.0 / 150.0).abs() < 1e-9);
        assert_eq!(p.rate.iter().map(|b| b.ok).sum::<usize>(), 11);
    }

    #[test]
    fn every_request_is_in_the_distribution_and_a_slow_down_in_its_slice() {
        let mut rs: Vec<Record> = (0..20)
            .map(|i| rec(i + 1, i as i64, "GetObject", 200, 3.0 + i as f64))
            .collect();
        let mut slow = rec(21, 30, "PutObject", 503, 400.0);
        slow.rare_mut().error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
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
        // every request in the latencies' grid, in the bucket of its latency
        assert_eq!(p.latency_heat.iter().map(|c| c.n).sum::<usize>(), 21);
        assert!(
            p.latency_heat
                .iter()
                .any(|c| c.bucket == latency_bucket(400.0) && c.n == 1)
        );
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
        let by = |op: &str| p.groups.iter().find(|g| g.kind == op).unwrap();
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
        assert!(p.groups.iter().any(|g| g.kind == "HeadObject" && g.missed));
        assert!(p.groups.iter().any(|g| g.kind == "HeadObject" && !g.missed));
        assert_eq!((p.cost.writes, p.cost.reads, p.cost.free), (1, 2, 1));
        assert!((p.cost.amount - (0.005 + 2.0 * 0.0004) / 1000.0).abs() < 1e-12);
        assert_eq!(p.rate.iter().map(|b| b.miss).sum::<usize>(), 1);
    }

    #[test]
    fn an_empty_run_has_an_empty_profile() {
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
                kind: if i < 4 {
                    "GetObject".into()
                } else {
                    "HeadObject".into()
                },
                status: Some(200),
                latency_ms: 10.0,
                lists: reqlog::Lists {
                    request_headers: vec![Header::new(
                        sonde_proxy::callers::CALLER_HEADER,
                        if i < 4 {
                            "etl/load.py:12 read"
                        } else {
                            "etl/load.py:30 check"
                        },
                    )],
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            })
            .collect();
        let p = profile(&rs, &RequestPrices::default());
        assert_eq!(p.callers.len(), 2);
        assert_eq!(p.callers[0].place, "etl/load.py:12 read");
        assert_eq!(p.callers[0].requests, 4);
        assert!((p.callers.iter().map(|c| c.waited_ms).sum::<f64>() - p.waiting_ms).abs() < 1e-6);
        // each sample says its answer; every request is drawn in a run this small
        assert_eq!(p.samples.len(), 6);
        assert!(p.samples.iter().all(|s| s.status == Some(200)));
        assert!(!p.callers_cut);
    }

    /// 24.0 said `12+` of exactly 12 callers: the profile says when it left
    /// some out.
    #[test]
    fn a_profile_says_whether_it_left_callers_out() {
        use reqlog::Header;
        let of = |places: u64| -> Vec<Record> {
            (0..places)
                .map(|i| Record {
                    id: i + 1,
                    time: chrono::DateTime::from_timestamp_millis(
                        1_700_000_000_000 + i as i64 * 20,
                    )
                    .unwrap(),
                    method: "GET".into(),
                    kind: "GetObject".into(),
                    status: Some(200),
                    latency_ms: 10.0,
                    lists: reqlog::Lists {
                        request_headers: vec![Header::new(
                            sonde_proxy::callers::CALLER_HEADER,
                            format!("etl/load.py:{i} read"),
                        )],
                        ..Default::default()
                    }
                    .into(),
                    ..Default::default()
                })
                .collect()
        };
        let p = profile(&of(12), &RequestPrices::default());
        assert_eq!((p.callers.len(), p.callers_cut), (12, false));
        let p = profile(&of(13), &RequestPrices::default());
        assert_eq!((p.callers.len(), p.callers_cut), (12, true));
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["callersCut"], true);
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
        assert_eq!(c.groups[0].kind, "GetObject");
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
    fn the_latencies_grid_counts_every_request_on_three_buckets_a_decade() {
        assert_eq!(latency_bucket(1.0), 0);
        assert_eq!(latency_bucket(1.5), 1);
        assert_eq!(latency_bucket(10.0), 3);
        assert_eq!(latency_bucket(0.0), -3);
        // past what is drawn one by one: the grid still holds every request
        let rs: Vec<Record> = (0..5000)
            .map(|i| {
                rec(
                    i,
                    i as i64,
                    "GetObject",
                    200,
                    if i % 100 == 0 { 900.0 } else { 3.0 },
                )
            })
            .collect();
        let p = profile(&rs, &RequestPrices::default());
        assert!(p.samples.len() < rs.len());
        assert_eq!(p.latency_heat.iter().map(|c| c.n).sum::<usize>(), 5000);
        let slow: usize = p
            .latency_heat
            .iter()
            .filter(|c| c.bucket == latency_bucket(900.0))
            .map(|c| c.n)
            .sum();
        assert_eq!(
            slow, 50,
            "the slow ones as many as there were, not as the sample has them"
        );
    }

    #[test]
    fn a_stretchs_profile_is_on_the_runs_axis_with_slices_of_its_own() {
        let rs: Vec<Record> = (0..100)
            .map(|i| rec(i, i as i64 * 100, "GetObject", 200, 5.0))
            .collect();
        let whole = profile(&rs, &RequestPrices::default());
        let origin = rs.iter().map(start_us).min();
        // the requests of 2 s to 3 s
        let held: Vec<Record> = rs
            .iter()
            .filter(|r| (2000..3000).contains(&((start_us(r) - origin.unwrap()) / 1000)))
            .cloned()
            .collect();
        let p = profile_from(&held, &RequestPrices::default(), origin);
        assert_eq!((whole.start_ms, p.start_ms), (0.0, 2000.0));
        assert!(
            p.bin_ms < whole.bin_ms,
            "finer slices: {} against {}",
            p.bin_ms,
            whole.bin_ms
        );
        assert_eq!(p.rate[0].t_ms, 2000.0);
        assert!(p.samples.iter().all(|s| (2000.0..3000.0).contains(&s.t_ms)));
        assert!(p.peak_at_ms >= 2000.0);
    }

    #[test]
    fn a_percentile_is_the_same_number_wherever_it_is_shown() {
        assert_eq!(percentile(&[], 0.5), 0.0);
        assert_eq!(percentile(&[10.0, 30.0], 0.5), 10.0);
        assert_eq!(percentile(&[10.0, 30.0], 0.95), 30.0);
        // two requests of one kind: the run's median is the kind's, the
        // slice's and the summary's
        let rs = [
            rec(1, 0, "GetObject", 200, 10.0),
            rec(2, 1, "GetObject", 200, 30.0),
        ];
        let p = profile(&rs, &RequestPrices::default());
        assert_eq!(p.p50_ms, 10.0);
        assert_eq!(p.groups[0].p50_ms, 10.0);
        assert_eq!(p.rate[0].p50_ms, 10.0);
        let s = crate::summary::summarize(&rs);
        assert_eq!(s.operations[0].p50_ms, 10.0);
    }

    #[test]
    fn a_request_that_never_reached_the_store_is_not_on_its_bill() {
        let mut refused = rec(2, 10, "PutObject", 403, 0.0);
        refused.lists_mut().response_headers =
            vec![reqlog::Header::new("x-sonde-proxy", "refused")];
        let mut lost = rec(3, 20, "PutObject", 200, 5.0);
        lost.status = None;
        lost.rare_mut().transport_error = Some("connection refused".into());
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
            kind: "PutObject".into(),
            bucket: Some("b".into()),
            key: Some("k".into()),
            status: Some(200),
            ..Default::default()
        };
        assert!(price(&put, &prices) > 0.0);
        // a trial's 503: the store never saw it
        let mut fault = put.clone();
        fault.status = Some(503);
        fault.rare_mut().injected = Some("slowdown".into());
        fault
            .lists_mut()
            .response_headers
            .push(Header::new("x-sonde-fault", "slowdown"));
        assert!(!reached_store(&fault));
        assert_eq!(price(&fault, &prices), 0.0);
        // an answer lost on purpose: the store made the write, and bills it
        let mut lost = fault.clone();
        lost.lists_mut()
            .response_headers
            .push(Header::new("x-sonde-store-status", "200"));
        assert_eq!(price(&lost, &prices), price(&put, &prices));
        let p = profile(&[put, fault, lost], &prices);
        assert_eq!(p.cost.writes, 2);
    }

    #[test]
    fn requests_that_need_the_one_before_stay_after_it_and_the_others_go_at_once() {
        let at = |id: u64, op: &str, key: &str| {
            let mut r = Record {
                id,
                time: chrono::DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 3)
                    .unwrap(),
                method: if op == "PutObject" {
                    "PUT".into()
                } else {
                    "GET".into()
                },
                kind: op.into(),
                status: Some(200),
                latency_ms: 2.0,
                ..Default::default()
            };
            r.bucket = Some("b".into());
            r.key = Some(key.into());
            r
        };
        // twelve reads of twelve keys, one after another: eight at once would do
        let reads: Vec<Record> = (1..=12)
            .map(|i| at(i, "GetObject", &format!("k{i}")))
            .collect();
        assert_eq!(chains(&reads).iter().map(|c| c.requests).sum::<usize>(), 12);
        // each key written and then read back: the read needs its write, so
        // it stays after it (sent at once it would read another thing); the
        // writes go at once, then the reads
        let back: Vec<Record> = (0..12u64)
            .flat_map(|i| {
                let k = format!("k{i}");
                [
                    at(2 * i + 1, "PutObject", &k),
                    at(2 * i + 2, "GetObject", &k),
                ]
            })
            .collect();
        let c = chains(&back);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].levels, [0, 1].repeat(12));
        // the twelve writes in two waves (4 ms), then the reads in two (4 ms)
        assert!((c[0].at_once_ms - 8.0).abs() < 1e-9, "{c:?}");
        // a listing of where the program wrote needs those writes too
        let mut listed: Vec<Record> = (1..=11)
            .map(|i| at(i, "PutObject", &format!("out/{i}")))
            .collect();
        let mut l = Record {
            id: 12,
            time: chrono::DateTime::from_timestamp_millis(1_700_000_000_036).unwrap(),
            method: "GET".into(),
            kind: "ListObjectsV2".into(),
            status: Some(200),
            latency_ms: 2.0,
            ..Default::default()
        };
        l.bucket = Some("b".into());
        l.lists_mut().query = vec![("prefix".into(), "out/".into())];
        listed.push(l);
        let c = chains(&listed);
        assert_eq!(c[0].requests, 12);
        assert_eq!(c[0].levels, [vec![0; 11], vec![1]].concat());
    }

    /// 32.0's eval: what a request waits for that its key alone does not
    /// say. A listing's next page needs the page before (its token: a
    /// boto3 paginator is no requests one after another); the parts of an
    /// upload go at once after it began, and it completes after them; a
    /// copy reads its source, so a move's delete waits for its copy.
    #[test]
    fn pages_parts_and_moves_wait_for_what_they_need() {
        let at = |id: u64, op: &str, method: &str, key: Option<&str>, query: &[(&str, &str)]| {
            let mut r = Record {
                id,
                time: chrono::DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 21)
                    .unwrap(),
                method: method.into(),
                kind: op.into(),
                status: Some(200),
                latency_ms: 20.0,
                bucket: Some("b".into()),
                key: key.map(Into::into),
                ..Default::default()
            };
            r.lists_mut().query = query
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect();
            r
        };
        // twelve pages of one listing, each the token of the one before
        let pages: Vec<Record> = (1..=12u64)
            .map(|i| {
                let token = format!("t{i}");
                let mut q = vec![("prefix", "in/")];
                if i > 1 {
                    q.push(("continuation-token", token.as_str()));
                }
                at(i, "ListObjectsV2", "GET", None, &q)
            })
            .collect();
        let c = chains(&pages);
        assert_eq!(c[0].levels, (0..12).collect::<Vec<u32>>());
        // and so no finding: none of them could go at once
        let eps = crate::episode::episodes(&pages);
        let found = crate::episode::observations(&pages, &eps);
        assert!(
            found.iter().all(|o| o.identity != "obs.serial|"),
            "{found:?}"
        );
        // ListObjects' marker, and V2's start-after, alike
        for page in ["marker", "start-after"] {
            let rs: Vec<Record> = (1..=12u64)
                .map(|i| {
                    let mut q = vec![("prefix", "in/")];
                    if i > 1 {
                        q.push((page, "in/k"));
                    }
                    at(i, "ListObjects", "GET", None, &q)
                })
                .collect();
            assert_eq!(chains(&rs)[0].levels, (0..12).collect::<Vec<u32>>());
        }
        // twenty parts of one upload sent one at a time: at once after it
        // began, then it completes
        let mut parts = vec![at(
            1,
            "CreateMultipartUpload",
            "POST",
            Some("big"),
            &[("uploads", "")],
        )];
        for n in 1..=20u64 {
            let number = n.to_string();
            parts.push(at(
                n + 1,
                "UploadPart",
                "PUT",
                Some("big"),
                &[("uploadId", "u1"), ("partNumber", number.as_str())],
            ));
        }
        parts.push(at(
            22,
            "CompleteMultipartUpload",
            "POST",
            Some("big"),
            &[("uploadId", "u1")],
        ));
        let c = chains(&parts);
        assert_eq!(c[0].levels, [vec![0], vec![1; 20], vec![2]].concat());
        let eps = crate::episode::episodes(&parts);
        let found = crate::episode::observations(&parts, &eps);
        assert!(
            found.iter().any(|o| o.identity == "obs.serial|"),
            "{found:?}"
        );
        // moves: each a copy, then the delete of its source
        let mut moves = Vec::new();
        for i in 0..6u64 {
            let mut copy = at(
                2 * i + 1,
                "CopyObject",
                "PUT",
                Some(&format!("to/{i}")),
                &[],
            );
            copy.lists_mut().request_headers.push(reqlog::Header::new(
                "x-amz-copy-source",
                format!("/b/from/{i}"),
            ));
            moves.push(copy);
            moves.push(at(
                2 * i + 2,
                "DeleteObject",
                "DELETE",
                Some(&format!("from/{i}")),
                &[],
            ));
        }
        assert_eq!(chains(&moves)[0].levels, [0, 1].repeat(6));
    }
}

#[cfg(test)]
mod prefixes {
    #[test]
    fn a_requests_prefix_is_the_one_its_faults_are_paced_by() {
        let r = reqlog::Record {
            bucket: Some("b".into()),
            key: Some("a/b/c/d/e.parquet".into()),
            ..Default::default()
        };
        assert_eq!(
            super::prefix_of(&r),
            sonde_proxy::fault::rate_prefix(r.bucket.as_deref(), r.key.as_deref())
        );
    }
}
