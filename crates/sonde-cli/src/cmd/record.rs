//! Recording: `sonde run -- <command>`, `sonde proxy`, `sonde show <session>`.

use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Args;
use reqlog::{Record, RequestLog};
use sonde_core::Sonde;
use sonde_core::guard::human_bytes;
use sonde_proxy::session::{Session, SessionHeader, SessionWriter};
use sonde_proxy::summary::{Summary, summarize};
use sonde_proxy::{Policy, Proxy, ProxyConfig};

use crate::Cli;
use crate::output::{self, Format};

#[derive(Args)]
pub struct Recording {
    /// The endpoint requests go to (default: --url, or the only one configured).
    #[arg(long, short = 'e')]
    endpoint: Option<String>,
    /// Refuse operations that remove data (deletes, aborts, lifecycle rules).
    #[arg(long, conflicts_with = "read_only")]
    no_delete: bool,
    /// Let only GET and HEAD through.
    #[arg(long)]
    read_only: bool,
    /// The session file (default: a new file in the data directory's sessions/).
    #[arg(long, short = 'o')]
    out: Option<PathBuf>,
}

#[derive(Args)]
pub struct RunArgs {
    #[command(flatten)]
    rec: Recording,
    /// Print each request on stderr as it happens.
    #[arg(long)]
    live: bool,
    /// The command, after `--`.
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

#[derive(Args)]
pub struct ProxyArgs {
    #[command(flatten)]
    rec: Recording,
    /// Address to listen on.
    #[arg(long, default_value = "127.0.0.1:9000")]
    listen: SocketAddr,
}

#[derive(Args)]
pub struct ShowArgs {
    /// A session file.
    file: PathBuf,
    /// Only the summary.
    #[arg(long)]
    summary: bool,
    /// Only error responses.
    #[arg(long)]
    errors: bool,
    #[arg(long, short = 'f', value_enum, default_value_t)]
    format: Format,
}

struct Started {
    proxy: Proxy,
    log: RequestLog,
    writer: Arc<SessionWriter>,
    path: PathBuf,
}

async fn start(
    s: &Arc<Sonde>,
    r: &Recording,
    listen: SocketAddr,
    command: Option<String>,
) -> anyhow::Result<Started> {
    let ep = super::endpoint(s, r.endpoint.as_deref())?;
    let client = s.client(&ep)?;
    let cfg = client.config();
    let path = match &r.out {
        Some(p) => p.clone(),
        None => {
            let dir = s.paths().sessions();
            std::fs::create_dir_all(&dir)?;
            let name = command
                .as_deref()
                .and_then(|c| c.split_whitespace().next())
                .map(|c| {
                    Path::new(c)
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "proxy".into());
            dir.join(format!(
                "{}-{name}.sonde",
                chrono::Local::now().format("%Y%m%d-%H%M%S")
            ))
        }
    };
    let header = SessionHeader::new(&ep, cfg.endpoint.as_str(), &cfg.region, command);
    let writer = Arc::new(SessionWriter::create(&path, &header)?);
    let log = RequestLog::new(4096);
    log.set_sink(Some(writer.clone()));
    let policy = if r.read_only {
        Policy::ReadOnly
    } else if r.no_delete {
        Policy::NoDelete
    } else {
        Policy::Allow
    };
    let proxy = Proxy::start(
        client,
        log.clone(),
        ProxyConfig {
            listen,
            policy,
            ..Default::default()
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("cannot listen on {listen}: {e}"))?;
    Ok(Started {
        proxy,
        log,
        writer,
        path,
    })
}

/// Prints records as they arrive until the task is aborted.
fn follow(log: &RequestLog, to_stderr: bool) -> tokio::task::JoinHandle<()> {
    let mut rx = log.subscribe();
    let log = log.clone();
    let color = if to_stderr {
        std::io::stderr().is_terminal()
    } else {
        std::io::stdout().is_terminal()
    };
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(id) => {
                    if let Some(r) = log.get(id) {
                        let line = timeline_line(&r, color);
                        if to_stderr {
                            eprintln!("{line}");
                        } else {
                            println!("{line}");
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            }
        }
    })
}

fn paint(s: &str, code: &str, color: bool) -> String {
    if color {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

/// One request on one line: time, status, latency, bytes, operation, target.
pub fn timeline_line(r: &Record, color: bool) -> String {
    let time = r
        .time
        .with_timezone(&chrono::Local)
        .format("%H:%M:%S%.3f")
        .to_string();
    let refused = r
        .response_headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("x-sonde-proxy"));
    let miss = sonde_proxy::summary::is_miss(r);
    let status = match r.status {
        _ if refused => paint("BLOCK", "33", color),
        _ if miss => paint("404", "2", color),
        Some(s) if s >= 500 => paint(&s.to_string(), "31", color),
        Some(s) if s >= 400 => paint(&s.to_string(), "33", color),
        Some(s) => paint(&s.to_string(), "2", color),
        None => paint("ERR", "31", color),
    };
    let upload = matches!(r.method.as_str(), "PUT" | "POST");
    let bytes = match (r.request_bytes, r.response_bytes) {
        (Some(up), _) if up > 0 || (upload && !r.is_error()) => format!("↑{}", human_bytes(up)),
        (_, Some(down)) if down > 0 && !r.is_error() => format!("↓{}", human_bytes(down)),
        _ => String::new(),
    };
    let target = match (&r.bucket, &r.key) {
        (Some(b), Some(k)) => format!("{b}/{k}"),
        (Some(b), None) => format!("{b}/"),
        _ => "/".into(),
    };
    let conditions = r.request_headers.iter().filter_map(|h| {
        let n = h.name.to_ascii_lowercase();
        (n.starts_with("if-") || n == "x-amz-copy-source-if-match")
            .then(|| format!("{n}: {}", h.value))
    });
    let detail = r
        .query
        .iter()
        .filter(|(k, _)| {
            matches!(
                k.as_str(),
                "prefix" | "partNumber" | "start-after" | "continuation-token"
            )
        })
        .map(|(k, v)| {
            if k == "continuation-token" {
                "next page".to_string()
            } else {
                format!("{k}={v}")
            }
        })
        .chain(conditions)
        .collect::<Vec<_>>()
        .join("  ");
    // pad outside the paint: escape codes have no width
    let pad = " ".repeat(5usize.saturating_sub(visible_len(r, refused)));
    let latency = format!("{:.0} ms", r.latency_ms);
    let mut line = format!(
        "{}  {status}{pad}  {latency:>7}  {bytes:>10}  {:<24} {target}",
        paint(&time, "2", color),
        r.operation,
    );
    if !detail.is_empty() {
        line.push_str(&paint(&format!("  {detail}"), "2", color));
    }
    if let Some(e) = &r.transport_error {
        line.push_str(&paint(&format!("  {e}"), "31", color));
    } else if r.is_error()
        && !miss
        && let Some(code) = r.error_body.as_deref().and_then(|b| {
            let start = b.find("<Code>")? + 6;
            Some(&b[start..start + b[start..].find("</Code>")?])
        })
    {
        line.push_str(&paint(&format!("  {code}"), "33", color));
    }
    line
}

fn visible_len(r: &Record, refused: bool) -> usize {
    match r.status {
        _ if refused => 5,
        Some(s) => s.to_string().len(),
        None => 3,
    }
}

fn print_summary(s: &Summary, endpoint: &str, path: &Path, color: bool) {
    let mut head = format!(
        "{} requests to {endpoint} in {:.1} s",
        s.requests, s.seconds
    );
    if s.errors > 0 {
        head.push_str(&format!(
            " · {}",
            paint(&format!("{} errors", s.errors), "33", color)
        ));
    }
    if s.misses > 0 {
        head.push_str(&format!(" · {} not found", s.misses));
    }
    head.push_str(&format!(
        " · ↑{} ↓{}",
        human_bytes(s.bytes_up),
        human_bytes(s.bytes_down)
    ));
    eprintln!("\n{}", paint(&head, "1", color));
    if !s.operations.is_empty() {
        let rows: Vec<Vec<String>> = s
            .operations
            .iter()
            .map(|o| {
                vec![
                    o.operation.clone(),
                    o.count.to_string(),
                    if o.errors > 0 {
                        o.errors.to_string()
                    } else {
                        String::new()
                    },
                    if o.misses > 0 {
                        o.misses.to_string()
                    } else {
                        String::new()
                    },
                    format!("{:.0} ms", o.p50_ms),
                    format!("{:.0} ms", o.max_ms),
                ]
            })
            .collect();
        eprintln!(
            "{}",
            output::table(
                &["operation", "requests", "errors", "not found", "p50", "max"],
                &rows
            )
        );
    }
    for (what, n) in &s.error_codes {
        eprintln!("  {n} × {what}");
    }
    eprintln!(
        "{}",
        paint(&format!("session: {}", path.display()), "2", color)
    );
}

pub async fn run(s: &Arc<Sonde>, _cli: &Cli, a: &RunArgs) -> anyhow::Result<i32> {
    let command = a.command.join(" ");
    let started = start(
        s,
        &a.rec,
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Some(command),
    )
    .await?;
    let (set, unset) = started.proxy.env();
    let mut child = tokio::process::Command::new(&a.command[0]);
    child.args(&a.command[1..]);
    for k in &unset {
        child.env_remove(k);
    }
    for (k, v) in &set {
        child.env(k, v);
    }
    let printer = a.live.then(|| follow(&started.log, true));
    let mut child = child
        .spawn()
        .map_err(|e| anyhow::anyhow!("cannot start {}: {e}", a.command[0]))?;
    // Ctrl-C reaches the child (same process group); wait for it to finish
    let status = loop {
        tokio::select! {
            st = child.wait() => break st?,
            _ = tokio::signal::ctrl_c() => continue,
        }
    };
    if let Some(p) = printer {
        p.abort();
    }
    started.writer.flush()?;
    let session = Session::open(&started.path).map_err(|e| anyhow::anyhow!(e))?;
    let color = std::io::stderr().is_terminal();
    print_summary(
        &summarize(&session.records),
        &session.header.endpoint,
        &started.path,
        color,
    );
    Ok(status.code().unwrap_or(1))
}

pub async fn proxy(s: &Arc<Sonde>, _cli: &Cli, a: &ProxyArgs) -> anyhow::Result<i32> {
    let started = start(s, &a.rec, a.listen, None).await?;
    let (set, unset) = started.proxy.env();
    eprintln!(
        "sonde is listening on {}. Point the application at it:\n",
        started.proxy.url()
    );
    for k in &unset {
        eprintln!("  unset {k}");
    }
    for (k, v) in &set {
        eprintln!("  export {k}={v}");
    }
    eprintln!("\nCtrl-C stops.\n");
    let printer = follow(&started.log, false);
    tokio::signal::ctrl_c().await?;
    printer.abort();
    started.writer.flush()?;
    let session = Session::open(&started.path).map_err(|e| anyhow::anyhow!(e))?;
    print_summary(
        &summarize(&session.records),
        &session.header.endpoint,
        &started.path,
        std::io::stderr().is_terminal(),
    );
    Ok(0)
}

pub async fn show(a: &ShowArgs) -> anyhow::Result<i32> {
    let session = Session::open(&a.file).map_err(|e| anyhow::anyhow!(e))?;
    let records: Vec<&Record> = session
        .records
        .iter()
        .filter(|r| !a.errors || r.is_error())
        .collect();
    match a.format {
        Format::Json => {
            let owned: Vec<Record> = records.into_iter().cloned().collect();
            println!(
                "{}",
                output::json(&serde_json::json!({
                    "session": session.header,
                    "summary": summarize(&owned),
                    "records": owned,
                }))
            );
        }
        _ => {
            let color = std::io::stdout().is_terminal();
            if let Some(c) = &session.header.command {
                println!("{}", paint(&format!("$ {c}"), "1", color));
            }
            if !a.summary {
                for r in &records {
                    println!("{}", timeline_line(r, color));
                }
            }
            let owned: Vec<Record> = records.into_iter().cloned().collect();
            print_summary(
                &summarize(&owned),
                &session.header.endpoint,
                &a.file,
                std::io::stderr().is_terminal(),
            );
        }
    }
    Ok(0)
}
