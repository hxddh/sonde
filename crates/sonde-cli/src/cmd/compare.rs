//! `sonde diff` and `sonde compare`: a session and its replay, two runs.

use sonde_proxy::program::{money_text, ms_text};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Args;
use reqlog::Record;
use sonde_core::Sonde;
use sonde_proxy::profile::{RequestPrices, profile};

use crate::output::{self, Format};

use super::record::*;

#[derive(Args)]
pub struct CompareArgs {
    /// The run before a change.
    before: PathBuf,
    /// The run after it.
    after: PathBuf,
    /// The format written: table or json.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    format: Format,
}

#[derive(Args)]
pub struct DiffArgs {
    /// The original session.
    original: PathBuf,
    /// Its replay (`sonde replay -o`).
    replay: PathBuf,
    /// The format written: table or json.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    format: Format,
}

pub(super) fn print_comparison(c: &sonde_proxy::replay::Comparison, color: bool) {
    let mut head = format!("{} compared: {} the same", c.compared, c.same);
    if c.differ() > 0 {
        head.push_str(&format!(
            " · {}",
            paint(&format!("{} differ", c.differ()), "33", color)
        ));
    }
    if c.environment > 0 {
        head.push_str(&format!(
            " · {} because the target already held it",
            c.environment
        ));
    }
    println!("{}", paint(&head, "1", color));
    for d in &c.differences {
        let mut line = format!(
            "{} {:<24} recorded {} → replay {}   ×{}   e.g. {}",
            paint("✗", "33", color),
            d.operation,
            d.original,
            d.replay,
            d.count,
            d.example
        );
        if !d.conditions.is_empty() {
            line.push_str(&paint(
                &format!("  ({})", d.conditions.join(", ")),
                "2",
                color,
            ));
        }
        println!("{line}");
    }
    for (why, n) in &c.unmatched {
        // without a reason (a replay of another time, compared by `sonde
        // diff`) the reason is not the words again
        let line = if why == "not replayed" {
            format!("  {n} not replayed")
        } else {
            format!("  {n} not replayed: {why}")
        };
        println!("{}", paint(&line, "2", color));
    }
}

/// The same workload on two stores: each kind's p50 and p95 on each, slowest change first.
pub(super) fn print_latency(
    a: &[Record],
    b: &[Record],
    names: (&str, &str),
    prices: &RequestPrices,
) {
    print_latency_of(
        &sonde_proxy::profile::latency_changes(&profile(a, prices), &profile(b, prices)),
        names,
    );
}

/// [`print_latency`] of the changes reckoned already.
fn print_latency_of(changes: &[sonde_proxy::profile::LatencyChange], names: (&str, &str)) {
    if changes.is_empty() {
        return;
    }
    let rows: Vec<Vec<String>> = changes
        .iter()
        .take(10)
        .map(|c| {
            let ratio = c.p95_ms.1.max(0.01) / c.p95_ms.0.max(0.01);
            vec![
                if c.missed {
                    format!("{} (not found)", c.operation)
                } else {
                    c.operation.clone()
                },
                format!("{} → {}", ms_text(c.p50_ms.0), ms_text(c.p50_ms.1)),
                format!("{} → {}", ms_text(c.p95_ms.0), ms_text(c.p95_ms.1)),
                format!("{ratio:.1}×"),
            ]
        })
        .collect();
    println!(
        "\nlatency, {} → {}\n{}",
        names.0,
        names.1,
        output::table(&["kind", "p50", "p95", "p95 ×"], &rows)
    );
}

pub async fn diff(s: &Arc<Sonde>, a: &DiffArgs) -> anyhow::Result<i32> {
    if let Some(why) = output::unsupported(a.format, "diff", &[Format::Table, Format::Json]) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    // read one after the other, each for what it amounts to: the replay
    // kept as its answers and its profile, then the session read against them
    let original_file = super::sessions::resolve(s, &a.original);
    let prices = sonde_proxy::session::read_header(&original_file)
        .map(|h| request_prices_for(s, &h.endpoint))
        .unwrap_or_default();
    let (replay_header, answers, is_replay, replay_profile) = {
        let (_, replay) = super::sessions::open_lean(s, &a.replay)?;
        (
            replay.header,
            sonde_proxy::replay::answers(&replay.records),
            sonde_proxy::replay::is_replay(&replay.records),
            profile(&replay.records, &prices),
        )
    };
    let (_, original) = super::sessions::open_lean(s, &a.original)?;
    // two ordinary runs pair no request: "0 compared: 0 the same" would read as agreement
    // (a replay that sent nothing is still one: its header says what it is)
    let named_replay = replay_header
        .command
        .as_deref()
        .is_some_and(|c| c.starts_with("replay of "));
    if !named_replay && !is_replay {
        eprintln!("sonde: {}", not_a_replay(a, &original.records));
        return Ok(64);
    }
    let c = sonde_proxy::replay::compare_answers(&original.records, &answers, &[]);
    let latency = sonde_proxy::profile::latency_changes(
        &profile(&original.records, &prices),
        &replay_profile,
    );
    match a.format {
        Format::Json => println!(
            "{}",
            output::json(&serde_json::json!({
                "comparison": c,
                "latency": latency,
            }))
        ),
        _ => {
            print_comparison(&c, std::io::stdout().is_terminal());
            print_latency_of(
                &latency,
                (&original.header.endpoint, &replay_header.endpoint),
            );
        }
    }
    Ok(if c.differ() > 0 { 2 } else { 0 })
}

/// What `sonde diff` says of two sessions that are not a session and its
/// replay, and what compares them instead.
fn not_a_replay(a: &DiffArgs, original: &[reqlog::Record]) -> String {
    let (first, second) = (a.original.display(), a.replay.display());
    if sonde_proxy::replay::is_replay(original) {
        return format!(
            "{second} is not a replay, {first} is: the session comes first (sonde diff {second} {first})"
        );
    }
    format!(
        "these are not a session and its replay: {second} was not made by `sonde replay`. Two runs of a program are compared with `sonde compare {first} {second}`"
    )
}

/// A figure before and after, with how much it moved: `28 (no change)`,
/// `28 → 31 (+11%)`. A move too small to show as a percent is not `+0%`.
fn change(a: f64, b: f64, text: impl Fn(f64) -> String) -> String {
    let (before, after) = (text(a), text(b));
    if a == b || before == after {
        return format!("{before} (no change)");
    }
    let pct = (b - a) * 100.0 / a;
    let pct = if a <= 0.0 {
        String::new()
    } else if pct.abs() < 0.5 {
        " (under 1%)".to_string()
    } else {
        format!(" ({pct:+.0}%)")
    };
    format!("{before} → {after}{pct}")
}

/// What `compare`'s table of kinds leaves out: `cut` that changed past its
/// rows, `same` that did not change. `show` says its own rest the same way.
fn more_kinds(cut: usize, same: usize) -> Option<String> {
    let kinds = |n: usize| if n == 1 { "kind" } else { "kinds" };
    match (cut, same) {
        (0, 0) => None,
        (0, s) => Some(format!(
            "  … and {s} more {} of request, unchanged",
            kinds(s)
        )),
        (c, 0) => Some(format!("  … and {c} more {} of request", kinds(c))),
        (c, s) => Some(format!(
            "  … and {} more {} of request, {s} of them unchanged",
            c + s,
            kinds(c + s)
        )),
    }
}

/// `sonde compare`: two runs of a program, before and after a change.
pub async fn compare(s: &Arc<Sonde>, a: &CompareArgs) -> anyhow::Result<i32> {
    if let Some(why) = output::unsupported(a.format, "compare", &[Format::Table, Format::Json]) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    // one run read, reckoned and let go before the other is read: two runs
    // cost what the larger does, not both at once
    let run = |f: &PathBuf| -> anyhow::Result<_> {
        let (_, session) = super::sessions::open_lean(s, f)?;
        let prices = request_prices_for(s, &session.header.endpoint);
        let run = sonde_proxy::profile::Run::analysed(&session.header, &session.records, &prices);
        Ok((session.header, run))
    };
    let (ha, ra) = run(&a.before)?;
    let (hb, rb) = run(&a.after)?;
    let c = sonde_proxy::profile::compare_runs(&ra, &rb).of_programs(
        sonde_proxy::program::key_of(&ha).as_deref(),
        sonde_proxy::program::key_of(&hb).as_deref(),
    );
    if matches!(a.format, Format::Json) {
        println!("{}", output::json(&c));
        return Ok(0);
    }
    let color = std::io::stdout().is_terminal();
    let n = |v: f64| format!("{v:.0}");
    if !c.same_program {
        println!(
            "{}",
            paint(
                "two different programs: their figures compare, their findings do not",
                "33",
                color
            )
        );
    }
    // nothing moved: said once, before the figures that show it
    let unchanged = c.requests.0 == c.requests.1
        && c.errors.0 == c.errors.1
        && ms_text(c.busy_ms.0) == ms_text(c.busy_ms.1)
        && c.cost.0 == c.cost.1
        && c.resolved.is_empty()
        && c.appeared.is_empty()
        && c.groups.iter().all(|g| g.requests.0 == g.requests.1);
    if unchanged {
        println!(
            "{}",
            paint(
                "no change: the same requests, waiting, errors, cost and findings",
                "1",
                color
            )
        );
    }
    println!("{}", paint("before → after", "1", color));
    println!(
        "  requests   {}",
        change(c.requests.0 as f64, c.requests.1 as f64, n)
    );
    println!("  waiting    {}", change(c.busy_ms.0, c.busy_ms.1, ms_text));
    println!("  took       {}", change(c.wall_ms.0, c.wall_ms.1, ms_text));
    println!(
        "  errors     {}",
        change(c.errors.0 as f64, c.errors.1 as f64, n)
    );
    println!(
        "  cost ≈     {}",
        change(c.cost.0, c.cost.1, |v| money_text(v, &c.currency))
    );
    // the kinds that changed, the largest change first; the rest counted
    const ROWS: usize = 10;
    let changed: Vec<&sonde_proxy::profile::GroupChange> = c
        .groups
        .iter()
        .filter(|g| g.requests.0 != g.requests.1 || (g.busy_ms.0 - g.busy_ms.1).abs() >= 1.0)
        .collect();
    let rows: Vec<Vec<String>> = changed
        .iter()
        .take(ROWS)
        .map(|g| {
            vec![
                if g.missed {
                    format!("{} (not found)", g.operation)
                } else {
                    g.operation.clone()
                },
                format!("{} → {}", g.requests.0, g.requests.1),
                format!("{} → {}", ms_text(g.busy_ms.0), ms_text(g.busy_ms.1)),
            ]
        })
        .collect();
    if !rows.is_empty() {
        println!("{}", output::table(&["kind", "requests", "waiting"], &rows));
        if let Some(more) = more_kinds(changed.len() - rows.len(), c.groups.len() - changed.len()) {
            println!("{more}");
        }
    }
    for f in &c.resolved {
        println!("{} {}", paint("✓", "32", color), f.text);
    }
    for f in &c.appeared {
        println!("{} {}", paint("!", "33", color), f.text);
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_says_how_many_kinds_its_table_leaves_out() {
        assert_eq!(more_kinds(0, 0), None);
        assert_eq!(
            more_kinds(0, 1).as_deref(),
            Some("  … and 1 more kind of request, unchanged")
        );
        assert_eq!(
            more_kinds(3, 0).as_deref(),
            Some("  … and 3 more kinds of request")
        );
        assert_eq!(
            more_kinds(2, 5).as_deref(),
            Some("  … and 7 more kinds of request, 5 of them unchanged")
        );
    }

    #[test]
    fn a_figure_that_did_not_move_says_no_change_not_plus_zero_percent() {
        let n = |v: f64| format!("{v:.0}");
        assert_eq!(change(28.0, 28.0, n), "28 (no change)");
        assert_eq!(change(28.0, 31.0, n), "28 → 31 (+11%)");
        assert_eq!(change(31.0, 28.0, n), "31 → 28 (-10%)");
        // moved, by less than shows: neither +0% nor -0%
        assert_eq!(change(1000.0, 1001.0, n), "1000 → 1001 (under 1%)");
        assert_eq!(change(1001.0, 1000.0, n), "1001 → 1000 (under 1%)");
        // the same once written: 435.2 ms and 435.4 ms
        assert_eq!(change(435.2, 435.4, ms_text), "435 ms (no change)");
        assert_eq!(change(0.0, 3.0, n), "0 → 3");
    }
}
