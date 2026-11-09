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

/// A command's shell words as typed, quotes kept (`TOKEN="ab cd"` is one
/// word), and `;`, `&&`, `||`, `|`, `&` as words of their own.
fn shell_words(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    let flush = |word: &mut String, words: &mut Vec<String>| {
        if !word.is_empty() {
            words.push(std::mem::take(word));
        }
    };
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                word.push(c);
                if c == '\\' && q == '"' {
                    if let Some(n) = chars.next() {
                        word.push(n);
                    }
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    word.push(c);
                }
                '\\' => {
                    word.push(c);
                    if let Some(n) = chars.next() {
                        word.push(n);
                    }
                }
                ';' | '|' | '&' => {
                    flush(&mut word, &mut words);
                    let mut op = c.to_string();
                    if chars.peek() == Some(&c) && c != ';' {
                        op.push(chars.next().unwrap());
                    }
                    words.push(op);
                }
                c if c.is_whitespace() => flush(&mut word, &mut words),
                c => word.push(c),
            },
        }
    }
    flush(&mut word, &mut words);
    words
}

fn is_separator(word: &str) -> bool {
    matches!(word, ";" | "&&" | "||" | "|" | "&")
}

/// The word without its quotes and escapes.
fn unquote(word: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '\\') | (Some('"'), '\\') => out.extend(chars.next()),
            (_, c) => out.push(c),
        }
    }
    out
}

/// Commands that set the stage for the program rather than being it.
const STAGE: &[&str] = &[
    "cd", "sleep", "echo", "export", "set", "source", ".", "true", "time", "nice", "exec",
    "printf", "wait", "nohup",
];

/// The program a command runs, by name (`python` for
/// `AWS_SECRET_ACCESS_KEY=… /usr/bin/python etl.py`): past the variables it
/// sets, `env` and its options, never one of their values. Of a compound
/// command (`cd etl && python load.py`), the first that is not a step
/// setting the stage.
pub fn program_name(command: &str) -> Option<String> {
    let words = shell_words(command);
    let mut first = None;
    for part in words.split(|w| is_separator(w)) {
        let mut it = part.iter().peekable();
        let mut name = None;
        while let Some(w) = it.next() {
            if is_assignment(w) {
                continue;
            }
            if w == "env" {
                // env's own options; -u and -C take a value
                while let Some(o) = it.peek() {
                    if is_assignment(o) {
                        it.next();
                    } else if o.starts_with('-') {
                        let takes = matches!(o.as_str(), "-u" | "-C" | "--unset" | "--chdir");
                        it.next();
                        if takes {
                            it.next();
                        }
                    } else {
                        break;
                    }
                }
                continue;
            }
            name = std::path::Path::new(&unquote(w))
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .filter(|f| !f.is_empty());
            break;
        }
        let Some(name) = name else { continue };
        if !STAGE.contains(&name.as_str()) {
            return Some(name);
        }
        first.get_or_insert(name);
    }
    first
}

/// The command with the value of every variable it sets hidden
/// (`AWS_SECRET_ACCESS_KEY=…`), quoted values whole: what may leave the machine.
pub fn scrub_command(command: &str) -> String {
    shell_words(command)
        .into_iter()
        .map(|w| match w.split_once('=') {
            Some((name, _)) if is_assignment(&w) => format!("{name}=…"),
            _ => w,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A command as one line a shell runs as it was given: each argument quoted
/// when it needs to be (how `sonde run -- prog "my file.py"` is kept).
pub fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            let plain = !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c));
            if plain {
                a.clone()
            } else if cfg!(windows) {
                format!("\"{}\"", a.replace('"', "\\\""))
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
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
    // a name of its own: the app and the CLI may write at the same time
    let tmp = dir.join(format!(
        "baselines.json.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
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
/// What a program's request cost comes to in a month at the pace it runs:
/// its runs' average cost times how often it ran (runs a day, from the
/// span of `runs`' start times). None with fewer than three runs or less
/// than an hour between the first and the last: no pace to go by.
pub fn monthly_cost(runs: &[RunPoint]) -> Option<(f64, f64)> {
    if runs.len() < 3 {
        return None;
    }
    let at = |r: &RunPoint| chrono::DateTime::parse_from_rfc3339(&r.created).ok();
    let (first, last) = (at(runs.first()?)?, at(runs.last()?)?);
    let days = (last - first).num_seconds() as f64 / 86_400.0;
    if days < 1.0 / 24.0 {
        return None;
    }
    let per_day = (runs.len() - 1) as f64 / days;
    let average = runs.iter().map(|r| r.cost).sum::<f64>() / runs.len() as f64;
    Some((average * per_day * 30.0, per_day))
}

/// What a run may use against its baseline with `tolerance` more (0.1 is
/// 10%): the limits `sonde check` holds a run to, and the app marks a
/// program "past its baseline" by.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineLimits {
    pub requests: f64,
    pub waiting_ms: f64,
    pub cost: f64,
    pub errors: f64,
}

pub const DEFAULT_TOLERANCE: f64 = 0.1;

pub fn baseline_limits(base: &RunPoint, tolerance: f64) -> BaselineLimits {
    let grow = 1.0 + tolerance;
    BaselineLimits {
        // a hair over, so 100 × 1.57 is 157 and not 156.99…
        requests: (base.requests as f64 * grow + 1e-9)
            .floor()
            .max(base.requests as f64),
        // a run this short varies more than its tolerance: a few ms of slack
        waiting_ms: base.busy_ms * grow + 5.0,
        // a free baseline (deletes only) still lets a run cost a hundredth of a cent
        cost: (base.cost * grow).max(base.cost + 0.0001),
        errors: base.errors as f64,
    }
}

/// The run is past what its baseline allows (see [`baseline_limits`]).
pub fn past_baseline(run: &RunPoint, base: &RunPoint, tolerance: f64) -> bool {
    let l = baseline_limits(base, tolerance);
    run.requests as f64 > l.requests
        || run.busy_ms > l.waiting_ms
        || run.cost > l.cost
        || run.errors as f64 > l.errors
}

/// Where a program's runs went past their baseline: the first run of the
/// stretch of runs past it that ends with the newest, and the run before it
/// (the last within), each with its commit when the run recorded one. What
/// to look at between the two commits is what made it worse.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Since {
    /// How many runs ago the first run past the baseline was (0: the newest).
    pub runs_ago: usize,
    pub first_past: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_past_commit: Option<crate::session::GitStamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_within: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_within_commit: Option<crate::session::GitStamp>,
}

/// [`Since`] of `runs` (oldest first, the newest last), against `base`.
/// None when the newest run is within it.
pub fn since(
    runs: &[(RunPoint, Option<crate::session::GitStamp>)],
    base: &RunPoint,
    tolerance: f64,
) -> Option<Since> {
    let past = |r: &RunPoint| r.path != base.path && past_baseline(r, base, tolerance);
    if !past(&runs.last()?.0) {
        return None;
    }
    let mut first = runs.len() - 1;
    while first > 0 && past(&runs[first - 1].0) {
        first -= 1;
    }
    let good = first.checked_sub(1).map(|i| &runs[i]);
    Some(Since {
        runs_ago: runs.len() - 1 - first,
        first_past: runs[first].0.path.clone(),
        first_past_commit: runs[first].1.clone(),
        last_within: good.map(|g| g.0.path.clone()),
        last_within_commit: good.and_then(|g| g.1.clone()),
    })
}

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
            let l = baseline_limits(base, t);
            limit("requests", r.requests as f64, l.requests, "baseline");
            limit("waiting", r.busy_ms, l.waiting_ms, "baseline");
            limit("cost", r.cost, l.cost, "baseline");
            limit("errors", r.errors as f64, l.errors, "baseline");
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
    fn a_month_at_the_pace_a_program_runs() {
        let pt = |h: u32, cost: f64| RunPoint {
            path: String::new(),
            created: format!("2026-11-01T{h:02}:00:00Z"),
            requests: 1,
            busy_ms: 1.0,
            wall_ms: 1.0,
            cost,
            currency: "USD".into(),
            errors: 0,
            warnings: 0,
            findings: 0,
        };
        // every hour, a cent a run: 24 runs a day, $7.20 a month
        let (month, per_day) = monthly_cost(&[pt(0, 0.01), pt(1, 0.01), pt(2, 0.01)]).unwrap();
        assert!((per_day - 24.0).abs() < 1e-9);
        assert!((month - 7.2).abs() < 1e-9);
        assert_eq!(monthly_cost(&[pt(0, 0.01), pt(1, 0.01)]), None);
    }

    #[test]
    fn the_run_where_a_program_went_past_its_baseline() {
        let pt = |path: &str, requests: usize| RunPoint {
            path: path.into(),
            created: String::new(),
            requests,
            busy_ms: 10.0,
            wall_ms: 10.0,
            cost: 0.0,
            currency: "USD".into(),
            errors: 0,
            warnings: 0,
            findings: 0,
        };
        let git = |c: &str| {
            Some(crate::session::GitStamp {
                commit: c.into(),
                branch: None,
                dirty: false,
            })
        };
        let base = pt("a", 100);
        let runs = vec![
            (pt("a", 100), git("aaa")),
            (pt("b", 105), git("bbb")),
            (pt("c", 140), git("ccc")),
            (pt("d", 150), git("ddd")),
        ];
        let s = since(&runs, &base, DEFAULT_TOLERANCE).unwrap();
        assert_eq!(s.runs_ago, 1);
        assert_eq!(s.first_past, "c");
        assert_eq!(s.first_past_commit.unwrap().commit, "ccc");
        assert_eq!(s.last_within.as_deref(), Some("b"));
        assert_eq!(s.last_within_commit.unwrap().commit, "bbb");
        // within again: nothing to say
        let mut back = runs.clone();
        back.push((pt("e", 101), None));
        assert_eq!(since(&back, &base, DEFAULT_TOLERANCE), None);
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
        // quoted values are one word, and stay hidden whole
        let q = r#"TOKEN="ab cd" SECRET='x y' ./app --flag "my file""#;
        assert_eq!(program_name(q).as_deref(), Some("app"));
        assert_eq!(
            scrub_command(q),
            r#"TOKEN=… SECRET=… ./app --flag "my file""#
        );
        assert_eq!(
            program_name("env -i X=1 python a.py").as_deref(),
            Some("python")
        );
        assert_eq!(
            program_name("env -u HOME python a.py").as_deref(),
            Some("python")
        );
        // a compound command: the program, not the step before it
        assert_eq!(program_name("sleep 4; curl -s x").as_deref(), Some("curl"));
        assert_eq!(
            program_name("cd etl && A=1 python load.py").as_deref(),
            Some("python")
        );
        assert_eq!(program_name("cd etl").as_deref(), Some("cd"));
        assert_eq!(
            shell_join(&["python".into(), "my script.py".into(), "it's".into()]),
            if cfg!(windows) {
                r#"python "my script.py" "it's""#
            } else {
                r#"python 'my script.py' 'it'\''s'"#
            }
        );
    }
}
