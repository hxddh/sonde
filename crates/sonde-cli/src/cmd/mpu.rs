//! Incomplete multipart uploads: `sonde bucket uploads`, `abort-uploads`
//! and `lifecycle`.

use std::sync::Arc;

use clap::{Args, Subcommand};
use sonde_core::Sonde;
use sonde_core::services::{exec, mpu};

use crate::output::{self, Format};

#[derive(Args, Clone)]
pub(super) struct Target {
    /// The endpoint (default: --url, or the only one configured).
    #[arg(long, short = 'e')]
    endpoint: Option<String>,
    /// The bucket.
    #[arg(long, short = 'b')]
    bucket: String,
    /// Only uploads under this key prefix (default: the whole bucket).
    #[arg(long, short = 'p', default_value = "", hide_default_value = true)]
    prefix: String,
}

#[derive(Subcommand)]
pub(super) enum MpuAction {
    /// Incomplete multipart uploads: their parts' bytes, age and cost, and the lifecycle rule.
    #[command(name = "uploads")]
    Scan {
        #[command(flatten)]
        target: Target,
        /// The format written: table, json or csv.
        #[arg(
            long,
            short = 'f',
            value_enum,
            default_value_t,
            hide_possible_values = true
        )]
        format: Format,
    },
    /// Aborts uploads older than a week: a dry run, unless --confirm repeats the count it prints.
    #[command(name = "abort-uploads")]
    Abort {
        #[command(flatten)]
        target: Target,
        /// Only uploads older than this many days (default: 7).
        #[arg(long)]
        older_than_days: Option<f64>,
        /// The number of uploads the dry run listed: aborts them.
        #[arg(long, value_name = "COUNT")]
        confirm: Option<String>,
    },
    /// Adds an AbortIncompleteMultipartUpload rule, the bucket's other rules kept: a dry run, unless --yes.
    Lifecycle {
        #[command(flatten)]
        target: Target,
        /// Abort an upload this many days after it began.
        #[arg(long, default_value_t = 7)]
        days: u32,
        /// Add the rule the dry run shows.
        #[arg(long)]
        yes: bool,
    },
}

fn human(b: u64) -> String {
    sonde_core::guard::human_bytes(b)
}

pub(super) async fn act(s: &Arc<Sonde>, action: &MpuAction) -> anyhow::Result<i32> {
    // the scan is data; a dry run and its confirmation are read by a person
    // (they have no -f)
    if let MpuAction::Scan { format, .. } = action
        && let Some(why) = output::unsupported(
            *format,
            "bucket uploads",
            &[Format::Table, Format::Json, Format::Csv],
        )
    {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    match action {
        MpuAction::Scan { target: t, format } => {
            let ep = super::endpoint(s, t.endpoint.as_deref())?;
            let r: mpu::MpuReport = serde_json::from_value(
                super::wait(s, &mpu::scan(s, &ep, &t.bucket, &t.prefix), false).await?,
            )?;
            match format {
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
                                sonde_fmt::cents(u.cost_per_month),
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
                        "\n{}, {} stale (> {} days): {} ({} stale), {} {}/month",
                        output::count(r.total_uploads as usize, "upload"),
                        r.stale_uploads,
                        r.threshold_days,
                        human(r.total_bytes),
                        human(r.stale_bytes),
                        sonde_fmt::cents(r.cost_per_month),
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
                    "dry run: to abort, run again with --confirm {}",
                    plan.confirm
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
            yes,
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
            if !yes {
                println!("\ndry run: to add the rule, run again with --yes");
                return Ok(4);
            }
            let o = exec::execute(s, &plan.id, &plan.confirm).await?;
            if let Some(e) = o.errors.first() {
                eprintln!("{e}");
                return Ok(70);
            }
            println!("applied");
            Ok(0)
        }
    }
}
