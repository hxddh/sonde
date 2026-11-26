//! Incomplete multipart uploads: `sonde bucket uploads`, `abort-uploads`
//! and `lifecycle`.

use std::sync::Arc;

use clap::{Args, Subcommand};
use sonde_core::Sonde;
use sonde_core::config::STALE_UPLOAD_DAYS;
use sonde_core::guard::{Action, Guard, Outcome, Plan, Scope};
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
    /// Aborts uploads older than a week: a dry run, unless --yes.
    #[command(name = "abort-uploads")]
    Abort {
        #[command(flatten)]
        target: Target,
        /// Only uploads older than this many days (default: 7).
        #[arg(long)]
        older_than_days: Option<f64>,
        /// Abort the uploads this command's dry run listed (in the last 10 minutes), and no other.
        #[arg(long)]
        yes: bool,
        /// With --yes: only if the dry run to confirm is this one (the id it printed).
        #[arg(long, value_name = "ID", requires = "yes")]
        plan: Option<String>,
    },
    /// Adds an AbortIncompleteMultipartUpload rule, the bucket's other rules kept: a dry run, unless --yes.
    Lifecycle {
        #[command(flatten)]
        target: Target,
        /// Abort an upload this many days after it began.
        #[arg(long, default_value_t = 7)]
        days: u32,
        /// Write the document this command's dry run showed (in the last 10 minutes), if the bucket's is still the one it read.
        #[arg(long)]
        yes: bool,
        /// With --yes: only if the dry run to confirm is this one (the id it printed).
        #[arg(long, value_name = "ID", requires = "yes")]
        plan: Option<String>,
    },
}

fn human(b: u64) -> String {
    sonde_core::guard::human_bytes(b)
}

/// A lifecycle document's rules, one line each, as a person reads them:
/// `abort-mpu (logs/): abort uploads 3 days after they began`. A document
/// that does not read is shown as it is.
fn rules_text(doc: Option<&str>) -> Vec<String> {
    let Some(doc) = doc else {
        return Vec::new();
    };
    let Ok(lc) = s3raw::xml::lifecycle(doc) else {
        return vec![doc.to_string()];
    };
    let days = |n: u32| format!("{n} day{}", if n == 1 { "" } else { "s" });
    lc.rule
        .iter()
        .map(|r| {
            let mut does = Vec::new();
            if let Some(n) = r
                .abort_incomplete_multipart_upload
                .as_ref()
                .and_then(|a| a.days_after_initiation)
            {
                does.push(format!("abort uploads {} after they began", days(n)));
            }
            if let Some(e) = &r.expiration {
                match (e.days, &e.date) {
                    (Some(n), _) => does.push(format!("expire objects after {}", days(n))),
                    (None, Some(d)) => does.push(format!("expire objects on {d}")),
                    _ if e.expired_object_delete_marker == Some(true) => {
                        does.push("remove expired delete markers".into())
                    }
                    _ => {}
                }
            }
            if let Some(n) = r
                .noncurrent_version_expiration
                .as_ref()
                .and_then(|n| n.noncurrent_days)
            {
                does.push(format!("expire old versions after {}", days(n)));
            }
            if does.is_empty() {
                does.push("other actions".into());
            }
            let scope = match r.scope_prefix() {
                Some(p) if p.is_empty() => "the whole bucket".to_string(),
                Some(p) => p,
                None => "a filter".to_string(),
            };
            format!(
                "{} ({scope}{}): {}",
                r.id.as_deref().unwrap_or("(no id)"),
                if r.enabled() { "" } else { ", disabled" },
                does.join(", ")
            )
        })
        .collect()
}

/// The command a dry run's plan is kept for: `--yes` runs the plan of the
/// same command only (endpoint, bucket, prefix, its own option).
fn scope(s: &Sonde, action: Action, ep: &str, t: &Target, option: String) -> Scope {
    Scope {
        action,
        endpoint: ep.to_string(),
        url: s.endpoint_config(ep).map(|e| e.url).unwrap_or_default(),
        bucket: t.bucket.clone(),
        arguments: format!("prefix={} {option}", t.prefix),
    }
}

/// Keeps a dry run's plan for its `--yes`, and says until when.
fn keep(s: &Sonde, scope: &Scope, plan: &Plan) -> anyhow::Result<String> {
    s.guard().save(scope, plan).map_err(|e| {
        anyhow::anyhow!("the dry run could not be kept ({e}): --yes would have no plan to run")
    })?;
    Ok(format!(
        "plan {}, kept until {}",
        plan.id,
        Guard::expires(plan).format("%H:%M:%S UTC")
    ))
}

/// Whether the outcome is on record: said on stderr when the audit log
/// could not take it (the change itself was made).
fn on_record(o: &Outcome) -> bool {
    if let Some(e) = &o.audit_error {
        eprintln!(
            "sonde: warning: the change was made, but its outcome could not be written to the audit log ({e}): only its \"started\" line is there"
        );
    }
    o.audit_error.is_none()
}

fn print_items(plan: &Plan) {
    for i in &plan.items {
        println!(
            "  abort {}  {}  {}",
            i.key,
            i.id.clone().unwrap_or_default(),
            i.detail
        );
    }
    println!("{}", plan.summary);
}

pub(super) async fn act(s: &Arc<Sonde>, action: &MpuAction) -> anyhow::Result<i32> {
    match action {
        MpuAction::Scan { target: t, format } => {
            let ep = super::endpoint(s, t.endpoint.as_deref())?;
            let r: mpu::MpuReport = serde_json::from_value(
                super::wait(s, &mpu::scan(s, &ep, &t.bucket, &t.prefix), false).await?,
            )?;
            match format {
                Format::Json => output::print_json(&r),
                _ if r.uploads.is_empty() => {
                    println!("no incomplete uploads in {}/{}", t.bucket, t.prefix);
                    println!("Lifecycle: {}", r.lifecycle.message);
                }
                _ => {
                    // a cost too small for cents is said in its digits, not 0.00
                    let money = |v: f64| {
                        sonde_analysis::program::money_text(v, &r.currency)
                            .trim_end_matches(&r.currency)
                            .trim_end()
                            .to_string()
                    };
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
                                money(u.cost_per_month),
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
                        "\n{}, {} stale (> {} days): {} ({} stale), {}/month",
                        output::count(r.total_uploads as usize, "upload"),
                        r.stale_uploads,
                        r.threshold_days,
                        human(r.total_bytes),
                        human(r.stale_bytes),
                        sonde_analysis::program::money_text(r.cost_per_month, &r.currency),
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
            yes,
            plan: id,
        } => {
            let ep = super::endpoint(s, t.endpoint.as_deref())?;
            let min = older_than_days.unwrap_or(STALE_UPLOAD_DAYS as f64);
            let scope = scope(
                s,
                Action::AbortUploads,
                &ep,
                t,
                format!("older-than-days={min}"),
            );
            // what --yes runs is the dry run's plan, taken before anything
            // is sent: never one made from what the bucket holds by now
            let confirmed = match yes {
                true => Some(s.guard().take_saved(&scope, id.as_deref())?),
                false => None,
            };
            if confirmed.as_ref().is_some_and(|p| p.items.is_empty()) {
                println!("the dry run listed no upload older than {min} days: nothing to abort");
                return Ok(0);
            }
            let r: mpu::MpuReport = serde_json::from_value(
                super::wait(s, &mpu::scan(s, &ep, &t.bucket, &t.prefix), true).await?,
            )?;
            let chosen: Vec<mpu::UploadRow> = r
                .uploads
                .into_iter()
                .filter(|u| u.age_days.unwrap_or(0.0) > min)
                .collect();
            let Some(plan) = confirmed else {
                let plan = mpu::plan_abort(s, &ep, &t.bucket, &chosen);
                let kept = keep(s, &scope, &plan)?;
                if chosen.is_empty() {
                    println!("no upload older than {min} days");
                    return Ok(0);
                }
                print_items(&plan);
                println!("\ndry run ({kept}): to abort them, run again with --yes");
                return Ok(4);
            };
            // each upload it listed, as it listed it: else nothing is aborted
            mpu::as_planned(&plan, &chosen)?;
            print_items(&plan);
            let unlisted = chosen
                .iter()
                .filter(|u| {
                    !plan
                        .items
                        .iter()
                        .any(|i| i.key == u.key && i.id.as_deref() == Some(u.upload_id.as_str()))
                })
                .count();
            let o = exec::run(s, plan).await?;
            println!("aborted {}, failed {}", o.succeeded, o.failed);
            for e in &o.errors {
                eprintln!("  {e}");
            }
            if unlisted > 0 {
                eprintln!(
                    "{} older than {min} days now, not in the dry run: left as {}. Run the dry run again to list {}",
                    output::count(unlisted, "more upload"),
                    if unlisted == 1 { "it is" } else { "they are" },
                    if unlisted == 1 { "it" } else { "them" },
                );
            }
            let recorded = on_record(&o);
            Ok(if o.failed > 0 || !recorded { 70 } else { 0 })
        }
        MpuAction::Lifecycle {
            target: t,
            days,
            yes,
            plan: id,
        } => {
            let ep = super::endpoint(s, t.endpoint.as_deref())?;
            let scope = scope(s, Action::PutLifecycle, &ep, t, format!("days={days}"));
            // --yes writes the document the dry run showed, and only over the
            // one it read (checked as it runs): never one merged anew
            let plan = match yes {
                true => s.guard().take_saved(&scope, id.as_deref())?,
                false => mpu::plan_lifecycle_rule(s, &ep, &t.bucket, &t.prefix, *days).await?,
            };
            // the rules, read: what the bucket has, and what it would have
            let before = rules_text(plan.previous_document.as_deref());
            let after = rules_text(plan.document.as_deref());
            println!("lifecycle rules of {}:", t.bucket);
            if before.is_empty() {
                println!("  (none)");
            }
            for line in &before {
                println!("  {line}");
            }
            println!("the rule to add:");
            for line in after.iter().filter(|l| !before.contains(l)) {
                println!("  {line}");
            }
            if !yes {
                let kept = keep(s, &scope, &plan)?;
                println!("\ndry run ({kept}): to add the rule, run again with --yes");
                return Ok(4);
            }
            let o = exec::run(s, plan).await?;
            if let Some(e) = o.errors.first() {
                eprintln!("{e}");
                return Ok(70);
            }
            println!("applied");
            Ok(if on_record(&o) { 0 } else { 70 })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 20.0's dry run printed both lifecycle documents as raw XML.
    #[test]
    fn a_lifecycle_rule_is_said_as_a_person_reads_it() {
        let before = "<LifecycleConfiguration><Rule><ID>logs</ID><Filter><Prefix>logs/</Prefix></Filter><Status>Enabled</Status><Expiration><Days>30</Days></Expiration></Rule></LifecycleConfiguration>";
        assert_eq!(
            rules_text(Some(before)),
            ["logs (logs/): expire objects after 30 days"]
        );
        let after = s3raw::xml::add_rule(
            Some(before),
            &s3raw::xml::abort_rule_xml("sonde-abort-incomplete-mpu", "", 1),
        );
        assert_eq!(
            rules_text(Some(&after))[1],
            "sonde-abort-incomplete-mpu (the whole bucket): abort uploads 1 day after they began"
        );
        assert!(rules_text(None).is_empty());
    }
}
