//! The fix loop in the terminal: each finding with its saving, its line of
//! code and the fix for the SDK that sent it; and a run against the
//! program's run before it (`sonde run`, `show`, `compare`).

use std::path::Path;

use reqlog::Record;
use sonde_core::Sonde;
use sonde_proxy::episode::Observation;
use sonde_proxy::fixloop::{FindingChange, SinceLast, State};
use sonde_proxy::profile::RunChange;
use sonde_proxy::program::{money_text, ms_text};
use sonde_proxy::session::SessionHeader;
use sonde_proxy::view::Analysis;

use super::record::{paint, request_prices_for};
use crate::output;

/// A finding: what it is, what acting on it saves, where in the program,
/// and the fix for the SDK that sent its requests (`sdk`: the session's,
/// when the finding's own requests do not say).
pub(super) fn finding_lines(o: &Observation, sdk: Option<&str>, color: bool) -> Vec<String> {
    let mark = if o.level == "warn" {
        paint("!", "33", color)
    } else {
        paint("·", "2", color)
    };
    let mut out = vec![format!("{mark} {}", o.text)];
    if let Some(saving) = &o.saving_text {
        out.push(paint(&format!("    saving: {saving}"), "32", color));
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
    if let Some(fix) = sonde_proxy::fixes::for_sdk(&o.fixes, sdk) {
        let whose = match (fix.sdk, sdk) {
            ("config", _) | (_, None) => "fix:".to_string(),
            (_, Some(sdk)) => format!("fix for {}:", sdk.split(' ').next().unwrap_or(sdk)),
        };
        out.push(paint(&format!("    {whose}"), "2", color));
        out.extend(fix.code.lines().map(|l| format!("      {l}")));
    }
    out
}

/// A finding between two runs, on one line: `✓ fixed  … · saved 20
/// requests (predicted 20)`.
pub(super) fn change_line(f: &FindingChange, color: bool) -> String {
    let (mark, tone) = match f.state {
        State::Fixed => ("✓", "32"),
        State::Less => ("↓", "36"),
        State::New | State::Back => ("!", "33"),
        State::Open => ("·", "2"),
    };
    let mut line = format!(
        "  {} {:<5}  {}",
        paint(mark, tone, color),
        paint(f.state.label(), tone, color),
        f.text
    );
    if let Some(saved) = &f.saved_text {
        line.push_str(&paint(&format!(" · {saved}"), "32", color));
    }
    line
}

/// The findings of a change worth a line (fixed, less, new, back), and how
/// many are still open, said once.
pub(super) fn change_lines(findings: &[FindingChange], color: bool) -> Vec<String> {
    let mut out: Vec<String> = findings
        .iter()
        .filter(|f| f.state != State::Open)
        .map(|f| change_line(f, color))
        .collect();
    let open = findings.iter().filter(|f| f.state == State::Open).count();
    if open > 0 && !out.is_empty() {
        out.push(paint(&format!("  {open} still open"), "2", color));
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

/// The figures of a change on one line: requests, waiting, cost.
fn figures_line(c: &RunChange) -> String {
    let n = |v: f64| format!("{v:.0}");
    format!(
        "requests {} · waiting {} · cost ≈ {}",
        moved(c.requests.0 as f64, c.requests.1 as f64, n),
        moved(c.busy_ms.0, c.busy_ms.1, ms_text),
        moved(c.cost.0, c.cost.1, |v| money_text(v, &c.currency)),
    )
}

/// A run against the program's run before it, in a few lines: what it
/// asks of the store, and its findings fixed, less, new and back. Nothing
/// for a program's first run.
pub(super) fn since_lines(since: &SinceLast, color: bool) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(why) = &since.unsaved {
        out.push(format!("note: {why}"));
    }
    let (Some(c), Some(previous)) = (&since.change, &since.previous) else {
        return out;
    };
    let name = Path::new(previous)
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    out.push(paint(&format!("since the run before ({name})"), "1", color));
    out.push(format!("  {}", figures_line(c)));
    out.extend(change_lines(&since.findings, color));
    out
}

/// `path` (a run, its `header` and `records`, analysed as `a`) against its
/// program's run before it; the program's findings kept up to it.
pub(super) fn since_last(
    s: &Sonde,
    path: &Path,
    header: &SessionHeader,
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
    use sonde_proxy::fixloop::Saving;

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

    fn change(state: State, text: &str, saved: Option<&str>) -> FindingChange {
        FindingChange {
            identity: text.into(),
            state,
            level: "info",
            text: text.into(),
            phrases: Vec::new(),
            was: None,
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
            false,
        );
        assert_eq!(
            lines,
            [
                "  ✓ fixed  HEAD before GET · saved 20 requests (predicted 20)",
                "  ↓ less   41 one after another",
                "  ! back   listed 5 times",
                "  2 still open",
            ]
        );
        // nothing moved: no "still open" alone
        assert!(change_lines(&[change(State::Open, "x", None)], false).is_empty());
    }
}
