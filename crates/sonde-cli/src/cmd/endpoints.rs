use std::sync::Arc;

use clap::{Args, Subcommand};
use sonde_core::Sonde;

use crate::Cli;
use crate::output::{self, Format};

#[derive(Args)]
pub struct EndpointArgs {
    #[command(subcommand)]
    action: EndpointAction,
    /// The format written: table or json (list and test).
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true,
        global = true
    )]
    format: Format,
}

#[derive(Subcommand)]
enum EndpointAction {
    /// Configured endpoints (never their credentials).
    List,
    /// Lists buckets and, where the vendor has it, asks STS who the credentials belong to.
    Test { endpoint: Option<String> },
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
    },
    /// Adds endpoints for the profiles of ~/.aws/config.
    Import {
        /// Only these profiles (default: all that are not configured yet).
        profiles: Vec<String>,
    },
}

pub async fn run(s: &Arc<Sonde>, cli: &Cli, a: &EndpointArgs) -> anyhow::Result<i32> {
    let (command, writes): (_, &[Format]) = match &a.action {
        EndpointAction::List => ("endpoints list", &[Format::Table, Format::Json]),
        EndpointAction::Test { .. } => ("endpoints test", &[Format::Table, Format::Json]),
        // these say what they did on stderr: there is nothing to format
        EndpointAction::Add { .. } => ("endpoints add", &[Format::Table]),
        EndpointAction::Import { .. } => ("endpoints import", &[Format::Table]),
    };
    if let Some(why) = output::unsupported(a.format, command, writes) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    match &a.action {
        EndpointAction::Add {
            id,
            endpoint_url,
            profile,
        } => {
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
        EndpointAction::List => {
            let e = s.endpoints();
            match a.format {
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
        EndpointAction::Test { endpoint } => {
            let ep = super::endpoint(s, endpoint.as_deref())?;
            let r = s.test_endpoint(&ep).await;
            match a.format {
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
