use std::sync::Arc;

use clap::{Args, Subcommand};
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
        /// The format written: table or json.
        #[arg(
            long,
            short = 'f',
            value_enum,
            default_value_t,
            hide_possible_values = true
        )]
        format: Format,
    },
    /// Lists buckets and, where the vendor has it, asks STS who the credentials belong to.
    Test {
        /// The endpoint's id (default: --url, or the only one configured).
        endpoint: Option<String>,
        /// The format written: table or json.
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
        /// (--region, --path-style and --ca-file apply to it).
        #[arg(value_name = "URL")]
        endpoint_url: String,
        /// Credentials from this profile of ~/.aws/config (default: the AWS default chain).
        #[arg(long)]
        profile: Option<String>,
        /// Replace the endpoint of this id (else an id taken is refused).
        #[arg(long)]
        replace: bool,
    },
    /// Adds endpoints for the profiles of ~/.aws/config.
    Import {
        /// Only these profiles (default: all that are not configured yet).
        profiles: Vec<String>,
    },
}

pub async fn run(s: &Arc<Sonde>, cli: &Cli, a: &EndpointArgs) -> anyhow::Result<i32> {
    // add and import say what they did on stderr: they have no -f
    let (command, format) = match &a.action {
        EndpointAction::List { format } => ("endpoints list", *format),
        EndpointAction::Test { format, .. } => ("endpoints test", *format),
        _ => ("endpoints", Format::Table),
    };
    if let Some(why) = output::unsupported(format, command, &[Format::Table, Format::Json]) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
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
            let vendor = sonde_core::Vendor::guess(endpoint_url);
            let mut e = sonde_core::EndpointConfig::new(id, id, endpoint_url, &cli.region, vendor);
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
                Format::Json => println!("{}", output::json(&e)),
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
        EndpointAction::Test { endpoint, .. } => {
            let ep = to_test(s, endpoint.as_deref())?;
            let r = s.test_endpoint(&ep).await;
            match format {
                Format::Json => println!("{}", output::json(&r)),
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
        EndpointAction::Import { profiles } => {
            let mut added = 0;
            for e in s.importable_profiles() {
                let name = match &e.credentials {
                    sonde_core::CredentialSource::Profile { name } => name.clone(),
                    _ => continue,
                };
                if !profiles.is_empty() && !profiles.contains(&name) {
                    continue;
                }
                println!("added {} ({}) → {}", e.id, name, e.url);
                s.upsert_endpoint(e).map_err(|m| anyhow::anyhow!(m))?;
                added += 1;
            }
            if added == 0 {
                println!("nothing to import");
            }
            Ok(0)
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
