pub mod bucket;
pub mod check;
pub mod compare;
pub mod demo;
pub mod du;
pub mod endpoints;
pub mod fixloop;
pub mod layout;
pub mod logs;
pub mod mpu;
pub mod probe;
pub mod proxy;
pub mod record;
pub mod replay;
pub mod run;
pub mod runs;
pub mod share;
pub mod show;

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

/// A file given to read that is not what it should be (a run with a
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
    text.replace("See Incomplete uploads.", "See `sonde bucket uploads`.")
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
    // of this run only: neither the endpoint nor the limits are ever saved
    if let Some(url) = &cli.url {
        let vendor = Vendor::guess(url);
        let mut e = EndpointConfig::new(ADHOC, url, url, &region(cli, url), vendor);
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

/// The region of an endpoint given at `url`: `--region`, else the region
/// an AWS URL names (`s3.eu-west-1.amazonaws.com`, `s3-eu-west-1…`,
/// `s3.dualstack.eu-west-1…`), else `AWS_REGION` or `AWS_DEFAULT_REGION`,
/// else us-east-1.
pub fn region(cli: &Cli, url: &str) -> String {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    cli.region
        .clone()
        .or_else(|| region_of_url(url))
        .or_else(|| env("AWS_REGION"))
        .or_else(|| env("AWS_DEFAULT_REGION"))
        .unwrap_or_else(|| "us-east-1".into())
}

/// The region an AWS S3 URL names in its host, if it names one.
fn region_of_url(url: &str) -> Option<String> {
    let host = url.split_once("://").map_or(url, |(_, h)| h);
    let host = host.split(['/', ':', '?']).next()?.to_ascii_lowercase();
    if !(host.ends_with(".amazonaws.com") || host.ends_with(".amazonaws.com.cn")) {
        return None;
    }
    // eu-west-1, us-gov-west-1, cn-north-1: letters, then a number last
    let is_region = |r: &str| {
        let parts: Vec<&str> = r.split('-').collect();
        parts.len() >= 3
            && parts[0].len() == 2
            && parts[..parts.len() - 1]
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase()))
            && parts[parts.len() - 1].bytes().all(|b| b.is_ascii_digit())
            && !parts[parts.len() - 1].is_empty()
    };
    // from S3's own label on: a bucket's name before it is no region
    let labels: Vec<&str> = host.split('.').collect();
    let s3 = labels
        .iter()
        .position(|l| *l == "s3" || l.starts_with("s3-"))?;
    labels[s3..]
        .iter()
        .map(|l| l.strip_prefix("s3-").unwrap_or(l))
        .find(|l| is_region(l))
        .map(String::from)
}

/// The endpoint a command acts on: `--endpoint`, `--url`, or the only one
/// configured. None to be found is the configuration's mistake (64).
pub fn endpoint(s: &Sonde, given: Option<&str>) -> anyhow::Result<String> {
    if let Some(e) = given {
        if s.endpoint_config(e).is_none() {
            return Err(usage(format!(
                "no endpoint {e:?}: `sonde endpoints list` shows the configured ones, --url adds one for this run"
            )));
        }
        return Ok(e.to_string());
    }
    if s.endpoint_config(ADHOC).is_some() {
        return Ok(ADHOC.into());
    }
    let all = s.endpoints();
    match all.len() {
        1 => Ok(all[0].id.clone()),
        0 => Err(usage(format!(
            "no endpoint configured: add one with `sonde endpoints add ID URL` (kept in {}), or name one for this run with `--url URL --path-style`",
            s.paths().config_file().display()
        ))),
        _ => Err(usage(format!(
            "several endpoints configured: pass --endpoint ({})",
            all.iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// The endpoint a recording goes to: [`endpoint`]'s, and with none
/// configured, what an AWS SDK would use (the default credential chain and
/// region, as `endpoints import` reads them), said on stderr. When that
/// chain finds no credentials either, the usage error says both ways.
pub async fn endpoint_to_record(s: &Sonde, given: Option<&str>) -> anyhow::Result<String> {
    if given.is_some() || s.endpoint_config(ADHOC).is_some() || !s.endpoints().is_empty() {
        return endpoint(s, given);
    }
    let files = sonde_core::credentials::ini::AwsFiles::load();
    let (e, from) = sonde_core::sonde::aws_default(AWS_DEFAULT, &files, &|k| std::env::var(k).ok());
    let (url, region) = (e.url.clone(), e.region.clone());
    s.add_transient_endpoint(e)
        .map_err(|m| anyhow::anyhow!(m))?;
    let found = match s.client(AWS_DEFAULT) {
        Ok(c) => c.credentials().await.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    match found {
        Ok(_) => {
            eprintln!(
                "sonde: no endpoint configured: the AWS default credentials and region {region} ({from}) → {url}"
            );
            Ok(AWS_DEFAULT.into())
        }
        Err(why) => Err(usage(format!(
            "no endpoint configured, and the AWS default chain: {why}. Add an endpoint with `sonde endpoints add ID URL` (kept in {}), or name one for this run with `--url URL --path-style`",
            s.paths().config_file().display()
        ))),
    }
}

/// The endpoint id a recording with nothing configured goes to
/// ([`endpoint_to_record`]).
pub const AWS_DEFAULT: &str = "aws";

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

    /// 27.0 stored us-east-1 for any URL without --region.
    #[test]
    fn an_aws_url_names_its_region() {
        for (url, region) in [
            ("https://s3.eu-west-1.amazonaws.com", Some("eu-west-1")),
            ("https://s3-ap-south-1.amazonaws.com/", Some("ap-south-1")),
            (
                "https://s3.dualstack.us-west-2.amazonaws.com",
                Some("us-west-2"),
            ),
            (
                "https://my-bkt-1.s3.us-gov-west-1.amazonaws.com",
                Some("us-gov-west-1"),
            ),
            ("https://s3.cn-north-1.amazonaws.com.cn", Some("cn-north-1")),
            ("https://my-bkt-1.s3.amazonaws.com", None),
            ("http://localhost:9000", None),
            ("https://s3.eu-west-1.example.com", None),
        ] {
            assert_eq!(region_of_url(url).as_deref(), region, "{url}");
        }
    }

    #[test]
    fn what_points_at_the_app_is_said_for_a_terminal() {
        assert_eq!(
            said("billed but not listed as objects. See Incomplete uploads."),
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
