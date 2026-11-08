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
    /// A tour: a small program's requests to a built-in store, recorded and explained.
    Demo(cmd::record::DemoArgs),
    /// Run a command with its S3 requests recorded: sonde run -- <command>.
    Run(cmd::record::RunArgs),
    /// A local S3 endpoint that records what talks to it.
    Proxy(cmd::record::ProxyArgs),
    /// The recorded sessions, newest first; prune old ones.
    Sessions(cmd::sessions::SessionsArgs),
    /// A proxy left running: a live status line, each new warning, rotating session files.
    Watch(cmd::record::WatchArgs),
    /// Print a recorded session.
    Show(cmd::record::ShowArgs),
    /// Replay a recorded session on another endpoint and compare the answers.
    Replay(cmd::record::ReplayArgs),
    /// Compare a session with its replay.
    Diff(cmd::record::DiffArgs),
    /// Two runs of a program, before and after a change: requests, time, cost, findings.
    Compare(cmd::record::CompareArgs),
    /// A run against a budget or a baseline run: exit 1 when it is over (for CI).
    Check(cmd::record::CheckArgs),
    /// A session as a web page anyone opens in a browser.
    Share(cmd::share::ShareArgs),
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
    // `sonde … | head`: a closed pipe ends the output quietly
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| info.payload().downcast_ref::<&str>().copied())
            .unwrap_or("");
        if msg.contains("Broken pipe") {
            std::process::exit(0);
        }
        previous(info);
    }));
    // a mistake in the command is a usage error (64), as docs/cli.md says; help and --version are not
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(if e.use_stderr() { 64 } else { 0 });
        }
    };
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
            Command::Demo(a) => cmd::record::demo(&sonde, a).await,
            Command::Run(a) => cmd::record::run(&sonde, &cli, a).await,
            Command::Proxy(a) => cmd::record::proxy(&sonde, &cli, a).await,
            Command::Sessions(a) => cmd::sessions::run(&sonde, a).await,
            Command::Watch(a) => cmd::record::watch(&sonde, a).await,
            Command::Show(a) => cmd::record::show(&sonde, a).await,
            Command::Replay(a) => cmd::record::replay(&sonde, &cli, a).await,
            Command::Diff(a) => cmd::record::diff(&sonde, a).await,
            Command::Share(a) => cmd::share::run(&sonde, a).await,
            Command::Compare(a) => cmd::record::compare(&sonde, a).await,
            Command::Check(a) => cmd::record::check(&sonde, a).await,
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
