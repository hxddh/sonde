//! `sonde show <session>`: a session's operations, requests and findings.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Args;
use reqlog::Record;
use sonde_core::Sonde;
use sonde_proxy::episode::{Episode, Outcome};
use sonde_proxy::split::Dimension;
use sonde_proxy::view::Analysis;

use crate::output::{self, Format};

use super::record::*;

#[derive(Args)]
pub struct ShowArgs {
    /// A session file (default: the newest in the data directory's sessions/).
    pub(super) file: Option<PathBuf>,
    /// Only the summary.
    #[arg(long)]
    pub(super) summary: bool,
    /// Only what failed.
    #[arg(long)]
    pub(super) errors: bool,
    /// One line per request instead of per operation.
    #[arg(long)]
    pub(super) requests: bool,
    /// Every line of a long session: past a few thousand, only the first
    /// and the last hundred are printed.
    #[arg(long)]
    pub(super) all: bool,
    /// Open it in the sonde app instead (a sonde:// link).
    #[arg(long)]
    pub(super) app: bool,
    /// The operations table split by prefix, size, caller, sdk, status or
    /// operation, and where the slow and the costly requests are.
    #[arg(long, value_name = "DIMENSION", value_parser = by_arg,
          conflicts_with_all = ["summary", "errors", "requests", "all", "app"])]
    pub(super) by: Option<Dimension>,
    /// The format written: table or json.
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
    split: &sonde_proxy::split::Split,
    header: &sonde_proxy::session::SessionHeader,
    requests: usize,
    seconds: f64,
    color: bool,
) {
    use sonde_proxy::program::money_text;
    if let Some(c) = &header.command {
        println!("{}", paint(&format!("$ {c}"), "1", color));
    }
    let from = split
        .sdk
        .as_deref()
        .map(|s| format!(" from {s}"))
        .unwrap_or_default();
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
    let rows: Vec<Vec<String>> = split
        .rows
        .iter()
        .map(|r| {
            vec![
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
                share_text(r.share),
                money_text(r.cost, &split.currency),
            ]
        })
        .collect();
    println!(
        "{}",
        output::table(
            &[
                split.by.name(),
                "requests",
                "errors",
                "p50",
                "p95",
                "p99",
                "waiting",
                "cost",
            ],
            &rows,
        )
    );
    if !split.text.is_empty() {
        println!("{}", split.text);
    }
}

/// One operation on one line: time, format, what happened, size of it, notes.
/// `width` pads the title; `hide` lists notes said once for the session.
/// `formats`: whether the session has format steps (else no format column).
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
    match &e.outcome {
        Outcome::Ok => {}
        Outcome::Failed(why) => line.push_str(&paint(&format!("  ✗ {why}"), "31", color)),
        Outcome::Refused => line.push_str(&paint("  refused by sonde", "33", color)),
    }
    let notes: Vec<&str> = e
        .notes
        .iter()
        .filter(|n| !hide.contains(n))
        .map(String::as_str)
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
    if let Some(why) = output::unsupported(a.format, "show", &[Format::Table, Format::Json]) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    let file = match &a.file {
        Some(f) => super::sessions::resolve(s, f),
        None => newest_session(s)?,
    };
    // read for what it amounts to: a request in full (`-f json`) is written
    // from the file, never held with the rest
    let (file, session) = super::sessions::open_lean(s, &file)?;
    if a.app {
        return open_in_app(&file);
    }
    let all = &session.records;
    let prices = request_prices_for(s, &session.header.endpoint);
    // what it amounts to, once: every part below reads this
    let analysis = Analysis::of(&session, &prices);
    let eps = &analysis.episodes;
    if let Some(by) = a.by {
        let split = sonde_proxy::split::split(all, by, &prices);
        match a.format {
            Format::Json => println!(
                "{}",
                output::json(&serde_json::json!({
                    "session": session.header,
                    "split": split,
                }))
            ),
            _ => print_split(
                &split,
                &session.header,
                all.len(),
                analysis.summary.seconds,
                std::io::stdout().is_terminal(),
            ),
        }
        return Ok(0);
    }
    // how its program stood under faults, each scenario since when
    let trials = sonde_proxy::trial::load_with_history(
        &file,
        sonde_proxy::program::key_of(&session.header).as_deref(),
    );
    match a.format {
        // the summary alone: what the table's summary shows, and no more
        Format::Json if a.summary => {
            let prof = &analysis.profile;
            println!(
                "{}",
                output::json(&serde_json::json!({
                    "session": session.header,
                    "summary": analysis.summary,
                    "operations": eps.len(),
                    "waiting": {
                        "busyMs": prof.busy_ms,
                        "wallMs": prof.wall_ms,
                        "peakInFlight": prof.peak_in_flight,
                        "p50Ms": prof.p50_ms,
                        "p95Ms": prof.p95_ms,
                        "p99Ms": prof.p99_ms,
                        "byKind": prof.groups,
                    },
                    "cost": prof.cost,
                    "whatIf": analysis.what_if,
                    "observations": analysis.observations,
                    "trials": trials,
                }))
            );
        }
        // written as it is made: an operation, then a request, at a time
        Format::Json => {
            use std::io::Write;
            let out = std::io::BufWriter::with_capacity(1 << 16, std::io::stdout().lock());
            let mut w = sonde_proxy::share::JsonOut::new(out, true);
            w.begin_object()?;
            w.array(
                "episodes",
                eps.iter().filter(|e| !a.errors || e.outcome != Outcome::Ok),
            )?;
            w.field("observations", &analysis.observations)?;
            w.key("records")?;
            w.begin_array()?;
            sonde_proxy::share::each_record(&file, &mut all.iter().map(|r| r.id), &mut |r| {
                if !a.errors || sonde_proxy::summary::is_failure(&r) {
                    w.value(&r)?;
                }
                Ok(())
            })?;
            w.end_array()?;
            w.field("session", &session.header)?;
            w.field("summary", &analysis.summary)?;
            w.field("whatIf", &analysis.what_if)?;
            w.field("trials", &trials)?;
            w.end_object()?;
            let mut out = w.finish()?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
        _ => {
            let color = std::io::stdout().is_terminal();
            if let Some(c) = &session.header.command {
                println!("{}", paint(&format!("$ {c}"), "1", color));
            }
            if !a.summary && a.requests {
                let shown: Vec<&Record> = all
                    .iter()
                    .filter(|r| !a.errors || sonde_proxy::summary::is_failure(r))
                    .collect();
                if a.errors && shown.is_empty() {
                    println!("no errors: none of its requests failed");
                }
                print_lines(shown.len(), a.all, "request", &|i| {
                    timeline_line(shown[i], color)
                });
            } else if !a.summary {
                let shown: Vec<&Episode> = eps
                    .iter()
                    .filter(|e| !a.errors || e.outcome != Outcome::Ok)
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
            }
            print_summary_of(
                &analysis,
                &session.header,
                &file,
                std::io::stdout().is_terminal(),
                true,
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

/// Hands `sonde://open?path=…` to the system, which opens the app on the session.
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

/// A session's summary and findings on stdout, as `sonde show --summary`.
pub async fn show_file(s: &Arc<Sonde>, file: &Path) -> anyhow::Result<i32> {
    show(
        s,
        &ShowArgs {
            file: Some(file.to_path_buf()),
            summary: true,
            errors: false,
            requests: false,
            all: false,
            app: false,
            by: None,
            format: Format::Table,
        },
    )
    .await
}
