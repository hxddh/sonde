//! `sonde replay`: a session's requests sent again, to one store or several.

use sonde_proxy::program::ms_text;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Args;
use sonde_core::Sonde;
use sonde_proxy::profile::{Profile, profile};
use sonde_proxy::session::Session;

use crate::Cli;
use crate::output::{self, Format};

use super::compare::{print_comparison, print_latency};
use super::record::*;

fn bucket_pair(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .filter(|(a, b)| !a.is_empty() && !b.is_empty())
        .ok_or_else(|| format!("{s:?}: expected original=target"))
}

#[derive(Args)]
pub struct ReplayArgs {
    /// The session to replay.
    file: PathBuf,
    /// The endpoint to replay on (default: --url, or the only one configured).
    /// Several (`-e a -e b`) replay on each at once and compare them side by side.
    #[arg(long, short = 'e')]
    endpoint: Vec<String>,
    /// Use another bucket: original=target (repeatable).
    #[arg(long, value_parser = bucket_pair)]
    bucket: Vec<(String, String)>,
    /// The key prefix the replay writes under (default: replay-<time>/).
    #[arg(long, conflicts_with = "in_place")]
    prefix: Option<String>,
    /// Replay keys as they were recorded, bucket changes included.
    #[arg(long)]
    in_place: bool,
    /// Keep what the replay wrote (default: deleted afterwards).
    #[arg(long)]
    keep: bool,
    /// Do not ask for confirmation.
    #[arg(long, short = 'y')]
    yes: bool,
    /// The replay's session file (default: in the data directory's sessions/).
    #[arg(long, short = 'o')]
    out: Option<PathBuf>,
    /// The format written: table or json; with several -e, markdown and csv too.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    format: Format,
}

pub async fn replay(s: &Arc<Sonde>, _cli: &Cli, a: &ReplayArgs) -> anyhow::Result<i32> {
    use sonde_proxy::replay::{self as rp, ReplayOptions};
    if a.endpoint.len() > 1 {
        return replay_many(s, a).await;
    }
    if let Some(why) = output::unsupported(
        a.format,
        "replay on one endpoint",
        &[Format::Table, Format::Json],
    ) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    let (asked, original) = super::sessions::open(s, &a.file)?;
    let ep = super::endpoint(s, a.endpoint.first().map(String::as_str))?;
    let base = s.client(&ep)?;
    let prefix = if a.in_place {
        String::new()
    } else {
        match &a.prefix {
            Some(p) => rp::normalize_prefix(p).map_err(|e| anyhow::anyhow!(e))?,
            None => rp::default_prefix(),
        }
    };
    let opts = ReplayOptions {
        prefix: prefix.clone(),
        buckets: a.bucket.clone(),
    };
    let plan = rp::plan(&original.records, &opts);
    let mut buckets: Vec<String> = original
        .records
        .iter()
        .filter_map(|r| r.bucket.as_deref().map(|b| opts_bucket(&opts, b)))
        .collect();
    buckets.sort();
    buckets.dedup();
    let color = std::io::stderr().is_terminal();
    let place = if prefix.is_empty() {
        paint(
            "in place: keys as recorded, bucket changes included",
            "33",
            color,
        )
    } else {
        format!("under {prefix} in {}", buckets.join(", "))
    };
    eprintln!(
        "Replay {} from {} on {ep} ({}), {place}.",
        output::count(plan.requests, "request"),
        asked.display(),
        base.config().endpoint
    );
    eprintln!(
        "  {} and {}{}.",
        output::count(plan.writes, "write"),
        output::count(plan.deletes, "delete"),
        if prefix.is_empty() {
            ""
        } else {
            ", all under that prefix: nothing outside it is touched"
        }
    );
    for (why, n) in &plan.skipped {
        eprintln!("  {n} left out: {why}");
    }
    if !prefix.is_empty() && !a.keep {
        eprintln!("  Afterwards, everything under {prefix} is deleted (--keep keeps it).");
    }
    if !a.yes {
        if !std::io::stdin().is_terminal() {
            eprintln!("sonde: replay writes to {ep}: pass --yes to confirm");
            return Ok(77);
        }
        eprint!("Type yes to replay: ");
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if answer.trim() != "yes" {
            eprintln!("Nothing was sent.");
            return Ok(77);
        }
    }
    let path = match &a.out {
        Some(p) => p.clone(),
        None => {
            let dir = s.paths().sessions();
            std::fs::create_dir_all(&dir)?;
            sonde_proxy::session::fresh_path(
                &dir,
                &format!("{}-replay", chrono::Local::now().format("%Y%m%d-%H%M%S")),
            )
        }
    };
    let show = color;
    let run = rp::replay_to_file(&base, &original, &opts, &path, a.keep, |i, n| {
        if show {
            eprint!("\r\x1b[2K{i}/{n}");
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?;
    let _ = sonde_proxy::session::compress(&path);
    if show {
        eprint!("\r\x1b[2K");
    }
    if !prefix.is_empty() && !a.keep {
        eprintln!(
            "Removed {}{} under {prefix}.",
            output::count(run.removed, "object"),
            match run.removed_versions {
                0 => String::new(),
                n => format!(
                    " and {} (the bucket keeps them)",
                    output::count(n, "version or delete marker")
                        .replace("version or delete markers", "versions and delete markers")
                ),
            }
        );
        if run.remaining > 0 {
            // each thing once, by what it is
            let kinds: Vec<String> = [
                (run.remaining_objects, "object"),
                (run.remaining_versions, "older version"),
                (run.remaining_markers, "delete marker"),
            ]
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, what)| output::count(*n, what))
            .collect();
            eprintln!(
                "sonde: {} still under {prefix}: the store would not delete {}. `sonde du -b BUCKET -p {prefix}` shows what is there.",
                kinds.join(", "),
                if run.remaining == 1 { "it" } else { "them" }
            );
        }
        if run.unaborted_uploads > 0 {
            eprintln!(
                "sonde: {} under {prefix} would not abort: {} parts stay billed. `sonde mpu` lists what is unfinished.",
                output::count(run.unaborted_uploads, "multipart upload"),
                if run.unaborted_uploads == 1 {
                    "its"
                } else {
                    "their"
                }
            );
        }
    }
    let left_behind = run.remaining > 0 || run.unaborted_uploads > 0;
    let c = run.comparison;
    match a.format {
        Format::Json => println!(
            "{}",
            output::json(&serde_json::json!({
                "plan": plan,
                "session": path,
                "comparison": c,
                "removed": run.removed,
                "removedVersions": run.removed_versions,
                "remaining": run.remaining,
                "remainingObjects": run.remaining_objects,
                "remainingVersions": run.remaining_versions,
                "remainingMarkers": run.remaining_markers,
                "unabortedUploads": run.unaborted_uploads,
            }))
        ),
        _ => {
            print_comparison(&c, std::io::stdout().is_terminal());
            if let Ok(replayed) = Session::open(&path) {
                print_latency(
                    &original.records,
                    &replayed.records,
                    (&original.header.endpoint, &replayed.header.endpoint),
                    &request_prices_for(s, &original.header.endpoint),
                );
            }
            eprintln!(
                "{}",
                paint(&format!("session: {}", path.display()), "2", color)
            );
        }
    }
    // what it wrote and could not remove is a failure, whatever the answers
    Ok(if left_behind {
        70
    } else if c.differ() > 0 {
        2
    } else {
        0
    })
}

/// One session replayed on several stores at once: each answers under its
/// own prefix (cleaned up after), and the stores are compared side by side:
/// how many answers differ from the recording, and each kind's p95.
async fn replay_many(s: &Arc<Sonde>, a: &ReplayArgs) -> anyhow::Result<i32> {
    use sonde_proxy::replay::{self as rp, ReplayOptions};
    if let Some(why) = output::unsupported(
        a.format,
        "replay",
        &[Format::Table, Format::Json, Format::Markdown, Format::Csv],
    ) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    let (asked, original) = super::sessions::open(s, &a.file)?;
    if a.in_place {
        anyhow::bail!("--in-place replays on one endpoint at a time");
    }
    let prefix = match &a.prefix {
        Some(p) => rp::normalize_prefix(p).map_err(|e| anyhow::anyhow!(e))?,
        None => rp::default_prefix(),
    };
    let opts = ReplayOptions {
        prefix: prefix.clone(),
        buckets: a.bucket.clone(),
    };
    let plan = rp::plan(&original.records, &opts);
    let mut targets = Vec::new();
    for e in &a.endpoint {
        let id = super::endpoint(s, Some(e))?;
        targets.push((id.clone(), s.client(&id)?));
    }
    eprintln!(
        "Replay {} from {} on {} at once, under {prefix} on each ({}, {}; deleted afterwards unless --keep).",
        output::count(plan.requests, "request"),
        asked.display(),
        a.endpoint.join(", "),
        output::count(plan.writes, "write"),
        output::count(plan.deletes, "delete")
    );
    if !a.yes {
        if !std::io::stdin().is_terminal() {
            eprintln!(
                "sonde: replay writes to {}: pass --yes to confirm",
                a.endpoint.join(", ")
            );
            return Ok(77);
        }
        eprint!("Type yes to replay: ");
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if answer.trim() != "yes" {
            eprintln!("Nothing was sent.");
            return Ok(77);
        }
    }
    let dir = s.paths().sessions();
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let runs = futures::future::join_all(targets.iter().map(|(id, client)| {
        let path = sonde_proxy::session::fresh_path(&dir, &format!("{stamp}-replay-{id}"));
        let (original, opts) = (&original, &opts);
        async move {
            let run = rp::replay_to_file(client, original, opts, &path, a.keep, |_, _| {}).await;
            let _ = sonde_proxy::session::compress(&path);
            (id.clone(), path, run)
        }
    }))
    .await;
    let prices = request_prices_for(s, &original.header.endpoint);
    let before = profile(&original.records, &prices);
    let mut columns = vec![original.header.endpoint.clone()];
    let mut stores = Vec::new();
    let mut differ_total = 0;
    let mut failed = 0;
    for (id, path, run) in runs {
        let run = match run {
            Ok(r) => r,
            Err(e) => {
                eprintln!("sonde: {id}: {e}");
                failed += 1;
                continue;
            }
        };
        differ_total += run.comparison.differ();
        if run.remaining > 0 || run.unaborted_uploads > 0 {
            eprintln!(
                "sonde: {id}: {} and {} still under {prefix}: the store would not remove what the replay wrote",
                output::count(run.remaining, "object or version")
                    .replace("object or versions", "objects or versions"),
                output::count(run.unaborted_uploads, "unfinished upload"),
            );
            failed += 1;
        }
        let replayed = Session::open(&path).map_err(|e| anyhow::anyhow!(e))?;
        columns.push(id.clone());
        stores.push((
            id,
            path,
            run.comparison,
            profile(&replayed.records, &prices),
        ));
    }
    let kinds: Vec<(String, bool)> = before
        .groups
        .iter()
        .map(|g| (g.operation.clone(), g.missed))
        .collect();
    let p95 = |p: &Profile, op: &str, missed: bool| {
        p.groups
            .iter()
            .find(|g| g.operation == op && g.missed == missed)
            .map(|g| g.p95_ms)
    };
    if matches!(a.format, Format::Json) {
        let doc = serde_json::json!({
            "plan": plan,
            "stores": stores.iter().map(|(id, path, c, p)| serde_json::json!({
                "endpoint": id,
                "session": path,
                "comparison": c,
                "latency": sonde_proxy::profile::latency_changes(&before, p),
            })).collect::<Vec<_>>(),
        });
        println!("{}", output::json(&doc));
    } else {
        let mut rows: Vec<Vec<String>> = vec![{
            let mut r = vec!["answers that differ".to_string(), "—".to_string()];
            r.extend(
                stores
                    .iter()
                    .map(|(_, _, c, _)| format!("{} of {}", c.differ(), c.compared)),
            );
            r
        }];
        for (op, missed) in &kinds {
            let name = if *missed {
                format!("{op} (not found)")
            } else {
                op.clone()
            };
            let mut r = vec![
                format!("{name} p95"),
                p95(&before, op, *missed).map(ms_text).unwrap_or_default(),
            ];
            r.extend(
                stores
                    .iter()
                    .map(|(_, _, _, p)| p95(p, op, *missed).map(ms_text).unwrap_or_default()),
            );
            rows.push(r);
        }
        let mut headers = vec![""];
        headers.extend(columns.iter().map(String::as_str));
        output::print(a.format, &headers, &rows);
        for (id, path, _, _) in &stores {
            eprintln!("{id}: {}", path.display());
        }
    }
    Ok(replay_exit(failed, differ_total))
}

/// What a replay on several stores exits with: a store it could not be
/// replayed on is a failure (70), whatever the others answered; answers
/// that differ are 2.
fn replay_exit(failed: usize, differ: usize) -> i32 {
    if failed > 0 {
        70
    } else if differ > 0 {
        2
    } else {
        0
    }
}

fn opts_bucket(o: &sonde_proxy::replay::ReplayOptions, b: &str) -> String {
    o.buckets
        .iter()
        .find(|(from, _)| from == b)
        .map(|(_, to)| to.clone())
        .unwrap_or_else(|| b.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_that_failed_on_a_store_does_not_exit_as_if_all_agreed() {
        assert_eq!(replay_exit(0, 0), 0);
        assert_eq!(replay_exit(0, 3), 2);
        assert_eq!(replay_exit(1, 0), 70);
        assert_eq!(replay_exit(2, 3), 70);
    }
}
