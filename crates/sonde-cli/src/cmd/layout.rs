use std::sync::Arc;

use clap::{Args, Subcommand};
use decoders::{Format as Lf, Layout, Severity};
use sonde_core::Sonde;

use crate::Cli;
use crate::output::{self, Format};

#[derive(Args)]
pub struct LayoutArgs {
    #[command(subcommand)]
    action: LayoutAction,
}

#[derive(Subcommand)]
enum LayoutAction {
    /// Detects and decodes the layouts under a prefix (or a local directory with --dir).
    Inspect {
        #[arg(long, short = 'e')]
        endpoint: Option<String>,
        #[arg(long, short = 'b')]
        bucket: Option<String>,
        #[arg(long, short = 'p', default_value = "")]
        prefix: String,
        /// A local directory instead of a bucket.
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
        /// slatedb, iceberg, lakefs, woodpecker, turbopuffer, warpstream, generic (default: detect).
        #[arg(long)]
        kind: Option<String>,
        /// Replay: the logical state at this time (RFC 3339).
        #[arg(long)]
        at: Option<String>,
        /// The format written: table or json.
        #[arg(long, short = 'f', value_enum, default_value_t, hide_possible_values = true)]
        format: Format,
    },
}

fn print(l: &Layout, at: Option<chrono::DateTime<chrono::Utc>>) {
    println!(
        "{} at {:?}{}\n{}\n",
        l.format.label(),
        l.root,
        l.format_version
            .as_ref()
            .map(|v| format!(" ({v})"))
            .unwrap_or_default(),
        l.headline
    );
    if !l.decoded {
        println!("(not decoded)\n");
    }
    println!(
        "{}",
        output::kv(
            &l.facts
                .iter()
                .map(|f| (f.label.clone(), f.value.clone()))
                .collect::<Vec<_>>()
        )
    );
    if !l.issues.is_empty() {
        println!();
        for i in &l.issues {
            let tag = match i.severity {
                Severity::Error => "error",
                Severity::Warning => "warn ",
                Severity::Info => "info ",
            };
            println!("{tag} {}", i.message);
        }
    }
    for t in &l.tables {
        if t.rows.is_empty() {
            continue;
        }
        println!("\n{}", t.title);
        let h: Vec<&str> = t.columns.iter().map(String::as_str).collect();
        println!(
            "{}",
            output::table(&h, &t.rows.iter().take(50).cloned().collect::<Vec<_>>())
        );
        if t.rows.len() > 50 {
            println!("… {} more", t.rows.len() - 50);
        }
    }
    if let Some(t) = at {
        match l.state_at(t) {
            Some(s) => {
                println!("\nState at {t}: {}", s.label);
                println!(
                    "{}",
                    output::kv(
                        &s.facts
                            .iter()
                            .map(|f| (f.label.clone(), f.value.clone()))
                            .collect::<Vec<_>>()
                    )
                );
            }
            None => println!("\nNo state recorded at or before {t}"),
        }
    }
}

pub async fn run(s: &Arc<Sonde>, _cli: &Cli, a: &LayoutArgs) -> anyhow::Result<i32> {
    let LayoutAction::Inspect {
        endpoint,
        bucket,
        prefix,
        dir,
        kind,
        at,
        format,
    } = &a.action;
    if let Some(why) =
        output::unsupported(*format, "layout inspect", &[Format::Table, Format::Json])
    {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    let at = at
        .as_deref()
        .map(|t| chrono::DateTime::parse_from_rfc3339(t).map(|d| d.with_timezone(&chrono::Utc)))
        .transpose()
        .map_err(|e| anyhow::anyhow!("--at: {e}"))?;
    let kind = kind
        .as_deref()
        .map(|k| Lf::from_id(k).ok_or_else(|| anyhow::anyhow!("--kind {k}: unknown")))
        .transpose()?;
    let layouts: Vec<Layout> = if let Some(d) = dir {
        let src = decoders::DirSource::new(d);
        let targets = match kind {
            Some(k) => vec![(k, prefix.clone())],
            None => decoders::detect(&src, prefix)
                .await?
                .into_iter()
                .map(|d| (d.format, d.root))
                .collect(),
        };
        let mut v = vec![];
        for (k, r) in targets {
            v.push(decoders::analyze(&src, k, &r).await?);
        }
        v
    } else {
        let ep = super::endpoint(s, endpoint.as_deref())?;
        let b = bucket
            .clone()
            .ok_or_else(|| anyhow::anyhow!("--bucket (or --dir)"))?;
        let targets: Vec<(Lf, String)> = match kind {
            Some(k) => vec![(k, prefix.clone())],
            None => {
                let v = super::wait(
                    s,
                    &sonde_core::services::layout::detect(s, &ep, &b, prefix),
                    false,
                )
                .await?;
                serde_json::from_value::<Vec<decoders::Detection>>(v)?
                    .into_iter()
                    .map(|d| (d.format, d.root))
                    .collect()
            }
        };
        let mut v = vec![];
        for (k, r) in targets {
            v.push(serde_json::from_value(
                super::wait(
                    s,
                    &sonde_core::services::layout::analyze(s, &ep, &b, k, &r),
                    false,
                )
                .await?,
            )?);
        }
        v
    };
    if layouts.is_empty() {
        eprintln!(
            "no known layout under {prefix:?}; --kind generic shows the write cadence of any prefix"
        );
        return Ok(4);
    }
    match format {
        Format::Json => println!("{}", output::json(&layouts)),
        _ => {
            for (i, l) in layouts.iter().enumerate() {
                if i > 0 {
                    println!("\n{}\n", "═".repeat(60));
                }
                print(l, at);
            }
        }
    }
    Ok(if layouts.iter().any(|l| l.has_errors()) {
        1
    } else if layouts
        .iter()
        .any(|l| l.issues.iter().any(|i| i.severity == Severity::Warning))
    {
        4
    } else {
        0
    })
}
