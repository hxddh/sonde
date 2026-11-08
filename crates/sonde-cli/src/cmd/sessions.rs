//! `sonde sessions`: the recorded sessions, newest first, and pruning old ones.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Subcommand};
use sonde_core::Sonde;
use sonde_proxy::session::read_header;

use crate::output::{self, Format};

#[derive(Args)]
pub struct SessionsArgs {
    #[command(subcommand)]
    action: Option<Action>,
    #[arg(long, short = 'f', value_enum, default_value_t, global = true)]
    format: Format,
}

#[derive(Subcommand)]
enum Action {
    /// List them, newest first (the default).
    List,
    /// Delete all but the newest N (baselines are kept). Without --yes, only says what it would delete.
    Prune {
        #[arg(long)]
        keep: usize,
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
    endpoint: String,
    bytes: u64,
}

/// The sessions in the data directory, newest first.
fn rows(s: &Sonde) -> Vec<Row> {
    let dir = s.paths().sessions();
    let mut out: Vec<Row> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("sonde") {
                return None;
            }
            let h = read_header(&p)?;
            Some(Row {
                name: p.file_name()?.to_string_lossy().into_owned(),
                path: p.to_string_lossy().into_owned(),
                created: h.created.to_rfc3339(),
                program: sonde_proxy::program::program_key(h.command.as_deref()),
                endpoint: h.endpoint,
                bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
            })
        })
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created).then(b.name.cmp(&a.name)));
    out
}

/// A session named on the command line: a path, or a name in the sessions directory.
pub fn resolve(s: &Sonde, f: &PathBuf) -> PathBuf {
    if f.exists() {
        return f.clone();
    }
    let dir = s.paths().sessions();
    for cand in [dir.join(f), dir.join(format!("{}.sonde", f.display()))] {
        if cand.is_file() {
            return cand;
        }
    }
    f.clone()
}

pub async fn run(s: &Arc<Sonde>, a: &SessionsArgs) -> anyhow::Result<i32> {
    let all = rows(s);
    match &a.action {
        None | Some(Action::List) => {
            if matches!(a.format, Format::Json) {
                println!("{}", output::json(&all));
                return Ok(0);
            }
            if all.is_empty() {
                eprintln!(
                    "no sessions yet: `sonde demo` for a tour, `sonde run -- <command>` for your program"
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
            let rows: Vec<Vec<String>> = all
                .iter()
                .map(|r| {
                    vec![
                        r.name.clone(),
                        local(&r.created),
                        r.program.clone().unwrap_or_default(),
                        r.endpoint.clone(),
                        output::bytes(r.bytes),
                    ]
                })
                .collect();
            output::print(
                a.format,
                &["session", "recorded", "program", "endpoint", "size"],
                &rows,
            );
            Ok(0)
        }
        Some(Action::Prune { keep, yes }) => {
            let dir = s.paths().sessions();
            let baselines: std::collections::HashSet<String> =
                sonde_proxy::program::read_baselines(&dir)
                    .into_values()
                    .collect();
            let gone: Vec<&Row> = all
                .iter()
                .skip(*keep)
                .filter(|r| !baselines.contains(&r.name))
                .collect();
            for r in &gone {
                if *yes {
                    std::fs::remove_file(&r.path)?;
                }
                println!(
                    "{}{}",
                    if *yes { "deleted " } else { "would delete " },
                    r.name
                );
            }
            if !*yes && !gone.is_empty() {
                eprintln!("{} sessions: add --yes to delete them", gone.len());
            }
            Ok(0)
        }
    }
}

#[derive(Args)]
pub struct ImportArgs {
    /// An S3 server access log, a CloudTrail file (`{"Records": …}` or one event a line), or a HAR.
    file: PathBuf,
    /// The session file (default: a new one in the data directory's sessions/).
    #[arg(long, short = 'o')]
    out: Option<PathBuf>,
}

/// Production traffic as a session: the same operations, findings and
/// figures as a recording, with no proxy in front of the program.
pub async fn import(s: &Arc<Sonde>, a: &ImportArgs) -> anyhow::Result<i32> {
    let bytes = std::fs::read(&a.file)?;
    let text = String::from_utf8_lossy(&bytes);
    let name = a
        .file
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "imported".into());
    let (source, session) = match sonde_proxy::import::import(&text, &name) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("sonde: {}: {e}", a.file.display());
            return Ok(64);
        }
    };
    let path = match &a.out {
        Some(p) => p.clone(),
        None => {
            let dir = s.paths().sessions();
            std::fs::create_dir_all(&dir)?;
            sonde_proxy::session::fresh_path(&dir, &name)
        }
    };
    sonde_proxy::import::save(&session, &path)?;
    eprintln!(
        "{} requests from a {} → {}",
        session.records.len(),
        source.name(),
        path.display()
    );
    super::record::show_file(s, &path).await
}
