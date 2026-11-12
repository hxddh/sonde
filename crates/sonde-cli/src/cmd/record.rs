//! Recording: `sonde run -- <command>`, `sonde proxy`, `sonde show <session>`.

use sonde_proxy::callers::code_place;
use sonde_proxy::program::{money_text, ms_text};
use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Args;
use reqlog::{Record, RequestLog};
use sonde_core::Sonde;
use sonde_core::guard::human_bytes;
use sonde_proxy::episode::{Episode, Outcome, episodes, observations};
use sonde_proxy::profile::{Profile, RequestPrices, profile};
use sonde_proxy::session::{Session, SessionHeader, SessionWriter};
use sonde_proxy::view::Analysis;
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
    /// Then run the command again under faults chosen from what it did (a
    /// 503 on each request's first attempt, a lost answer on each write, a
    /// failed completion if it uploads in parts, a body cut short if it
    /// downloads, a lost race if it writes on a condition), and say whether
    /// it survived each. The command runs several times, so give each run a place of
    /// its own: `{trial}` in the command is replaced by `clean`, then by
    /// each scenario's name, and the variable SONDE_TRIAL is set to the same
    /// (`-- python etl.py s3://scratch/etl-{trial}/`).
    #[arg(long)]
    trial: bool,
    /// A fault scenario to try by name (repeatable): slowdown, error,
    /// lost-answer, fail-complete, cut-body, lost-race, slow, s3-limits, or
    /// one of sonde.toml's. `slowdown:3` and `error:3` fail each request's
    /// first three attempts rather than its first (1 to 5).
    /// `{trial}` and SONDE_TRIAL are as with --trial.
    #[arg(long = "fault", value_name = "SCENARIO")]
    faults: Vec<String>,
    /// The program's name: its runs and its baseline go by it, whatever the
    /// command's arguments are this time (default: `name` under `[program]`
    /// in sonde.toml, else the command itself).
    #[arg(long)]
    name: Option<String>,
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
    /// At most this much time waiting on the store: ms, or as sonde.toml
    /// writes it (`2s`, `250ms`).
    #[arg(long, value_parser = duration_arg, value_name = "TIME")]
    max_waiting: Option<f64>,
    /// At most this request cost (in the settings' currency).
    #[arg(long)]
    max_cost: Option<f64>,
    /// At most this many failed requests.
    #[arg(long)]
    max_errors: Option<usize>,
    /// 95% of the requests within this long: ms, or `250ms`, `2s`.
    #[arg(long, value_parser = duration_arg, value_name = "TIME")]
    max_p95: Option<f64>,
    /// Against the baseline: requests, waiting and cost may grow this many
    /// percent (10 unless sonde.toml says).
    #[arg(long)]
    tolerance: Option<f64>,
    /// Against the baseline: fail on a warning it did not have.
    #[arg(long)]
    no_new_warnings: bool,
    /// Write `sonde.toml` here with a `[check]` that this run passes (its
    /// figures with --tolerance, 10%, of room) and, when the program has no
    /// name, a `[program] name` for its runs and its baseline to go by.
    /// Trials are not written: `sonde run --trial` tries the program, and a
    /// `[[scenario]]` is the project's own to add.
    #[arg(long)]
    init: bool,
    /// The format written: table, json, markdown (a pull request's comment) or github (annotations).
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    format: Format,
}

#[derive(Args)]
pub struct CompareArgs {
    /// The run before a change.
    before: PathBuf,
    /// The run after it.
    after: PathBuf,
    /// The format written: table or json.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
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
    /// Every line of a long session: past a few thousand, only the first
    /// and the last hundred are printed.
    #[arg(long)]
    all: bool,
    /// Open it in the sonde app instead (a sonde:// link).
    #[arg(long)]
    app: bool,
    /// The format written: table or json.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    format: Format,
}

struct Started {
    proxy: Proxy,
    log: RequestLog,
    writer: Arc<SessionWriter>,
    path: PathBuf,
}

/// A trial run: the scenario it is put through, the clean run it is a trial
/// of, and the file it is recorded to.
struct Under {
    scenario: sonde_proxy::fault::Scenario,
    of: String,
    path: PathBuf,
}

/// What a recording is of, beside its command: the name the program goes
/// by, and the trial it is, if it is one.
#[derive(Default)]
struct Of<'a> {
    name: Option<String>,
    under: Option<&'a Under>,
}

async fn start(
    s: &Arc<Sonde>,
    r: &Recording,
    listen: SocketAddr,
    command: Option<String>,
    allow_remote: bool,
) -> anyhow::Result<Started> {
    start_under(s, r, listen, command, allow_remote, &Of::default()).await
}

async fn start_under(
    s: &Arc<Sonde>,
    r: &Recording,
    listen: SocketAddr,
    command: Option<String>,
    allow_remote: bool,
    of: &Of<'_>,
) -> anyhow::Result<Started> {
    let under = of.under;
    let ep = super::endpoint(s, r.endpoint.as_deref())?;
    let client = s.client(&ep)?;
    let cfg = client.config();
    let path = match (&under, &r.out) {
        (Some(u), _) => u.path.clone(),
        (None, Some(p)) => p.clone(),
        (None, None) => {
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
    header.name = of.name.clone().filter(|n| !n.trim().is_empty());
    // a session recorded again starts with no verdict of its last recording
    if under.is_none() {
        sonde_proxy::trial::remove(&path);
    }
    if let Some(u) = under {
        header.scenario = Some(u.scenario.name.clone());
        header.trial_of = Some(u.of.clone());
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
            faults: under.map(|u| Arc::new(sonde_proxy::fault::Faults::new(&u.scenario))),
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

/// Where the time went: the kinds of request the program waited on most.
/// `whole` (a run's own summary, which has no cost table and four signal
/// lines) says the in-flight peak and the cost here; `sonde show` has said
/// them above, once.
fn print_profile(p: &Profile, color: bool, whole: bool, say: &dyn Fn(String)) {
    if p.groups.is_empty() {
        return;
    }
    let mut line = format!(
        "waiting on S3 {} ({:.0}% of the run)",
        output::duration(p.busy_ms),
        p.busy_ms * 100.0 / p.wall_ms.max(1e-9),
    );
    if whole {
        line.push_str(&format!(
            " · request cost ≈ {}",
            money_text(p.cost.amount, &p.cost.currency)
        ));
    }
    say(paint(&line, "1", color).to_string());
    const ROWS: usize = 8;
    let rows: Vec<Vec<String>> = p
        .groups
        .iter()
        .take(ROWS)
        .map(|g| {
            vec![
                if g.missed {
                    format!("{} (not found)", g.operation)
                } else {
                    g.operation.clone()
                },
                format!("{:.0}%", g.share * 100.0),
                output::duration(g.waited_ms),
                output::grouped(g.requests as u64),
                output::duration(g.p50_ms),
                output::duration(g.p95_ms),
            ]
        })
        .collect();
    say(output::table(
        &["waiting on", "share", "time", "requests", "p50", "p95"],
        &rows,
    )
    .to_string());
    if p.groups.len() > ROWS {
        say(format!(
            "  … and {} more kinds of request",
            p.groups.len() - ROWS
        ));
    }
}

fn print_summary(
    records: &[Record],
    endpoint: &str,
    path: &Path,
    color: bool,
    prices: &RequestPrices,
    stdout: bool,
) {
    // (a run just recorded: its times are its own)
    let header = SessionHeader::new(endpoint, "", "", None);
    let analysis = Analysis::of_records(&header, records, prices);
    print_summary_of(&analysis, &header, path, color, stdout);
}

/// What a session amounts to, each fact once: the head line has the
/// requests, the operations and the time; the signals the rate, the errors,
/// the latency, the bytes and (for `sonde show`) the cost; then where the
/// waiting went, and the findings.
fn print_summary_of(a: &Analysis, header: &SessionHeader, path: &Path, color: bool, stdout: bool) {
    // `sonde show` answers on stdout; run and proxy keep stdout for the program's own output
    let say = |line: String| {
        if stdout {
            println!("{line}")
        } else {
            eprintln!("{line}")
        }
    };
    let s = &a.summary;
    let endpoint = &header.endpoint;
    let mut head = format!(
        "{} ({}) to {endpoint} in {}",
        output::count(s.requests as usize, "request"),
        output::count(a.episodes.len(), "operation"),
        output::duration(s.seconds * 1000.0)
    );
    if s.errors > 0 {
        head.push_str(&format!(
            " · {}",
            paint(&output::count(s.errors as usize, "error"), "33", color)
        ));
    }
    if s.misses > 0 {
        head.push_str(&format!(" · {} not found", output::grouped(s.misses)));
    }
    say(format!("\n{}", paint(&head, "1", color)));
    if !s.operations.is_empty() {
        // a percentile of a handful of requests is one of them: marked
        let few = s.operations.iter().any(|o| o.few);
        let ms = |v: f64, o: &sonde_proxy::summary::OpStats| {
            format!("{}{}", output::duration(v), if o.few { "*" } else { "" })
        };
        let rows: Vec<Vec<String>> = s
            .operations
            .iter()
            .map(|o| {
                vec![
                    o.operation.clone(),
                    output::grouped(o.count),
                    if o.errors > 0 {
                        format!(
                            "{} ({})",
                            output::grouped(o.errors),
                            share_text(o.error_share)
                        )
                    } else {
                        String::new()
                    },
                    if o.misses > 0 {
                        output::grouped(o.misses)
                    } else {
                        String::new()
                    },
                    ms(o.p50_ms, o),
                    ms(o.p95_ms, o),
                    ms(o.p99_ms, o),
                    output::duration(o.max_ms),
                ]
            })
            .collect();
        say(output::table(
            &[
                "operation",
                "requests",
                "errors",
                "not found",
                "p50",
                "p95",
                "p99",
                "max",
            ],
            &rows,
        )
        .to_string());
        if few {
            say(format!(
                "  * of fewer than {} requests",
                sonde_proxy::summary::FEW
            ));
        }
    }
    for (what, n) in &s.error_codes {
        say(format!("  {} × {what}", output::grouped(*n)));
    }
    // the four signals, each with its line over the run
    let mut signals = a.signals.clone();
    if header.whole_seconds {
        // where in its second a request began is not known: how many were
        // in flight at once is not said
        signals.in_flight.peak = 0;
        say("  its times are to the second (an imported log): a second's requests are spread over it in the log's order, and how many were in flight at once is not known".into());
    }
    let signals = sonde_proxy::signals::text(&signals, color);
    // a run's own summary keeps to the four lines; `sonde show` has the tables too
    for line in signals.lines().take(if stdout { usize::MAX } else { 4 }) {
        say(line.to_string());
    }
    print_profile(&a.profile, color, !stdout, &say);
    for o in &a.observations {
        let mark = if o.level == "warn" {
            paint("!", "33", color)
        } else {
            paint("·", "2", color)
        };
        say(format!("{mark} {}", o.text));
        // where in the program, when it says
        for c in &o.callers {
            say(paint(
                &format!(
                    "    at {} ({})",
                    c.place,
                    output::count(c.requests, "request")
                ),
                "2",
                color,
            ));
        }
    }
    // acting on all of them, at once
    if let Some(w) = &a.what_if {
        say(paint(&w.line(), "32", color));
    }
    say(paint(&format!("session: {}", path.display()), "2", color).to_string());
}

/// A share as a percentage: `0.4%`, `12%`.
fn share_text(share: f64) -> String {
    let p = share * 100.0;
    if p > 0.0 && p < 0.1 {
        "<0.1%".into()
    } else if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

/// The signal that asked sonde to stop, once one has: 2 (Ctrl-C) or 15
/// (SIGTERM). Listened for from the start of a run to its end, so that none
/// is lost between one run of a trial and the next.
#[derive(Clone)]
struct Stop(tokio::sync::watch::Receiver<i32>);

impl Stop {
    fn listen() -> Stop {
        let (tx, rx) = tokio::sync::watch::channel(0);
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let (Ok(mut int), Ok(mut term)) = (
                    signal(SignalKind::interrupt()),
                    signal(SignalKind::terminate()),
                ) else {
                    return;
                };
                loop {
                    let n = tokio::select! {
                        _ = int.recv() => 2,
                        _ = term.recv() => 15,
                    };
                    if tx.send(n).is_err() {
                        return;
                    }
                }
            }
            #[cfg(not(unix))]
            while tokio::signal::ctrl_c().await.is_ok() {
                if tx.send(2).is_err() {
                    return;
                }
            }
        });
        Stop(rx)
    }

    /// The signal received, if one was.
    fn signal(&self) -> Option<i32> {
        let n = *self.0.borrow();
        (n != 0).then_some(n)
    }

    /// Waits for the next signal.
    async fn asked(&mut self) -> i32 {
        if self.0.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
        *self.0.borrow()
    }
}

/// What every run of one `sonde run` shares.
struct RunCtx {
    /// A trial follows the clean run: `{trial}` and SONDE_TRIAL say which
    /// run each is.
    trying: bool,
    /// The program's name, when it has one.
    name: Option<String>,
    /// What the clean run read on its input, when that is not a terminal
    /// and a trial follows: every run is fed the same.
    input: Option<Arc<std::sync::Mutex<KeptInput>>>,
    stop: Stop,
}

/// The most of a clean run's input kept in memory for the runs after it:
/// past it, it is kept in a file.
const INPUT_IN_MEMORY: usize = 8 << 20;

/// What the clean run read on its input, kept so that every run of a trial
/// is fed the same: in memory while it is small, then in a file of its own
/// (unnamed, gone when sonde ends), so that a job fed gigabytes by a pipe
/// does not hold them all.
struct KeptInput {
    limit: usize,
    head: Vec<u8>,
    file: Option<std::fs::File>,
    /// The file could not be written: the runs after are fed nothing, and
    /// are told so once.
    lost: bool,
}

impl Default for KeptInput {
    fn default() -> Self {
        KeptInput {
            limit: INPUT_IN_MEMORY,
            head: Vec::new(),
            file: None,
            lost: false,
        }
    }
}

/// A kept input, to be read once more.
enum Again {
    Bytes(Vec<u8>),
    File(std::fs::File),
    Lost,
}

impl KeptInput {
    fn push(&mut self, chunk: &[u8]) {
        use std::io::Write;
        if self.lost {
            return;
        }
        if self.file.is_none() && self.head.len() + chunk.len() > self.limit {
            match tempfile::tempfile().and_then(|mut f| f.write_all(&self.head).map(|_| f)) {
                Ok(f) => {
                    self.file = Some(f);
                    self.head = Vec::new();
                }
                Err(e) => return self.lose(e),
            }
        }
        match &mut self.file {
            Some(f) => {
                if let Err(e) = f.write_all(chunk) {
                    self.lose(e);
                }
            }
            None => self.head.extend_from_slice(chunk),
        }
    }

    fn lose(&mut self, why: std::io::Error) {
        eprintln!(
            "sonde: the program's input could not be kept for the trial's runs ({why}): they read none"
        );
        self.lost = true;
        self.file = None;
        self.head = Vec::new();
    }

    /// The input from its start: its bytes, or its file opened anew.
    fn again(&mut self) -> Again {
        use std::io::{Seek, SeekFrom};
        if self.lost {
            return Again::Lost;
        }
        match &mut self.file {
            // (runs come one after the other: the place in the file is one run's)
            Some(f) => match f.try_clone().and_then(|mut c| {
                c.seek(SeekFrom::Start(0))?;
                Ok(c)
            }) {
                Ok(c) => Again::File(c),
                Err(_) => Again::Lost,
            },
            None => Again::Bytes(self.head.clone()),
        }
    }
}

/// One run of the command.
struct Ran {
    file: PathBuf,
    /// The command's exit code.
    code: i32,
    /// How long the command ran.
    wall_ms: f64,
    /// The signal sonde was sent while it ran.
    signal: Option<i32>,
}

/// Whether the command gives each run of a trial a place of its own, as far
/// as its words say: `{trial}` in an argument, or SONDE_TRIAL named in one.
fn names_its_place(command: &[String]) -> bool {
    command
        .iter()
        .any(|w| w.contains("{trial}") || w.contains("SONDE_TRIAL"))
}

pub async fn run(s: &Arc<Sonde>, _cli: &Cli, a: &RunArgs) -> anyhow::Result<i32> {
    // sonde.toml and the scenarios asked for by name are checked before
    // anything runs
    let project = load_project(None)?;
    let mut asked: Vec<sonde_proxy::fault::Scenario> = Vec::new();
    let mut specs: Vec<&str> = Vec::new();
    for name in &a.faults {
        let scenario = named_scenario(name, &project.scenarios)?;
        // named twice, run once: the second run would write over the first
        match asked.iter().position(|s| s.name == scenario.name) {
            None => {
                asked.push(scenario);
                specs.push(name);
            }
            // one scenario at two strengths has one file to go to: refused,
            // not silently run at the first
            Some(at) if specs[at] != name.as_str() => {
                return Err(super::usage(format!(
                    "--fault {} and --fault {name} are one scenario and would write one session: name it once",
                    specs[at]
                )));
            }
            Some(_) => {}
        }
    }
    let trying = a.trial || !asked.is_empty();
    let ctx = RunCtx {
        trying,
        name: a.name.clone().or(project.file.program.name.clone()),
        input: (trying && !std::io::stdin().is_terminal()).then(Default::default),
        stop: Stop::listen(),
    };
    if trying && !names_its_place(&a.command) {
        eprintln!(
            "warning: the command has no {{trial}} and does not name SONDE_TRIAL: every run of the trial writes to the same place"
        );
    }
    let clean = run_once(s, a, None, &ctx).await?;
    let (file, code) = (clean.file.clone(), clean.code);
    if !trying {
        return Ok(if clean.signal == Some(15) { 143 } else { code });
    }
    // a trial is read against a run that worked, to its end
    if let Some(signal) = clean.signal {
        eprintln!("\nNo trial: the clean run was interrupted.");
        return Ok(if code == 0 { 128 + signal } else { code });
    }
    if code != 0 {
        eprintln!(
            "\nNo trial: the clean run exited {code}. A trial is read against a run that worked: nothing was tried."
        );
        return Ok(code);
    }
    let clean_session = Session::open(&file).map_err(|e| anyhow::anyhow!(e))?;
    if clean_session.records.is_empty() {
        eprintln!("\nNo trial: the program sent no requests to fail.");
        return Ok(code);
    }
    let scenarios = if !asked.is_empty() {
        asked
    } else if !project.scenarios.is_empty() {
        project.scenarios
    } else {
        sonde_proxy::trial::choose(&clean_session.records)
    };
    let of = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // trials are kept in a folder of their own: they are not runs of the
    // program (no trend, baseline or "newest" counts them)
    let dir = sonde_proxy::trial::folder_for(&file);
    std::fs::create_dir_all(&dir)?;
    let color = std::io::stderr().is_terminal();
    eprintln!(
        "\nTrial: the command runs {} more time{}, each under one scenario.",
        scenarios.len(),
        if scenarios.len() == 1 { "" } else { "s" }
    );
    let mut trials = Vec::new();
    let mut interrupted = None;
    for scenario in scenarios {
        let under = Under {
            // (the run's last trials were removed when it was recorded: the
            // name is free, and is the one the verdict keeps)
            path: dir.join(format!(
                "{}.sonde",
                sonde_proxy::trial::session_stem(&file, &scenario.name)
            )),
            of: of.clone(),
            scenario,
        };
        let ran = run_once(s, a, Some(&under), &ctx).await?;
        if let Some(signal) = ran.signal {
            // half a run says nothing of the program: no verdict, no session
            let _ = std::fs::remove_file(&ran.file);
            interrupted = Some((signal, under.scenario.name.clone()));
            break;
        }
        let run = Session::open(&ran.file).map_err(|e| anyhow::anyhow!(e))?;
        let mut t = sonde_proxy::trial::judge(
            &under.scenario,
            &clean_session.records,
            code,
            &run.records,
            ran.code,
        );
        // the command's own time: what a delay costs is not all in requests
        (t.wall_ms, t.clean_wall_ms) = (ran.wall_ms, clean.wall_ms);
        t.session = ran
            .file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        eprintln!("{}", trial_line(&t, color));
        trials.push(t);
    }
    if trials.is_empty() {
        let _ = std::fs::remove_dir(&dir);
    } else {
        sonde_proxy::trial::save(&file, &trials)?;
    }
    if let Some((signal, scenario)) = interrupted {
        eprintln!(
            "\nInterrupted under {scenario}: no verdict for it, and none for the scenarios after it."
        );
        return Ok(128 + signal);
    }
    use sonde_proxy::trial::Standing;
    let broke = trials
        .iter()
        .filter(|t| t.standing == Standing::Broke)
        .count();
    let tried = trials
        .iter()
        .filter(|t| t.standing != Standing::Untested)
        .count();
    eprintln!(
        "\n{}",
        if tried == 0 {
            "Nothing was tried: no scenario met a request of the program's.".to_string()
        } else if broke == 0 {
            "The program did not break.".to_string()
        } else {
            format!("The program broke under {broke} of {tried}.")
        }
    );
    eprintln!("Each trial is a session:");
    for t in &trials {
        if let Some(path) = sonde_proxy::trial::session_path(&file, t) {
            eprintln!("  sonde show {}", path.display());
        }
    }
    Ok(if broke > 0 { 1 } else { 0 })
}

/// A trial's line: its standing, the scenario, what happened.
pub fn trial_line(t: &sonde_proxy::trial::Trial, color: bool) -> String {
    use sonde_proxy::trial::Standing;
    let code = match t.standing {
        Standing::Survived => "32",
        Standing::Degraded => "33",
        Standing::Broke => "31",
        Standing::Untested => "2",
    };
    format!(
        "  {}  {:<14} {}",
        paint(&format!("{:<8}", t.standing.label()), code, color),
        t.asked(),
        t.line()
    )
}

/// A project's `sonde.toml`, read and checked: its budget, its program's
/// name, its scenarios.
#[derive(Default)]
struct Project {
    path: Option<PathBuf>,
    file: ProjectFile,
    scenarios: Vec<sonde_proxy::fault::Scenario>,
}

/// `sonde.toml` at `explicit`, or here or in a folder above. A file that
/// does not read, or says what sonde does not know, is a mistake in the
/// configuration (64): nothing of it is passed over in silence.
fn load_project(explicit: Option<&PathBuf>) -> anyhow::Result<Project> {
    let Some(path) = explicit.cloned().or_else(find_project_file) else {
        return Ok(Project::default());
    };
    let wrong = |e: String| super::usage(format!("{}: {e}", path.display()));
    let text = std::fs::read_to_string(&path).map_err(|e| wrong(e.to_string()))?;
    let file: ProjectFile = toml::from_str(&text).map_err(|e| {
        let hint = if text.contains("[[scenarios]]") {
            "\na scenario is a [[scenario]] table, one for each"
        } else {
            ""
        };
        wrong(format!("{}{hint}", e.to_string().trim_end()))
    })?;
    sonde_proxy::fault::project_program_name(file.program.name.as_deref()).map_err(wrong)?;
    let scenarios =
        sonde_proxy::fault::project_scenarios(file.scenario.iter().map(ProjectScenario::scenario))
            .map_err(wrong)?;
    Ok(Project {
        path: Some(path),
        file,
        scenarios,
    })
}

fn named_scenario(
    name: &str,
    project: &[sonde_proxy::fault::Scenario],
) -> anyhow::Result<sonde_proxy::fault::Scenario> {
    if let Some(s) = project.iter().find(|s| s.name == name) {
        return Ok(s.clone());
    }
    let asked = sonde_proxy::fault::scenario_asked(name)
        .map_err(|e| super::usage(format!("--fault {e}")))?;
    asked.ok_or_else(|| {
        let known: Vec<String> = sonde_proxy::fault::scenarios()
            .iter()
            .chain(project)
            .map(|s| format!("  {:<14} {}", s.name, s.about))
            .collect();
        super::usage(format!(
            "--fault {name}: no such scenario. The scenarios:\n{}",
            known.join("\n")
        ))
    })
}

/// One run of the command, recorded: the session's file and the program's
/// exit code. Under a scenario, the proxy answers with its faults.
async fn run_once(
    s: &Arc<Sonde>,
    a: &RunArgs,
    under: Option<&Under>,
    ctx: &RunCtx,
) -> anyhow::Result<Ran> {
    use tokio::io::AsyncWriteExt;
    let command = sonde_proxy::program::shell_join(&a.command);
    let mut started = start_under(
        s,
        &a.rec,
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Some(command),
        false,
        &Of {
            name: ctx.name.clone(),
            under,
        },
    )
    .await?;
    let (mut set, unset) = started.proxy.env();
    if a.callers || s.settings().name_callers {
        set.extend(sonde_proxy::callers::caller_env(&s.paths().python_hook())?);
    }
    // each run a place of its own, if the command asks for one: `{trial}`
    // in the command (and SONDE_TRIAL) is "clean", then the scenario's name.
    // A run that no trial follows is left as it was written.
    let trial = under.map_or("clean", |u| u.scenario.name.as_str());
    let placed = |word: &String| {
        if ctx.trying {
            word.replace("{trial}", trial)
        } else {
            word.clone()
        }
    };
    let program = placed(&a.command[0]);
    let mut child = tokio::process::Command::new(&program);
    child.args(a.command[1..].iter().map(placed));
    if ctx.trying {
        child.env("SONDE_TRIAL", trial);
    }
    for k in &unset {
        child.env_remove(k);
    }
    for (k, v) in &set {
        child.env(k, v);
    }
    if under.is_some() {
        // a trial's output is the clean run's again: the verdict is what is read
        child.stdout(std::process::Stdio::null());
        child.stderr(std::process::Stdio::null());
    }
    // a run is a process group of its own, so that ending it ends what it
    // started too (a supervisor's children, a shell's pipeline). Not the
    // clean run at a terminal: its program may read the keyboard, and
    // Ctrl-C reaches it there by itself.
    let own_group = cfg!(unix) && (under.is_some() || !std::io::stdin().is_terminal());
    #[cfg(unix)]
    if own_group {
        child.process_group(0);
    }
    match (&ctx.input, under.is_some()) {
        // fed by a pipe: every run reads what the first read
        (Some(_), _) => {
            child.stdin(std::process::Stdio::piped());
        }
        // a terminal answers one run, not four
        (None, true) => {
            child.stdin(std::process::Stdio::null());
        }
        (None, false) => {}
    }
    let printer = (a.live && under.is_none()).then(|| follow(&started.log, true));
    let began = std::time::Instant::now();
    let mut child = child
        .spawn()
        .map_err(|e| anyhow::anyhow!("cannot start {program}: {e}"))?;
    let feeder = match (&ctx.input, child.stdin.take()) {
        (Some(kept), Some(mut to)) if under.is_none() => {
            // sonde's input, to the program and kept: read on a thread of
            // its own, which a program that reads none of it leaves waiting
            // (a few chunks ahead at most: a program that reads slowly
            // slows whoever writes to sonde, as a pipe would)
            let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
            std::thread::spawn(move || {
                use std::io::Read;
                let mut buf = vec![0u8; 64 << 10];
                let mut stdin = std::io::stdin().lock();
                while let Ok(n) = stdin.read(&mut buf) {
                    if n == 0 || tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
            let kept = kept.clone();
            Some(tokio::spawn(async move {
                while let Some(chunk) = rx.recv().await {
                    kept.lock().expect("input").push(&chunk);
                    if to.write_all(&chunk).await.is_err() {
                        break;
                    }
                }
                // its end is the program's end of input
                drop(to);
            }))
        }
        (Some(kept), Some(mut to)) => {
            let again = kept.lock().expect("input").again();
            Some(tokio::spawn(async move {
                match again {
                    Again::Bytes(bytes) => {
                        let _ = to.write_all(&bytes).await;
                    }
                    // from the file it was kept in, a piece at a time
                    Again::File(file) => {
                        let mut from = tokio::fs::File::from_std(file);
                        let _ = tokio::io::copy(&mut from, &mut to).await;
                    }
                    Again::Lost => {}
                }
                drop(to);
            }))
        }
        _ => None,
    };
    let mut stop = ctx.stop.clone();
    let mut signal = stop.signal();
    let status = loop {
        tokio::select! {
            st = child.wait() => break st?,
            n = stop.asked() => {
                signal = Some(n);
                // Ctrl-C is the clean run's program's to answer: it is
                // waited for, and what it sent is kept (in a group of its
                // own the signal is passed on to it). A trial's run is
                // ended, and so is any run sonde is told to terminate: the
                // whole of it, not its first process alone.
                if under.is_some() || n == 15 {
                    if !(own_group && signal_group(&child, true)) {
                        let _ = child.start_kill();
                    }
                } else if own_group {
                    signal_group(&child, false);
                }
            }
        }
    };
    let wall_ms = began.elapsed().as_secs_f64() * 1000.0;
    if let Some(f) = feeder {
        f.abort();
    }
    if let Some(p) = printer {
        p.abort();
    }
    // requests still in flight are recorded before the file is read and packed
    shut_down(&mut started.proxy, SHUTDOWN_GRACE).await;
    started.writer.flush()?;
    if under.is_none() {
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
    }
    // finished: compressed to a tenth or less, still a .sonde file
    let file = started.path.clone();
    drop(started);
    let _ = sonde_proxy::session::compress(&file);
    Ok(Ran {
        file,
        code: exit_code(status),
        wall_ms,
        signal,
    })
}

/// Signals the process group `child` leads: an interrupt, or (`kill`) its
/// end. Whether it was sent.
#[cfg(unix)]
fn signal_group(child: &tokio::process::Child, kill: bool) -> bool {
    use rustix::process::{Pid, Signal, kill_process_group};
    let signal = if kill { Signal::KILL } else { Signal::INT };
    child
        .id()
        .and_then(|id| Pid::from_raw(id as i32))
        .is_some_and(|pid| kill_process_group(pid, signal).is_ok())
}

#[cfg(not(unix))]
fn signal_group(_child: &tokio::process::Child, _kill: bool) -> bool {
    false
}

/// How long a stopping proxy waits for the requests in flight.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Stops the proxy and waits `grace` for what is in flight. When requests
/// were still open after it, says so once: the session is short of them, and
/// nothing else would tell.
async fn shut_down(proxy: &mut Proxy, grace: std::time::Duration) -> bool {
    let drained = proxy.shutdown(grace).await;
    if !drained {
        eprintln!("sonde: {}", not_drained(grace));
    }
    drained
}

fn not_drained(grace: std::time::Duration) -> String {
    format!(
        "requests were still in flight {} s after the proxy stopped: the session does not have them",
        grace.as_secs_f64()
    )
}

/// The program's exit code as a shell gives it: its own, or 128 and the
/// signal that ended it.
fn exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return 128 + signal;
    }
    status.code().unwrap_or(1)
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
    let mut started = start(s, &a.rec, a.listen, None, a.allow_remote).await?;
    print_env(&started.proxy);
    let printer = follow(&started.log, false);
    tokio::signal::ctrl_c().await?;
    printer.abort();
    // requests still in flight are recorded before the file is read and packed
    shut_down(&mut started.proxy, SHUTDOWN_GRACE).await;
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

/// The newest session recorded from a program's own requests (`run`,
/// `proxy`, `watch`): what a bare `sonde show` or `sonde check` is about. A
/// replay, an import and a trial are sessions too, and are named to be read.
pub fn newest_session(s: &Sonde) -> anyhow::Result<PathBuf> {
    let dir = s.paths().sessions();
    let mut newest: Option<(chrono::DateTime<chrono::Utc>, PathBuf)> = None;
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("sonde") {
            continue;
        }
        let Some(h) = sonde_proxy::session::read_header(&p).filter(|h| h.is_recording()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(n, _)| h.created > *n) {
            newest = Some((h.created, p));
        }
    }
    newest.map(|(_, p)| p).ok_or_else(|| {
        super::usage(format!(
            "no recorded sessions in {} yet: record one with `sonde run -- <command>`",
            dir.display()
        ))
    })
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
                    .map(|e| e.title.chars().count())
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
    /// The format written: table or json; with several -e, markdown and csv too.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    format: Format,
}

#[derive(Args)]
pub struct DiffArgs {
    /// The original session.
    original: PathBuf,
    /// Its replay (`sonde replay -o`).
    replay: PathBuf,
    /// The format written: table or json.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
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
        // without a reason (a replay of another time, compared by `sonde
        // diff`) the reason is not the words again
        let line = if why == "not replayed" {
            format!("  {n} not replayed")
        } else {
            format!("  {n} not replayed: {why}")
        };
        println!("{}", paint(&line, "2", color));
    }
}

pub async fn replay(s: &Arc<Sonde>, _cli: &Cli, a: &ReplayArgs) -> anyhow::Result<i32> {
    use sonde_proxy::replay::{self as rp, ReplayOptions};
    if a.endpoint.len() > 1 {
        return replay_many(s, a).await;
    }
    if let Some(why) = output::unsupported(
        a.format,
        "replay on one endpoint",
        &[Format::Table, Format::Json],
    ) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    let (asked, original) = super::sessions::open(s, &a.file)?;
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
        "Replay {} from {} on {ep} ({}), {place}.",
        output::count(plan.requests, "request"),
        asked.display(),
        base.config().endpoint
    );
    eprintln!(
        "  {} and {}{}.",
        output::count(plan.writes, "write"),
        output::count(plan.deletes, "delete"),
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
        eprintln!(
            "Removed {}{} under {prefix}.",
            output::count(run.removed, "object"),
            match run.removed_versions {
                0 => String::new(),
                n => format!(
                    " and {} (the bucket keeps them)",
                    output::count(n, "version or delete marker")
                        .replace("version or delete markers", "versions and delete markers")
                ),
            }
        );
        if run.remaining > 0 {
            // each thing once, by what it is
            let kinds: Vec<String> = [
                (run.remaining_objects, "object"),
                (run.remaining_versions, "older version"),
                (run.remaining_markers, "delete marker"),
            ]
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, what)| output::count(*n, what))
            .collect();
            eprintln!(
                "sonde: {} still under {prefix}: the store would not delete {}. `sonde du -b BUCKET -p {prefix}` shows what is there.",
                kinds.join(", "),
                if run.remaining == 1 { "it" } else { "them" }
            );
        }
        if run.unaborted_uploads > 0 {
            eprintln!(
                "sonde: {} under {prefix} would not abort: {} parts stay billed. `sonde mpu` lists what is unfinished.",
                output::count(run.unaborted_uploads, "multipart upload"),
                if run.unaborted_uploads == 1 {
                    "its"
                } else {
                    "their"
                }
            );
        }
    }
    let left_behind = run.remaining > 0 || run.unaborted_uploads > 0;
    let c = run.comparison;
    match a.format {
        Format::Json => println!(
            "{}",
            output::json(&serde_json::json!({
                "plan": plan,
                "session": path,
                "comparison": c,
                "removed": run.removed,
                "removedVersions": run.removed_versions,
                "remaining": run.remaining,
                "remainingObjects": run.remaining_objects,
                "remainingVersions": run.remaining_versions,
                "remainingMarkers": run.remaining_markers,
                "unabortedUploads": run.unaborted_uploads,
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
    // what it wrote and could not remove is a failure, whatever the answers
    Ok(if left_behind {
        70
    } else if c.differ() > 0 {
        2
    } else {
        0
    })
}

/// One session replayed on several stores at once: each answers under its
/// own prefix (cleaned up after), and the stores are compared side by side:
/// how many answers differ from the recording, and each kind's p95.
async fn replay_many(s: &Arc<Sonde>, a: &ReplayArgs) -> anyhow::Result<i32> {
    use sonde_proxy::replay::{self as rp, ReplayOptions};
    if let Some(why) = output::unsupported(
        a.format,
        "replay",
        &[Format::Table, Format::Json, Format::Markdown, Format::Csv],
    ) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    let (asked, original) = super::sessions::open(s, &a.file)?;
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
        "Replay {} from {} on {} at once, under {prefix} on each ({}, {}; deleted afterwards unless --keep).",
        output::count(plan.requests, "request"),
        asked.display(),
        a.endpoint.join(", "),
        output::count(plan.writes, "write"),
        output::count(plan.deletes, "delete")
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
    let mut failed = 0;
    for (id, path, run) in runs {
        let run = match run {
            Ok(r) => r,
            Err(e) => {
                eprintln!("sonde: {id}: {e}");
                failed += 1;
                continue;
            }
        };
        differ_total += run.comparison.differ();
        if run.remaining > 0 || run.unaborted_uploads > 0 {
            eprintln!(
                "sonde: {id}: {} and {} still under {prefix}: the store would not remove what the replay wrote",
                output::count(run.remaining, "object or version")
                    .replace("object or versions", "objects or versions"),
                output::count(run.unaborted_uploads, "unfinished upload"),
            );
            failed += 1;
        }
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
    Ok(replay_exit(failed, differ_total))
}

/// What a replay on several stores exits with: a store it could not be
/// replayed on is a failure (70), whatever the others answered; answers
/// that differ are 2.
fn replay_exit(failed: usize, differ: usize) -> i32 {
    if failed > 0 {
        70
    } else if differ > 0 {
        2
    } else {
        0
    }
}

/// The same workload on two stores: each kind's p50 and p95 on each, slowest change first.
fn print_latency(a: &[Record], b: &[Record], names: (&str, &str), prices: &RequestPrices) {
    print_latency_of(
        &sonde_proxy::profile::latency_changes(&profile(a, prices), &profile(b, prices)),
        names,
    );
}

/// [`print_latency`] of the changes reckoned already.
fn print_latency_of(changes: &[sonde_proxy::profile::LatencyChange], names: (&str, &str)) {
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
    if let Some(why) = output::unsupported(a.format, "diff", &[Format::Table, Format::Json]) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    // read one after the other, each for what it amounts to: the replay
    // kept as its answers and its profile, then the session read against them
    let original_file = super::sessions::resolve(s, &a.original);
    let prices = sonde_proxy::session::read_header(&original_file)
        .map(|h| request_prices_for(s, &h.endpoint))
        .unwrap_or_default();
    let (replay_header, answers, is_replay, replay_profile) = {
        let (_, replay) = super::sessions::open_lean(s, &a.replay)?;
        (
            replay.header,
            sonde_proxy::replay::answers(&replay.records),
            sonde_proxy::replay::is_replay(&replay.records),
            profile(&replay.records, &prices),
        )
    };
    let (_, original) = super::sessions::open_lean(s, &a.original)?;
    // two ordinary runs pair no request: "0 compared: 0 the same" would read as agreement
    // (a replay that sent nothing is still one: its header says what it is)
    let named_replay = replay_header
        .command
        .as_deref()
        .is_some_and(|c| c.starts_with("replay of "));
    if !named_replay && !is_replay {
        eprintln!("sonde: {}", not_a_replay(a, &original.records));
        return Ok(64);
    }
    let c = sonde_proxy::replay::compare_answers(&original.records, &answers, &[]);
    let latency = sonde_proxy::profile::latency_changes(
        &profile(&original.records, &prices),
        &replay_profile,
    );
    match a.format {
        Format::Json => println!(
            "{}",
            output::json(&serde_json::json!({
                "comparison": c,
                "latency": latency,
            }))
        ),
        _ => {
            print_comparison(&c, std::io::stdout().is_terminal());
            print_latency_of(
                &latency,
                (&original.header.endpoint, &replay_header.endpoint),
            );
        }
    }
    Ok(if c.differ() > 0 { 2 } else { 0 })
}

/// What `sonde diff` says of two sessions that are not a session and its
/// replay, and what compares them instead.
fn not_a_replay(a: &DiffArgs, original: &[reqlog::Record]) -> String {
    let (first, second) = (a.original.display(), a.replay.display());
    if sonde_proxy::replay::is_replay(original) {
        return format!(
            "{second} is not a replay, {first} is: the session comes first (sonde diff {second} {first})"
        );
    }
    format!(
        "these are not a session and its replay: {second} was not made by `sonde replay`. Two runs of a program are compared with `sonde compare {first} {second}`"
    )
}

/// A figure before and after, with how much it moved: `28 (no change)`,
/// `28 → 31 (+11%)`. A move too small to show as a percent is not `+0%`.
fn change(a: f64, b: f64, text: impl Fn(f64) -> String) -> String {
    let (before, after) = (text(a), text(b));
    if a == b || before == after {
        return format!("{before} (no change)");
    }
    let pct = (b - a) * 100.0 / a;
    let pct = if a <= 0.0 {
        String::new()
    } else if pct.abs() < 0.5 {
        " (under 1%)".to_string()
    } else {
        format!(" ({pct:+.0}%)")
    };
    format!("{before} → {after}{pct}")
}

/// What `compare`'s table of kinds leaves out: `cut` that changed past its
/// rows, `same` that did not change. `show` says its own rest the same way.
fn more_kinds(cut: usize, same: usize) -> Option<String> {
    let kinds = |n: usize| if n == 1 { "kind" } else { "kinds" };
    match (cut, same) {
        (0, 0) => None,
        (0, s) => Some(format!(
            "  … and {s} more {} of request, unchanged",
            kinds(s)
        )),
        (c, 0) => Some(format!("  … and {c} more {} of request", kinds(c))),
        (c, s) => Some(format!(
            "  … and {} more {} of request, {s} of them unchanged",
            c + s,
            kinds(c + s)
        )),
    }
}

/// `sonde compare`: two runs of a program, before and after a change.
pub async fn compare(s: &Arc<Sonde>, a: &CompareArgs) -> anyhow::Result<i32> {
    if let Some(why) = output::unsupported(a.format, "compare", &[Format::Table, Format::Json]) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    // one run read, reckoned and let go before the other is read: two runs
    // cost what the larger does, not both at once
    let run = |f: &PathBuf| -> anyhow::Result<_> {
        let (_, session) = super::sessions::open_lean(s, f)?;
        let prices = request_prices_for(s, &session.header.endpoint);
        let run = sonde_proxy::profile::Run::analysed(&session.header, &session.records, &prices);
        Ok((session.header, run))
    };
    let (ha, ra) = run(&a.before)?;
    let (hb, rb) = run(&a.after)?;
    let c = sonde_proxy::profile::compare_runs(&ra, &rb).of_programs(
        sonde_proxy::program::key_of(&ha).as_deref(),
        sonde_proxy::program::key_of(&hb).as_deref(),
    );
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
    // nothing moved: said once, before the figures that show it
    let unchanged = c.requests.0 == c.requests.1
        && c.errors.0 == c.errors.1
        && ms_text(c.busy_ms.0) == ms_text(c.busy_ms.1)
        && c.cost.0 == c.cost.1
        && c.resolved.is_empty()
        && c.appeared.is_empty()
        && c.groups.iter().all(|g| g.requests.0 == g.requests.1);
    if unchanged {
        println!(
            "{}",
            paint(
                "no change: the same requests, waiting, errors, cost and findings",
                "1",
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
    // the kinds that changed, the largest change first; the rest counted
    const ROWS: usize = 10;
    let changed: Vec<&sonde_proxy::profile::GroupChange> = c
        .groups
        .iter()
        .filter(|g| g.requests.0 != g.requests.1 || (g.busy_ms.0 - g.busy_ms.1).abs() >= 1.0)
        .collect();
    let rows: Vec<Vec<String>> = changed
        .iter()
        .take(ROWS)
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
        if let Some(more) = more_kinds(changed.len() - rows.len(), c.groups.len() - changed.len()) {
            println!("{more}");
        }
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

/// A fault scenario of the project's own, a `[[scenario]]` of `sonde.toml`:
/// `fault = "503 SlowDown"` (or `500`, `lost-answer`, `cut-body`,
/// `lost-race`, `delay 200ms`), `on`
/// the operations it is for, `prefix` the keys, `times` the attempts at each
/// request that meet it (1 unless said; 0 is every attempt).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ProjectScenario {
    name: String,
    fault: String,
    #[serde(default)]
    on: Vec<String>,
    #[serde(default)]
    prefix: String,
    times: Option<u32>,
}

impl ProjectScenario {
    /// Checked and built where the app's are ([`sonde_proxy::fault::project_scenario`]).
    fn scenario(&self) -> Result<sonde_proxy::fault::Scenario, String> {
        sonde_proxy::fault::project_scenario(
            &self.name,
            &self.fault,
            &self.on,
            &self.prefix,
            self.times,
        )
    }
}

/// What a project says of its program: the name its runs go by.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ProjectProgram {
    name: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ProjectFile {
    #[serde(default)]
    check: ProjectCheck,
    #[serde(default)]
    program: ProjectProgram,
    #[serde(default)]
    scenario: Vec<ProjectScenario>,
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

/// A duration on the command line, as `sonde.toml` writes one.
fn duration_arg(text: &str) -> Result<f64, String> {
    duration_text(text)
        .ok_or_else(|| format!("{text:?} is not a time (2s, 250ms, or ms as a number)"))
}

/// "250ms", "2s", "1.5 s", or ms as a number.
fn duration_text(text: &str) -> Option<f64> {
    let t = text.trim();
    let (n, scale) = if let Some(n) = t.strip_suffix("ms") {
        (n, 1.0)
    } else if let Some(n) = t.strip_suffix('s') {
        (n, 1000.0)
    } else {
        (t, 1.0)
    };
    n.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| v * scale)
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
            duration_text(s).ok_or_else(|| super::usage(format!("{name}: {s:?} is not a duration")))
        }
        other => Err(super::usage(format!("{name}: {other} is not a duration"))),
    }
}

/// `sonde check --init`: a `sonde.toml` whose `[check]` this run passes,
/// with room to grow, and the CI step that holds every run to it.
fn init_project(
    a: &CheckArgs,
    run: &sonde_proxy::program::RunPoint,
    header: &SessionHeader,
) -> anyhow::Result<i32> {
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
    // a program without a name goes by its whole command: another argument
    // and it is another program, with no baseline. One is written for it
    let named = header.name.as_deref().is_some_and(|n| !n.trim().is_empty());
    let name = (!named)
        .then(|| header.command.as_deref().and_then(name_for))
        .flatten();
    let mut text = [
        "# sonde check: what a run of this program may use against S3.".to_string(),
        format!(
            "# Made from a run of {}; edit freely.",
            output::count(run.requests, "request")
        ),
        "[check]".into(),
        format!("max-requests = {}", (run.requests as f64 * grow).ceil() as u64),
        // time moves from one identical run to the next: a limit made from
        // one run fails the next one in two. Left for a person to set
        format!(
            "# max-waiting = \"{waiting}ms\"   # measured {}: time varies run to run, set it with room",
            ms_text(run.busy_ms)
        ),
        format!(
            "# max-p95 = \"{:.0}ms\"   # measured {}",
            (run.p95_ms * 2.0).ceil().max(1.0),
            ms_text(run.p95_ms)
        ),
        format!("max-cost = {cost}"),
        format!("max-errors = {}", run.errors),
        "# against a baseline run: how much more it may use, in percent".into(),
        format!("tolerance = {tolerance}"),
        "# baseline = \"auto\"   # the baseline set for the program (in the app, or --set-baseline)".into(),
        "# baseline = \"https://…/main.sonde\"   # or the main branch's run, kept by CI".into(),
        String::new(),
    ]
    .join("\n");
    if let Some(name) = &name {
        text.push_str(&format!(
            "\n# the name this program's runs and its baseline go by, whatever its arguments\n[program]\nname = \"{name}\"\n"
        ));
    }
    std::fs::write(&path, text)?;
    eprintln!(
        "wrote {}: a [check] this run passes ({}, {} of room)",
        path.display(),
        output::count(run.requests, "request"),
        format_args!("{tolerance}%")
    );
    if let Some(name) = &name {
        eprintln!(
            "  and [program] name = \"{name}\": later runs (`sonde run`, here) and their baseline go by it, not by the command's arguments"
        );
    }
    eprintln!("  no trials: `sonde run --trial` tries the program under faults");
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

/// A name for a program from its command: the script an interpreter runs
/// (`etl` of `python etl.py --day 2`), else the program itself. Letters,
/// digits, `-` and `_`.
fn name_for(command: &str) -> Option<String> {
    const INTERPRETERS: &[&str] = &[
        "python", "python3", "node", "ruby", "java", "bash", "sh", "deno", "bun", "perl", "php",
        "uv", "poetry", "npx", "dotnet",
    ];
    let program = sonde_proxy::program::program_name(command)?;
    let stem = |f: &str| {
        let name = f.rsplit(['/', '\\']).next().unwrap_or(f);
        name.split('.').next().unwrap_or(name).to_string()
    };
    let base = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    let mut name = stem(&program);
    if INTERPRETERS.contains(&base) || INTERPRETERS.contains(&program.as_str()) {
        // what it runs: the first word after it that is a file, not an option
        let after = command
            .split_whitespace()
            .skip_while(|w| !w.ends_with(&program));
        if let Some(script) = after
            .skip(1)
            .find(|w| !w.starts_with('-') && w.contains('.') && !w.contains('='))
        {
            name = stem(script);
        }
    }
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-').to_string();
    (!name.is_empty()).then_some(name)
}

/// The runs of `key` up to `upto` (oldest first, 30 at most), each with
/// the commit it recorded: what [`sonde_proxy::program::since`] walks.
fn program_runs(
    s: &Sonde,
    key: &str,
    upto: (&Path, &SessionHeader, &sonde_proxy::program::RunPoint),
) -> Vec<(
    sonde_proxy::program::RunPoint,
    Option<sonde_proxy::session::GitStamp>,
)> {
    let dir = s.paths().sessions();
    let (upto, upto_header, upto_point) = upto;
    let upto_created = upto_header.created;
    let mut found: Vec<(chrono::DateTime<chrono::Utc>, PathBuf)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sonde"))
        .filter_map(|p| {
            let h = sonde_proxy::session::read_header(&p)?;
            (sonde_proxy::program::key_of(&h).as_deref() == Some(key) && h.created <= upto_created)
                .then_some((h.created, p))
        })
        .collect();
    found.sort();
    // the run checked is the newest, wherever its file is
    found.retain(|(_, p)| p.canonicalize().ok() != upto.canonicalize().ok());
    let from = found.len().saturating_sub(29);
    // an earlier run's figures are kept beside it once reckoned
    // (`<name>.summary.json`): thirty runs are thirty small files, not
    // thirty sessions read whole
    let mut runs: Vec<_> = found[from..]
        .iter()
        .filter_map(|(_, p)| {
            let endpoint = sonde_proxy::session::read_header(p)?.endpoint;
            let (counted, figures) =
                sonde_proxy::summary::figures(p, &request_prices_for(s, &endpoint))?;
            Some((figures.point, counted.git))
        })
        .collect();
    runs.push((upto_point.clone(), upto_header.git.clone()));
    runs
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
    if !url.starts_with("https://") {
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
    if let Some(why) = output::unsupported(
        a.format,
        "check",
        &[
            Format::Table,
            Format::Json,
            Format::Markdown,
            Format::Github,
        ],
    ) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    use sonde_proxy::program::{Budget, check, run_point};
    // the project's budget, which the flags override
    // --init writes the file: there is none to read yet
    let loaded = if a.init {
        Project::default()
    } else {
        load_project(a.config.as_ref())?
    };
    let project_path = loaded.path.clone();
    let project = loaded.file.check;
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
    // read once, for what it amounts to: its figures, its findings, its header
    let read = |p: &PathBuf| -> anyhow::Result<_> {
        let session = super::sessions::read(p, sonde_proxy::session::Keep::Analysis)?;
        let Analysis {
            profile: pr,
            observations: ob,
            ..
        } = Analysis::of(&session, &request_prices_for(s, &session.header.endpoint));
        let point = run_point(
            &p.to_string_lossy(),
            &session.header.created.to_rfc3339(),
            &session.records,
            &pr,
            &ob,
        );
        // (its records are let go here: two sessions are never held at once)
        Ok((point, pr, ob, session.header))
    };
    if !file.exists() {
        return Err(super::usage(format!(
            "no session {}: not a file, and not a name in {} (`sonde sessions` lists them)",
            file.display(),
            s.paths().sessions().display()
        )));
    }
    let run = read(&file)?;
    let header = run.3.clone();
    if a.init {
        return init_project(a, &run.0, &header);
    }
    let program = sonde_proxy::program::key_of(&header);
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
        // a damaged file is not written over: every other baseline would go
        let mut all = match sonde_proxy::program::read_baselines(&dir) {
            Ok(all) => all,
            Err(e) => {
                eprintln!("sonde: {e} (no baseline was set)");
                return Ok(64);
            }
        };
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        all.insert(key.clone(), name);
        sonde_proxy::program::write_baselines(&dir, &all)?;
        eprintln!("baseline for `{key}`: {}", file.display());
    }
    let mut no_baseline: Option<String> = None;
    let base_path = match baseline_arg.as_deref() {
        None => None,
        Some(_) if fetched.is_some() => fetched.as_ref().map(|p| p.to_path_buf()),
        Some("auto") => {
            // damaged is not "none set": the run would go unchecked
            let all = match sonde_proxy::program::read_baselines(&dir) {
                Ok(all) => all,
                Err(e) => {
                    eprintln!("sonde: {e}");
                    return Ok(64);
                }
            };
            let found = program
                .as_ref()
                .and_then(|k| all.get(k).map(|n| dir.join(n)).filter(|p| p.is_file()));
            if found.is_none() {
                // no baseline yet is not a mistake: a first run in CI has
                // none. The budget is checked, and this is said
                no_baseline = Some(no_baseline_text(program.as_deref(), &header, &all));
            }
            found
        }
        Some(p) => Some(super::sessions::resolve(s, &PathBuf::from(p))),
    };
    // the baseline: its figures and its findings
    let base = base_path.as_ref().map(read).transpose()?;
    if let Some(why) = &no_baseline {
        eprintln!("{why}");
    }
    // both hold a run to its baseline: without one they would check nothing
    if base.is_none() && no_baseline.is_none() && (a.tolerance.is_some() || a.no_new_warnings) {
        eprintln!(
            "{} needs a baseline: add --baseline FILE|auto|URL (or `baseline` in sonde.toml)",
            if a.no_new_warnings {
                "--no-new-warnings"
            } else {
                "--tolerance"
            }
        );
        return Ok(64);
    }
    if let Some(b) = &base {
        let bp = sonde_proxy::program::key_of(&b.3);
        if bp != program {
            eprintln!(
                "note: the baseline ran `{}`, this run `{}`",
                bp.as_deref().unwrap_or("no command"),
                program.as_deref().unwrap_or("no command")
            );
        }
    }
    // a mistake in sonde.toml names the file it is in
    let in_project = |e: anyhow::Error| match &project_path {
        Some(p) => super::usage(format!("{}: {e}", p.display())),
        None => e,
    };
    let budget = Budget {
        max_requests: a.max_requests.or(project.max_requests),
        max_waiting_ms: match (a.max_waiting, &project.max_waiting) {
            (Some(v), _) => Some(v),
            (None, Some(v)) => Some(waiting_ms(v).map_err(in_project)?),
            (None, None) => None,
        },
        max_cost: a.max_cost.or(project.max_cost),
        max_errors: a.max_errors.or(project.max_errors),
        max_p95_ms: match (a.max_p95, &project.max_p95) {
            (Some(v), _) => Some(v),
            (None, Some(v)) => Some(duration_ms("max-p95", v).map_err(in_project)?),
            (None, None) => None,
        },
        expect: project.expect.clone(),
        tolerance: base
            .as_ref()
            .map(|_| a.tolerance.or(project.tolerance).unwrap_or(10.0) / 100.0),
        no_new_warnings: a.no_new_warnings || project.no_new_warnings,
    };
    // (a run is held by its figures and its findings: `check` reads no
    // record of it, nor of its baseline)
    let c = check(
        (&run.0, &[][..], &run.1, &run.2),
        base.as_ref().map(|b| (&b.0, &[][..], &b.1, &b.2[..])),
        &budget,
    );
    // what the run's last trial found: a program that broke fails the
    // check, with a budget or without one
    use sonde_proxy::program::{check_closing, check_passed, trial_mark};
    let trials = sonde_proxy::trial::load(&file);
    // `c.passed` is of its rows; the whole check also fails on a trial that broke
    let passed = check_passed(&c, &trials);
    let closing = match &no_baseline {
        // said in every format: the rows held to a baseline are not there
        Some(_) if !c.verdicts.is_empty() => {
            format!("{} (no baseline yet)", check_closing(&c, &trials))
        }
        _ => check_closing(&c, &trials),
    };
    // setting a baseline alone is a whole command
    if c.verdicts.is_empty() && trials.is_empty() && a.set_baseline {
        return Ok(0);
    }
    // nothing to hold the run to is a mistake in the command, in every format
    if c.verdicts.is_empty() && trials.is_empty() {
        // with `baseline = "auto"` and no baseline yet there is nothing to
        // hold the run to, and nothing wrong with it: the first run passes
        if no_baseline.is_some() {
            println!("nothing checked: no budget, and no baseline yet");
            return Ok(0);
        }
        eprintln!(
            "nothing to check: give a budget (--max-requests, --max-waiting, --max-cost, --max-errors, --max-p95, or sonde.toml's [check]) or a --baseline, or try the program (`sonde run --trial`)"
        );
        return Ok(64);
    }
    // which program this is, and what it goes by: a baseline is a program's
    let named = match (&program, header.name.as_deref().map(str::trim)) {
        (Some(p), Some(n)) if !n.is_empty() => format!("program {p} (its name)"),
        (Some(p), _) => format!(
            "program `{p}` (its command; a name of its own: `name` under [program] in sonde.toml, or `sonde run --name`)"
        ),
        (None, _) => "no program (the session ran no command)".to_string(),
    };
    // past the baseline: since which run, and the commits around it
    let since = match (&base, &program) {
        (Some(b), Some(key)) if !c.passed => {
            let runs = program_runs(s, key, (&file, &header, &run.0));
            sonde_proxy::program::since(&runs, &b.0, budget.tolerance.unwrap_or(0.1))
        }
        _ => None,
    };
    let since_text = since.as_ref().map(since_line);
    if matches!(a.format, Format::Json) {
        let mut doc = serde_json::json!({
            "run": run.0,
            "program": program,
            "programNamed": header.name.is_some(),
            "check": c,
            "trials": trials,
            "passed": passed,
            "verdict": closing,
            "since": since,
        });
        // `baseline = "auto"` found none: the rows held to one are not there
        if let Some(why) = &no_baseline {
            doc["noBaseline"] = serde_json::json!(why);
        }
        println!("{}", output::json(&doc));
        return Ok(if passed { 0 } else { 1 });
    }
    let color = std::io::stdout().is_terminal();
    let judged = session_line(&file, Some(&header));
    let shown = |what: &str, v: f64| match what {
        "waiting" | "p95" => ms_text(v),
        "cost" => money_text(v, &run.0.currency),
        _ => format!("{v:.0}"),
    };
    // GitHub Actions: each verdict that failed on the run, each finding on
    // the line of code that sent its requests
    if matches!(a.format, Format::Github) {
        // since when it is past its baseline: said once, on the first row
        // the baseline failed
        let mut since_said = false;
        for v in c.verdicts.iter().filter(|v| !v.ok) {
            let since = match &since_text {
                Some(t) if v.against == "baseline" && !since_said => {
                    since_said = true;
                    format!(" · {t}")
                }
                _ => String::new(),
            };
            println!(
                "::error title=sonde check::{}",
                gh_escape(&format!(
                    "{} {} > {} ({}, {}){since}",
                    v.label(),
                    shown(v.what, v.value),
                    shown(v.what, v.limit),
                    v.over().unwrap_or_default(),
                    v.against,
                ))
            );
        }
        for t in trials
            .iter()
            .filter(|t| t.standing == sonde_proxy::trial::Standing::Broke)
        {
            println!(
                "::error title=sonde check::{}",
                gh_escape(&format!("breaks under {}: {}", t.scenario, t.line()))
            );
        }
        for t in trials
            .iter()
            .filter(|t| t.standing == sonde_proxy::trial::Standing::Degraded)
        {
            println!(
                "::warning title=sonde check::{}",
                gh_escape(&format!("gets through {}: {}", t.scenario, t.line()))
            );
        }
        for w in &c.new_warnings {
            println!(
                "::warning title=sonde: new since the baseline::{}",
                gh_escape(w)
            );
        }
        for f in run.2.iter().take(10) {
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
        println!("sonde: {closing} · {judged}");
        return Ok(if passed { 0 } else { 1 });
    }
    // a pull request's comment: the verdicts, what is new, and the run's findings
    if matches!(a.format, Format::Markdown) {
        print!(
            "{}",
            sonde_proxy::program::check_markdown_with(
                &c,
                &run.0.currency,
                since_text.as_deref(),
                &run.2,
                &trials
            )
        );
        println!("\n*{}*", sonde_proxy::program::md_text(&judged));
        return Ok(if passed { 0 } else { 1 });
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
                v.label().to_string(),
                shown(v.what, v.value),
                format!("≤ {}", shown(v.what, v.limit)),
                v.against.to_string(),
                v.over().unwrap_or_default(),
            ]
        })
        .collect();
    // which run this is about comes first: "the newest" is not a name
    println!("{judged}");
    println!("{named}");
    if let (Some(b), Some(base)) = (&base_path, &base) {
        println!("against {}", session_line(b, Some(&base.3)));
    }
    if !rows.is_empty() {
        println!(
            "{}",
            output::table(&["", "", "run", "limit", "from", "over"], &rows)
        );
    }
    for t in &trials {
        use sonde_proxy::trial::Standing;
        println!(
            "{}  {} {}{}",
            paint(
                trial_mark(t.standing),
                match t.standing {
                    Standing::Broke => "31",
                    Standing::Degraded => "33",
                    Standing::Untested => "2",
                    Standing::Survived => "32",
                },
                color
            ),
            match t.standing {
                Standing::Survived => "survives",
                Standing::Degraded => "gets through",
                Standing::Broke => "breaks under",
                Standing::Untested => "untested:",
            },
            t.asked(),
            if t.line().is_empty() {
                String::new()
            } else {
                format!(" · {}", t.line())
            }
        );
    }
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
        paint(&closing, if passed { "32" } else { "31" }, color)
    );
    Ok(if passed { 0 } else { 1 })
}

/// Why `baseline = "auto"` found none: which program this is and, when a
/// baseline is set for the same program run with other arguments, that one
/// and how to give the program a name the two share.
fn no_baseline_text(
    program: Option<&str>,
    header: &SessionHeader,
    baselines: &std::collections::BTreeMap<String, String>,
) -> String {
    let Some(key) = program else {
        return "no baseline yet: this session ran no command, so it is no program's run"
            .to_string();
    };
    let mut text = format!(
        "no baseline yet for program `{key}`: set one with `sonde check FILE --set-baseline` or in the app"
    );
    // the same executable and script under another key: its arguments differ
    let named = header.name.as_deref().is_some_and(|n| !n.trim().is_empty());
    let of = |command: &str| {
        (
            sonde_proxy::program::program_name(command),
            name_for(command),
        )
    };
    if !named
        && let Some(other) = baselines
            .keys()
            .find(|k| k.as_str() != key && of(k) == of(key) && of(key).0.is_some())
    {
        let name = name_for(key).unwrap_or_else(|| "NAME".into());
        text.push_str(&format!(
            "\n  there is one for `{other}`: the same program with other arguments. A program without a name goes by its whole command; give it one and its runs share a baseline: `name = \"{name}\"` under [program] in sonde.toml, or `sonde run --name {name}`"
        ));
    }
    text
}

/// A session by name and time, as `sonde check` says which run it judged:
/// `session 20261110-091555-etl.sonde · recorded 2026-11-10 09:15`, the time
/// as `sonde sessions` lists it.
fn session_line(file: &Path, header: Option<&sonde_proxy::session::SessionHeader>) -> String {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.display().to_string());
    let read;
    let header = match header {
        Some(h) => Some(h),
        None => {
            read = Session::open(file).ok().map(|s| s.header);
            read.as_ref()
        }
    };
    match header {
        Some(h) => format!(
            "session {name} · recorded {}",
            h.created
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
        ),
        None => format!("session {name}"),
    }
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
            all: false,
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
        async move { app.send_raw(r).await.ok() }
    };
    let on =
        |op: &str, method: http::Method, k: &str| S3Request::new(op, method).bucket("demo").key(k);
    let put = |k: &str, body: Vec<u8>| on("PutObject", http::Method::PUT, k).body(Body::from(body));
    // what a small export job does. it looks for its settings where they might be
    for name in ["sonde-tour.toml", "tour.toml", "tour.yaml", "tour.json"] {
        send(on(
            "HeadObject",
            http::Method::HEAD,
            &format!("tour/{name}"),
        ))
        .await;
    }
    // reads the year's reports, asking first whether each is there
    for q in 1..=3 {
        let k = format!("reports/2026-q{q}.csv");
        send(on("HeadObject", http::Method::HEAD, &k)).await;
        send(on("GetObject", http::Method::GET, &k)).await;
    }
    // uploads the export in parts
    let export = "tour/export.csv";
    let created =
        send(on("CreateMultipartUpload", http::Method::POST, export).query("uploads", "")).await;
    if let Some(id) = created.and_then(|r| s3raw::xml::initiate_multipart_upload(&r.text()).ok()) {
        let mut parts = Vec::new();
        for n in 1..=5u32 {
            let part = send(
                on("UploadPart", http::Method::PUT, export)
                    .query("partNumber", n.to_string())
                    .query("uploadId", &id.upload_id)
                    .body(Body::from(vec![b'0' + n as u8; 64 * 1024])),
            )
            .await;
            let etag = part.and_then(|r| r.header("etag").map(String::from));
            parts.push(format!(
                "<Part><PartNumber>{n}</PartNumber><ETag>{}</ETag></Part>",
                s3raw::xml::escape(&etag.unwrap_or_default())
            ));
        }
        send(
            on("CompleteMultipartUpload", http::Method::POST, export)
                .query("uploadId", &id.upload_id)
                .body(Body::from(format!(
                    "<CompleteMultipartUpload>{}</CompleteMultipartUpload>",
                    parts.concat()
                ))),
        )
        .await;
    }
    // writes the same rows as a Delta table: the log read, three files, one commit
    let table = "tour/table";
    send(
        S3Request::new("ListObjectsV2", http::Method::GET)
            .bucket("demo")
            .query("list-type", "2")
            .query("prefix", format!("{table}/_delta_log/")),
    )
    .await;
    send(on(
        "GetObject",
        http::Method::GET,
        &format!("{table}/_delta_log/_last_checkpoint"),
    ))
    .await;
    let mut adds = String::from("{\"protocol\":{\"minReaderVersion\":1,\"minWriterVersion\":2}}\n");
    for i in 0..3 {
        let name = format!("part-{i:05}-5f0c2a1e-tour-c000.snappy.parquet");
        send(put(&format!("{table}/{name}"), vec![b'P'; 8 * 1024])).await;
        adds.push_str(&format!(
            "{{\"add\":{{\"path\":\"{name}\",\"size\":8192,\"dataChange\":true}}}}\n"
        ));
    }
    send(
        put(
            &format!("{table}/_delta_log/{:020}.json", 0),
            adds.into_bytes(),
        )
        .header("if-none-match", "*"),
    )
    .await;
    // the store is busy for this one (the demo's store says so once): the client tries again
    send(put(
        sonde_core::demo::BUSY_ONCE,
        br#"{"rows":36,"files":3}"#.to_vec(),
    ))
    .await;
    // and lists what it wrote, a few keys a page
    let mut token: Option<String> = None;
    for _ in 0..20 {
        let page = send(
            S3Request::new("ListObjectsV2", http::Method::GET)
                .bucket("demo")
                .query("list-type", "2")
                .query("prefix", "tour/")
                .query("max-keys", "2")
                .query_opt("continuation-token", token.take()),
        )
        .await;
        token = page
            .and_then(|r| s3raw::xml::list_objects_v2(&r.text()).ok())
            .and_then(|l| l.next_continuation_token);
        if token.is_none() {
            break;
        }
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
            all: false,
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
#[serde(from = "LogsMarkFile")]
struct LogsMark {
    watermark: chrono::DateTime<chrono::Utc>,
    /// Each file read, with when it was written: what its forgetting goes by.
    seen: std::collections::BTreeMap<String, chrono::DateTime<chrono::Utc>>,
}

/// A mark as its file holds it: `seen` with each file's time, or (written
/// before 13.0) the names alone.
#[derive(serde::Deserialize)]
struct LogsMarkFile {
    watermark: chrono::DateTime<chrono::Utc>,
    seen: SeenFiles,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum SeenFiles {
    Timed(std::collections::BTreeMap<String, chrono::DateTime<chrono::Utc>>),
    Named(std::collections::BTreeSet<String>),
}

impl From<LogsMarkFile> for LogsMark {
    fn from(f: LogsMarkFile) -> Self {
        LogsMark {
            watermark: f.watermark,
            seen: match f.seen {
                SeenFiles::Timed(m) => m,
                // no time kept: remembered until the mark has moved two hours on
                SeenFiles::Named(n) => n.into_iter().map(|k| (k, f.watermark)).collect(),
            },
        }
    }
}

/// How long before the mark a file is still listed, and so still remembered.
const LOGS_GRACE: chrono::Duration = chrono::Duration::hours(2);

impl LogsMark {
    /// Whether a file written at `at` is to be read: new enough to be looked
    /// at, and not read yet.
    fn wants(&self, key: &str, at: chrono::DateTime<chrono::Utc>) -> bool {
        at > self.watermark - LOGS_GRACE && !self.seen.contains_key(key)
    }

    /// A file read (or given up on): not read again.
    fn read(&mut self, key: &str, at: chrono::DateTime<chrono::Utc>) {
        self.seen.insert(key.to_string(), at);
    }

    /// Forgets the files too old to be listed again, and only those: one
    /// still within the grace would be read a second time.
    fn forget_old(&mut self) {
        let keep_after = self.watermark - LOGS_GRACE;
        self.seen.retain(|_, at| *at > keep_after);
    }
}

/// What `watch --logs --once` exits with: the listing failed, or files were
/// listed and not one could be downloaded (nothing was read: 70), the pass is
/// over the budget (1, as `sonde check`), or 0.
fn once_exit(listing_failed: bool, none_read: bool, over_budget: bool) -> i32 {
    if listing_failed || none_read {
        70
    } else {
        i32::from(over_budget)
    }
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
    // the last pass's outcome: what `--once` exits with
    let (mut listing_failed, mut none_read, mut over);
    loop {
        // the files written since the mark (less a grace for late ones), not read yet
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
                mark.wants(&o.key, at).then(|| (o.key.clone(), o.size, at))
            }));
            token = page.next_continuation_token.filter(|_| page.is_truncated);
            if token.is_none() {
                break Ok(());
            }
        };
        // a store that did not answer this time is asked again next time
        listing_failed = listed.is_err();
        over = false;
        if let Err(e) = listed {
            eprintln!(
                "{} listing {src}: {e}",
                paint("!", "33", std::io::stderr().is_terminal())
            );
            keys.clear();
        }
        let mut records = Vec::new();
        // of the files asked for: how many answered, how many did not
        let (mut downloaded, mut unread) = (0usize, 0usize);
        for (k, size, at) in &keys {
            // a log file is small; something this large is not one
            if *size > 256 << 20 {
                mark.read(k, *at);
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
                    unread += 1;
                    continue;
                }
            };
            downloaded += 1;
            let text = sonde_proxy::import::text_of(&r.body)
                .unwrap_or_else(|_| String::from_utf8_lossy(&r.body).into_owned());
            if let Ok(i) = sonde_proxy::import::import_counted(&text, k) {
                if i.malformed > 0 {
                    eprintln!(
                        "{} {k}: {} line{} could not be read",
                        paint("!", "33", std::io::stderr().is_terminal()),
                        i.malformed,
                        if i.malformed == 1 { "" } else { "s" }
                    );
                }
                records.extend(i.session.records);
            }
            mark.read(k, *at);
            if *at > mark.watermark {
                mark.watermark = *at;
            }
        }
        // every file failed: a pass that read nothing did not find nothing
        // (no permission to GET, say); one file gone among many is not that
        none_read = unread > 0 && downloaded == 0;
        if none_read {
            eprintln!(
                "{} none of the {unread} log file{} listed under {src} could be read",
                paint("!", "33", std::io::stderr().is_terminal()),
                if unread == 1 { "" } else { "s" }
            );
        }
        // the seen files old enough to be past the grace are forgotten
        mark.forget_old();
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
            // said here whether or not anything is told elsewhere
            if let Some(what) = over_budget(s, records) {
                over = true;
                let text = format!(
                    "sonde watch ({src}): {} is over budget: {what}",
                    path.display()
                );
                eprintln!(
                    "{} {text}",
                    paint("!", "33", std::io::stderr().is_terminal())
                );
                if let Some(url) = &a.alert {
                    sent.push(post_alert(url, text));
                }
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
    // a watch stopped by hand is done; a single pass answers for itself
    Ok(if a.once {
        once_exit(listing_failed, none_read, over)
    } else {
        0
    })
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

/// The project's budget (`sonde.toml`'s `[check]`), when there is one.
fn project_budget() -> Option<sonde_proxy::program::Budget> {
    let text = std::fs::read_to_string(find_project_file()?).ok()?;
    let p: ProjectCheck = toml::from_str::<ProjectFile>(&text).ok()?.check;
    Some(sonde_proxy::program::Budget {
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
    })
}

/// A finished file held to the project's budget: what went over, when
/// anything did.
fn over_budget(s: &Arc<Sonde>, records: &[Record]) -> Option<String> {
    let budget = project_budget()?;
    let endpoint = records.first().map(|r| r.endpoint.as_str()).unwrap_or("");
    let prof = profile(records, &request_prices_for(s, endpoint));
    let obs = observations(records, &episodes(records));
    let point = sonde_proxy::program::run_point("", "", records, &prof, &obs);
    over_limits(&point, &budget)
}

/// The same from a file's figures alone: without a baseline a budget's
/// limits are all on them (requests, waiting, cost, errors, p95).
fn over_limits(
    point: &sonde_proxy::program::RunPoint,
    budget: &sonde_proxy::program::Budget,
) -> Option<String> {
    let none = profile(&[], &RequestPrices::default());
    let c = sonde_proxy::program::check((point, &[], &none, &[]), None, budget);
    let over: Vec<String> = c
        .verdicts
        .iter()
        .filter(|v| !v.ok)
        .map(|v| match v.what {
            "cost" => format!(
                "cost {} > {}",
                money_text(v.value, &point.currency),
                money_text(v.limit, &point.currency)
            ),
            "waiting" | "p95" => format!("{} {} > {}", v.what, ms_text(v.value), ms_text(v.limit)),
            what => format!("{what} {:.0} > {:.0}", v.value, v.limit),
        })
        .collect();
    (!over.is_empty()).then(|| over.join(", "))
}

/// How many of a file's newest records `watch` reads for its warnings.
const WATCH_RECENT: usize = 50_000;

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
    // a file's records are on disk: held here are the figures its budget is
    // on and the newest records, which the warnings are read from
    let mut figures: Option<sonde_proxy::program::RunFigures> = None;
    let mut recent: VecDeque<Record> = VecDeque::new();
    let take = |figures: &mut Option<sonde_proxy::program::RunFigures>,
                recent: &mut VecDeque<Record>,
                r: Record| {
        figures
            .get_or_insert_with(|| {
                sonde_proxy::program::RunFigures::new(request_prices_for(s, &r.endpoint))
            })
            .add(&r);
        if recent.len() == WATCH_RECENT {
            recent.pop_front();
        }
        recent.push_back(r);
    };
    let over = |figures: Option<sonde_proxy::program::RunFigures>| {
        over_limits(&figures?.point(), &project_budget()?)
    };
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
                        take(&mut figures, &mut recent, r);
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
                let p95 = sonde_proxy::profile::percentile(&lat, 0.95);
                let line = format!(
                    "last minute: {n} requests ({:.1}/s) · {errors} errors · p95 {} · {} in this file",
                    n as f64 / 60.0,
                    ms_text(p95),
                    figures.as_ref().map_or(0, |f| f.requests())
                );
                if tty {
                    eprint!("\r\x1b[2K{line}");
                } else if seconds.is_multiple_of(30) {
                    eprintln!("{line}");
                }
                // what this file's requests say, every five seconds: each warning once
                if seconds.is_multiple_of(5) && !recent.is_empty() {
                    let recent = &*recent.make_contiguous();
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
                    sent.retain(|h| !h.is_finished());
                    if tty {
                        eprint!("\r\x1b[2K");
                    }
                    eprintln!("{} {}", paint("·", "2", tty), old_path.display());
                    // said here whether or not anything is told elsewhere
                    if let Some(over) = over(figures.take()) {
                        let text = format!("sonde watch: {} is over budget: {over}", old_path.display());
                        eprintln!("{} {text}", paint("!", "33", tty));
                        if let Some(url) = &a.alert {
                            sent.push(post_alert(url, text));
                        }
                    }
                    recent.clear();
                    warned.clear();
                    rotated = Instant::now();
                }
            }
        }
    }
    if tty {
        eprintln!();
    }
    // requests still in flight are recorded before the file is packed
    let Started { mut proxy, log, .. } = started;
    shut_down(&mut proxy, SHUTDOWN_GRACE).await;
    while let Ok(id) = rx.try_recv() {
        if let Some(r) = log.get(id) {
            take(&mut figures, &mut recent, r);
        }
    }
    log.set_sink(None);
    drop(proxy);
    finish(writer, &path);
    // the last file is held to the budget too
    if let Some(over) = over(figures.take()) {
        let text = format!("sonde watch: {} is over budget: {over}", path.display());
        eprintln!("{} {text}", paint("!", "33", tty));
        if let Some(url) = &a.alert {
            sent.push(post_alert(url, text));
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(10), futures::future::join_all(sent)).await;
    eprintln!("{}", path.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonde_proxy::program::md_text;

    #[test]
    fn compare_says_how_many_kinds_its_table_leaves_out() {
        assert_eq!(more_kinds(0, 0), None);
        assert_eq!(
            more_kinds(0, 1).as_deref(),
            Some("  … and 1 more kind of request, unchanged")
        );
        assert_eq!(
            more_kinds(3, 0).as_deref(),
            Some("  … and 3 more kinds of request")
        );
        assert_eq!(
            more_kinds(2, 5).as_deref(),
            Some("  … and 7 more kinds of request, 5 of them unchanged")
        );
    }
    #[test]
    fn an_input_past_its_limit_is_kept_in_a_file_and_read_again_whole() {
        use std::io::Read;
        let mut kept = KeptInput {
            limit: 10,
            ..Default::default()
        };
        kept.push(b"12345");
        kept.push(b"678");
        assert!(kept.file.is_none(), "small: in memory");
        assert!(matches!(kept.again(), Again::Bytes(b) if b == b"12345678"));
        kept.push(b"9abcdef");
        kept.push(b"gh");
        assert!(
            kept.file.is_some() && kept.head.is_empty(),
            "past it: a file"
        );
        // every run reads all of it, from its start
        for _ in 0..2 {
            let Again::File(mut f) = kept.again() else {
                panic!("kept in its file");
            };
            let mut all = String::new();
            f.read_to_string(&mut all).unwrap();
            assert_eq!(all, "123456789abcdefgh");
        }
        assert_eq!(KeptInput::default().limit, 8 << 20);
    }

    #[test]
    fn a_watched_file_is_held_to_the_budget_by_its_figures() {
        use sonde_proxy::program::{Budget, RunFigures};
        let mut f = RunFigures::new(RequestPrices::default());
        for i in 0..3 {
            f.add(&Record {
                id: i,
                method: "GET".into(),
                operation: "GetObject".into(),
                status: Some(200),
                latency_ms: 2000.0,
                ..Default::default()
            });
        }
        let budget = |max_requests, max_p95_ms| Budget {
            max_requests,
            max_p95_ms,
            ..Default::default()
        };
        assert_eq!(over_limits(&f.point(), &budget(Some(3), None)), None);
        assert_eq!(
            over_limits(&f.point(), &budget(Some(2), Some(500.0))).as_deref(),
            Some("requests 3 > 2, p95 2.0 s > 500 ms")
        );
    }

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
    fn a_figure_that_did_not_move_says_no_change_not_plus_zero_percent() {
        let n = |v: f64| format!("{v:.0}");
        assert_eq!(change(28.0, 28.0, n), "28 (no change)");
        assert_eq!(change(28.0, 31.0, n), "28 → 31 (+11%)");
        assert_eq!(change(31.0, 28.0, n), "31 → 28 (-10%)");
        // moved, by less than shows: neither +0% nor -0%
        assert_eq!(change(1000.0, 1001.0, n), "1000 → 1001 (under 1%)");
        assert_eq!(change(1001.0, 1000.0, n), "1001 → 1000 (under 1%)");
        // the same once written: 435.2 ms and 435.4 ms
        assert_eq!(change(435.2, 435.4, ms_text), "435 ms (no change)");
        assert_eq!(change(0.0, 3.0, n), "0 → 3");
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

    #[test]
    fn a_replay_that_failed_on_a_store_does_not_exit_as_if_all_agreed() {
        assert_eq!(replay_exit(0, 0), 0);
        assert_eq!(replay_exit(0, 3), 2);
        assert_eq!(replay_exit(1, 0), 70);
        assert_eq!(replay_exit(2, 3), 70);
    }

    #[test]
    fn a_single_pass_over_the_logs_answers_for_itself() {
        assert_eq!(once_exit(false, false, false), 0);
        assert_eq!(once_exit(false, false, true), 1);
        assert_eq!(once_exit(true, false, false), 70);
        assert_eq!(once_exit(true, false, true), 70);
        assert_eq!(once_exit(false, true, false), 70);
    }

    #[cfg(unix)]
    #[test]
    fn a_program_ended_by_a_signal_exits_as_a_shell_says_it() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::ExitStatus;
        // a wait status: the signal in its low bits, the exit code above
        assert_eq!(exit_code(ExitStatus::from_raw(9)), 137);
        assert_eq!(exit_code(ExitStatus::from_raw(15)), 143);
        assert_eq!(exit_code(ExitStatus::from_raw(3 << 8)), 3);
        assert_eq!(exit_code(ExitStatus::from_raw(0)), 0);
    }

    #[test]
    fn a_log_file_read_is_remembered_until_it_is_too_old_to_be_listed() {
        let t = |h: i64| chrono::DateTime::from_timestamp(1_770_000_000 + h * 3600, 0).unwrap();
        let mut mark = LogsMark {
            watermark: t(0),
            seen: Default::default(),
        };
        // more files than the old limit, all within the grace
        for i in 0..50_010 {
            mark.read(&format!("logs/{i:06}"), t(0));
        }
        // a pass with one new file, an hour on
        assert!(mark.wants("logs/new", t(1)));
        mark.read("logs/new", t(1));
        mark.watermark = t(1);
        mark.forget_old();
        assert_eq!(mark.seen.len(), 50_011);
        assert!(!mark.wants("logs/000000", t(0)), "read a second time");
        // two hours past them they are not listed, so not remembered
        mark.read("logs/later", t(3));
        mark.watermark = t(3);
        mark.forget_old();
        assert_eq!(mark.seen.len(), 1);
        assert!(!mark.wants("logs/000000", t(0)));
        // a mark written before 13.0 (names only) still reads
        let old: LogsMark =
            serde_json::from_str(r#"{"watermark":"2026-02-06T00:00:00Z","seen":["a","b"]}"#)
                .unwrap();
        assert_eq!(old.seen.len(), 2);
        assert!(!old.wants("a", old.watermark));
        let again: LogsMark = serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!(again.seen, old.seen);
    }

    #[tokio::test]
    async fn a_single_pass_that_could_not_list_the_logs_does_not_exit_0() {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap());
        sonde_core::demo::install(&s).await.unwrap();
        let src = "s3://no-such-bucket-of-logs/logs/";
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
            logs: Some(src.into()),
            every: "15m".into(),
            once: true,
        };
        assert_eq!(watch_logs(&s, &args, src).await.unwrap(), 70);
    }

    #[tokio::test]
    async fn a_proxy_stopped_with_a_request_in_flight_is_not_said_to_have_drained() {
        use s3raw::{ClientConfig, Credentials, S3Client, S3Request, StaticProvider};
        // a store that takes the request and never answers
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let store = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((sock, _)) = listener.accept().await {
                held.push(sock);
            }
        });
        let client = |url: &str, key: &str, secret: &str| {
            S3Client::new(
                ClientConfig::new("t", url, "us-east-1").unwrap(),
                Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
                RequestLog::default(),
            )
            .unwrap()
        };
        let mut proxy = Proxy::start(
            client(&store, "k", "s"),
            RequestLog::default(),
            ProxyConfig::default(),
        )
        .await
        .unwrap();
        let (key, secret) = proxy.client_keys();
        let app = client(&proxy.url(), key, secret);
        let asking = tokio::spawn(async move {
            let get = S3Request::new("GetObject", http::Method::GET)
                .bucket("b")
                .key("k")
                .no_retry();
            let _ = app.send_raw(get).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let grace = std::time::Duration::from_millis(200);
        assert!(!shut_down(&mut proxy, grace).await);
        assert!(not_drained(grace).contains("still in flight 0.2 s after"));
        asking.abort();

        // with nothing in flight it drains, and nothing is said
        let mut idle = Proxy::start(
            client(&store, "k", "s"),
            RequestLog::default(),
            ProxyConfig::default(),
        )
        .await
        .unwrap();
        assert!(shut_down(&mut idle, grace).await);
    }

    /// A store that lists two log files and refuses to hand over either.
    async fn store_that_lists_and_refuses_gets() -> std::net::SocketAddr {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    loop {
                        let mut chunk = [0u8; 4096];
                        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            match sock.read(&mut chunk).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                            }
                        }
                        let head = String::from_utf8_lossy(&buf).into_owned();
                        buf.clear();
                        let (status, body) = if head.contains("list-type=2") {
                            let now = chrono::Utc::now().to_rfc3339();
                            let file = |n: u8| {
                                format!(
                                    "<Contents><Key>logs/2026-02-06-00-00-4{n}-AB</Key><LastModified>{now}</LastModified><ETag>\"x\"</ETag><Size>300</Size></Contents>"
                                )
                            };
                            (
                                "200 OK",
                                format!(
                                    "<ListBucketResult><Name>lake</Name><Prefix>logs/</Prefix><KeyCount>2</KeyCount><IsTruncated>false</IsTruncated>{}{}</ListBucketResult>",
                                    file(0),
                                    file(1)
                                ),
                            )
                        } else {
                            (
                                "403 Forbidden",
                                "<Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>".to_string(),
                            )
                        };
                        let resp = format!(
                            "HTTP/1.1 {status}\r\ncontent-type: application/xml\r\ncontent-length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        if sock.write_all(resp.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn a_single_pass_that_could_read_none_of_the_files_listed_does_not_exit_0() {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap());
        let addr = store_that_lists_and_refuses_gets().await;
        let mut e = sonde_core::EndpointConfig::new(
            "refusing",
            "refusing",
            &format!("http://{addr}"),
            "us-east-1",
            sonde_core::Vendor::Minio,
        );
        e.path_style = true;
        e.credentials = sonde_core::CredentialSource::Static {
            access_key_id: "k".into(),
            secret_access_key: "s".into(),
        };
        s.add_transient_endpoint(e).unwrap();
        let src = "s3://lake/logs/";
        let args = WatchArgs {
            proxy: ProxyArgs {
                rec: Recording {
                    endpoint: Some("refusing".into()),
                    no_delete: false,
                    read_only: true,
                    out: None,
                },
                listen: "127.0.0.1:0".parse().unwrap(),
                allow_remote: false,
            },
            rotate: 60,
            alert: None,
            logs: Some(src.into()),
            every: "15m".into(),
            once: true,
        };
        assert_eq!(watch_logs(&s, &args, src).await.unwrap(), 70);
    }

    #[tokio::test]
    async fn a_baseline_is_fetched_over_https_only() {
        // this machine's own names too
        for url in ["http://localhost:8080/base.sonde", "http://127.0.0.1/b"] {
            let e = fetch_baseline(url).await.unwrap_err();
            assert!(e.to_string().contains("only https"), "{e}");
        }
        let e = fetch_baseline("http://example.com/base.sonde?sig=1")
            .await
            .unwrap_err();
        assert!(e.to_string().contains("only https"), "{e}");
        assert!(!e.to_string().contains("sig=1"));
    }
}
