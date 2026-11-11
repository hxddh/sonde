//! Faults: what the proxy answers in the store's place, so that a program
//! meets the failures S3 documents before production does.
//!
//! A rule matches by operation and key, never by count of arrival ("the third
//! UploadPart" is a race in a concurrent program). What is counted is the
//! attempts at one request ([`identity`]: its method, place and part, and
//! for a request without a key its paging and its body): a rule with
//! `times = 1` fails each request's first attempt and lets its retry through,
//! so a program that retries passes and one that does not is seen not to.
//!
//! Two faults are not an error in the store's place. A body cut short
//! ([`Effect::CutBody`]) is the store's own answer, stopped part way. A lost
//! race ([`Effect::LostRace`]) is a 412 to a conditional write that would
//! have won; it is counted by commit ([`commit_family`]), not by key: the
//! next number in a table's log is the lost commit's retry, and is let
//! through.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// What a rule does to a request it matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Effect {
    /// Answered with this error; the store never sees the request.
    Status { status: u16, code: String },
    /// Sent to the store, and its answer replaced by a 500: the write
    /// happened and the program is not told (the ambiguous write).
    LostAnswer,
    /// Sent to the store after this long.
    Delay { ms: u64 },
    /// S3's documented rate for one prefix: past it, 503 SlowDown.
    Rate { reads: u32, writes: u32 },
    /// A download's answer goes out with its headers and its whole length;
    /// the body stops part way ([`cut_at`]) and the connection drops.
    CutBody,
    /// A conditional write is answered 412 though it would have won. One
    /// with `If-None-Match: *` is sent on first (the object is the
    /// program's own: sonde writes nothing); one with `If-Match` is not.
    LostRace,
}

/// The smallest body [`Effect::CutBody`] cuts: under it there is no half
/// worth the name.
pub const CUT_MIN: u64 = 64;

/// Where a body of `len` bytes is cut: at its half, one byte at least.
/// None for a body too small to cut.
pub fn cut_at(len: u64) -> Option<u64> {
    (len >= CUT_MIN).then(|| (len / 2).max(1))
}

/// The condition a write carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    /// `If-None-Match: *`: only if nothing is there.
    Create,
    /// `If-Match`: only if what is there is what was read.
    Match,
}

/// The condition among a request's headers, if it has one.
pub fn condition<'a>(mut headers: impl Iterator<Item = (&'a str, &'a str)>) -> Option<Condition> {
    headers.find_map(|(k, v)| {
        if k.eq_ignore_ascii_case("if-none-match") && v.trim() == "*" {
            Some(Condition::Create)
        } else if k.eq_ignore_ascii_case("if-match") {
            Some(Condition::Match)
        } else {
            None
        }
    })
}

/// [`condition`] of a recorded request.
pub fn condition_of(r: &reqlog::Record) -> Option<Condition> {
    condition(
        r.request_headers
            .iter()
            .map(|h| (h.name.as_str(), h.value.as_str())),
    )
}

/// The commit a conditional write is an attempt at. The log of a table
/// format names each commit by the next number, so the write that follows a
/// lost one has another key and is still its retry: SlateDB's manifests, WAL
/// and compaction states, Iceberg's table metadata, Delta's `_delta_log`
/// and Lance's `_versions` are each one family for a folder. Any other key
/// is a family of its own (a lock, a pointer written with `If-Match`).
pub fn commit_family(bucket: Option<&str>, key: Option<&str>) -> String {
    use crate::episode::Part;
    let (b, k) = (bucket.unwrap_or(""), key.unwrap_or(""));
    let kind = match crate::episode::part_of(k) {
        Some(Part::SlateManifest(_)) => "manifest",
        Some(Part::SlateWal(_)) => "wal",
        Some(Part::SlateCompactions(_)) => "compactions",
        Some(Part::IcebergMetadata(_)) => "metadata",
        Some(Part::DeltaCommit(_)) => "commit",
        Some(Part::LanceManifest(_)) => "version",
        _ => return format!("{b}/{k}"),
    };
    let dir = k.rfind('/').map_or("", |i| &k[..=i]);
    format!("{b}/{dir}<{kind}>")
}

/// One rule: which requests, what happens to them, how many times each.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// Operation names; one ending in `*` matches by its start. Empty: all.
    #[serde(default)]
    pub on: Vec<String>,
    /// Keys under this prefix only (empty: all; a request without a key
    /// matches only an empty prefix).
    #[serde(default)]
    pub prefix: String,
    pub effect: Effect,
    /// Attempts at one request that meet the fault; 0 is every attempt.
    #[serde(default)]
    pub times: u32,
    /// Writes that carry a condition only (`If-None-Match: *` or
    /// `If-Match`).
    #[serde(default)]
    pub conditional: bool,
}

/// A named set of rules: what one trial run is put through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub name: String,
    /// What it does, in a line.
    pub about: String,
    pub rules: Vec<Rule>,
}

const WRITES: &[&str] = &[
    "PutObject",
    "CopyObject",
    "DeleteObject",
    "DeleteObjects",
    "UploadPart",
    "UploadPartCopy",
    "CompleteMultipartUpload",
];

fn rule(on: &[&str], effect: Effect, times: u32) -> Rule {
    Rule {
        on: on.iter().map(|s| s.to_string()).collect(),
        prefix: String::new(),
        effect,
        times,
        conditional: false,
    }
}

fn status(status: u16, code: &str) -> Effect {
    Effect::Status {
        status,
        code: code.into(),
    }
}

/// The scenarios sonde knows by name.
pub fn scenarios() -> Vec<Scenario> {
    let s = |name: &str, about: &str, rules: Vec<Rule>| Scenario {
        name: name.into(),
        about: about.into(),
        rules,
    };
    vec![
        s(
            "slowdown",
            "503 SlowDown on each request's first attempt",
            vec![rule(&[], status(503, "SlowDown"), 1)],
        ),
        s(
            "error",
            "500 InternalError on each request's first attempt",
            vec![rule(&[], status(500, "InternalError"), 1)],
        ),
        s(
            "lost-answer",
            "each write reaches the store, and its first answer is lost",
            vec![rule(WRITES, Effect::LostAnswer, 1)],
        ),
        s(
            "fail-complete",
            "CompleteMultipartUpload fails once",
            vec![rule(
                &["CompleteMultipartUpload"],
                status(500, "InternalError"),
                1,
            )],
        ),
        s(
            "cut-body",
            "each download's first answer stops half way through its body",
            vec![rule(&["GetObject"], Effect::CutBody, 1)],
        ),
        s(
            "lost-race",
            "each commit's first conditional write is answered 412",
            vec![Rule {
                conditional: true,
                ..rule(&[], Effect::LostRace, 1)
            }],
        ),
        s(
            "slow",
            "every request waits 300 ms more",
            vec![rule(&[], Effect::Delay { ms: 300 }, 0)],
        ),
        s(
            "s3-limits",
            "S3's rate for one prefix: 5,500 reads and 3,500 writes a second",
            vec![rule(
                &[],
                Effect::Rate {
                    reads: 5500,
                    writes: 3500,
                },
                0,
            )],
        ),
    ]
}

/// The query parameters that tell one request without a key from another:
/// which listing, and which page of it.
const PAGING: &[&str] = &[
    "continuation-token",
    "delimiter",
    "key-marker",
    "marker",
    "prefix",
    "start-after",
    "upload-id-marker",
    "uploads",
    "version-id-marker",
    "versions",
];

/// The header a record carries its control body's hash in: the body kept on
/// a record is cut and redacted, the hash is of what was sent.
pub const BODY_HASH_HEADER: &str = "x-sonde-body-sha256";

/// The hash a request's body is told apart by.
pub fn body_hash(body: &[u8]) -> String {
    s3raw::sigv4::sha256_hex(body)
}

/// One request, whichever attempt at it: its method, operation, place and
/// part. An upload's id is the store's and changes when the whole upload is
/// started again, so it is no part of the name. A request without a key
/// (a listing, DeleteObjects, a bucket's setting) is told from the next by
/// its paging and by its body's hash: two batches of deletes are two
/// requests. The faults count attempts by it and the trial reads retries by
/// it: one function, so the two cannot disagree.
pub fn identity(
    method: &str,
    op: &str,
    bucket: Option<&str>,
    key: Option<&str>,
    query: &[(String, String)],
    body_hash: Option<&str>,
) -> String {
    let part = query
        .iter()
        .find(|(k, _)| k == "partNumber")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let mut id = format!(
        "{method} {op} {}/{} {part}",
        bucket.unwrap_or(""),
        key.unwrap_or("")
    );
    if key.is_none() {
        let mut paging: Vec<&(String, String)> = query
            .iter()
            .filter(|(k, _)| PAGING.contains(&k.as_str()))
            .collect();
        paging.sort();
        for (k, v) in paging {
            id.push_str(&format!("&{k}={v}"));
        }
        if let Some(h) = body_hash {
            id.push_str(&format!(" #{h}"));
        }
    }
    id
}

/// The hash a record's control body is told apart by: the one sonde kept
/// with it, or, for a record from elsewhere (an import), the hash of the
/// body itself.
pub fn kept_body_hash(r: &reqlog::Record) -> Option<String> {
    r.request_headers
        .iter()
        // sonde's own is the last: the proxy drops one the program sent, and
        // a record from elsewhere is not trusted to have done so
        .rev()
        .find(|h| h.name.eq_ignore_ascii_case(BODY_HASH_HEADER))
        .map(|h| h.value.to_string())
        .or_else(|| r.request_body.as_deref().map(|b| body_hash(b.as_bytes())))
}

/// [`identity`] of a recorded request.
pub fn identity_of(r: &reqlog::Record) -> String {
    identity(
        &r.method,
        &r.operation,
        r.bucket.as_deref(),
        r.key.as_deref(),
        &r.query,
        kept_body_hash(r).as_deref(),
    )
}

/// The operation names a rule's `on` may say: the ones [`crate::op::classify`]
/// gives a request.
pub fn known_operation(name: &str) -> bool {
    let all = crate::op::operation_names();
    match name.strip_suffix('*') {
        Some(start) => all.iter().any(|n| n.starts_with(start)),
        None => all.iter().any(|n| n == name),
    }
}

/// A scenario by its name.
pub fn scenario(name: &str) -> Option<Scenario> {
    scenarios().into_iter().find(|s| s.name == name)
}

/// A fault as written in `sonde.toml` or on the command line: `503 SlowDown`,
/// `500`, `lost-answer`, `cut-body`, `lost-race`, `delay 200ms`. Nothing
/// may follow it: `503 Slow Down` is a mistake, not a code named `Slow`.
pub fn parse_effect(text: &str) -> Result<Effect, String> {
    let t = text.trim();
    let mut words = t.split_whitespace();
    let first = words.next().unwrap_or("");
    let second = words.next();
    if let Some(extra) = words.next() {
        return Err(format!(
            "{t}: {extra:?} is one word too many (a fault is a status and one code, as in 503 SlowDown, or lost-answer, cut-body, lost-race, delay 200ms)"
        ));
    }
    match (first, second) {
        ("lost-answer", None) => return Ok(Effect::LostAnswer),
        ("cut-body", None) => return Ok(Effect::CutBody),
        ("lost-race", None) => return Ok(Effect::LostRace),
        _ => {}
    }
    if first == "delay" {
        let v = second.ok_or("delay: how long? (delay 200ms)")?;
        let ms = match v.strip_suffix("ms") {
            Some(n) => n.parse::<u64>().ok(),
            None => v
                .strip_suffix('s')
                .and_then(|n| n.parse::<f64>().ok())
                .filter(|s| *s >= 0.0 && *s <= 3600.0)
                .map(|s| (s * 1000.0) as u64),
        };
        return ms
            .map(|ms| Effect::Delay { ms })
            .ok_or_else(|| format!("delay {v}: a time such as 200ms or 2s"));
    }
    if let Ok(code) = first.parse::<u16>() {
        if !(400..600).contains(&code) {
            return Err(format!("{code}: a fault's status is 400 to 599"));
        }
        let name = second.map(str::to_string).unwrap_or_else(|| {
            match code {
                503 => "SlowDown",
                500 => "InternalError",
                403 => "AccessDenied",
                404 => "NoSuchKey",
                412 => "PreconditionFailed",
                _ => "InternalError",
            }
            .to_string()
        });
        return Ok(Effect::Status {
            status: code,
            code: name,
        });
    }
    Err(format!(
        "{t}: a fault is a status (503 SlowDown), lost-answer, cut-body, lost-race, or delay 200ms"
    ))
}

/// What the proxy does with one request.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Answer this error now.
    Answer { status: u16, code: String },
    /// Send it on, then answer 500 whatever the store said.
    LoseAnswer,
    /// Wait, then send it on.
    Wait(Duration),
    /// Send it on and stop the answer's body part way. An answer that
    /// cannot be cut (an error, a body too small) was no attempt met:
    /// [`Faults::unmeet`] takes `met` back.
    CutBody { met: Met },
    /// Answer 412. `forward`: the write is sent on first (`If-None-Match:
    /// *`), and a store that refused it itself was no attempt met either.
    LoseRace { forward: bool, met: Met },
}

/// What a verdict counted, to be taken back when the fault could not be
/// done ([`Faults::unmeet`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Met(Option<(usize, String)>);

/// One request, as the faults see it.
#[derive(Debug, Clone, Copy)]
pub struct Asked<'a> {
    pub method: &'a str,
    pub op: &'a str,
    pub bucket: Option<&'a str>,
    pub key: Option<&'a str>,
    pub query: &'a [(String, String)],
    /// Its control body's hash ([`body_hash`]), when it has one.
    pub body_hash: Option<&'a str>,
    pub headers: &'a [(String, String)],
}

impl<'a> Asked<'a> {
    /// A request with nothing but its method, operation and place.
    pub fn new(
        method: &'a str,
        op: &'a str,
        bucket: Option<&'a str>,
        key: Option<&'a str>,
    ) -> Self {
        Asked {
            method,
            op,
            bucket,
            key,
            query: &[],
            body_hash: None,
            headers: &[],
        }
    }
}

/// A scenario's rules at work in one proxy, with what they have counted.
pub struct Faults {
    pub name: String,
    rules: Vec<Rule>,
    /// Attempts met, by rule and request.
    met: Mutex<HashMap<(usize, String), u32>>,
    /// When the requests of the last second came, by rule, prefix and kind.
    rate: Mutex<HashMap<(usize, String, bool), VecDeque<Instant>>>,
}

fn matches_op(on: &[String], op: &str) -> bool {
    on.is_empty()
        || on.iter().any(|p| match p.strip_suffix('*') {
            Some(start) => op.starts_with(start),
            None => p == op,
        })
}

/// A key's prefix for the rate: the bucket and the key's folder, three
/// levels at most, as the rate finding has it
/// ([`crate::profile::prefix_of`]). S3 partitions by load and tells no one
/// where; the folder is the reading both keep to.
pub fn rate_prefix(bucket: Option<&str>, key: Option<&str>) -> String {
    let b = bucket.unwrap_or("");
    let k = key.unwrap_or("");
    let dir = k.rfind('/').map_or("", |i| &k[..=i]);
    let short: String = dir.split_inclusive('/').take(3).collect();
    match (b.is_empty(), short.is_empty()) {
        (true, true) => "/".into(),
        (true, false) => short,
        _ => format!("{b}/{short}"),
    }
}

impl Faults {
    pub fn new(scenario: &Scenario) -> Self {
        Faults {
            name: scenario.name.clone(),
            rules: scenario.rules.clone(),
            met: Mutex::new(HashMap::new()),
            rate: Mutex::new(HashMap::new()),
        }
    }

    /// What happens to this request: the first rule that matches and still
    /// has attempts to meet.
    pub fn decide(&self, a: &Asked<'_>) -> Option<Verdict> {
        self.decide_at(a, Instant::now())
    }

    /// Takes back an attempt a verdict counted: the fault could not be done.
    pub fn unmeet(&self, met: &Met) {
        if let Some(key) = &met.0
            && let Some(n) = self.met.lock().get_mut(key)
        {
            *n = n.saturating_sub(1);
        }
    }

    fn decide_at(&self, a: &Asked<'_>, now: Instant) -> Option<Verdict> {
        let Asked {
            method,
            op,
            bucket,
            key,
            ..
        } = *a;
        let condition = condition(a.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        let write = matches!(method, "PUT" | "POST" | "DELETE");
        for (i, r) in self.rules.iter().enumerate() {
            if !matches_op(&r.on, op) {
                continue;
            }
            if !r.prefix.is_empty() && !key.is_some_and(|k| k.starts_with(&r.prefix)) {
                continue;
            }
            if (r.conditional || r.effect == Effect::LostRace) && !(write && condition.is_some()) {
                continue;
            }
            if r.effect == Effect::CutBody && method != "GET" {
                continue;
            }
            if let Effect::Rate { reads, writes } = r.effect {
                let read = method == "GET" || method == "HEAD";
                let limit = if read { reads } else { writes };
                let mut rate = self.rate.lock();
                let recent = rate.entry((i, rate_prefix(bucket, key), read)).or_default();
                // the second that ends now, not the clock's: a burst across
                // a second's edge is one burst
                while recent
                    .front()
                    .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(1))
                {
                    recent.pop_front();
                }
                if recent.len() >= limit as usize {
                    return Some(Verdict::Answer {
                        status: 503,
                        code: "SlowDown".into(),
                    });
                }
                recent.push_back(now);
                continue;
            }
            let mut counted = Met(None);
            if r.times > 0 {
                // a lost race is counted by commit: the next number in the
                // log is this commit's retry
                let id = if r.effect == Effect::LostRace {
                    commit_family(bucket, key)
                } else {
                    identity(method, op, bucket, key, a.query, a.body_hash)
                };
                let mut met = self.met.lock();
                let n = met.entry((i, id.clone())).or_insert(0);
                if *n >= r.times {
                    continue;
                }
                *n += 1;
                counted = Met(Some((i, id)));
            }
            return Some(match &r.effect {
                Effect::Status { status, code } => Verdict::Answer {
                    status: *status,
                    code: code.clone(),
                },
                Effect::LostAnswer => Verdict::LoseAnswer,
                Effect::Delay { ms } => Verdict::Wait(Duration::from_millis(*ms)),
                Effect::CutBody => Verdict::CutBody { met: counted },
                Effect::LostRace => Verdict::LoseRace {
                    forward: condition == Some(Condition::Create),
                    met: counted,
                },
                Effect::Rate { .. } => unreachable!("handled above"),
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(f: &Faults, method: &str, op: &str, key: &str) -> Option<Verdict> {
        f.decide(&Asked::new(method, op, Some("b"), Some(key)))
    }

    #[test]
    fn each_requests_first_attempt_fails_and_its_retry_passes() {
        let f = Faults::new(&scenario("slowdown").unwrap());
        let slow = Some(Verdict::Answer {
            status: 503,
            code: "SlowDown".into(),
        });
        assert_eq!(decide(&f, "PUT", "PutObject", "a"), slow);
        assert_eq!(decide(&f, "PUT", "PutObject", "a"), None, "the retry");
        // another request, whenever it arrives, meets it once too
        assert_eq!(decide(&f, "PUT", "PutObject", "b"), slow);
        assert_eq!(decide(&f, "GET", "GetObject", "a"), slow);
        assert_eq!(decide(&f, "GET", "GetObject", "a"), None);
    }

    #[test]
    fn parts_of_one_upload_are_requests_of_their_own() {
        let f = Faults::new(&scenario("error").unwrap());
        let part = |n: &str| {
            f.decide(&Asked {
                query: &[
                    ("partNumber".into(), n.into()),
                    ("uploadId".into(), "u".into()),
                ],
                ..Asked::new("PUT", "UploadPart", Some("b"), Some("k"))
            })
        };
        assert!(part("1").is_some() && part("2").is_some());
        assert!(part("1").is_none() && part("2").is_none());
    }

    #[test]
    fn a_rule_is_for_its_operations_and_its_prefix() {
        let f = Faults::new(&Scenario {
            name: "t".into(),
            about: String::new(),
            rules: vec![Rule {
                on: vec!["Put*".into()],
                prefix: "wal/".into(),
                effect: Effect::LostAnswer,
                times: 0,
                conditional: false,
            }],
        });
        assert_eq!(
            decide(&f, "PUT", "PutObject", "wal/1"),
            Some(Verdict::LoseAnswer)
        );
        assert_eq!(
            decide(&f, "PUT", "PutObject", "wal/1"),
            Some(Verdict::LoseAnswer)
        );
        assert_eq!(decide(&f, "PUT", "PutObject", "sst/1"), None);
        assert_eq!(decide(&f, "GET", "GetObject", "wal/1"), None);
    }

    #[test]
    fn past_a_prefixes_rate_the_answer_is_slow_down() {
        let f = Faults::new(&Scenario {
            name: "t".into(),
            about: String::new(),
            rules: vec![rule(
                &[],
                Effect::Rate {
                    reads: 2,
                    writes: 1,
                },
                0,
            )],
        });
        assert_eq!(decide(&f, "GET", "GetObject", "p/a"), None);
        assert_eq!(decide(&f, "GET", "GetObject", "p/b"), None);
        assert!(decide(&f, "GET", "GetObject", "p/c").is_some());
        assert_eq!(
            decide(&f, "GET", "GetObject", "q/a"),
            None,
            "another prefix"
        );
        assert_eq!(
            decide(&f, "PUT", "PutObject", "p/a"),
            None,
            "writes have their own"
        );
        assert!(decide(&f, "PUT", "PutObject", "p/b").is_some());
    }

    #[test]
    fn two_batches_of_deletes_and_two_pages_of_a_listing_are_requests_of_their_own() {
        let f = Faults::new(&scenario("slowdown").unwrap());
        let batch = |body: &str| {
            f.decide(&Asked {
                query: &[("delete".into(), String::new())],
                body_hash: Some(&body_hash(body.as_bytes())),
                ..Asked::new("POST", "DeleteObjects", Some("b"), None)
            })
        };
        assert!(batch("<Delete>a</Delete>").is_some());
        assert!(
            batch("<Delete>b</Delete>").is_some(),
            "the second batch is not the first one's retry"
        );
        assert!(batch("<Delete>a</Delete>").is_none(), "its retry is");
        let page = |q: &[(&str, &str)]| {
            let q: Vec<(String, String)> = q
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            f.decide(&Asked {
                query: &q,
                ..Asked::new("GET", "ListObjectsV2", Some("b"), None)
            })
        };
        assert!(page(&[("list-type", "2"), ("prefix", "p/")]).is_some());
        for marker in ["start-after", "key-marker", "marker", "continuation-token"] {
            assert!(
                page(&[("list-type", "2"), ("prefix", "p/"), (marker, "k9")]).is_some(),
                "a page by {marker} is another request"
            );
        }
        assert!(page(&[("list-type", "2"), ("prefix", "p/"), ("delimiter", "/")]).is_some());
        // the order of the query is not the request
        assert!(page(&[("prefix", "p/"), ("list-type", "2")]).is_none());
    }

    #[test]
    fn a_record_is_named_as_the_fault_named_its_request() {
        let mut r = reqlog::Record {
            method: "POST".into(),
            operation: "DeleteObjects".into(),
            bucket: Some("b".into()),
            query: vec![("delete".into(), String::new())],
            request_body: Some("cut and redac…".into()),
            ..Default::default()
        };
        let sent = body_hash(b"<Delete>the whole of it</Delete>");
        r.request_headers
            .push(reqlog::Header::new(BODY_HASH_HEADER, &sent));
        assert_eq!(
            identity_of(&r),
            identity(
                "POST",
                "DeleteObjects",
                Some("b"),
                None,
                &r.query,
                Some(&sent)
            )
        );
        // a key's request is its key: the body (a part's ETags) is not its name
        r.key = Some("k".into());
        assert!(!identity_of(&r).contains(&sent));
    }

    #[test]
    fn the_rate_is_of_the_second_that_ends_now_and_of_the_findings_prefix() {
        let f = Faults::new(&Scenario {
            name: "t".into(),
            about: String::new(),
            rules: vec![rule(
                &[],
                Effect::Rate {
                    reads: 2,
                    writes: 2,
                },
                0,
            )],
        });
        let t0 = Instant::now();
        let at = |ms: u64, key: &str| {
            f.decide_at(
                &Asked::new("GET", "GetObject", Some("b"), Some(key)),
                t0 + Duration::from_millis(ms),
            )
        };
        // two at the end of one clock second, two at the start of the next:
        // four within 200 ms is past two a second
        assert_eq!(at(900, "p/a"), None);
        assert_eq!(at(950, "p/b"), None);
        assert!(at(1050, "p/c").is_some(), "a fixed window would let it by");
        assert!(at(1899, "p/d").is_some());
        assert_eq!(at(1901, "p/e"), None, "the first has left the second");
        // the prefix is the finding's: three levels of folder at most
        let r = reqlog::Record {
            bucket: Some("b".into()),
            key: Some("a/b/c/d/e.parquet".into()),
            ..Default::default()
        };
        assert_eq!(
            rate_prefix(r.bucket.as_deref(), r.key.as_deref()),
            crate::profile::prefix_of(&r)
        );
        assert_eq!(rate_prefix(Some("b"), Some("a/b/c/d/e")), "b/a/b/c/");
        assert_eq!(rate_prefix(Some("b"), Some("top")), "b/");
    }

    #[test]
    fn an_operation_name_is_one_a_request_can_have() {
        assert!(known_operation("PutObject") && known_operation("Put*"));
        assert!(known_operation("GetBucketLifecycleConfiguration"));
        assert!(known_operation("DeleteObjects") && known_operation("ListObjectsV2"));
        assert!(!known_operation("PutObjekt") && !known_operation("Putt*"));
        assert!(!known_operation(""));
    }

    #[test]
    fn faults_as_written() {
        assert_eq!(
            parse_effect("503 SlowDown").unwrap(),
            status(503, "SlowDown")
        );
        assert_eq!(parse_effect("500").unwrap(), status(500, "InternalError"));
        assert_eq!(parse_effect("lost-answer").unwrap(), Effect::LostAnswer);
        assert_eq!(
            parse_effect("delay 200ms").unwrap(),
            Effect::Delay { ms: 200 }
        );
        assert_eq!(
            parse_effect("delay 2s").unwrap(),
            Effect::Delay { ms: 2000 }
        );
        assert!(parse_effect("200").is_err());
        assert!(parse_effect("delay").is_err());
        assert!(parse_effect("explode").is_err());
        assert_eq!(parse_effect("cut-body").unwrap(), Effect::CutBody);
        assert_eq!(parse_effect("lost-race").unwrap(), Effect::LostRace);
        // a code is one word: nothing is dropped in silence
        let e = parse_effect("503 Slow Down extra").unwrap_err();
        assert!(e.contains("one word too many"), "{e}");
        assert!(parse_effect("503 Slow Down").is_err());
        assert!(parse_effect("lost-answer twice").is_err());
        assert!(parse_effect("delay 200ms more").is_err());
    }

    #[test]
    fn a_body_is_cut_at_its_half_and_a_small_one_not_at_all() {
        assert_eq!(cut_at(1000), Some(500));
        assert_eq!(cut_at(CUT_MIN), Some(CUT_MIN / 2));
        assert_eq!(cut_at(CUT_MIN - 1), None);
        assert_eq!(cut_at(0), None);
        // a download is cut once, whatever range it asks for next: the
        // range is no part of its name, or the resume would be cut again
        let f = Faults::new(&scenario("cut-body").unwrap());
        let get = |range: &str| {
            let h = [("range".to_string(), range.to_string())];
            f.decide(&Asked {
                headers: &h,
                ..Asked::new("GET", "GetObject", Some("b"), Some("k"))
            })
        };
        let first = get("bytes=0-");
        assert!(matches!(first, Some(Verdict::CutBody { .. })), "{first:?}");
        assert_eq!(get("bytes=500-"), None, "the resume");
        // a cut that could not be made is taken back
        if let Some(Verdict::CutBody { met }) = first {
            f.unmeet(&met);
        }
        assert!(get("bytes=0-").is_some());
        assert_eq!(decide(&f, "HEAD", "HeadObject", "k"), None);
        assert_eq!(decide(&f, "PUT", "PutObject", "k"), None);
    }

    #[test]
    fn a_race_is_lost_once_for_each_commit_not_for_each_key() {
        let f = Faults::new(&scenario("lost-race").unwrap());
        let put = |key: &str, header: Option<(&str, &str)>| {
            let h: Vec<(String, String)> = header
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            f.decide(&Asked {
                headers: &h,
                ..Asked::new("PUT", "PutObject", Some("b"), Some(key))
            })
        };
        let create = Some(("If-None-Match", "*"));
        // a write without a condition races no one
        assert_eq!(put("t/_delta_log/00000000000000000001.json", None), None);
        let lost = put("t/_delta_log/00000000000000000001.json", create);
        assert!(
            matches!(lost, Some(Verdict::LoseRace { forward: true, .. })),
            "{lost:?}"
        );
        // the next number is that commit's retry
        assert_eq!(put("t/_delta_log/00000000000000000002.json", create), None);
        // another table's log is another commit
        assert!(put("u/_delta_log/00000000000000000001.json", create).is_some());
        // If-Match is answered without being sent on, and once for its key
        let swap = put("lock", Some(("if-match", "\"abc\"")));
        assert!(
            matches!(swap, Some(Verdict::LoseRace { forward: false, .. })),
            "{swap:?}"
        );
        assert_eq!(put("lock", Some(("if-match", "\"abc\""))), None);
        // If-None-Match with an ETag is a read's condition, not a create
        assert_eq!(put("other", Some(("if-none-match", "\"abc\""))), None);
    }

    #[test]
    fn a_commit_is_named_by_its_tables_log_whatever_its_number() {
        let fam = |k: &str| commit_family(Some("b"), Some(k));
        assert_eq!(
            fam("db/manifest/00000000000000000003.manifest"),
            fam("db/manifest/00000000000000000004.manifest")
        );
        assert_ne!(
            fam("db/manifest/00000000000000000003.manifest"),
            fam("db/wal/00000000000000000003.sst")
        );
        assert_eq!(
            fam("t/metadata/00003-6a1f.metadata.json"),
            fam("t/metadata/00004-77b2.metadata.json")
        );
        // Lance counts down from u64's end
        assert_eq!(
            fam("d.lance/_versions/18446744073709551613.manifest"),
            fam("d.lance/_versions/18446744073709551612.manifest")
        );
        assert_ne!(fam("a/_delta_log/00000000000000000001.json"), fam("a/x"));
        assert_ne!(fam("lock-a"), fam("lock-b"));
    }
}
