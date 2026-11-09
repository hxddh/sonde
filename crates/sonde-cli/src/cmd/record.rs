//! Recording: `sonde run -- <command>`, `sonde proxy`, `sonde show <session>`.

use sonde_proxy::callers::code_place;
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
    /// Name the place in a program's code that sent each request (Python's
    /// boto3, the AWS SDK for JavaScript v3, the AWS SDK for Java 2):
    /// findings then say where to change it. Also the `name-callers` setting.
    #[arg(long)]
    callers: bool,
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
    /// The project's budget file (default: `sonde.toml` here or in a folder
    /// above): its `[check]` gives the limits and the baseline the flags
    /// leave out.
    #[arg(long)]
    config: Option<PathBuf>,
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
    /// 95% of the requests within this many ms.
    #[arg(long)]
    max_p95: Option<f64>,
    /// Against the baseline: requests, waiting and cost may grow this many
    /// percent (10 unless sonde.toml says).
    #[arg(long)]
    tolerance: Option<f64>,
    /// Against the baseline: fail on a warning it did not have.
    #[arg(long)]
    no_new_warnings: bool,
    /// Write `sonde.toml` here with a `[check]` that this run passes: its
    /// figures with --tolerance (10%) of room, and its program's baseline.
    #[arg(long)]
    init: bool,
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
                .and_then(sonde_proxy::program::program_name)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "proxy".into());
            sonde_proxy::session::fresh_path(
                &dir,
                &format!("{}-{name}", chrono::Local::now().format("%Y%m%d-%H%M%S")),
            )
        }
    };
    let ran = command.is_some();
    let mut header = SessionHeader::new(&ep, cfg.endpoint.as_str(), &cfg.region, command);
    if ran && let Ok(dir) = std::env::current_dir() {
        header = header.ran_in(&dir);
    }
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

/// The request prices of a session on `endpoint`: the ones the app shows.
pub fn request_prices_for(s: &Sonde, endpoint: &str) -> RequestPrices {
    let (w, r, l, currency) = s.request_prices_for(endpoint);
    RequestPrices {
        per_1000_writes: w,
        per_1000_reads: r,
        per_1000_lists: l,
        currency,
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
                ms_text(g.waited_ms),
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
    let prof = profile(records, prices);
    print_profile(&prof, color, &say);
    let obs = observations(records, &eps);
    for o in &obs {
        let mark = if o.level == "warn" {
            paint("!", "33", color)
        } else {
            paint("·", "2", color)
        };
        say(format!("{mark} {}", o.text));
        // where in the program, when it says
        for c in &o.callers {
            say(paint(
                &format!("    at {} ({} requests)", c.place, c.requests),
                "2",
                color,
            ));
        }
    }
    // acting on all of them, at once
    if let Some(w) = sonde_proxy::fixes::what_if(records, &prof, &obs) {
        say(paint(
            &format!(
                "acting on {} finding{}: {} → {} requests · waiting {} → {} · cost {} → {}",
                w.findings,
                if w.findings == 1 { "" } else { "s" },
                w.requests.0,
                w.requests.1,
                ms_text(w.waiting_ms.0),
                ms_text(w.waiting_ms.1),
                money_text(w.cost.0, &w.currency),
                money_text(w.cost.1, &w.currency),
            ),
            "32",
            color,
        ));
    }
    say(paint(&format!("session: {}", path.display()), "2", color).to_string());
}

pub async fn run(s: &Arc<Sonde>, _cli: &Cli, a: &RunArgs) -> anyhow::Result<i32> {
    let command = sonde_proxy::program::shell_join(&a.command);
    let started = start(
        s,
        &a.rec,
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Some(command),
        false,
    )
    .await?;
    let (mut set, unset) = started.proxy.env();
    if a.callers || s.settings().name_callers {
        set.extend(sonde_proxy::callers::caller_env(&s.paths().python_hook())?);
    }
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
        &request_prices_for(s, &session.header.endpoint),
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
        &request_prices_for(s, &session.header.endpoint),
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
                    "whatIf": sonde_proxy::fixes::what_if(all, &profile(all, &request_prices_for(s, &session.header.endpoint)), &observations(all, &eps)),
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
                &request_prices_for(s, &session.header.endpoint),
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
    /// Several (`-e a -e b`) replay on each at once and compare them side by side.
    #[arg(long, short = 'e')]
    endpoint: Vec<String>,
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
    if a.endpoint.len() > 1 {
        return replay_many(s, a).await;
    }
    let original = Session::open(&a.file).map_err(|e| anyhow::anyhow!(e))?;
    let ep = super::endpoint(s, a.endpoint.first().map(String::as_str))?;
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
                    &request_prices_for(s, &original.header.endpoint),
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

/// One session replayed on several stores at once: each answers under its
/// own prefix (cleaned up after), and the stores are compared side by side:
/// how many answers differ from the recording, and each kind's p95.
async fn replay_many(s: &Arc<Sonde>, a: &ReplayArgs) -> anyhow::Result<i32> {
    use sonde_proxy::replay::{self as rp, ReplayOptions};
    let original = Session::open(&a.file).map_err(|e| anyhow::anyhow!(e))?;
    if a.in_place {
        anyhow::bail!("--in-place replays on one endpoint at a time");
    }
    let prefix = match &a.prefix {
        Some(p) => rp::normalize_prefix(p).map_err(|e| anyhow::anyhow!(e))?,
        None => rp::default_prefix(),
    };
    let opts = ReplayOptions {
        prefix: prefix.clone(),
        buckets: a.bucket.clone(),
    };
    let plan = rp::plan(&original.records, &opts);
    let mut targets = Vec::new();
    for e in &a.endpoint {
        let id = super::endpoint(s, Some(e))?;
        targets.push((id.clone(), s.client(&id)?));
    }
    eprintln!(
        "Replay {} requests from {} on {} at once, under {prefix} on each ({} writes, {} deletes; deleted afterwards unless --keep).",
        plan.requests,
        a.file.display(),
        a.endpoint.join(", "),
        plan.writes,
        plan.deletes
    );
    if !a.yes {
        if !std::io::stdin().is_terminal() {
            eprintln!(
                "sonde: replay writes to {}: pass --yes to confirm",
                a.endpoint.join(", ")
            );
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
    let dir = s.paths().sessions();
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let runs = futures::future::join_all(targets.iter().map(|(id, client)| {
        let path = sonde_proxy::session::fresh_path(&dir, &format!("{stamp}-replay-{id}"));
        let (original, opts) = (&original, &opts);
        async move {
            let run = rp::replay_to_file(client, original, opts, &path, a.keep, |_, _| {}).await;
            let _ = sonde_proxy::session::compress(&path);
            (id.clone(), path, run)
        }
    }))
    .await;
    let prices = request_prices_for(s, &original.header.endpoint);
    let before = profile(&original.records, &prices);
    let mut columns = vec![original.header.endpoint.clone()];
    let mut stores = Vec::new();
    let mut differ_total = 0;
    for (id, path, run) in runs {
        let run = match run {
            Ok(r) => r,
            Err(e) => {
                eprintln!("sonde: {id}: {e}");
                continue;
            }
        };
        differ_total += run.comparison.differ();
        let replayed = Session::open(&path).map_err(|e| anyhow::anyhow!(e))?;
        columns.push(id.clone());
        stores.push((
            id,
            path,
            run.comparison,
            profile(&replayed.records, &prices),
        ));
    }
    let kinds: Vec<(String, bool)> = before
        .groups
        .iter()
        .map(|g| (g.operation.clone(), g.missed))
        .collect();
    let p95 = |p: &Profile, op: &str, missed: bool| {
        p.groups
            .iter()
            .find(|g| g.operation == op && g.missed == missed)
            .map(|g| g.p95_ms)
    };
    if matches!(a.format, Format::Json) {
        let doc = serde_json::json!({
            "plan": plan,
            "stores": stores.iter().map(|(id, path, c, p)| serde_json::json!({
                "endpoint": id,
                "session": path,
                "comparison": c,
                "latency": sonde_proxy::profile::latency_changes(&before, p),
            })).collect::<Vec<_>>(),
        });
        println!("{}", output::json(&doc));
    } else {
        let mut rows: Vec<Vec<String>> = vec![{
            let mut r = vec!["answers that differ".to_string(), "—".to_string()];
            r.extend(
                stores
                    .iter()
                    .map(|(_, _, c, _)| format!("{} of {}", c.differ(), c.compared)),
            );
            r
        }];
        for (op, missed) in &kinds {
            let name = if *missed {
                format!("{op} (not found)")
            } else {
                op.clone()
            };
            let mut r = vec![
                format!("{name} p95"),
                p95(&before, op, *missed).map(ms_text).unwrap_or_default(),
            ];
            r.extend(
                stores
                    .iter()
                    .map(|(_, _, _, p)| p95(p, op, *missed).map(ms_text).unwrap_or_default()),
            );
            rows.push(r);
        }
        let mut headers = vec![""];
        headers.extend(columns.iter().map(String::as_str));
        output::print(a.format, &headers, &rows);
        for (id, path, _, _) in &stores {
            eprintln!("{id}: {}", path.display());
        }
    }
    Ok(if differ_total > 0 { 2 } else { 0 })
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
    let prices = request_prices_for(s, &original.header.endpoint);
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
    let (pa, pb) = (
        profile(&sa.records, &request_prices_for(s, &sa.header.endpoint)),
        profile(&sb.records, &request_prices_for(s, &sb.header.endpoint)),
    );
    let (oa, ob) = (
        observations(&sa.records, &episodes(&sa.records)),
        observations(&sb.records, &episodes(&sb.records)),
    );
    let c = sonde_proxy::profile::compare_runs((&sa.records, &pa, &oa), (&sb.records, &pb, &ob))
        .of_programs(sa.header.command.as_deref(), sb.header.command.as_deref());
    if matches!(a.format, Format::Json) {
        println!("{}", output::json(&c));
        return Ok(0);
    }
    let color = std::io::stdout().is_terminal();
    let n = |v: f64| format!("{v:.0}");
    if !c.same_program {
        println!(
            "{}",
            paint(
                "two different programs: their figures compare, their findings do not",
                "33",
                color
            )
        );
    }
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
/// A project's budget for `sonde check`: the `[check]` table of its
/// `sonde.toml`, kept with the code it is about (`max-waiting` in ms, or
/// "2s"; `baseline` a session, `auto`, or an https URL of one).
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ProjectCheck {
    max_requests: Option<usize>,
    max_waiting: Option<toml::Value>,
    max_cost: Option<f64>,
    max_errors: Option<usize>,
    max_p95: Option<toml::Value>,
    /// Findings kept on purpose: never a new warning.
    #[serde(default)]
    expect: Vec<String>,
    tolerance: Option<f64>,
    #[serde(default)]
    no_new_warnings: bool,
    baseline: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ProjectFile {
    #[serde(default)]
    check: ProjectCheck,
}

/// `sonde.toml` here or in a folder above (to the repository's root).
fn find_project_file() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let f = dir.join("sonde.toml");
        if f.is_file() {
            return Some(f);
        }
        if dir.join(".git").exists() || !dir.pop() {
            return None;
        }
    }
}

/// `max-waiting` as `sonde.toml` writes it.
fn waiting_ms(v: &toml::Value) -> anyhow::Result<f64> {
    duration_ms("max-waiting", v)
}

/// A duration as `sonde.toml` writes it: ms as a number, or "250ms", "2s", "1.5 s".
fn duration_ms(name: &str, v: &toml::Value) -> anyhow::Result<f64> {
    match v {
        toml::Value::Integer(n) => Ok(*n as f64),
        toml::Value::Float(f) => Ok(*f),
        toml::Value::String(s) => {
            let t = s.trim();
            let (n, scale) = if let Some(n) = t.strip_suffix("ms") {
                (n, 1.0)
            } else if let Some(n) = t.strip_suffix('s') {
                (n, 1000.0)
            } else {
                (t, 1.0)
            };
            Ok(n.trim()
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("{name}: {s:?} is not a duration"))?
                * scale)
        }
        other => anyhow::bail!("{name}: {other} is not a duration"),
    }
}

/// `sonde check --init`: a `sonde.toml` whose `[check]` this run passes,
/// with room to grow, and the CI step that holds every run to it.
fn init_project(a: &CheckArgs, run: &sonde_proxy::program::RunPoint) -> anyhow::Result<i32> {
    let path = a
        .config
        .clone()
        .unwrap_or_else(|| PathBuf::from("sonde.toml"));
    if path.exists() {
        eprintln!(
            "{} exists: edit its [check], or remove it first",
            path.display()
        );
        return Ok(64);
    }
    let tolerance = a.tolerance.unwrap_or(10.0);
    let grow = 1.0 + tolerance / 100.0;
    // whole figures a person reads and edits
    let waiting = (run.busy_ms * grow + 5.0).ceil();
    let cost = if run.cost > 0.0 {
        let c = run.cost * grow;
        let digits = (-c.log10().floor() as i32 + 1).clamp(2, 8);
        // up, never down: the run it was made from passes it
        let scale = 10f64.powi(digits);
        format!("{:.*}", digits as usize, (c * scale).ceil() / scale)
    } else {
        "0.0001".into()
    };
    let text = [
        "# sonde check: what a run of this program may use against S3.".to_string(),
        format!("# Made from a run of {} requests; edit freely.", run.requests),
        "[check]".into(),
        format!("max-requests = {}", (run.requests as f64 * grow).ceil() as u64),
        format!("max-waiting = \"{waiting}ms\""),
        format!("max-cost = {cost}"),
        format!("max-errors = {}", run.errors),
        "# against a baseline run: how much more it may use, in percent".into(),
        format!("tolerance = {tolerance}"),
        "# baseline = \"auto\"   # the baseline set for the program (in the app, or --set-baseline)".into(),
        "# baseline = \"https://…/main.sonde\"   # or the main branch's run, kept by CI".into(),
        String::new(),
    ]
    .join("\n");
    std::fs::write(&path, text)?;
    eprintln!("wrote {}", path.display());
    eprintln!(
        "{}",
        [
            "",
            "In CI (GitHub Actions), run the program through sonde and check it:",
            "",
            "      - run: sonde run -- <your command>",
            "      - run: sonde check -f github",
            "",
            "For a comment on the pull request, `sonde check -f markdown`.",
        ]
        .join("\n")
    );
    Ok(0)
}

/// The runs of `key` up to `upto` (oldest first, 30 at most), each with
/// the commit it recorded: what [`sonde_proxy::program::since`] walks.
fn program_runs(
    s: &Sonde,
    key: &str,
    upto: &Path,
) -> Vec<(
    sonde_proxy::program::RunPoint,
    Option<sonde_proxy::session::GitStamp>,
)> {
    let dir = s.paths().sessions();
    let upto_created = sonde_proxy::session::read_header(upto).map(|h| h.created);
    let mut found: Vec<(chrono::DateTime<chrono::Utc>, PathBuf)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sonde"))
        .filter_map(|p| {
            let h = sonde_proxy::session::read_header(&p)?;
            (sonde_proxy::program::program_key(h.command.as_deref()).as_deref() == Some(key)
                && upto_created.is_none_or(|u| h.created <= u))
            .then_some((h.created, p))
        })
        .collect();
    found.sort();
    // the run checked is the newest, wherever its file is
    found.retain(|(_, p)| p.canonicalize().ok() != upto.canonicalize().ok());
    found.push((upto_created.unwrap_or_default(), upto.to_path_buf()));
    let from = found.len().saturating_sub(30);
    found[from..]
        .iter()
        .filter_map(|(_, p)| {
            let session = Session::open(p).ok()?;
            let pr = profile(
                &session.records,
                &request_prices_for(s, &session.header.endpoint),
            );
            let ob = observations(&session.records, &episodes(&session.records));
            let pt = sonde_proxy::program::run_point(
                &p.to_string_lossy(),
                &session.header.created.to_rfc3339(),
                &session.records,
                &pr,
                &ob,
            );
            Some((pt, session.header.git))
        })
        .collect()
}

/// [`sonde_proxy::program::Since`] in a line: "past the baseline since
/// a1b2c3d (2 runs ago); within at 9f8e7d6".
fn since_line(s: &sonde_proxy::program::Since) -> String {
    let short = |g: &Option<sonde_proxy::session::GitStamp>, path: &str| match g {
        Some(g) => format!(
            "{}{}",
            &g.commit[..g.commit.len().min(7)],
            if g.dirty { " (with changes)" } else { "" }
        ),
        None => Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    let ago = match s.runs_ago {
        0 => "this run".to_string(),
        1 => "1 run ago".to_string(),
        n => format!("{n} runs ago"),
    };
    let mut t = format!(
        "past the baseline since {} ({ago})",
        short(&s.first_past_commit, &s.first_past)
    );
    if let Some(good) = &s.last_within {
        t.push_str(&format!(
            "; within at {}",
            short(&s.last_within_commit, good)
        ));
    }
    t
}

/// A code place's file as the repository has it: the program ran in `cwd`
/// (perhaps a folder of the repository), annotations name files from its root.
fn repo_path(cwd: Option<&str>, file: &str) -> String {
    let Some(cwd) = cwd else {
        return file.to_string();
    };
    let full = Path::new(cwd).join(file);
    let root = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .or_else(|| std::env::var_os("GITHUB_WORKSPACE").map(PathBuf::from));
    root.and_then(|r| {
        let r = r.canonicalize().unwrap_or(r);
        let f = full.canonicalize().unwrap_or(full.clone());
        f.strip_prefix(&r)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
    })
    .unwrap_or_else(|| file.to_string())
}

/// A workflow command's message: `%`, CR and LF escaped.
fn gh_escape(t: &str) -> String {
    t.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// A workflow command's property value: `:` and `,` escaped too.
fn gh_property(t: &str) -> String {
    gh_escape(t).replace(':', "%3A").replace(',', "%2C")
}

/// Text the program controls (a finding names its keys), as Markdown that
/// shows it as written: no formatting, no HTML, no table cell break, no
/// @mention.
fn md_text(t: &str) -> String {
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

/// A URL as it may be printed: without its query, which may be a presigned
/// URL's signature.
fn url_shown(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// A baseline at an https URL (a CI artifact, a presigned URL), fetched to a
/// temporary file: in a minute at most, and 1 GiB at most.
async fn fetch_baseline(url: &str) -> anyhow::Result<tempfile::TempPath> {
    const MAX: u64 = 1 << 30;
    let shown = url_shown(url);
    let local = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h == "127.0.0.1" || h == "localhost" || h == "[::1]")
        })
        .unwrap_or(false);
    if !url.starts_with("https://") && !local {
        anyhow::bail!("baseline {shown}: only https URLs are fetched");
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        // a redirect keeps to https
        .redirect(reqwest::redirect::Policy::custom(|a| {
            if a.url().scheme() == "https" && a.previous().len() < 10 {
                a.follow()
            } else {
                a.stop()
            }
        }))
        .build()?;
    let fail = |e: reqwest::Error| anyhow::anyhow!("baseline {shown}: {}", e.without_url());
    let mut resp = client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(fail)?;
    if resp.content_length().is_some_and(|n| n > MAX) {
        anyhow::bail!("baseline {shown}: larger than 1 GiB");
    }
    let mut f = tempfile::Builder::new().suffix(".sonde").tempfile()?;
    let mut got = 0u64;
    while let Some(chunk) = resp.chunk().await.map_err(fail)? {
        got += chunk.len() as u64;
        if got > MAX {
            anyhow::bail!("baseline {shown}: larger than 1 GiB");
        }
        std::io::Write::write_all(&mut f, &chunk)?;
    }
    Ok(f.into_temp_path())
}

pub async fn check(s: &Arc<Sonde>, a: &CheckArgs) -> anyhow::Result<i32> {
    use sonde_proxy::program::{Budget, check, run_point};
    // the project's budget, which the flags override
    // --init writes the file: there is none to read yet
    let project_path = if a.init {
        None
    } else {
        a.config.clone().or_else(find_project_file)
    };
    let project: ProjectCheck = match &project_path {
        Some(p) => {
            let text =
                std::fs::read_to_string(p).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?;
            toml::from_str::<ProjectFile>(&text)
                .map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?
                .check
        }
        None => ProjectCheck::default(),
    };
    // a path in sonde.toml is the project's: relative to the file, not to
    // the folder `sonde check` runs in
    let project_baseline = project.baseline.clone().map(|b| {
        let path_like = b != "auto" && !b.starts_with("https://") && !b.starts_with("http://");
        match project_path.as_ref().and_then(|p| p.parent()) {
            Some(dir)
                if path_like
                    && Path::new(&b).is_relative()
                    && !dir.as_os_str().is_empty()
                    && dir.join(&b).exists() =>
            {
                dir.join(&b).to_string_lossy().into_owned()
            }
            _ => b,
        }
    });
    let baseline_arg = a.baseline.clone().or(project_baseline);
    let mut fetched: Option<tempfile::TempPath> = None;
    if let Some(u) = baseline_arg
        .as_deref()
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
    {
        fetched = Some(fetch_baseline(u).await?);
    }
    let file = match &a.file {
        Some(f) => super::sessions::resolve(s, f),
        None => newest_session(s)?,
    };
    let read = |p: &PathBuf| -> anyhow::Result<_> {
        let session = Session::open(p).map_err(|e| anyhow::anyhow!(e))?;
        let pr = profile(
            &session.records,
            &request_prices_for(s, &session.header.endpoint),
        );
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
    if a.init {
        return init_project(a, &run.0);
    }
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
    let base_path = match baseline_arg.as_deref() {
        None => None,
        Some(_) if fetched.is_some() => fetched.as_ref().map(|p| p.to_path_buf()),
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
        max_requests: a.max_requests.or(project.max_requests),
        max_waiting_ms: match (a.max_waiting, &project.max_waiting) {
            (Some(v), _) => Some(v),
            (None, Some(v)) => Some(waiting_ms(v)?),
            (None, None) => None,
        },
        max_cost: a.max_cost.or(project.max_cost),
        max_errors: a.max_errors.or(project.max_errors),
        max_p95_ms: match (a.max_p95, &project.max_p95) {
            (Some(v), _) => Some(v),
            (None, Some(v)) => Some(duration_ms("max-p95", v)?),
            (None, None) => None,
        },
        expect: project.expect.clone(),
        tolerance: base
            .as_ref()
            .map(|_| a.tolerance.or(project.tolerance).unwrap_or(10.0) / 100.0),
        no_new_warnings: a.no_new_warnings || project.no_new_warnings,
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
            "nothing to check: give a budget (--max-requests, --max-waiting, --max-cost, --max-errors, --max-p95, or sonde.toml's [check]) or a --baseline"
        );
        return Ok(64);
    }
    // past the baseline: since which run, and the commits around it
    let since = match (&base, &program) {
        (Some(b), Some(key)) if !c.passed => {
            let runs = program_runs(s, key, &file);
            sonde_proxy::program::since(&runs, &b.0, budget.tolerance.unwrap_or(0.1))
        }
        _ => None,
    };
    let since_text = since.as_ref().map(since_line);
    if matches!(a.format, Format::Json) {
        println!(
            "{}",
            output::json(&serde_json::json!({ "run": run.0, "check": c, "since": since }))
        );
        return Ok(if c.passed { 0 } else { 1 });
    }
    let color = std::io::stdout().is_terminal();
    let shown = |what: &str, v: f64| match what {
        "waiting" | "p95" => ms_text(v),
        "cost" => money_text(v, &run.0.currency),
        _ => format!("{v:.0}"),
    };
    // GitHub Actions: each verdict that failed on the run, each finding on
    // the line of code that sent its requests
    if matches!(a.format, Format::Github) {
        for v in c.verdicts.iter().filter(|v| !v.ok) {
            println!(
                "::error title=sonde check::{}",
                gh_escape(&format!(
                    "{} {} > {} ({}){}",
                    v.what,
                    shown(v.what, v.value),
                    shown(v.what, v.limit),
                    v.against,
                    since_text
                        .as_deref()
                        .map(|t| format!(" · {t}"))
                        .unwrap_or_default()
                ))
            );
        }
        for w in &c.new_warnings {
            println!(
                "::warning title=sonde: new since the baseline::{}",
                gh_escape(w)
            );
        }
        for f in run.3.iter().take(10) {
            // what the project expects is said, never warned of
            let level = if f.level == "warn" && !sonde_proxy::program::expected(f, &budget.expect) {
                "warning"
            } else {
                "notice"
            };
            match f.callers.first().and_then(|c| code_place(&c.place)) {
                Some((file, line)) => println!(
                    "::{level} file={},line={line},title=sonde::{}",
                    gh_property(&repo_path(header.cwd.as_deref(), &file)),
                    gh_escape(&f.text)
                ),
                None => println!("::{level} title=sonde::{}", gh_escape(&f.text)),
            }
        }
        println!(
            "sonde: {}",
            if c.passed {
                "within budget"
            } else {
                "over budget"
            }
        );
        return Ok(if c.passed { 0 } else { 1 });
    }
    // a pull request's comment: the verdicts, what is new, and the run's findings
    if matches!(a.format, Format::Markdown) {
        let mut md = format!(
            "### sonde: {}\n\n| | | run | limit | from |\n|---|---|---:|---:|---|\n",
            if c.passed {
                "within budget ✅"
            } else {
                "over budget ❌"
            }
        );
        for v in &c.verdicts {
            md.push_str(&format!(
                "| {} | {} | {} | ≤ {} | {} |\n",
                if v.ok { "pass" } else { "**FAIL**" },
                v.what,
                shown(v.what, v.value),
                shown(v.what, v.limit),
                v.against
            ));
        }
        if let Some(t) = &since_text {
            md.push_str(&format!("\n**Since** {}\n", md_text(t)));
        }
        if !c.new_warnings.is_empty() {
            md.push_str("\n**New since the baseline**\n\n");
            for w in &c.new_warnings {
                md.push_str(&format!("- {}\n", md_text(w)));
            }
        }
        if !c.expected.is_empty() {
            md.push_str(&format!(
                "\n*Expected (sonde.toml): {}*\n",
                c.expected.len()
            ));
        }
        let findings: Vec<_> = run.3.iter().take(5).collect();
        if !findings.is_empty() {
            md.push_str("\n<details><summary>What stands out</summary>\n\n");
            for f in findings {
                md.push_str(&format!("- {}", md_text(&f.text)));
                if let Some(c) = f.callers.first() {
                    md.push_str(&format!(
                        " — `{}`",
                        c.place.replace('`', "'").replace(['\n', '\r'], " ")
                    ));
                }
                md.push('\n');
            }
            md.push_str("\n</details>\n");
        }
        print!("{md}");
        return Ok(if c.passed { 0 } else { 1 });
    }
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
    for w in &c.expected {
        println!("{} {w} (expected)", paint("·", "2", color));
    }
    if let Some(t) = &since_text {
        println!("{}", paint(t, "33", color));
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
    /// Post each new warning, and each finished file over sonde.toml's
    /// budget, to this webhook as `{"text": …}` (Slack, Teams, Mattermost
    /// and most chat services take it).
    #[arg(long)]
    alert: Option<String>,
    /// Watch production through its logs instead of a proxy: the S3 server
    /// access logs or CloudTrail files written under this `s3://bucket/prefix`,
    /// read through the endpoint (read only) as they arrive.
    #[arg(long, value_name = "S3_URL")]
    logs: Option<String>,
    /// With --logs: look for new log files this often ("15m", "1h", "30s").
    #[arg(long, default_value = "15m")]
    every: String,
    /// With --logs: read what is new once, then exit (for cron or CI).
    #[arg(long)]
    once: bool,
}

/// Where `watch --logs` got to: the newest log file's time read, and the
/// files read since a little before it.
#[derive(serde::Serialize, serde::Deserialize)]
struct LogsMark {
    watermark: chrono::DateTime<chrono::Utc>,
    seen: std::collections::BTreeSet<String>,
}

/// The program key of a log source's sessions: its runs make a history and
/// `sonde check` holds them to a baseline like any program's.
pub const LOGS_OF: &str = "logs of ";

/// A duration as `--every` writes it: "15m", "1h", "30s", "90" (seconds).
fn every(v: &str) -> anyhow::Result<std::time::Duration> {
    let t = v.trim();
    let (n, unit) = t.split_at(t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len()));
    let n: u64 = n
        .parse()
        .map_err(|_| anyhow::anyhow!("--every {v:?}: say 30s, 15m or 1h"))?;
    let secs = match unit.trim() {
        "" | "s" => n,
        "m" | "min" => n * 60,
        "h" => n * 3600,
        _ => anyhow::bail!("--every {v:?}: say 30s, 15m or 1h"),
    };
    Ok(std::time::Duration::from_secs(secs.max(10)))
}

/// `sonde watch --logs s3://bucket/prefix`: production seen through its
/// logs. Each pass reads the log files written since the last one (through
/// the endpoint's credentials, GET and LIST only), keeps their requests as a
/// session of the program `logs of s3://…`, says each new warning, and holds
/// the pass to sonde.toml's budget; `--alert` posts both. Where it got to is
/// kept, so a restart goes on from there.
async fn watch_logs(s: &Arc<Sonde>, a: &WatchArgs, src: &str) -> anyhow::Result<i32> {
    use std::collections::HashSet;
    let Some((bucket, prefix)) = src
        .strip_prefix("s3://")
        .map(|r| r.split_once('/').unwrap_or((r, "")))
        .filter(|(b, _)| !b.is_empty())
    else {
        eprintln!("--logs takes s3://bucket/prefix");
        return Ok(64);
    };
    let ep = super::endpoint(s, a.proxy.rec.endpoint.as_deref())?;
    let client = s.client(&ep)?;
    let pause = every(&a.every)?;
    let dir = s.paths().sessions();
    std::fs::create_dir_all(&dir)?;
    let key = format!("{LOGS_OF}s3://{bucket}/{prefix}");
    // where the last pass got to, per source
    let mark_file = dir.join(format!(
        "logs-{:016x}.mark",
        key.bytes()
            .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64)
                .wrapping_mul(0x100_0000_01b3))
    ));
    // where it got to: the newest file's time read, and the files read near
    // it (logs arrive late, and prefixes of regions or days list in any
    // order, so a time and the files seen, not the last key listed)
    let mut mark: LogsMark = std::fs::read(&mark_file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| LogsMark {
            // a first pass reads what arrived in the last interval, not the
            // whole history under the prefix
            watermark: chrono::Utc::now() - chrono::Duration::from_std(pause).unwrap_or_default(),
            seen: Default::default(),
        });
    let mut warned: HashSet<String> = HashSet::new();
    let mut sent = Vec::new();
    eprintln!("watching {src} every {}s", pause.as_secs());
    loop {
        // the files written since the mark (less a grace for late ones), not read yet
        let grace = mark.watermark - chrono::Duration::hours(2);
        let mut keys: Vec<(String, u64, chrono::DateTime<chrono::Utc>)> = Vec::new();
        let mut token = None;
        let listed = loop {
            let page = client
                .list_objects_v2(
                    bucket,
                    &s3raw::ops::ListV2 {
                        prefix: Some(prefix.to_string()),
                        continuation_token: token.clone(),
                        url_encoding: true,
                        ..Default::default()
                    },
                )
                .await;
            let (page, _) = match page {
                Ok(p) => p,
                Err(e) => break Err(e),
            };
            keys.extend(page.contents.iter().filter_map(|o| {
                let at = o
                    .last_modified
                    .as_deref()
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())?
                    .with_timezone(&chrono::Utc);
                (at > grace && !mark.seen.contains(&o.key)).then(|| (o.key.clone(), o.size, at))
            }));
            token = page.next_continuation_token.filter(|_| page.is_truncated);
            if token.is_none() {
                break Ok(());
            }
        };
        // a store that did not answer this time is asked again next time
        if let Err(e) = listed {
            eprintln!(
                "{} listing {src}: {e}",
                paint("!", "33", std::io::stderr().is_terminal())
            );
            keys.clear();
        }
        let mut records = Vec::new();
        for (k, size, at) in &keys {
            // a log file is small; something this large is not one
            if *size > 256 << 20 {
                mark.seen.insert(k.clone());
                continue;
            }
            let r = match client.get_object(bucket, k, None).await {
                Ok(r) => r,
                Err(e) => {
                    // gone (a lifecycle rule) or not reachable now: tried again next pass
                    eprintln!(
                        "{} {k}: {e}",
                        paint("!", "33", std::io::stderr().is_terminal())
                    );
                    continue;
                }
            };
            let mut bytes = r.body.to_vec();
            if bytes.starts_with(&[0x1f, 0x8b]) {
                let mut out = Vec::new();
                if std::io::Read::read_to_end(
                    &mut flate2::read::MultiGzDecoder::new(&bytes[..]),
                    &mut out,
                )
                .is_ok()
                {
                    bytes = out;
                }
            }
            let text = String::from_utf8_lossy(&bytes);
            if let Ok((_, session)) = sonde_proxy::import::import(&text, k) {
                records.extend(session.records);
            }
            mark.seen.insert(k.clone());
            if *at > mark.watermark {
                mark.watermark = *at;
            }
        }
        // the seen files old enough to be past the grace are forgotten
        let keep_after = mark.watermark - chrono::Duration::hours(2);
        let recent: std::collections::BTreeSet<String> = keys
            .iter()
            .filter(|(_, _, at)| *at > keep_after)
            .map(|(k, _, _)| k.clone())
            .collect();
        if mark.seen.len() > 50_000 {
            mark.seen.retain(|k| recent.contains(k));
        }
        if let Ok(b) = serde_json::to_vec(&mark) {
            let _ = std::fs::write(&mark_file, b);
        }
        // the requests are the endpoint's: priced and checked as its
        for r in records.iter_mut() {
            r.endpoint = ep.clone();
        }
        if !records.is_empty() {
            records.sort_by_key(|r| r.time);
            for (i, r) in records.iter_mut().enumerate() {
                r.id = i as u64 + 1;
            }
            let mut header = SessionHeader::new(&ep, "", "", Some(key.clone()));
            header.created = chrono::Utc::now();
            let path = sonde_proxy::session::fresh_path(
                &dir,
                &format!(
                    "{}-logs-{bucket}",
                    chrono::Local::now().format("%Y%m%d-%H%M%S")
                ),
            );
            let session = Session {
                header,
                records,
                logs: Vec::new(),
            };
            sonde_proxy::import::save(&session, &path)?;
            let records = &session.records;
            eprintln!(
                "{} {} requests · {}",
                paint("·", "2", std::io::stderr().is_terminal()),
                records.len(),
                path.display()
            );
            for o in observations(records, &episodes(records)) {
                if o.level == "warn" && warned.insert(sonde_proxy::profile::finding_identity(&o)) {
                    let place = o
                        .callers
                        .first()
                        .map(|c| format!(" — {}", c.place))
                        .unwrap_or_default();
                    eprintln!(
                        "{} {}{place}",
                        paint("!", "33", std::io::stderr().is_terminal()),
                        o.text
                    );
                    if let Some(url) = &a.alert {
                        sent.push(post_alert(
                            url,
                            format!("sonde watch ({src}): {}{place}", o.text),
                        ));
                    }
                }
            }
            if let (Some(url), Some(over)) = (&a.alert, over_budget(s, records)) {
                sent.push(post_alert(
                    url,
                    format!(
                        "sonde watch ({src}): {} is over budget: {over}",
                        path.display()
                    ),
                ));
            }
        }
        sent.retain(|h: &tokio::task::JoinHandle<()>| !h.is_finished());
        if a.once {
            break;
        }
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(pause) => {}
        }
    }
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        futures::future::join_all(sent),
    )
    .await;
    Ok(0)
}

/// Posts `text` to a webhook, in the background; a failure is said once on stderr.
fn post_alert(url: &str, text: String) -> tokio::task::JoinHandle<()> {
    let url = url.to_string();
    tokio::spawn(async move {
        let r = reqwest::Client::new()
            .post(&url)
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await
            .and_then(|r| r.error_for_status());
        if let Err(e) = r {
            static SAID: std::sync::Once = std::sync::Once::new();
            SAID.call_once(|| {
                eprintln!("alert to {} failed: {}", url_shown(&url), e.without_url())
            });
        }
    })
}

/// A finished file held to the project's budget (`sonde.toml`'s `[check]`):
/// what went over, when anything did.
fn over_budget(s: &Arc<Sonde>, records: &[Record]) -> Option<String> {
    use sonde_proxy::program::{Budget, check, run_point};
    let text = std::fs::read_to_string(find_project_file()?).ok()?;
    let p: ProjectCheck = toml::from_str::<ProjectFile>(&text).ok()?.check;
    let budget = Budget {
        max_requests: p.max_requests,
        max_waiting_ms: p.max_waiting.as_ref().and_then(|v| waiting_ms(v).ok()),
        max_cost: p.max_cost,
        max_errors: p.max_errors,
        max_p95_ms: p
            .max_p95
            .as_ref()
            .and_then(|v| duration_ms("max-p95", v).ok()),
        expect: p.expect.clone(),
        tolerance: None,
        no_new_warnings: false,
    };
    let endpoint = records.first().map(|r| r.endpoint.as_str()).unwrap_or("");
    let prof = profile(records, &request_prices_for(s, endpoint));
    let obs = observations(records, &episodes(records));
    let point = run_point("", "", records, &prof, &obs);
    let c = check((&point, records, &prof, &obs), None, &budget);
    let over: Vec<String> = c
        .verdicts
        .iter()
        .filter(|v| !v.ok)
        .map(|v| match v.what {
            "cost" => format!(
                "cost {} > {}",
                money_text(v.value, &prof.cost.currency),
                money_text(v.limit, &prof.cost.currency)
            ),
            "waiting" | "p95" => format!("{} {} > {}", v.what, ms_text(v.value), ms_text(v.limit)),
            what => format!("{what} {:.0} > {:.0}", v.value, v.limit),
        })
        .collect();
    (!over.is_empty()).then(|| over.join(", "))
}

/// A proxy left running: one status line for the last minute, each new
/// warning as it appears, and a new session file every --rotate minutes.
pub async fn watch(s: &Arc<Sonde>, a: &WatchArgs) -> anyhow::Result<i32> {
    use std::collections::{HashSet, VecDeque};
    use std::time::{Duration, Instant};
    if let Some(src) = &a.logs {
        return watch_logs(s, a, src).await;
    }
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
    // alerts on their way: waited for (a little) before sonde exits
    let mut sent: Vec<tokio::task::JoinHandle<()>> = Vec::new();
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
                            if let Some(url) = &a.alert {
                                sent.push(post_alert(url, format!("sonde watch: {}", o.text)));
                            }
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
                    if let (Some(url), Some(over)) = (&a.alert, over_budget(s, &file_records)) {
                        sent.push(post_alert(url, format!("sonde watch: {} is over budget: {over}", old_path.display())));
                    }
                    sent.retain(|h| !h.is_finished());
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
    // the last file is held to the budget too
    if let (Some(url), Some(over)) = (&a.alert, over_budget(s, &file_records)) {
        sent.push(post_alert(
            url,
            format!("sonde watch: {} is over budget: {over}", path.display()),
        ));
    }
    let _ = tokio::time::timeout(Duration::from_secs(10), futures::future::join_all(sent)).await;
    eprintln!("{}", path.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_findings_text_is_shown_as_written_in_a_pr_comment() {
        assert_eq!(
            md_text("key `a|b` <b>x</b> @team *hot*"),
            "key \\`a\\|b\\` \\<b\\>x\\</b\\> @\u{2060}team \\*hot\\*"
        );
    }

    #[test]
    fn a_code_place_is_its_file_and_line_spaces_and_drives_too() {
        assert_eq!(
            code_place("etl/load.py:212 read"),
            Some(("etl/load.py".into(), 212))
        );
        assert_eq!(
            code_place("my dir/x.py:3 f"),
            Some(("my dir/x.py".into(), 3))
        );
        assert_eq!(
            code_place(r"C:\src\x.py:9 main"),
            Some((r"C:\src\x.py".into(), 9))
        );
        assert_eq!(code_place("no line here"), None);
    }

    #[test]
    fn a_presigned_url_is_printed_without_its_signature() {
        assert_eq!(
            url_shown("https://b.s3.amazonaws.com/base.sonde?X-Amz-Signature=abc"),
            "https://b.s3.amazonaws.com/base.sonde"
        );
    }

    #[test]
    fn every_reads_seconds_minutes_and_hours() {
        assert_eq!(every("15m").unwrap().as_secs(), 900);
        assert_eq!(every("1h").unwrap().as_secs(), 3600);
        assert_eq!(every("90").unwrap().as_secs(), 90);
        assert!(every("soon").is_err());
    }

    #[tokio::test]
    async fn watching_logs_keeps_each_pass_as_a_run_of_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap());
        sonde_core::demo::install(&s).await.unwrap();
        // an access log written into the bucket
        let log = "79a59df900b949e55d96a1e698fbacedfd6e09d98eacf8f8d5218e7cd47ef2be demo [06/Feb/2026:00:00:38 +0000] 192.0.2.3 - 3E57427F3EXAMPLE REST.GET.OBJECT data/a.csv \"GET /demo/data/a.csv HTTP/1.1\" 200 - 113 113 7 6 \"-\" \"boto3/1.34\" - x SigV4 ECDHE TLSv1.2 - -\n\
                   79a59df900b949e55d96a1e698fbacedfd6e09d98eacf8f8d5218e7cd47ef2be demo [06/Feb/2026:00:00:39 +0000] 192.0.2.3 - 3E57427F3EXAMPLF REST.HEAD.OBJECT data/missing \"HEAD /demo/data/missing HTTP/1.1\" 404 NoSuchKey - - 4 4 \"-\" \"boto3/1.34\" - x SigV4 ECDHE TLSv1.2 - -\n";
        s.client("demo")
            .unwrap()
            .put_object(
                "demo",
                "logs/2026-02-06-00-00-40-AB",
                bytes::Bytes::from(log),
                &[],
            )
            .await
            .unwrap();
        let args = WatchArgs {
            proxy: ProxyArgs {
                rec: Recording {
                    endpoint: Some("demo".into()),
                    no_delete: false,
                    read_only: true,
                    out: None,
                },
                listen: "127.0.0.1:0".parse().unwrap(),
                allow_remote: false,
            },
            rotate: 60,
            alert: None,
            logs: Some("s3://demo/logs/".into()),
            every: "15m".into(),
            once: true,
        };
        assert_eq!(watch_logs(&s, &args, "s3://demo/logs/").await.unwrap(), 0);
        let sessions: Vec<_> = std::fs::read_dir(s.paths().sessions())
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "sonde"))
            .collect();
        assert_eq!(sessions.len(), 1);
        let session = Session::open(&sessions[0]).unwrap();
        assert_eq!(session.records.len(), 2);
        assert_eq!(
            session.header.command.as_deref(),
            Some("logs of s3://demo/logs/")
        );
        // a second pass has nothing new: no second session
        assert_eq!(watch_logs(&s, &args, "s3://demo/logs/").await.unwrap(), 0);
        let count = || {
            std::fs::read_dir(s.paths().sessions())
                .unwrap()
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "sonde"))
                .count()
        };
        assert_eq!(count(), 1);
        // a file that lists before the ones read (another region, a late
        // delivery) is still read
        s.client("demo")
            .unwrap()
            .put_object(
                "demo",
                "logs/0-earlier/2026-02-06-00-00-41-CD",
                bytes::Bytes::from(log),
                &[],
            )
            .await
            .unwrap();
        assert_eq!(watch_logs(&s, &args, "s3://demo/logs/").await.unwrap(), 0);
        assert_eq!(count(), 2);
    }

    #[tokio::test]
    async fn a_baseline_is_fetched_over_https_only() {
        let e = fetch_baseline("http://example.com/base.sonde?sig=1")
            .await
            .unwrap_err();
        assert!(e.to_string().contains("only https"), "{e}");
        assert!(!e.to_string().contains("sig=1"));
    }
}
