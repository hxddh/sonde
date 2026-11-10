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
            // which sessions are baselines is unknown: none is deleted
            let baselines: std::collections::HashSet<String> =
                match sonde_proxy::program::read_baselines(&dir) {
                    Ok(all) => all.into_values().collect(),
                    Err(e) => {
                        eprintln!("sonde: {e} (nothing was deleted)");
                        return Ok(64);
                    }
                };
            let gone: Vec<&Row> = all
                .iter()
                .skip(*keep)
                .filter(|r| !baselines.contains(&r.name))
                .collect();
            for r in &gone {
                if *yes {
                    std::fs::remove_file(&r.path)?;
                    sonde_proxy::trial::remove(std::path::Path::new(&r.path));
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
    // as S3 delivers CloudTrail, and as logs are often kept: gzip
    let text = match sonde_proxy::import::text_of(&bytes) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("sonde: {}: {e}", a.file.display());
            return Ok(64);
        }
    };
    let name = a
        .file
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "imported".into());
    let name = name.strip_suffix(".json").unwrap_or(&name).to_string();
    let imported = match sonde_proxy::import::import_counted(&text, &name) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("sonde: {}: {e}", a.file.display());
            return Ok(64);
        }
    };
    let (source, session) = (imported.source, &imported.session);
    let path = match &a.out {
        Some(p) => p.clone(),
        None => {
            let dir = s.paths().sessions();
            std::fs::create_dir_all(&dir)?;
            sonde_proxy::session::fresh_path(&dir, &name)
        }
    };
    sonde_proxy::import::save(session, &path)?;
    eprintln!(
        "{} requests from a {} → {}",
        session.records.len(),
        source.name(),
        path.display()
    );
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
    super::record::show_file(s, &path).await
}

#[derive(Args)]
pub struct ExportArgs {
    /// The session (default: the newest).
    file: Option<PathBuf>,
    /// OpenTelemetry traces (OTLP/JSON): each operation a span, its requests under it.
    #[arg(long, required = true)]
    otlp: bool,
    /// Write it here (default: stdout).
    #[arg(long, short = 'o')]
    out: Option<PathBuf>,
    /// Send it to an OTLP/HTTP collector instead, e.g. http://localhost:4318/v1/traces.
    #[arg(long, conflicts_with = "out")]
    to: Option<String>,
}

pub async fn export(s: &Arc<Sonde>, a: &ExportArgs) -> anyhow::Result<i32> {
    let file = match &a.file {
        Some(f) => resolve(s, f),
        None => super::record::newest_session(s)?,
    };
    let session = sonde_proxy::session::Session::open(&file).map_err(|e| anyhow::anyhow!(e))?;
    let doc = sonde_proxy::otlp::traces(&session);
    match (&a.to, &a.out) {
        (Some(url), _) => {
            let resp = reqwest::Client::new()
                .post(url)
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&doc)?)
                .send()
                .await?;
            if !resp.status().is_success() {
                eprintln!("sonde: {url} answered {}", resp.status());
                return Ok(70);
            }
            eprintln!("{} requests sent to {url} as traces", session.records.len());
        }
        (None, Some(p)) => std::fs::write(p, serde_json::to_vec_pretty(&doc)?)?,
        (None, None) => println!("{}", output::json(&doc)),
    }
    Ok(0)
}
