use std::sync::Arc;

use clap::{Args, Subcommand};
use probes::runner::RunOptions;
use probes::{Run, Verdict};
use sonde_core::Sonde;

use crate::Cli;
use crate::output::{self, Format};

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ProbeArgs {
    #[command(subcommand)]
    action: Option<ProbeAction>,
    /// Endpoint ids to probe, in parallel (repeatable).
    #[arg(long = "endpoint", short = 'e')]
    endpoints: Vec<String>,
    /// The scratch bucket (it must exist; probes write under sonde-probe/ and clean up).
    #[arg(long, short = 'b')]
    bucket: Option<String>,
    /// Probe ids or prefixes to run (repeatable): `checksum.`, `mpu.min-part-size`.
    #[arg(long)]
    only: Vec<String>,
    /// Let probes create and remove temporary buckets (versioning, Object Lock).
    #[arg(long)]
    temp_buckets: bool,
    /// Skip probes that wait in real time.
    #[arg(long)]
    no_slow: bool,
    /// The format written: text or json.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    format: Format,
    /// Write the report here instead of stdout.
    #[arg(long, short = 'o')]
    out: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
enum ProbeAction {
    /// The probes and the AWS behavior each checks.
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
    /// Saved runs, newest first.
    Runs {
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
    /// What changed between two saved runs (`<run>:<endpoint>`).
    Diff {
        /// The run before, `<run>:<endpoint>` as `sonde probe runs` lists it.
        before: String,
        /// The run after, the same way.
        after: String,
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
}

fn load(s: &Sonde, spec: &str) -> anyhow::Result<Run> {
    let (id, ep) = spec
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("a run is <run-id>:<endpoint> (see `sonde probe runs`)"))?;
    probes::store::load(s.paths(), id, ep).ok_or_else(|| anyhow::anyhow!("no saved run {spec}"))
}

pub async fn run(s: &Arc<Sonde>, cli: &Cli, a: &ProbeArgs) -> anyhow::Result<i32> {
    let format = match &a.action {
        Some(ProbeAction::List { format })
        | Some(ProbeAction::Runs { format })
        | Some(ProbeAction::Diff { format, .. }) => *format,
        None => a.format,
    };
    match &a.action {
        Some(ProbeAction::List { .. }) => {
            let c = probes::catalog();
            match format {
                Format::Json => output::print_json(&c),
                _ => println!(
                    "{}",
                    output::table(
                        &["id", "category", "title"],
                        &c.iter()
                            .map(|m| vec![m.id.clone(), m.category.label().into(), m.title.clone()])
                            .collect::<Vec<_>>()
                    )
                ),
            }
            return Ok(0);
        }
        Some(ProbeAction::Runs { .. }) => {
            let r = probes::store::list(s.paths());
            match format {
                Format::Json => output::print_json(&r),
                _ => println!(
                    "{}",
                    output::table(
                        &["run", "endpoint", "vendor", "started", "✓", "✗", "≠", "∅"],
                        &r.iter()
                            .map(|x| vec![
                                format!("{}:{}", x.id, x.endpoint),
                                x.endpoint_name.clone(),
                                x.vendor.clone(),
                                x.started.clone(),
                                x.pass.to_string(),
                                x.fail.to_string(),
                                x.differs.to_string(),
                                x.unsupported.to_string()
                            ])
                            .collect::<Vec<_>>()
                    )
                ),
            }
            return Ok(0);
        }
        Some(ProbeAction::Diff { before, after, .. }) => {
            let (b, af) = (load(s, before)?, load(s, after)?);
            let d = probes::store::diff(&b, &af);
            match format {
                Format::Json => output::print_json(&d),
                _ if d.is_empty() => println!("no change"),
                _ => println!(
                    "{}",
                    output::table(
                        &["probe", "before", "after", "now"],
                        &d.iter()
                            .map(|c| vec![
                                c.probe.clone(),
                                c.before.map(|v| v.label()).unwrap_or("—").into(),
                                c.after.map(|v| v.label()).unwrap_or("—").into(),
                                c.after_summary.clone().unwrap_or_default()
                            ])
                            .collect::<Vec<_>>()
                    )
                ),
            }
            return Ok(if d.is_empty() { 0 } else { 4 });
        }
        None => {}
    }
    // naming the scratch bucket on the command line is the consent; the run is audited
    let endpoints = if a.endpoints.is_empty() {
        vec![super::endpoint(s, None)?]
    } else {
        a.endpoints
            .iter()
            .map(|e| super::endpoint(s, Some(e)))
            .collect::<anyhow::Result<Vec<_>>>()?
    };
    let bucket = a
        .bucket
        .clone()
        .ok_or_else(|| anyhow::anyhow!("--bucket: a scratch bucket the probes may write to"))?;
    let opts = RunOptions {
        bucket,
        allow_temp_buckets: a.temp_buckets,
        include_slow: !a.no_slow,
        only: a.only.clone(),
        ..Default::default()
    };
    let jobs = probes::runner::spawn(s, &endpoints, opts);
    let mut runs = vec![];
    // every endpoint's run is waited to its end, so that each cleans up
    let mut failed = false;
    for (r, ep) in super::wait_all(s.jobs(), &jobs, false)
        .await
        .into_iter()
        .zip(&endpoints)
    {
        match r {
            Ok(v) => runs.push(serde_json::from_value::<Run>(v)?),
            Err(e) => {
                eprintln!("sonde: probes on {ep}: {e}");
                failed = true;
            }
        }
    }
    if failed {
        return Ok(70);
    }
    let text = match a.format {
        Format::Json => output::json(&runs),
        Format::Text => {
            let m = probes::report::matrix(&runs);
            let mut headers = vec!["probe"];
            headers.extend(m.columns.iter().map(|c| c.name.as_str()));
            // what did not pass says why, in its row: on several endpoints,
            // each named
            headers.push("why");
            let several = m.columns.len() > 1;
            let rows: Vec<Vec<String>> = m
                .rows
                .iter()
                .map(|r| {
                    let why: Vec<String> = r
                        .cells
                        .iter()
                        .zip(&m.columns)
                        .filter_map(|(c, col)| {
                            let c = c.as_ref().filter(|c| c.verdict != Verdict::Pass)?;
                            Some(if several {
                                format!("{}: {}", col.name, c.summary)
                            } else {
                                c.summary.clone()
                            })
                        })
                        .collect();
                    std::iter::once(r.probe.id.clone())
                        .chain(r.cells.iter().map(|c| {
                            c.as_ref()
                                .map(|c| format!("{} {}", c.verdict.symbol(), c.verdict.label()))
                                .unwrap_or_default()
                        }))
                        .chain([why.join("; ")])
                        .collect()
                })
                .collect();
            let mut t = output::table(&headers, &rows);
            for r in &runs {
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
                    let n = r.count(v);
                    let word = match v {
                        Verdict::Differs => "differ",
                        Verdict::Error if n != 1 => "errors",
                        _ => v.label(),
                    };
                    (n > 0 || matches!(v, Verdict::Pass | Verdict::Fail))
                        .then(|| format!("{n} {word}"))
                })
                .collect();
                t.push_str(&format!(
                    "\n\n{}: {} probes: {} — saved as {}:{}",
                    r.endpoint_name,
                    r.results.len(),
                    counts.join(", "),
                    r.id,
                    r.endpoint
                ));
            }
            t
        }
    };
    match &a.out {
        Some(p) => std::fs::write(p, text)?,
        None => println!("{text}"),
    }
    let _ = cli;
    Ok(probes::report::exit_code(&runs))
}
