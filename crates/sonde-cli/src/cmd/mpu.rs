use std::sync::Arc;

use clap::{Args, Subcommand};
use sonde_core::Sonde;
use sonde_core::services::{exec, mpu};

use crate::Cli;
use crate::output::{self, Format};

#[derive(Args)]
pub struct MpuArgs {
    #[command(subcommand)]
    action: MpuAction,
}

#[derive(Args, Clone)]
struct Target {
    #[arg(long, short = 'e')]
    endpoint: Option<String>,
    #[arg(long, short = 'b')]
    bucket: String,
    #[arg(long, short = 'p', default_value = "")]
    prefix: String,
    /// The format written: table, json or csv (scan).
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    format: Format,
}

#[derive(Subcommand)]
enum MpuAction {
    /// Lists incomplete uploads with their parts' bytes, age and cost, and checks the lifecycle.
    Scan(Target),
    /// Aborts stale uploads: a dry run, unless --confirm repeats the count it prints.
    Abort {
        #[command(flatten)]
        target: Target,
        /// Only uploads older than this many days (default: the stale threshold).
        #[arg(long)]
        older_than_days: Option<f64>,
        /// The number of uploads the dry run listed.
        #[arg(long)]
        confirm: Option<String>,
    },
    /// Adds an AbortIncompleteMultipartUpload rule: a dry run, unless --confirm gives the bucket name.
    Lifecycle {
        #[command(flatten)]
        target: Target,
        #[arg(long, default_value_t = 7)]
        days: u32,
        #[arg(long)]
        confirm: Option<String>,
    },
}

fn human(b: u64) -> String {
    sonde_core::guard::human_bytes(b)
}

pub async fn run(s: &Arc<Sonde>, _cli: &Cli, a: &MpuArgs) -> anyhow::Result<i32> {
    // the scan is data; a dry run and its confirmation are read by a person
    let (format, command, writes): (_, _, &[Format]) = match &a.action {
        MpuAction::Scan(t) => (
            t.format,
            "mpu scan",
            &[Format::Table, Format::Json, Format::Csv],
        ),
        MpuAction::Abort { target, .. } => (target.format, "mpu abort", &[Format::Table]),
        MpuAction::Lifecycle { target, .. } => (target.format, "mpu lifecycle", &[Format::Table]),
    };
    if let Some(why) = output::unsupported(format, command, writes) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    match &a.action {
        MpuAction::Scan(t) => {
            let ep = super::endpoint(s, t.endpoint.as_deref())?;
            let r: mpu::MpuReport = serde_json::from_value(
                super::wait(s, &mpu::scan(s, &ep, &t.bucket, &t.prefix), false).await?,
            )?;
            match t.format {
                Format::Json => println!("{}", output::json(&r)),
                Format::Csv => {
                    println!("key,upload_id,initiated,age_days,parts,bytes,cost_per_month,stale");
                    for u in &r.uploads {
                        println!(
                            "{},{},{},{:.2},{},{},{:.4},{}",
                            u.key,
                            u.upload_id,
                            u.initiated.clone().unwrap_or_default(),
                            u.age_days.unwrap_or(0.0),
                            u.parts,
                            u.bytes,
                            u.cost_per_month,
                            u.stale
                        );
                    }
                }
                _ => {
                    let rows: Vec<Vec<String>> = r
                        .uploads
                        .iter()
                        .map(|u| {
                            vec![
                                if u.stale { "!".into() } else { String::new() },
                                u.key.clone(),
                                u.upload_id.chars().take(16).collect(),
                                format!("{:.1} d", u.age_days.unwrap_or(0.0)),
                                u.parts.to_string(),
                                human(u.bytes),
                                format!("{:.2}", u.cost_per_month),
                            ]
                        })
                        .collect();
                    println!(
                        "{}",
                        output::table(
                            &[
                                "",
                                "key",
                                "upload id",
                                "age",
                                "parts",
                                "bytes",
                                &format!("{}/month", r.currency)
                            ],
                            &rows
                        )
                    );
                    println!(
                        "\n{}, {} stale (> {} days): {} ({} stale), {:.2} {}/month",
                        output::count(r.total_uploads as usize, "upload"),
                        r.stale_uploads,
                        r.threshold_days,
                        human(r.total_bytes),
                        human(r.stale_bytes),
                        r.cost_per_month,
                        r.currency
                    );
                    println!("Lifecycle: {}", r.lifecycle.message);
                }
            }
            Ok(
                if r.stale_uploads > 0 || r.lifecycle.status != mpu::LifecycleStatus::Covered {
                    4
                } else {
                    0
                },
            )
        }
        MpuAction::Abort {
            target: t,
            older_than_days,
            confirm,
        } => {
            let ep = super::endpoint(s, t.endpoint.as_deref())?;
            let r: mpu::MpuReport = serde_json::from_value(
                super::wait(s, &mpu::scan(s, &ep, &t.bucket, &t.prefix), true).await?,
            )?;
            let min = older_than_days.unwrap_or(r.threshold_days as f64);
            let chosen: Vec<mpu::UploadRow> = r
                .uploads
                .into_iter()
                .filter(|u| u.age_days.unwrap_or(0.0) > min)
                .collect();
            if chosen.is_empty() {
                println!("no upload older than {min} days");
                return Ok(0);
            }
            let plan = mpu::plan_abort(s, &ep, &t.bucket, &chosen);
            for i in &plan.items {
                println!(
                    "  abort {}  {}  {}",
                    i.key,
                    i.id.clone().unwrap_or_default(),
                    i.detail
                );
            }
            println!("{}", plan.summary);
            let Some(c) = confirm else {
                println!(
                    "dry run: to abort, run again with --confirm {} (and `read-only = false` under [settings] in {})",
                    plan.confirm,
                    s.paths().config_file().display()
                );
                return Ok(4);
            };
            let o = exec::execute(s, &plan.id, c).await?;
            println!("aborted {}, failed {}", o.succeeded, o.failed);
            for e in &o.errors {
                eprintln!("  {e}");
            }
            Ok(if o.failed > 0 { 70 } else { 0 })
        }
        MpuAction::Lifecycle {
            target: t,
            days,
            confirm,
        } => {
            let ep = super::endpoint(s, t.endpoint.as_deref())?;
            let plan = mpu::plan_lifecycle_rule(s, &ep, &t.bucket, &t.prefix, *days).await?;
            println!(
                "current:\n{}\n\nnew:\n{}",
                plan.previous_document
                    .clone()
                    .unwrap_or_else(|| "(none)".into()),
                plan.document.clone().unwrap_or_default()
            );
            let Some(c) = confirm else {
                println!(
                    "\ndry run: to apply, run again with --confirm {} (and `read-only = false` under [settings] in {})",
                    plan.confirm,
                    s.paths().config_file().display()
                );
                return Ok(4);
            };
            let o = exec::execute(s, &plan.id, c).await?;
            if let Some(e) = o.errors.first() {
                eprintln!("{e}");
                return Ok(70);
            }
            println!("applied");
            Ok(0)
        }
    }
}
