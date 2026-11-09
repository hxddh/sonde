//! A program over its runs: the sessions of one command, each reduced to the
//! figures that move when the program changes (requests, waiting, cost,
//! errors, findings), and a run checked against a budget or a baseline run:
//! what `sonde check` gates CI on, and what the app's history shows.

use serde::{Deserialize, Serialize};

use crate::episode::Observation;
use crate::profile::Profile;
use crate::summary::is_miss;
use reqlog::Record;

/// The program a session ran, as runs of it are grouped: its command with
/// spaces collapsed. A replay or a recording without a command has none.
pub fn program_key(command: Option<&str>) -> Option<String> {
    let c = command?.split_whitespace().collect::<Vec<_>>().join(" ");
    if c.is_empty() || c.starts_with("replay of ") {
        None
    } else {
        Some(c)
    }
}

/// A shell word that sets a variable for the command: `NAME=value`.
fn is_assignment(word: &str) -> bool {
    match word.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.starts_with(|c: char| c.is_ascii_digit())
        }
        None => false,
    }
}

/// The program a command runs, by name (`python` for
/// `AWS_SECRET_ACCESS_KEY=… /usr/bin/python etl.py`): past the variables it
/// sets and `env`, never one of their values.
pub fn program_name(command: &str) -> Option<String> {
    let word = command
        .split_whitespace()
        .find(|w| !is_assignment(w) && *w != "env")?;
    std::path::Path::new(word)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .filter(|f| !f.is_empty())
}

/// The command with the value of every variable it sets hidden
/// (`AWS_SECRET_ACCESS_KEY=…`): what may leave the machine.
pub fn scrub_command(command: &str) -> String {
    command
        .split_whitespace()
        .map(|w| match w.split_once('=') {
            Some((name, _)) if is_assignment(w) => format!("{name}=…"),
            _ => w.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Each program's baseline run, by file name: `baselines.json` beside the sessions.
pub fn read_baselines(dir: &std::path::Path) -> std::collections::BTreeMap<String, String> {
    std::fs::read(dir.join("baselines.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn write_baselines(
    dir: &std::path::Path,
    all: &std::collections::BTreeMap<String, String>,
) -> std::io::Result<()> {
    let tmp = dir.join("baselines.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(all)?)?;
    std::fs::rename(&tmp, dir.join("baselines.json"))
}

/// A run, by the figures that move when the program changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPoint {
    pub path: String,
    pub created: String,
    pub requests: usize,
    pub busy_ms: f64,
    pub wall_ms: f64,
    pub cost: f64,
    pub currency: String,
    pub errors: usize,
    pub warnings: usize,
    pub findings: usize,
}

pub fn run_point(
    path: &str,
    created: &str,
    records: &[Record],
    profile: &Profile,
    findings: &[Observation],
) -> RunPoint {
    RunPoint {
        path: path.into(),
        created: created.into(),
        requests: records.len(),
        busy_ms: profile.busy_ms,
        wall_ms: profile.wall_ms,
        cost: profile.cost.amount,
        currency: profile.cost.currency.clone(),
        errors: records
            .iter()
            .filter(|r| r.is_error() && !is_miss(r))
            .count(),
        warnings: findings.iter().filter(|o| o.level == "warn").count(),
        findings: findings.len(),
    }
}

/// What a run may use: limits of its own, and how much more than its
/// baseline (a share: 0.1 is 10%).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    pub max_requests: Option<usize>,
    pub max_waiting_ms: Option<f64>,
    pub max_cost: Option<f64>,
    pub max_errors: Option<usize>,
    /// Against the baseline: requests, waiting and cost may grow this much.
    pub tolerance: Option<f64>,
    /// Against the baseline: no warning it did not have.
    pub no_new_warnings: bool,
}

/// One line of a check.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    /// `requests`, `waiting`, `cost`, `errors` or `warnings`.
    pub what: &'static str,
    pub value: f64,
    pub limit: f64,
    /// Where the limit comes from: `budget` or `baseline`.
    pub against: &'static str,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub passed: bool,
    pub verdicts: Vec<Verdict>,
    /// Warnings the baseline did not have, as they read.
    pub new_warnings: Vec<String>,
}

/// A run against a budget, and against a baseline run when there is one.
pub fn check(
    run: (&RunPoint, &[Record], &Profile, &[Observation]),
    baseline: Option<(&RunPoint, &[Record], &Profile, &[Observation])>,
    budget: &Budget,
) -> Check {
    let (r, ..) = run;
    let mut v = Vec::new();
    let mut limit = |what, value: f64, limit: f64, against| {
        v.push(Verdict {
            what,
            value,
            limit,
            against,
            ok: value <= limit + 1e-12,
        })
    };
    if let Some(n) = budget.max_requests {
        limit("requests", r.requests as f64, n as f64, "budget");
    }
    if let Some(ms) = budget.max_waiting_ms {
        limit("waiting", r.busy_ms, ms, "budget");
    }
    if let Some(c) = budget.max_cost {
        limit("cost", r.cost, c, "budget");
    }
    if let Some(n) = budget.max_errors {
        limit("errors", r.errors as f64, n as f64, "budget");
    }
    let mut new_warnings = Vec::new();
    if let Some(b) = baseline {
        let base = b.0;
        if let Some(t) = budget.tolerance {
            let grow = 1.0 + t;
            limit(
                "requests",
                r.requests as f64,
                // a hair over, so 100 × 1.57 is 157 and not 156.99…
                (base.requests as f64 * grow + 1e-9)
                    .floor()
                    .max(base.requests as f64),
                "baseline",
            );
            // a run this short varies more than its tolerance: a few ms of slack
            limit("waiting", r.busy_ms, base.busy_ms * grow + 5.0, "baseline");
            // a free baseline (deletes only) still lets a run cost a hundredth of a cent
            limit(
                "cost",
                r.cost,
                (base.cost * grow).max(base.cost + 0.0001),
                "baseline",
            );
            limit("errors", r.errors as f64, base.errors as f64, "baseline");
        }
        if budget.no_new_warnings {
            // a warning the baseline did not have as a warning: new, or grown from a note
            let was_warning: std::collections::HashSet<String> =
                b.3.iter()
                    .filter(|o| o.level == "warn")
                    .map(crate::profile::finding_identity)
                    .collect();
            new_warnings = run
                .3
                .iter()
                .filter(|o| {
                    o.level == "warn" && !was_warning.contains(&crate::profile::finding_identity(o))
                })
                .map(|o| o.text.clone())
                .collect();
            limit("warnings", new_warnings.len() as f64, 0.0, "baseline");
        }
    }
    Check {
        passed: v.iter().all(|x| x.ok),
        verdicts: v,
        new_warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::{episodes, observations};
    use crate::profile::{RequestPrices, profile};
    use chrono::DateTime;

    fn rec(id: u64, key: &str) -> Record {
        Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 10).unwrap(),
            method: "GET".into(),
            operation: "GetObject".into(),
            bucket: Some("b".into()),
            key: Some(key.into()),
            status: Some(200),
            latency_ms: 4.0,
            ..Default::default()
        }
    }

    fn point(rs: &[Record]) -> (RunPoint, Profile, Vec<Observation>) {
        let p = profile(rs, &RequestPrices::default());
        let o = observations(rs, &episodes(rs));
        (run_point("x", "", rs, &p, &o), p, o)
    }

    #[test]
    fn programs_are_their_commands_and_replays_are_none() {
        assert_eq!(
            program_key(Some("  python  etl.py  --day 1 ")).as_deref(),
            Some("python etl.py --day 1")
        );
        assert_eq!(program_key(Some("replay of etl")), None);
        assert_eq!(program_key(None), None);
    }

    #[test]
    fn a_run_within_budget_and_one_grown_past_its_baseline() {
        let base: Vec<Record> = (0..10).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        // the change reads one key again and again
        let mut grown = base.clone();
        grown.extend((0..12).map(|i| rec(100 + i, "hot")));
        let (pb, prb, ob) = point(&base);
        let (pg, prg, og) = point(&grown);
        let budget = Budget {
            max_requests: Some(50),
            tolerance: Some(0.1),
            no_new_warnings: true,
            ..Default::default()
        };
        let ok = check(
            (&pb, &base, &prb, &ob),
            Some((&pb, &base, &prb, &ob)),
            &budget,
        );
        assert!(ok.passed, "{ok:#?}");
        let c = check(
            (&pg, &grown, &prg, &og),
            Some((&pb, &base, &prb, &ob)),
            &budget,
        );
        assert!(!c.passed);
        let req = c
            .verdicts
            .iter()
            .find(|v| v.what == "requests" && v.against == "baseline")
            .unwrap();
        assert_eq!((req.value, req.limit, req.ok), (22.0, 11.0, false));
        assert!(
            c.verdicts
                .iter()
                .any(|v| v.what == "requests" && v.against == "budget" && v.ok)
        );
    }

    #[test]
    fn nothing_to_check_passes_nothing_and_a_free_baseline_allows_a_cent() {
        let base: Vec<Record> = (0..10).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        let (pb, prb, ob) = point(&base);
        let empty = check((&pb, &base, &prb, &ob), None, &Budget::default());
        assert!(empty.verdicts.is_empty());
        // a baseline that cost nothing, a run that cost a read
        let mut free = pb.clone();
        free.cost = 0.0;
        let mut run = pb.clone();
        run.cost = 4e-7;
        let c = check(
            (&run, &base, &prb, &ob),
            Some((&free, &base, &prb, &ob)),
            &Budget {
                tolerance: Some(0.1),
                ..Default::default()
            },
        );
        assert!(
            c.verdicts.iter().find(|v| v.what == "cost").unwrap().ok,
            "{c:#?}"
        );
    }

    #[test]
    fn a_tolerance_of_57_percent_allows_157_of_100() {
        let base: Vec<Record> = (0..10).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        let (mut pb, prb, ob) = point(&base);
        pb.requests = 100;
        let mut run = pb.clone();
        run.requests = 157;
        let c = check(
            (&run, &base, &prb, &ob),
            Some((&pb, &base, &prb, &ob)),
            &Budget {
                tolerance: Some(0.57),
                ..Default::default()
            },
        );
        assert!(
            c.verdicts.iter().find(|v| v.what == "requests").unwrap().ok,
            "{c:#?}"
        );
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn a_secret_set_for_the_command_never_names_it() {
        let c = "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI env X=1 /usr/bin/python etl.py --day 1";
        assert_eq!(program_name(c).as_deref(), Some("python"));
        let s = scrub_command(c);
        assert!(!s.contains("wJalr"), "{s}");
        assert_eq!(
            s,
            "AWS_SECRET_ACCESS_KEY=… env X=… /usr/bin/python etl.py --day 1"
        );
        assert_eq!(program_name("aws s3 ls").as_deref(), Some("aws"));
        assert_eq!(program_name("A=1 B=2"), None);
    }
}
