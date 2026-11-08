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
use sonde_proxy::profile::{Profile, RequestPrices, profile};
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
    /// Take requests from other machines too (any Host). Anyone who has the
    /// keys sonde prints can then use the endpoint's credentials.
    #[arg(long)]
    allow_remote: bool,
}

#[derive(Args)]
pub struct CheckArgs {
    /// The run to check (default: the newest session).
    file: Option<PathBuf>,
    /// A run to hold it to: --tolerance and --no-new-warnings compare with it.
    /// `auto` is the baseline set for the run's program (in the app, or with --set-baseline).
    #[arg(long)]
    baseline: Option<String>,
    /// Make this run its program's baseline, for later `--baseline auto` (and the app).
    #[arg(long)]
    set_baseline: bool,
    /// At most this many requests.
    #[arg(long)]
    max_requests: Option<usize>,
    /// At most this much time waiting on the store, in ms.
    #[arg(long)]
    max_waiting: Option<f64>,
    /// At most this request cost (in the settings' currency).
    #[arg(long)]
    max_cost: Option<f64>,
    /// At most this many failed requests.
    #[arg(long)]
    max_errors: Option<usize>,
    /// Against the baseline: requests, waiting and cost may grow this many percent.
    #[arg(long, default_value_t = 10.0)]
    tolerance: f64,
    /// Against the baseline: fail on a warning it did not have.
    #[arg(long)]
    no_new_warnings: bool,
    #[arg(long, short = 'f', value_enum, default_value_t)]
    format: Format,
}

#[derive(Args)]
pub struct CompareArgs {
    /// The run before a change.
    before: PathBuf,
    /// The run after it.
    after: PathBuf,
    #[arg(long, short = 'f', value_enum, default_value_t)]
    format: Format,
}

#[derive(Args)]
pub struct ShowArgs {
    /// A session file (default: the newest in the data directory's sessions/).
    file: Option<PathBuf>,
    /// Only the summary.
    #[arg(long)]
    summary: bool,
    /// Only what failed.
    #[arg(long)]
    errors: bool,
    /// One line per request instead of per operation.
    #[arg(long)]
    requests: bool,
    /// Open it in the sonde app instead (a sonde:// link).
    #[arg(long)]
    app: bool,
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
    allow_remote: bool,
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
            sonde_proxy::session::fresh_path(
                &dir,
                &format!("{}-{name}", chrono::Local::now().format("%Y%m%d-%H%M%S")),
            )
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
            allow_remote,
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

/// The request prices the settings give.
pub fn request_prices(s: &Sonde) -> RequestPrices {
    let p = s.settings().prices;
    RequestPrices {
        per_1000_writes: p.per_1000_writes,
        per_1000_reads: p.per_1000_reads,
        currency: p.currency,
    }
}

fn ms_text(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{:.1} s", ms / 1000.0)
    } else {
        format!("{ms:.0} ms")
    }
}

/// Money to two significant digits under a cent: a session's requests often
/// cost thousandths.
fn money_text(v: f64, currency: &str) -> String {
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

/// Where the time went: the kinds of request the program waited on most.
fn print_profile(p: &Profile, color: bool, say: &dyn Fn(String)) {
    if p.groups.is_empty() {
        return;
    }
    say(paint(
        &format!(
            "waiting on S3 {} of {} ({:.0}%) · up to {} in flight · request cost ≈ {}",
            ms_text(p.busy_ms),
            ms_text(p.wall_ms),
            p.busy_ms * 100.0 / p.wall_ms.max(1e-9),
            p.peak_in_flight,
            money_text(p.cost.amount, &p.cost.currency)
        ),
        "1",
        color,
    )
    .to_string());
    let rows: Vec<Vec<String>> = p
        .groups
        .iter()
        .take(8)
        .map(|g| {
            vec![
                if g.missed {
                    format!("{} (not found)", g.operation)
                } else {
                    g.operation.clone()
                },
                format!("{:.0}%", g.share * 100.0),
                ms_text(g.busy_ms),
                g.requests.to_string(),
                ms_text(g.p50_ms),
                ms_text(g.p95_ms),
            ]
        })
        .collect();
    say(output::table(
        &["waiting on", "share", "time", "requests", "p50", "p95"],
        &rows,
    )
    .to_string());
}

fn print_summary(
    records: &[Record],
    endpoint: &str,
    path: &Path,
    color: bool,
    prices: &RequestPrices,
    stdout: bool,
) {
    // `sonde show` answers on stdout; run and proxy keep stdout for the program's own output
    let say = |line: String| {
        if stdout {
            println!("{line}")
        } else {
            eprintln!("{line}")
        }
    };
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
    say(format!("\n{}", paint(&head, "1", color)));
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
        say(output::table(
            &["operation", "requests", "errors", "not found", "p50", "max"],
            &rows,
        )
        .to_string());
    }
    for (what, n) in &s.error_codes {
        say(format!("  {n} × {what}"));
    }
    print_profile(&profile(records, prices), color, &say);
    for o in observations(records, &eps) {
        let mark = if o.level == "warn" {
            paint("!", "33", color)
        } else {
            paint("·", "2", color)
        };
        say(format!("{mark} {}", o.text));
    }
    say(paint(&format!("session: {}", path.display()), "2", color).to_string());
}

pub async fn run(s: &Arc<Sonde>, _cli: &Cli, a: &RunArgs) -> anyhow::Result<i32> {
    let command = a.command.join(" ");
    let started = start(
        s,
        &a.rec,
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Some(command),
        false,
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
        &request_prices(s),
        false,
    );
    if session.records.is_empty() {
        eprintln!("{}", NO_REQUESTS);
    }
    // finished: compressed to a tenth or less, still a .sonde file
    let file = started.path.clone();
    drop(started);
    let _ = sonde_proxy::session::compress(&file);
    Ok(status.code().unwrap_or(1))
}

/// What to try when a program sent sonde nothing: most SDKs read
/// AWS_ENDPOINT_URL, some need the endpoint said their own way.
const NO_REQUESTS: &str = "\nThe program sent sonde no requests. sonde set AWS_ENDPOINT_URL, which current SDKs read; some need it said their own way:
  boto3 / botocore    1.28 or newer, or boto3.client(\"s3\", endpoint_url=os.environ[\"AWS_ENDPOINT_URL\"])
  AWS CLI             v2.13 or newer, or --endpoint-url \"$AWS_ENDPOINT_URL\"
  Go SDK v2           1.20 or newer (aws-sdk-go v1 does not read it)
  Java SDK v2         2.21 or newer; Hadoop s3a: -Dfs.s3a.endpoint=$AWS_ENDPOINT_URL -Dfs.s3a.path.style.access=true
  DuckDB              SET s3_endpoint='127.0.0.1:PORT'; SET s3_url_style='path'; SET s3_use_ssl=false
  Node (@aws-sdk/*)   3.4xx or newer, or new S3Client({ endpoint: process.env.AWS_ENDPOINT_URL, forcePathStyle: true })
Credentials must be the ones sonde set (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY): a profile or a role bypasses them.";

fn check_listen(a: &ProxyArgs) -> anyhow::Result<()> {
    if !a.listen.ip().is_loopback() && !a.allow_remote {
        anyhow::bail!(
            "{} is reachable from other machines: add --allow-remote if that is what you want (anyone with the keys sonde prints can then use the endpoint's credentials)",
            a.listen
        );
    }
    if a.allow_remote {
        eprintln!(
            "warning: sonde takes requests from other machines on {}",
            a.listen
        );
    }
    Ok(())
}

fn print_env(proxy: &Proxy) {
    let (set, unset) = proxy.env();
    eprintln!(
        "sonde is listening on {}. Point the application at it:\n",
        proxy.url()
    );
    for k in &unset {
        eprintln!("  unset {k}");
    }
    for (k, v) in &set {
        eprintln!("  export {k}={v}");
    }
    eprintln!("\nCtrl-C stops.\n");
}

pub async fn proxy(s: &Arc<Sonde>, _cli: &Cli, a: &ProxyArgs) -> anyhow::Result<i32> {
    check_listen(a)?;
    let started = start(s, &a.rec, a.listen, None, a.allow_remote).await?;
    print_env(&started.proxy);
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
        &request_prices(s),
        false,
    );
    // finished: compressed to a tenth or less, still a .sonde file
    let file = started.path.clone();
    drop(started);
    let _ = sonde_proxy::session::compress(&file);
    Ok(0)
}

/// The newest session file in the data directory.
pub fn newest_session(s: &Sonde) -> anyhow::Result<PathBuf> {
    let dir = s.paths().sessions();
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("sonde") {
            continue;
        }
        let t = e
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(n, _)| t > *n) {
            newest = Some((t, p));
        }
    }
    newest.map(|(_, p)| p).ok_or_else(|| {
        anyhow::anyhow!(
            "no sessions in {} yet: record one with `sonde run -- <command>`",
            dir.display()
        )
    })
}

pub async fn show(s: &Arc<Sonde>, a: &ShowArgs) -> anyhow::Result<i32> {
    let file = match &a.file {
        Some(f) => super::sessions::resolve(s, f),
        None => newest_session(s)?,
    };
    let session = Session::open(&file).map_err(|e| anyhow::anyhow!(e))?;
    if a.app {
        return open_in_app(&file);
    }
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
                &file,
                std::io::stdout().is_terminal(),
                &request_prices(s),
                true,
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
        match &a.prefix {
            Some(p) => rp::normalize_prefix(p).map_err(|e| anyhow::anyhow!(e))?,
            None => rp::default_prefix(),
        }
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
            sonde_proxy::session::fresh_path(
                &dir,
                &format!("{}-replay", chrono::Local::now().format("%Y%m%d-%H%M%S")),
            )
        }
    };
    let show = color;
    let run = rp::replay_to_file(&base, &original, &opts, &path, a.keep, |i, n| {
        if show {
            eprint!("\r\x1b[2K{i}/{n}");
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?;
    let _ = sonde_proxy::session::compress(&path);
    if show {
        eprint!("\r\x1b[2K");
    }
    if !prefix.is_empty() && !a.keep {
        eprintln!("Removed {} objects under {prefix}.", run.removed);
    }
    let c = run.comparison;
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
            if let Ok(replayed) = Session::open(&path) {
                print_latency(
                    &original.records,
                    &replayed.records,
                    (&original.header.endpoint, &replayed.header.endpoint),
                    &request_prices(s),
                );
            }
            eprintln!(
                "{}",
                paint(&format!("session: {}", path.display()), "2", color)
            );
        }
    }
    Ok(if c.differ() > 0 { 2 } else { 0 })
}

/// The same workload on two stores: each kind's p50 and p95 on each, slowest change first.
fn print_latency(a: &[Record], b: &[Record], names: (&str, &str), prices: &RequestPrices) {
    let changes = sonde_proxy::profile::latency_changes(&profile(a, prices), &profile(b, prices));
    if changes.is_empty() {
        return;
    }
    let rows: Vec<Vec<String>> = changes
        .iter()
        .take(10)
        .map(|c| {
            let ratio = c.p95_ms.1.max(0.01) / c.p95_ms.0.max(0.01);
            vec![
                if c.missed {
                    format!("{} (not found)", c.operation)
                } else {
                    c.operation.clone()
                },
                format!("{} → {}", ms_text(c.p50_ms.0), ms_text(c.p50_ms.1)),
                format!("{} → {}", ms_text(c.p95_ms.0), ms_text(c.p95_ms.1)),
                format!("{ratio:.1}×"),
            ]
        })
        .collect();
    println!(
        "\nlatency, {} → {}\n{}",
        names.0,
        names.1,
        output::table(&["kind", "p50", "p95", "p95 ×"], &rows)
    );
}

fn opts_bucket(o: &sonde_proxy::replay::ReplayOptions, b: &str) -> String {
    o.buckets
        .iter()
        .find(|(from, _)| from == b)
        .map(|(_, to)| to.clone())
        .unwrap_or_else(|| b.to_string())
}

pub async fn diff(s: &Arc<Sonde>, a: &DiffArgs) -> anyhow::Result<i32> {
    let original = Session::open(&a.original).map_err(|e| anyhow::anyhow!(e))?;
    let replay = Session::open(&a.replay).map_err(|e| anyhow::anyhow!(e))?;
    let c = sonde_proxy::replay::compare(&original.records, &replay.records, &[]);
    let prices = request_prices(s);
    match a.format {
        Format::Json => println!(
            "{}",
            output::json(&serde_json::json!({
                "comparison": c,
                "latency": sonde_proxy::profile::latency_changes(&profile(&original.records, &prices), &profile(&replay.records, &prices)),
            }))
        ),
        _ => {
            print_comparison(&c, std::io::stdout().is_terminal());
            print_latency(
                &original.records,
                &replay.records,
                (&original.header.endpoint, &replay.header.endpoint),
                &prices,
            );
        }
    }
    Ok(if c.differ() > 0 { 2 } else { 0 })
}

fn change(a: f64, b: f64, text: impl Fn(f64) -> String) -> String {
    let pct = if a > 0.0 {
        format!(" ({:+.0}%)", (b - a) * 100.0 / a)
    } else {
        String::new()
    };
    format!("{} → {}{pct}", text(a), text(b))
}

/// `sonde compare`: two runs of a program, before and after a change.
pub async fn compare(s: &Arc<Sonde>, a: &CompareArgs) -> anyhow::Result<i32> {
    let open = |p: &PathBuf| Session::open(p).map_err(|e| anyhow::anyhow!(e));
    let (sa, sb) = (open(&a.before)?, open(&a.after)?);
    let prices = request_prices(s);
    let (pa, pb) = (profile(&sa.records, &prices), profile(&sb.records, &prices));
    let (oa, ob) = (
        observations(&sa.records, &episodes(&sa.records)),
        observations(&sb.records, &episodes(&sb.records)),
    );
    let c = sonde_proxy::profile::compare_runs((&sa.records, &pa, &oa), (&sb.records, &pb, &ob));
    if matches!(a.format, Format::Json) {
        println!("{}", output::json(&c));
        return Ok(0);
    }
    let color = std::io::stdout().is_terminal();
    let n = |v: f64| format!("{v:.0}");
    println!("{}", paint("before → after", "1", color));
    println!(
        "  requests   {}",
        change(c.requests.0 as f64, c.requests.1 as f64, n)
    );
    println!("  waiting    {}", change(c.busy_ms.0, c.busy_ms.1, ms_text));
    println!("  took       {}", change(c.wall_ms.0, c.wall_ms.1, ms_text));
    println!(
        "  errors     {}",
        change(c.errors.0 as f64, c.errors.1 as f64, n)
    );
    println!(
        "  cost ≈     {}",
        change(c.cost.0, c.cost.1, |v| money_text(v, &c.currency))
    );
    let rows: Vec<Vec<String>> = c
        .groups
        .iter()
        .filter(|g| g.requests.0 != g.requests.1 || (g.busy_ms.0 - g.busy_ms.1).abs() >= 1.0)
        .take(10)
        .map(|g| {
            vec![
                if g.missed {
                    format!("{} (not found)", g.operation)
                } else {
                    g.operation.clone()
                },
                format!("{} → {}", g.requests.0, g.requests.1),
                format!("{} → {}", ms_text(g.busy_ms.0), ms_text(g.busy_ms.1)),
            ]
        })
        .collect();
    if !rows.is_empty() {
        println!("{}", output::table(&["kind", "requests", "waiting"], &rows));
    }
    for f in &c.resolved {
        println!("{} {}", paint("✓", "32", color), f.text);
    }
    for f in &c.appeared {
        println!("{} {}", paint("!", "33", color), f.text);
    }
    Ok(0)
}

/// `sonde check`: a run against a budget and a baseline run; exit 1 when it is past either.
pub async fn check(s: &Arc<Sonde>, a: &CheckArgs) -> anyhow::Result<i32> {
    use sonde_proxy::program::{Budget, check, run_point};
    let file = match &a.file {
        Some(f) => super::sessions::resolve(s, f),
        None => newest_session(s)?,
    };
    let prices = request_prices(s);
    let read = |p: &PathBuf| -> anyhow::Result<_> {
        let session = Session::open(p).map_err(|e| anyhow::anyhow!(e))?;
        let pr = profile(&session.records, &prices);
        let ob = observations(&session.records, &episodes(&session.records));
        let point = run_point(
            &p.to_string_lossy(),
            &session.header.created.to_rfc3339(),
            &session.records,
            &pr,
            &ob,
        );
        Ok((point, session.records, pr, ob))
    };
    let run = read(&file)?;
    let header = sonde_proxy::session::Session::open(&file)
        .map_err(|e| anyhow::anyhow!(e))?
        .header;
    let program = sonde_proxy::program::program_key(header.command.as_deref());
    let dir = s.paths().sessions();
    if a.set_baseline {
        let Some(key) = &program else {
            eprintln!("this session ran no command, so it is no program's baseline");
            return Ok(64);
        };
        // compared whole: `x.sonde` given from inside the directory has an empty parent
        let parent = std::path::absolute(&file)
            .ok()
            .and_then(|f| f.parent().and_then(|p| p.canonicalize().ok()));
        if parent.is_none() || parent != dir.canonicalize().ok() {
            eprintln!(
                "a baseline is a session in {}: record it there (no -o), or pass --baseline FILE",
                dir.display()
            );
            return Ok(64);
        }
        let mut all = sonde_proxy::program::read_baselines(&dir);
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        all.insert(key.clone(), name);
        sonde_proxy::program::write_baselines(&dir, &all)?;
        eprintln!("baseline for `{key}`: {}", file.display());
    }
    let base_path = match a.baseline.as_deref() {
        None => None,
        Some("auto") => {
            let found = program.as_ref().and_then(|k| {
                sonde_proxy::program::read_baselines(&dir)
                    .get(k)
                    .map(|n| dir.join(n))
                    .filter(|p| p.is_file())
            });
            if found.is_none() {
                eprintln!(
                    "no baseline set for this program: set one with `sonde check FILE --set-baseline` or in the app"
                );
                return Ok(64);
            }
            found
        }
        Some(p) => Some(super::sessions::resolve(s, &PathBuf::from(p))),
    };
    let base = base_path.as_ref().map(read).transpose()?;
    if let Some(b) = &base_path {
        let bh = Session::open(b).map_err(|e| anyhow::anyhow!(e))?.header;
        let bp = sonde_proxy::program::program_key(bh.command.as_deref());
        if bp != program {
            eprintln!(
                "note: the baseline ran `{}`, this run `{}`",
                bp.as_deref().unwrap_or("no command"),
                program.as_deref().unwrap_or("no command")
            );
        }
    }
    let budget = Budget {
        max_requests: a.max_requests,
        max_waiting_ms: a.max_waiting,
        max_cost: a.max_cost,
        max_errors: a.max_errors,
        tolerance: base.as_ref().map(|_| a.tolerance / 100.0),
        no_new_warnings: a.no_new_warnings,
    };
    let c = check(
        (&run.0, &run.1, &run.2, &run.3),
        base.as_ref().map(|b| (&b.0, &b.1[..], &b.2, &b.3[..])),
        &budget,
    );
    // setting a baseline alone is a whole command
    if c.verdicts.is_empty() && a.set_baseline {
        return Ok(0);
    }
    // nothing to hold the run to is a mistake in the command, in every format
    if c.verdicts.is_empty() {
        eprintln!(
            "nothing to check: give a budget (--max-requests, --max-waiting, --max-cost, --max-errors) or a --baseline"
        );
        return Ok(64);
    }
    if matches!(a.format, Format::Json) {
        println!(
            "{}",
            output::json(&serde_json::json!({ "run": run.0, "check": c }))
        );
        return Ok(if c.passed { 0 } else { 1 });
    }
    let color = std::io::stdout().is_terminal();
    let shown = |what: &str, v: f64| match what {
        "waiting" => ms_text(v),
        "cost" => money_text(v, &run.0.currency),
        _ => format!("{v:.0}"),
    };
    let rows: Vec<Vec<String>> = c
        .verdicts
        .iter()
        .map(|v| {
            vec![
                if v.ok {
                    paint("pass", "32", color)
                } else {
                    paint("FAIL", "31", color)
                },
                v.what.to_string(),
                shown(v.what, v.value),
                format!("≤ {}", shown(v.what, v.limit)),
                v.against.to_string(),
            ]
        })
        .collect();
    println!(
        "{}",
        output::table(&["", "", "run", "limit", "from"], &rows)
    );
    for w in &c.new_warnings {
        println!("{} {w}", paint("!", "33", color));
    }
    println!(
        "{}",
        paint(
            if c.passed {
                "within budget"
            } else {
                "over budget"
            },
            if c.passed { "32" } else { "31" },
            color
        )
    );
    Ok(if c.passed { 0 } else { 1 })
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

#[derive(Args)]
pub struct DemoArgs {
    /// Open the recorded session in the sonde app afterwards.
    #[arg(long)]
    app: bool,
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
            app: false,
            format: Format::Table,
        },
    )
    .await
}

/// A tour in one command: a small program's requests to the built-in demo
/// store, recorded as any program's would be, then shown with its findings.
pub async fn demo(s: &Arc<Sonde>, a: &DemoArgs) -> anyhow::Result<i32> {
    use s3raw::{Body, ClientConfig, Credentials, S3Client, S3Request, StaticProvider};
    let ep = sonde_core::demo::install(s)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let rec = Recording {
        endpoint: Some(ep.id.clone()),
        no_delete: false,
        read_only: false,
        out: None,
    };
    let started = start(
        s,
        &rec,
        "127.0.0.1:0".parse()?,
        Some("sonde demo".into()),
        false,
    )
    .await?;
    let (key, secret) = started.proxy.client_keys();
    let app = S3Client::new(
        ClientConfig::new("demo-app", &started.proxy.url(), "us-east-1")?,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        RequestLog::default(),
    )?;
    let send = |r: S3Request| {
        let app = &app;
        async move {
            let _ = app.send_raw(r).await;
        }
    };
    let get = |k: &str| {
        S3Request::new("GetObject", http::Method::GET)
            .bucket("demo")
            .key(k)
    };
    send(
        S3Request::new("PutObject", http::Method::PUT)
            .bucket("demo")
            .key("tour/config.json")
            .body(Body::from(r#"{"format":"csv"}"#)),
    )
    .await;
    // what a small ETL job does: write its parts one at a time
    for i in 0..12 {
        send(
            S3Request::new("PutObject", http::Method::PUT)
                .bucket("demo")
                .key(&format!("tour/part-{i:02}.csv"))
                .body(Body::from(format!("id,value\n{i},{}\n", i * 7))),
        )
        .await;
    }
    // look before reading each, one after another
    for i in 0..12 {
        let k = format!("tour/part-{i:02}.csv");
        send(
            S3Request::new("HeadObject", http::Method::HEAD)
                .bucket("demo")
                .key(&k),
        )
        .await;
        send(get(&k)).await;
    }
    // read the same configuration for every part, and list the folder again and again
    for _ in 0..12 {
        send(get("tour/config.json")).await;
    }
    for _ in 0..4 {
        send(
            S3Request::new("ListObjectsV2", http::Method::GET)
                .bucket("demo")
                .query("list-type", "2")
                .query("prefix", "tour/"),
        )
        .await;
    }
    let file = started.path.clone();
    drop(app);
    drop(started);
    let _ = sonde_proxy::session::compress(&file);
    eprintln!(
        "recorded the demo program in {}\n  sonde show {}    the session again\n  sonde run -- <your program>    your own program's requests\n",
        file.display(),
        file.display()
    );
    show(
        s,
        &ShowArgs {
            file: Some(file),
            summary: true,
            errors: false,
            requests: false,
            app: a.app,
            format: Format::Table,
        },
    )
    .await?;
    Ok(0)
}

#[derive(Args)]
pub struct WatchArgs {
    #[command(flatten)]
    proxy: ProxyArgs,
    /// Start a new session file every this many minutes (the finished one is compressed).
    #[arg(long, default_value_t = 60)]
    rotate: u64,
}

/// A proxy left running: one status line for the last minute, each new
/// warning as it appears, and a new session file every --rotate minutes.
pub async fn watch(s: &Arc<Sonde>, a: &WatchArgs) -> anyhow::Result<i32> {
    use std::collections::{HashSet, VecDeque};
    use std::time::{Duration, Instant};
    check_listen(&a.proxy)?;
    let started = start(s, &a.proxy.rec, a.proxy.listen, None, a.proxy.allow_remote).await?;
    print_env(&started.proxy);
    let header = sonde_proxy::session::read_header(&started.path)
        .ok_or_else(|| anyhow::anyhow!("cannot read {}", started.path.display()))?;
    let tty = std::io::stderr().is_terminal();
    let dir = started
        .path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let mut rx = started.log.subscribe();
    let mut writer = started.writer.clone();
    let mut path = started.path.clone();
    let mut file_records: Vec<Record> = Vec::new();
    let mut minute: VecDeque<(Instant, Record)> = VecDeque::new();
    let mut warned: HashSet<String> = HashSet::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut rotated = Instant::now();
    let mut seconds = 0u64;
    let finish = |w: Arc<SessionWriter>, p: &Path| {
        let _ = w.flush();
        drop(w);
        let _ = sonde_proxy::session::compress(p);
    };
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            got = rx.recv() => match got {
                Ok(id) => {
                    if let Some(r) = started.log.get(id) {
                        minute.push_back((Instant::now(), r.clone()));
                        file_records.push(r);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            },
            _ = tick.tick() => {
                seconds += 1;
                while minute.front().is_some_and(|(t, _)| t.elapsed() > Duration::from_secs(60)) {
                    minute.pop_front();
                }
                let n = minute.len();
                let errors = minute.iter().filter(|(_, r)| r.is_error() && !sonde_proxy::summary::is_miss(r)).count();
                let mut lat: Vec<f64> = minute.iter().map(|(_, r)| r.latency_ms).collect();
                lat.sort_by(f64::total_cmp);
                let p95 = lat.get(((lat.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)).copied().unwrap_or(0.0);
                let line = format!(
                    "last minute: {n} requests ({:.1}/s) · {errors} errors · p95 {} · {} in this file",
                    n as f64 / 60.0,
                    ms_text(p95),
                    file_records.len()
                );
                if tty {
                    eprint!("\r\x1b[2K{line}");
                } else if seconds.is_multiple_of(30) {
                    eprintln!("{line}");
                }
                // what this file's requests say, every five seconds: each warning once
                if seconds.is_multiple_of(5) && !file_records.is_empty() {
                    let recent = &file_records[file_records.len().saturating_sub(50_000)..];
                    for o in observations(recent, &episodes(recent)) {
                        if o.level == "warn" && warned.insert(sonde_proxy::profile::finding_identity(&o)) {
                            if tty {
                                eprint!("\r\x1b[2K");
                            }
                            eprintln!("{} {}", paint("!", "33", tty), o.text);
                        }
                    }
                }
                if rotated.elapsed() >= Duration::from_secs(a.rotate.max(1) * 60) {
                    let next = sonde_proxy::session::fresh_path(
                        &dir,
                        &format!("{}-watch", chrono::Local::now().format("%Y%m%d-%H%M%S")),
                    );
                    let mut h = header.clone();
                    h.created = chrono::Utc::now();
                    let w = Arc::new(SessionWriter::create(&next, &h)?);
                    started.log.set_sink(Some(w.clone()));
                    let old = std::mem::replace(&mut writer, w);
                    let old_path = std::mem::replace(&mut path, next);
                    finish(old, &old_path);
                    if tty {
                        eprint!("\r\x1b[2K");
                    }
                    eprintln!("{} {}", paint("·", "2", tty), old_path.display());
                    file_records.clear();
                    warned.clear();
                    rotated = Instant::now();
                }
            }
        }
    }
    if tty {
        eprintln!();
    }
    started.log.set_sink(None);
    let Started { proxy, .. } = started;
    drop(proxy);
    finish(writer, &path);
    eprintln!("{}", path.display());
    Ok(0)
}
