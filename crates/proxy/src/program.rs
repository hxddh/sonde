//! A program over its runs: the sessions of one command, each reduced to the
//! figures that move when the program changes (requests, waiting, cost,
//! errors, findings), and a run checked against a budget or a baseline run:
//! what `sonde check` gates CI on, and what the app's history shows.

use serde::{Deserialize, Serialize};

use crate::episode::Observation;
use crate::profile::Profile;
use crate::summary::is_miss;
use reqlog::Record;

/// The program a session ran, as runs of it are grouped: its command's
/// words (quotes undone, spaces collapsed) with what changes from run to run
/// said as what it is ([`crate::profile::stable`]: `etl.py --date
/// 2026-11-14` and `--date 2026-11-13` are one program), and a trial's place
/// left out (`{trial}`, `$SONDE_TRIAL`: adding one keeps the program's
/// history). A replay, an imported log or a recording without a command has
/// none.
pub fn program_key(command: Option<&str>) -> Option<String> {
    let c = command?.trim();
    if c.is_empty() || c.starts_with("replay of ") || c.starts_with("imported from ") {
        return None;
    }
    let words: Vec<String> = shell_words(c)
        .iter()
        .map(|w| without_trial(&unquote(w)))
        .collect();
    let words: Vec<&str> = words.iter().flat_map(|w| w.split_whitespace()).collect();
    Some(crate::profile::stable(&words.join(" ")))
}

/// A word without the trial's place in it (`{trial}`, `$SONDE_TRIAL`,
/// `${SONDE_TRIAL}`, `%SONDE_TRIAL%`) and the one separator before it:
/// `s3://b/etl-{trial}/` is `s3://b/etl/`.
fn without_trial(word: &str) -> String {
    let mut w = word.to_string();
    for place in ["{trial}", "${SONDE_TRIAL}", "$SONDE_TRIAL", "%SONDE_TRIAL%"] {
        while let Some(at) = w.find(place) {
            let from = w[..at]
                .char_indices()
                .next_back()
                .filter(|(_, c)| "-_./=:".contains(*c))
                .map_or(at, |(i, _)| i);
            w.replace_range(from..at + place.len(), "");
        }
    }
    w
}

/// The program a session is a run of: the name it was given, else its
/// command ([`program_key`]). A named program keeps its runs and its
/// baseline when an argument or the interpreter's path changes.
pub fn key_of(header: &crate::session::SessionHeader) -> Option<String> {
    // (a replay or an import of a named run is still none)
    program_key(header.command.as_deref())?;
    match header.name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => Some(name.to_string()),
        _ => program_key(header.command.as_deref()),
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
                // an escape only before what it escapes: `C:\Python\x.exe`
                // is a Windows path, not escapes
                '\\' if chars
                    .peek()
                    .is_some_and(|n| n.is_whitespace() || "\\'\";|&$`".contains(*n)) =>
                {
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
    let mut chars = word.chars().peekable();
    while let Some(c) = chars.next() {
        // an escape only before what it escapes (a Windows path keeps its backslashes)
        let escapes = chars
            .peek()
            .is_some_and(|n| n.is_whitespace() || "\\'\";|&$`".contains(*n));
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '\\') | (Some('"'), '\\') if escapes => out.extend(chars.next()),
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
            // its file's name, a path written with either separator
            name = unquote(w)
                .rsplit(['/', '\\'])
                .next()
                .map(String::from)
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
        .map(|w| scrub_word(&w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A word with any secret in it hidden: a variable's value, an option that
/// names a secret (`--password=…`), and both inside a quoted command
/// (`sh -c 'KEY=… python x.py'`).
fn scrub_word(w: &str) -> String {
    if let Some((name, _)) = w.split_once('=') {
        if is_assignment(w) {
            return format!("{name}=…");
        }
        let lower = name.to_ascii_lowercase();
        if name.starts_with('-')
            && ["secret", "password", "passwd", "token", "key", "credential"]
                .iter()
                .any(|s| lower.contains(s))
        {
            return format!("{name}=…");
        }
    }
    for q in ['\'', '"'] {
        if w.len() >= 2 && w.starts_with(q) && w.ends_with(q) {
            let inner = &w[1..w.len() - 1];
            if inner.contains('=') {
                return format!("{q}{}{q}", scrub_command(inner));
            }
        }
    }
    w.to_string()
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
                // backslashes before the closing quote would escape it
                let body = a.replace('"', "\\\"");
                let tail = body.len() - body.trim_end_matches('\\').len();
                format!("\"{body}{}\"", "\\".repeat(tail))
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A map beside the sessions that is there and does not read.
#[derive(Debug, Clone, PartialEq)]
pub struct Unreadable {
    pub path: std::path::PathBuf,
    pub why: String,
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} cannot be read ({}): fix or remove the file",
            self.path.display(),
            self.why
        )
    }
}

impl std::error::Error for Unreadable {}

/// A JSON map beside the sessions (`titles.json`, `baselines.json`,
/// `imports.json`): empty when there is none, an error when it is there and
/// does not read. Read as empty, a file that is only damaged would say
/// nothing is set, and the next write would drop every entry in it.
pub fn read_map(
    path: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, String>, Unreadable> {
    let unreadable = |why: String| Unreadable {
        path: path.to_path_buf(),
        why,
    };
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| unreadable(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(unreadable(e.to_string())),
    }
}

/// Each program's baseline run, by file name: `baselines.json` beside the
/// sessions. None set when there is no file; a damaged one is an error.
pub fn read_baselines(
    dir: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, String>, Unreadable> {
    read_map(&dir.join("baselines.json"))
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
    reqlog::private::write(&tmp, serde_json::to_vec_pretty(all)?)?;
    std::fs::rename(&tmp, dir.join("baselines.json"))
}

/// What a program keeps beside its runs, by its key: its baseline, its
/// trials' verdicts, its findings, what it wrote.
pub const KEPT_BY_PROGRAM: &[&str] = &[
    "baselines.json",
    "verdicts.json",
    "findings.json",
    "written.json",
];

/// Moves what program `from` keeps in `dir` ([`KEPT_BY_PROGRAM`]) to
/// program `to`: a program given a name keeps its baseline and history.
/// What `to` keeps already stays. Returns the files it moved something in.
pub fn rename_program(
    dir: &std::path::Path,
    from: &str,
    to: &str,
) -> std::io::Result<Vec<&'static str>> {
    let mut moved = Vec::new();
    for name in KEPT_BY_PROGRAM {
        let path = dir.join(name);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let mut all: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if all.contains_key(to) {
            continue;
        }
        let Some(kept) = all.remove(from) else {
            continue;
        };
        all.insert(to.to_string(), kept);
        let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
        reqlog::private::write(&tmp, serde_json::to_vec_pretty(&all)?)?;
        std::fs::rename(&tmp, &path)?;
        moved.push(*name);
    }
    Ok(moved)
}

/// A run, by the figures that move when the program changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPoint {
    pub path: String,
    pub created: String,
    pub requests: usize,
    pub waiting_ms: f64,
    pub wall_ms: f64,
    pub cost: f64,
    pub currency: String,
    pub errors: usize,
    pub warnings: usize,
    pub findings: usize,
    /// How long 95% of its requests took at most (0 when unknown).
    pub p95_ms: f64,
    /// The first request the recording policy refused: sonde's answer,
    /// why a program that failed did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
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
        waiting_ms: profile.waiting_ms,
        wall_ms: profile.wall_ms,
        p95_ms: profile.p95_ms,
        cost: profile.cost.amount,
        currency: profile.cost.currency.clone(),
        errors: records
            .iter()
            .filter(|r| r.is_error() && !is_miss(r))
            .count(),
        warnings: findings.iter().filter(|o| o.level == "warn").count(),
        findings: findings.len(),
        refused: records
            .iter()
            .find(|r| crate::refused_by_policy(r))
            .and_then(|r| r.error_body.clone()),
    }
}

/// The figures a budget holds a run to, kept as its requests arrive: a
/// proxy left running holds eight bytes a request (its latency, for the
/// p95) and the stretches of time with a request in flight, not the records.
/// The same figures as [`run_point`] of the same records.
#[derive(Debug, Clone)]
pub struct RunFigures {
    prices: crate::profile::RequestPrices,
    requests: usize,
    errors: usize,
    cost: f64,
    latencies: Vec<f64>,
    /// Disjoint stretches with a request in flight, start to end, in µs.
    busy: std::collections::BTreeMap<i64, i64>,
    /// The first request's start and the last one's end.
    span: Option<(i64, i64)>,
}

impl RunFigures {
    pub fn new(prices: crate::profile::RequestPrices) -> RunFigures {
        RunFigures {
            prices,
            requests: 0,
            errors: 0,
            cost: 0.0,
            latencies: Vec::new(),
            busy: Default::default(),
            span: None,
        }
    }

    pub fn requests(&self) -> usize {
        self.requests
    }

    pub fn add(&mut self, r: &Record) {
        self.requests += 1;
        self.errors += usize::from(r.is_error() && !is_miss(r));
        self.cost += crate::profile::price(r, &self.prices);
        self.latencies.push(r.latency_ms.max(0.0));
        let mut start = r.time.timestamp_micros();
        let mut end = start + r.latency_us();
        self.span = Some(match self.span {
            Some((s, e)) => (s.min(start), e.max(end)),
            None => (start, end),
        });
        // no time in flight adds none (a log without latencies stays empty)
        if end <= start {
            return;
        }
        // the stretches it touches become one with it
        while let Some((&s, &e)) = self.busy.range(..=end).next_back()
            && e >= start
        {
            self.busy.remove(&s);
            (start, end) = (start.min(s), end.max(e));
        }
        self.busy.insert(start, end);
    }

    /// The run so far, as a point (no path, no findings: a budget's limits
    /// are on the figures).
    pub fn point(&self) -> RunPoint {
        let mut sorted = self.latencies.clone();
        sorted.sort_by(f64::total_cmp);
        let busy_us: i64 = self.busy.iter().map(|(s, e)| e - s).sum();
        let wall_us = self.span.map_or(0, |(s, e)| (e - s).max(1));
        RunPoint {
            path: String::new(),
            created: String::new(),
            requests: self.requests,
            waiting_ms: busy_us as f64 / 1000.0,
            wall_ms: wall_us as f64 / 1000.0,
            p95_ms: crate::profile::percentile(&sorted, 0.95),
            cost: self.cost,
            currency: self.prices.currency.clone(),
            errors: self.errors,
            warnings: 0,
            findings: 0,
            refused: None,
        }
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
    /// How long 95% of the requests may take at most, in ms.
    pub max_p95_ms: Option<f64>,
    /// Findings the project expects (a habit it keeps on purpose): never a
    /// new warning. Each matches a finding's identity, its kind
    /// (`obs.hot`) or a part of its text.
    pub expect: Vec<String>,
    /// Against the baseline: requests, waiting and cost may grow this much.
    pub tolerance: Option<f64>,
    /// Against the baseline: no warning it did not have.
    pub no_new_warnings: bool,
}

/// One line of a check.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    /// `requests`, `waiting`, `cost`, `errors`, `p95` or `warnings`.
    pub what: &'static str,
    pub value: f64,
    pub limit: f64,
    /// Where the limit comes from: `budget` or `baseline`.
    pub against: &'static str,
    /// The baseline's own figure, for a row held to the baseline: the
    /// limit is that with the tolerance (and a little slack) on top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<f64>,
    pub ok: bool,
}

impl Verdict {
    /// The row's name as it reads: `warnings` counts those the baseline did
    /// not have, and a run with an old warning is not a run with none.
    pub fn label(&self) -> &'static str {
        match self.what {
            "warnings" => "new warnings",
            what => what,
        }
    }

    /// By how much a failed row is over what it is held to: `+23%`, or the
    /// count itself where that is none at all. A budget's row is over its
    /// limit; a baseline's row is over the baseline (a run 90% over its
    /// baseline is `+90%`, not the 73% it is past baseline-and-tolerance).
    /// None for a row that passed.
    pub fn over(&self) -> Option<String> {
        if self.ok {
            return None;
        }
        let of = self.base.unwrap_or(self.limit);
        if of <= 0.0 {
            return Some(format!("+{:.0}", self.value - of.max(0.0)));
        }
        let pct = (self.value - of) / of * 100.0;
        Some(if pct < 1.0 {
            "+<1%".to_string()
        } else {
            format!("+{pct:.0}%")
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub passed: bool,
    pub verdicts: Vec<Verdict>,
    /// Warnings the baseline did not have, as they read.
    pub new_warnings: Vec<String>,
    /// Findings of the run the project expects (`expect`), as they read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected: Vec<String>,
}

/// Whether the project expects finding `o`: an entry of `expect` is its
/// identity, or its kind for every finding of that kind. A key in an entry
/// is read as an identity says it (`jobs/{n}/`) by the rules of now
/// ([`crate::profile::IDENTITY_RULES`]), whatever it was named in.
pub fn expected(o: &Observation, expect: &[String]) -> bool {
    if expect.is_empty() {
        return false;
    }
    let id = crate::profile::finding_identity(o);
    let kind = id.split('|').next().unwrap_or("");
    // an entry made by older rules is read by these; one that names what
    // two findings of a run share names both
    expect.iter().any(|e| {
        let e = e.trim();
        e == kind || e == id || expected_identity(e) == id || e == expected_identity(&id)
    })
}

/// An entry of `expect` as an identity: what it is about made
/// [`crate::profile::stable`].
fn expected_identity(e: &str) -> String {
    let mut parts = e.split('|');
    let kind = parts.next().unwrap_or("");
    let about: Vec<String> = parts.map(crate::profile::stable).collect();
    format!("{kind}|{}", about.join("|"))
}

/// Why an entry of `[check] expect` is no finding's identity, when it is
/// not: its kind is none sonde finds.
pub fn expect_error(e: &str) -> Option<String> {
    let kind = e.trim().split('|').next().unwrap_or("");
    if crate::profile::FINDING_KINDS.contains(&kind) {
        return None;
    }
    Some(format!(
        "[check] expect = {e:?} is not a finding's identity: give one as `sonde show -f json` writes it (\"obs.hot|bucket/key\"), or a kind for all of it ({})",
        crate::profile::FINDING_KINDS.join(", ")
    ))
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

/// Waiting a run may add to its baseline's whatever the tolerance: a short
/// run on a shared machine (a CI runner) varies by tens of milliseconds, and a
/// budget of 1 ms + 10% failed on that noise alone.
pub const WAITING_SLACK_MS: f64 = 50.0;

pub fn baseline_limits(base: &RunPoint, tolerance: f64) -> BaselineLimits {
    let grow = 1.0 + tolerance;
    BaselineLimits {
        // a hair over, so 100 × 1.57 is 157 and not 156.99…
        requests: (base.requests as f64 * grow + 1e-9)
            .floor()
            .max(base.requests as f64),
        // a run this short varies more than its tolerance
        waiting_ms: base.waiting_ms * grow + WAITING_SLACK_MS,
        // a free baseline (deletes only) still lets a run cost a hundredth of a
        // cent; one that cost anything is held to its percent, however small
        // (the same floor on top was 100% of a run costing 0.0001)
        cost: if base.cost > 0.0 {
            base.cost * grow
        } else {
            0.0001
        },
        errors: base.errors as f64,
    }
}

/// The run is past what its baseline allows (see [`baseline_limits`]).
pub fn past_baseline(run: &RunPoint, base: &RunPoint, tolerance: f64) -> bool {
    let l = baseline_limits(base, tolerance);
    run.requests as f64 > l.requests
        || run.waiting_ms > l.waiting_ms
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
            base: None,
            ok: value <= limit + 1e-12,
        })
    };
    if let Some(n) = budget.max_requests {
        limit("requests", r.requests as f64, n as f64, "budget");
    }
    if let Some(ms) = budget.max_waiting_ms {
        limit("waiting", r.waiting_ms, ms, "budget");
    }
    if let Some(c) = budget.max_cost {
        limit("cost", r.cost, c, "budget");
    }
    if let Some(n) = budget.max_errors {
        limit("errors", r.errors as f64, n as f64, "budget");
    }
    if let Some(ms) = budget.max_p95_ms {
        limit("p95", r.p95_ms, ms, "budget");
    }
    let mut new_warnings = Vec::new();
    if let Some(b) = baseline {
        let base = b.0;
        if let Some(t) = budget.tolerance {
            let l = baseline_limits(base, t);
            let mut held = |what, value: f64, limit: f64, base: f64| {
                v.push(Verdict {
                    what,
                    value,
                    limit,
                    against: "baseline",
                    base: Some(base),
                    ok: value <= limit + 1e-12,
                })
            };
            held(
                "requests",
                r.requests as f64,
                l.requests,
                base.requests as f64,
            );
            held("waiting", r.waiting_ms, l.waiting_ms, base.waiting_ms);
            held("cost", r.cost, l.cost, base.cost);
            held("errors", r.errors as f64, l.errors, base.errors as f64);
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
                    o.level == "warn"
                        && !was_warning.contains(&crate::profile::finding_identity(o))
                        && !expected(o, &budget.expect)
                })
                .map(|o| o.text.clone())
                .collect();
            v.push(Verdict {
                what: "warnings",
                value: new_warnings.len() as f64,
                limit: 0.0,
                against: "baseline",
                base: None,
                ok: new_warnings.is_empty(),
            });
        }
    }
    Check {
        passed: v.iter().all(|x| x.ok),
        verdicts: v,
        new_warnings,
        expected: run
            .3
            .iter()
            .filter(|o| expected(o, &budget.expect))
            .map(|o| o.text.clone())
            .collect(),
    }
}

/// A length of time ([`sonde_fmt::duration`]).
pub fn ms_text(ms: f64) -> String {
    sonde_fmt::duration(ms)
}

/// Money to two significant digits under a cent: a session's requests often
/// cost thousandths.
pub fn money_text(v: f64, currency: &str) -> String {
    if !v.is_finite() || v < 0.0 {
        return format!("— {currency}");
    }
    if v > 0.0 && v < 0.01 {
        // two significant digits, after rounding (0.00996 is 0.010, not 0.0100)
        let exp = v.log10().floor() as i32;
        let scale = 10f64.powi(1 - exp);
        let r = (v * scale).round() / scale;
        if r < 1e-6 {
            return format!("<0.000001 {currency}");
        }
        let digits = (1 - r.log10().floor() as i32).max(2) as usize;
        format!("{r:.digits$} {currency}")
    } else {
        format!("{v:.2} {currency}")
    }
}

/// Text the program controls (a finding names its keys), as Markdown that
/// shows it as written: no formatting, no HTML, no table cell break, no
/// @mention.
pub fn md_text(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '#' | '~' | '!' => {
                out.push('\\');
                out.push(c);
            }
            // a word joiner keeps GitHub from reading a mention
            '@' => out.push_str("@\u{2060}"),
            '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// A check as a pull request's comment (`sonde check -f markdown`, and the
/// app's preview of it): the verdicts, since which run, what is new, what is
/// expected, and the run's findings with the code that sent their requests.
pub fn check_markdown(
    c: &Check,
    currency: &str,
    since: Option<&str>,
    findings: &[Observation],
) -> String {
    check_markdown_with(c, currency, since, findings, &[])
}

/// What a check found, in a few words: the budget's verdict, the
/// baseline's and the trials', each said as what it is. A run within its
/// budget that is past its baseline is not "over budget", and one that
/// broke under a trial failed whatever its figures.
pub fn check_closing(c: &Check, trials: &[crate::trial::Trial]) -> String {
    use crate::trial::Standing;
    let broke = trials.iter().any(|t| t.standing == Standing::Broke);
    let tried = trials
        .iter()
        .any(|t| !matches!(t.standing, Standing::Untested | Standing::Refused));
    let of = |against: &str| {
        let mut rows = c
            .verdicts
            .iter()
            .filter(|v| v.against == against)
            .peekable();
        // (none, or whether every one passed)
        rows.peek().is_some().then(|| rows.all(|v| v.ok))
    };
    let held = match (of("budget"), of("baseline")) {
        (None, None) if c.verdicts.is_empty() => None,
        (Some(false), Some(false)) => Some("over budget and past its baseline"),
        (Some(false), _) => Some("over budget"),
        (_, Some(false)) => Some("past its baseline"),
        (None, Some(true)) => Some("within its baseline"),
        (Some(true), Some(true)) => Some("within budget and its baseline"),
        _ => Some("within budget"),
    };
    match (held, broke) {
        (None, true) => "broke under a trial".into(),
        (None, false) if tried => "did not break under its trials".into(),
        (None, false) if !trials.is_empty() => "nothing was tried".into(),
        (None, false) => "nothing checked".into(),
        (Some(held), false) => held.into(),
        (Some(held), true) => format!("{held}, and broke under a trial"),
    }
}

/// [`check_closing`] of a run whose program may have failed (`exit`, not
/// 0): a failed run is said to have failed first, whatever its figures.
pub fn run_closing(c: &Check, trials: &[crate::trial::Trial], exit: Option<i32>) -> String {
    let closing = check_closing(c, trials);
    match exit {
        Some(x) if closing == "nothing checked" => format!("the program failed (exit {x})"),
        Some(x) => format!("the program failed (exit {x}); {closing}"),
        None => closing,
    }
}

/// [`check_passed`] of a run whose program may have failed (`exit`, not
/// 0): a run that failed fails its check.
pub fn run_passed(c: &Check, trials: &[crate::trial::Trial], exit: Option<i32>) -> bool {
    exit.is_none() && check_passed(c, trials)
}

/// Whether a check, with the run's trials, passed: every row of the budget
/// and the baseline, and no trial that broke.
pub fn check_passed(c: &Check, trials: &[crate::trial::Trial]) -> bool {
    c.verdicts.iter().all(|v| v.ok)
        && !trials
            .iter()
            .any(|t| t.standing == crate::trial::Standing::Broke)
}

/// A trial's mark in a check: `pass` survived, `warn` degraded (it got
/// through, and not cleanly) or refused by the policy (not judged), `FAIL`
/// broke, `skip` untested.
pub fn trial_mark(s: crate::trial::Standing) -> &'static str {
    use crate::trial::Standing;
    match s {
        Standing::Survived => "pass",
        Standing::Degraded => "warn",
        Standing::Broke => "FAIL",
        Standing::Untested => "skip",
        Standing::Refused => "warn",
    }
}

/// A Markdown table whose columns line up as text too (a comment is read
/// in a log as often as rendered): every cell as wide as its column,
/// figures to the right (`right`).
fn md_table(head: &[&str], right: &[bool], rows: &[Vec<String>]) -> String {
    let width = |t: &str| t.chars().count();
    let w: Vec<usize> = (0..head.len())
        .map(|i| {
            rows.iter()
                .map(|r| width(&r[i]))
                .chain([width(head[i]), 3])
                .max()
                .unwrap_or(3)
        })
        .collect();
    let line = |cells: &mut dyn Iterator<Item = (usize, String)>| {
        let cells: Vec<String> = cells
            .map(|(i, c)| {
                let pad = " ".repeat(w[i].saturating_sub(width(&c)));
                if right[i] {
                    format!("{pad}{c}")
                } else {
                    format!("{c}{pad}")
                }
            })
            .collect();
        format!("| {} |\n", cells.join(" | "))
    };
    let mut t = line(&mut head.iter().map(|h| h.to_string()).enumerate());
    t.push_str(&line(&mut (0..head.len()).map(|i| {
        (
            i,
            if right[i] {
                format!("{}:", "-".repeat(w[i] - 1))
            } else {
                "-".repeat(w[i])
            },
        )
    })));
    for r in rows {
        t.push_str(&line(&mut r.iter().cloned().enumerate()));
    }
    t
}

/// [`check_markdown`], with the run's trials: their rows, and a heading
/// that says which of the two failed.
pub fn check_markdown_with(
    c: &Check,
    currency: &str,
    since: Option<&str>,
    findings: &[Observation],
    trials: &[crate::trial::Trial],
) -> String {
    check_comment(
        c,
        &Comment {
            currency,
            since,
            findings,
            trials,
            ..Default::default()
        },
    )
}

/// What a pull request's comment says besides the check's rows.
#[derive(Default)]
pub struct Comment<'a> {
    pub currency: &'a str,
    /// Since which run it is past its baseline, as a line.
    pub since: Option<&'a str>,
    /// The run's findings.
    pub findings: &'a [Observation],
    pub trials: &'a [crate::trial::Trial],
    /// The run against its baseline ([`crate::profile::compare_runs`]).
    pub change: Option<&'a crate::profile::RunChange>,
    /// Why there is no baseline, when one was asked for and none was found.
    pub no_baseline: Option<&'a str>,
    /// Where the whole report is (`sonde share --html`, kept by CI).
    pub report_url: Option<&'a str>,
    /// The program's exit status, when it is not 0, and why when the
    /// policy refused a request ([`RunPoint::refused`]).
    pub failed: Option<(i32, Option<&'a str>)>,
}

/// Waiting moves this much from one identical run to the next: a change
/// smaller than this and [`WAITING_SLACK_MS`] is not said as one.
const WAITING_NOISE: f64 = 0.1;

/// Whether waiting moved by more than a run's own noise.
pub fn waiting_moved(a: f64, b: f64) -> bool {
    (b - a).abs() >= WAITING_SLACK_MS && (b - a).abs() >= a * WAITING_NOISE
}

/// A figure before and after: `52 → 86 (+65%)`.
fn moved(a: f64, b: f64, text: &dyn Fn(f64) -> String) -> String {
    let pct = if a > 0.0 {
        format!(" ({:+.0}%)", (b - a) * 100.0 / a)
    } else {
        String::new()
    };
    format!("{} → {}{pct}", text(a), text(b))
}

/// A check as a pull request's comment: the verdict first; the rows that
/// failed, never those that passed; against a baseline only what changed
/// (the figures, the kinds of request, the findings new and gone, new first);
/// the trials that did not survive; one link to the report.
pub fn check_comment(c: &Check, m: &Comment) -> String {
    let currency = m.currency;
    let shown = |what: &str, v: f64| match what {
        "waiting" | "p95" => ms_text(v),
        "cost" => money_text(v, currency),
        _ => format!("{v:.0}"),
    };
    let mut md = format!(
        "### sonde: {} {}\n",
        run_closing(c, m.trials, m.failed.map(|(x, _)| x)),
        if run_passed(c, m.trials, m.failed.map(|(x, _)| x)) {
            "✅"
        } else {
            "❌"
        }
    );
    if let Some((code, why)) = m.failed {
        md.push_str(&format!("\n> ❌ **The program failed** (exit {code})"));
        match why {
            Some(w) => md.push_str(&format!(": {}\n", md_text(w))),
            None => md.push('\n'),
        }
    }
    if let Some(t) = m.since {
        md.push_str(&format!("\n{}\n", md_text(t)));
    }
    if let Some(why) = m.no_baseline {
        md.push_str(&format!(
            "\n> ⚠️ {}: nothing was compared\n",
            // what is missing, not how to set one on this machine
            md_text(why.split(": ").next().unwrap_or(why))
        ));
    }
    // a baseline's rows are said as what changed, below
    let said_below = |v: &Verdict| m.change.is_some() && v.against == "baseline";
    let failed: Vec<Vec<String>> = c
        .verdicts
        .iter()
        .filter(|v| !v.ok && !said_below(v))
        .map(|v| {
            vec![
                format!("**FAIL** {}", v.over().unwrap_or_default()),
                v.label().to_string(),
                shown(v.what, v.value),
                format!("≤ {}", shown(v.what, v.limit)),
                v.against.to_string(),
            ]
        })
        .collect();
    if !failed.is_empty() {
        md.push('\n');
        md.push_str(&md_table(
            &["", "", "run", "limit", "from"],
            &[false, false, true, true, false],
            &failed,
        ));
    }
    let place = |text: &str| {
        m.findings
            .iter()
            .find(|f| f.text == text)
            .and_then(|f| f.callers.first())
            .map(|c| {
                format!(
                    " — `{}`",
                    c.place.replace('`', "'").replace(['\n', '\r'], " ")
                )
            })
            .unwrap_or_default()
    };
    if let Some(ch) = m.change {
        let row = |what: &str| {
            c.verdicts
                .iter()
                .find(|v| v.what == what && v.against == "baseline")
        };
        let mut figures = Vec::new();
        for (what, a, b) in [
            ("requests", ch.requests.0 as f64, ch.requests.1 as f64),
            ("waiting", ch.waiting_ms.0, ch.waiting_ms.1),
            ("cost", ch.cost.0, ch.cost.1),
            ("errors", ch.errors.0 as f64, ch.errors.1 as f64),
        ] {
            let past = row(what).is_some_and(|v| !v.ok);
            let changed = if what == "waiting" {
                waiting_moved(a, b)
            } else {
                shown(what, a) != shown(what, b)
            };
            if !past && !changed {
                continue;
            }
            let text = format!("{what} {}", moved(a, b, &|v| shown(what, v)));
            figures.push(match row(what).filter(|_| past) {
                Some(v) => format!("**{text}**, past the {} it may use", shown(what, v.limit)),
                None => text,
            });
        }
        md.push_str("\n**Against the baseline**");
        if figures.is_empty() {
            md.push_str(": the same requests, waiting, cost and errors\n");
        } else {
            md.push_str("\n\n");
            for f in &figures {
                md.push_str(&format!("- {f}\n"));
            }
        }
        let kinds: Vec<Vec<String>> = ch
            .groups
            .iter()
            .filter(|g| {
                g.requests.0 != g.requests.1 || waiting_moved(g.waiting_ms.0, g.waiting_ms.1)
            })
            .take(8)
            .map(|g| {
                vec![
                    md_text(&if g.missed {
                        format!("{} (not found)", g.operation)
                    } else {
                        g.operation.clone()
                    }),
                    format!("{} → {}", g.requests.0, g.requests.1),
                    format!("{} → {}", ms_text(g.waiting_ms.0), ms_text(g.waiting_ms.1)),
                ]
            })
            .collect();
        if !kinds.is_empty() {
            md.push('\n');
            md.push_str(&md_table(
                &["kind", "requests", "waiting"],
                &[false, true, true],
                &kinds,
            ));
        }
        // new first: what this change brought, as `compare` says it
        let new: Vec<&str> = ch.appeared.iter().map(|f| f.text.as_str()).collect();
        if !new.is_empty() {
            md.push_str("\n**New findings**\n\n");
            for t in &new {
                md.push_str(&format!("- {}{}\n", md_text(t), place(t)));
            }
        }
        // one the baseline had as a note, a warning now
        let grown: Vec<&String> = c
            .new_warnings
            .iter()
            .filter(|w| !new.contains(&w.as_str()))
            .collect();
        if !grown.is_empty() {
            md.push_str("\n**Warnings now**\n\n");
            for t in grown {
                md.push_str(&format!("- {}{}\n", md_text(t), place(t)));
            }
        }
        if !ch.resolved.is_empty() {
            md.push_str("\n**Gone**\n\n");
            for f in &ch.resolved {
                md.push_str(&format!("- {}\n", md_text(&f.text)));
            }
        }
    } else if !c.new_warnings.is_empty() {
        md.push_str("\n**New since the baseline**\n\n");
        for w in &c.new_warnings {
            md.push_str(&format!("- {}{}\n", md_text(w), place(w)));
        }
    }
    // the trials that did not survive; the rest counted
    use crate::trial::Standing;
    let rows: Vec<Vec<String>> = m
        .trials
        .iter()
        .filter(|t| matches!(t.standing, Standing::Broke | Standing::Degraded))
        .map(|t| {
            vec![
                match trial_mark(t.standing) {
                    "FAIL" => "**FAIL**".to_string(),
                    mark => mark.to_string(),
                },
                md_text(&t.asked()),
                md_text(&t.line()),
            ]
        })
        .collect();
    if !rows.is_empty() {
        md.push('\n');
        md.push_str(&md_table(&["", "under", ""], &[false, false, false], &rows));
    }
    let survived = m
        .trials
        .iter()
        .filter(|t| t.standing == Standing::Survived)
        .count();
    if survived > 0 {
        md.push_str(&format!(
            "\n{survived} scenario{} survived\n",
            if survived == 1 { "" } else { "s" }
        ));
    }
    if !c.expected.is_empty() {
        md.push_str(&format!(
            "\n*Expected (sonde.toml): {}*\n",
            c.expected.len()
        ));
    }
    // without a baseline, what the run has: its first findings
    let findings: Vec<_> = m.findings.iter().take(5).collect();
    if m.change.is_none() && !findings.is_empty() {
        md.push_str("\n<details><summary>Findings</summary>\n\n");
        for f in findings {
            md.push_str(&format!("- {}{}\n", md_text(&f.text), place(&f.text)));
        }
        md.push_str("\n</details>\n");
    }
    if let Some(url) = m.report_url {
        // a link as given, its parentheses kept from ending it
        md.push_str(&format!(
            "\n[The report]({})\n",
            url.replace('(', "%28")
                .replace(')', "%29")
                .replace(' ', "%20")
        ));
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::{episodes, observations};
    use crate::profile::{RequestPrices, profile};
    use chrono::DateTime;

    /// A table's row as its cells, whatever they are padded to.
    fn squeezed(md: &str) -> String {
        let mut out = String::new();
        for w in md.split(' ').filter(|w| !w.is_empty()) {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
            }
            out.push_str(w);
        }
        out
    }

    #[test]
    fn damaged_baselines_are_an_error_and_missing_ones_are_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_baselines(dir.path()).unwrap().is_empty(), "none set");
        let mut all = std::collections::BTreeMap::new();
        all.insert("python etl.py".to_string(), "a.sonde".to_string());
        write_baselines(dir.path(), &all).unwrap();
        assert_eq!(read_baselines(dir.path()).unwrap(), all);
        // cut short: read as empty, the next write would drop every baseline
        let f = dir.path().join("baselines.json");
        std::fs::write(&f, "{\"python etl.py\": \"a.so").unwrap();
        let e = read_baselines(dir.path()).unwrap_err();
        assert_eq!(e.path, f);
        assert!(e.to_string().contains("fix or remove the file"), "{e}");
    }

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
    fn figures_kept_as_requests_arrive_are_those_of_the_records() {
        let mut rs = Vec::new();
        for i in 0..200u64 {
            let mut r = rec(i, &format!("k{}", i % 7));
            // overlapping, touching and apart; some slow, some with no time
            r.latency_ms = match i % 5 {
                0 => 0.0,
                1 => 37.5,
                2 => 10.0,
                _ => 4.0 + i as f64 / 10.0,
            };
            match i % 11 {
                0 => r.status = Some(503),
                1 => r.status = Some(404),
                2 => r.status = None,
                3 => (r.method, r.operation) = ("PUT".into(), "PutObject".into()),
                4 => r.operation = "ListObjectsV2".into(),
                5 => r.method = "DELETE".into(),
                _ => {}
            }
            if i > 150 {
                // a pause, then a late burst
                r.time += chrono::Duration::seconds(30);
            }
            rs.push(r);
        }
        // arrival is by end, not by start
        rs.sort_by_key(|r| r.time.timestamp_micros() + r.latency_us());
        let prices = RequestPrices {
            per_1000_lists: Some(0.004),
            ..Default::default()
        };
        let mut f = RunFigures::new(prices.clone());
        rs.iter().for_each(|r| f.add(r));
        let p = profile(&rs, &prices);
        let want = run_point("", "", &rs, &p, &[]);
        assert_eq!(f.point(), want);
        assert!(want.errors > 0 && want.cost > 0.0 && want.p95_ms > 10.0);
        assert!(want.waiting_ms < want.wall_ms);
        assert_eq!(RunFigures::new(prices.clone()).point().requests, 0);
        assert_eq!(
            RunFigures::new(prices.clone()).point(),
            run_point("", "", &[], &profile(&[], &prices), &[])
        );
    }

    #[test]
    fn a_month_at_the_pace_a_program_runs() {
        let pt = |h: u32, cost: f64| RunPoint {
            path: String::new(),
            created: format!("2026-11-01T{h:02}:00:00Z"),
            requests: 1,
            waiting_ms: 1.0,
            wall_ms: 1.0,
            cost,
            currency: "USD".into(),
            errors: 0,
            warnings: 0,
            findings: 0,
            p95_ms: 0.0,
            refused: None,
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
            waiting_ms: 10.0,
            wall_ms: 10.0,
            cost: 0.0,
            currency: "USD".into(),
            errors: 0,
            warnings: 0,
            findings: 0,
            p95_ms: 0.0,
            refused: None,
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
    fn an_expected_finding_is_never_new_and_p95_has_its_limit() {
        let base: Vec<Record> = (0..10).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        let mut grown = base.clone();
        grown.extend((0..12).map(|i| rec(100 + i, "hot")));
        let (pb, prb, ob) = point(&base);
        let (pg, prg, og) = point(&grown);
        let new = |expect: Vec<String>| {
            check(
                (&pg, &grown, &prg, &og),
                Some((&pb, &base, &prb, &ob)),
                &Budget {
                    no_new_warnings: true,
                    expect,
                    ..Default::default()
                },
            )
        };
        assert!(new(Vec::new()).expected.is_empty());
        // the finding, expected by its identity or its kind; never by words
        // of its text, which change
        let hot = og.iter().find(|o| o.text.contains("hot")).unwrap();
        let id = crate::profile::finding_identity(hot);
        for e in [id.clone(), id.split('|').next().unwrap().to_string()] {
            let c = new(vec![e.clone()]);
            assert!(c.new_warnings.is_empty(), "{e}: {c:#?}");
            assert!(!c.expected.is_empty());
            assert!(c.passed);
        }
        assert!(new(vec!["hot".into()]).expected.is_empty());
        // a key named as the program named it is read as an identity says it
        let job = |key: &str| Observation {
            phrases: vec![crate::phrase::p("obs.hot", &[("key", key.into())])],
            identity: String::new(),
            ..hot.clone()
        };
        assert!(expected(
            &job("b/jobs/1794558464595/state.json"),
            &["obs.hot|b/jobs/1794558465084/state.json".into()]
        ));
        assert!(!expected(
            &job("b/jobs/1794558464595/state.json"),
            &["obs.hot|b/other".into()]
        ));
        // one sonde does not find is no identity
        assert_eq!(expect_error("obs.hot|b/k"), None);
        assert_eq!(expect_error("obs.listed"), None);
        for e in ["obs.retried", "hot", "obs.hto|b/k"] {
            assert!(
                expect_error(e)
                    .unwrap()
                    .contains("not a finding's identity"),
                "{e}"
            );
        }
        // 95% of the requests within a limit
        let c = check(
            (&pg, &grown, &prg, &og),
            None,
            &Budget {
                max_p95_ms: Some(pg.p95_ms - 0.5),
                ..Default::default()
            },
        );
        let v = c.verdicts.iter().find(|v| v.what == "p95").unwrap();
        assert!(!v.ok && !c.passed);
    }

    #[test]
    fn programs_are_their_commands_and_replays_are_none() {
        assert_eq!(
            program_key(Some("  python  etl.py  --day 1 ")).as_deref(),
            Some("python etl.py --day 1")
        );
        assert_eq!(program_key(Some("replay of etl")), None);
        assert_eq!(program_key(Some("imported from CloudTrail")), None);
        assert_eq!(program_key(None), None);
    }

    /// What changes from one run of a command to the next is not another
    /// program: a date, a scratch folder's id, a trial's place.
    #[test]
    fn a_program_is_its_command_with_what_changes_between_runs_left_out() {
        let key = |c: &str| program_key(Some(c)).unwrap();
        assert_eq!(
            key("python etl.py --date 2026-11-14"),
            key("python etl.py --date 2026-11-13")
        );
        assert_eq!(
            key("python etl.py --date 2026-11-14"),
            "python etl.py --date {date}"
        );
        assert_eq!(
            key("./load --run 3fa2b1c9-58cc-4372-a567-0e02b2c3d479"),
            key("./load --run 0e02b2c3-58cc-4372-a567-3fa2b1c9d479")
        );
        // a trial's place, in the command or by its variable: the program
        // it was before one was added
        assert_eq!(
            key("python etl.py 's3://scratch/etl-{trial}/'"),
            key("python etl.py s3://scratch/etl/")
        );
        assert_eq!(
            key("python etl.py --prefix={trial}"),
            key("python etl.py --prefix")
        );
        assert_eq!(key("sh -c 'etl $SONDE_TRIAL'"), key("sh -c etl"));
        assert_eq!(key("python etl.py {trial}"), key("python etl.py"));
        // what does not change stays: two programs stay two
        assert_ne!(
            key("python etl.py --day mon"),
            key("python etl.py --day tue")
        );
        assert_ne!(key("python load.py"), key("python etl.py"));
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
    fn a_small_cost_is_held_to_its_percent_and_a_failed_row_says_by_how_much() {
        let base: Vec<Record> = (0..10).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        let (mut pb, prb, ob) = point(&base);
        // what a short run costs: a ten-thousandth
        pb.cost = 0.0001;
        pb.requests = 100;
        let mut run = pb.clone();
        run.cost = 0.00019;
        run.requests = 123;
        run.errors = 2;
        let c = check(
            (&run, &base, &prb, &ob),
            Some((&pb, &base, &prb, &ob)),
            &Budget {
                max_requests: Some(100),
                tolerance: Some(0.1),
                ..Default::default()
            },
        );
        let row = |what: &str, against: &str| {
            c.verdicts
                .iter()
                .find(|v| v.what == what && v.against == against)
                .unwrap()
        };
        let cost = row("cost", "baseline");
        assert!(!cost.ok, "90% more is past 10%: {cost:?}");
        assert!((cost.limit - 0.00011).abs() < 1e-9, "{cost:?}");
        // over the baseline the row names, not over baseline-and-tolerance
        assert_eq!(cost.over().as_deref(), Some("+90%"));
        assert_eq!(row("requests", "budget").over().as_deref(), Some("+23%"));
        // no limit to be a percent of: the count
        assert_eq!(row("errors", "baseline").over().as_deref(), Some("+2"));
        assert_eq!(row("waiting", "baseline").over(), None, "it passed");
        let md = check_markdown(&c, "USD", None, &[]);
        assert!(
            squeezed(&md).contains("| **FAIL** +23% | requests | 123 | ≤ 100 | budget |"),
            "{md}"
        );
        // the columns line up as text: every row as long as the heading's
        let rows: Vec<&str> = md.lines().filter(|l| l.starts_with('|')).collect();
        assert!(rows.len() > 3, "{md}");
        assert!(
            rows.iter()
                .all(|r| r.chars().count() == rows[0].chars().count()),
            "{md}"
        );
        assert!(
            md.starts_with("### sonde: over budget and past its baseline"),
            "{md}"
        );
    }

    #[test]
    fn a_check_says_whether_the_budget_or_a_trial_failed_it() {
        use crate::trial::{Standing, judge};
        let rs: Vec<Record> = (0..3).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        let (pt, pr, ob) = point(&rs);
        let scenario = crate::fault::scenario("slowdown").unwrap();
        let mut broke = judge(&scenario, &rs, 0, &rs, 1);
        assert_eq!(broke.standing, Standing::Broke);
        let mut fine = broke.clone();
        fine.standing = Standing::Degraded;
        let within = check(
            (&pt, &rs, &pr, &ob),
            None,
            &Budget {
                max_requests: Some(10),
                ..Default::default()
            },
        );
        let none = check((&pt, &rs, &pr, &ob), None, &Budget::default());
        assert_eq!(check_closing(&within, &[]), "within budget");
        assert_eq!(
            check_closing(&within, &[broke.clone()]),
            "within budget, and broke under a trial"
        );
        assert!(!check_passed(&within, &[broke.clone()]));
        // trials alone are a check: no budget is needed to fail one
        assert_eq!(
            check_closing(&none, &[broke.clone()]),
            "broke under a trial"
        );
        assert!(!check_passed(&none, &[broke.clone()]));
        assert_eq!(
            check_closing(&none, &[fine.clone()]),
            "did not break under its trials"
        );
        assert!(check_passed(&none, &[fine.clone()]));
        broke.reasons = vec!["exited 1 | really".into()];
        let md = check_markdown_with(&within, "USD", None, &[], &[broke, fine]);
        assert!(
            md.starts_with("### sonde: within budget, and broke under a trial ❌"),
            "{md}"
        );
        assert!(!md.contains("over budget"), "{md}");
        assert!(
            squeezed(&md).contains("| **FAIL** | slowdown | exited 1 \\| really |"),
            "{md}"
        );
        assert!(
            squeezed(&md).contains("| warn | slowdown |"),
            "degraded is not a pass: {md}"
        );
    }

    #[test]
    fn a_named_program_is_its_name_whatever_its_command() {
        let mut h =
            crate::session::SessionHeader::new("e", "", "", Some("python etl.py /tmp/a".into()));
        assert_eq!(key_of(&h).as_deref(), Some("python etl.py /tmp/a"));
        h.name = Some("nightly-etl".into());
        assert_eq!(key_of(&h).as_deref(), Some("nightly-etl"));
        h.command = Some("/opt/py3.13/bin/python etl.py /tmp/b".into());
        assert_eq!(key_of(&h).as_deref(), Some("nightly-etl"));
        // a replay of it is not a run of it
        h.command = Some("replay of python etl.py".into());
        assert_eq!(key_of(&h), None);
        assert!(!h.is_recording());
        h.command = None;
        assert!(h.is_recording(), "a proxy's session");
        h.scenario = Some("slowdown".into());
        assert!(!h.is_recording(), "a trial");
    }

    #[test]
    fn the_warnings_row_says_it_counts_the_new_ones() {
        let quiet: Vec<Record> = (0..10).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        // a run with a warning its baseline has too
        let mut warned = quiet.clone();
        let mut failed = rec(50, "k0");
        failed.status = Some(503);
        warned.push(failed);
        warned.push(rec(51, "k0"));
        let (p, pr, o) = point(&warned);
        assert!(o.iter().any(|f| f.level == "warn"), "{o:#?}");
        let c = check(
            (&p, &warned, &pr, &o),
            Some((&p, &warned, &pr, &o)),
            &Budget {
                no_new_warnings: true,
                ..Default::default()
            },
        );
        let row = c.verdicts.iter().find(|v| v.what == "warnings").unwrap();
        assert_eq!((row.value, row.label()), (0.0, "new warnings"));
        let md = check_markdown(&c, "USD", None, &o);
        // a row that passed is not repeated in a comment
        assert!(!md.contains("| pass"), "{md}");
        // held to a baseline alone, and within it: not "within budget"
        assert!(md.starts_with("### sonde: within its baseline"), "{md}");
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

    /// A reviewer reads the verdict, what changed against the baseline (by
    /// kind, the new findings first) and one link; never a row that passed.
    #[test]
    fn a_pr_comment_says_only_what_changed_new_findings_first_and_one_link() {
        use crate::profile::{Run, compare_runs};
        let base: Vec<Record> = (0..20).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        // the same reads, each after a HEAD of its key
        let mut after = Vec::new();
        for (i, r) in base.iter().enumerate() {
            let mut head = r.clone();
            head.id = 100 + i as u64;
            head.method = "HEAD".into();
            head.operation = "HeadObject".into();
            head.time = r.time - chrono::Duration::milliseconds(5);
            after.push(head);
            after.push(r.clone());
        }
        let (pb, prb, ob) = point(&base);
        let (pa, pra, oa) = point(&after);
        let c = check(
            (&pa, &after, &pra, &oa),
            Some((&pb, &base, &prb, &ob)),
            &Budget {
                max_requests: Some(1000),
                tolerance: Some(0.1),
                ..Default::default()
            },
        );
        assert!(!c.passed);
        let ch = compare_runs(
            &Run::of(&base, prb.clone(), ob.to_vec()),
            &Run::of(&after, pra.clone(), oa.to_vec()),
        );
        assert!(!ch.appeared.is_empty(), "HEAD before GET is new: {oa:#?}");
        let md = check_comment(
            &c,
            &Comment {
                currency: "USD",
                findings: &oa,
                change: Some(&ch),
                report_url: Some("https://example.com/r(1)"),
                ..Default::default()
            },
        );
        assert!(md.starts_with("### sonde: past its baseline ❌"), "{md}");
        // the figure past the baseline, bold, said once; no table of verdicts
        assert!(
            md.contains("- **requests 20 → 40 (+100%)**, past the 22 it may use"),
            "{md}"
        );
        assert!(!md.contains("| pass") && !md.contains("**FAIL**"), "{md}");
        assert!(squeezed(&md).contains("| HeadObject | 0 → 20 |"), "{md}");
        let new = md.find("**New findings**").expect("new findings");
        assert!(md[new..].contains("HEAD"), "{md}");
        assert!(!md.contains("<summary>Findings</summary>"), "{md}");
        assert_eq!(md.matches("](").count(), 1, "one link: {md}");
        assert!(md.contains("[The report](https://example.com/r%281%29)"));
        // no baseline: said, and nothing compared
        let alone = check((&pa, &after, &pra, &oa), None, &Budget::default());
        let md = check_comment(
            &alone,
            &Comment {
                currency: "USD",
                no_baseline: Some("no baseline yet for program `etl`"),
                ..Default::default()
            },
        );
        assert!(
            md.contains("> ⚠️ no baseline yet for program \\`etl\\`: nothing was compared"),
            "{md}"
        );
        // a note of the baseline's that is a warning now is not new: the
        // finding is the same one, as `compare` says
        let same = compare_runs(
            &Run::of(&after, pra.clone(), oa.to_vec()),
            &Run::of(&after, pra.clone(), oa.to_vec()),
        );
        let grown = Check {
            new_warnings: vec!["20 objects were checked with HEAD".into()],
            ..c.clone()
        };
        let md = check_comment(
            &grown,
            &Comment {
                currency: "USD",
                findings: &oa,
                change: Some(&same),
                ..Default::default()
            },
        );
        assert!(!md.contains("**New findings**"), "{md}");
        assert!(md.contains("**Warnings now**\n\n- 20 objects"), "{md}");
        // a program that failed is said first, with why
        let mut refused = rec(1, "k");
        refused.method = "DELETE".into();
        refused.status = Some(403);
        refused.error_body = Some("sonde refused DeleteObject: no-delete mode".into());
        refused
            .response_headers
            .push(reqlog::Header::new("x-sonde-policy", "no-delete"));
        let (pf, prf, of) = point(std::slice::from_ref(&refused));
        assert_eq!(
            pf.refused.as_deref(),
            Some("sonde refused DeleteObject: no-delete mode")
        );
        let c = check((&pf, &[refused], &prf, &of), None, &Budget::default());
        let md = check_comment(
            &c,
            &Comment {
                currency: "USD",
                failed: Some((1, pf.refused.as_deref())),
                ..Default::default()
            },
        );
        assert!(md.lines().next().unwrap().ends_with('❌'), "{md}");
        assert!(
            md.contains(
                "> ❌ **The program failed** (exit 1): sonde refused DeleteObject: no-delete mode"
            ),
            "{md}"
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
