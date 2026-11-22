use std::sync::Arc;

use clap::{Args, Subcommand};
use probes::Verdict;
use probes::runner::RunOptions;
use sonde_core::Sonde;

use crate::Cli;
use crate::output::{self, Format};

#[derive(Args)]
pub struct EndpointArgs {
    #[command(subcommand)]
    action: EndpointAction,
}

#[derive(Subcommand)]
enum EndpointAction {
    /// Configured endpoints (never their credentials).
    List {
        /// The format written: text or json.
        #[arg(
            long,
            short = 'f',
            value_enum,
            default_value_t,
            hide_possible_values = true
        )]
        format: Format,
    },
    /// Lists buckets and, where the vendor has it, asks STS who the
    /// credentials belong to; with --probes, how it answers next to AWS.
    Test {
        /// The endpoint's id (default: --url, or the only one configured).
        endpoint: Option<String>,
        /// Run the compatibility probes in BUCKET (it must exist; they write
        /// under sonde-probe/ and remove what they wrote): each with its
        /// verdict next to AWS. Exit 1 when a guarantee broke, 2 when it
        /// differs from AWS.
        #[arg(long, value_name = "BUCKET")]
        probes: Option<String>,
        /// The format written: text or json.
        #[arg(
            long,
            short = 'f',
            value_enum,
            default_value_t,
            hide_possible_values = true
        )]
        format: Format,
    },
    /// Adds an endpoint: its URL, credentials from a profile or the AWS default chain (never a key on the command line).
    Add {
        /// A short id: what -e names it by.
        id: String,
        /// The endpoint URL, e.g. https://s3.eu-west-1.amazonaws.com or http://localhost:9000
        /// (--region, --path-style and --ca-file apply to it). With
        /// --profile, default: the profile's endpoint_url, else AWS in its
        /// region.
        #[arg(value_name = "URL", required_unless_present = "profile")]
        endpoint_url: Option<String>,
        /// Credentials from this profile of ~/.aws/config (default: the AWS default chain).
        #[arg(long)]
        profile: Option<String>,
        /// Replace the endpoint of this id (else an id taken is refused).
        #[arg(long)]
        replace: bool,
    },
}

pub async fn run(s: &Arc<Sonde>, cli: &Cli, a: &EndpointArgs) -> anyhow::Result<i32> {
    // add says what it did on stderr: it has no -f
    let format = match &a.action {
        EndpointAction::List { format } | EndpointAction::Test { format, .. } => *format,
        _ => Format::Text,
    };
    match &a.action {
        EndpointAction::Add {
            id,
            endpoint_url,
            profile,
            replace,
        } => {
            if !replace && s.endpoint_config(id).is_some() {
                eprintln!(
                    "sonde: there is an endpoint {id:?} already: --replace replaces it (`sonde endpoints list` shows it)"
                );
                return Ok(64);
            }
            // a profile alone: its endpoint and region, as an SDK reads them
            let mut e = match (endpoint_url, profile) {
                (None, Some(name)) => sonde_core::sonde::profile_endpoint(
                    &sonde_core::credentials::ini::AwsFiles::load(),
                    id,
                    name,
                    cli.region.as_deref(),
                )
                .ok_or_else(|| super::usage(format!("no profile {name:?} in ~/.aws/config")))?,
                (url, _) => {
                    let url = url.as_deref().unwrap_or_default();
                    let vendor = sonde_core::Vendor::guess(url);
                    let region = super::region(cli, url);
                    sonde_core::EndpointConfig::new(id, id, url, &region, vendor)
                }
            };
            e.path_style |= cli.path_style;
            e.ca_file = cli.ca_file.clone();
            if let Some(name) = profile {
                e.credentials = sonde_core::CredentialSource::Profile { name: name.clone() };
            }
            match s.upsert_endpoint(e) {
                Ok(v) => {
                    eprintln!(
                        "added {} ({}): `sonde endpoints test {}` checks it",
                        v.id, v.url, v.id
                    );
                    Ok(0)
                }
                Err(e) => {
                    eprintln!("sonde: {e}");
                    Ok(64)
                }
            }
        }
        EndpointAction::List { .. } => {
            let e = s.endpoints();
            match format {
                Format::Json => output::print_json(&e),
                _ => println!(
                    "{}",
                    output::table(
                        &["id", "name", "vendor", "url", "region", "credentials"],
                        &e.iter()
                            .map(|e| vec![
                                e.id.clone(),
                                e.name.clone(),
                                e.vendor_label.clone(),
                                e.url.clone(),
                                e.region.clone(),
                                e.credentials.clone()
                            ])
                            .collect::<Vec<_>>()
                    )
                ),
            }
            Ok(0)
        }
        EndpointAction::Test {
            endpoint,
            probes: Some(bucket),
            ..
        } => {
            let ep = to_test(s, endpoint.as_deref())?;
            probe(s, &ep, bucket, format).await
        }
        EndpointAction::Test { endpoint, .. } => {
            let ep = to_test(s, endpoint.as_deref())?;
            let r = s.test_endpoint(&ep).await;
            match format {
                Format::Json => output::print_json(&r),
                _ => {
                    println!(
                        "{}: {} in {:.0} ms",
                        r.endpoint,
                        if r.ok { "reachable" } else { "FAILED" },
                        r.latency_ms
                    );
                    if let Some(c) = &r.credentials_source {
                        println!("  credentials: {c}");
                    }
                    if let Some(n) = r.bucket_count {
                        println!("  buckets: {n}");
                    }
                    if let Some(o) = &r.owner {
                        println!("  owner: {o}");
                    }
                    if let Some(srv) = &r.server {
                        println!("  server: {srv}");
                    }
                    if let Some(i) = &r.identity {
                        println!(
                            "  identity: {} (account {})",
                            i.arn.clone().unwrap_or_default(),
                            i.account.clone().unwrap_or_default()
                        );
                    }
                    if let Some(n) = &r.identity_note {
                        println!("  identity: {n}");
                    }
                    if let Some(e) = &r.error {
                        println!("  error: {e}");
                    }
                }
            }
            Ok(if r.ok { 0 } else { 70 })
        }
    }
}

/// The endpoint `endpoints test` tests: the one named, or the only one.
/// Otherwise a usage error that says how to name one here.
fn to_test(s: &Sonde, given: Option<&str>) -> anyhow::Result<String> {
    let all = s.endpoints();
    let ids = || {
        all.iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match given {
        Some(e) if s.endpoint_config(e).is_some() => Ok(e.to_string()),
        Some(e) if all.is_empty() => Err(super::usage(format!(
            "no endpoint {e:?}: none is configured (`sonde endpoints add ID URL`)"
        ))),
        Some(e) => Err(super::usage(format!(
            "no endpoint {e:?}: the configured ones are {}",
            ids()
        ))),
        None if all.len() == 1 => Ok(all[0].id.clone()),
        None if all.is_empty() => Err(super::usage(
            "no endpoint configured: add one with `sonde endpoints add ID URL`",
        )),
        None => Err(super::usage(format!(
            "several endpoints configured: name one, `sonde endpoints test ID` ({})",
            ids()
        ))),
    }
}

/// `endpoints test E --probes BUCKET`: the compatibility probes on `ep`,
/// in `bucket` (naming it is the consent; the run is audited). What did
/// not pass says why in its row. Exit 1 when a guarantee broke, 2 when it
/// differs from AWS, 70 when probes could not run.
async fn probe(s: &Arc<Sonde>, ep: &str, bucket: &str, format: Format) -> anyhow::Result<i32> {
    let opts = RunOptions {
        bucket: bucket.to_string(),
        ..Default::default()
    };
    let job = probes::runner::spawn(s, ep, opts);
    let run: probes::Run = serde_json::from_value(super::wait(s, &job, false).await?)?;
    match format {
        Format::Json => output::print_json(&run),
        Format::Text => {
            let rows: Vec<Vec<String>> = run
                .results
                .iter()
                .map(|r| {
                    vec![
                        r.id.clone(),
                        format!("{} {}", r.verdict.symbol(), r.verdict.label()),
                        if r.verdict == Verdict::Pass {
                            String::new()
                        } else {
                            r.summary.clone()
                        },
                    ]
                })
                .collect();
            println!("{}", output::table(&["probe", "verdict", "why"], &rows));
            // every probe counted: those that erred or were skipped too
            let counts: Vec<String> = [
                Verdict::Pass,
                Verdict::Fail,
                Verdict::Differs,
                Verdict::Unsupported,
                Verdict::Error,
                Verdict::Skipped,
            ]
            .into_iter()
            .filter_map(|v| {
                let n = run.count(v);
                let word = match v {
                    Verdict::Differs => "differ",
                    Verdict::Error if n != 1 => "errors",
                    _ => v.label(),
                };
                (n > 0 || matches!(v, Verdict::Pass | Verdict::Fail)).then(|| format!("{n} {word}"))
            })
            .collect();
            println!(
                "\n{}: {} probes: {}",
                run.endpoint_name,
                run.results.len(),
                counts.join(", ")
            );
        }
    }
    Ok(probes::report::exit_code(std::slice::from_ref(&run)))
}
