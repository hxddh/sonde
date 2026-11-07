//! Recording: `sonde run -- <command>`, `sonde proxy`, `sonde show <session>`.

use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Args;
use reqlog::{Record, RequestLog};
use sonde_core::Sonde;
use sonde_core::guard::human_bytes;
use sonde_proxy::episode::{Episode, Outcome, episodes, observations, shared_notes};
use sonde_proxy::session::{Session, SessionHeader, SessionWriter};
use sonde_proxy::summary::summarize;
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
    /// Only what failed.
    #[arg(long)]
    errors: bool,
    /// One line per request instead of per operation.
    #[arg(long)]
    requests: bool,
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
        "{n} req{} · {:.0} ms",
        if n == 1 { "" } else { "s" },
        e.millis()
    );
    let pad = width.saturating_sub(e.title.chars().count());
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
        e.title,
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

fn print_summary(records: &[Record], endpoint: &str, path: &Path, color: bool) {
    let s = &summarize(records);
    let eps = episodes(records);
    let mut head = format!(
        "{} requests ({} operations) to {endpoint} in {:.1} s",
        s.requests,
        eps.len(),
        s.seconds
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
    for o in observations(records, &eps) {
        let mark = if o.level == "warn" {
            paint("!", "33", color)
        } else {
            paint("·", "2", color)
        };
        eprintln!("{mark} {}", o.text);
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
        &session.records,
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
        &session.records,
        &session.header.endpoint,
        &started.path,
        std::io::stderr().is_terminal(),
    );
    Ok(0)
}

pub async fn show(a: &ShowArgs) -> anyhow::Result<i32> {
    let session = Session::open(&a.file).map_err(|e| anyhow::anyhow!(e))?;
    let all = &session.records;
    let eps = episodes(all);
    match a.format {
        Format::Json => {
            println!(
                "{}",
                output::json(&serde_json::json!({
                    "session": session.header,
                    "summary": summarize(all),
                    "episodes": eps,
                    "observations": observations(all, &eps),
                    "records": all,
                }))
            );
        }
        _ => {
            let color = std::io::stdout().is_terminal();
            if let Some(c) = &session.header.command {
                println!("{}", paint(&format!("$ {c}"), "1", color));
            }
            if !a.summary && a.requests {
                for r in all
                    .iter()
                    .filter(|r| !a.errors || sonde_proxy::summary::is_failure(r))
                {
                    println!("{}", timeline_line(r, color));
                }
            } else if !a.summary {
                let shown: Vec<&Episode> = eps
                    .iter()
                    .filter(|e| !a.errors || e.outcome != Outcome::Ok)
                    .collect();
                let width = shown
                    .iter()
                    .map(|e| e.title.chars().count())
                    .max()
                    .unwrap_or(0)
                    .min(60);
                let hide: Vec<String> = shared_notes(&eps)
                    .into_iter()
                    .map(|(_, n, _, _)| n)
                    .collect();
                let formats = eps.iter().any(|e| e.format.is_some());
                for e in shown {
                    println!("{}", episode_line(e, width, &hide, formats, color));
                }
            }
            print_summary(
                all,
                &session.header.endpoint,
                &a.file,
                std::io::stderr().is_terminal(),
            );
        }
    }
    Ok(0)
}

fn bucket_pair(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .filter(|(a, b)| !a.is_empty() && !b.is_empty())
        .ok_or_else(|| format!("{s:?}: expected original=target"))
}

#[derive(Args)]
pub struct ReplayArgs {
    /// The session to replay.
    file: PathBuf,
    /// The endpoint to replay on (default: --url, or the only one configured).
    #[arg(long, short = 'e')]
    endpoint: Option<String>,
    /// Use another bucket: original=target (repeatable).
    #[arg(long, value_parser = bucket_pair)]
    bucket: Vec<(String, String)>,
    /// The key prefix the replay writes under (default: replay-<time>/).
    #[arg(long, conflicts_with = "in_place")]
    prefix: Option<String>,
    /// Replay keys as they were recorded, bucket changes included.
    #[arg(long)]
    in_place: bool,
    /// Keep what the replay wrote (default: deleted afterwards).
    #[arg(long)]
    keep: bool,
    /// Do not ask for confirmation.
    #[arg(long, short = 'y')]
    yes: bool,
    /// The replay's session file (default: in the data directory's sessions/).
    #[arg(long, short = 'o')]
    out: Option<PathBuf>,
    #[arg(long, short = 'f', value_enum, default_value_t)]
    format: Format,
}

#[derive(Args)]
pub struct DiffArgs {
    /// The original session.
    original: PathBuf,
    /// Its replay (`sonde replay -o`).
    replay: PathBuf,
    #[arg(long, short = 'f', value_enum, default_value_t)]
    format: Format,
}

fn print_comparison(c: &sonde_proxy::replay::Comparison, color: bool) {
    let mut head = format!("{} compared: {} the same", c.compared, c.same);
    if c.differ() > 0 {
        head.push_str(&format!(
            " · {}",
            paint(&format!("{} differ", c.differ()), "33", color)
        ));
    }
    if c.environment > 0 {
        head.push_str(&format!(
            " · {} because the target already held it",
            c.environment
        ));
    }
    println!("{}", paint(&head, "1", color));
    for d in &c.differences {
        let mut line = format!(
            "{} {:<24} recorded {} → replay {}   ×{}   e.g. {}",
            paint("✗", "33", color),
            d.operation,
            d.original,
            d.replay,
            d.count,
            d.example
        );
        if !d.conditions.is_empty() {
            line.push_str(&paint(
                &format!("  ({})", d.conditions.join(", ")),
                "2",
                color,
            ));
        }
        println!("{line}");
    }
    for (why, n) in &c.unmatched {
        println!(
            "{}",
            paint(&format!("  {n} not replayed: {why}"), "2", color)
        );
    }
}

pub async fn replay(s: &Arc<Sonde>, _cli: &Cli, a: &ReplayArgs) -> anyhow::Result<i32> {
    use sonde_proxy::replay::{self as rp, ReplayOptions};
    let original = Session::open(&a.file).map_err(|e| anyhow::anyhow!(e))?;
    let ep = super::endpoint(s, a.endpoint.as_deref())?;
    let base = s.client(&ep)?;
    let prefix = if a.in_place {
        String::new()
    } else {
        a.prefix
            .clone()
            .unwrap_or_else(|| format!("replay-{}/", chrono::Local::now().format("%Y%m%d-%H%M%S")))
    };
    let opts = ReplayOptions {
        prefix: prefix.clone(),
        buckets: a.bucket.clone(),
    };
    let plan = rp::plan(&original.records, &opts);
    let mut buckets: Vec<String> = original
        .records
        .iter()
        .filter_map(|r| r.bucket.as_deref().map(|b| opts_bucket(&opts, b)))
        .collect();
    buckets.sort();
    buckets.dedup();
    let color = std::io::stderr().is_terminal();
    let place = if prefix.is_empty() {
        paint(
            "in place: keys as recorded, bucket changes included",
            "33",
            color,
        )
    } else {
        format!("under {prefix} in {}", buckets.join(", "))
    };
    eprintln!(
        "Replay {} requests from {} on {ep} ({}), {place}.",
        plan.requests,
        a.file.display(),
        base.config().endpoint
    );
    eprintln!(
        "  {} writes and {} deletes{}.",
        plan.writes,
        plan.deletes,
        if prefix.is_empty() {
            ""
        } else {
            ", all under that prefix: nothing outside it is touched"
        }
    );
    for (why, n) in &plan.skipped {
        eprintln!("  {n} left out: {why}");
    }
    if !prefix.is_empty() && !a.keep {
        eprintln!("  Afterwards, everything under {prefix} is deleted (--keep keeps it).");
    }
    if !a.yes {
        if !std::io::stdin().is_terminal() {
            eprintln!("sonde: replay writes to {ep}: pass --yes to confirm");
            return Ok(77);
        }
        eprint!("Type yes to replay: ");
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if answer.trim() != "yes" {
            eprintln!("Nothing was sent.");
            return Ok(77);
        }
    }
    let path = match &a.out {
        Some(p) => p.clone(),
        None => {
            let dir = s.paths().sessions();
            std::fs::create_dir_all(&dir)?;
            dir.join(format!(
                "{}-replay.sonde",
                chrono::Local::now().format("%Y%m%d-%H%M%S")
            ))
        }
    };
    let cfg = base.config();
    let header = SessionHeader::new(
        &ep,
        cfg.endpoint.as_str(),
        &cfg.region,
        Some(format!("sonde replay {}", a.file.display())),
    );
    let writer = Arc::new(SessionWriter::create(&path, &header)?);
    let log = RequestLog::new(4096);
    log.set_sink(Some(writer.clone()));
    let client = base.with_log(log).with_retry(s3raw::RetryPolicy::none());
    let show = color;
    let done = rp::replay(&client, &original.records, &opts, |i, n| {
        if show {
            eprint!("\r\x1b[2K{i}/{n}");
        }
    })
    .await;
    if show {
        eprint!("\r\x1b[2K");
    }
    writer.flush()?;
    if !prefix.is_empty() && !a.keep {
        let mut removed = 0usize;
        for b in &buckets {
            if let Ok(objects) = base.list_all(b, &prefix, usize::MAX).await {
                for chunk in objects.chunks(1000) {
                    let keys: Vec<(String, Option<String>)> =
                        chunk.iter().map(|o| (o.key.clone(), None)).collect();
                    if base.delete_objects(b, &keys, true).await.is_ok() {
                        removed += keys.len();
                    }
                }
            }
            if let Ok(up) = base
                .list_multipart_uploads(b, Some(&prefix), None, None)
                .await
            {
                for u in up.upload {
                    let _ = base.abort_multipart_upload(b, &u.key, &u.upload_id).await;
                }
            }
        }
        eprintln!("Removed {removed} objects under {prefix}.");
    }
    let replayed = Session::open(&path).map_err(|e| anyhow::anyhow!(e))?;
    let c = rp::compare(&original.records, &replayed.records, &done);
    match a.format {
        Format::Json => println!(
            "{}",
            output::json(&serde_json::json!({
                "plan": plan,
                "session": path,
                "comparison": c,
            }))
        ),
        _ => {
            print_comparison(&c, std::io::stdout().is_terminal());
            eprintln!(
                "{}",
                paint(&format!("session: {}", path.display()), "2", color)
            );
        }
    }
    Ok(if c.differ() > 0 { 2 } else { 0 })
}

fn opts_bucket(o: &sonde_proxy::replay::ReplayOptions, b: &str) -> String {
    o.buckets
        .iter()
        .find(|(from, _)| from == b)
        .map(|(_, to)| to.clone())
        .unwrap_or_else(|| b.to_string())
}

pub async fn diff(a: &DiffArgs) -> anyhow::Result<i32> {
    let original = Session::open(&a.original).map_err(|e| anyhow::anyhow!(e))?;
    let replay = Session::open(&a.replay).map_err(|e| anyhow::anyhow!(e))?;
    let c = sonde_proxy::replay::compare(&original.records, &replay.records, &[]);
    match a.format {
        Format::Json => println!("{}", output::json(&c)),
        _ => print_comparison(&c, std::io::stdout().is_terminal()),
    }
    Ok(if c.differ() > 0 { 2 } else { 0 })
}
