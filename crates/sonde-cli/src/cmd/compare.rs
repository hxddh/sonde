//! `sonde compare`: two runs of a program, a session and its replay, or a
//! session replayed on another endpoint (`--replay`).

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
    /// The runs: before and after a change. One run: it and its program's
    /// run before it. None: the newest run and the one before it. A session
    /// and its replay, in either order: their answers compared. With
    /// --replay: the one session to replay.
    #[arg(value_name = "RUN", num_args = 0..=2)]
    pub(super) runs: Vec<PathBuf>,
    /// Replay RUN on this endpoint and compare the answers. Several
    /// (`--replay a --replay b`) replay on each at once, side by side.
    #[arg(long, value_name = "ENDPOINT")]
    pub(super) replay: Vec<String>,
    #[command(flatten)]
    pub(super) how: super::replay::ReplayOpts,
    /// The format written: table or json; replayed on several, markdown and csv too.
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    pub(super) format: Format,
}

/// `sonde diff` (17.0), kept for 18.0: `sonde compare ORIGINAL REPLAY`.
#[derive(Args)]
pub struct DiffArgs {
    /// The original session.
    original: PathBuf,
    /// Its replay.
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

/// `sonde diff`, said as its 18.0 form and done as that.
pub async fn diff_alias(s: &Arc<Sonde>, a: &DiffArgs) -> anyhow::Result<i32> {
    super::renamed(
        "sonde diff",
        &format!(
            "sonde compare {} {}",
            a.original.display(),
            a.replay.display()
        ),
    );
    diff(s, &a.original, &a.replay, a.format).await
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

/// A session and its replay: the answers that differ, and each kind's latency on the two stores.
async fn diff(
    s: &Arc<Sonde>,
    original: &PathBuf,
    replay: &PathBuf,
    format: Format,
) -> anyhow::Result<i32> {
    if let Some(why) = output::unsupported(format, "compare", &[Format::Table, Format::Json]) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    // read one after the other, each for what it amounts to: the replay
    // kept as its answers and its profile, then the session read against them
    let original_file = super::sessions::resolve(s, original);
    let prices = sonde_proxy::session::read_header(&original_file)
        .map(|h| request_prices_for(s, &h.endpoint))
        .unwrap_or_default();
    let (replay_header, answers, is_replay, replay_profile) = {
        let (_, replay) = super::sessions::open_lean(s, replay)?;
        (
            replay.header,
            sonde_proxy::replay::answers(&replay.records),
            sonde_proxy::replay::is_replay(&replay.records),
            profile(&replay.records, &prices),
        )
    };
    let (_, original) = super::sessions::open_lean(s, original)?;
    // a replay that sent nothing is still one: its header says what it is
    if !named_replay(&replay_header) && !is_replay {
        return Err(super::usage(format!(
            "{} is not a replay of {}",
            replay.display(),
            original_file.display()
        )));
    }
    let c = sonde_proxy::replay::compare_answers(&original.records, &answers, &[]);
    let latency = sonde_proxy::profile::latency_changes(
        &profile(&original.records, &prices),
        &replay_profile,
    );
    match format {
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

fn named_replay(h: &sonde_proxy::session::SessionHeader) -> bool {
    h.command
        .as_deref()
        .is_some_and(|c| c.starts_with("replay of "))
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

/// `sonde compare`: two runs of a program, before and after a change; a
/// session and its replay; or a session replayed on another endpoint now.
pub async fn compare(s: &Arc<Sonde>, a: &CompareArgs) -> anyhow::Result<i32> {
    if !a.replay.is_empty() {
        let [one] = a.runs.as_slice() else {
            return Err(super::usage(
                "--replay ENDPOINT replays one session: name it",
            ));
        };
        return super::replay::replay(s, one, &a.replay, &a.how, a.format).await;
    }
    if a.how != super::replay::ReplayOpts::default() {
        return Err(super::usage(
            "--bucket, --prefix, --in-place, --keep, --yes and -o are for --replay ENDPOINT",
        ));
    }
    // a session and its replay, in either order: their answers compared
    if let [x, y] = a.runs.as_slice() {
        let header =
            |f: &PathBuf| sonde_proxy::session::read_header(&super::sessions::resolve(s, f));
        match (header(x), header(y)) {
            (_, Some(h)) if named_replay(&h) => return diff(s, x, y, a.format).await,
            (Some(h), _) if named_replay(&h) => return diff(s, y, x, a.format).await,
            _ => {}
        }
    }
    if let Some(why) = output::unsupported(a.format, "compare", &[Format::Table, Format::Json]) {
        eprintln!("sonde: {why}");
        return Ok(64);
    }
    let (before, after) = runs_compared(s, &a.runs)?;
    // one run read, reckoned and let go before the other is read: two runs
    // cost what the larger does, not both at once (the later counted
    // against what the earlier's findings would save)
    let run = |f: &PathBuf, against: Option<&sonde_proxy::profile::Run>| -> anyhow::Result<_> {
        let (_, session) = super::sessions::open_lean(s, f)?;
        let prices = request_prices_for(s, &session.header.endpoint);
        let mut run =
            sonde_proxy::profile::Run::analysed(&session.header, &session.records, &prices);
        if let Some(a) = against {
            run.measure(&session.records, a);
        }
        Ok((session.header, run))
    };
    let (ha, ra) = run(&before, None)?;
    let (hb, rb) = run(&after, Some(&ra))?;
    let (ka, kb) = (
        sonde_proxy::program::key_of(&ha),
        sonde_proxy::program::key_of(&hb),
    );
    let mut c = sonde_proxy::profile::compare_runs(&ra, &rb);
    c.same_program = ka.is_some() && ka == kb;
    if c.same_program {
        // a finding fixed in an earlier run of the program is back, not new
        if let (Some(k), Ok(kept)) = (
            &ka,
            sonde_proxy::fixloop::read(&sonde_proxy::fixloop::folder_of(&after)),
        ) && let Some(program) = kept.get(k)
        {
            program.mark_back(&mut c.findings);
        }
    } else {
        c = c.of_programs(None, None);
    }
    if matches!(a.format, Format::Json) {
        println!("{}", output::json(&c));
        return Ok(0);
    }
    let color = std::io::stdout().is_terminal();
    let n = |v: f64| format!("{v:.0}");
    use super::fixloop::moved as change;
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
    println!(
        "{}",
        paint(
            &format!("{} → {}", file_name(&before), file_name(&after)),
            "1",
            color
        )
    );
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
    for line in super::fixloop::change_lines(&c.findings, color) {
        println!("{line}");
    }
    Ok(0)
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The two runs `sonde compare` compares: the two given; one given and its
/// program's run before it; none, the newest run and the one before it.
fn runs_compared(s: &Sonde, runs: &[PathBuf]) -> anyhow::Result<(PathBuf, PathBuf)> {
    match runs {
        [before, after] => Ok((
            super::sessions::resolve(s, before),
            super::sessions::resolve(s, after),
        )),
        [one] => before_of(&super::sessions::resolve(s, one)),
        _ => before_of(&newest_session(s)?),
    }
}

/// A run and its program's run before it.
fn before_of(after: &std::path::Path) -> anyhow::Result<(PathBuf, PathBuf)> {
    let h = sonde_proxy::session::read_header(after)
        .ok_or_else(|| super::usage(format!("{} is not a sonde session", after.display())))?;
    let key = sonde_proxy::program::key_of(&h).ok_or_else(|| {
        super::usage(format!(
            "{} is no run of a program (a replay or an import): name the two runs to compare",
            after.display()
        ))
    })?;
    let before = sonde_proxy::fixloop::previous_run(
        &sonde_proxy::fixloop::folder_of(after),
        &key,
        h.created,
        after,
    )
    .ok_or_else(|| {
        super::usage(format!(
            "{} is the first run of `{key}`: run it again after a change, then compare",
            file_name(after)
        ))
    })?;
    Ok((before, after.to_path_buf()))
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
}
