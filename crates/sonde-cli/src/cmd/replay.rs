//! `sonde compare --replay`: a run's requests sent again, to one store or several.

use sonde_analysis::program::ms_text;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Args;
use sonde_analysis::profile::{Profile, profile};
use sonde_analysis::run::RunFile;
use sonde_core::Sonde;

use crate::output::{self, Format};

use super::compare::{print_comparison, print_latency};
use super::record::*;

fn bucket_pair(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .filter(|(a, b)| !a.is_empty() && !b.is_empty())
        .ok_or_else(|| format!("{s:?}: expected original=target"))
}

/// How a run is replayed: where its writes go. It writes under a prefix
/// of its own and removes what it wrote there, always.
#[derive(Args, Clone, Default, PartialEq)]
pub struct ReplayOpts {
    /// Use another bucket: original=target (repeatable).
    #[arg(long, value_parser = bucket_pair)]
    pub(super) bucket: Vec<(String, String)>,
    /// The key prefix the replay writes under (default: replay-<time>/).
    #[arg(long)]
    pub(super) prefix: Option<String>,
    /// Replay what the dry run says (what it writes, under its prefix, it removes).
    #[arg(long)]
    pub(super) yes: bool,
    /// The replay's file (default: in the data directory's runs/).
    #[arg(long, short = 'o')]
    pub(super) out: Option<PathBuf>,
}

/// A run to replay, where, and how.
struct ReplayArgs {
    file: PathBuf,
    endpoint: Vec<String>,
    how: ReplayOpts,
    format: Format,
}

/// `file` replayed on `endpoints` (one, or several side by side), and the
/// answers compared with the recording's.
pub async fn replay(
    s: &Arc<Sonde>,
    file: &std::path::Path,
    endpoints: &[String],
    how: &ReplayOpts,
    format: Format,
) -> anyhow::Result<i32> {
    let a = ReplayArgs {
        file: file.to_path_buf(),
        endpoint: endpoints.to_vec(),
        how: how.clone(),
        format,
    };
    replay_on(s, &a).await
}

async fn replay_on(s: &Arc<Sonde>, a: &ReplayArgs) -> anyhow::Result<i32> {
    use sonde_analysis::replay::{self as rp, ReplayOptions};
    if a.endpoint.len() > 1 {
        return replay_many(s, a).await;
    }
    let (asked, original) = super::runs::open(s, &a.file)?;
    let ep = super::endpoint(s, a.endpoint.first().map(String::as_str))?;
    let base = s.client(&ep)?;
    let prefix = match &a.how.prefix {
        Some(p) => rp::normalize_prefix(p).map_err(|e| anyhow::anyhow!(e))?,
        None => rp::default_prefix(),
    };
    let opts = ReplayOptions {
        prefix: prefix.clone(),
        buckets: a.how.bucket.clone(),
    };
    let plan = rp::plan(&original.records, &opts);
    let relies = probes::relies::relied_on(&original.records);
    let mut buckets: Vec<String> = original
        .records
        .iter()
        .filter_map(|r| r.bucket.as_deref().map(|b| opts_bucket(&opts, b)))
        .collect();
    buckets.sort();
    buckets.dedup();
    let color = std::io::stderr().is_terminal();
    eprintln!(
        "Replay {} from {} on {ep} ({}), under {prefix} in {}.",
        output::count(plan.requests, "request"),
        asked.display(),
        base.config().endpoint,
        buckets.join(", ")
    );
    eprintln!(
        "  {} and {}, all under that prefix: nothing outside it is touched.",
        output::count(plan.writes, "write"),
        output::count(plan.deletes, "delete"),
    );
    for line in before_lines(&plan, &relies, buckets.first()) {
        eprintln!("  {line}");
    }
    for (why, n) in &plan.skipped {
        eprintln!("  {n} left out: {why}");
    }
    eprintln!("  Afterwards, everything under {prefix} is deleted.");
    if !a.how.yes {
        eprintln!("\ndry run: to replay, run again with --yes");
        return Ok(4);
    }
    let path = match &a.how.out {
        Some(p) => p.clone(),
        None => {
            let dir = s.paths().runs();
            std::fs::create_dir_all(&dir)?;
            sonde_analysis::run::fresh_path(
                &dir,
                &format!("{}-replay", chrono::Local::now().format("%Y%m%d-%H%M%S")),
            )
        }
    };
    let show = color;
    let run = rp::replay_to_file(&base, &original, &opts, &path, |i, n| {
        if show {
            eprint!("\r\x1b[2K{i}/{n}");
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?;
    let _ = sonde_analysis::run::compress(&path);
    if show {
        eprint!("\r\x1b[2K");
    }
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
            "sonde: {} still under {prefix}: the store would not delete {}. `sonde bucket size -b BUCKET -p {prefix}` shows what is there.",
            kinds.join(", "),
            if run.remaining == 1 { "it" } else { "them" }
        );
    }
    if run.unaborted_uploads > 0 {
        eprintln!(
            "sonde: {} under {prefix} would not abort: {} parts stay billed. `sonde bucket uploads` lists what is unfinished.",
            output::count(run.unaborted_uploads, "multipart upload"),
            if run.unaborted_uploads == 1 {
                "its"
            } else {
                "their"
            }
        );
    }
    let left_behind = run.remaining > 0 || run.unaborted_uploads > 0;
    let c = run.comparison;
    // what it relies on, asked of the store it was replayed on
    let relied = answer_relies(s, &ep, buckets.first(), &relies, &path).await;
    let replayed = RunFile::open(&path).ok();
    let costs = vec![
        store_cost(s, &original.header.endpoint, &original.records),
        store_cost(s, &ep, replayed.as_ref().map_or(&[][..], |r| &r.records)),
    ];
    match a.format {
        Format::Json => output::print_json(&serde_json::json!({
            "plan": plan,
            "run": path,
            "comparison": c,
            "relies": relied,
            "cost": costs,
            "removed": run.removed,
            "removedVersions": run.removed_versions,
            "remaining": run.remaining,
            "remainingObjects": run.remaining_objects,
            "remainingVersions": run.remaining_versions,
            "remainingMarkers": run.remaining_markers,
            "unabortedUploads": run.unaborted_uploads,
        })),
        _ => {
            let out = std::io::stdout().is_terminal();
            print_comparison(&c, out);
            if let Some(replayed) = &replayed {
                print_latency(
                    &original.records,
                    &replayed.records,
                    (&original.header.endpoint, &replayed.header.endpoint),
                    &request_prices_for(s, &original.header.endpoint),
                );
            }
            println!("\ncost  {}", cost_line(&costs));
            for line in relies_lines(&program_of(&original.header), &ep, &relied, out) {
                println!("{line}");
            }
            eprintln!("{}", paint(&format!("run: {}", path.display()), "2", color));
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

/// One run replayed on several stores at once: each answers under its
/// own prefix (cleaned up after), and the stores are compared side by side:
/// how many answers differ from the recording, and each kind's p95.
async fn replay_many(s: &Arc<Sonde>, a: &ReplayArgs) -> anyhow::Result<i32> {
    use sonde_analysis::replay::{self as rp, ReplayOptions};
    let (asked, original) = super::runs::open(s, &a.file)?;
    let prefix = match &a.how.prefix {
        Some(p) => rp::normalize_prefix(p).map_err(|e| anyhow::anyhow!(e))?,
        None => rp::default_prefix(),
    };
    let opts = ReplayOptions {
        prefix: prefix.clone(),
        buckets: a.how.bucket.clone(),
    };
    let plan = rp::plan(&original.records, &opts);
    let relies = probes::relies::relied_on(&original.records);
    let first_bucket = rp::buckets_of(&original.records, &opts).into_iter().next();
    let mut targets = Vec::new();
    for e in &a.endpoint {
        let id = super::endpoint(s, Some(e))?;
        targets.push((id.clone(), s.client(&id)?));
    }
    eprintln!(
        "Replay {} from {} on {} at once, under {prefix} on each ({}, {}; deleted afterwards).",
        output::count(plan.requests, "request"),
        asked.display(),
        a.endpoint.join(", "),
        output::count(plan.writes, "write"),
        output::count(plan.deletes, "delete")
    );
    for line in before_lines(&plan, &relies, first_bucket.as_ref()) {
        eprintln!("  {line}");
    }
    if !a.how.yes {
        eprintln!("\ndry run: to replay, run again with --yes");
        return Ok(4);
    }
    let dir = s.paths().runs();
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let runs = futures::future::join_all(targets.iter().map(|(id, client)| {
        let path = sonde_analysis::run::fresh_path(&dir, &format!("{stamp}-replay-{id}"));
        let (original, opts) = (&original, &opts);
        async move {
            let run = rp::replay_to_file(client, original, opts, &path, |_, _| {}).await;
            let _ = sonde_analysis::run::compress(&path);
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
    let mut costs = vec![store_cost(s, &original.header.endpoint, &original.records)];
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
        let replayed = RunFile::open(&path).map_err(|e| anyhow::anyhow!(e))?;
        let relied = answer_relies(s, &id, first_bucket.as_ref(), &relies, &path).await;
        columns.push(id.clone());
        costs.push(store_cost(s, &id, &replayed.records));
        stores.push((
            id,
            path,
            run.comparison,
            profile(&replayed.records, &prices),
            relied,
        ));
    }
    let kinds: Vec<(String, bool)> = before
        .groups
        .iter()
        .map(|g| (g.kind.clone(), g.missed))
        .collect();
    let p95 = |p: &Profile, op: &str, missed: bool| {
        p.groups
            .iter()
            .find(|g| g.kind == op && g.missed == missed)
            .map(|g| g.p95_ms)
    };
    if matches!(a.format, Format::Json) {
        let doc = serde_json::json!({
            "plan": plan,
            "cost": costs,
            "stores": stores.iter().map(|(id, path, c, p, relied)| serde_json::json!({
                "endpoint": id,
                "run": path,
                "comparison": c,
                "latency": sonde_analysis::profile::latency_changes(&before, p),
                "relies": relied,
            })).collect::<Vec<_>>(),
        });
        output::print_json(&doc);
    } else {
        let mut rows: Vec<Vec<String>> = vec![{
            let mut r = vec!["answers that differ".to_string(), "—".to_string()];
            r.extend(
                stores
                    .iter()
                    .map(|(_, _, c, _, _)| format!("{} of {}", c.differ(), c.compared)),
            );
            r
        }];
        // each store's request cost at its own vendor's prices
        rows.push(
            costs
                .iter()
                .fold(vec!["request cost".to_string()], |mut r, c| {
                    r.push(money(c));
                    r
                }),
        );
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
                    .map(|(_, _, _, p, _)| p95(p, op, *missed).map(ms_text).unwrap_or_default()),
            );
            rows.push(r);
        }
        let mut headers = vec![""];
        headers.extend(columns.iter().map(String::as_str));
        output::print(&headers, &rows);
        let color = std::io::stdout().is_terminal();
        let program = program_of(&original.header);
        for (id, _, _, _, relied) in &stores {
            for line in relies_lines(&program, id, relied, color) {
                println!("{line}");
            }
        }
        for (id, path, _, _, _) in &stores {
            eprintln!("{id}: {}", path.display());
        }
    }
    Ok(replay_exit(failed, differ_total))
}

/// What a dry run adds to the plan: the stand-ins written first, and the
/// probes of what the program relies on.
fn before_lines(
    plan: &sonde_analysis::replay::Plan,
    relies: &[probes::relies::Reliance],
    bucket: Option<&String>,
) -> Vec<String> {
    let mut out = Vec::new();
    if plan.seeds > 0 {
        out.push(format!(
            "First, {} ({}) for what the run read and had not written.",
            output::count(plan.seeds, "stand-in"),
            output::bytes(plan.seed_bytes)
        ));
    }
    if let (false, Some(b)) = (relies.is_empty(), bucket) {
        out.push(format!(
            "Then {} of what it relies on, under {b}/sonde-probe/ (removed after).",
            output::count(relies.len(), "probe")
        ));
    }
    out
}

/// The probes of what the program relies on, run on `endpoint` in
/// `bucket`; kept with the replay's run for the app. A failure to run them
/// is said, and leaves the list empty.
async fn answer_relies(
    s: &Arc<Sonde>,
    endpoint: &str,
    bucket: Option<&String>,
    relies: &[probes::relies::Reliance],
    run: &std::path::Path,
) -> Vec<probes::relies::Relied> {
    let Some(bucket) = bucket else {
        return Vec::new();
    };
    match probes::relies::answer(s, endpoint, bucket, relies).await {
        Ok(r) => {
            if !r.is_empty() {
                sonde_analysis::index::set(
                    run,
                    sonde_analysis::index::Change::Relies(
                        serde_json::to_value(&r).unwrap_or_default(),
                    ),
                );
            }
            r
        }
        Err(e) => {
            eprintln!(
                "sonde: {endpoint}: the probes of what the program relies on did not run: {e}"
            );
            Vec::new()
        }
    }
}

/// The name a program goes by in "what it relies on".
pub(super) fn program_of(h: &sonde_analysis::run::RunHeader) -> String {
    sonde_analysis::program::key_of(h).unwrap_or_else(|| "the program".into())
}

/// "What etl relies on, and how rfs answers": a line a probe.
pub(super) fn relies_lines(
    program: &str,
    endpoint: &str,
    relied: &[probes::relies::Relied],
    color: bool,
) -> Vec<String> {
    use probes::Verdict;
    if relied.is_empty() {
        return Vec::new();
    }
    let mut out = vec![
        String::new(),
        paint(
            &format!("What {program} relies on, and how {endpoint} answers"),
            "1",
            color,
        ),
    ];
    let wide = relied.iter().map(|r| r.on.probe.len()).max().unwrap_or(0);
    let label = relied
        .iter()
        .map(|r| r.verdict.label().len())
        .max()
        .unwrap_or(0);
    for r in relied {
        let tone = match r.verdict {
            Verdict::Pass => "32",
            Verdict::Fail => "31",
            _ => "33",
        };
        let mut line = format!(
            "  {} {:<label$}  {:<wide$}  {} ×{}",
            paint(r.verdict.symbol(), tone, color),
            r.verdict.label(),
            r.on.probe,
            r.on.kind,
            r.on.requests,
        );
        if r.verdict != Verdict::Pass && !r.summary.is_empty() {
            line.push_str(&paint(&format!("  {}", r.summary), "2", color));
        }
        out.push(line);
    }
    out
}

/// A store's request cost for `records`, at its vendor's prices.
fn store_cost(s: &Sonde, endpoint: &str, records: &[reqlog::Record]) -> serde_json::Value {
    let prices = request_prices_for(s, endpoint);
    let c = profile(records, &prices).cost;
    serde_json::json!({ "endpoint": endpoint, "amount": c.amount, "currency": c.currency })
}

fn money(c: &serde_json::Value) -> String {
    sonde_analysis::program::money_text(
        c["amount"].as_f64().unwrap_or(0.0),
        c["currency"].as_str().unwrap_or(""),
    )
}

/// Each store's request cost, the recording's first.
fn cost_line(costs: &[serde_json::Value]) -> String {
    costs
        .iter()
        .map(|c| format!("{} on {}", money(c), c["endpoint"].as_str().unwrap_or("")))
        .collect::<Vec<_>>()
        .join(" → ")
        + " (requests, at each store's prices)"
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

fn opts_bucket(o: &sonde_analysis::replay::ReplayOptions, b: &str) -> String {
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
