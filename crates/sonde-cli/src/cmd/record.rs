//! Recording: what `sonde run`, `proxy`, `watch` and `demo` share (the
//! recording's arguments, the proxy started, a session's summary). Each
//! command is a module of its own, named here as it was.

use sonde_proxy::program::money_text;
use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Args;
use reqlog::{Record, RequestLog};
use sonde_core::Sonde;
use sonde_core::guard::human_bytes;
use sonde_proxy::profile::{Profile, RequestPrices};
use sonde_proxy::project::Project;
use sonde_proxy::session::{SessionHeader, SessionWriter};
use sonde_proxy::view::Analysis;
use sonde_proxy::{Policy, Proxy, ProxyConfig};

use crate::output;

pub use super::check::{CheckArgs, check};
pub use super::compare::{CompareArgs, DiffArgs, compare, diff};
pub use super::demo::{DemoArgs, demo};
pub use super::proxy::{ProxyArgs, proxy};
pub use super::replay::{ReplayArgs, replay};
pub use super::run::{RunArgs, run};
pub use super::show::{ShowArgs, show, show_file};
pub use super::watch::{WatchArgs, watch};

#[derive(Args, Clone)]
pub struct Recording {
    /// The endpoint requests go to (default: --url, or the only one configured).
    #[arg(long, short = 'e')]
    pub(super) endpoint: Option<String>,
    /// What the program may do to the store: `no-delete` refuses what
    /// removes or replaces data (deletes, aborts, lifecycle rules, bucket
    /// policies, versioning suspended, a write over an object the recording
    /// did not write); `read-only` lets only GET and HEAD through; `allow`
    /// lets everything through. Default: `[record] policy` in sonde.toml,
    /// else no-delete.
    #[arg(long, value_name = "POLICY", value_parser = ["allow", "no-delete", "read-only"])]
    pub(super) policy: Option<String>,
    /// `--policy allow`: the program may delete and write over what it finds.
    #[arg(long, conflicts_with_all = ["policy", "no_delete", "read_only"])]
    pub(super) allow_writes: bool,
    /// `--policy no-delete`.
    #[arg(long, conflicts_with_all = ["policy", "read_only"])]
    pub(super) no_delete: bool,
    /// `--policy read-only`.
    #[arg(long, conflicts_with = "policy")]
    pub(super) read_only: bool,
    /// The session file (default: a new file in the data directory's sessions/).
    #[arg(long, short = 'o')]
    pub(super) out: Option<PathBuf>,
    /// Serve TLS too, for SDKs that require it: AWS_ENDPOINT_URL is https,
    /// and the program is given a certificate made on the first run to trust
    /// (AWS_CA_BUNDLE, SSL_CERT_FILE, NODE_EXTRA_CA_CERTS). Also `tls` under
    /// `[record]` in sonde.toml.
    #[arg(long)]
    pub(super) tls: bool,
}

pub(super) struct Started {
    pub(super) proxy: Proxy,
    pub(super) log: RequestLog,
    pub(super) writer: Arc<SessionWriter>,
    pub(super) path: PathBuf,
}

/// A trial run: the scenario it is put through, the clean run it is a trial
/// of, and the file it is recorded to.
pub(super) struct Under {
    pub(super) scenario: sonde_proxy::fault::Scenario,
    pub(super) of: String,
    pub(super) path: PathBuf,
}

/// What a recording is of, beside its command: the name the program goes
/// by, and the trial it is, if it is one.
#[derive(Default)]
pub(super) struct Of<'a> {
    pub(super) name: Option<String>,
    pub(super) under: Option<&'a Under>,
    /// What the runs before it wrote (a trial's clean run): no-delete lets
    /// them be written again.
    pub(super) written: Option<Arc<sonde_proxy::Written>>,
}

impl Recording {
    /// The policy asked for: the flags', else `[record] policy` of
    /// sonde.toml, else no-delete. Said when it lets the program delete.
    fn policy(&self) -> anyhow::Result<Policy> {
        let flag = if self.allow_writes {
            Some(Policy::Allow)
        } else if self.read_only {
            Some(Policy::ReadOnly)
        } else if self.no_delete {
            Some(Policy::NoDelete)
        } else {
            self.policy.as_deref().and_then(Policy::named)
        };
        if let Some(p) = flag {
            return Ok(p);
        }
        let project = load_project(None)?;
        if project.policy == Some(Policy::Allow)
            && let Some(path) = &project.path
        {
            eprintln!(
                "note: {} lets the program delete and write over objects ([record] policy = \"allow\")",
                path.display()
            );
        }
        Ok(project.policy.unwrap_or_default())
    }
}

pub(super) async fn start(
    s: &Arc<Sonde>,
    r: &Recording,
    listen: SocketAddr,
    command: Option<String>,
    allow_remote: bool,
) -> anyhow::Result<Started> {
    start_under(s, r, listen, command, allow_remote, &Of::default()).await
}

pub(super) async fn start_under(
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
    let policy = r.policy()?;
    let tls = if r.tls {
        let dir = s.paths().data_dir.join("tls");
        let made = sonde_proxy::tls::Tls::load_or_make(&dir)
            .map_err(|e| anyhow::anyhow!("cannot make the endpoint's certificate: {e}"))?;
        Some(Arc::new(made))
    } else {
        None
    };
    let proxy = Proxy::start(
        client,
        log.clone(),
        ProxyConfig {
            listen,
            policy,
            allow_remote,
            faults: under.map(|u| Arc::new(sonde_proxy::fault::Faults::new(&u.scenario))),
            tls,
            written: of.written.clone().unwrap_or_default(),
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
pub(super) fn follow(log: &RequestLog, to_stderr: bool) -> tokio::task::JoinHandle<()> {
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

pub(super) fn paint(s: &str, code: &str, color: bool) -> String {
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
pub(super) fn print_profile(p: &Profile, color: bool, whole: bool, say: &dyn Fn(String)) {
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

pub(super) fn print_summary(
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
pub(super) fn print_summary_of(
    a: &Analysis,
    header: &SessionHeader,
    path: &Path,
    color: bool,
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
    let s = &a.summary;
    let endpoint = &header.endpoint;
    // which SDK sent them, when its user agent says
    let from = a
        .sdk
        .as_deref()
        .map(|sdk| format!(" from {sdk}"))
        .unwrap_or_default();
    let mut head = format!(
        "{} ({}){from} to {endpoint} in {}",
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
pub(super) fn share_text(share: f64) -> String {
    let p = share * 100.0;
    if p > 0.0 && p < 0.1 {
        "<0.1%".into()
    } else if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

/// `sonde.toml` at `explicit`, or here or in a folder above. A file that
/// does not read, or says what sonde does not know, is a mistake in the
/// configuration (64): nothing of it is passed over in silence.
pub(super) fn load_project(explicit: Option<&PathBuf>) -> anyhow::Result<Project> {
    let found = explicit.cloned().or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|d| sonde_proxy::project::find(&d))
    });
    let Some(path) = found else {
        return Ok(Project::default());
    };
    sonde_proxy::project::read(&path).map_err(super::usage)
}

/// How long a stopping proxy waits for the requests in flight.
pub(super) const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Stops the proxy and waits `grace` for what is in flight. When requests
/// were still open after it, says so once: the session is short of them, and
/// nothing else would tell.
pub(super) async fn shut_down(proxy: &mut Proxy, grace: std::time::Duration) -> bool {
    let drained = proxy.shutdown(grace).await;
    if !drained {
        eprintln!("sonde: {}", not_drained(grace));
    }
    drained
}

pub(super) fn not_drained(grace: std::time::Duration) -> String {
    format!(
        "requests were still in flight {} s after the proxy stopped: the session does not have them",
        grace.as_secs_f64()
    )
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
