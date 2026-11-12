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

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

#[derive(Parser)]
// (bin_name: "Usage: sonde", not the file's name, "sonde.exe" on Windows)
#[command(name = "sonde", bin_name = "sonde", version, about = "See what your software does to S3: its requests recorded, read as operations, replayed elsewhere", long_about = None)]
pub struct Cli {
    /// Configuration directory (default: $SONDE_CONFIG_DIR or the platform's).
    #[arg(
        long,
        global = true,
        env = "SONDE_CONFIG_DIR",
        help_heading = "Global options"
    )]
    config_dir: Option<std::path::PathBuf>,
    /// Data directory (sessions, request log, scans, probe runs, audit log).
    #[arg(
        long,
        global = true,
        env = "SONDE_DATA_DIR",
        help_heading = "Global options"
    )]
    data_dir: Option<std::path::PathBuf>,
    /// Use an endpoint not in the configuration: its URL (credentials from the AWS default chain).
    #[arg(long, global = true, help_heading = "Global options")]
    url: Option<String>,
    /// Region for --url.
    #[arg(
        long,
        global = true,
        default_value = "us-east-1",
        help_heading = "Global options"
    )]
    pub region: String,
    /// Path-style addressing for --url.
    #[arg(long, global = true, help_heading = "Global options")]
    pub path_style: bool,
    /// A PEM CA bundle for --url.
    #[arg(long, global = true, help_heading = "Global options")]
    pub ca_file: Option<std::path::PathBuf>,
    /// Requests per second per job.
    #[arg(long, global = true, help_heading = "Global options")]
    rate: Option<f64>,
    /// Requests in flight per job.
    #[arg(long, global = true, help_heading = "Global options")]
    concurrency: Option<usize>,
    /// Write sonde's own requests of this run (redacted) to a file: .har or
    /// .json. Not for run, proxy and watch: what they record is the session.
    #[arg(long, global = true, help_heading = "Global options")]
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
    /// A local S3 endpoint that records what talks to it: each request as it arrives, one session, its summary at Ctrl-C.
    Proxy(cmd::record::ProxyArgs),
    /// The recorded sessions, newest first; prune old ones.
    Sessions(cmd::sessions::SessionsArgs),
    /// A proxy left running: a live status line, each new warning, rotating session files.
    Watch(cmd::record::WatchArgs),
    /// Production traffic as a session: an S3 server access log, CloudTrail events or a HAR.
    Import(cmd::sessions::ImportArgs),
    /// A session for other tools: OpenTelemetry traces, to a file or a collector.
    Export(cmd::sessions::ExportArgs),
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
    /// What a prefix holds, decoded: SlateDB, Iceberg, lakeFS, Woodpecker, turbopuffer, WarpStream, or any prefix's write cadence.
    Layout(cmd::layout::LayoutArgs),
    /// Configured endpoints.
    Endpoints(cmd::endpoints::EndpointArgs),
    /// The terminal UI.
    #[cfg(feature = "tui")]
    Tui(cmd::tui::TuiArgs),
}

/// The commands as `sonde --help` lists them: by what they are for, not in
/// one list of eighteen.
const GROUPS: &[(&str, &[&str])] = &[
    ("Record", &["demo", "run", "proxy", "watch", "import"]),
    (
        "Read a session",
        &["sessions", "show", "compare", "check", "share", "export"],
    ),
    ("Replay on another store", &["replay", "diff"]),
    (
        "The store itself",
        &["probe", "mpu", "du", "layout", "endpoints"],
    ),
    ("More", &["tui"]),
];

/// The top-level command with its help grouped. Each line is the command's
/// own (its doc comment), so the two cannot drift; a command no group names
/// is listed under the last.
fn grouped(cmd: clap::Command) -> clap::Command {
    let about = |c: &clap::Command| c.get_about().map(|a| a.to_string()).unwrap_or_default();
    let mut named: Vec<(String, String)> = cmd
        .get_subcommands()
        .filter(|c| !c.is_hide_set())
        .map(|c| (c.get_name().to_string(), about(c)))
        .collect();
    let width = named.iter().map(|(n, _)| n.len()).max().unwrap_or(0).max(4);
    let mut help = String::from("{about-with-newline}\n{usage-heading} {usage}\n");
    for (i, (heading, names)) in GROUPS.iter().enumerate() {
        let mut lines: Vec<(String, String)> = names
            .iter()
            .filter_map(|n| {
                let at = named.iter().position(|(name, _)| name == n)?;
                Some(named.remove(at))
            })
            .collect();
        if i + 1 == GROUPS.len() {
            lines.append(&mut named);
            lines.push((
                "help".into(),
                "Print this message or the help of a command: sonde help run".into(),
            ));
        }
        help.push_str(&format!("\n{heading}:\n"));
        for (name, about) in lines {
            help.push_str(&format!("  {name:<width$}  {about}\n"));
        }
    }
    help.push_str("\nOptions:\n{options}");
    scoped(cmd.help_template(help))
}

/// The commands that only read what is on this machine: a session's file,
/// the configuration. The options that say which store to talk to, and how
/// fast, are not theirs.
const READS_FILES: &[&str] = &[
    "sessions", "show", "compare", "check", "diff", "share", "export", "import",
];

/// The options of the endpoint and of the pace, which a command that reads
/// files has no use for: left out of its help (they are still taken, as
/// every global option is).
const OF_THE_STORE: &[&str] = &[
    "url",
    "region",
    "path_style",
    "ca_file",
    "rate",
    "concurrency",
    "log_out",
];

/// The command tree with each command's help showing the global options
/// it can use.
fn scoped(mut cmd: clap::Command) -> clap::Command {
    // a command that has an option of its own under a global one's id keeps
    // its own: the same option, hidden
    let hidden: Vec<clap::Arg> = cmd
        .get_arguments()
        .filter(|a| OF_THE_STORE.contains(&a.get_id().as_str()))
        .map(|a| a.clone().hide(true))
        .collect();
    for name in READS_FILES {
        cmd = cmd.mut_subcommand(name, |sub| sub.args(hidden.clone()));
    }
    cmd
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
    let parsed = grouped(Cli::command())
        .try_get_matches()
        .and_then(|m| Cli::from_arg_matches(&m));
    let cli = match parsed {
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
        // run, proxy and watch record the program's requests, not sonde's: the
        // file would be written empty
        if let (Some(path), Some(what)) = (
            &cli.log_out,
            match &cli.command {
                Command::Run(_) => Some("run"),
                Command::Proxy(_) => Some("proxy"),
                Command::Watch(_) => Some("watch"),
                _ => None,
            },
        ) {
            eprintln!(
                "sonde: --log-out {} is not for `sonde {what}`: it writes sonde's own requests, and {what} sends none. What the program sent is the session file (-o FILE, or the one named when it ends); `sonde show FILE -f json` has every request, `sonde export FILE` its traces.",
                path.display()
            );
            return 64;
        }
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
            Command::Import(a) => cmd::sessions::import(&sonde, a).await,
            Command::Export(a) => cmd::sessions::export(&sonde, a).await,
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
            // a mistake in the command or in sonde.toml: 64, as the docs say
            Err(e) if e.downcast_ref::<cmd::Usage>().is_some() => {
                eprintln!("sonde: {e:#}");
                64
            }
            Err(e) => {
                eprintln!("sonde: {}", cmd::said(&sonde, &format!("{e:#}")));
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
    // an exit status past what a process can return (Windows' are 32-bit)
    // still says it failed: 1, not its low byte (256 would read as 0)
    ExitCode::from(match code {
        0..=255 => code as u8,
        _ => 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_is_in_one_group_of_the_help() {
        let cmd = Cli::command();
        let grouped: Vec<&str> = GROUPS.iter().flat_map(|(_, n)| n.iter().copied()).collect();
        for c in cmd.get_subcommands().filter(|c| !c.is_hide_set()) {
            assert_eq!(
                grouped.iter().filter(|n| **n == c.get_name()).count(),
                1,
                "{} is in no group, or in two",
                c.get_name()
            );
        }
        // and no group names a command that is gone (tui is there by feature)
        for name in grouped.iter().filter(|n| **n != "tui") {
            assert!(
                cmd.get_subcommands().any(|c| c.get_name() == *name),
                "{name} is no command"
            );
        }
    }
}
