//! The fix loop in the terminal: each finding with its saving, its line of
//! code and the fix for the SDK that sent it; and a run against the
//! program's run before it (`sonde run`, `show`, `compare`).

use std::path::Path;

use reqlog::Record;
use sonde_analysis::episode::Observation;
use sonde_analysis::fixloop::{FindingChange, SinceLast, State};
use sonde_analysis::profile::RunChange;
use sonde_analysis::program::{money_text, ms_text};
use sonde_analysis::run::RunHeader;
use sonde_analysis::view::Analysis;
use sonde_core::Sonde;

use super::record::{paint, request_prices_for};
use crate::output;

/// A finding as the head of its diagnosis: what it is, what acting on the
/// diagnosis saves (`saving`), what else it is (`also`: its other findings,
/// each a line), where in the program, and the fix for the SDK that sent its
/// requests (`sdk`: the run's, when the finding's own requests do not say).
pub(super) fn finding_lines(
    o: &Observation,
    saving: Option<&str>,
    also: &[String],
    sdk: Option<&str>,
    color: bool,
) -> Vec<String> {
    let mut out = vec![format!("{} {}", mark(o, color), o.text)];
    if let Some(why) = &o.expected {
        out.push(paint(
            &format!("    expected: {why} (sonde.toml)"),
            "2",
            color,
        ));
    }
    if let Some(saving) = saving.filter(|s| !s.is_empty()) {
        out.push(paint(&format!("    saving: {saving}"), "32", color));
    }
    for line in also {
        out.push(paint(&format!("    also: {line}"), "2", color));
    }
    for c in &o.callers {
        out.push(paint(
            &format!(
                "    at {} ({})",
                c.place,
                output::count(c.requests, "request")
            ),
            "2",
            color,
        ));
    }
    let sdk = o.sdk.as_deref().or(sdk);
    if let Some(fix) = sonde_analysis::fixes::for_sdk(&o.fixes, sdk) {
        // the engine's setting first: the one edit to make
        if let Some(s) = &fix.setting {
            out.push(paint(&format!("    set: {}", s.line()), "36", color));
        }
        let whose = match (fix.sdk, sdk) {
            ("config", _) | (_, None) => "fix:".to_string(),
            (_, Some(sdk)) => format!("fix for {}:", sdk.split(' ').next().unwrap_or(sdk)),
        };
        out.push(paint(&format!("    {whose}"), "2", color));
        out.extend(fix.code.lines().map(|l| format!("      {l}")));
    }
    out
}

/// `!` for a warning, `·` for the rest (and for one the project keeps on
/// purpose).
fn mark(o: &Observation, color: bool) -> String {
    if o.level == "warn" && o.expected.is_none() {
        paint("!", "33", color)
    } else {
        paint("·", "2", color)
    }
}

/// A run's diagnoses: the first in full (its saving, its other findings,
/// its callers, its fix), the others a line each with where in the program
/// (each in full with `all`).
pub(super) fn diagnosis_lines(a: &Analysis, all: bool, color: bool) -> Vec<String> {
    let mut out = Vec::new();
    for (n, d) in a.diagnoses.iter().enumerate() {
        let Some(head) = d.findings.first().and_then(|&i| a.observations.get(i)) else {
            continue;
        };
        if n == 0 || all {
            // (one kept on purpose says why)
            let said = |text: String, o: &Observation| match &o.expected {
                Some(why) => format!("{text} (expected: {why})"),
                None => text,
            };
            let mut also: Vec<String> = d.findings[1..]
                .iter()
                .filter(|&&i| Some(i) != d.serial)
                .filter_map(|&i| a.observations.get(i))
                .map(|o| said(o.text.clone(), o))
                .collect();
            if let (Some(rest), Some(o)) = (&d.rest, d.serial.and_then(|i| a.observations.get(i))) {
                also.push(said(rest.en(), o));
            }
            out.extend(finding_lines(
                head,
                Some(&d.saving_text),
                &also,
                a.sdk.as_deref(),
                color,
            ));
            continue;
        }
        let mut line = format!("{} {}", mark(head, color), head.text);
        if !d.saving_text.is_empty() {
            line.push_str(&paint(&format!(" · {}", d.saving_text), "32", color));
        }
        if let Some(c) = head.callers.first() {
            line.push_str(&paint(&format!(" (at {})", c.place), "2", color));
        }
        if let Some(why) = &head.expected {
            line.push_str(&paint(
                &format!(" · expected: {why} (sonde.toml)"),
                "2",
                color,
            ));
        }
        out.push(line);
    }
    out
}

/// A finding between two runs, on one line: `✓ fixed  … · saved 20
/// requests (predicted 20)`; with the command changed or the workload grown
/// between the runs (`uncounted`, [`RunChange::uncounted`]), fixed, less
/// and more say so in place of a saving.
pub(super) fn change_line(f: &FindingChange, uncounted: Option<&str>, color: bool) -> String {
    let (mark, tone) = match f.state {
        State::Fixed => ("✓", "32"),
        State::Less => ("↓", "36"),
        State::More => ("↑", "33"),
        State::New | State::Back => ("!", "33"),
        State::Open => ("·", "2"),
    };
    let mut line = format!(
        "  {} {:<5}  {}",
        paint(mark, tone, color),
        paint(f.state.label(), tone, color),
        f.text
    );
    // how much of it, when it moved: `92 → 216`
    if let Some([was, now]) = f.counts
        && was != now
        && was > 0
        && now > 0
    {
        line.push_str(&paint(
            &format!(
                " · {} → {}",
                output::grouped(was as u64),
                output::grouped(now as u64)
            ),
            "2",
            color,
        ));
    }
    if let Some(saved) = &f.saved_text {
        line.push_str(&paint(&format!(" · {saved}"), "32", color));
    }
    if let Some(why) = uncounted
        && matches!(f.state, State::Fixed | State::Less | State::More)
    {
        line.push_str(&paint(&format!(" · {why}"), "33", color));
    }
    line
}

/// The findings of a change worth a line (fixed, less, more, new, back),
/// then those still open as they were, counted: `as above` when the run's
/// findings are `listed` above (`run`, `show`), not by `compare`, which
/// lists none.
pub(super) fn change_lines(
    findings: &[FindingChange],
    uncounted: Option<&str>,
    listed: bool,
    color: bool,
) -> Vec<String> {
    let mut out: Vec<String> = findings
        .iter()
        .filter(|f| f.state != State::Open)
        .map(|f| change_line(f, uncounted, color))
        .collect();
    let open: Vec<&FindingChange> = findings.iter().filter(|f| f.state == State::Open).collect();
    // (each is listed above, with its line and fix: counted here)
    if !open.is_empty() && !out.is_empty() {
        let above = if listed { ", as above" } else { "" };
        out.push(paint(
            &format!("  {} still open{above}", open.len()),
            "2",
            color,
        ));
    }
    out
}

/// A figure before and after, with how much it moved: `28 (no change)`,
/// `28 → 31 (+11%)`. A move too small to show as a percent is not `+0%`.
pub(super) fn moved(a: f64, b: f64, text: impl Fn(f64) -> String) -> String {
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

/// Why what moved between two runs is not counted as a fix (the command
/// changed, the workload grew): said before the figures.
pub(super) fn uncounted_line(why: &str) -> String {
    format!("{why}: what moved is not counted as a fix")
}

/// The figures of a change on one line: requests, waiting, cost.
fn figures_line(c: &RunChange) -> String {
    let n = |v: f64| crate::output::grouped(v.max(0.0).round() as u64);
    format!(
        "requests {} · waiting {} · cost {}",
        moved(c.requests.0 as f64, c.requests.1 as f64, n),
        if c.waiting_known {
            moved(c.waiting_ms.0, c.waiting_ms.1, ms_text)
        } else {
            "— (not known)".into()
        },
        cost_moved(c.cost, &c.currency),
    )
}

/// A cost before and after as [`moved`] says it; under a cent, its share
/// alone (`-9%`).
pub(super) fn cost_moved((a, b): (f64, f64), currency: &str) -> String {
    if a.max(b) < sonde_analysis::program::CENT {
        return sonde_analysis::program::share_moved(a, b);
    }
    moved(a, b, |v| money_text(v, currency))
}

/// A run against the program's run before it, in a few lines: what it
/// asks of the store, and its findings fixed, less, new and back. Nothing
/// for a program's first run.
pub(super) fn since_lines(since: &SinceLast, color: bool) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(why) = &since.unsaved {
        out.push(format!("note: {why}"));
    }
    if let Some(exit) = since.failed {
        out.push(paint(
            &format!("the run failed (exit {exit}); not compared"),
            "33",
            color,
        ));
        return out;
    }
    if since.empty {
        out.push(paint(
            &format!(
                "{}: the run recorded no request; not compared",
                sonde_analysis::program::NOTHING_REACHED
            ),
            "33",
            color,
        ));
        return out;
    }
    let (Some(c), Some(previous)) = (&since.change, &since.previous) else {
        return out;
    };
    let name = Path::new(previous)
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    out.push(paint(&format!("since the run before ({name})"), "1", color));
    let uncounted = c.uncounted(false);
    if let Some(why) = c.uncounted(true) {
        out.push(paint(&format!("  {}", uncounted_line(&why)), "33", color));
    }
    out.push(format!("  {}", figures_line(c)));
    out.extend(change_lines(
        &since.findings,
        uncounted.as_deref(),
        true,
        color,
    ));
    out
}

/// `path` (a run, its `header` and `records`, analysed as `a`) against its
/// program's run before it; the program's findings kept up to it.
pub(super) fn since_last(
    s: &Sonde,
    path: &Path,
    header: &RunHeader,
    records: &[Record],
    a: &Analysis,
) -> Option<SinceLast> {
    a.since_last(path, header, records, &|endpoint| {
        request_prices_for(s, endpoint)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonde_analysis::fixloop::Saving;

    #[test]
    fn a_figure_that_did_not_move_says_no_change_not_plus_zero_percent() {
        let n = |v: f64| format!("{v:.0}");
        assert_eq!(moved(28.0, 28.0, n), "28 (no change)");
        assert_eq!(moved(28.0, 31.0, n), "28 → 31 (+11%)");
        assert_eq!(moved(31.0, 28.0, n), "31 → 28 (-10%)");
        // moved, by less than shows: neither +0% nor -0%
        assert_eq!(moved(1000.0, 1001.0, n), "1000 → 1001 (under 1%)");
        assert_eq!(moved(1001.0, 1000.0, n), "1001 → 1000 (under 1%)");
        // the same once written: 435.2 ms and 435.4 ms
        assert_eq!(moved(435.2, 435.4, ms_text), "435 ms (no change)");
        assert_eq!(moved(0.0, 3.0, n), "0 → 3");
    }

    #[test]
    fn a_finding_an_engines_setting_acts_on_says_the_setting_before_the_code() {
        use sonde_analysis::episode::{episodes, observations};
        let ua = reqlog::Header::new("user-agent", "rclone/v1.68.2");
        let rs: Vec<Record> = (0..12u64)
            .map(|i| Record {
                id: i + 1,
                time: chrono::DateTime::from_timestamp_millis(1_700_000_000_000 + i as i64 * 50)
                    .unwrap(),
                method: if i % 2 == 0 { "HEAD" } else { "GET" }.into(),
                kind: if i % 2 == 0 {
                    "HeadObject"
                } else {
                    "GetObject"
                }
                .into(),
                bucket: Some("lake".into()),
                key: Some(format!("in/{}.csv", i / 2).into()),
                status: Some(200),
                latency_ms: 5.0,
                lists: reqlog::Lists {
                    request_headers: vec![ua.clone()],
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            })
            .collect();
        let obs = observations(&rs, &episodes(&rs));
        let o = obs.iter().find(|o| o.kind == "obs.headget").unwrap();
        let lines = finding_lines(o, None, &[], None, false);
        let set = lines
            .iter()
            .position(|l| l == "    set: --s3-no-head-object = true (rclone)")
            .unwrap_or_else(|| panic!("{lines:#?}"));
        let fix = lines.iter().position(|l| l.trim() == "fix:").unwrap();
        assert!(set < fix, "{lines:#?}");
    }

    fn change(state: State, text: &str, saved: Option<&str>) -> FindingChange {
        FindingChange {
            identity: text.into(),
            state,
            level: "info",
            text: text.into(),
            phrases: Vec::new(),
            was: None,
            counts: None,
            predicted: Saving::default(),
            measured: None,
            saved: None,
            saved_text: saved.map(String::from),
        }
    }

    #[test]
    fn a_change_says_what_moved_and_counts_what_is_still_open() {
        let lines = change_lines(
            &[
                change(
                    State::Fixed,
                    "HEAD before GET",
                    Some("saved 20 requests (predicted 20)"),
                ),
                change(State::Less, "41 one after another", None),
                change(State::Back, "listed 5 times", None),
                change(State::Open, "small uploads", None),
                change(State::Open, "hot key", None),
            ],
            None,
            true,
            false,
        );
        assert_eq!(
            lines,
            [
                "  ✓ fixed  HEAD before GET · saved 20 requests (predicted 20)",
                "  ↓ less   41 one after another",
                "  ! back   listed 5 times",
                // each listed above, with its line and fix
                "  2 still open, as above",
            ]
        );
        // nothing moved: no "still open" alone
        assert!(change_lines(&[change(State::Open, "x", None)], None, true, false).is_empty());
        // compare lists no findings: nothing is above
        assert_eq!(
            change_lines(
                &[
                    change(State::Less, "41 one after another", None),
                    change(State::Open, "hot key", None),
                ],
                None,
                false,
                false,
            ),
            ["  ↓ less   41 one after another", "  1 still open"]
        );
    }

    #[test]
    fn a_finding_that_grew_says_more_with_its_counts() {
        let mut grew = change(State::More, "b/logs/ was listed 216 times", None);
        grew.counts = Some([92, 216]);
        assert_eq!(
            change_line(&grew, None, false),
            "  ↑ more   b/logs/ was listed 216 times · 92 → 216"
        );
    }
}
