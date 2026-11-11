//! A trial: one run of a program under a fault scenario, and what the
//! program did about it, read from what the store (and sonde in its place)
//! saw. "Survived" claims only what can be seen from here: every failed
//! request was sent again, nothing more was left behind than a clean run
//! leaves, and the program exited as its clean run did.
//!
//! A retry is counted against the clean run. A request is named by
//! [`crate::fault::identity`]; the clean run says how many times the program
//! means to send each. A failed request was sent again only if the trial run
//! got at least that many answers for it: a program that writes `state`,
//! waits and writes `state` again did not retry its first write because a
//! second one followed.

use std::collections::{BTreeMap, HashMap};

use reqlog::Record;
use serde::{Deserialize, Serialize};

use crate::episode::{episodes, observations};
use crate::fault::identity_of;
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
    /// Requests sonde failed in the store's place.
    pub faults: usize,
    /// Of those, the ones the program sent again (see the module's note).
    pub retried: usize,
    /// Of the faults, the ones no retry is owed for (a 404, a 403: any 4xx
    /// but 408 and 429). They are neither retried nor dropped.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub not_retryable: usize,
    /// Reads and writes it never sent again.
    pub dropped_reads: usize,
    pub dropped_writes: usize,
    /// Of the dropped writes, the ones the store made all the same: their
    /// answer was lost, and the program did not ask again.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub lost_writes: usize,
    /// The most attempts at one request: its failures in a row, and the
    /// attempt that followed them.
    pub attempts: u32,
    /// The waits before a retry, in milliseconds: shortest, median, longest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backoff_ms: Option<[f64; 3]>,
    /// Whether the waits before one attempt number differ enough to be
    /// jittered (retries in step make a throttled store's load arrive in
    /// waves). Not said with fewer than four waits before the same attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<bool>,
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
    pub wrote_twice: usize,
    /// Why it is not "survived", a line each.
    pub reasons: Vec<String>,
    /// The trial run's session: its file's name in the run's `trials/`
    /// folder ([`session_path`] gives the path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Whether sonde failed this request (a delay is injected too, and is not
/// a failure).
pub fn faulted(r: &Record) -> bool {
    r.injected.is_some()
        && r.response_headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("x-sonde-fault"))
}

/// What the store answered a request whose answer sonde lost.
pub fn store_status(r: &Record) -> Option<u16> {
    r.response_headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("x-sonde-store-status"))
        .and_then(|h| h.value.trim().parse().ok())
}

/// The store did what was asked, and the program was told it failed.
fn lost_answer(r: &Record) -> bool {
    store_status(r).is_some_and(|s| (200..300).contains(&s))
}

fn is_write(r: &Record) -> bool {
    matches!(r.method.as_str(), "PUT" | "POST" | "DELETE")
}

/// The program got the store's own answer, and not one that asks for
/// another attempt: a 404 or a 412 is an answer, a 503 is not.
fn answered(r: &Record) -> bool {
    !faulted(r)
        && r.transport_error.is_none()
        && r.status.is_some_and(|s| s < 500 && s != 429 && s != 408)
}

/// Whether a failed request is one a program should send again: a 5xx, a
/// 408 or a 429. A 404 or a 403 is the answer, however often it is asked.
fn retry_owed(r: &Record) -> bool {
    r.status.is_none_or(|s| s >= 500 || s == 408 || s == 429)
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

/// From the first request's start to the last answer, in ms.
fn span_ms(records: &[Record]) -> f64 {
    let start = records.iter().map(|r| r.time).min();
    let end = records
        .iter()
        .map(|r| r.time + chrono::Duration::microseconds(r.latency_us()))
        .max();
    match (start, end) {
        (Some(s), Some(e)) => (e - s).num_microseconds().unwrap_or(0) as f64 / 1000.0,
        _ => 0.0,
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Whether waits before one attempt number are jittered: a quarter of them
/// or more are a tenth off their median.
fn jittered(waits: &mut [f64]) -> bool {
    waits.sort_by(|a, b| a.total_cmp(b));
    let mid = waits[waits.len() / 2];
    waits
        .iter()
        .filter(|w| (*w - mid).abs() > mid.max(1.0) * 0.1)
        .count()
        * 4
        >= waits.len()
}

/// Reads the trial: `clean` is the run without faults, `run` the one with.
pub fn judge(
    scenario: &crate::fault::Scenario,
    clean: &[Record],
    clean_exit: i32,
    run: &[Record],
    exit: i32,
) -> Trial {
    // how many times the program means to send each request
    let mut meant: HashMap<String, usize> = HashMap::new();
    for r in clean {
        *meant.entry(identity_of(r)).or_default() += 1;
    }
    let mut by_request: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for r in run {
        by_request.entry(identity_of(r)).or_default().push(r);
    }
    let (mut faults, mut retried, mut not_retryable) = (0, 0, 0);
    let (mut dropped_reads, mut dropped_writes, mut lost_writes) = (0, 0, 0);
    let (mut attempts, mut wrote_twice) = (0u32, 0);
    // (which attempt the wait came before, the wait)
    let mut waits: Vec<(u32, f64)> = Vec::new();
    // retries of a write the store had made, which the store then refused
    let mut refused: Vec<String> = Vec::new();
    for (id, group) in by_request.iter_mut() {
        group.sort_by_key(|r| (r.time, r.id));
        let failed: Vec<usize> = (0..group.len()).filter(|&i| faulted(group[i])).collect();
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
        // the program is short of this many answers: that many failures were
        // never made good, and a program gives up on the last ones
        let short = match meant.get(id).copied().unwrap_or(0) {
            // a request the clean run never sent (a key named by the clock)
            // is meant once, and only an answer after its failure makes it good
            0 => {
                let after = group[owed[0]..].iter().filter(|r| answered(r)).count();
                1usize.saturating_sub(after)
            }
            want => want.saturating_sub(group.iter().filter(|r| answered(r)).count()),
        };
        // a failure nothing followed was not sent again, whatever the count
        let last = (owed.last() == Some(&(group.len() - 1))) as usize;
        let unretried = owed.len().min(short.max(last));
        let made_good = owed.len() - unretried;
        retried += made_good;
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
                if !is_write(r) {
                    dropped_reads += 1;
                } else {
                    dropped_writes += 1;
                    lost_writes += lost_answer(r) as usize;
                }
                continue;
            }
            let next = group[i + 1];
            attempts = attempts.max(streak + 1);
            let told = r.time + chrono::Duration::microseconds(r.latency_us());
            let wait = (next.time - told).num_microseconds().unwrap_or(0) as f64 / 1000.0;
            waits.push((streak, wait.max(0.0)));
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
                } else {
                    wrote_twice += 1;
                }
            }
        }
    }
    let mut all: Vec<f64> = waits.iter().map(|(_, w)| *w).collect();
    all.sort_by(|a, b| a.total_cmp(b));
    let backoff_ms = (!all.is_empty()).then(|| [all[0], all[all.len() / 2], all[all.len() - 1]]);
    // jitter: within one attempt number (a backoff that doubles is not
    // jittered by doubling), and only with enough waits to tell
    let mut by_attempt: BTreeMap<u32, Vec<f64>> = BTreeMap::new();
    for (n, w) in &waits {
        by_attempt.entry(*n).or_default().push(*w);
    }
    let told: Vec<bool> = by_attempt
        .values_mut()
        .filter(|w| w.len() >= 4)
        .map(|w| jittered(w))
        .collect();
    let jitter = (!told.is_empty()).then(|| told.iter().any(|j| *j));
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
    if clean_exit != 0 && standing == Standing::Survived {
        // a program that fails without faults says nothing by its exit
        standing = Standing::Degraded;
        reasons.push(format!("the clean run exited {clean_exit} itself"));
    }
    if exit != clean_exit {
        standing = Standing::Broke;
        reasons.push(format!("exited {exit} (the clean run exited {clean_exit})"));
        // and why, where the requests say
        refused.sort();
        refused.dedup();
        reasons.extend(refused);
    }
    Trial {
        scenario: scenario.name.clone(),
        about: scenario.about.clone(),
        standing,
        exit,
        clean_exit,
        faults,
        retried,
        not_retryable,
        dropped_reads,
        dropped_writes,
        lost_writes,
        attempts,
        backoff_ms,
        jitter,
        requests: run.len(),
        clean_requests: clean.len(),
        delayed,
        wall_ms: span_ms(run),
        clean_wall_ms: span_ms(clean),
        left_uploads,
        wrote_twice,
        reasons,
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
    /// What happened, in a line: the figures behind the standing.
    pub fn line(&self) -> String {
        let mut parts = Vec::new();
        let owed = self.faults.saturating_sub(self.not_retryable);
        if self.faults > 0 {
            let n = format!("{} fault{}", self.faults, plural(self.faults));
            parts.push(if owed == 0 {
                // a 404 is not sent again: the verdict is the exit's
                format!("{n}, none a retry is owed for")
            } else if self.retried == owed {
                format!("{n}, {}retried", if owed == 1 { "" } else { "all " })
            } else {
                format!("{n}, {} retried", self.retried)
            });
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
        parts.join(" · ")
    }
}

/// The scenarios worth a program's time, from what its clean run did: the
/// two every program meets, the lost answer if it writes, the failed
/// completion if it uploads in parts.
pub fn choose(clean: &[Record]) -> Vec<crate::fault::Scenario> {
    let has = |f: &dyn Fn(&Record) -> bool| clean.iter().any(f);
    let mut names = vec!["slowdown", "error"];
    if has(&|r| r.method == "PUT" || r.method == "POST" || r.method == "DELETE") {
        names.push("lost-answer");
    }
    if has(&|r| r.operation == "CompleteMultipartUpload") {
        names.push("fail-complete");
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
    fn a_poll_is_retried_when_the_program_gets_the_answers_it_meant_to() {
        let clean: Vec<Record> = (0..4)
            .map(|i| get(i + 1, i as i64 * 100, "job", 200))
            .collect();
        // the first poll fails, and the loop polls on: four answers still
        let mut run = vec![fault(get(1, 0, "job", 503), "slowdown")];
        run.extend((1..5).map(|i| get(i + 1, i as i64 * 100, "job", 200)));
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.retried, t.dropped_reads), (1, 0));
        assert_eq!(t.standing, Standing::Survived);
        // a loop that counts its polls is one answer short: a read given up
        run.pop();
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!((t.retried, t.dropped_reads), (0, 1));
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
            "1 fault, none a retry is owed for · exited 3 (the clean run exited 0)"
        );
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
    }
}
