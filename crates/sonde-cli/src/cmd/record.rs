//! Recording: what `sonde run`, `proxy` and `demo` share (the
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
pub use super::compare::{CompareArgs, DiffArgs, compare, diff_alias};
pub use super::demo::{DemoArgs, demo};
pub use super::proxy::{ProxyArgs, proxy};
pub use super::run::{RunArgs, run};
pub use super::show::{ShowArgs, show, show_file};
pub use super::watch::{WatchArgs, watch_alias};

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
    #[arg(long, conflicts_with = "policy")]
    pub(super) allow_writes: bool,
    /// Gone in 18.0 (`--policy no-delete`, the default): said, and refused.
    #[arg(long, hide = true)]
    pub(super) no_delete: bool,
    /// Gone in 18.0 (`--policy read-only`): said, and refused.
    #[arg(long, hide = true)]
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
        // one way to say it: --policy, and --allow-writes for short
        for (gone, is) in [(self.no_delete, "no-delete"), (self.read_only, "read-only")] {
            if gone {
                return Err(super::usage(format!(
                    "--{is} is gone in 18.0: say --policy {is}{}",
                    if is == "no-delete" {
                        " (the default)"
                    } else {
                        ""
                    }
                )));
            }
        }
        let flag = if self.allow_writes {
            Some(Policy::Allow)
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
    // what the flags say is read first: a flag that is gone is said before anything else
    let policy = r.policy()?;
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
    // under no-delete a program may write over what its earlier runs wrote:
    // kept beside its runs (a trial's runs share its clean run's)
    let written = of.written.clone().unwrap_or_default();
    if under.is_none()
        && let Some(program) = sonde_proxy::program::key_of(&header)
    {
        written.remember(&runs_dir(&path), &program, cfg.endpoint.as_str());
    }
    let writer = Arc::new(SessionWriter::create(&path, &header)?);
    let log = RequestLog::new(4096);
    log.set_sink(Some(writer.clone()));
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
            written,
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

/// The folder a session is in, where its program's runs are kept.
pub(super) fn runs_dir(session: &std::path::Path) -> PathBuf {
    match session.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
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

/// The kinds of request, in one table: how many, how many failed, how long
/// 95% took at most, the run's waiting and cost that were theirs. `all`
/// adds the median and the slowest, and every kind. Times not recorded (a
/// CloudTrail import) are "—"; so is the waiting of a log timed to the
/// second (`whole_seconds`), whose requests sonde spread over it.
pub(super) fn kinds_table(p: &Profile, all: bool, whole_seconds: bool) -> Vec<String> {
    use sonde_proxy::summary::FEW;
    if p.groups.is_empty() {
        return Vec::new();
    }
    const ROWS: usize = 8;
    let shown = if all { p.groups.len() } else { ROWS };
    // a percentile of a handful of requests is one of them: marked, when
    // others are of more
    let few = |g: &sonde_proxy::profile::Group| g.requests < FEW as usize;
    let mark = p.groups.iter().any(few) && !p.groups.iter().all(few);
    let timed = p.groups.iter().any(|g| g.max_ms > 0.0);
    let time = |ms: f64, star: &str| {
        if timed {
            format!("{}{star}", output::duration(ms))
        } else {
            "—".to_string()
        }
    };
    let rows: Vec<Vec<String>> = p
        .groups
        .iter()
        .take(shown)
        .map(|g| {
            let star = if mark && few(g) { "*" } else { "" };
            let mut row = vec![
                if g.missed {
                    format!("{} (not found)", g.operation)
                } else {
                    g.operation.clone()
                },
                output::grouped(g.requests as u64),
                if g.errors > 0 && !g.missed {
                    output::grouped(g.errors as u64)
                } else {
                    String::new()
                },
                time(g.p95_ms, star),
                if timed && !whole_seconds {
                    format!(
                        "{} ({:.0}%)",
                        output::duration(g.waited_ms),
                        g.share * 100.0
                    )
                } else {
                    "—".to_string()
                },
                if p.cost.amount > 0.0 {
                    share_text(g.cost / p.cost.amount)
                } else {
                    String::new()
                },
            ];
            if all {
                row.push(time(g.p50_ms, star));
                row.push(time(g.max_ms, ""));
            }
            row
        })
        .collect();
    let mut heads = vec!["kind", "requests", "errors", "p95", "waiting", "cost"];
    if all {
        heads.extend(["p50", "max"]);
    }
    let mut out = vec![output::table(&heads, &rows).to_string()];
    if p.groups.len() > shown {
        let more = p.groups.len() - shown;
        out.push(format!(
            "  … and {more} more {} of request (`--all` lists them)",
            if more == 1 { "kind" } else { "kinds" }
        ));
    }
    if mark {
        out.push(format!("  * of fewer than {FEW} requests"));
    }
    out
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
    print_summary_of(&analysis, &header, path, color, stdout, false, None);
}

/// What a session amounts to, in one fixed shape for `run`, `show` and
/// `demo`: the head line (requests, operations, the SDK, the store, the
/// time, the waiting and the cost), the findings first (each with its
/// saving, its line of code and its fix), what acting on them would leave,
/// one table of the kinds of request, and the change since the program's
/// run before. `all` adds the signals over the run and their tables.
pub(super) fn print_summary_of(
    a: &Analysis,
    header: &SessionHeader,
    path: &Path,
    color: bool,
    stdout: bool,
    all: bool,
    since: Option<&sonde_proxy::fixloop::SinceLast>,
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
    let p = &a.profile;
    let mut head = format!(
        "{} ({})",
        output::count(s.requests as usize, "request"),
        output::count(a.episodes.len(), "operation"),
    );
    if let Some(sdk) = &a.sdk {
        head.push_str(&format!(" from {sdk}"));
    }
    head.push_str(&format!(
        " to {} in {}",
        header.endpoint,
        output::duration(s.seconds * 1000.0)
    ));
    // (spread over whole seconds by sonde, waiting would be sonde's guess)
    if p.busy_ms > 0.0 && !header.whole_seconds {
        head.push_str(&format!(", waiting {}", output::duration(p.busy_ms)));
    }
    if p.cost.amount > 0.0 {
        head.push_str(&format!(
            ", cost ≈ {}",
            money_text(p.cost.amount, &p.cost.currency)
        ));
    }
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
    // a trial's faults are sonde's doing: said before the figures they are in
    if !a.injected.is_empty() {
        say(format!(
            "  a trial{}: {} met a fault sonde injected, so the errors and retries here are the trial's, not the store's",
            header
                .scenario
                .as_deref()
                .map(|s| format!(" under {s}"))
                .unwrap_or_default(),
            output::count(a.injected.len(), "request")
        ));
    }
    if header.whole_seconds {
        // where in its second a request began is not known
        say("  its times are to the second (an imported log): a second's requests are spread over it in the log's order, and how many were in flight at once, and so the waiting, is not known".into());
    }
    for o in &a.observations {
        for line in super::fixloop::finding_lines(o, a.sdk.as_deref(), color) {
            say(line);
        }
    }
    // acting on all of them, at once
    if let Some(w) = &a.what_if {
        say(paint(&w.line(), "32", color));
    }
    for line in kinds_table(p, all, header.whole_seconds) {
        say(line);
    }
    for (what, n) in &s.error_codes {
        say(format!("  {} × {what}", output::grouped(*n)));
    }
    if all {
        let mut signals = a.signals.clone();
        if header.whole_seconds {
            signals.in_flight.peak = 0;
        }
        for line in sonde_proxy::signals::text(&signals, color).lines() {
            say(line.to_string());
        }
    }
    if let Some(since) = since {
        for line in super::fixloop::since_lines(since, color) {
            say(line);
        }
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
/// `proxy`): what a bare `sonde show` or `sonde check` is about. A
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
