//! A program over its runs: the runs of one command, each reduced to the
//! figures that move when the program changes (requests, waiting, cost,
//! errors, findings), and a run checked against a budget or a baseline run:
//! what `sonde check` gates CI on, and what the app's history shows.

use serde::{Deserialize, Serialize};

use crate::episode::Observation;
use crate::profile::Profile;
use reqlog::Record;

/// The program a run ran, as runs of it are grouped: what its command
/// runs ([`short_name`]: `etl` for `python3 etl.py 5`), whatever its
/// arguments; a log source's imports are its own (`logs of s3://b/logs/`,
/// [`LOGS_OF`]). A replay, a file imported once or a recording without a
/// command has none.
pub fn program_key(command: Option<&str>) -> Option<String> {
    let c = command?.trim();
    if c.is_empty() || c.starts_with("replay of ") || c.starts_with("imported from ") {
        return None;
    }
    if c.starts_with(LOGS_OF) {
        return Some(c.to_string());
    }
    short_name(c)
}

/// The command of a log source's imports, before its place: their runs make
/// a history and `sonde check` holds them to a baseline like any program's.
pub const LOGS_OF: &str = "logs of ";

/// What changed between two runs' commands: the words that differ, after
/// the option they follow, `--limit 200 → 20`. Runs of one program may
/// differ so; less of a finding then is a smaller workload, not a fix. A
/// trial's place (`{trial}`, `$SONDE_TRIAL`) is no change. None when the
/// words are the same, or either is not known. Secrets are hidden.
pub fn command_change(a: Option<&str>, b: Option<&str>) -> Option<String> {
    let (a, b) = (a?, b?);
    let words = |c: &str| -> Vec<String> {
        let c = shell_inner(c);
        shell_words(&c)
            .iter()
            .map(|w| without_trial(&unquote(&scrub_word(w))))
            .flat_map(|w| w.split_whitespace().map(String::from).collect::<Vec<_>>())
            .collect()
    };
    let (wa, wb) = (words(a), words(b));
    if wa == wb {
        return None;
    }
    let same = wa.iter().zip(&wb).take_while(|(x, y)| x == y).count();
    let tail = wa[same..]
        .iter()
        .rev()
        .zip(wb[same..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let part = |w: &[String]| -> String {
        let t = w.join(" ");
        if t.is_empty() {
            "—".into()
        } else if t.chars().count() > 40 {
            format!("{}…", t.chars().take(39).collect::<String>())
        } else {
            t
        }
    };
    let option = same
        .checked_sub(1)
        .map(|i| &wa[i])
        .filter(|w| w.starts_with('-'))
        .map_or(String::new(), |w| format!("{w} "));
    Some(format!(
        "{option}{} → {}",
        part(&wa[same..wa.len() - tail]),
        part(&wb[same..wb.len() - tail])
    ))
}

/// The command a shell is given to run, `X` of `sh -c X` and `bash -c X`
/// (a single argument to `-c`, nothing after it): the same command as `X`
/// typed alone (the Action runs its `run` as `bash -c`). Any other
/// command as it is.
fn shell_inner(command: &str) -> std::borrow::Cow<'_, str> {
    let words = shell_words(command);
    match &words[..] {
        [shell, c, inner]
            if c == "-c"
                && matches!(
                    unquote(shell).rsplit(['/', '\\']).next(),
                    Some("sh" | "bash")
                ) =>
        {
            unquote(inner).into()
        }
        _ => command.into(),
    }
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

/// The program a run belongs to: the name it was given, else what its
/// command runs ([`program_key`]).
pub fn key_of(header: &crate::run::RunHeader) -> Option<String> {
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
    "printf", "wait", "nohup", "exit",
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

/// What a command runs, by a short name: the script an interpreter runs
/// (`etl` of `python etl.py --day 2`, `pytest` of `python -m pytest`), the
/// command a shell is given (`aws` of `sh -c 'aws s3 cp …'`), the image a
/// container runs (`aws-cli` of `docker run amazon/aws-cli s3 ls`), else the
/// program itself (`load` of `./load --all`). Letters, digits, `-` and `_`:
/// a program without a name of its own goes by it ([`program_key`]), and its
/// run files are named by it.
pub fn short_name(command: &str) -> Option<String> {
    const INTERPRETERS: &[&str] = &[
        "python", "python3", "node", "ruby", "java", "deno", "bun", "perl", "php", "uv", "poetry",
        "npx", "dotnet",
    ];
    const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash"];
    let program = program_name(command)?;
    let stem = |f: &str| {
        let name = f.rsplit(['/', '\\']).next().unwrap_or(f);
        name.split('.').next().unwrap_or(name).to_string()
    };
    let base = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    // the words after the program's own, to the end of its part
    let words: Vec<String> = shell_words(command).iter().map(|w| unquote(w)).collect();
    let after: Vec<&str> = words
        .iter()
        .skip_while(|w| w.rsplit(['/', '\\']).next() != Some(program.as_str()))
        .skip(1)
        .take_while(|w| !is_separator(w))
        .map(String::as_str)
        .collect();
    // (a shell that only sets the stage, `sh -c 'exit 3'`, is the shell)
    if SHELLS.contains(&base)
        && let Some(at) = after.iter().position(|w| *w == "-c")
        && let Some(inner) = after.get(at + 1)
        && program_name(inner).is_some_and(|p| !STAGE.contains(&p.as_str()))
    {
        return short_name(inner);
    }
    if matches!(base, "docker" | "podman")
        && let Some(image) = sonde_proxy::container::image(&after)
    {
        // `registry/amazon/aws-cli:2.40` is `aws-cli`
        let last = image.rsplit('/').next().unwrap_or(image);
        return safe_name(last.split([':', '@']).next().unwrap_or(last));
    }
    let mut name = stem(&program);
    if INTERPRETERS.contains(&base)
        || INTERPRETERS.contains(&program.as_str())
        || SHELLS.contains(&base)
    {
        // what it runs: a module (-m), the first word after it that is a
        // file, not an option; code given inline (-c, -e) has no name of its own
        let mut it = after.iter();
        while let Some(w) = it.next() {
            match *w {
                "-m" => {
                    if let Some(m) = it.next() {
                        name = m.to_string();
                    }
                    break;
                }
                "-c" | "-e" | "--eval" => break,
                w if !w.starts_with('-') && w.contains('.') && !w.contains('=') => {
                    name = stem(w);
                    break;
                }
                _ => {}
            }
        }
    }
    safe_name(&name)
}

/// A name with letters, digits, `-` and `_` only (each other character a
/// `-`, none at either end): none when nothing is left.
fn safe_name(name: &str) -> Option<String> {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-').to_string();
    (!name.is_empty()).then_some(name)
}

/// What a run's file is named by, after its time: the program's
/// name (`--name`, `[program] name`), else its command's short name
/// ([`short_name`]: `etl` of `python3 etl.py`, not `python3`).
pub fn file_stem(name: Option<&str>, command: Option<&str>) -> Option<String> {
    name.and_then(safe_name)
        .or_else(|| command.and_then(short_name))
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

/// Each program's baseline run, by its file name, as the folder's index
/// keeps it ([`crate::index`]).
pub fn baselines(dir: &std::path::Path) -> std::collections::BTreeMap<String, String> {
    crate::index::of(dir).folder.baselines.clone()
}

/// Makes `run` (a file name in `dir`) the baseline of `program`; none: no baseline.
pub fn set_baseline(
    dir: &std::path::Path,
    program: &str,
    run: Option<&str>,
) -> std::io::Result<()> {
    crate::index::update(dir, |_| {
        (
            vec![(
                program.to_string(),
                crate::index::Change::Baseline(run.map(String::from)),
            )],
            (),
        )
    })
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
    /// Bytes read out of the store: what `max-transfer` holds a run to.
    #[serde(default)]
    pub bytes_out: u64,
    /// Requests that failed after their retries (a miss is no failure, and
    /// an attempt retried until it succeeded is none): what `max-errors`
    /// holds a run to ([`crate::signals::gave_up`]).
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
        bytes_out: profile.cost.transfer.bytes,
        errors: crate::signals::gave_up(records)
            .into_iter()
            .filter(|&g| g)
            .count(),
        warnings: findings.iter().filter(|o| o.level == "warn").count(),
        findings: findings.len(),
        refused: records
            .iter()
            .find(|r| sonde_proxy::refused_by_policy(r))
            .and_then(|r| r.rare().error_body.clone()),
    }
}

/// The figures a budget holds a run to, kept as its requests arrive: a
/// proxy left running holds eight bytes a request (its latency, for the
/// p95) and the stretches of time with a request in flight, not the records.
/// The same figures as [`run_point`] of the same records, but for a
/// failure's retry, which is known here as it arrives: the next request of
/// the failure's name once it was answered, within a minute (without the
/// rule that tells a program's poll from an SDK's retries).
#[derive(Debug, Clone)]
pub struct RunFigures {
    prices: crate::profile::RequestPrices,
    requests: usize,
    errors: usize,
    /// Failures a retry is owed for, by the request's name: when each was
    /// answered (µs). Counted in `errors` until their retry comes.
    owed: std::collections::HashMap<String, i64>,
    cost: f64,
    bytes_out: u64,
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
            owed: Default::default(),
            cost: 0.0,
            bytes_out: 0,
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
        let mut start = r.time.timestamp_micros();
        let mut end = start + r.latency_us();
        // a failure retried is a request that went on: no longer one that failed
        let name = crate::signals::request_name(r);
        if let Some(&told) = self.owed.get(&name)
            && start >= told
        {
            self.owed.remove(&name);
            if start - told <= crate::signals::RETRY_WINDOW_US {
                self.errors -= 1;
            }
        }
        if crate::summary::is_failure(r) {
            self.errors += 1;
            if crate::episode::retryable(r) {
                self.owed.insert(name, end);
            }
        }
        self.cost += crate::profile::price(r, &self.prices);
        if crate::profile::reached_store(r) {
            self.bytes_out += crate::profile::out_bytes(r);
        }
        self.latencies.push(r.latency_ms.max(0.0));
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
            bytes_out: self.bytes_out,
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
    pub max_errors: Option<usize>,
    /// Bytes read out of the store, at most.
    pub max_transfer: Option<u64>,
    /// Against the baseline: requests, waiting and cost may grow this much.
    /// A baseline also holds the run to no warning it did not have.
    pub tolerance: Option<f64>,
    /// The run's times are not meaningful (the built-in demo store's): no
    /// row holds its waiting, to a budget or to a baseline.
    #[serde(skip)]
    pub untimed: bool,
}

/// One line of a check.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    /// `requests`, `waiting`, `cost`, `transfer` (bytes out), `errors`,
    /// `p95` or `warnings`.
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
    /// The run recorded no request: nothing reached sonde (the program's
    /// client went elsewhere). It fails, whatever it is held to.
    pub empty: bool,
    pub verdicts: Vec<Verdict>,
    /// Warnings the baseline did not have, as they read.
    pub new_warnings: Vec<String>,
    /// Findings of the run the project keeps on purpose (`expect`), each
    /// with why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected: Vec<Expected>,
}

/// A finding the project keeps on purpose, as `check` says it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Expected {
    pub identity: String,
    pub text: String,
    /// Why it is kept (`sonde.toml`'s `[check] expect`).
    pub expected: String,
}

/// Why the project keeps finding `o` (`expect`), when it does: an entry of
/// `expect` is its identity, or its kind for every finding of that kind
/// (its identity first). A key in an entry is read as an identity says it
/// (`jobs/{n}/`) by the rules of now ([`crate::profile::IDENTITY_RULES`]),
/// whatever it was named in.
pub fn expected<'a>(
    o: &Observation,
    expect: &'a std::collections::BTreeMap<String, String>,
) -> Option<&'a str> {
    if expect.is_empty() {
        return None;
    }
    let id = crate::profile::finding_identity(o);
    let kind = id.split('|').next().unwrap_or("");
    // an entry made by older rules is read by these; one that names what
    // two findings of a run share names both
    let named = expect.iter().find(|(e, _)| {
        let e = e.trim();
        e == id || expected_identity(e) == id || e == expected_identity(&id)
    });
    named
        .or_else(|| expect.iter().find(|(e, _)| e.trim() == kind))
        .map(|(_, why)| why.trim())
}

/// Marks each finding the project keeps on purpose with why
/// ([`Observation::expected`]): what `show`, `run` and `check` say of it.
pub fn mark_expected(
    findings: &mut [Observation],
    expect: &std::collections::BTreeMap<String, String>,
) {
    for o in findings {
        o.expected = expected(o, expect).map(String::from);
    }
}

/// An entry of `expect` as an identity: what it is about made
/// [`crate::profile::stable`].
fn expected_identity(e: &str) -> String {
    crate::profile::by_rules(e)
}

/// What is wrong with an entry of `[check] expect`, when something is: its
/// kind is none sonde finds, or it gives no reason.
pub fn expect_error(e: &str, why: &str) -> Option<String> {
    let kind = e.trim().split('|').next().unwrap_or("");
    if crate::profile::FINDING_KINDS.contains(&kind) {
        return why.trim().is_empty().then(|| {
            format!("[check] expect: {e:?} needs its reason, what the team keeps it for (\"{e}\" = \"rewritten by the scheduler\")")
        });
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
    pub first_past_commit: Option<crate::run::GitStamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_within: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_within_commit: Option<crate::run::GitStamp>,
}

/// [`Since`] of `runs` (oldest first, the newest last), against `base`.
/// None when the newest run is within it.
pub fn since(
    runs: &[(RunPoint, Option<crate::run::GitStamp>)],
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
    if let Some(ms) = budget.max_waiting_ms.filter(|_| !budget.untimed) {
        limit("waiting", r.waiting_ms, ms, "budget");
    }
    if let Some(n) = budget.max_errors {
        limit("errors", r.errors as f64, n as f64, "budget");
    }
    if let Some(n) = budget.max_transfer {
        limit("transfer", r.bytes_out as f64, n as f64, "budget");
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
            if !budget.untimed {
                held("waiting", r.waiting_ms, l.waiting_ms, base.waiting_ms);
            }
            held("cost", r.cost, l.cost, base.cost);
            held("errors", r.errors as f64, l.errors, base.errors as f64);
        }
        {
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
                        && o.expected.is_none()
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
    let empty = r.requests == 0;
    Check {
        passed: !empty && v.iter().all(|x| x.ok),
        empty,
        verdicts: v,
        new_warnings,
        expected: run
            .3
            .iter()
            .filter_map(|o| {
                Some(Expected {
                    identity: crate::profile::finding_identity(o),
                    text: o.text.clone(),
                    expected: o.expected.clone()?,
                })
            })
            .collect(),
    }
}

/// A length of time ([`sonde_fmt::duration`]).
pub fn ms_text(ms: f64) -> String {
    sonde_fmt::duration(ms)
}

/// A cent: a request cost under it is no amount anyone acts on, so a change
/// of one is said by its share alone ([`share_moved`]).
pub const CENT: f64 = 0.01;

/// How much a figure moved, as a share: `-9%`, `under 1%`, `no change`.
pub fn share_moved(a: f64, b: f64) -> String {
    if a == b {
        return "no change".into();
    }
    if a <= 0.0 {
        return "new".into();
    }
    let pct = (b - a) * 100.0 / a;
    if pct.abs() < 0.5 {
        "under 1%".into()
    } else {
        format!("{pct:+.0}%")
    }
}

/// Money to two significant digits under a cent: a run's requests often
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

/// A sentence of sonde's own with code spans in it (``program `etl.py` ``), as
/// Markdown: each span kept a span, its text as written (a backslash in a
/// span is shown, not read as an escape), the rest as [`md_text`]. A
/// sentence whose backticks do not pair is all text.
pub fn md_said(t: &str) -> String {
    let parts: Vec<&str> = t.split('`').collect();
    if parts.len().is_multiple_of(2) {
        return md_text(t);
    }
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i % 2 == 0 {
                md_text(p)
            } else {
                format!("`{}`", p.replace(['\n', '\r'], " "))
            }
        })
        .collect()
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
    if c.empty {
        return NOTHING_REACHED.into();
    }
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
        (Some(false), Some(false)) => Some("over budget and over its baseline"),
        (Some(false), _) => Some("over budget"),
        (_, Some(false)) => Some("over its baseline"),
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

/// The verdict on a run that recorded no request.
pub const NOTHING_REACHED: &str = "nothing reached sonde";

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

/// Whether a check, with the run's trials, passed: a run that reached
/// sonde, every row of the budget and the baseline, and no trial that broke.
pub fn check_passed(c: &Check, trials: &[crate::trial::Trial]) -> bool {
    c.passed
        && !trials
            .iter()
            .any(|t| t.standing == crate::trial::Standing::Broke)
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
    /// Where the whole report is (`sonde share`'s page, kept by CI).
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
        "transfer" => crate::episode::human_bytes(v as u64),
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
            Some(w) => md.push_str(&format!(": {}\n", md_said(w))),
            None => md.push('\n'),
        }
    }
    if let Some(t) = m.since {
        md.push_str(&format!("\n{}\n", md_said(t)));
    }
    if let Some(why) = m.no_baseline {
        md.push_str(&format!(
            "\n> ⚠️ {}: nothing was compared\n",
            // what is missing, not how to set one on this machine
            md_said(why.split(": ").next().unwrap_or(why))
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
                format!("**over** {}", v.over().unwrap_or_default()),
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
    let kept = |text: &str| c.expected.iter().any(|e| e.text == text);
    // where a finding's requests came from, and its engine's setting to change
    let place = |text: &str| {
        let f = m.findings.iter().find(|f| f.text == text);
        let mut out = f
            .and_then(|f| f.callers.first())
            .map(|c| {
                format!(
                    " — `{}`",
                    c.place.replace('`', "'").replace(['\n', '\r'], " ")
                )
            })
            .unwrap_or_default();
        if let Some(s) = f
            .and_then(|f| crate::fixes::for_sdk(&f.fixes, f.sdk.as_deref()))
            .and_then(|x| x.setting.as_ref())
        {
            out.push_str(&format!(
                " — set `{}`",
                s.line().replace('`', "'").replace(['\n', '\r'], " ")
            ));
        }
        out
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
                        format!("{} (not found)", g.kind)
                    } else {
                        g.kind.clone()
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
        // new first: what this change brought, as `compare` says it (one
        // kept on purpose is said under Expected, with why)
        let new: Vec<&str> = ch
            .appeared
            .iter()
            .map(|f| f.text.as_str())
            .filter(|t| !kept(t))
            .collect();
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
                match t.standing {
                    Standing::Broke => "**broke**".to_string(),
                    s => s.label().to_string(),
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
    // kept on purpose: each with why, for a reviewer and an agent to leave
    if !c.expected.is_empty() {
        md.push_str("\n**Expected** (sonde.toml)\n\n");
        for e in &c.expected {
            md.push_str(&format!(
                "- {}{} — expected: {}\n",
                md_text(&e.text),
                place(&e.text),
                md_said(&e.expected)
            ));
        }
    }
    // without a baseline, what the run has: its first findings
    let findings: Vec<_> = m
        .findings
        .iter()
        .filter(|f| !kept(&f.text))
        .take(5)
        .collect();
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
    fn a_programs_baseline_is_kept_in_the_folders_index() {
        let dir = tempfile::tempdir().unwrap();
        assert!(baselines(dir.path()).is_empty(), "none set");
        set_baseline(dir.path(), "etl", Some("a.sonde")).unwrap();
        assert_eq!(baselines(dir.path())["etl"], "a.sonde");
        set_baseline(dir.path(), "etl", None).unwrap();
        assert!(baselines(dir.path()).is_empty());
    }

    fn rec(id: u64, key: &str) -> Record {
        Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 10).unwrap(),
            method: "GET".into(),
            kind: "GetObject".into(),
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

    /// `max-errors` counts the requests that failed after their retries:
    /// one 503 retried into a 200 against real S3 fails no budget.
    #[test]
    fn an_error_is_a_request_that_failed_after_its_retries() {
        let at = |id: u64, ms: i64, key: &str, status: u16| {
            let mut r = rec(id, key);
            r.time += chrono::Duration::milliseconds(ms);
            r.status = Some(status);
            r
        };
        let errors = |rs: &[Record]| {
            let mut f = RunFigures::new(RequestPrices::default());
            rs.iter().for_each(|r| f.add(r));
            let p = point(rs).0;
            assert_eq!(f.point().errors, p.errors, "kept as they arrive, the same");
            p.errors
        };
        let mut rs: Vec<Record> = (0..20).map(|i| at(i, i as i64 * 100, "k", 200)).collect();
        assert_eq!(errors(&rs), 0);
        // a 503, sent again 50 ms later and answered
        rs.push(at(30, 2500, "hot", 503));
        rs.push(at(31, 2560, "hot", 200));
        assert_eq!(errors(&rs), 0);
        let c = check(
            (&point(&rs).0, &rs, &point(&rs).1, &point(&rs).2),
            None,
            &Budget {
                max_errors: Some(0),
                ..Default::default()
            },
        );
        assert!(c.passed, "{c:#?}");
        // retried twice and failed again: one request lost, not three
        rs.push(at(40, 4000, "lost", 503));
        rs.push(at(41, 4100, "lost", 503));
        rs.push(at(42, 4300, "lost", 503));
        assert_eq!(errors(&rs), 1);
        // a 403 is not sent again: it failed
        rs.push(at(50, 5000, "secret", 403));
        assert_eq!(errors(&rs), 2);
        // a miss is no failure
        rs.push(at(60, 6000, "none", 404));
        assert_eq!(errors(&rs), 2);
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
                3 => (r.method, r.kind) = ("PUT".into(), "PutObject".into()),
                4 => r.kind = "ListObjectsV2".into(),
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
            bytes_out: 0,
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
            bytes_out: 0,
            errors: 0,
            warnings: 0,
            findings: 0,
            p95_ms: 0.0,
            refused: None,
        };
        let git = |c: &str| {
            Some(crate::run::GitStamp {
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
    fn an_expected_finding_is_never_new_and_says_why() {
        use std::collections::BTreeMap;
        let base: Vec<Record> = (0..10).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        let mut grown = base.clone();
        grown.extend((0..12).map(|i| rec(100 + i, "hot")));
        let (pb, prb, ob) = point(&base);
        let (pg, prg, og) = point(&grown);
        let new = |expect: &[(&str, &str)]| {
            let expect: BTreeMap<String, String> = expect
                .iter()
                .map(|(e, why)| (e.to_string(), why.to_string()))
                .collect();
            let mut og = og.clone();
            mark_expected(&mut og, &expect);
            check(
                (&pg, &grown, &prg, &og),
                Some((&pb, &base, &prb, &ob)),
                &Budget::default(),
            )
        };
        assert!(new(&[]).expected.is_empty());
        // the finding, expected by its identity or its kind; never by words
        // of its text, which change
        let hot = og.iter().find(|o| o.text.contains("hot")).unwrap();
        let id = crate::profile::finding_identity(hot);
        for e in [id.clone(), id.split('|').next().unwrap().to_string()] {
            let c = new(&[(&e, "rewritten by the scheduler")]);
            assert!(c.new_warnings.is_empty(), "{e}: {c:#?}");
            assert_eq!(c.expected.len(), 1);
            assert_eq!(c.expected[0].expected, "rewritten by the scheduler");
            assert_eq!(c.expected[0].identity, id);
            assert!(c.passed);
            // said with why in the comment, and not again as new
            let md = check_comment(&c, &Comment::default());
            assert!(
                md.contains("**Expected** (sonde.toml)")
                    && md.contains("expected: rewritten by the scheduler"),
                "{md}"
            );
        }
        assert!(new(&[("hot", "x")]).expected.is_empty());
        // its identity is said before its kind
        let both = BTreeMap::from([
            ("obs.hot".to_string(), "all of them".to_string()),
            (id.clone(), "this one".to_string()),
        ]);
        assert_eq!(expected(hot, &both), Some("this one"));
        // a key named as the program named it is read as an identity says it
        let job = |key: &str| Observation {
            phrases: vec![crate::phrase::p("obs.hot", &[("key", key.into())])],
            identity: String::new(),
            kind: String::new(),
            saves_cost: 0.0,
            ..hot.clone()
        };
        let one = |e: &str| BTreeMap::from([(e.to_string(), "kept".to_string())]);
        assert_eq!(
            expected(
                &job("b/jobs/1794558464595/state.json"),
                &one("obs.hot|b/jobs/1794558465084/state.json")
            ),
            Some("kept")
        );
        assert_eq!(
            expected(
                &job("b/jobs/1794558464595/state.json"),
                &one("obs.hot|b/other")
            ),
            None
        );
        // one sonde does not find is no identity; each gives its reason
        assert_eq!(expect_error("obs.hot|b/k", "kept"), None);
        assert_eq!(expect_error("obs.listed", "kept"), None);
        assert!(
            expect_error("obs.listed", "")
                .unwrap()
                .contains("needs its reason")
        );
        for e in ["obs.retried", "hot", "obs.hto|b/k"] {
            assert!(
                expect_error(e, "kept")
                    .unwrap()
                    .contains("not a finding's identity"),
                "{e}"
            );
        }
    }

    #[test]
    fn a_demo_run_is_never_held_to_its_waiting() {
        let base: Vec<Record> = (0..10).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        let (pb, prb, ob) = point(&base);
        let (mut pg, prg, og) = point(&base);
        pg.waiting_ms = pb.waiting_ms * 3.0 + 500.0;
        let held = |untimed: bool| {
            check(
                (&pg, &base, &prg, &og),
                Some((&pb, &base, &prb, &ob)),
                &Budget {
                    max_waiting_ms: Some(1.0),
                    tolerance: Some(0.1),
                    untimed,
                    ..Default::default()
                },
            )
        };
        let timed = held(false);
        assert!(!timed.passed);
        assert_eq!(
            timed
                .verdicts
                .iter()
                .filter(|v| v.what == "waiting")
                .count(),
            2
        );
        let demo = held(true);
        assert!(demo.passed, "{demo:#?}");
        assert!(demo.verdicts.iter().all(|v| v.what != "waiting"));
    }

    #[test]
    fn a_run_file_is_named_by_its_program_not_its_interpreter() {
        // 20.0 named every Python run `…-python3.sonde`
        assert_eq!(
            file_stem(None, Some("python3 etl.py --day 1")).as_deref(),
            Some("etl")
        );
        assert_eq!(
            file_stem(Some("nightly etl"), Some("python3 etl.py")).as_deref(),
            Some("nightly-etl")
        );
        assert_eq!(
            file_stem(None, Some("./load --all")).as_deref(),
            Some("load")
        );
        assert_eq!(file_stem(None, None), None);
    }

    /// A program is what its command runs, whatever its arguments: a run
    /// with another date, number or id is another run of it.
    #[test]
    fn a_program_is_what_its_command_runs() {
        let key = |c: &str| program_key(Some(c));
        for (command, program) in [
            ("python3 etl.py 5", "etl"),
            ("  python3  etl.py  --day 1 ", "etl"),
            ("/opt/py3.13/bin/python etl.py --date 2026-11-14", "etl"),
            ("AWS_PROFILE=x python -u jobs/load.py --limit 30", "load"),
            ("node dist/sync.mjs", "sync"),
            ("./load --all", "load"),
            ("sh get.sh", "get"),
            ("aws s3 ls", "aws"),
            ("logs of s3://b/logs/", "logs of s3://b/logs/"),
            // 21.0 named these `big`, `example` and `docker`
            ("sh -c 'aws s3 cp big.bin s3://b/'", "aws"),
            ("bash -c \"curl https://example.invalid\"", "curl"),
            ("sh -c 'exit 3'", "sh"),
            ("docker run --rm -e A amazon/aws-cli:2.40 s3 ls", "aws-cli"),
            ("podman run -v a:b rclone/rclone ls", "rclone"),
            ("python3 -m pytest tests/", "pytest"),
            ("python3 -c 'import boto3; s3.get_object()'", "python3"),
        ] {
            assert_eq!(key(command).as_deref(), Some(program), "{command}");
        }
        assert_eq!(key("python etl.py 20"), key("python etl.py 5"));
        assert_ne!(key("python load.py"), key("python etl.py"));
        assert_eq!(key("replay of etl"), None);
        assert_eq!(key("imported from CloudTrail"), None);
        assert_eq!(program_key(None), None);
    }

    #[test]
    fn a_changed_command_says_the_words_that_changed() {
        let change = |a: &str, b: &str| command_change(Some(a), Some(b));
        assert_eq!(
            change("python etl.py --limit 200", "python etl.py --limit 20").as_deref(),
            Some("--limit 200 → 20")
        );
        assert_eq!(
            change("python etl.py --limit 200 --v", "python etl.py --v").as_deref(),
            Some("--limit 200 → —")
        );
        assert_eq!(
            change("python a.py s3://b/x", "python b.py s3://b/x").as_deref(),
            Some("a.py → b.py")
        );
        // an argument of one program is a change, a date as any other
        assert_eq!(
            change("etl --date 2026-11-13", "etl --date 2026-11-14").as_deref(),
            Some("--date 2026-11-13 → 2026-11-14")
        );
        assert_eq!(change("etl", "etl"), None);
        // a trial's place is none
        assert_eq!(change("etl s3://b/x-{trial}/", "etl s3://b/x/"), None);
        // a secret never shows
        assert_eq!(
            change("TOKEN=abc etl", "TOKEN=xyz etl --all").as_deref(),
            Some("— → --all")
        );
        // a shell given the command (the Action's `bash -c`) is the command
        // typed alone: 24.0.1's Action against a 24.0.0 baseline
        assert_eq!(
            change("bash -c 'python3 etl.py --day 2'", "python3 etl.py --day 3"),
            change("python3 etl.py --day 2", "python3 etl.py --day 3")
        );
        assert_eq!(change("bash -c 'python3 etl.py'", "python3 etl.py"), None);
        assert_eq!(
            change("python3 etl.py", "/bin/sh -c \"python3 etl.py\""),
            None
        );
        // more than the one argument is another command
        assert!(change("bash -c 'python3 etl.py' x", "python3 etl.py").is_some());
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
            squeezed(&md).contains("| **over** +23% | requests | 123 | ≤ 100 | budget |"),
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
            md.starts_with("### sonde: over budget and over its baseline"),
            "{md}"
        );
    }

    #[test]
    fn a_check_says_whether_the_budget_or_a_trial_failed_it() {
        use crate::trial::{Standing, judge};
        let rs: Vec<Record> = (0..3).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        let (pt, pr, ob) = point(&rs);
        let scenario = sonde_proxy::fault::scenario("slowdown").unwrap();
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
            squeezed(&md).contains("| **broke** | slowdown | exited 1 \\| really |"),
            "{md}"
        );
        assert!(
            squeezed(&md).contains("| degraded | slowdown |"),
            "degraded is not a pass: {md}"
        );
    }

    #[test]
    fn a_named_program_is_its_name_whatever_its_command() {
        let mut h = crate::run::RunHeader::new("e", "", "", Some("python etl.py /tmp/a".into()));
        assert_eq!(key_of(&h).as_deref(), Some("etl"));
        h.name = Some("nightly-etl".into());
        assert_eq!(key_of(&h).as_deref(), Some("nightly-etl"));
        h.command = Some("/opt/py3.13/bin/python etl.py /tmp/b".into());
        assert_eq!(key_of(&h).as_deref(), Some("nightly-etl"));
        // a replay of it is not a run of it
        h.command = Some("replay of python etl.py".into());
        assert_eq!(key_of(&h), None);
        assert!(!h.is_recording());
        h.command = None;
        assert!(h.is_recording(), "a proxy's run");
        h.scenario = Some("slowdown".into());
        assert!(!h.is_recording(), "a trial");
    }

    #[test]
    fn the_warnings_row_says_it_counts_the_new_ones() {
        let quiet: Vec<Record> = (0..10).map(|i| rec(i + 1, &format!("k{i}"))).collect();
        // a run with a warning its baseline has too: a request it gave up
        // on after a 503
        let mut warned = quiet.clone();
        let mut failed = rec(50, "k0");
        failed.status = Some(503);
        warned.push(failed);
        let (p, pr, o) = point(&warned);
        assert!(o.iter().any(|f| f.level == "warn"), "{o:#?}");
        let c = check(
            (&p, &warned, &pr, &o),
            Some((&p, &warned, &pr, &o)),
            &Budget {
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
            head.kind = "HeadObject".into();
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
        assert!(md.starts_with("### sonde: over its baseline ❌"), "{md}");
        // the figure past the baseline, bold, said once; no table of verdicts
        assert!(
            md.contains("- **requests 20 → 40 (+100%)**, past the 22 it may use"),
            "{md}"
        );
        assert!(
            !md.contains("| survived") && !md.contains("**over**"),
            "{md}"
        );
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
            md.contains("> ⚠️ no baseline yet for program `etl`: nothing was compared"),
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
        refused.rare_mut().error_body = Some("sonde refused DeleteObject: no-delete mode".into());
        refused
            .lists_mut()
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

    /// A finding an engine's setting acts on says the setting in the
    /// comment, where a reviewer reads it.
    #[test]
    fn a_pr_comment_names_the_engines_setting() {
        let mut after = Vec::new();
        for i in 0..6u64 {
            let mut get = rec(i * 2 + 2, &format!("in/k{i}"));
            get.time += chrono::Duration::milliseconds(i as i64 * 100);
            let mut head = get.clone();
            head.id = i * 2 + 1;
            head.method = "HEAD".into();
            head.kind = "HeadObject".into();
            head.time -= chrono::Duration::milliseconds(5);
            for r in [&mut head, &mut get] {
                r.lists_mut()
                    .request_headers
                    .push(reqlog::Header::new("user-agent", "rclone/v1.68.2"));
            }
            after.extend([head, get]);
        }
        let (pa, pra, oa) = point(&after);
        assert!(oa.iter().any(|o| o.kind == "obs.headget"), "{oa:#?}");
        let c = check((&pa, &after, &pra, &oa), None, &Budget::default());
        let md = check_comment(
            &c,
            &Comment {
                currency: "USD",
                findings: &oa,
                ..Default::default()
            },
        );
        assert!(
            md.contains("— set `--s3-no-head-object = true (rclone)`"),
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
