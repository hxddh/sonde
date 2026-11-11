use std::sync::Arc;

use clap::Args;
use sonde_core::Sonde;
use sonde_core::services::du;

use crate::Cli;
use crate::output::{self, Format};

#[derive(Args)]
pub struct DuArgs {
    #[arg(long, short = 'e')]
    endpoint: Option<String>,
    #[arg(long, short = 'b')]
    bucket: String,
    #[arg(long, short = 'p', default_value = "")]
    prefix: String,
    /// Leave out incomplete multipart uploads.
    #[arg(long)]
    no_multipart: bool,
    /// Print the last cached scan instead of scanning.
    #[arg(long)]
    cached: bool,
    /// The format written: table, json or csv.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    format: Format,
}

pub async fn run(s: &Arc<Sonde>, _cli: &Cli, a: &DuArgs) -> anyhow::Result<i32> {
    if let Some(why) =
        output::unsupported(a.format, "du", &[Format::Table, Format::Json, Format::Csv])
    {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    let ep = super::endpoint(s, a.endpoint.as_deref())?;
    let r = if a.cached {
        du::load_cache(s, &ep, &a.bucket, &a.prefix)
            .ok_or_else(|| anyhow::anyhow!("no cached scan of {}/{}", a.bucket, a.prefix))?
    } else {
        serde_json::from_value(
            super::wait(
                s,
                &du::scan(s, &ep, &a.bucket, &a.prefix, !a.no_multipart),
                false,
            )
            .await?,
        )?
    };
    let h = sonde_core::guard::human_bytes;
    match a.format {
        Format::Json => println!("{}", output::json(&r)),
        Format::Csv => print!("{}", du::to_csv(&r)),
        _ => {
            println!(
                "{}/{}: {} total ({:.2} {}/month), via {}\n",
                r.bucket,
                r.prefix,
                h(r.total_bytes),
                r.cost_per_month,
                r.currency,
                r.source
            );
            println!(
                "{}",
                output::table(
                    &["", "objects", "bytes"],
                    &[
                        vec![
                            "current".into(),
                            r.current.objects.to_string(),
                            h(r.current.bytes)
                        ],
                        vec![
                            "noncurrent".into(),
                            // not permitted to list versions: unknown, not 0
                            if r.versions_unknown.is_some() {
                                "unknown".into()
                            } else {
                                r.noncurrent.objects.to_string()
                            },
                            if r.versions_unknown.is_some() {
                                "not counted".into()
                            } else {
                                h(r.noncurrent.bytes)
                            }
                        ],
                        vec![
                            "delete markers".into(),
                            if r.versions_unknown.is_some() {
                                "unknown".into()
                            } else {
                                r.delete_markers.to_string()
                            },
                            "—".into()
                        ],
                        vec![
                            "incomplete uploads".into(),
                            r.multipart.objects.to_string(),
                            h(r.multipart.bytes)
                        ],
                    ]
                )
            );
            println!(
                "\n{}",
                output::table(
                    &["storage class", "current", "noncurrent"],
                    &r.by_class
                        .iter()
                        .map(|c| vec![c.name.clone(), h(c.current.bytes), h(c.noncurrent.bytes)])
                        .collect::<Vec<_>>()
                )
            );
            println!(
                "\n{}",
                output::table(
                    &["prefix", "objects", "current", "noncurrent", "markers"],
                    &r.by_prefix
                        .iter()
                        .take(20)
                        .map(|c| vec![
                            c.name.clone(),
                            (c.current.objects + c.noncurrent.objects).to_string(),
                            h(c.current.bytes),
                            h(c.noncurrent.bytes),
                            c.delete_markers.to_string()
                        ])
                        .collect::<Vec<_>>()
                )
            );
            println!(
                "\n{}",
                output::table(
                    &["size", "objects", "bytes"],
                    &r.by_size
                        .iter()
                        .map(|b| vec![b.label.clone(), b.objects.to_string(), h(b.bytes)])
                        .collect::<Vec<_>>()
                )
            );
            println!("\nWhy so big:");
            for f in &r.findings {
                println!("  • {}", super::said(s, f));
            }
        }
    }
    Ok(0)
}
