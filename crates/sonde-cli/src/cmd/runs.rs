//! `sonde runs`: the recorded runs, newest first, and pruning old ones.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Subcommand};
use sonde_core::Sonde;

use crate::output::{self, Format};

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct RunsArgs {
    #[command(subcommand)]
    action: Option<Action>,
    #[command(flatten)]
    list: ListArgs,
}

#[derive(Args)]
struct ListArgs {
    /// The format written: text or json.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    format: Format,
}

#[derive(Subcommand)]
enum Action {
    /// List them, newest first (the default).
    List(ListArgs),
    /// Delete all but the newest N (baselines are kept): a dry run, unless --yes.
    Prune {
        /// How many of the newest runs to keep.
        #[arg(long, value_name = "N")]
        keep: usize,
        /// Delete the runs the dry run lists.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(serde::Serialize)]
struct Row {
    name: String,
    path: String,
    created: String,
    program: Option<String>,
    /// The command it ran, as it ran it: what tells two runs of a program apart.
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    /// The program's name, when it was given one.
    #[serde(skip)]
    named: Option<String>,
    endpoint: String,
    bytes: u64,
    /// What it holds: its requests, how long they spanned, how many failed.
    requests: u64,
    seconds: f64,
    errors: u64,
    /// How its trials stood, the worst of them: `broke`, `degraded`,
    /// `survived`, `untested`. None when it was never tried.
    #[serde(skip_serializing_if = "Option::is_none")]
    trials: Option<&'static str>,
    /// How its program exited (128 + a signal; 143 when sonde was sent
    /// SIGTERM). None when not known: a proxy's, a replay's, an import's.
    #[serde(skip_serializing_if = "Option::is_none")]
    exit: Option<i32>,
    /// Its first line in the middle that is no record: `show` refuses the
    /// file, so it is not listed as a run that reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    unreadable: Option<String>,
}

/// The runs in the data directory, newest first: each from the counts
/// kept in the folder's index (made the first time it is listed and when it
/// is recorded), never from the run read whole.
fn rows(s: &Sonde) -> Vec<Row> {
    let dir = s.paths().runs();
    let mut out: Vec<Row> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("sonde") {
                return None;
            }
            let c = sonde_proxy::summary::counted(&p)?;
            Some(Row {
                name: p.file_name()?.to_string_lossy().into_owned(),
                path: p.to_string_lossy().into_owned(),
                created: c.created.to_rfc3339(),
                program: c.program,
                command: c.command,
                named: c.name.filter(|n| !n.trim().is_empty()),
                endpoint: c.endpoint,
                bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
                requests: c.requests,
                seconds: c.seconds,
                errors: c.errors,
                trials: sonde_proxy::trial::worst(&sonde_proxy::trial::load(&p)).map(|s| s.label()),
                exit: sonde_proxy::index::get(&p).exit,
                unreadable: c.unreadable,
            })
        })
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created).then(b.name.cmp(&a.name)));
    out
}

/// How wide the terminal is: `COLUMNS` when it is set, else the terminal's
/// own answer. None when the output is not a terminal (a file, a pipe):
/// nothing is cut for one.
fn terminal_width() -> Option<usize> {
    if let Some(n) = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
    {
        return Some(n);
    }
    #[cfg(unix)]
    {
        use std::io::IsTerminal;
        let out = std::io::stdout();
        if out.is_terminal() {
            return rustix::termios::tcgetwinsize(&out)
                .ok()
                .map(|w| w.ws_col as usize)
                .filter(|n| *n > 0);
        }
    }
    None
}

/// `text` in `room` characters: its start and its end, the middle left out
/// (a command's end is its arguments, which tell two runs apart).
fn middle(text: &str, room: usize) -> String {
    let n = text.chars().count();
    if n <= room || room < 5 {
        return text.to_string();
    }
    let head = (room - 1) / 2;
    let tail = room - 1 - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(n - tail).collect();
    format!("{start}…{end}")
}

/// A run named on the command line: a path, or a name in the runs directory.
pub fn resolve(s: &Sonde, f: &PathBuf) -> PathBuf {
    if f.exists() {
        return f.clone();
    }
    // by name: a session of the sessions folder, with `.sonde` or without;
    // then a trial's session, which is kept in `trials/` there
    let dir = s.paths().runs();
    let named = format!("{}.sonde", f.display());
    for cand in [
        dir.join(f),
        dir.join(&named),
        dir.join("trials").join(f),
        dir.join("trials").join(&named),
    ] {
        if cand.is_file() {
            return cand;
        }
    }
    f.clone()
}

/// A run named on the command line, opened: every command that takes a
/// run takes it by path or by name, and one that is not there is said
/// with where it was looked for.
pub fn open(s: &Sonde, f: &PathBuf) -> anyhow::Result<(PathBuf, sonde_proxy::session::Session)> {
    let file = resolve(s, f);
    if !file.exists() {
        return Err(super::usage(format!(
            "no run {}: not a file, and not a name in {} (`sonde runs` lists them)",
            f.display(),
            s.paths().runs().display()
        )));
    }
    let session = read(&file, sonde_proxy::session::Keep::All)?;
    Ok((file, session))
}

/// A run's file read: one that is not a run is bad data (65), one
/// that could not be read an operation that failed (70), and one cut short
/// is read up to its cut and said on stderr.
pub fn read(
    file: &std::path::Path,
    keep: sonde_proxy::session::Keep,
) -> anyhow::Result<sonde_proxy::session::Session> {
    use sonde_proxy::session::{ReadError, Session};
    match Session::read_said(file, keep) {
        Ok(r) => {
            if let Some(cut) = r.cut {
                eprintln!("sonde: {}: {cut}", file.display());
            }
            Ok(r.session)
        }
        Err(ReadError::Bad(e)) => Err(super::bad_data(format!("{}: {e}", file.display()))),
        Err(ReadError::Io(e)) => Err(anyhow::anyhow!(e)),
    }
}

/// [`open`] for what a run amounts to (its operations, findings,
/// figures): its records without what no analysis reads.
pub fn open_lean(
    s: &Sonde,
    f: &PathBuf,
) -> anyhow::Result<(PathBuf, sonde_proxy::session::Session)> {
    let file = resolve(s, f);
    if !file.exists() {
        return Err(super::usage(format!(
            "no run {}: not a file, and not a name in {} (`sonde runs` lists them)",
            f.display(),
            s.paths().runs().display()
        )));
    }
    let session = read(&file, sonde_proxy::session::Keep::Analysis)?;
    Ok((file, session))
}

pub async fn run(s: &Arc<Sonde>, a: &RunsArgs) -> anyhow::Result<i32> {
    let format = match &a.action {
        Some(Action::List(l)) => l.format,
        _ => a.list.format,
    };
    let all = rows(s);
    match &a.action {
        None | Some(Action::List(_)) => {
            if matches!(format, Format::Json) {
                println!("{}", output::json(&all));
                return Ok(0);
            }
            if all.is_empty() {
                eprintln!(
                    "no runs yet: `sonde run --demo` for a tour, `sonde run -- <command>` for your program"
                );
                return Ok(0);
            }
            let local = |c: &str| {
                chrono::DateTime::parse_from_rfc3339(c)
                    .map(|d| {
                        d.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_default()
            };
            let mut rows: Vec<Vec<String>> = all
                .iter()
                .map(|r| {
                    vec![
                        r.name.clone(),
                        local(&r.created),
                        // its name, when it has one; else its command as it ran
                        r.named
                            .clone()
                            .or(r.command.clone())
                            .or(r.program.clone())
                            .unwrap_or_default(),
                        r.endpoint.clone(),
                        if r.unreadable.is_some() {
                            format!("{} !", output::grouped(r.requests))
                        } else {
                            output::grouped(r.requests)
                        },
                        output::duration(r.seconds * 1000.0),
                        output::grouped(r.errors),
                        r.exit.map(|x| x.to_string()).unwrap_or_default(),
                        r.trials.unwrap_or("").to_string(),
                        output::bytes(r.bytes),
                    ]
                })
                .collect();
            let mut heads = vec![
                "run", "recorded", "program", "endpoint", "requests", "took", "errors", "exit",
                "trials", "size",
            ];
            // a table has no column with nothing in it (no run's exit is
            // known: imports, replays); a file's columns stay what they are
            const EXIT: usize = 7;
            if matches!(format, Format::Text) && all.iter().all(|r| r.exit.is_none()) {
                heads.remove(EXIT);
                for r in &mut rows {
                    r.remove(EXIT);
                }
            }
            // a command may be a line long: in a terminal it is cut in its
            // middle so that every row fits (a file or a pipe gets it whole)
            if matches!(format, Format::Text)
                && let Some(width) = terminal_width()
            {
                const PROGRAM: usize = 2;
                let others: usize = (0..heads.len())
                    .filter(|i| *i != PROGRAM)
                    .map(|i| {
                        rows.iter()
                            .map(|r| r[i].chars().count())
                            .chain([heads[i].chars().count()])
                            .max()
                            .unwrap_or(0)
                            + 2
                    })
                    .sum();
                let room = width.saturating_sub(others).max(16);
                for r in &mut rows {
                    r[PROGRAM] = middle(&r[PROGRAM], room);
                }
            }
            output::print(&heads, &rows);
            for r in all.iter().filter(|r| r.unreadable.is_some()) {
                eprintln!(
                    "sonde: {} is not a run: {} (`sonde show` refuses it; its other lines are counted)",
                    r.name,
                    r.unreadable.as_deref().unwrap_or_default()
                );
            }
            Ok(0)
        }
        Some(Action::Prune { keep, yes }) => {
            let dir = s.paths().runs();
            let baselines: std::collections::HashSet<String> =
                sonde_proxy::program::baselines(&dir).into_values().collect();
            let gone: Vec<&Row> = all
                .iter()
                .skip(*keep)
                .filter(|r| !baselines.contains(&r.name))
                .collect();
            if gone.is_empty() {
                // said, not silence: nothing was past the newest N
                println!(
                    "nothing to remove: {} here, {} kept{}",
                    output::count(all.len(), "run"),
                    keep,
                    if all.len() > *keep {
                        " and the rest baselines"
                    } else {
                        ""
                    }
                );
                return Ok(0);
            }
            let yes = *yes;
            for r in &gone {
                if yes {
                    std::fs::remove_file(&r.path)?;
                    sonde_proxy::trial::remove(std::path::Path::new(&r.path));
                    // and what was counted of it, kept beside it
                    sonde_proxy::summary::forget(std::path::Path::new(&r.path));
                }
                println!(
                    "{}{}",
                    if yes { "deleted " } else { "would delete " },
                    r.name
                );
            }
            if !yes {
                eprintln!(
                    "dry run: to delete {}, run again with --yes",
                    if gone.len() == 1 { "it" } else { "them" }
                );
                // a dry run with something to do: 4, as every dry run
                return Ok(4);
            }
            Ok(0)
        }
    }
}

#[derive(Args)]
pub struct ImportArgs {
    /// An S3 server access log, a CloudTrail file (`{"Records": …}` or one
    /// event a line), or a HAR; or a folder of them (gzip or plain, folders
    /// within too), read in the order of their names into one run. Or
    /// `s3://bucket/prefix` where a store writes its access logs or
    /// CloudTrail files: what arrived since the last import of it (the
    /// first, the last day's), read through the endpoint (GET and LIST only).
    #[arg(value_name = "FILE|FOLDER|S3_URL")]
    file: PathBuf,
    /// The run's file (default: a new one in the data directory's runs/).
    #[arg(long, short = 'o')]
    out: Option<PathBuf>,
    /// With -o: write over the file if it is there (else it is refused).
    #[arg(long, requires = "out")]
    force: bool,
    /// With s3://: the endpoint the logs are read through.
    #[arg(long, short = 'e')]
    endpoint: Option<String>,
}

/// Production traffic as a run: the same operations, findings and
/// figures as a recording, with no proxy in front of the program.
pub async fn import(s: &Arc<Sonde>, a: &ImportArgs) -> anyhow::Result<i32> {
    let given = a.file.to_string_lossy();
    if given.starts_with("s3://") {
        if a.out.is_some() {
            return Err(super::usage(
                "-o is for a file or folder: logs read from s3:// are kept in the runs folder",
            ));
        }
        return super::logs::import_logs(s, a.endpoint.as_deref(), &given).await;
    }
    if a.endpoint.is_some() {
        return Err(super::usage(
            "--endpoint is for logs read from s3://bucket/prefix",
        ));
    }
    if let Some(out) = &a.out {
        super::record::refuse_existing(out, a.force)?;
    }
    // a folder is every file in it: S3 delivers logs as thousands of small ones
    let is_dir = a.file.is_dir();
    let name = if is_dir {
        // `.` and `logs/` have a name too
        a.file
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
    } else {
        a.file.file_stem().map(|n| n.to_string_lossy().into_owned())
    }
    .unwrap_or_else(|| "imported".into());
    let name = name.strip_suffix(".json").unwrap_or(&name).to_string();
    let mut folder = None;
    let imported = if is_dir {
        sonde_proxy::import::import_dir(&a.file, &name).map(|d| {
            folder = Some((d.files, d.left_out));
            d.imported
        })
    } else {
        // as S3 delivers CloudTrail, and as logs are often kept: gzip
        let bytes = std::fs::read(&a.file)
            .map_err(|e| super::usage(format!("{}: {e}", a.file.display())))?;
        sonde_proxy::import::text_of(&bytes)
            .and_then(|text| sonde_proxy::import::import_counted(&text, &name))
    };
    let imported = match imported {
        Ok(x) => x,
        // a file cut short or with nothing in it is bad data (65); the wrong
        // file, a usage error
        Err(e) => {
            eprintln!("sonde: {}: {e}", a.file.display());
            return Ok(if sonde_proxy::import::is_bad_data(&e) {
                65
            } else {
                64
            });
        }
    };
    let (source, session) = (imported.source, &imported.session);
    let path = match &a.out {
        Some(p) => p.clone(),
        None => {
            let dir = s.paths().runs();
            std::fs::create_dir_all(&dir)?;
            sonde_proxy::session::fresh_path(&dir, &name)
        }
    };
    sonde_proxy::import::save(session, &path)?;
    match &folder {
        Some((files, _)) => eprintln!(
            "{} from {files} file{} of {} → {}",
            output::count(session.records.len(), "request"),
            if *files == 1 { "" } else { "s" },
            source.name(),
            path.display()
        ),
        None => eprintln!(
            "{} from {} → {}",
            output::count(session.records.len(), "request"),
            source.a(),
            path.display()
        ),
    }
    if let Some((_, left_out)) = &folder
        && !left_out.is_empty()
    {
        eprintln!(
            "  {} file{} left out:",
            left_out.len(),
            if left_out.len() == 1 { "" } else { "s" }
        );
        for (file, why) in left_out.iter().take(5) {
            eprintln!("    {file}: {why}");
        }
        if left_out.len() > 5 {
            eprintln!("    … and {} more", left_out.len() - 5);
        }
    }
    let lines = |n: usize| format!("{n} line{}", if n == 1 { "" } else { "s" });
    if imported.malformed > 0 {
        eprintln!(
            "  {} could not be read and left out",
            lines(imported.malformed)
        );
    }
    if imported.not_requests > 0 {
        eprintln!(
            "  {} left out: the store's own work (lifecycle, batch), not requests",
            lines(imported.not_requests)
        );
    }
    if imported.other_hosts > 0 {
        eprintln!(
            "  {} left out: requests to other hosts, not to S3",
            if imported.other_hosts == 1 {
                "1 entry".to_string()
            } else {
                format!("{} entries", output::grouped(imported.other_hosts as u64))
            }
        );
    }
    if imported.other_services > 0 {
        eprintln!(
            "  {} left out: other services', not S3's",
            output::count(imported.other_services, "event")
        );
    }
    super::record::show_file(s, &path).await
}
