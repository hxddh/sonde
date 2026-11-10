//! A trial: one run of a program under a fault scenario, and what the
//! program did about it, read from what the store (and sonde in its place)
//! saw. "Survived" claims only what can be seen from here: every failed
//! request was sent again, nothing more was left behind than a clean run
//! leaves, and the program exited as its clean run did.

use std::collections::BTreeMap;

use reqlog::Record;
use serde::{Deserialize, Serialize};

use crate::episode::{episodes, observations};
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
    /// Of those, the ones the program sent again.
    pub retried: usize,
    /// Reads and writes it never sent again.
    pub dropped_reads: usize,
    pub dropped_writes: usize,
    /// The most attempts at one request.
    pub attempts: u32,
    /// The waits before a retry, in milliseconds: shortest, median, longest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backoff_ms: Option<[f64; 3]>,
    /// Whether the waits differ enough to be jittered (retries in step make
    /// a throttled store's load arrive in waves).
    pub jitter: bool,
    /// Requests under the scenario and in the clean run.
    pub requests: usize,
    pub clean_requests: usize,
    /// Multipart uploads left unfinished, beyond the clean run's.
    pub left_uploads: usize,
    /// Writes the store made twice: made, their answer lost, made again.
    pub wrote_twice: usize,
    /// Why it is not "survived", a line each.
    pub reasons: Vec<String>,
    /// The session file of the trial's run, for opening it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// One request, whichever attempt: its method, place and part (an upload's
/// id is the store's and changes when the whole upload is started again).
fn identity(r: &Record) -> String {
    let q = |n: &str| {
        r.query
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    };
    let listing = if r.key.is_none() {
        format!(
            "{}|{}|{}",
            q("prefix"),
            q("continuation-token"),
            q("marker")
        )
    } else {
        String::new()
    };
    format!(
        "{} {} {}/{} {} {listing}",
        r.method,
        r.operation,
        r.bucket.as_deref().unwrap_or(""),
        r.key.as_deref().unwrap_or(""),
        q("partNumber")
    )
}

/// Whether sonde failed this request (a delay is injected too, and is not
/// a failure).
pub fn faulted(r: &Record) -> bool {
    r.injected.is_some()
        && r.response_headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("x-sonde-fault"))
}

fn lost_answer(r: &Record) -> bool {
    r.response_headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("x-sonde-store-status") && h.value.starts_with('2'))
}

fn unfinished_uploads(records: &[Record]) -> usize {
    let eps = episodes(records);
    observations(records, &eps)
        .iter()
        .find(|o| o.phrases.first().is_some_and(|p| p.key == "obs.unfinished"))
        .and_then(|o| o.phrases[0].args.get("n")?.parse().ok())
        .unwrap_or(0)
}

/// Reads the trial: `clean` is the run without faults, `run` the one with.
pub fn judge(
    scenario: &crate::fault::Scenario,
    clean: &[Record],
    clean_exit: i32,
    run: &[Record],
    exit: i32,
) -> Trial {
    let mut by_request: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for r in run {
        by_request.entry(identity(r)).or_default().push(r);
    }
    let (mut faults, mut retried, mut dropped_reads, mut dropped_writes) = (0, 0, 0, 0);
    let (mut attempts, mut wrote_twice) = (0u32, 0);
    let mut waits: Vec<f64> = Vec::new();
    for group in by_request.values_mut() {
        group.sort_by_key(|r| (r.time, r.id));
        attempts = attempts.max(group.len() as u32);
        for (i, r) in group.iter().enumerate() {
            if !faulted(r) {
                continue;
            }
            faults += 1;
            match group.get(i + 1) {
                Some(next) => {
                    retried += 1;
                    let answered =
                        r.time + chrono::Duration::microseconds((r.latency_ms * 1000.0) as i64);
                    let wait =
                        (next.time - answered).num_microseconds().unwrap_or(0) as f64 / 1000.0;
                    waits.push(wait.max(0.0));
                    if lost_answer(r) {
                        wrote_twice += 1;
                    }
                }
                // a write the store made and the program was told failed:
                // the data is there, the program moved on
                None if lost_answer(r) => dropped_reads += 1,
                None if r.method == "GET" || r.method == "HEAD" => dropped_reads += 1,
                None => dropped_writes += 1,
            }
        }
    }
    waits.sort_by(|a, b| a.total_cmp(b));
    let backoff_ms =
        (!waits.is_empty()).then(|| [waits[0], waits[waits.len() / 2], waits[waits.len() - 1]]);
    // jitter: the waits are not all one figure (within a tenth of the median)
    let jitter = backoff_ms.is_some_and(|[_, mid, _]| {
        waits.len() >= 4
            && waits
                .iter()
                .filter(|w| (*w - mid).abs() > mid.max(1.0) * 0.1)
                .count()
                * 4
                >= waits.len()
    });
    let left_uploads = unfinished_uploads(run).saturating_sub(unfinished_uploads(clean));

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
            if left_uploads == 1 { "" } else { "s" }
        ));
    }
    if dropped_reads > 0 {
        standing = Standing::Degraded;
        reasons.push(format!(
            "gave up on {dropped_reads} failed request{} without sending {} again",
            if dropped_reads == 1 { "" } else { "s" },
            if dropped_reads == 1 { "it" } else { "them" }
        ));
    }
    if dropped_writes > 0 && exit == 0 {
        standing = Standing::Broke;
        reasons.push(format!(
            "{dropped_writes} write{} failed, {} never sent again, and the program exited 0",
            if dropped_writes == 1 { "" } else { "s" },
            if dropped_writes == 1 { "was" } else { "were" }
        ));
    }
    if clean_exit != 0 && standing == Standing::Survived {
        // a program that fails without faults says nothing by its exit
        standing = Standing::Degraded;
        reasons.push(format!("the clean run exited {clean_exit} itself"));
    }
    if exit != clean_exit {
        standing = Standing::Broke;
        reasons.push(format!("exited {exit} (the clean run exited {clean_exit})"));
    }
    Trial {
        scenario: scenario.name.clone(),
        about: scenario.about.clone(),
        standing,
        exit,
        clean_exit,
        faults,
        retried,
        dropped_reads,
        dropped_writes,
        attempts,
        backoff_ms,
        jitter,
        requests: run.len(),
        clean_requests: clean.len(),
        left_uploads,
        wrote_twice,
        reasons,
        session: None,
    }
}

/// Where a run's trials are kept: beside its session, so that the app and
/// `sonde check` read what the last trial found.
pub fn file_for(session: &std::path::Path) -> std::path::PathBuf {
    let name = session
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    session.with_file_name(format!("{}.trials.json", name.trim_end_matches(".sonde")))
}

/// The folder a run's trial sessions are recorded in, beside its session.
pub fn folder_for(session: &std::path::Path) -> std::path::PathBuf {
    session.with_file_name("trials")
}

/// Removes what a run's trial left (when the run itself is deleted).
pub fn remove(session: &std::path::Path) {
    for t in load(session) {
        if let Some(p) = t.session {
            let _ = std::fs::remove_file(p);
        }
    }
    let _ = std::fs::remove_file(file_for(session));
    let _ = std::fs::remove_dir(folder_for(session));
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

impl Trial {
    /// What happened, in a line: the figures behind the standing.
    pub fn line(&self) -> String {
        let mut parts = Vec::new();
        if self.faults > 0 {
            let n = format!(
                "{} fault{}",
                self.faults,
                if self.faults == 1 { "" } else { "s" }
            );
            parts.push(if self.retried == self.faults {
                format!("{n}, {}retried", if self.faults == 1 { "" } else { "all " })
            } else {
                format!("{n}, {} retried", self.retried)
            });
        }
        if let Some([lo, mid, hi]) = self.backoff_ms {
            parts.push(if (hi - lo).abs() < 1.0 {
                format!("after {}", ms_text(mid))
            } else {
                format!(
                    "after {} to {} ({})",
                    ms_text(lo),
                    ms_text(hi),
                    if self.jitter { "jittered" } else { "in step" }
                )
            });
        }
        if self.clean_requests > 0 && self.requests != self.clean_requests {
            parts.push(format!(
                "{:.1}× the requests",
                self.requests as f64 / self.clean_requests as f64
            ));
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
    fn a_write_dropped_in_silence_is_broke_and_a_read_given_up_is_degraded() {
        let clean = vec![rec(1, 0, "PUT", "PutObject", "a", 200)];
        let run = vec![fault(rec(1, 0, "PUT", "PutObject", "a", 503), "slowdown")];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Broke);
        assert!(t.reasons[0].contains("never sent again"), "{:?}", t.reasons);

        let run = vec![fault(rec(1, 0, "GET", "GetObject", "a", 503), "slowdown")];
        let t = judge(&slowdown(), &clean, 0, &run, 0);
        assert_eq!(t.standing, Standing::Degraded);
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
        let mut lost = fault(rec(1, 0, "PUT", "PutObject", "a", 500), "lost-answer");
        lost.response_headers
            .push(Header::new("x-sonde-store-status", "200"));
        let run = vec![lost, rec(2, 50, "PUT", "PutObject", "a", 200)];
        let t = judge(&s, &clean, 0, &run, 0);
        assert_eq!((t.standing, t.wrote_twice), (Standing::Survived, 1));
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
