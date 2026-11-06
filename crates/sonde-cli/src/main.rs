//! `sonde`: headless diagnosis for scripts and CI, and the terminal UI.
//!
//! Exit codes (also in docs/cli.md):
//!
//! | code | meaning |
//! |---|---|
//! | 0 | done, nothing to report |
//! | 1 | a probe failed (a guarantee broke), or the check found a problem |
//! | 2 | probes behaved differently from AWS (no failure) |
//! | 3 | probes could not run (errors only) |
//! | 4 | findings worth attention (stale uploads, a missing lifecycle rule, layout warnings) |
//! | 64 | usage error |
//! | 70 | the operation failed (connection, credentials, permissions) |
//! | 77 | refused: read-only mode or a confirmation mismatch |

mod cmd;
mod output;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sonde", version, about = "Diagnose S3 and S3-compatible object storage", long_about = None)]
pub struct Cli {
    /// Configuration directory (default: $SONDE_CONFIG_DIR or the platform's).
    #[arg(long, global = true, env = "SONDE_CONFIG_DIR")]
    config_dir: Option<std::path::PathBuf>,
    /// Data directory (request log, scans, probe runs, audit log).
    #[arg(long, global = true, env = "SONDE_DATA_DIR")]
    data_dir: Option<std::path::PathBuf>,
    /// Use an endpoint not in the configuration: its URL (credentials from the AWS default chain).
    #[arg(long, global = true)]
    url: Option<String>,
    /// Region for --url.
    #[arg(long, global = true, default_value = "us-east-1")]
    region: String,
    /// Path-style addressing for --url.
    #[arg(long, global = true)]
    path_style: bool,
    /// A PEM CA bundle for --url.
    #[arg(long, global = true)]
    ca_file: Option<std::path::PathBuf>,
    /// Requests per second per job.
    #[arg(long, global = true)]
    rate: Option<f64>,
    /// Requests in flight per job.
    #[arg(long, global = true)]
    concurrency: Option<usize>,
    /// Write the request log of this run (redacted) to a file: .har or .json.
    #[arg(long, global = true)]
    log_out: Option<std::path::PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compatibility probes.
    Probe(cmd::probe::ProbeArgs),
    /// Incomplete multipart uploads.
    Mpu(cmd::mpu::MpuArgs),
    /// Capacity breakdown: why is this bucket so big?
    Du(cmd::du::DuArgs),
    /// Storage layout lenses (SlateDB, Iceberg, lakeFS, Woodpecker, cadence).
    Layout(cmd::layout::LayoutArgs),
    /// Configured endpoints.
    Endpoints(cmd::endpoints::EndpointArgs),
    /// The terminal UI.
    #[cfg(feature = "tui")]
    Tui(cmd::tui::TuiArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    #[cfg(feature = "tui")]
    if let Command::Tui(args) = &cli.command {
        return cmd::tui::run(&cli, args);
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let code = rt.block_on(async {
        let sonde = match cmd::open(&cli) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("sonde: {e}");
                return 64;
            }
        };
        let code = match &cli.command {
            Command::Probe(a) => cmd::probe::run(&sonde, &cli, a).await,
            Command::Mpu(a) => cmd::mpu::run(&sonde, &cli, a).await,
            Command::Du(a) => cmd::du::run(&sonde, &cli, a).await,
            Command::Layout(a) => cmd::layout::run(&sonde, &cli, a).await,
            Command::Endpoints(a) => cmd::endpoints::run(&sonde, &cli, a).await,
            #[cfg(feature = "tui")]
            Command::Tui(_) => unreachable!(),
        };
        let code = match code {
            Ok(c) => c,
            Err(e) => {
                eprintln!("sonde: {e:#}");
                if let Some(s3) = e.downcast_ref::<s3raw::S3Error>() {
                    if let Some(id) = s3.record_id
                        && let Some(r) = sonde.log().get(id)
                    {
                        eprintln!(
                            "  {} {} → {}",
                            r.method,
                            r.url,
                            r.status
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "no response".into())
                        );
                    }
                    if s3.kind == s3raw::ErrorKind::Refused {
                        77
                    } else {
                        70
                    }
                } else {
                    70
                }
            }
        };
        if let Some(path) = &cli.log_out {
            let records = sonde.log().snapshot();
            let doc = if path.extension().is_some_and(|e| e == "har") {
                reqlog::export::to_har(&records)
            } else {
                reqlog::export::to_json(&records)
            };
            if let Err(e) =
                std::fs::write(path, serde_json::to_vec_pretty(&doc).unwrap_or_default())
            {
                eprintln!("sonde: --log-out {}: {e}", path.display());
            }
        }
        code
    });
    ExitCode::from(code as u8)
}
