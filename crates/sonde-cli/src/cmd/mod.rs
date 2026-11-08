pub mod du;
pub mod endpoints;
pub mod layout;
pub mod mpu;
pub mod probe;
pub mod record;
pub mod sessions;
pub mod share;
#[cfg(feature = "tui")]
pub mod tui;

use std::sync::Arc;

use sonde_core::{Config, CredentialSource, EndpointConfig, Paths, Sonde, Vendor};

use crate::Cli;

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
    let paths = paths(cli);
    let mut config = Config::load(&paths.config_file())?;
    if let Some(url) = &cli.url {
        let vendor = Vendor::guess(url);
        let mut e = EndpointConfig::new(ADHOC, url, url, &cli.region, vendor);
        e.path_style = cli.path_style;
        e.ca_file = cli.ca_file.clone();
        e.credentials = CredentialSource::Default;
        config.endpoints.retain(|x| x.id != ADHOC);
        config.endpoints.push(e);
    }
    if let Some(r) = cli.rate {
        config.settings.rate_limit = Some(r);
    }
    if let Some(c) = cli.concurrency {
        config.settings.concurrency = c.clamp(1, 1024);
    }
    config.validate()?;
    Ok(Sonde::with_config(paths, config))
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
            "no endpoint configured: pass --url, or add one to {}",
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
    let mut rx = s.jobs().subscribe(job);
    let jobs = s.jobs().clone();
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
