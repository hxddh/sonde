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
    /// The requests split by kind, prefix, size, caller, sdk, status or
    /// key, and where the slow and the costly requests are.
    #[arg(long, value_name = "DIMENSION", value_parser = by_arg,
          conflicts_with_all = ["errors", "requests", "all", "app"])]
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

fn by_arg(text: &str) -> Result<Dimension, String> {
    Dimension::parse(text).ok_or_else(|| {
        let all: Vec<&str> = Dimension::ALL.iter().map(|d| d.name()).collect();
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
    let eps = &analysis.episodes;
    if let Some(by) = a.by {
        let split = sonde_analysis::split::split(all, by, &prices);
        match a.format {
            Format::Json => println!(
                "{}",
                output::json(&serde_json::json!({
                    "run": run.header,
                    "split": split,
                }))
            ),
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
    // against the program's run before it, as `run` said it: in every format
    let since = super::fixloop::since_last(s, &file, &run.header, all, &analysis);
    match a.format {
        // the answer: the summary, the findings each cut to what to change
        // and where; every operation (--all), the failed ones (--errors), the
        // requests (--requests) when asked, written as they are made
        Format::Json => {
            use std::io::Write;
            let prof = &analysis.profile;
            let sdk = analysis.sdk.as_deref();
            let cwd = run.header.cwd.as_deref().map(Path::new);
            let findings: Vec<serde_json::Value> = analysis
                .observations
                .iter()
                .map(|o| finding_json(o, a.all, sdk, cwd))
                .collect();
            let out = std::io::BufWriter::with_capacity(1 << 16, crate::output::Stdout);
            let mut w = sonde_analysis::share::JsonOut::new(out, true);
            w.begin_object()?;
            w.field("run", &run.header)?;
            w.field("summary", &analysis.summary)?;
            w.field("operationsTotal", &eps.len())?;
            w.field(
                "waiting",
                &serde_json::json!({
                    "waitingMs": prof.waiting_ms,
                    "wallMs": prof.wall_ms,
                    "peakInFlight": prof.peak_in_flight,
                    "p50Ms": prof.p50_ms,
                    "p95Ms": prof.p95_ms,
                    "p99Ms": prof.p99_ms,
                    "byKind": prof.groups,
                }),
            )?;
            w.field("cost", &prof.cost)?;
            w.field("month", &analysis.month)?;
            w.field("whatIf", &analysis.what_if)?;
            w.field("findings", &findings)?;
            w.field("verify", &verify_command(&run.header))?;
            w.field("since", &since)?;
            w.field("trials", &trials)?;
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
                sonde_analysis::share::each_record(
                    &file,
                    &mut all.iter().map(|r| r.id),
                    &mut |r| {
                        if !a.errors || sonde_analysis::summary::is_failure(&r) {
                            w.value(&r)?;
                        }
                        Ok(())
                    },
                )?;
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
                since.as_ref(),
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

/// A run's summary and findings on stdout, as `sonde show` writes them.
pub async fn show_file(s: &Arc<Sonde>, file: &Path) -> anyhow::Result<i32> {
    show(
        s,
        &ShowArgs {
            file: Some(file.to_path_buf()),
            errors: false,
            requests: false,
            all: false,
            app: false,
            by: None,
            format: Format::Text,
        },
    )
    .await
}

/// Operations a finding names in `show -f json` (all of them with --all).
const FINDING_OPERATIONS: usize = 20;

/// A finding as `show -f json` writes it: its first operations and how many
/// there are, the fix for the run's SDK (each SDK's with `all`, or when
/// none is known), and each caller's line of code read from the folder the
/// program ran in (`cwd`), when the file is there.
fn finding_json(
    o: &sonde_analysis::episode::Observation,
    all: bool,
    sdk: Option<&str>,
    cwd: Option<&Path>,
) -> serde_json::Value {
    let mut v = serde_json::to_value(o).unwrap_or_default();
    let Some(m) = v.as_object_mut() else {
        return v;
    };
    m.insert("operationsTotal".into(), o.episodes.len().into());
    if !all {
        if o.episodes.len() > FINDING_OPERATIONS {
            m.insert(
                "operations".into(),
                serde_json::json!(&o.episodes[..FINDING_OPERATIONS]),
            );
        }
        let sdk = o.sdk.as_deref().or(sdk);
        if let Some(fix) = sonde_analysis::fixes::for_sdk(&o.fixes, sdk) {
            m.insert("fixes".into(), serde_json::json!([fix]));
        }
    }
    if let Some(callers) = m.get_mut("callers").and_then(|c| c.as_array_mut()) {
        for (c, caller) in callers.iter_mut().zip(&o.callers) {
            if let (Some(file), Some(line), Some(c)) =
                (caller.file.as_deref(), caller.line, c.as_object_mut())
                && let Some(code) = code_line(cwd, file, line)
            {
                c.insert("code".into(), code.into());
            }
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

/// The command that records the program again, to see a fix work (`sonde
/// run --demo -- python3 etl.py`, in the folder it ran in): none for a run
/// that is no program's (an import, a replay).
fn verify_command(header: &sonde_analysis::run::RunHeader) -> Option<String> {
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
    let store = if header.endpoint == "demo" {
        "--demo".to_string()
    } else {
        format!("-e {}", header.endpoint)
    };
    let name = header
        .name
        .as_deref()
        .map(|n| format!(" --name {n}"))
        .unwrap_or_default();
    Some(format!("sonde run {store}{name} -- {command}"))
}
