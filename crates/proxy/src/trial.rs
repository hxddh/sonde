//! A trial: one run of a program under a fault scenario, and what the
//! program did about it, read from what the store (and sonde in its place)
//! saw. "Survived" claims only what can be seen from here: every failed
//! request was sent again, nothing more was left behind than a clean run
//! leaves, and the program exited as its clean run did.
//!
//! A retry is counted against the clean run. A request is named by
//! [`crate::fault::identity`]; the clean run says how many times the program
//! means to send each. A failed write was sent again only if the trial run
//! got at least that many answers for it: a program that writes `state`,
//! waits and writes `state` again did not retry its first write because a
//! second one followed. A failed read was made good by any answer after it;
//! when the clean run reads the key more than once (a poll), that answer is
//! called a retry only if it came sooner than the program's own rhythm, and
//! "read again later" otherwise: which of the two it was cannot be seen.
//!
//! Each run of a trial is given a place of its own by its name (`{trial}`,
//! `SONDE_TRIAL`: `clean`, then the scenario's), so the two runs' keys
//! differ by that name. Before a request is matched to the clean run the
//! run's own name is taken out of its bucket, key, query and control body
//! ([`unplace`]): only where it stands as a whole word, so that a scenario
//! named `error` leaves a key `errors/x` alone. A key that holds the word
//! by itself in both runs (`error/x` under the scenario `error`) is matched
//! as it was written.

use std::collections::{BTreeMap, HashMap};

use reqlog::Record;
use serde::{Deserialize, Serialize};

use crate::episode::{episodes, observations, status_owes_retry};
use crate::fault::{Condition, commit_family, condition_of, identity, identity_of};
use crate::program::ms_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Standing {
    /// Every fault was retried, the exit is the clean run's, nothing left.
    Survived,
    /// It finished, but gave up on a read or left something behind.
    Degraded,
    /// It exited with an error, or dropped a write and said nothing.
    Broke,
    /// The scenario met no request (nothing to complete, nothing to write).
    Untested,
}

impl Standing {
    pub fn label(self) -> &'static str {
        match self {
            Standing::Survived => "survived",
            Standing::Degraded => "degraded",
            Standing::Broke => "broke",
            Standing::Untested => "untested",
        }
    }
}

/// The request a verdict turns on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Where {
    pub operation: String,
    /// `bucket/key`, or the bucket and what was listed.
    pub key: String,
    /// The line of the program that sent it, when the run named callers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
}

/// A sentence as a phrase: a key of [`crate::phrase`] and its arguments,
/// for the app to say in the reader's language.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Said {
    pub key: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
}

impl Said {
    fn new(key: &str, args: &[(&str, String)]) -> Said {
        Said {
            key: key.into(),
            args: args
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    /// The sentence in English.
    pub fn en(&self) -> String {
        crate::phrase::en_of(&self.key, &self.args)
    }
}

/// What one scenario did to one program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trial {
    pub scenario: String,
    pub about: String,
    pub standing: Standing,
    /// The program's exit code under the scenario, and without it.
    pub exit: i32,
    pub clean_exit: i32,
    /// Requests sonde failed in the store's place (bodies cut and races
    /// lost among them).
    pub faults: usize,
    /// Of those, the ones the program sent again (see the module's note).
    pub retried: usize,
    /// Of the faults, the ones no retry is owed for: a 404, a 403, a 501
    /// (any status but 408, 429, 500, 502, 503 and 504), and a request the
    /// clean run itself never got an answer for. Neither retried nor dropped.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub not_retryable: usize,
    /// Failed reads of a key the clean run reads more than once, answered
    /// later: a retry or the program's next round, not told apart.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub read_again: usize,
    /// Reads and writes it never sent again.
    pub dropped_reads: usize,
    pub dropped_writes: usize,
    /// Of the dropped writes, the ones the store made all the same: their
    /// answer was lost, and the program did not ask again.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub lost_writes: usize,
    /// Downloads whose body sonde cut short; of them, the ones made good by
    /// a read from the cut on (`resumed`) and by a read of the whole again
    /// (`reread`). The rest were taken as they came.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cut: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub resumed: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub reread: usize,
    /// Conditional writes answered 412 though they would have won.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub races: usize,
    /// Of the races, the ones whose write the store had made (sonde let it
    /// through) and which the program read back and did not write again,
    /// making every other commit it meant to: the store holds what the
    /// clean run's holds. Whether the program knew its own write, or would
    /// have gone on the same over another writer's, the requests do not say.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub read_back: usize,
    /// A SlateDB writer that lost its race stopped and wrote nothing more:
    /// fenced, as the format means it. Its exit is not held against it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub fenced: bool,
    /// The most attempts at one request: its failures in a row, and the
    /// attempt that followed them.
    pub attempts: u32,
    /// The waits before a retry, in milliseconds: shortest, median, longest.
    /// Only waits within one sender: none for a request several senders
    /// had in flight at once (whose next attempt is whose cannot be seen).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backoff_ms: Option<[f64; 3]>,
    /// Whether the waits before one attempt number differ enough to be
    /// jittered (retries in step make a throttled store's load arrive in
    /// waves). Not said with fewer than four waits before the same attempt,
    /// nor of waits under [`JITTER_FLOOR_MS`] (`jitter_too_short`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<bool>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub jitter_too_short: bool,
    /// Requests under the scenario and in the clean run.
    pub requests: usize,
    pub clean_requests: usize,
    /// Requests the scenario delayed (and did not fail).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub delayed: usize,
    /// How long the run took, and the clean run, in milliseconds: the
    /// command's own time when `sonde run` measured it, else from the first
    /// request to the last answer.
    #[serde(default)]
    pub wall_ms: f64,
    #[serde(default)]
    pub clean_wall_ms: f64,
    /// Multipart uploads left unfinished, beyond the clean run's.
    pub left_uploads: usize,
    /// Writes the store made twice: made, their answer lost, made again.
    /// Not deletes (a delete made twice removed once), nor creates sent
    /// with `If-None-Match: *` (the store was asked not to).
    pub wrote_twice: usize,
    /// Why it is not "survived", a line each.
    pub reasons: Vec<String>,
    /// The request the verdict turns on, when it is not "survived": the
    /// first write never sent again, the first body taken cut, the first
    /// race not run again, else the first read given up or (when only the
    /// exit differs) the last request that failed.
    #[serde(rename = "where", default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Where>,
    /// What to change, where the requests say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advice: Option<Said>,
    /// The trial run's session: its file's name in the run's `trials/`
    /// folder ([`session_path`] gives the path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Under it a wait is the machine's timing as much as the program's: no
/// jitter is read from waits this short.
pub const JITTER_FLOOR_MS: f64 = 50.0;

fn response_header<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.response_headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.trim())
}

/// Whether sonde failed this request (a delay is injected too, and is not
/// a failure).
pub fn faulted(r: &Record) -> bool {
    r.injected.is_some() && response_header(r, "x-sonde-fault").is_some()
}

/// What the store answered a request whose answer sonde lost.
pub fn store_status(r: &Record) -> Option<u16> {
    response_header(r, "x-sonde-store-status").and_then(|v| v.parse().ok())
}

/// The store did what was asked, and the program was told it failed.
fn lost_answer(r: &Record) -> bool {
    store_status(r).is_some_and(|s| (200..300).contains(&s))
}

fn is_write(r: &Record) -> bool {
    matches!(r.method.as_str(), "PUT" | "POST" | "DELETE")
}

/// A download whose body sonde cut short.
fn cut_body(r: &Record) -> bool {
    faulted(r) && response_header(r, crate::CUT_HEADER).is_some()
}

/// A conditional write sonde answered 412.
fn lost_race(r: &Record) -> bool {
    faulted(r) && r.status == Some(412) && is_write(r) && condition_of(r).is_some()
}

/// The program got the store's own answer, and not one that asks for
/// another attempt: a 404, a 412 or a 501 is an answer, a 503 is not.
fn answered(r: &Record) -> bool {
    !faulted(r) && r.transport_error.is_none() && r.status.is_some_and(|s| !status_owes_retry(s))
}

fn done(r: &Record) -> bool {
    answered(r) && r.status.is_some_and(|s| (200..300).contains(&s))
}

/// Whether a failed request is one a program should send again
/// ([`status_owes_retry`]: the rule the retries read from a session go by).
/// A 404 or a 501 is the answer, however often it is asked.
fn retry_owed(r: &Record) -> bool {
    r.status.is_none_or(status_owes_retry)
}

fn s3_code(r: &Record) -> Option<&str> {
    let b = r.error_body.as_deref()?;
    let start = b.find("<Code>")? + 6;
    Some(&b[start..start + b[start..].find("</Code>")?])
}

fn unfinished_uploads(records: &[Record]) -> usize {
    let eps = episodes(records);
    observations(records, &eps)
        .iter()
        .find(|o| o.phrases.first().is_some_and(|p| p.key == "obs.unfinished"))
        .and_then(|o| o.phrases[0].args.get("n")?.parse().ok())
        .unwrap_or(0)
}

fn end_of(r: &Record) -> chrono::DateTime<chrono::Utc> {
    r.time + chrono::Duration::microseconds(r.latency_us())
}

fn ms_between(a: chrono::DateTime<chrono::Utc>, b: chrono::DateTime<chrono::Utc>) -> f64 {
    (b - a).num_microseconds().unwrap_or(0) as f64 / 1000.0
}

/// From the first request's start to the last answer, in ms.
fn span_ms(records: &[Record]) -> f64 {
    let start = records.iter().map(|r| r.time).min();
    let end = records.iter().map(end_of).max();
    match (start, end) {
        (Some(s), Some(e)) => ms_between(s, e),
        _ => 0.0,
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Whether waits before one attempt number are jittered: a quarter of them
/// or more are a tenth off their median.
fn jittered(waits: &[f64], mid: f64) -> bool {
    waits
        .iter()
        .filter(|w| (*w - mid).abs() > mid.max(1.0) * 0.1)
        .count()
        * 4
        >= waits.len()
}

/// What a run's name stands as in a request once it is taken out.
const PLACE: &str = "{trial}";

/// `text` without the run's own `name` where it stands as a whole word
/// (no letter or digit on either side): `etl-clean/state` is
/// `etl-{trial}/state`, and `cleaned/x` is left as it is.
pub fn unplace<'a>(text: &'a str, name: &str) -> std::borrow::Cow<'a, str> {
    if name.is_empty() || !text.contains(name) {
        return text.into();
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut before: Option<char> = None;
    while let Some(i) = rest.find(name) {
        let after = rest[i + name.len()..].chars().next();
        let prev = rest[..i].chars().next_back().or(before);
        out.push_str(&rest[..i]);
        if word(prev) || word(after) {
            out.push_str(name);
        } else {
            out.push_str(PLACE);
        }
        before = name.chars().next_back();
        rest = &rest[i + name.len()..];
    }
    out.push_str(rest);
    out.into()
}

/// A request's name ([`identity_of`]) with its run's own name taken out.
fn identity_without(r: &Record, name: &str) -> String {
    let query: Vec<(String, String)> = r
        .query
        .iter()
        .map(|(k, v)| (k.clone(), unplace(v, name).into_owned()))
        .collect();
    // the body's hash is of what was sent: with the name in it, it is the
    // hash of what is kept, the name taken out (a body cut short when it
    // was kept is left to its hash)
    let body = r
        .request_body
        .as_deref()
        .filter(|b| r.key.is_none() && !r.request_body_truncated && b.contains(name))
        .map(|b| crate::fault::body_hash(unplace(b, name).as_bytes()))
        .or_else(|| crate::fault::kept_body_hash(r));
    identity(
        &r.method,
        &r.operation,
        r.bucket.as_deref().map(|b| unplace(b, name)).as_deref(),
        r.key.as_deref().map(|k| unplace(k, name)).as_deref(),
        &query,
        body.as_deref(),
    )
}

fn family_without(r: &Record, name: &str) -> String {
    commit_family(
        r.bucket.as_deref().map(|b| unplace(b, name)).as_deref(),
        r.key.as_deref().map(|k| unplace(k, name)).as_deref(),
    )
}

/// The clean run's requests of one name.
#[derive(Default)]
struct Meant {
    /// The ones the store answered: what the program means to get.
    answered: usize,
    /// Two of them were in flight at once: several senders.
    concurrent: bool,
    /// The shortest wait between one's end and the next one's start, in ms.
    rhythm: Option<f64>,
    last_end: Option<chrono::DateTime<chrono::Utc>>,
}

/// The bytes of the object an answered download carried, first and last.
fn carried(r: &Record) -> Option<(u64, u64)> {
    if let Some(range) = response_header(r, "content-range") {
        let (span, _) = range.strip_prefix("bytes ")?.split_once('/')?;
        let (a, b) = span.split_once('-')?;
        return Some((a.trim().parse().ok()?, b.trim().parse().ok()?));
    }
    let len: u64 = response_header(r, "content-length")
        .and_then(|v| v.parse().ok())
        .or(r.response_bytes)?;
    (len > 0).then(|| (0, len - 1))
}

/// What became of a body cut short.
enum Cut {
    /// A later download carried from the cut on.
    Resumed,
    /// A later download carried the whole of it again.
    Reread,
    /// Later downloads carried some of what was missing, not all.
    Hole,
    /// Nothing after it carried what was missing.
    Taken,
}

/// Reads what the downloads after `cut` (answered, of the same object)
/// made of it.
fn cut_outcome(cut: &Record, later: &[&Record]) -> Cut {
    let Some((first, last)) = carried(cut) else {
        return Cut::Taken;
    };
    let sent = cut
        .response_bytes
        .or_else(|| response_header(cut, crate::CUT_HEADER)?.parse().ok())
        .unwrap_or(0);
    let from = first + sent;
    let mut spans: Vec<(u64, u64)> = later
        .iter()
        .filter_map(|r| carried(r))
        .filter(|(a, b)| *a <= last && *b >= from)
        .collect();
    if spans.is_empty() {
        return Cut::Taken;
    }
    spans.sort_unstable();
    let whole = spans.iter().any(|(a, b)| *a <= first && *b >= last);
    // what is missing, covered from its start on without a gap
    let mut upto = from;
    for (a, b) in spans {
        if a > upto {
            break;
        }
        upto = upto.max(b + 1);
    }
    if upto <= last {
        Cut::Hole
    } else if whole {
        Cut::Reread
    } else {
        Cut::Resumed
    }
}

/// `bucket/key` of a request, or its bucket and what it listed.
fn place_of(r: &Record) -> String {
    let b = r.bucket.as_deref().unwrap_or("");
    match &r.key {
        Some(k) => format!("{b}/{k}"),
        None => {
            let prefix = r
                .query
                .iter()
                .find(|(k, _)| k == "prefix")
                .map_or("", |(_, v)| v.as_str());
            format!("{b}/{prefix}")
        }
    }
}

fn where_of(r: &Record) -> Where {
    Where {
        operation: r.operation.clone(),
        key: place_of(r),
        caller: crate::callers::caller(r),
    }
}

/// The SDK's own setting that turns retries on ([`crate::fixes::retries_on`]).
fn retry_setting(sdk: &str) -> Option<&'static str> {
    let name = sdk.split(' ').next().unwrap_or("");
    crate::fixes::retries_on(match name {
        "boto3" | "botocore" | "aws-cli" | "s3fs" => "python",
        "aws-sdk-java" | "hadoop-aws" | "hadoop" => "java",
        "aws-sdk-go-v2" | "aws-sdk-go" => "go",
        "aws-sdk-js" => "js",
        _ => return None,
    })
}

/// Reads the trial: `clean` is the run without faults, `run` the one with.
pub fn judge(
    scenario: &crate::fault::Scenario,
    clean: &[Record],
    clean_exit: i32,
    run: &[Record],
    exit: i32,
) -> Trial {
    let name = scenario.name.as_str();
    // how many times the program means to send each request, named without
    // the clean run's place; and the same requests as they were written
    let mut meant: HashMap<String, Meant> = HashMap::new();
    let mut as_written: HashMap<String, String> = HashMap::new();
    let mut won: HashMap<String, usize> = HashMap::new();
    let mut in_time: Vec<&Record> = clean.iter().collect();
    in_time.sort_by_key(|r| (r.time, r.id));
    for r in in_time {
        let id = identity_without(r, "clean");
        let raw = identity_of(r);
        if raw != id {
            as_written.insert(raw, id.clone());
        }
        let m = meant.entry(id).or_default();
        m.answered += answered(r) as usize;
        if let Some(end) = m.last_end {
            let gap = ms_between(end, r.time);
            if gap < 0.0 {
                m.concurrent = true;
            } else {
                m.rhythm = Some(m.rhythm.map_or(gap, |g| g.min(gap)));
            }
        }
        m.last_end = Some(m.last_end.map_or(end_of(r), |e| e.max(end_of(r))));
        if is_write(r) && condition_of(r).is_some() && done(r) {
            *won.entry(family_without(r, "clean")).or_default() += 1;
        }
    }
    // each of the trial's requests under the clean run's name for it
    let named = |r: &Record| -> String {
        let id = identity_without(r, name);
        if meant.contains_key(&id) {
            return id;
        }
        let raw = identity_of(r);
        match as_written.get(&raw) {
            Some(clean_id) => clean_id.clone(),
            None if meant.contains_key(&raw) => raw,
            None => id,
        }
    };
    let mut by_request: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    let mut families: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for r in run {
        if is_write(r) && condition_of(r).is_some() {
            families.entry(family_without(r, name)).or_default().push(r);
        }
        by_request.entry(named(r)).or_default().push(r);
    }
    let (mut faults, mut retried, mut not_retryable, mut read_again) = (0, 0, 0, 0);
    let (mut dropped_reads, mut dropped_writes, mut lost_writes) = (0, 0, 0);
    let (mut attempts, mut wrote_twice) = (0u32, 0);
    let (mut cut, mut resumed, mut reread, mut holes, mut taken) = (0, 0, 0, 0, 0);
    // (which attempt the wait came before, the wait)
    let mut waits: Vec<(u32, f64)> = Vec::new();
    // retries of a write the store had made, which the store then refused
    let mut refused: Vec<String> = Vec::new();
    // the first request of each kind the verdict may turn on
    let mut at_write: Option<&Record> = None;
    let mut at_cut: Option<&Record> = None;
    let mut at_read: Option<&Record> = None;
    fn keep<'a>(slot: &mut Option<&'a Record>, r: &'a Record) {
        if slot.is_none_or(|s| (r.time, r.id) < (s.time, s.id)) {
            *slot = Some(r);
        }
    }
    for (id, group) in by_request.iter_mut() {
        group.sort_by_key(|r| (r.time, r.id));
        let m = meant.get(id);
        let want = m.map_or(0, |m| m.answered);
        // several senders had this request in flight at once, here or in
        // the clean run: whose attempt follows whose cannot be seen
        let concurrent =
            m.is_some_and(|m| m.concurrent) || group.windows(2).any(|w| w[1].time < end_of(w[0]));
        // the wait from a failure to the attempt after it, within a sender
        let mut wait = |streak: u32, r: &Record, next: &Record| {
            let same_place = match (crate::callers::caller(r), crate::callers::caller(next)) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            };
            if !concurrent && same_place {
                waits.push((streak, ms_between(end_of(r), next.time).max(0.0)));
            }
        };

        // bodies cut short: made good by what later downloads carried
        let cuts: Vec<usize> = (0..group.len()).filter(|&i| cut_body(group[i])).collect();
        let complete = group.iter().filter(|r| done(r)).count();
        for &i in &cuts {
            let r = group[i];
            faults += 1;
            cut += 1;
            let later: Vec<&Record> = group[i + 1..].iter().copied().filter(|r| done(r)).collect();
            // a key the clean run reads twice was read twice here too, or
            // the second read is not the first one's
            let outcome = match cut_outcome(r, &later) {
                Cut::Resumed | Cut::Reread if complete < want => Cut::Taken,
                o => o,
            };
            match outcome {
                Cut::Resumed | Cut::Reread => {
                    if matches!(outcome, Cut::Resumed) {
                        resumed += 1;
                    } else {
                        reread += 1;
                    }
                    retried += 1;
                    attempts = attempts.max(2);
                    if let Some(next) = group.get(i + 1) {
                        wait(1, r, next);
                    }
                }
                Cut::Hole => {
                    holes += 1;
                    keep(&mut at_cut, r);
                }
                Cut::Taken => {
                    taken += 1;
                    attempts = attempts.max(1);
                    keep(&mut at_cut, r);
                }
            }
        }

        let failed: Vec<usize> = (0..group.len())
            .filter(|&i| faulted(group[i]) && !cut_body(group[i]) && !lost_race(group[i]))
            .collect();
        if failed.is_empty() {
            continue;
        }
        faults += failed.len();
        let owed: Vec<usize> = failed
            .iter()
            .copied()
            .filter(|&i| retry_owed(group[i]))
            .collect();
        not_retryable += failed.len() - owed.len();
        if owed.is_empty() {
            continue;
        }
        // what the clean run itself never got an answer for is not owed:
        // the store fails it with no fault of sonde's
        if m.is_some() && want == 0 {
            not_retryable += owed.len();
            continue;
        }
        let read = !is_write(group[owed[0]]);
        let after_last = |i: usize| group[i + 1..].iter().any(|r| answered(r));
        let unretried = if read {
            // a read is made good by an answer after it, however many the
            // clean run got: a loop that polls for a fixed time gets fewer
            // when backoff uses the time
            owed.iter().filter(|&&i| !after_last(i)).count()
        } else {
            // the program is short of this many answers: that many failures
            // were never made good, and a program gives up on the last ones
            let short = match want {
                // a request the clean run never sent (a key named by the
                // clock) is meant once, and only an answer after its failure
                // makes it good
                0 => 1usize.saturating_sub(after_last(owed[0]) as usize),
                want => want.saturating_sub(group.iter().filter(|r| answered(r)).count()),
            };
            // a failure nothing followed was not sent again, whatever the count
            let last = (owed.last() == Some(&(group.len() - 1))) as usize;
            owed.len().min(short.max(last))
        };
        let made_good = owed.len() - unretried;
        let mut streak = 0u32;
        for (n, &i) in owed.iter().enumerate() {
            let r = group[i];
            streak = if i > 0 && faulted(group[i - 1]) {
                streak + 1
            } else {
                1
            };
            if n >= made_good {
                attempts = attempts.max(streak);
                if read {
                    dropped_reads += 1;
                    keep(&mut at_read, r);
                } else {
                    dropped_writes += 1;
                    lost_writes += lost_answer(r) as usize;
                    keep(&mut at_write, r);
                }
                continue;
            }
            let next = group[i + 1];
            attempts = attempts.max(streak + 1);
            let waited = ms_between(end_of(r), next.time).max(0.0);
            // a key the clean run reads again and again: what follows a
            // failed read is a retry when it comes sooner than the program
            // ever asks by itself, and otherwise not told from its next round
            let polled = read
                && want > 1
                && !faulted(next)
                && m.and_then(|m| m.rhythm).is_none_or(|g| waited >= g / 2.0);
            if polled {
                read_again += 1;
                continue;
            }
            retried += 1;
            wait(streak, r, next);
            if lost_answer(r) && is_write(r) && answered(next) {
                if next.is_error() {
                    // the store had done it: the second attempt found it done
                    refused.push(format!(
                        "the retried {} was answered {}{}: {}",
                        next.operation,
                        next.status.unwrap_or(0),
                        s3_code(next).map(|c| format!(" {c}")).unwrap_or_default(),
                        if next.operation == "CompleteMultipartUpload" {
                            "the upload had completed"
                        } else {
                            "the store had made the first attempt"
                        }
                    ));
                } else if r.method != "DELETE"
                    && r.operation != "DeleteObjects"
                    && condition_of(r) != Some(Condition::Create)
                {
                    wrote_twice += 1;
                }
            }
        }
    }

    // races lost: by commit, not by key. The clean run says how many
    // commits the program means to make in each log.
    let (mut races, mut unrun, mut overwrote, mut blind, mut read_back) = (0, 0, 0, 0, 0);
    let mut fenced = false;
    let mut at_race: Option<&Record> = None;
    let mut in_time: Vec<&Record> = run.iter().collect();
    in_time.sort_by_key(|r| (r.time, r.id));
    for (family, group) in families.iter_mut() {
        group.sort_by_key(|r| (r.time, r.id));
        let lost: Vec<usize> = (0..group.len()).filter(|&i| lost_race(group[i])).collect();
        if lost.is_empty() {
            continue;
        }
        faults += lost.len();
        races += lost.len();
        let made = group.iter().filter(|r| done(r)).count();
        let after_first = group[lost[0] + 1..].iter().filter(|r| done(r)).count();
        let short = match won.get(family).copied().unwrap_or(0) {
            0 => 1usize.saturating_sub(after_first),
            want => want.saturating_sub(made),
        };
        let unretried = lost.len().min(short);
        let made_good = lost.len() - unretried;
        retried += made_good;
        // every commit but the lost ones was made: with them in the store,
        // it holds as many as the clean run's
        let rest_made = short <= lost.len();
        for (n, &i) in lost.iter().enumerate() {
            let r = group[i];
            // the contested key written over without a condition: the
            // other writer's commit, had there been one, is gone
            let over = in_time.iter().any(|w| {
                (w.time, w.id) > (r.time, r.id)
                    && is_write(w)
                    && w.method != "DELETE"
                    && condition_of(w).is_none()
                    && done(w)
                    && w.bucket == r.bucket
                    && w.key == r.key
                    && !w
                        .query
                        .iter()
                        .any(|(k, _)| k == "uploadId" || k == "uploads")
            });
            if over {
                overwrote += 1;
                keep(&mut at_race, r);
            }
            if n >= made_good {
                attempts = attempts.max(1);
                // SlateDB fences a writer that loses: to stop there, having
                // written nothing more, is what the format asks of it
                let slate = r
                    .key
                    .as_deref()
                    .and_then(crate::episode::part_of)
                    .is_some_and(|p| p.format() == crate::episode::Format::SlateDB);
                let wrote_on = in_time
                    .iter()
                    .any(|w| (w.time, w.id) > (r.time, r.id) && is_write(w) && !faulted(w));
                // the write is in the store (sonde let it through) and the
                // program read the key after its 412: it may have known its
                // own write there, and been right to stop
                let looked = lost_answer(r)
                    && in_time.iter().any(|w| {
                        (w.time, w.id) > (r.time, r.id)
                            && !is_write(w)
                            && done(w)
                            && w.bucket == r.bucket
                            && w.key == r.key
                    });
                if slate && exit != 0 && !wrote_on {
                    fenced = true;
                } else if looked && rest_made && exit == clean_exit && !over {
                    read_back += 1;
                } else {
                    unrun += 1;
                    keep(&mut at_race, r);
                }
                continue;
            }
            attempts = attempts.max(2);
            if let Some(next) = group.get(i + 1) {
                let header = |x: &Record| {
                    x.request_headers
                        .iter()
                        .find(|h| h.name.eq_ignore_ascii_case("if-match"))
                        .map(|h| h.value.clone())
                };
                // the same ETag sent again: nothing was read in between
                // that could have told the program what it lost to
                if header(r).is_some() && header(r) == header(next) {
                    blind += 1;
                }
                if !group.windows(2).any(|w| w[1].time < end_of(w[0])) {
                    waits.push((1, ms_between(end_of(r), next.time).max(0.0)));
                }
            }
        }
    }
    // what the lost commits wrote and nothing removed: objects written
    // without a condition, beyond the clean run's
    let left = |records: &[Record]| -> i64 {
        records
            .iter()
            .filter(|r| done(r) && condition_of(r).is_none())
            .map(|r| match r.operation.as_str() {
                "PutObject" | "CopyObject" | "CompleteMultipartUpload" => 1,
                "DeleteObject" => -1,
                "DeleteObjects" => {
                    -(r.request_body
                        .as_deref()
                        .map_or(1, |b| b.matches("<Key>").count().max(1))
                        as i64)
                }
                _ => 0,
            })
            .sum()
    };
    let orphans = if races > 0 {
        (left(run) - left(clean)).max(0) as usize
    } else {
        0
    };

    let mut all: Vec<f64> = waits.iter().map(|(_, w)| *w).collect();
    all.sort_by(|a, b| a.total_cmp(b));
    let backoff_ms = (!all.is_empty()).then(|| [all[0], all[all.len() / 2], all[all.len() - 1]]);
    // jitter: within one attempt number (a backoff that doubles is not
    // jittered by doubling), only with enough waits to tell, and only of
    // waits long enough to be the program's
    let mut by_attempt: BTreeMap<u32, Vec<f64>> = BTreeMap::new();
    for (n, w) in &waits {
        by_attempt.entry(*n).or_default().push(*w);
    }
    let mut told: Vec<bool> = Vec::new();
    let mut jitter_too_short = false;
    for w in by_attempt.values_mut().filter(|w| w.len() >= 4) {
        w.sort_by(|a, b| a.total_cmp(b));
        let mid = w[w.len() / 2];
        if mid < JITTER_FLOOR_MS {
            jitter_too_short = true;
        } else {
            told.push(jittered(w, mid));
        }
    }
    let jitter = (!told.is_empty()).then(|| told.iter().any(|j| *j));
    let jitter_too_short = jitter_too_short && jitter.is_none();
    let left_uploads = unfinished_uploads(run).saturating_sub(unfinished_uploads(clean));
    let delayed = run
        .iter()
        .filter(|r| r.injected.is_some() && !faulted(r))
        .count();

    let mut reasons = Vec::new();
    let mut standing = Standing::Survived;
    let any = run.iter().any(|r| r.injected.is_some());
    if !any {
        standing = Standing::Untested;
        reasons.push("the scenario met none of the program's requests".into());
    }
    if left_uploads > 0 {
        standing = Standing::Degraded;
        reasons.push(format!(
            "left {left_uploads} multipart upload{} unfinished (their parts are billed until aborted)",
            plural(left_uploads)
        ));
    }
    if dropped_reads > 0 {
        standing = Standing::Degraded;
        reasons.push(format!(
            "gave up on {dropped_reads} failed read{} without sending {} again",
            plural(dropped_reads),
            if dropped_reads == 1 { "it" } else { "them" }
        ));
    }
    if orphans > 0 {
        standing = Standing::Degraded;
        reasons.push(format!(
            "{orphans} more object{} left than the clean run leaves: what the lost commit wrote, unless the commit that followed names {}",
            plural(orphans),
            if orphans == 1 { "it" } else { "them" }
        ));
    }
    if blind > 0 {
        standing = Standing::Degraded;
        reasons.push(format!(
            "{blind} lost write{} sent again with the same If-Match: a real writer's ETag would refuse it again",
            plural(blind)
        ));
    }
    let plain = dropped_writes - lost_writes;
    if lost_writes > 0 {
        standing = Standing::Degraded;
        reasons.push(format!(
            "{lost_writes} write{} answered 500 and not retried; the store has {}",
            plural(lost_writes),
            if lost_writes == 1 { "it" } else { "them" }
        ));
    }
    if plain > 0 {
        standing = Standing::Degraded;
        reasons.push(format!(
            "{plain} write{} failed and {} never sent again",
            plural(plain),
            if plain == 1 { "was" } else { "were" }
        ));
    }
    if dropped_writes > 0 && exit == 0 {
        // told of a failed write, it said all was well
        standing = Standing::Broke;
        if let Some(last) = reasons.last_mut() {
            last.push_str(", and the program exited 0");
        }
    }
    if taken > 0 {
        // half a body and no read of the rest: with the clean run's exit,
        // the half was taken for the whole
        standing = if exit == clean_exit {
            Standing::Broke
        } else {
            Standing::Degraded
        };
        reasons.push(format!(
            "{taken} download{} cut short and the rest never read{}",
            plural(taken),
            if exit == clean_exit {
                format!(": the half was taken as the whole (exit {exit}, as the clean run)")
            } else {
                String::new()
            }
        ));
    }
    if holes > 0 {
        standing = Standing::Broke;
        reasons.push(format!(
            "{holes} download{} cut short and read on from past the cut: bytes are missing",
            plural(holes)
        ));
    }
    if unrun > 0 {
        standing = if exit == clean_exit {
            Standing::Broke
        } else {
            Standing::Degraded
        };
        reasons.push(format!(
            "{unrun} conditional write{} lost {} race and the commit was never made{}",
            plural(unrun),
            if unrun == 1 { "its" } else { "their" },
            if exit == clean_exit {
                format!(", and the program exited {exit}")
            } else {
                String::new()
            }
        ));
    }
    if overwrote > 0 {
        standing = Standing::Broke;
        reasons.push(format!(
            "{overwrote} lost race{} answered by writing the key without a condition: the other writer's commit would be gone",
            plural(overwrote)
        ));
    }
    if clean_exit != 0 && standing == Standing::Survived {
        // a program that fails without faults says nothing by its exit
        standing = Standing::Degraded;
        reasons.push(format!("the clean run exited {clean_exit} itself"));
    }
    if exit != clean_exit && !fenced {
        standing = Standing::Broke;
        reasons.push(format!("exited {exit} (the clean run exited {clean_exit})"));
        // and why, where the requests say
        refused.sort();
        refused.dedup();
        reasons.extend(refused);
    }

    // where: the request the verdict turns on, and what to change there
    let last_failed = || {
        in_time
            .iter()
            .rev()
            .find(|r| faulted(r) || r.is_error() || r.transport_error.is_some())
            .copied()
    };
    let turned_on = match standing {
        Standing::Survived | Standing::Untested => None,
        _ => at_write
            .or(at_cut)
            .or(at_race)
            .or(at_read)
            .or_else(|| (exit != clean_exit).then(last_failed).flatten()),
    };
    // (a request that was sent again, or owed no retry, has no advice:
    // the exit is all that is known of it)
    let given_up = |r: &Record| {
        [at_write, at_cut, at_race, at_read]
            .iter()
            .flatten()
            .any(|x| x.id == r.id)
    };
    let advice = turned_on.filter(|r| given_up(r)).map(|r| {
        let sdk = crate::callers::sdk(r);
        if cut_body(r) {
            Said::new("trial.advice.cut", &[])
        } else if lost_race(r) {
            Said::new("trial.advice.race", &[])
        } else {
            match sdk.as_deref().and_then(|s| Some((s, retry_setting(s)?))) {
                Some((sdk, setting)) => Said::new(
                    "trial.advice.retries",
                    &[("sdk", sdk.to_string()), ("setting", setting.to_string())],
                ),
                None => Said::new("trial.advice.retry", &[]),
            }
        }
    });
    Trial {
        scenario: scenario.name.clone(),
        about: scenario.about.clone(),
        standing,
        exit,
        clean_exit,
        faults,
        retried,
        not_retryable,
        read_again,
        dropped_reads: dropped_reads + taken + holes,
        dropped_writes: dropped_writes + unrun,
        lost_writes,
        cut,
        resumed,
        reread,
        races,
        read_back,
        fenced,
        attempts,
        backoff_ms,
        jitter,
        jitter_too_short,
        requests: run.len(),
        clean_requests: clean.len(),
        delayed,
        wall_ms: span_ms(run),
        clean_wall_ms: span_ms(clean),
        left_uploads,
        wrote_twice,
        reasons,
        at: turned_on.map(where_of),
        advice,
        session: None,
    }
}

fn stem_of(session: &std::path::Path) -> String {
    let name = session
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.strip_suffix(".sonde").unwrap_or(&name).to_string()
}

/// Where a run's trials are kept: beside its session, so that the app and
/// `sonde check` read what the last trial found.
pub fn file_for(session: &std::path::Path) -> std::path::PathBuf {
    session.with_file_name(format!("{}.trials.json", stem_of(session)))
}

/// The folder a run's trial sessions are recorded in, beside its session.
pub fn folder_for(session: &std::path::Path) -> std::path::PathBuf {
    session.with_file_name("trials")
}

/// The stem a trial's session file is given: `<run>.<scenario>`.
pub fn session_stem(session: &std::path::Path, scenario: &str) -> String {
    format!("{}.{scenario}", stem_of(session))
}

/// Whether a file in a run's `trials/` folder is one of that run's trial
/// sessions: `<run>.<scenario>.sonde`, a scenario's name having no dot.
fn is_trial_of(session: &std::path::Path, name: &str) -> bool {
    name.strip_prefix(&format!("{}.", stem_of(session)))
        .and_then(|rest| rest.strip_suffix(".sonde"))
        .is_some_and(|scenario| {
            !scenario.is_empty()
                && scenario
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

/// Where a trial's session is: in its run's `trials/` folder, under the
/// name the verdict keeps. Only a name of that run's is taken: what the
/// file says (it may have come with a shared session) is never a path.
pub fn session_path(session: &std::path::Path, t: &Trial) -> Option<std::path::PathBuf> {
    let kept = t.session.as_deref()?;
    let name = kept.rsplit(['/', '\\']).next()?;
    is_trial_of(session, name).then(|| folder_for(session).join(name))
}

/// Removes what a run's trials left: its verdicts, and the files in its own
/// `trials/` folder that are named for it. Before a run is recorded again,
/// and when it is deleted. Nothing the verdicts' file names is removed.
pub fn remove(session: &std::path::Path) {
    let dir = folder_for(session);
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        // a file of the folder itself, never one a link points to
        let file = e.file_type().is_ok_and(|t| t.is_file());
        if file && is_trial_of(session, &name) {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }
    let _ = std::fs::remove_file(file_for(session));
    // (only when nothing else is in it)
    let _ = std::fs::remove_dir(dir);
}

pub fn save(session: &std::path::Path, trials: &[Trial]) -> std::io::Result<()> {
    std::fs::write(file_for(session), serde_json::to_vec_pretty(trials)?)
}

/// A run's trials; none when it was never tried (or the file is damaged).
pub fn load(session: &std::path::Path) -> Vec<Trial> {
    std::fs::read(file_for(session))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// A run's trials in a word: the worst standing among them (broke, then
/// degraded, then survived; untested when none met a request).
pub fn worst(trials: &[Trial]) -> Option<Standing> {
    [
        Standing::Broke,
        Standing::Degraded,
        Standing::Survived,
        Standing::Untested,
    ]
    .into_iter()
    .find(|s| trials.iter().any(|t| t.standing == *s))
}

impl Trial {
    /// What happened, in a line: the figures behind the standing, then why
    /// it is not "survived", the request it turns on and what to change.
    pub fn line(&self) -> String {
        let mut parts = Vec::new();
        let owed = self.faults.saturating_sub(self.not_retryable);
        if self.faults > 0 {
            let n = format!("{} fault{}", self.faults, plural(self.faults));
            let mut said = if owed == 0 {
                // a 404 is not sent again: the verdict is the exit's
                format!("{n}, none a retry is owed for")
            } else if self.retried == owed {
                format!("{n}, {}retried", if owed == 1 { "" } else { "all " })
            } else {
                format!("{n}, {} retried", self.retried)
            };
            if self.read_again > 0 {
                // a poll: the answer after a failed read may be the loop's own
                said.push_str(&format!(
                    ", {} read again later (a key the program reads repeatedly: a retry or its next round)",
                    self.read_again
                ));
            }
            parts.push(said);
        }
        if self.cut > 0 {
            let mut how = Vec::new();
            if self.resumed > 0 {
                how.push(format!("{} resumed from the cut", self.resumed));
            }
            if self.reread > 0 {
                how.push(format!("{} read again whole", self.reread));
            }
            parts.push(format!(
                "{} bod{} cut{}",
                self.cut,
                if self.cut == 1 { "y" } else { "ies" },
                if how.is_empty() {
                    String::new()
                } else {
                    format!(": {}", how.join(", "))
                }
            ));
        }
        if self.read_back > 0 {
            parts.push(format!(
                "{} lost write{} read back and not written again: the store has {} (sonde let {} through), so no commit is missing",
                self.read_back,
                plural(self.read_back),
                if self.read_back == 1 { "it" } else { "them" },
                if self.read_back == 1 { "it" } else { "them" },
            ));
        }
        if self.fenced {
            parts.push(format!(
                "fenced: it stopped at the 412 (exit {}) and wrote nothing after, as SlateDB means a writer to",
                self.exit
            ));
        }
        if let Some([lo, mid, hi]) = self.backoff_ms {
            // as they read: 1.02 s and 1.04 s are both "1.0 s"
            let (low, high) = (ms_text(lo), ms_text(hi));
            parts.push(if low == high {
                format!("after {}", ms_text(mid))
            } else {
                match self.jitter {
                    Some(true) => format!("after {low} to {high} (jittered)"),
                    Some(false) => format!("after {low} to {high} (in step)"),
                    None if self.jitter_too_short => {
                        format!("after {low} to {high} (too short to tell jitter)")
                    }
                    None => format!("after {low} to {high}"),
                }
            });
        }
        if self.delayed > 0 && self.faults == 0 && self.wall_ms > 0.0 {
            parts.push(format!(
                "{} delayed · took {} (the clean run {})",
                self.delayed,
                ms_text(self.wall_ms),
                ms_text(self.clean_wall_ms)
            ));
        }
        if self.clean_requests > 0 && self.requests != self.clean_requests {
            let ratio = format!("{:.1}", self.requests as f64 / self.clean_requests as f64);
            if ratio != "1.0" {
                parts.push(format!("{ratio}× the requests"));
            }
        }
        if self.wrote_twice > 0 {
            parts.push(format!("{} written twice", self.wrote_twice));
        }
        parts.extend(self.reasons.iter().cloned());
        if let Some(place) = self.place() {
            parts.push(place);
        }
        if let Some(a) = &self.advice {
            parts.push(a.en());
        }
        parts.join(" · ")
    }

    /// The request the verdict turns on: `at PutObject b/state (etl.py:12
    /// in main)`.
    pub fn place(&self) -> Option<String> {
        let w = self.at.as_ref()?;
        Some(match &w.caller {
            // the file by its name: the whole path is in `where`
            Some(c) => {
                let (file, rest) = c.split_once(' ').unwrap_or((c, ""));
                let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
                let sep = if rest.is_empty() { "" } else { " " };
                format!("at {} {} ({name}{sep}{rest})", w.operation, w.key)
            }
            None => format!("at {} {}", w.operation, w.key),
        })
    }
}

/// The scenarios worth a program's time, from what its clean run did: the
/// two every program meets, the lost answer if it writes, the failed
/// completion if it uploads in parts, the body cut short if it downloads
/// one large enough to cut, the lost race if it writes on a condition.
pub fn choose(clean: &[Record]) -> Vec<crate::fault::Scenario> {
    let has = |f: &dyn Fn(&Record) -> bool| clean.iter().any(f);
    let mut names = vec!["slowdown", "error"];
    if has(&|r| r.method == "PUT" || r.method == "POST" || r.method == "DELETE") {
        names.push("lost-answer");
    }
    if has(&|r| r.operation == "CompleteMultipartUpload") {
        names.push("fail-complete");
    }
    if has(&|r| {
        r.operation == "GetObject"
            && done(r)
            && r.response_bytes
                .is_some_and(|n| crate::fault::cut_at(n).is_some())
    }) {
        names.push("cut-body");
    }
    if has(&|r| is_write(r) && condition_of(r).is_some()) {
        names.push("lost-race");
    }
    names
        .into_iter()
        .filter_map(crate::fault::scenario)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use reqlog::Header;

    fn rec(id: u64, at_ms: i64, method: &str, op: &str, key: &str, status: u16) -> Record {
        Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
            method: method.into(),
            operation: op.into(),
            bucket: Some("b".into()),
            key: Some(key.into()),
            status: Some(status),
            latency_ms: 5.0,
            ..Default::default()
        }
    }

    fn fault(mut r: Record, scenario: &str) -> Record {
        r.injected = Some(scenario.into());
        r.response_headers
            .push(Header::new("x-sonde-fault", scenario));
        r
    }

    fn slowdown() -> crate::fault::Scenario {
        crate::fault::scenario("slowdown").unwrap()
    }

    fn lost(r: Record, store: u16) -> Record {
        let mut r = fault(r, "lost-answer");
        r.status = Some(500);
        r.response_headers
            .push(Header::new("x-sonde-store-status", store.to_string()));
        r
    }

    fn put(id: u64, at_ms: i64, key: &str, status: u16) -> Record {
        rec(id, at_ms, "PUT", "PutObject", key, status)
    }

    fn get(id: u64, at_ms: i64, key: &str, status: u16) -> Record {
        rec(id, at_ms, "GET", "GetObject", key, status)
    }

    #[test]
    fn a_program_that_retries_survives_and_its_backoff_is_measured() {
        let clean = vec![rec(1, 0, "PUT", "PutObject", "a", 200)];
        let run = vec![
            fault(rec(1, 0, "PUT", "PutObject", "a", 503), "slowdown"),
            rec(2, 105, "PUT", "PutObject", "a", 200),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Survived, "{:?}", t.reasons);
        assert_eq!((t.faults, t.retried, t.attempts), (1, 1, 2));
        assert_eq!(t.backoff_ms, Some([100.0, 100.0, 100.0]));
        assert_eq!(
            t.line(),
            "1 fault, retried · after 100 ms · 2.0× the requests"
        );
    }

    #[test]
    fn a_key_written_twice_on_purpose_is_not_the_retry_of_its_first_write() {
        // write state, poll, write state again: the program's own second
        // write, 313 ms later, is not a retry of the first
        let clean = vec![
            put(1, 0, "state", 200),
            get(2, 100, "job", 200),
            put(3, 313, "state", 200),
        ];
        let run = vec![
            fault(put(1, 0, "state", 503), "slowdown"),
            fault(get(2, 100, "job", 503), "slowdown"),
            put(3, 313, "state", 200),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.faults, t.retried), (2, 0), "{}", t.line());
        assert_eq!((t.dropped_writes, t.dropped_reads), (1, 1));
        assert_eq!(t.backoff_ms, None, "its own sleep is no backoff");
        assert_eq!(t.standing, Standing::Broke);
        // the same program, retrying: three answers for the two it means
        let run = vec![
            fault(put(1, 0, "state", 503), "slowdown"),
            put(2, 105, "state", 200),
            get(3, 200, "job", 200),
            put(4, 413, "state", 200),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.faults, t.retried, t.attempts), (1, 1, 2));
        assert_eq!(t.standing, Standing::Survived, "{:?}", t.reasons);
        assert_eq!(t.backoff_ms, Some([100.0, 100.0, 100.0]));
    }

    #[test]
    fn a_failed_poll_is_made_good_by_an_answer_after_it_and_called_a_retry_only_when_prompt() {
        let clean: Vec<Record> = (0..4)
            .map(|i| get(i + 1, i as i64 * 100, "job", 200))
            .collect();
        // the first poll fails, and the loop polls on at its own pace: the
        // answer after it is a retry or the next round, and is not called one
        let mut run = vec![fault(get(1, 0, "job", 503), "slowdown")];
        run.extend((1..5).map(|i| get(i + 1, i as i64 * 100, "job", 200)));
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.retried, t.read_again, t.dropped_reads), (0, 1, 0));
        assert_eq!(t.backoff_ms, None, "the loop's sleep is no backoff");
        assert_eq!(t.standing, Standing::Survived);
        assert_eq!(
            t.line(),
            "1 fault, 0 retried, 1 read again later (a key the program reads repeatedly: a retry or its next round) · 1.2× the requests"
        );
        // a loop that polls for a fixed time gets fewer answers when backoff
        // uses the time: nothing was given up
        run.pop();
        run.pop();
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.read_again, t.dropped_reads), (1, 0), "{}", t.line());
        assert_eq!(t.standing, Standing::Survived);
        // sooner than the loop ever asks: a retry
        let mut run = vec![
            fault(get(1, 0, "job", 503), "slowdown"),
            get(2, 15, "job", 200),
        ];
        run.extend((1..4).map(|i| get(i + 2, i as i64 * 100, "job", 200)));
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.retried, t.read_again), (1, 0));
        assert_eq!(t.backoff_ms, Some([10.0, 10.0, 10.0]));
        // a failed read nothing answered after is given up
        let run = vec![
            get(1, 0, "job", 200),
            fault(get(2, 100, "job", 503), "s3-limits"),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.retried, t.read_again, t.dropped_reads), (0, 0, 1));
        assert_eq!(t.standing, Standing::Degraded);
    }

    #[test]
    fn two_failures_in_a_row_are_two_retries_or_one_request_given_up() {
        let twice = crate::fault::Scenario {
            name: "twice".into(),
            about: String::new(),
            rules: Vec::new(),
        };
        let clean = vec![put(1, 0, "a", 200)];
        let run = vec![
            fault(put(1, 0, "a", 503), "twice"),
            fault(put(2, 105, "a", 503), "twice"),
            put(3, 310, "a", 200),
        ];
        let t = judge(&twice, &clean, 0, &run, 0);
        assert_eq!((t.faults, t.retried, t.attempts), (2, 2, 3));
        assert_eq!(t.standing, Standing::Survived);
        // one retry, then it gave up: the second failure was never made good
        let t = judge(&twice, &clean, 0, &run[..2], 0);
        assert_eq!((t.faults, t.retried, t.dropped_writes), (2, 1, 1));
        assert_eq!(t.attempts, 2);
        assert_eq!(t.standing, Standing::Broke);
    }

    #[test]
    fn an_upload_started_again_under_a_new_id_retries_its_parts() {
        let part = |id, at, upload: &str, status| {
            let mut r = rec(id, at, "PUT", "UploadPart", "big", status);
            r.query = vec![
                ("partNumber".into(), "1".into()),
                ("uploadId".into(), upload.into()),
            ];
            r
        };
        let clean = vec![part(1, 0, "u1", 200)];
        let run = vec![
            fault(part(1, 0, "u1", 503), "slowdown"),
            part(2, 205, "u2", 200),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.faults, t.retried), (1, 1));
        assert_eq!(t.backoff_ms.map(|b| b[1]), Some(200.0));
    }

    #[test]
    fn a_request_the_clean_run_never_sent_is_retried_by_an_answer_after_it() {
        // keys named by the clock: the clean run's are other keys
        let clean = vec![put(1, 0, "out/1700", 200)];
        let run = vec![
            fault(put(1, 0, "out/1800", 503), "slowdown"),
            put(2, 105, "out/1800", 200),
        ];
        assert_eq!(judge(&slowdown(), &clean, 0, &run, 0).retried, 1);
        // an answer before the failure is not its retry
        let limits = crate::fault::scenario("s3-limits").unwrap();
        let run = vec![
            put(1, 0, "out/1800", 200),
            fault(put(2, 5, "out/1800", 503), "s3-limits"),
        ];
        let t = judge(&limits, &clean, 0, &run, 0);
        assert_eq!((t.retried, t.dropped_writes), (0, 1));
    }

    #[test]
    fn two_batches_of_deletes_are_two_requests() {
        let batch = |id, at, body: &str, status| {
            let mut r = rec(id, at, "POST", "DeleteObjects", "", status);
            r.key = None;
            r.query = vec![("delete".into(), String::new())];
            r.request_body = Some(body.into());
            r
        };
        let clean = vec![batch(1, 0, "<a/>", 200), batch(2, 50, "<b/>", 200)];
        // the first batch fails and is never sent again; the second goes through
        let run = vec![
            fault(batch(1, 0, "<a/>", 503), "slowdown"),
            fault(batch(2, 50, "<b/>", 503), "slowdown"),
            batch(3, 160, "<b/>", 200),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.faults, t.retried, t.dropped_writes), (2, 1, 1));
        assert_eq!(t.standing, Standing::Broke);
    }

    #[test]
    fn a_write_dropped_in_silence_is_broke_and_a_read_given_up_is_degraded() {
        let clean = vec![rec(1, 0, "PUT", "PutObject", "a", 200)];
        let run = vec![fault(rec(1, 0, "PUT", "PutObject", "a", 503), "slowdown")];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Broke);
        assert_eq!(
            t.reasons[0],
            "1 write failed and was never sent again, and the program exited 0"
        );

        let run = vec![fault(rec(1, 0, "GET", "GetObject", "a", 503), "slowdown")];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Degraded);
        assert_eq!(
            t.reasons[0],
            "gave up on 1 failed read without sending it again"
        );
    }

    #[test]
    fn an_exit_unlike_the_clean_runs_is_broke() {
        let clean = vec![rec(1, 0, "PUT", "PutObject", "a", 200)];
        let run = vec![fault(rec(1, 0, "PUT", "PutObject", "a", 503), "slowdown")];
        let t = judge(&slowdown(), &clean, 0, &run, 1);
        assert_eq!(t.standing, Standing::Broke);
        assert!(t.reasons.iter().any(|r| r.starts_with("exited 1")));
        // a program that fails without faults too is not broken by them
        let t = judge(&slowdown(), &clean, 1, &run, 1);
        assert_ne!(t.standing, Standing::Survived);
    }

    #[test]
    fn a_lost_answer_retried_is_a_write_made_twice() {
        let s = crate::fault::scenario("lost-answer").unwrap();
        let clean = vec![rec(1, 0, "PUT", "PutObject", "a", 200)];
        let run = vec![
            lost(put(1, 0, "a", 500), 200),
            rec(2, 50, "PUT", "PutObject", "a", 200),
        ];
        let t = judge(&s, &clean, 0, &run, 0);
        assert_eq!((t.standing, t.wrote_twice), (Standing::Survived, 1));
    }

    #[test]
    fn a_retry_the_store_refused_is_not_a_second_write_and_says_why_it_broke() {
        let s = crate::fault::scenario("lost-answer").unwrap();
        let complete = |id, at, status| {
            let mut r = rec(id, at, "POST", "CompleteMultipartUpload", "big", status);
            r.query = vec![("uploadId".into(), "u1".into())];
            r
        };
        let mut create = rec(1, 0, "POST", "CreateMultipartUpload", "big", 200);
        create.query = vec![("uploads".into(), String::new())];
        let clean = vec![create.clone(), complete(2, 10, 200)];
        let mut refused = complete(3, 120, 404);
        refused.error_body = Some("<Error><Code>NoSuchUpload</Code></Error>".into());
        let run = vec![create, lost(complete(2, 10, 500), 200), refused];
        let t = judge(&s, &clean, 0, &run, 1);
        assert_eq!(t.wrote_twice, 0, "the store refused the second");
        assert_eq!(t.left_uploads, 0, "the store completed it");
        assert_eq!((t.faults, t.retried), (1, 1));
        assert_eq!(t.standing, Standing::Broke);
        assert_eq!(
            t.reasons,
            [
                "exited 1 (the clean run exited 0)",
                "the retried CompleteMultipartUpload was answered 404 NoSuchUpload: the upload had completed"
            ]
        );
        // a program that reads the 404 as its own success survived
        let t = judge(&s, &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Survived, "{:?}", t.reasons);
    }

    #[test]
    fn a_lost_answer_never_asked_about_again_is_a_write_and_a_read_is_a_read() {
        let s = crate::fault::scenario("lost-answer").unwrap();
        let clean = vec![
            put(1, 0, "a", 200),
            put(2, 10, "b", 200),
            get(3, 20, "a", 200),
        ];
        let run = vec![
            lost(put(1, 0, "a", 500), 200),
            lost(put(2, 10, "b", 500), 200),
            lost(get(3, 20, "a", 500), 200),
        ];
        let t = judge(&s, &clean, 0, &run, 0);
        assert_eq!(
            (t.dropped_writes, t.lost_writes, t.dropped_reads),
            (2, 2, 1)
        );
        assert_eq!(t.wrote_twice, 0);
        assert_eq!(t.standing, Standing::Broke);
        assert!(
            t.reasons.contains(
                &"2 writes answered 500 and not retried; the store has them, and the program exited 0"
                    .to_string()
            ),
            "{:?}",
            t.reasons
        );
        // a read whose answer was lost and was read again is not "written twice"
        let run = vec![lost(get(3, 20, "a", 500), 200), get(4, 130, "a", 200)];
        let t = judge(&s, &clean[2..], 0, &run, 0);
        assert_eq!((t.wrote_twice, t.retried), (0, 1));
    }

    #[test]
    fn jitter_is_judged_among_the_waits_before_one_attempt() {
        let twice = crate::fault::Scenario {
            name: "twice".into(),
            about: String::new(),
            rules: Vec::new(),
        };
        // a fixed 1 s, then 2 s: four requests, each failed twice
        let fixed = |first: &[i64], second: &[i64]| {
            let (mut clean, mut run) = (Vec::new(), Vec::new());
            for k in 0..first.len() {
                let key = format!("k{k}");
                clean.push(put(k as u64, 0, &key, 200));
                let base = k as u64 * 10 + 100;
                run.push(fault(put(base, 0, &key, 503), "twice"));
                run.push(fault(put(base + 1, 5 + first[k], &key, 503), "twice"));
                run.push(put(base + 2, 10 + first[k] + second[k], &key, 200));
            }
            judge(&twice, &clean, 0, &run, 0)
        };
        let t = fixed(&[1000, 1001, 1002, 1000], &[2000, 2001, 2003, 2000]);
        assert_eq!(t.jitter, Some(false), "1 s then 2 s is a backoff in step");
        assert_eq!(
            t.line(),
            "8 faults, all retried · after 1.0 s to 2.0 s (in step) · 3.0× the requests"
        );
        let t = fixed(&[100, 480, 900, 250], &[300, 1900, 800, 1200]);
        assert_eq!(t.jitter, Some(true));
        // three waits before each attempt say neither: the range alone
        let t = fixed(&[100, 480, 900], &[300, 1900, 800]);
        assert_eq!(t.jitter, None);
        assert_eq!(
            t.line(),
            "6 faults, all retried · after 100 ms to 1.9 s · 3.0× the requests"
        );
    }

    #[test]
    fn a_range_is_printed_only_when_its_ends_read_differently() {
        let clean = vec![put(1, 0, "a", 200), put(2, 0, "b", 200)];
        let run = vec![
            fault(put(1, 0, "a", 503), "slowdown"),
            fault(put(2, 0, "b", 503), "slowdown"),
            put(3, 1025, "a", 200),
            put(4, 1045, "b", 200),
        ];
        let line = judge(&slowdown(), &clean, 0, &run, 0).line();
        assert!(line.contains("after 1.0 s ·"), "{line}");
        assert!(!line.contains(" to "), "{line}");
    }

    #[test]
    fn a_fault_no_retry_is_owed_for_is_judged_by_the_exit_alone() {
        let s = crate::fault::Scenario {
            name: "gone".into(),
            about: "404 NoSuchKey".into(),
            rules: Vec::new(),
        };
        let clean = vec![get(1, 0, "a", 200)];
        let run = vec![fault(get(1, 0, "a", 404), "gone")];
        let t = judge(&s, &clean, 0, &run, 0);
        assert_eq!((t.faults, t.not_retryable, t.retried), (1, 1, 0));
        assert_eq!((t.dropped_reads, t.dropped_writes), (0, 0));
        assert_eq!(t.standing, Standing::Survived);
        assert_eq!(t.line(), "1 fault, none a retry is owed for");
        let t = judge(&s, &clean, 0, &run, 3);
        assert_eq!(t.standing, Standing::Broke);
        assert_eq!(
            t.line(),
            "1 fault, none a retry is owed for · exited 3 (the clean run exited 0) · at GetObject b/a"
        );
        assert_eq!(t.advice, None, "a 404 is not to be sent again");
        // a 429 is owed one
        let run = vec![fault(get(1, 0, "a", 429), "gone")];
        assert_eq!(judge(&s, &clean, 0, &run, 0).dropped_reads, 1);
    }

    #[test]
    fn a_delay_prints_the_time_it_took_against_the_clean_runs() {
        let s = crate::fault::scenario("slow").unwrap();
        let clean = vec![get(1, 0, "a", 200), get(2, 100, "b", 200)];
        let mut run = vec![get(1, 0, "a", 200), get(2, 400, "b", 200)];
        for r in &mut run {
            r.injected = Some("slow".into());
            r.latency_ms = 305.0;
        }
        let t = judge(&s, &clean, 0, &run, 0);
        assert_eq!(
            (t.standing, t.faults, t.delayed),
            (Standing::Survived, 0, 2)
        );
        assert_eq!((t.clean_wall_ms, t.wall_ms), (105.0, 705.0));
        assert_eq!(t.line(), "2 delayed · took 705 ms (the clean run 105 ms)");
    }

    #[test]
    fn removing_a_runs_trials_removes_its_own_files_and_nothing_a_file_names() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("run.sonde");
        let other = dir.path().join("run-2.sonde");
        let trials = folder_for(&session);
        std::fs::create_dir_all(&trials).unwrap();
        let outside = dir.path().join("precious.txt");
        for f in [
            trials.join("run.slowdown.sonde"),
            trials.join("run.error-2.sonde"),
            trials.join("run-2.slowdown.sonde"),
            trials.join("run.notes.txt"),
            outside.clone(),
        ] {
            std::fs::write(f, "x").unwrap();
        }
        // the verdicts name a file elsewhere: a shared session may say anything
        let mut t = judge(&slowdown(), &[], 0, &[], 0);
        t.session = Some(outside.to_string_lossy().into_owned());
        save(&session, &[t.clone()]).unwrap();
        assert_eq!(session_path(&session, &t), None, "no path of the file's");
        t.session = Some("run.slowdown.sonde".into());
        assert_eq!(
            session_path(&session, &t),
            Some(trials.join("run.slowdown.sonde"))
        );
        t.session = Some("../../etc/run.slowdown.sonde".into());
        assert_eq!(
            session_path(&session, &t),
            Some(trials.join("run.slowdown.sonde")),
            "a name, never a path"
        );
        remove(&session);
        assert!(outside.exists());
        assert!(!trials.join("run.slowdown.sonde").exists());
        assert!(!trials.join("run.error-2.sonde").exists());
        assert!(!file_for(&session).exists());
        assert!(
            trials.join("run-2.slowdown.sonde").exists(),
            "another run's"
        );
        assert!(
            trials.join("run.notes.txt").exists(),
            "not a trial's session"
        );
        remove(&other);
        assert!(!trials.join("run-2.slowdown.sonde").exists());
    }

    #[test]
    fn a_scenario_that_met_nothing_is_untested_and_the_choice_follows_the_clean_run() {
        let s = crate::fault::scenario("fail-complete").unwrap();
        let clean = vec![rec(1, 0, "GET", "GetObject", "a", 200)];
        let t = judge(&s, &clean, 0, &clean, 0);
        assert_eq!(t.standing, Standing::Untested);
        let names =
            |c: &[Record]| -> Vec<String> { choose(c).into_iter().map(|s| s.name).collect() };
        assert_eq!(names(&clean), ["slowdown", "error"]);
        let writes = vec![
            rec(1, 0, "PUT", "PutObject", "a", 200),
            rec(2, 9, "POST", "CompleteMultipartUpload", "b", 200),
        ];
        assert_eq!(
            names(&writes),
            ["slowdown", "error", "lost-answer", "fail-complete"]
        );
        // a download large enough to cut, and a write on a condition
        let mut big = get(1, 0, "a", 200);
        big.response_bytes = Some(4096);
        let mut small = get(2, 5, "b", 200);
        small.response_bytes = Some(10);
        assert_eq!(names(&[small.clone()]), ["slowdown", "error"]);
        assert_eq!(names(&[small, big]), ["slowdown", "error", "cut-body"]);
        assert_eq!(
            names(&[create(1, 0, "t/_delta_log/00000000000000000001.json", 200)]),
            ["slowdown", "error", "lost-answer", "lost-race"]
        );
    }

    fn create(id: u64, at_ms: i64, key: &str, status: u16) -> Record {
        let mut r = put(id, at_ms, key, status);
        r.request_headers.push(Header::new("If-None-Match", "*"));
        r
    }

    fn race(r: Record, store: Option<u16>) -> Record {
        let mut r = fault(r, "lost-race");
        r.status = Some(412);
        if let Some(s) = store {
            r.response_headers
                .push(Header::new("x-sonde-store-status", s.to_string()));
        }
        r
    }

    fn lost_race_scenario() -> crate::fault::Scenario {
        crate::fault::scenario("lost-race").unwrap()
    }

    /// A download of `key`: bytes `first..=last` of `whole`.
    fn part(id: u64, at_ms: i64, key: &str, first: u64, last: u64, whole: u64) -> Record {
        let mut r = get(
            id,
            at_ms,
            key,
            if last + 1 - first == whole { 200 } else { 206 },
        );
        let n = last + 1 - first;
        r.response_bytes = Some(n);
        r.response_headers
            .push(Header::new("content-length", n.to_string()));
        if r.status == Some(206) {
            r.response_headers.push(Header::new(
                "content-range",
                format!("bytes {first}-{last}/{whole}"),
            ));
        }
        r
    }

    /// The same, cut by sonde after `sent` bytes.
    fn cut(r: Record, sent: u64) -> Record {
        let mut r = fault(r, "cut-body");
        r.response_bytes = Some(sent);
        r.transport_error = Some("sonde cut the response body".into());
        r.response_headers
            .push(Header::new(crate::CUT_HEADER, sent.to_string()));
        r
    }

    fn cut_scenario() -> crate::fault::Scenario {
        crate::fault::scenario("cut-body").unwrap()
    }

    #[test]
    fn a_runs_own_name_is_taken_out_where_it_is_a_whole_word() {
        assert_eq!(unplace("etl-clean/state", "clean"), "etl-{trial}/state");
        assert_eq!(unplace("clean", "clean"), "{trial}");
        assert_eq!(unplace("a/clean/b/clean", "clean"), "a/{trial}/b/{trial}");
        assert_eq!(
            unplace("run_lost-answer.json", "lost-answer"),
            "run_{trial}.json"
        );
        // inside another word it is that word's
        assert_eq!(unplace("cleaned/x", "clean"), "cleaned/x");
        assert_eq!(unplace("errors/x", "error"), "errors/x");
        assert_eq!(unplace("unclean", "clean"), "unclean");
        assert_eq!(unplace("cleanclean", "clean"), "cleanclean");
        assert_eq!(unplace("x", ""), "x");
    }

    #[test]
    fn with_the_runs_name_in_its_key_a_program_that_never_retries_still_broke() {
        // the docs' own form: s3://scratch/etl-{trial}/. It writes `state`
        // twice and swallows errors: its second write is not the retry of
        // the first, whatever the key is called in each run
        let clean = vec![
            put(1, 0, "etl-clean/state", 200),
            put(2, 205, "etl-clean/state", 200),
        ];
        let run = vec![
            fault(put(1, 0, "etl-slowdown/state", 503), "slowdown"),
            put(2, 205, "etl-slowdown/state", 200),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!(
            (t.faults, t.retried, t.dropped_writes),
            (1, 0, 1),
            "{}",
            t.line()
        );
        assert_eq!(t.backoff_ms, None, "its own sleep is no backoff");
        assert_eq!(t.standing, Standing::Broke);
        // and one that retries survived
        let run = vec![
            fault(put(1, 0, "etl-slowdown/state", 503), "slowdown"),
            put(2, 25, "etl-slowdown/state", 200),
            put(3, 230, "etl-slowdown/state", 200),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!(
            (t.retried, t.standing),
            (1, Standing::Survived),
            "{}",
            t.line()
        );
        // the name in a listing's prefix and in a batch's body
        let list = |id, at, prefix: &str, status| {
            let mut r = rec(id, at, "GET", "ListObjectsV2", "", status);
            r.key = None;
            r.query = vec![("prefix".into(), prefix.into())];
            r
        };
        let batch = |id, at, run: &str, status| {
            let mut r = rec(id, at, "POST", "DeleteObjects", "", status);
            r.key = None;
            let body = format!("<Delete><Object><Key>etl-{run}/a</Key></Object></Delete>");
            r.request_headers.push(Header::new(
                crate::fault::BODY_HASH_HEADER,
                crate::fault::body_hash(body.as_bytes()),
            ));
            r.request_body = Some(body);
            r
        };
        let clean = vec![
            list(1, 0, "etl-clean/", 200),
            list(2, 100, "etl-clean/", 200),
            batch(3, 200, "clean", 200),
            batch(4, 300, "clean", 200),
        ];
        let run = vec![
            fault(list(1, 0, "etl-slowdown/", 503), "slowdown"),
            list(2, 100, "etl-slowdown/", 200),
            fault(batch(3, 200, "slowdown", 503), "slowdown"),
            batch(4, 300, "slowdown", 200),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!(
            (t.faults, t.retried, t.dropped_writes),
            (2, 0, 1),
            "{}",
            t.line()
        );
        assert_eq!(t.read_again, 1, "the listing is asked for twice anyway");
    }

    #[test]
    fn a_key_that_holds_the_scenarios_word_by_itself_is_matched_as_written() {
        let error = crate::fault::scenario("error").unwrap();
        // `error/log` is the program's own key in both runs
        let clean = vec![put(1, 0, "error/log", 200), put(2, 200, "error/log", 200)];
        let run = vec![
            fault(put(1, 0, "error/log", 500), "error"),
            put(2, 200, "error/log", 200),
        ];
        let t = judge(&error, &clean, 0, &run, 0);
        assert_eq!((t.retried, t.dropped_writes), (0, 1), "{}", t.line());
        // and `clean/x` is in the trial what it was in the clean run
        let clean = vec![put(1, 0, "clean/x", 200), put(2, 200, "clean/x", 200)];
        let run = vec![
            fault(put(1, 0, "clean/x", 503), "slowdown"),
            put(2, 200, "clean/x", 200),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.retried, t.dropped_writes), (0, 1), "{}", t.line());
        // `errors/x` is never touched
        let clean = vec![put(1, 0, "errors/x", 200)];
        let run = vec![
            fault(put(1, 0, "errors/x", 500), "error"),
            put(2, 30, "errors/x", 200),
        ];
        assert_eq!(judge(&error, &clean, 0, &run, 0).retried, 1);
    }

    #[test]
    fn what_the_clean_run_itself_failed_is_not_owed() {
        // a store without DeleteObjects answers 501, clean run and trial
        let batch = |id, at, status| {
            let mut r = rec(id, at, "POST", "DeleteObjects", "", status);
            r.key = None;
            r.request_body = Some("<Delete/>".into());
            r
        };
        let clean = vec![put(1, 0, "a", 200), batch(2, 10, 501)];
        let run = vec![
            fault(put(1, 0, "a", 503), "slowdown"),
            put(2, 30, "a", 200),
            fault(batch(3, 40, 503), "slowdown"),
            batch(4, 70, 501),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Survived, "{}", t.line());
        assert_eq!((t.faults, t.retried, t.dropped_writes), (2, 2, 0));
        // the SDK does not send a 501 again (nor did it in the clean run):
        // a failure that was the last word is not owed either
        let run = vec![
            fault(put(1, 0, "a", 503), "slowdown"),
            put(2, 30, "a", 200),
            fault(batch(3, 40, 503), "slowdown"),
        ];
        let clean_failed = vec![put(1, 0, "a", 200), batch(2, 10, 503)];
        let t = judge(&slowdown(), &clean_failed, 0, &run, 0);
        assert_eq!((t.not_retryable, t.dropped_writes), (1, 0), "{}", t.line());
        assert_eq!(t.standing, Standing::Survived);
        // a 501 sonde answers is no more owed a retry than a 404
        let s = crate::fault::Scenario {
            name: "no".into(),
            about: String::new(),
            rules: Vec::new(),
        };
        let run = vec![fault(put(1, 0, "a", 501), "no")];
        let t = judge(&s, &clean, 0, &run, 0);
        assert_eq!((t.not_retryable, t.dropped_writes), (1, 0));
        // and a 408 is
        let run = vec![fault(put(1, 0, "a", 408), "no")];
        assert_eq!(judge(&s, &clean, 0, &run, 0).dropped_writes, 1);
    }

    #[test]
    fn a_wait_is_measured_within_one_sender() {
        // sixteen threads write one key at once: the write after the failed
        // one is another thread's, 4 ms on, not its retry 900 ms later
        let clean: Vec<Record> = (0..4).map(|i| put(i + 1, i as i64 * 2, "k", 200)).collect();
        let run = vec![
            fault(put(1, 0, "k", 503), "slowdown"),
            put(2, 2, "k", 200),
            put(3, 4, "k", 200),
            put(4, 6, "k", 200),
            put(5, 900, "k", 200),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.retried, t.standing), (1, Standing::Survived));
        assert_eq!(t.backoff_ms, None, "{}", t.line());
        assert_eq!(t.line(), "1 fault, retried · 1.2× the requests");
        // two places in the code that write one key, one after the other
        let from = |mut r: Record, place: &str| {
            r.request_headers
                .push(Header::new(crate::callers::CALLER_HEADER, place));
            r
        };
        let clean = vec![put(1, 0, "k", 200), put(2, 100, "k", 200)];
        let run = vec![
            from(fault(put(1, 0, "k", 503), "slowdown"), "a.py:1 in f"),
            from(put(2, 100, "k", 200), "a.py:9 in g"),
            from(put(3, 400, "k", 200), "a.py:1 in f"),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.retried, t.backoff_ms), (1, None));
    }

    #[test]
    fn jitter_is_not_read_from_waits_too_short_to_be_the_programs() {
        // a fixed 10 ms backoff, as the machine timed it
        let waits = [9, 12, 10, 14, 10];
        let (mut clean, mut run) = (Vec::new(), Vec::new());
        for (k, w) in waits.iter().enumerate() {
            let key = format!("k{k}");
            clean.push(put(k as u64, 0, &key, 200));
            run.push(fault(put(100 + k as u64, 0, &key, 503), "slowdown"));
            run.push(put(200 + k as u64, 5 + w, &key, 200));
        }
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.jitter, t.jitter_too_short), (None, true));
        assert_eq!(
            t.line(),
            "5 faults, all retried · after 9 ms to 14 ms (too short to tell jitter) · 2.0× the requests"
        );
    }

    #[test]
    fn a_delete_made_twice_and_a_create_the_store_was_asked_not_to_repeat_are_not_written_twice() {
        let s = crate::fault::scenario("lost-answer").unwrap();
        let del = |id, at, status| rec(id, at, "DELETE", "DeleteObject", "a", status);
        let clean = vec![
            del(1, 0, 204),
            create(2, 10, "lock", 200),
            put(3, 20, "b", 200),
        ];
        let run = vec![
            lost(del(1, 0, 500), 204),
            del(2, 30, 204),
            lost(create(3, 40, "lock", 500), 200),
            // (a store that ignores the condition makes it again)
            create(4, 70, "lock", 200),
            lost(put(5, 80, "b", 500), 200),
            put(6, 110, "b", 200),
        ];
        let t = judge(&s, &clean, 0, &run, 0);
        assert_eq!((t.retried, t.wrote_twice), (3, 1), "{}", t.line());
    }

    #[test]
    fn a_body_cut_short_is_made_good_by_a_read_from_the_cut_on_or_of_the_whole() {
        let clean = vec![part(1, 0, "big", 0, 999, 1000)];
        // resumed: bytes 500 to the end
        let run = vec![
            cut(part(1, 0, "big", 0, 999, 1000), 500),
            part(2, 30, "big", 500, 999, 1000),
        ];
        let t = judge(&cut_scenario(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Survived, "{}", t.line());
        assert_eq!(
            (t.faults, t.retried, t.cut, t.resumed, t.reread),
            (1, 1, 1, 1, 0)
        );
        assert_eq!(
            t.line(),
            "1 fault, retried · 1 body cut: 1 resumed from the cut · after 25 ms · 2.0× the requests"
        );
        // read again whole
        let run = vec![
            cut(part(1, 0, "big", 0, 999, 1000), 500),
            part(2, 30, "big", 0, 999, 1000),
        ];
        let t = judge(&cut_scenario(), &clean, 0, &run, 0);
        assert_eq!(
            (t.standing, t.resumed, t.reread),
            (Standing::Survived, 0, 1)
        );
        // resumed in two pieces
        let run = vec![
            cut(part(1, 0, "big", 0, 999, 1000), 500),
            part(2, 30, "big", 500, 699, 1000),
            part(3, 60, "big", 700, 999, 1000),
        ];
        assert_eq!(judge(&cut_scenario(), &clean, 0, &run, 0).resumed, 1);
    }

    #[test]
    fn half_a_body_taken_as_the_whole_is_broke_and_so_is_a_hole() {
        let clean = vec![part(1, 0, "big", 0, 999, 1000), put(2, 50, "out", 200)];
        // it wrote its output from the half, and exited as the clean run did
        let run = vec![
            cut(part(1, 0, "big", 0, 999, 1000), 500),
            put(2, 50, "out", 200),
        ];
        let t = judge(&cut_scenario(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Broke);
        assert_eq!((t.faults, t.retried, t.dropped_reads), (1, 0, 1));
        assert_eq!(
            t.line(),
            "1 fault, 0 retried · 1 body cut · 1 download cut short and the rest never read: the half was taken as the whole (exit 0, as the clean run) · at GetObject b/big · check the bytes read against Content-Length, and read the rest (Range) or the whole again"
        );
        let w = t.at.as_ref().unwrap();
        assert_eq!(
            (w.operation.as_str(), w.key.as_str()),
            ("GetObject", "b/big")
        );
        // it saw the short body and failed: broke by its exit, not by the half
        let t = judge(&cut_scenario(), &clean, 0, &run[..1], 1);
        assert_eq!(t.standing, Standing::Broke);
        assert!(
            t.reasons
                .contains(&"1 download cut short and the rest never read".to_string())
                && t.reasons
                    .contains(&"exited 1 (the clean run exited 0)".to_string()),
            "{:?}",
            t.reasons
        );
        // resumed from past the cut: bytes 500 to 599 are missing
        let run = vec![
            cut(part(1, 0, "big", 0, 999, 1000), 500),
            part(2, 30, "big", 600, 999, 1000),
        ];
        let t = judge(&cut_scenario(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Broke);
        assert!(
            t.reasons[0].contains("bytes are missing"),
            "{:?}",
            t.reasons
        );
        // a range's own cut is made good by that range's rest, not by the next range
        let clean = vec![
            part(1, 0, "big", 0, 499, 1000),
            part(2, 10, "big", 500, 999, 1000),
        ];
        let run = vec![
            cut(part(1, 0, "big", 0, 499, 1000), 250),
            part(2, 10, "big", 500, 999, 1000),
        ];
        let t = judge(&cut_scenario(), &clean, 0, &run, 0);
        assert_eq!(
            (t.standing, t.retried),
            (Standing::Broke, 0),
            "{}",
            t.line()
        );
        // a key read twice on purpose: the second read is not the first one's
        let clean = vec![
            part(1, 0, "big", 0, 999, 1000),
            part(2, 100, "big", 0, 999, 1000),
        ];
        let run = vec![
            cut(part(1, 0, "big", 0, 999, 1000), 500),
            part(2, 100, "big", 0, 999, 1000),
        ];
        let t = judge(&cut_scenario(), &clean, 0, &run, 0);
        assert_eq!(
            (t.standing, t.retried),
            (Standing::Broke, 0),
            "{}",
            t.line()
        );
    }

    #[test]
    fn a_lost_race_is_run_again_by_the_next_commit_in_the_log() {
        let log = |n: u64| format!("t/_delta_log/{n:020}.json");
        let clean = vec![
            put(1, 0, "t/part-1.parquet", 200),
            create(2, 10, &log(1), 200),
        ];
        // told 412, it reads the log and commits the next number
        let run = vec![
            put(1, 0, "t/part-1.parquet", 200),
            race(create(2, 10, &log(1), 412), Some(200)),
            get(3, 20, &log(1), 200),
            create(4, 45, &log(2), 200),
        ];
        let t = judge(&lost_race_scenario(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Survived, "{}", t.line());
        assert_eq!((t.faults, t.races, t.retried, t.attempts), (1, 1, 1, 2));
        assert_eq!(t.backoff_ms, Some([30.0, 30.0, 30.0]));
        // it wrote its data again under another name and left the first
        let mut again = run.clone();
        again.insert(3, put(9, 30, "t/part-2.parquet", 200));
        let t = judge(&lost_race_scenario(), &clean, 0, &again, 0);
        assert_eq!(t.standing, Standing::Degraded);
        assert!(
            t.reasons[0].starts_with("1 more object left than the clean run leaves"),
            "{:?}",
            t.reasons
        );
        // it never commits again and exits 0: the commit is lost in silence
        let t = judge(&lost_race_scenario(), &clean, 0, &run[..2], 0);
        assert_eq!(t.standing, Standing::Broke);
        assert_eq!(
            t.reasons,
            [
                "1 conditional write lost its race and the commit was never made, and the program exited 0"
            ]
        );
        assert_eq!(t.at.as_ref().unwrap().key, format!("b/{}", log(1)));
        assert_eq!(t.advice.as_ref().unwrap().key, "trial.advice.race");
        // it reads the key it lost, finds its write there (sonde let it
        // through) and carries on: the store holds what the clean run's
        // does, and nothing more
        let t = judge(&lost_race_scenario(), &clean, 0, &run[..3], 0);
        assert_eq!(t.standing, Standing::Survived, "{}", t.line());
        assert_eq!((t.read_back, t.retried, t.dropped_writes), (1, 0, 0));
        assert_eq!((t.at.clone(), t.advice.clone()), (None, None));
        assert_eq!(
            t.line(),
            "1 fault, 0 retried · 1 lost write read back and not written again: the store has it (sonde let it through), so no commit is missing · 1.5× the requests"
        );
        // as Lance does: its other commits follow, each made
        let more = vec![
            create(1, 0, &log(1), 200),
            create(2, 50, &log(2), 200),
            create(3, 90, &log(3), 200),
        ];
        let lance = vec![
            race(create(1, 0, &log(1), 412), Some(200)),
            get(2, 10, &log(1), 200),
            create(3, 50, &log(2), 200),
            create(4, 90, &log(3), 200),
        ];
        let t = judge(&lost_race_scenario(), &more, 0, &lance, 0);
        assert_eq!(
            (t.standing, t.read_back),
            (Standing::Survived, 1),
            "{}",
            t.line()
        );
        // read back, and a commit it meant to make after it never made
        let t = judge(&lost_race_scenario(), &more, 0, &lance[..3], 0);
        assert_eq!(t.standing, Standing::Broke, "{}", t.line());
        // an If-Match answered without the store has nothing to read back
        // (the next test), and a 412 never looked into is given up
        // two commits meant, the first lost and never run again
        let clean2 = vec![create(1, 0, &log(1), 200), create(2, 100, &log(2), 200)];
        let run2 = vec![
            race(create(1, 0, &log(1), 412), Some(200)),
            create(2, 100, &log(2), 200),
        ];
        let t = judge(&lost_race_scenario(), &clean2, 0, &run2, 0);
        assert_eq!(
            (t.retried, t.standing),
            (0, Standing::Broke),
            "{}",
            t.line()
        );
        // it writes the contested key without its condition
        let run = vec![
            race(create(2, 10, &log(1), 412), Some(200)),
            put(3, 40, &log(1), 200),
        ];
        let t = judge(&lost_race_scenario(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Broke);
        assert!(
            t.reasons.iter().any(|r| r.contains("without a condition")),
            "{:?}",
            t.reasons
        );
    }

    #[test]
    fn a_fenced_slatedb_writer_that_stops_is_not_broke() {
        let wal = |n: u64| format!("db/wal/{n:020}.sst");
        let clean = vec![create(1, 0, &wal(1), 200), create(2, 50, &wal(2), 200)];
        let run = vec![race(create(1, 0, &wal(1), 412), Some(200))];
        let t = judge(&lost_race_scenario(), &clean, 0, &run, 1);
        assert_eq!(t.standing, Standing::Survived, "{}", t.line());
        assert!(t.fenced);
        assert_eq!(
            t.line(),
            "1 fault, 0 retried · fenced: it stopped at the 412 (exit 1) and wrote nothing after, as SlateDB means a writer to · 0.5× the requests"
        );
        // fenced, and it wrote on all the same
        let run = vec![
            race(create(1, 0, &wal(1), 412), Some(200)),
            put(2, 30, "db/compacted/x.sst", 200),
        ];
        let t = judge(&lost_race_scenario(), &clean, 0, &run, 1);
        assert_eq!((t.standing, t.fenced), (Standing::Broke, false));
        // any other log is not fenced by a 412: stopping is giving up
        let clean = vec![create(1, 0, "t/_delta_log/00000000000000000001.json", 200)];
        let run = vec![race(
            create(1, 0, "t/_delta_log/00000000000000000001.json", 412),
            Some(200),
        )];
        let t = judge(&lost_race_scenario(), &clean, 0, &run, 1);
        assert_eq!((t.standing, t.fenced), (Standing::Broke, false));
    }

    #[test]
    fn an_if_match_sent_again_unchanged_would_lose_again() {
        let swap = |id, at, etag: &str, status| {
            let mut r = put(id, at, "pointer", status);
            r.request_headers.push(Header::new("If-Match", etag));
            r
        };
        let clean = vec![get(1, 0, "pointer", 200), swap(2, 10, "\"v1\"", 200)];
        let run = vec![
            get(1, 0, "pointer", 200),
            race(swap(2, 10, "\"v1\"", 412), None),
            swap(3, 40, "\"v1\"", 200),
        ];
        let t = judge(&lost_race_scenario(), &clean, 0, &run, 0);
        assert_eq!(
            (t.retried, t.standing),
            (1, Standing::Degraded),
            "{}",
            t.line()
        );
        // read again first: another ETag, or the same one read anew
        let run = vec![
            get(1, 0, "pointer", 200),
            race(swap(2, 10, "\"v1\"", 412), None),
            get(3, 20, "pointer", 200),
            swap(4, 40, "\"v2\"", 200),
        ];
        let t = judge(&lost_race_scenario(), &clean, 0, &run, 0);
        assert_eq!(
            (t.retried, t.standing),
            (1, Standing::Survived),
            "{}",
            t.line()
        );
    }

    #[test]
    fn a_broke_verdict_says_where_and_what_to_change() {
        let from = |mut r: Record| {
            r.request_headers.push(Header::new(
                crate::callers::CALLER_HEADER,
                "/home/me/job/etl.py:12 in save",
            ));
            r.request_headers.push(Header::new(
                "User-Agent",
                "Boto3/1.34.0 md/Botocore#1.34.0 ua/2.0",
            ));
            r
        };
        let clean = vec![put(1, 0, "state", 200), get(2, 10, "job", 200)];
        let run = vec![
            from(fault(get(2, 0, "job", 503), "slowdown")),
            from(fault(put(1, 10, "state", 503), "slowdown")),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Broke);
        // the write, though the read failed first
        assert_eq!(
            t.at,
            Some(Where {
                operation: "PutObject".into(),
                key: "b/state".into(),
                caller: Some("/home/me/job/etl.py:12 in save".into()),
            })
        );
        let a = t.advice.as_ref().unwrap();
        assert_eq!(a.key, "trial.advice.retries");
        assert_eq!(a.args["sdk"], "boto3 1.34");
        assert!(a.args["setting"].contains("retries="), "{a:?}");
        let line = t.line();
        assert!(
            line.ends_with(&format!(
                " · at PutObject b/state (etl.py:12 in save) · boto3 1.34 did not send it again: turn its retries on ({})",
                a.args["setting"]
            )),
            "{line}"
        );
        // as JSON, for the app
        let j = serde_json::to_value(&t).unwrap();
        assert_eq!(j["where"]["operation"], "PutObject");
        assert_eq!(j["where"]["caller"], "/home/me/job/etl.py:12 in save");
        assert_eq!(j["advice"]["key"], "trial.advice.retries");
        let back: Trial = serde_json::from_value(j).unwrap();
        assert_eq!(back, t);
        // a program that survived has neither
        let run = vec![
            fault(put(1, 0, "state", 503), "slowdown"),
            put(2, 30, "state", 200),
            get(3, 40, "job", 200),
        ];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.at, t.advice), (None, None));
        // an SDK sonde has no setting for: the rule itself
        let run = vec![fault(put(1, 0, "state", 503), "slowdown")];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!(t.advice.unwrap().key, "trial.advice.retry");
    }
}
