pub mod bucket;
pub mod check;
pub mod compare;
pub mod demo;
pub mod du;
pub mod endpoints;
pub mod fixloop;
pub mod layout;
pub mod mpu;
pub mod probe;
pub mod proxy;
pub mod record;
pub mod replay;
pub mod run;
pub mod sessions;
pub mod share;
pub mod show;
#[cfg(feature = "tui")]
pub mod tui;
pub mod watch;

use std::sync::Arc;

use sonde_core::{CredentialSource, EndpointConfig, Paths, Sonde, Vendor};

use crate::Cli;

/// A mistake in the command or in a file sonde is configured by
/// (`sonde.toml`): exit 64, not the 70 of an operation that failed.
#[derive(Debug)]
pub struct Usage(pub String);

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Usage {}

pub fn usage(msg: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Usage(msg.into()))
}

/// A file given to read that is not what it should be (a session with a
/// line no record could be, no header, a newer format): exit 65, the data's
/// fault, not the 70 of an operation that failed.
#[derive(Debug)]
pub struct BadData(pub String);

impl std::fmt::Display for BadData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BadData {}

pub fn bad_data(msg: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(BadData(msg.into()))
}

/// An error as the CLI says it: where sonde's own text points at the app
/// (its Incomplete uploads), what to do from a terminal.
pub fn said(text: &str) -> String {
    text.replace("See Multipart.", "See `sonde bucket uploads`.")
}

/// A command folded into another in 18.0 still runs, for one version, and
/// says what it is now (on stderr: its output is the new command's).
pub fn renamed(old: &str, new: &str) {
    eprintln!("note: `{old}` is now `{new}`; the old form goes in 19.0");
}

/// The endpoint id `--url` registers for one run.
pub const ADHOC: &str = "adhoc";

pub fn paths(cli: &Cli) -> Paths {
    let mut p = Paths::from_env();
    if let Some(c) = &cli.config_dir {
        p.config_dir = c.clone();
    }
    if let Some(d) = &cli.data_dir {
        p.data_dir = d.clone();
    }
    p
}

pub fn open(cli: &Cli) -> Result<Arc<Sonde>, String> {
    let s = Sonde::open(paths(cli))?;
    // an older config.toml still loads: what it says that is no longer read is said
    for gone in s.settings().gone() {
        eprintln!("note: {}: {gone}", s.paths().config_file().display());
    }
    // of this run only: neither the endpoint nor the limits are ever saved
    if let Some(url) = &cli.url {
        let vendor = Vendor::guess(url);
        let mut e = EndpointConfig::new(ADHOC, url, url, &cli.region, vendor);
        e.path_style = cli.path_style;
        e.ca_file = cli.ca_file.clone();
        e.credentials = CredentialSource::Default;
        s.add_transient_endpoint(e)?;
    }
    s.set_overrides(sonde_core::sonde::Overrides {
        rate_limit: cli.rate,
        concurrency: cli.concurrency.map(|c| c.clamp(1, 1024)),
    })?;
    Ok(s)
}

/// The endpoint a command acts on: `--endpoint`, `--url`, or the only one configured.
pub fn endpoint(s: &Sonde, given: Option<&str>) -> anyhow::Result<String> {
    if let Some(e) = given {
        if s.endpoint_config(e).is_none() {
            anyhow::bail!(
                "no endpoint {e:?}: `sonde endpoints list` shows the configured ones, --url adds one for this run"
            );
        }
        return Ok(e.to_string());
    }
    if s.endpoint_config(ADHOC).is_some() {
        return Ok(ADHOC.into());
    }
    let all = s.endpoints();
    match all.len() {
        1 => Ok(all[0].id.clone()),
        0 => anyhow::bail!(
            "no endpoint configured: add one with `sonde endpoints add ID URL` (kept in {}), or name one for this run with `--url URL --path-style`",
            s.paths().config_file().display()
        ),
        _ => anyhow::bail!(
            "several endpoints configured: pass --endpoint ({})",
            all.iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Prints a job's progress on stderr while waiting for it.
pub async fn wait(s: &Sonde, job: &str, quiet: bool) -> Result<serde_json::Value, s3raw::S3Error> {
    wait_on(s.jobs(), job, quiet).await
}

/// Waits for jobs started together, each to its end. One that fails leaves
/// the others running to theirs; one that is cancelled (an interrupt) cancels
/// the rest, and each is still waited for, since a cancelled job cleans up
/// before it returns.
pub async fn wait_all(
    jobs: &sonde_core::JobManager,
    ids: &[String],
    quiet: bool,
) -> Vec<Result<serde_json::Value, s3raw::S3Error>> {
    let mut out = Vec::with_capacity(ids.len());
    for (i, id) in ids.iter().enumerate() {
        let r = wait_on(jobs, id, quiet).await;
        if matches!(&r, Err(e) if e.kind == s3raw::ErrorKind::Cancelled) {
            for rest in &ids[i + 1..] {
                jobs.cancel(rest);
            }
        }
        out.push(r);
    }
    out
}

async fn wait_on(
    jobs: &sonde_core::JobManager,
    job: &str,
    quiet: bool,
) -> Result<serde_json::Value, s3raw::S3Error> {
    let mut rx = jobs.subscribe(job);
    let jobs = jobs.clone();
    let id = job.to_string();
    let show = !quiet && std::io::IsTerminal::is_terminal(&std::io::stderr());
    let printer = tokio::spawn(async move {
        let Some(rx) = rx.as_mut() else { return };
        while let Ok(ev) = rx.recv().await {
            if let sonde_core::JobEvent::Progress { progress, .. } = ev
                && show
            {
                eprint!("\r\x1b[2K{}", progress.message);
            }
        }
    });
    let r = tokio::select! {
        r = jobs.wait(&id) => r,
        _ = tokio::signal::ctrl_c() => {
            jobs.cancel(&id);
            eprintln!("\ncancelled");
            jobs.wait(&id).await
        }
    };
    printer.abort();
    if show {
        eprint!("\r\x1b[2K");
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// A job with something to undo, as a probe run has: `cleaned` counts the
    /// ones that reached their cleanup.
    fn run(jobs: &sonde_core::JobManager, cleaned: &Arc<AtomicUsize>, fails: bool) -> String {
        let cleaned = cleaned.clone();
        jobs.spawn_cooperative("probe", "t", 1, None, move |ctx| async move {
            if fails {
                return Err(s3raw::S3Error::config("no such bucket"));
            }
            for _ in 0..20 {
                if ctx.is_cancelled() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            cleaned.fetch_add(1, Ordering::SeqCst);
            ctx.check()?;
            Ok(serde_json::json!(1))
        })
    }

    #[test]
    fn what_points_at_the_app_is_said_for_a_terminal() {
        assert_eq!(
            said("billed but not listed as objects. See Multipart."),
            "billed but not listed as objects. See `sonde bucket uploads`."
        );
    }

    #[tokio::test]
    async fn one_endpoint_failing_does_not_leave_the_others_runs_behind() {
        let jobs = sonde_core::JobManager::new();
        let cleaned = Arc::new(AtomicUsize::new(0));
        let ids = [run(&jobs, &cleaned, true), run(&jobs, &cleaned, false)];
        let r = wait_all(&jobs, &ids, true).await;
        assert!(r[0].is_err() && r[1].is_ok());
        assert_eq!(
            cleaned.load(Ordering::SeqCst),
            1,
            "the second ran to its end"
        );
    }

    #[tokio::test]
    async fn an_interrupt_ends_every_run_and_each_cleans_up() {
        let jobs = sonde_core::JobManager::new();
        let cleaned = Arc::new(AtomicUsize::new(0));
        let ids = [run(&jobs, &cleaned, false), run(&jobs, &cleaned, false)];
        tokio::time::sleep(Duration::from_millis(20)).await;
        jobs.cancel(&ids[0]);
        let r = wait_all(&jobs, &ids, true).await;
        assert!(
            r.iter().all(|r| r.is_err()),
            "the rest are cancelled with it"
        );
        assert_eq!(cleaned.load(Ordering::SeqCst), 2);
    }
}
