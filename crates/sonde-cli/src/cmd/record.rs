//! Recording: what `sonde run`, `proxy` and the tour share (the
//! recording's arguments, the proxy started, a run's summary). Each
//! command is a module of its own, named here as it was.

use sonde_analysis::program::money_text;
use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Args;
use reqlog::{Record, RequestLog};
use sonde_analysis::profile::{Profile, RequestPrices};
use sonde_analysis::project::Project;
use sonde_analysis::run::{RunHeader, RunWriter};
use sonde_analysis::view::Analysis;
use sonde_core::Sonde;
use sonde_core::guard::human_bytes;
use sonde_proxy::{Policy, Proxy, ProxyConfig};

use crate::output;

pub use super::check::{CheckArgs, check};
pub use super::compare::{CompareArgs, compare};
pub use super::proxy::{ProxyArgs, proxy};
pub use super::run::{RunArgs, run};
pub use super::show::{ShowArgs, show, show_file};

#[derive(Args, Clone)]
pub struct Recording {
    /// The endpoint requests go to (default: --url, or the only one configured).
    #[arg(long, short = 'e')]
    pub(super) endpoint: Option<String>,
    /// What the program may do to the store: `allow` lets everything
    /// through; `no-delete` refuses deletes, aborts and changes to lifecycle
    /// rules, bucket policies and versioning, but for what the recording
    /// made (writes go through); `read-only` lets only GET and HEAD through.
    /// Default: `[record] policy` in sonde.toml, else allow, and no-delete
    /// for --trial and --again.
    #[arg(long, value_name = "POLICY", value_parser = ["allow", "no-delete", "read-only"])]
    pub(super) policy: Option<String>,
    /// The run's file (default: a new file in the data directory's runs/).
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
    pub(super) writer: Arc<RunWriter>,
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
    /// What the runs before it made (a trial's clean run): no-delete lets
    /// it be removed.
    pub(super) made: Option<Arc<sonde_proxy::Made>>,
    /// A trial follows (or this is one of its runs): no-delete by default.
    pub(super) trying: bool,
}

impl Recording {
    /// The policy asked for: the flag's, else `[record] policy` of
    /// sonde.toml, else allow; a trial's runs, else no-delete (sonde's own
    /// faults could push a program down a path it would not take).
    fn policy(&self, trying: bool) -> anyhow::Result<Policy> {
        if let Some(p) = self.policy.as_deref().and_then(Policy::named) {
            return Ok(p);
        }
        let project = load_project(None)?;
        Ok(project.policy.unwrap_or(if trying {
            Policy::NoDelete
        } else {
            Policy::Allow
        }))
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
    let policy = r.policy(of.trying || under.is_some())?;
    let ep = super::endpoint_to_record(s, r.endpoint.as_deref()).await?;
    let client = s.client(&ep)?;
    let cfg = client.config();
    let path = match (&under, &r.out) {
        (Some(u), _) => u.path.clone(),
        (None, Some(p)) => p.clone(),
        (None, None) => {
            let dir = s.paths().runs();
            std::fs::create_dir_all(&dir)?;
            let name = sonde_analysis::program::file_stem(of.name.as_deref(), command.as_deref())
                .unwrap_or_else(|| "proxy".into());
            sonde_analysis::run::fresh_path(
                &dir,
                &format!("{}-{name}", chrono::Local::now().format("%Y%m%d-%H%M%S")),
            )
        }
    };
    let ran = command.is_some();
    let mut header = RunHeader::new(&ep, cfg.endpoint.as_str(), &cfg.region, command);
    if ran && let Ok(dir) = std::env::current_dir() {
        header = header.ran_in(&dir);
    }
    header.name = of.name.clone().filter(|n| !n.trim().is_empty());
    // a run recorded again starts with no verdict of its last recording
    if under.is_none() {
        sonde_analysis::trial::remove(&path);
    }
    if let Some(u) = under {
        header.scenario = Some(u.scenario.name.clone());
        header.trial_of = Some(u.of.clone());
    }
    // under no-delete a program may remove what it made (a trial's runs
    // share its clean run's)
    let made = of.made.clone().unwrap_or_default();
    let writer = Arc::new(RunWriter::create(&path, &header)?);
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
            made,
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

/// The folder a run is in, where its program's runs are kept.
pub(super) fn runs_dir(run: &std::path::Path) -> PathBuf {
    match run.parent() {
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
        .response_headers()
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("x-sonde-proxy"));
    let miss = sonde_analysis::summary::is_miss(r);
    let status = match r.status {
        _ if refused => paint("BLOCK", "33", color),
        _ if miss => paint("404", "2", color),
        Some(s) if s >= 500 => paint(&s.to_string(), "31", color),
        Some(s) if s >= 400 => paint(&s.to_string(), "33", color),
        Some(s) => paint(&s.to_string(), "2", color),
        None => paint("ERR", "31", color),
    };
    let upload = matches!(r.method.as_str(), "PUT" | "POST");
    let bytes = match (r.request_bytes.get(), r.response_bytes.get()) {
        (Some(up), _) if up > 0 || (upload && !r.is_error()) => format!("↑{}", human_bytes(up)),
        (_, Some(down)) if down > 0 && !r.is_error() => format!("↓{}", human_bytes(down)),
        _ => String::new(),
    };
    let target = match (&r.bucket, &r.key) {
        (Some(b), Some(k)) => format!("{b}/{k}"),
        (Some(b), None) => format!("{b}/"),
        _ => "/".into(),
    };
    let conditions = r.request_headers().iter().filter_map(|h| {
        let n = h.name.to_ascii_lowercase();
        (n.starts_with("if-") || n == "x-amz-copy-source-if-match")
            .then(|| format!("{n}: {}", h.value))
    });
    let detail = r
        .query()
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
        r.kind,
    );
    if !detail.is_empty() {
        line.push_str(&paint(&format!("  {detail}"), "2", color));
    }
    if let Some(e) = &r.rare().transport_error {
        line.push_str(&paint(&format!("  {e}"), "31", color));
    } else if r.is_error()
        && !miss
        && let Some(code) = r.rare().error_body.as_deref().and_then(|b| {
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

/// The request prices of a run on `endpoint`: the ones the app shows.
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
    use sonde_analysis::summary::FEW;
    if p.groups.is_empty() {
        return Vec::new();
    }
    const ROWS: usize = 8;
    let shown = if all { p.groups.len() } else { ROWS };
    // a percentile of a handful of requests is one of them: marked, when
    // others are of more
    let few = |g: &sonde_analysis::profile::Group| g.requests < FEW as usize;
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
                    format!("{} (not found)", g.kind)
                } else {
                    g.kind.clone()
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
                        "{} ({})",
                        output::duration(g.waited_ms),
                        share_text(g.share)
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
    let header = RunHeader::new(endpoint, "", "", None);
    let analysis = Analysis::of_records(&header, records, prices);
    print_summary_of(&analysis, &header, path, color, stdout, false, None);
}

/// What a run amounts to, in one fixed shape for `run`, `show` and
/// the tour: the head line (requests, operations, the SDK, the store, the
/// time, the waiting and the cost), the findings first (each with its
/// saving, its line of code and its fix), what acting on them would leave,
/// one table of the kinds of request, and the change since the program's
/// run before. `all` adds the signals over the run and their tables.
pub(super) fn print_summary_of(
    a: &Analysis,
    header: &RunHeader,
    path: &Path,
    color: bool,
    stdout: bool,
    all: bool,
    since: Option<&sonde_analysis::fixloop::SinceLast>,
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
    // operations said when they are not the requests again
    let mut head = output::count(s.requests as usize, "request");
    if a.episodes.len() != s.requests as usize {
        head.push_str(&format!(
            " ({})",
            output::count(a.episodes.len(), "operation")
        ));
    }
    if let Some(sdk) = &a.sdk {
        head.push_str(&format!(" from {sdk}"));
    }
    head.push_str(&format!(
        " to {} in {}",
        header.endpoint,
        output::duration(s.seconds * 1000.0)
    ));
    // (spread over whole seconds by sonde, waiting would be sonde's guess)
    if p.waiting_ms > 0.0 && !header.whole_seconds {
        head.push_str(&format!(", waiting {}", output::duration(p.waiting_ms)));
    }
    // (under a cent the table's shares say it)
    if p.cost.amount >= sonde_analysis::program::CENT {
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
        let refused = refused_lines(s, *n);
        if what == "refused by sonde" && !refused.is_empty() {
            for line in refused {
                say(paint(&line, "33", color));
            }
        } else {
            say(format!("  {} × {what}", output::grouped(*n)));
        }
    }
    if all {
        let mut signals = a.signals.clone();
        if header.whole_seconds {
            signals.in_flight.peak = 0;
        }
        for line in sonde_analysis::signals::text(&signals, color).lines() {
            say(line.to_string());
        }
    }
    if let Some(since) = since {
        for line in super::fixloop::since_lines(since, color) {
            say(line);
        }
    }
    say(paint(&format!("run: {}", path.display()), "2", color).to_string());
}

/// `-o FILE` and a file there: refused (64) unless `--force`. One rule for
/// every command that writes a file it is given (`run`, `import`, `share`).
pub(super) fn refuse_existing(out: &Path, force: bool) -> anyhow::Result<()> {
    if !force && out.symlink_metadata().is_ok() {
        return Err(super::usage(format!(
            "-o {}: the file is there; --force writes over it",
            out.display()
        )));
    }
    Ok(())
}

/// The requests sonde refused, named, a line for each reason: a policy's
/// with the flag that lifts it (`1 × refused by sonde's no-delete policy:
/// DeleteObject demo/old/report.csv (--policy allow lets it through)`), and
/// the rest (S3 Express, a path that names no bucket), which no flag lifts.
fn refused_lines(s: &sonde_analysis::summary::Summary, n: u64) -> Vec<String> {
    let mut reasons: Vec<(&str, Vec<&str>)> = Vec::new();
    for r in &s.refusals {
        match reasons.iter_mut().find(|(p, _)| *p == r.policy) {
            Some((_, named)) => named.push(&r.request),
            None => reasons.push((&r.policy, vec![&r.request])),
        }
    }
    let named = s.refusals.len() as u64;
    let last = reasons.len().saturating_sub(1);
    reasons
        .iter()
        .enumerate()
        .map(|(i, (policy, requests))| {
            // the requests past those named are counted on the last line
            let more = if i == last && n > named {
                format!(" and {} more", output::grouped(n - named))
            } else {
                String::new()
            };
            let count = if i == last {
                requests.len() as u64 + n.saturating_sub(named)
            } else {
                requests.len() as u64
            };
            let it = if count == 1 { "it" } else { "them" };
            match *policy {
                "" => format!(
                    "  {} × refused by sonde: {}{more}",
                    output::grouped(count),
                    requests.join(", ")
                ),
                policy => format!(
                    "  {} × refused by sonde's {policy} policy: {}{more} (--policy allow lets {it} through)",
                    output::grouped(count),
                    requests.join(", ")
                ),
            }
        })
        .collect()
}

/// A share as a percentage: `0.4%`, `12%` ([`sonde_fmt::percent`]).
pub(super) fn share_text(share: f64) -> String {
    sonde_fmt::percent(share)
}

/// `sonde.toml` at `explicit`, or here or in a folder above. A file that
/// does not read, or says what sonde does not know, is a mistake in the
/// configuration (64): nothing of it is passed over in silence.
pub(super) fn load_project(explicit: Option<&PathBuf>) -> anyhow::Result<Project> {
    let found = explicit.cloned().or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|d| sonde_analysis::project::find(&d))
    });
    let Some(path) = found else {
        return Ok(Project::default());
    };
    sonde_analysis::project::read(&path).map_err(super::usage)
}

/// How long a stopping proxy waits for the requests in flight.
pub(super) const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Stops the proxy and waits `grace` for what is in flight. When requests
/// were still open after it, says so once: the run is short of them, and
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
        "requests were still in flight {} s after the proxy stopped: the run does not have them",
        grace.as_secs_f64()
    )
}

/// The run a command is about when none is named: one rule for `show`,
/// `compare`, `share` and `check`. The newest run of the project's program
/// (`[program] name` in sonde.toml), else of the program recorded in this
/// folder (the run's `cwd`), else the newest run of
/// any. Only runs (`run`, `proxy`): a replay, an import and a trial are
/// named to be read. The run, and which it is in words.
pub fn which_run(s: &Sonde, name: Option<&str>) -> anyhow::Result<(PathBuf, String)> {
    let dir = s.paths().runs();
    let here = std::env::current_dir().ok();
    let here = here.as_ref().map(|h| h.canonicalize().unwrap_or(h.clone()));
    // the newest of the program named, of this folder, and of all
    type Newest = Option<(chrono::DateTime<chrono::Utc>, PathBuf, String)>;
    let (mut named, mut local, mut any): (Newest, Newest, Newest) = (None, None, None);
    let keep = |n: &mut Newest, at: chrono::DateTime<chrono::Utc>, p: &PathBuf, key: &str| {
        if n.as_ref().is_none_or(|(t, ..)| at > *t) {
            *n = Some((at, p.clone(), key.to_string()));
        }
    };
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("sonde") {
            continue;
        }
        let Some(h) = sonde_analysis::run::read_header(&p).filter(|h| h.is_recording()) else {
            continue;
        };
        let key = sonde_analysis::program::key_of(&h).unwrap_or_else(|| "proxy".into());
        keep(&mut any, h.created, &p, &key);
        if let Some(n) = name
            && of_program(&h, n)
        {
            keep(&mut named, h.created, &p, &key);
        }
        let cwd = h.cwd.as_deref().map(PathBuf::from);
        let cwd = cwd.map(|c| c.canonicalize().unwrap_or(c));
        if cwd.is_some() && cwd == here {
            keep(&mut local, h.created, &p, &key);
        }
    }
    match (name, named, local, any) {
        (Some(n), Some((_, p, _)), ..) => Ok((
            p,
            format!("the newest run of program \"{n}\" ([program] name in sonde.toml)"),
        )),
        (Some(n), None, ..) => Err(super::usage(format!(
            "no run of program \"{n}\" ([program] name in sonde.toml) in {}: record one with `sonde run -- <command>`, or name the run",
            dir.display()
        ))),
        (None, _, Some((_, p, key)), _) => Ok((
            p,
            format!("the newest run of program `{key}` recorded in this folder"),
        )),
        (None, _, None, Some((_, p, key))) => Ok((
            p,
            format!("the newest run (program `{key}`; none was recorded in this folder)"),
        )),
        (None, _, None, None) => Err(super::usage(format!(
            "no recorded runs in {} yet: record one with `sonde run -- <command>`",
            dir.display()
        ))),
    }
}

/// Whether a run is of the program named `name`.
pub fn of_program(h: &RunHeader, name: &str) -> bool {
    sonde_analysis::program::key_of(h).as_deref() == Some(name)
}

/// The project's program's name here (`[program] name` of the sonde.toml
/// this folder is in), when it says one. A sonde.toml that cannot be read
/// names none here: `run` and `check` say what is wrong with it.
pub fn project_name() -> Option<String> {
    let here = std::env::current_dir().ok()?;
    sonde_analysis::project::of(&here)
        .ok()??
        .file
        .program
        .name
        .filter(|n| !n.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 20.0 said `1 × refused by sonde` and nothing of what, why or how to
    /// let it through; 21.0 blamed an S3 Express refusal on the policy.
    #[test]
    fn refused_requests_are_named_a_line_for_each_reason() {
        use sonde_analysis::summary::{Refusal, Summary};
        let refusal = |policy: &str, request: &str| Refusal {
            policy: policy.into(),
            request: request.into(),
        };
        let mut s = Summary {
            refused: 1,
            refusals: vec![refusal("no-delete", "DeleteObject demo/old/report.csv")],
            ..Default::default()
        };
        assert_eq!(
            refused_lines(&s, 1),
            [
                "  1 × refused by sonde's no-delete policy: DeleteObject demo/old/report.csv (--policy allow lets it through)"
            ]
        );
        s.refusals.push(refusal("no-delete", "DeleteObject demo/a"));
        s.refusals.push(refusal("no-delete", "DeleteObject demo/b"));
        let lines = refused_lines(&s, 7);
        assert!(
            lines[0].ends_with("demo/b and 4 more (--policy allow lets them through)"),
            "{lines:?}"
        );
        // S3 Express is no policy's: its own line, with no flag
        s.refusals = vec![
            refusal("no-delete", "DeleteObject demo/a"),
            refusal("", "CreateSession b--use1-az4--x-s3"),
        ];
        assert_eq!(
            refused_lines(&s, 2),
            [
                "  1 × refused by sonde's no-delete policy: DeleteObject demo/a (--policy allow lets it through)",
                "  1 × refused by sonde: CreateSession b--use1-az4--x-s3"
            ]
        );
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
}
