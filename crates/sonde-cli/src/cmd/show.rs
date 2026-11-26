//! `sonde show <run>`: a run's operations, requests and findings.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Args;
use reqlog::Record;
use sonde_analysis::episode::{Episode, Outcome};
use sonde_analysis::split::Dimension;
use sonde_analysis::view::Analysis;
use sonde_core::Sonde;

use crate::output::{self, Format};

use super::record::*;

#[derive(Args)]
pub struct ShowArgs {
    /// A run's file or name (default: the newest run of this folder's
    /// program, or recorded in this folder, else the newest of any).
    pub(super) file: Option<PathBuf>,
    /// Only the operations that failed (with --requests, the requests).
    #[arg(long)]
    pub(super) errors: bool,
    /// Each request on its line, then the summary (with `-f json`, the
    /// requests whole).
    #[arg(long)]
    pub(super) requests: bool,
    /// Everything: each operation on its line, then the summary with every
    /// kind of request, the signals over the run, first byte and cost. With
    /// `-f json`: every operation, and each finding's operations and fixes
    /// whole.
    #[arg(long)]
    pub(super) all: bool,
    /// Open it in the sonde app instead (a sonde:// link).
    #[arg(long)]
    pub(super) app: bool,
    /// What changed since another run (a file or a name), in place of the
    /// program's run before: the figures, each kind of request that moved,
    /// and each finding fixed, less, more, back or new.
    #[arg(long, value_name = "RUN", conflicts_with = "app")]
    pub(super) since: Option<PathBuf>,
    /// The requests split by kind, prefix, size, caller or key, and where
    /// the slow and the costly requests are.
    #[arg(long, value_name = "DIMENSION", value_parser = by_arg,
          conflicts_with_all = ["errors", "requests", "all", "app", "since"])]
    pub(super) by: Option<Dimension>,
    /// The format written: text or json.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    pub(super) format: Format,
}

/// What `--by` splits by. Not the SDK, which the summary's head line
/// names, nor the status, which its error lines count.
const BY: [Dimension; 5] = [
    Dimension::Kind,
    Dimension::Prefix,
    Dimension::Size,
    Dimension::Caller,
    Dimension::Key,
];

fn by_arg(text: &str) -> Result<Dimension, String> {
    Dimension::parse(text)
        .filter(|d| BY.contains(d))
        .ok_or_else(|| {
            let all: Vec<&str> = BY.iter().map(|d| d.name()).collect();
            format!("{text:?} is not one of {}", all.join(", "))
        })
}

/// `sonde show --by`: the operations table split by one dimension, and the
/// sentence that says where the slow and the costly requests are.
fn print_split(
    split: &sonde_analysis::split::Split,
    header: &sonde_analysis::run::RunHeader,
    requests: usize,
    seconds: f64,
    callers: (usize, bool),
    color: bool,
) {
    use sonde_analysis::program::money_text;
    if let Some(c) = &header.command {
        println!("{}", paint(&format!("$ {c}"), "1", color));
    }
    let from = super::record::from_whom(split.sdk.as_deref(), header, callers.0, callers.1);
    println!(
        "{}",
        paint(
            &format!(
                "{}{from} to {} in {}",
                output::count(requests, "request"),
                header.endpoint,
                output::duration(seconds * 1000.0)
            ),
            "1",
            color
        )
    );
    // an imported log's waiting is not known: no column says it
    let waiting = !header.whole_seconds;
    let rows: Vec<Vec<String>> = split
        .rows
        .iter()
        .map(|r| {
            let mut row = vec![
                if r.other {
                    format!("other ({})", output::count(r.requests, "request"))
                } else {
                    r.key.clone()
                },
                output::grouped(r.requests as u64),
                if r.errors > 0 {
                    format!(
                        "{} ({})",
                        output::grouped(r.errors as u64),
                        share_text(r.error_share)
                    )
                } else {
                    String::new()
                },
                output::duration(r.p50_ms),
                output::duration(r.p95_ms),
                output::duration(r.p99_ms),
            ];
            if waiting {
                row.push(share_text(r.share));
            }
            row.push(money_text(r.cost, &split.currency));
            row
        })
        .collect();
    let mut heads = vec![split.by.name(), "requests", "errors", "p50", "p95", "p99"];
    if waiting {
        heads.push("waiting");
    }
    heads.push("cost");
    println!("{}", output::table(&heads, &rows));
    if !split.text.is_empty() {
        println!("{}", split.text);
    }
}

/// One operation on one line: time, format, what happened, size of it, notes.
/// `width` pads the title; `hide` lists notes said once for the run.
/// `formats`: whether the run has format steps (else no format column).
pub fn episode_line(
    e: &Episode,
    width: usize,
    hide: &[String],
    formats: bool,
    color: bool,
) -> String {
    let time = e
        .start
        .with_timezone(&chrono::Local)
        .format("%H:%M:%S%.3f")
        .to_string();
    let format = e.format.map(|f| f.name()).unwrap_or("");
    let n = e.requests();
    let meta = format!(
        "{} req{} · {}",
        output::grouped(n as u64),
        if n == 1 { "" } else { "s" },
        output::duration(e.millis())
    );
    let pad = width.saturating_sub(e.title().chars().count());
    let column = if formats {
        format!(
            "{}{}  ",
            paint(format, "36", color),
            " ".repeat(7usize.saturating_sub(format.len()))
        )
    } else {
        String::new()
    };
    let mut line = format!(
        "{}  {column}{}{}  {}",
        paint(&time, "2", color),
        e.title(),
        " ".repeat(pad),
        paint(&format!("{meta:>16}"), "2", color),
    );
    match e.outcome() {
        Outcome::Ok => {}
        Outcome::Failed(why) => line.push_str(&paint(&format!("  ✗ {why}"), "31", color)),
        Outcome::Refused => line.push_str(&paint("  refused by sonde", "33", color)),
    }
    let notes: Vec<String> = e
        .notes()
        .into_iter()
        .filter(|n| !hide.contains(n))
        .collect();
    if !notes.is_empty() {
        line.push_str(&paint(&format!("  · {}", notes.join(" · ")), "2", color));
    }
    line
}

/// Past this many lines `sonde show` prints the first and the last
/// [`SHOW_ENDS`] and says how many are between (`--all` prints them).
const SHOW_LINES: usize = 2000;
const SHOW_ENDS: usize = 100;

/// Prints `n` lines, each made when it is its turn: all of them, or the two
/// ends of a long list with what is between them said, not printed.
fn print_lines(n: usize, all: bool, what: &str, line: &dyn Fn(usize) -> String) {
    if all || n <= SHOW_LINES {
        for i in 0..n {
            println!("{}", line(i));
        }
        return;
    }
    for i in 0..SHOW_ENDS {
        println!("{}", line(i));
    }
    println!(
        "  … {} between these: `--all` prints them, `-f json` writes them",
        output::count(n - 2 * SHOW_ENDS, what)
    );
    for i in n - SHOW_ENDS..n {
        println!("{}", line(i));
    }
}

pub async fn show(s: &Arc<Sonde>, a: &ShowArgs) -> anyhow::Result<i32> {
    let file = match &a.file {
        Some(f) => super::runs::resolve(s, f),
        None => which_run(s, project_name().as_deref())?.0,
    };
    // the run to compare with, read and let go first
    let other = match &a.since {
        Some(o) => Some(super::fixloop::Other::read(s, &super::runs::resolve(s, o))?),
        None => None,
    };
    // read for what it amounts to: a request in full (`-f json`) is written
    // from the file, never held with the rest
    let (file, run) = super::runs::open_lean(s, &file)?;
    if a.app {
        return open_in_app(&file);
    }
    let all = &run.records;
    let prices = request_prices_for(s, &run.header.endpoint);
    // what it amounts to, once: every part below reads this
    let mut analysis = Analysis::of(&run, &prices);
    // what the project keeps on purpose, each finding with why
    sonde_analysis::program::mark_expected(
        &mut analysis.observations,
        &super::check::expect_of(&run.header),
    );
    if let Some(by) = a.by {
        let split = sonde_analysis::split::split(all, by, &prices);
        match a.format {
            Format::Json => output::print_json(&serde_json::json!({
                "run": run.header,
                "split": split,
            })),
            _ => print_split(
                &split,
                &run.header,
                all.len(),
                analysis.summary.seconds,
                (analysis.profile.callers.len(), analysis.profile.callers_cut),
                std::io::stdout().is_terminal(),
            ),
        }
        return Ok(0);
    }
    // how its program stood under faults, each scenario since when
    let trials = sonde_analysis::trial::load_with_history(
        &file,
        sonde_analysis::program::key_of(&run.header).as_deref(),
    );
    // against the program's run before it, as `run` said it, or against
    // the run asked for: in every format
    let mut since = match other {
        Some(other) => Some(super::fixloop::since_other(
            s,
            &file,
            &run.header,
            all,
            &analysis,
            other,
        )),
        None => super::fixloop::since_last(s, &file, &run.header, all, &analysis),
    };
    // what did not survive is among its findings, and in its fix loop
    // against the program's trial before
    analysis.add_trials(&trials);
    let eps = &analysis.episodes;
    if let Some(since) = &mut since {
        since
            .findings
            .extend(sonde_analysis::trial::changes(&trials));
    }
    let trials: Vec<_> = trials.iter().map(|t| t.verdict()).collect();
    match a.format {
        // the answer: the summary, the findings each cut to what to change
        // and where; every operation (--all), the failed ones (--errors), the
        // requests (--requests) when asked, written as they are made
        Format::Json => {
            use serde_json::{Value, json};
            use std::io::Write;
            let prof = &analysis.profile;
            let cwd = run.header.cwd.as_deref().map(Path::new);
            let out = crate::output::JsonWriter::new(std::io::BufWriter::with_capacity(
                1 << 16,
                crate::output::Stdout,
            ));
            let mut w = sonde_analysis::share::JsonOut::new(out, true);
            // each part lean ([`lean`]) and rounded ([`crate::output::rounded`])
            let field = |w: &mut sonde_analysis::share::JsonOut<_>, key: &str, mut v: Value| {
                lean(&mut v);
                crate::output::rounded(&mut v, key);
                w.field(key, &v)
            };
            w.begin_object()?;
            w.field("run", &run.header)?;
            field(&mut w, "summary", json!(analysis.summary))?;
            w.field("operationsTotal", &eps.len())?;
            field(
                &mut w,
                "waiting",
                json!({
                    "waitingMs": prof.waiting_ms,
                    "wallMs": prof.wall_ms,
                    "peakInFlight": prof.peak_in_flight,
                    "p50Ms": prof.p50_ms,
                    "p95Ms": prof.p95_ms,
                    "p99Ms": prof.p99_ms,
                    "byKind": prof.groups,
                }),
            )?;
            field(&mut w, "cost", json!(prof.cost))?;
            field(&mut w, "month", json!(analysis.month))?;
            field(&mut w, "time", json!(analysis.time))?;
            field(&mut w, "whatIf", json!(analysis.what_if))?;
            let diagnoses: Vec<Value> = analysis
                .diagnoses
                .iter()
                .map(|d| diagnosis_json(d, &analysis, a.all, cwd))
                .collect();
            field(&mut w, "diagnoses", json!(diagnoses))?;
            if a.all {
                let findings: Vec<Value> = analysis
                    .observations
                    .iter()
                    .map(|o| finding_json(o, cwd))
                    .collect();
                field(&mut w, "findings", json!(findings))?;
            }
            w.field("verify", &verify_command(s, &file, &run.header))?;
            let moved = a.all || a.since.is_some();
            field(&mut w, "since", since_json(since.as_ref(), moved))?;
            field(&mut w, "trials", json!(trials))?;
            if a.all || a.errors {
                w.array(
                    "operations",
                    eps.iter()
                        .filter(|e| !a.errors || *e.outcome() != Outcome::Ok),
                )?;
            }
            if a.requests {
                w.key("requests")?;
                w.begin_array()?;
                sonde_analysis::share::each_record(&file, run.afresh, &mut |r| {
                    if !a.errors || sonde_analysis::summary::is_failure(&r) {
                        w.value(&r)?;
                    }
                    Ok(())
                })?;
                w.end_array()?;
            }
            w.end_object()?;
            let mut out = w.finish()?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
        _ => {
            let color = std::io::stdout().is_terminal();
            if let Some(c) = &run.header.command {
                println!("{}", paint(&format!("$ {c}"), "1", color));
            }
            // a list asked for: the requests (then the summary), or what
            // failed (alone)
            if a.requests {
                let shown: Vec<&Record> = all
                    .iter()
                    .filter(|r| !a.errors || sonde_analysis::summary::is_failure(r))
                    .collect();
                if a.errors && shown.is_empty() {
                    println!("no errors: none of its requests failed");
                }
                print_lines(shown.len(), a.all, "request", &|i| {
                    timeline_line(shown[i], color)
                });
                if a.errors {
                    return Ok(0);
                }
            } else if a.errors || a.all {
                let shown: Vec<&Episode> = eps
                    .iter()
                    .filter(|e| !a.errors || *e.outcome() != Outcome::Ok)
                    .collect();
                let width = shown
                    .iter()
                    .map(|e| e.title().chars().count())
                    .max()
                    .unwrap_or(0)
                    .min(60);
                let hide = &analysis.shared_notes;
                let formats = eps.iter().any(|e| e.format.is_some());
                if a.errors && shown.is_empty() {
                    println!("no errors: none of its operations failed");
                }
                print_lines(shown.len(), a.all, "operation", &|i| {
                    episode_line(shown[i], width, hide, formats, color)
                });
                if a.errors {
                    return Ok(0);
                }
            }
            print_summary_of(
                &analysis,
                &run.header,
                &file,
                color,
                true,
                a.all,
                since.as_ref().map(|s| (s, a.since.is_some())),
                Says::Plan,
            );
            if !trials.is_empty() {
                println!("\nUnder faults:");
                for t in &trials {
                    println!("{}", super::run::trial_line(t, color));
                }
            }
        }
    }
    Ok(0)
}

/// Hands `sonde://open?path=…` to the system, which opens the app on the run.
fn open_in_app(file: &Path) -> anyhow::Result<i32> {
    let abs = std::fs::canonicalize(file)?;
    // Windows' canonical form starts \\?\ (a verbatim path): the app reads C:/…
    let path = abs.to_string_lossy().replace('\\', "/");
    let path = path
        .strip_prefix("//?/UNC/")
        .map(|p| format!("//{p}"))
        .or_else(|| path.strip_prefix("//?/").map(String::from))
        .unwrap_or(path);
    let encoded: String = path
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect();
    let url = format!("sonde://open?path={encoded}");
    let status = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&url).status()
    } else if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .status()
    } else {
        std::process::Command::new("xdg-open").arg(&url).status()
    };
    match status {
        Ok(s) if s.success() => Ok(0),
        _ => {
            eprintln!("could not open the sonde app; the link is {url}");
            Ok(1)
        }
    }
}

/// A run just made (`import`): its summary and findings on stderr, as
/// `run` says its own; stdout is for what was asked (`show` reads it).
pub fn summary_on_stderr(s: &Arc<Sonde>, file: &Path) -> anyhow::Result<i32> {
    let run = super::runs::read(file, sonde_analysis::run::Keep::All)?;
    let mut analysis = Analysis::of(&run, &request_prices_for(s, &run.header.endpoint));
    sonde_analysis::program::mark_expected(
        &mut analysis.observations,
        &super::check::expect_of(&run.header),
    );
    let since = super::fixloop::since_last(s, file, &run.header, &run.records, &analysis);
    print_summary_of(
        &analysis,
        &run.header,
        file,
        std::io::stderr().is_terminal(),
        false,
        false,
        since.as_ref().map(|s| (s, false)),
        Says::Plan,
    );
    Ok(0)
}

/// What `show -f json` leaves out of every part, as display or as the
/// same said twice: the phrases (their `text` beside them says it), a
/// finding's requests and level, its saving as phrases (`savingText` says
/// it), the per-kind table `summary` repeats (`waiting.byKind` is the one),
/// a kind's latencies added up.
const LEFT_OUT: [&str; 6] = ["phrases", "impact", "level", "saving", "kinds", "sumMs"];

/// `v` without [`LEFT_OUT`], at any depth.
fn lean(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            for k in LEFT_OUT {
                m.remove(k);
            }
            m.values_mut().for_each(lean);
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(lean),
        _ => {}
    }
}

/// A diagnosis as `show -f json` writes it: what an agent acts on, with no
/// join to make. Its head finding's `kind` and `text`, its other findings
/// said (`also`), why the project keeps any of them (`expected`, by
/// identity), where (`at`: its head's callers, each with its line of code),
/// what acting on it saves, and the `fix` for the SDK that sent its
/// requests. `all` keeps its findings by place (`findings`, `serial`).
fn diagnosis_json(
    d: &sonde_analysis::fixes::Diagnosis,
    a: &Analysis,
    all: bool,
    cwd: Option<&Path>,
) -> serde_json::Value {
    let mut v = serde_json::to_value(d).unwrap_or_default();
    let Some(m) = v.as_object_mut() else {
        return v;
    };
    m.remove("rest");
    if !all {
        m.remove("findings");
        m.remove("serial");
    }
    let Some(head) = d.findings.first().and_then(|&i| a.observations.get(i)) else {
        return v;
    };
    m.insert("kind".into(), head.kind.clone().into());
    m.insert("text".into(), head.text.clone().into());
    let also = super::fixloop::also_of(a, d);
    if !also.is_empty() {
        m.insert("also".into(), also.into());
    }
    let expected: serde_json::Map<String, serde_json::Value> = d
        .findings
        .iter()
        .filter_map(|&i| a.observations.get(i))
        .filter_map(|o| Some((o.identity.clone(), o.expected.clone()?.into())))
        .collect();
    if !expected.is_empty() {
        m.insert("expected".into(), expected.into());
    }
    let at = callers_json(head, cwd);
    if !at.is_empty() {
        m.insert("at".into(), at.into());
    }
    let sdk = head.sdk.as_deref().or(a.sdk.as_deref());
    if let Some(fix) = sonde_analysis::fixes::for_sdk(&head.fixes, sdk) {
        m.insert("fix".into(), serde_json::json!(fix));
    }
    v
}

/// A finding as `show --all -f json` writes it: whole, every operation and
/// each SDK's fix, and its callers with their lines of code.
fn finding_json(o: &sonde_analysis::episode::Observation, cwd: Option<&Path>) -> serde_json::Value {
    let mut v = serde_json::to_value(o).unwrap_or_default();
    if let Some(m) = v.as_object_mut() {
        m.insert("operationsTotal".into(), o.episodes.len().into());
        let callers = callers_json(o, cwd);
        if !callers.is_empty() {
            m.insert("callers".into(), callers.into());
        }
    }
    v
}

/// A finding's callers, each with its line of code read from the folder the
/// program ran in (`cwd`), when the file is there.
fn callers_json(
    o: &sonde_analysis::episode::Observation,
    cwd: Option<&Path>,
) -> Vec<serde_json::Value> {
    o.callers
        .iter()
        .map(|caller| {
            let mut c = serde_json::to_value(caller).unwrap_or_default();
            if let (Some(file), Some(line), Some(m)) =
                (caller.file.as_deref(), caller.line, c.as_object_mut())
                && let Some(code) = code_line(cwd, file, line)
            {
                m.insert("code".into(), code.into());
            }
            c
        })
        .collect()
}

/// The change since the run before as `show -f json` writes it: none for a
/// program's first run (nothing compared: its findings, all new, are the
/// plan; a trial's against the program's trial before are compared).
/// Each finding is said once, in `findings`, with the requests it saved
/// (`savedRequests`) beside what was predicted; the run's time once, in the
/// change (`wallMs`, `noiseMs`, `withinNoise`); the change keeps the figures, and each kind of request and
/// prefix that moved when they are asked for (`moved`: `--since`, `--all`),
/// as the text says them.
fn since_json(
    since: Option<&sonde_analysis::fixloop::SinceLast>,
    moved: bool,
) -> serde_json::Value {
    let compared = |s: &&sonde_analysis::fixloop::SinceLast| {
        s.previous.is_some()
            || s.failed.is_some()
            || s.empty
            || s.unsaved.is_some()
            || s.findings
                .iter()
                .any(|f| f.state != sonde_analysis::fixloop::State::New)
    };
    let Some(s) = since.filter(compared) else {
        return serde_json::Value::Null;
    };
    let mut v = serde_json::to_value(s).unwrap_or_default();
    if let Some(c) = v.get_mut("change").and_then(|c| c.as_object_mut()) {
        for k in ["findings", "resolved", "appeared", "latency"] {
            c.remove(k);
        }
        if !moved {
            c.remove("groups");
            c.remove("prefixes");
        }
    }
    for f in v
        .get_mut("findings")
        .and_then(|f| f.as_array_mut())
        .into_iter()
        .flatten()
        .filter_map(|f| f.as_object_mut())
    {
        // (the phrase is in `savedText`; `predicted` beside what was measured)
        f.remove("saved");
        if !f.contains_key("savedRequests") {
            f.remove("predicted");
        }
    }
    v
}

/// Line `line` of `file` (relative to `cwd`, where the program ran), trimmed
/// and cut to 200 characters; none when the file is not there to read.
fn code_line(cwd: Option<&Path>, file: &str, line: u32) -> Option<String> {
    use std::io::BufRead;
    let path = Path::new(file);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd?.join(path)
    };
    let f = std::fs::File::open(path).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let text = std::io::BufReader::new(f)
        .lines()
        .nth(line.checked_sub(1)? as usize)?
        .ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.chars().take(200).collect())
}

/// The command that records the program again, to see a fix work: as it
/// was run (`sonde run --url URL -- python3 etl.py`, `-e NAME`, `--demo`,
/// in the folder it ran in, under the policy and with the TLS it was
/// recorded with), and a run written with `-o` written beside it
/// again under the next name (`footer1.sonde`, then `footer2.sonde`). None
/// for a run that is no program's (an import, a replay).
fn verify_command(
    s: &Sonde,
    path: &Path,
    header: &sonde_analysis::run::RunHeader,
) -> Option<String> {
    let command = header.command.as_deref()?;
    // the tour's runs: the tour again
    if command.starts_with("sonde run") {
        return Some(command.to_string());
    }
    if sonde_analysis::program::program_key(Some(command)).is_none()
        || command.starts_with(sonde_analysis::program::LOGS_OF)
    {
        return None;
    }
    let mut args = vec!["sonde run".to_string()];
    args.extend(super::record::store_args(header));
    if let Some(n) = header.name.as_deref() {
        args.push(format!(
            "--name {}",
            sonde_analysis::program::shell_join(&[n.into()])
        ));
    }
    args.extend(policy_arg(header));
    if header.tls {
        args.push("--tls".into());
    }
    if let Some(out) = next_beside(s, path) {
        args.push(format!(
            "-o {}",
            sonde_analysis::program::shell_join(&[out.to_string_lossy().into_owned()])
        ));
    }
    args.push(format!("-- {command}"));
    Some(args.join(" "))
}

/// `--policy P` for a run recorded under a policy `sonde run` would not
/// choose by itself in the folder it ran in (`[record] policy` of its
/// sonde.toml, else allow): recorded again without it, the program would be
/// let do more, or less. None for a run that does not say (before 33.0), or
/// names no policy sonde has.
fn policy_arg(header: &sonde_analysis::run::RunHeader) -> Option<String> {
    let recorded = sonde_proxy::Policy::named(header.policy.as_deref()?)?;
    let own = header
        .cwd
        .as_deref()
        .and_then(|d| sonde_analysis::project::find(Path::new(d)))
        .and_then(|f| sonde_analysis::project::read(&f).ok())
        .and_then(|p| p.policy)
        .unwrap_or(sonde_proxy::Policy::Allow);
    (recorded != own).then(|| format!("--policy {}", recorded.name()))
}

/// Where a run written with `-o` (outside the runs folder) is written
/// again: beside it, its name's last number one more (`footer1.sonde`,
/// `footer2.sonde`), else `-2` (`etl.sonde`, `etl-2.sonde`), a name not
/// taken. None for a run in the runs folder.
fn next_beside(s: &Sonde, path: &Path) -> Option<PathBuf> {
    let path = std::path::absolute(path).ok()?;
    let dir = sonde_analysis::fixloop::folder_of(&path);
    let runs = s.paths().runs();
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if canon(&dir) == canon(&runs) {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    let stem = name.strip_suffix(".sonde").unwrap_or(&name);
    let digits = stem.len() - stem.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (base, mut n) = match stem[stem.len() - digits..].parse::<u64>() {
        Ok(n) if digits > 0 => (stem[..stem.len() - digits].to_string(), n + 1),
        _ => (format!("{stem}-"), 2),
    };
    loop {
        let p = dir.join(format!("{base}{n}.sonde"));
        if !p.exists() {
            return Some(p);
        }
        n += 1;
    }
}
