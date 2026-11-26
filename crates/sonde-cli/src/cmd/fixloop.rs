//! The fix loop in the terminal: each finding with its saving, its line of
//! code and the fix for the SDK that sent it; and a run against the
//! program's run before it, or another run (`sonde run`, `show`, `show
//! --since`).

use std::path::{Path, PathBuf};

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
            out.extend(diagnosis_full(a, d, head, color));
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

/// A diagnosis in full: its head finding with the diagnosis's saving, its
/// other findings (`also`), its callers and its fix.
fn diagnosis_full(
    a: &Analysis,
    d: &sonde_analysis::fixes::Diagnosis,
    head: &Observation,
    color: bool,
) -> Vec<String> {
    finding_lines(
        head,
        Some(&d.saving_text),
        &also_of(a, d),
        a.sdk.as_deref(),
        color,
    )
}

/// A diagnosis's findings past its head, each said (one kept on purpose
/// with why); its requests one after another as the rest of the change
/// (`and send the other 87 requests 8 at once`).
pub(super) fn also_of(a: &Analysis, d: &sonde_analysis::fixes::Diagnosis) -> Vec<String> {
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
    also
}

/// The findings against the run before as a run again says them, in place
/// of its plan: each fixed with what it saved; each still there (open, less,
/// more) on a line with its first line of code; each diagnosis with a new
/// finding in full, as the plan says it.
fn changed_lines(
    a: &Analysis,
    findings: &[FindingChange],
    uncounted: Option<&str>,
    color: bool,
) -> Vec<String> {
    let change = |id: &str| findings.iter().find(|f| f.identity == id);
    let is_new = |id: &String| change(id).is_some_and(|f| f.state == State::New);
    let mut out: Vec<String> = findings
        .iter()
        .filter(|f| f.state == State::Fixed)
        .map(|f| change_line(f, uncounted, color))
        .collect();
    let (new, kept): (Vec<_>, Vec<_>) = a
        .diagnoses
        .iter()
        .partition(|d| d.identities.iter().any(is_new));
    // a diagnosis a line, its head's; a finding of it that moved (less,
    // more) a line too, those as they were counted on the head's
    let moved = |o: &Observation| {
        change(&o.identity).is_some_and(|f| matches!(f.state, State::Less | State::More))
    };
    for d in kept {
        let found: Vec<&Observation> = d
            .findings
            .iter()
            .filter_map(|&i| a.observations.get(i))
            .collect();
        let Some((head, rest)) = found.split_first() else {
            continue;
        };
        let (moved, same): (Vec<&Observation>, Vec<&Observation>) =
            rest.iter().partition(|o| moved(o));
        for (n, o) in std::iter::once(head).chain(&moved).enumerate() {
            let mut line = match change(&o.identity) {
                Some(f) => change_line(f, uncounted, color),
                None => format!(
                    "  {} {:<5}  {}",
                    paint("·", "2", color),
                    paint(State::Open.label(), "2", color),
                    o.text
                ),
            };
            if let Some(c) = o.callers.first() {
                line.push_str(&paint(&format!(" (at {})", c.place), "2", color));
            }
            if let Some(why) = &o.expected {
                line.push_str(&paint(&format!(" · expected: {why}"), "2", color));
            }
            if n == 0 && !same.is_empty() {
                line.push_str(&paint(
                    &format!(" · and {} beside it", output::count(same.len(), "finding")),
                    "2",
                    color,
                ));
            }
            out.push(line);
        }
    }
    // a change no finding of the run's is (a kind of failure that moved)
    let found = |id: &str| a.observations.iter().any(|o| o.identity == id);
    out.extend(
        findings
            .iter()
            .filter(|f| f.state != State::Fixed && !found(&f.identity))
            .map(|f| change_line(f, uncounted, color)),
    );
    for d in new {
        let Some(head) = d.findings.first().and_then(|&i| a.observations.get(i)) else {
            continue;
        };
        let mut lines = diagnosis_full(a, d, head, color);
        // (its head as it stands: new, or still there with a new finding beside it)
        lines[0] = match change(&head.identity) {
            Some(f) if f.state != State::New => change_line(f, uncounted, color),
            _ => format!(
                "  {} {:<5}  {}",
                paint("!", "33", color),
                paint(State::New.label(), "33", color),
                head.text
            ),
        };
        out.extend(lines);
    }
    out
}

/// Whether a run's change since another can be said as a run again says it
/// (what changed, in place of the plan): two runs of one program, neither
/// failed nor empty.
pub(super) fn comparable(since: &SinceLast) -> bool {
    since.failed.is_none()
        && !since.empty
        && since.previous.is_some()
        && since.change.as_ref().is_some_and(|c| {
            c.same_program && c.failed.is_none() && c.requests.0 > 0 && c.requests.1 > 0
        })
}

/// A finding between two runs, on one line: `✓ fixed  … · 1.2 s less
/// (predicted 1.5 s)`, or `· within noise`; with the command changed or the workload grown
/// between the runs (`uncounted`, [`RunChange::uncounted`]), fixed, less
/// and more say so in place of a saving.
pub(super) fn change_line(f: &FindingChange, uncounted: Option<&str>, color: bool) -> String {
    let (mark, tone) = match f.state {
        State::Fixed => ("✓", "32"),
        State::Less => ("↓", "36"),
        State::More => ("↑", "33"),
        State::New => ("!", "33"),
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

/// The findings of a change worth a line (fixed, less, more, new),
/// then those still open as they were, counted: each is listed above, with
/// its line and fix (`show`).
fn change_lines(findings: &[FindingChange], uncounted: Option<&str>, color: bool) -> Vec<String> {
    let mut out: Vec<String> = findings
        .iter()
        .filter(|f| f.state != State::Open)
        .map(|f| change_line(f, uncounted, color))
        .collect();
    let open: Vec<&FindingChange> = findings.iter().filter(|f| f.state == State::Open).collect();
    // (each is listed above, with its line and fix: counted here)
    if !open.is_empty() && !out.is_empty() {
        out.push(paint(
            &format!("  {} still open, as above", open.len()),
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
/// How a run differs from the run before in what it leaves in the store,
/// said only when it does: `leaves 6 objects fewer than the run before, 1 of
/// a kind it no longer writes (b/out/_SUCCESS)`. Which objects stand, not
/// what is in them.
pub(super) fn left_line(l: &sonde_analysis::profile::LeftChange) -> String {
    let objects = |n: usize| {
        format!(
            "{} object{}",
            sonde_fmt::count(n as u64),
            if n == 1 { "" } else { "s" }
        )
    };
    let mut parts = Vec::new();
    if l.fewer > 0 {
        let mut t = format!("{} fewer", objects(l.fewer));
        if l.absent > 0 {
            t.push_str(&format!(
                ", {} of a kind it no longer writes",
                sonde_fmt::count(l.absent as u64)
            ));
        }
        parts.push(t);
    }
    if l.more > 0 {
        parts.push(format!("{} more", objects(l.more)));
    }
    let mut t = format!("leaves {} than the run before", parts.join(" and "));
    if let Some(k) = &l.key {
        t.push_str(&format!(" ({k})"));
    }
    t
}

pub(super) fn uncounted_line(why: &str) -> String {
    format!("{why}: what moved is not counted as a fix")
}

/// The run's wall time against the run before, said once for the run:
/// `time 4.51 s → 3.64 s (-19%)`, and how much the program's runs vary
/// when that is known (`its runs vary by 120 ms`), `within noise` when it
/// moved by less.
pub(super) fn time_moved(c: &RunChange) -> String {
    let mut t = format!("time {}", moved(c.wall_ms.0, c.wall_ms.1, ms_text));
    if let Some(band) = c.noise_ms {
        t.push_str(&format!(" · its runs vary by {}", ms_text(band)));
        if c.within_noise {
            t.push_str(": within noise");
        }
    }
    t
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

/// A run against the program's run before it (or the run `asked` for,
/// `show --since`), in a few lines: what it asks of the store, the kinds of
/// request that moved (asked), and its findings fixed, less, more and new.
/// Nothing for a program's first run. With the run's analysis (`again`: a
/// run again, its plan not said), each finding still there is on its own
/// line and a new one in full ([`changed_lines`]).
pub(super) fn since_lines(
    since: &SinceLast,
    asked: bool,
    again: Option<&Analysis>,
    color: bool,
) -> Vec<String> {
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
    let head = match (&c.endpoints, asked) {
        (Some((x, y)), _) => format!("since {name} on {x} (this run on {y})"),
        (None, true) => format!("since {name}"),
        (None, false) => format!("since the run before ({name})"),
    };
    out.push(paint(&format!("{head} · {}", time_moved(c)), "1", color));
    // (said only of a run asked for: the run before is the program's)
    let caveat = if !c.same_program {
        Some(
            "not two runs of one program: their figures compare, their findings do not".to_string(),
        )
    } else if let Some(x) = c.failed {
        Some(format!(
            "a run compared failed (exit {x}); their findings are not compared"
        ))
    } else if c.requests.0 == 0 || c.requests.1 == 0 {
        Some(format!(
            "{} in one of them: it recorded no request; their findings are not compared",
            sonde_analysis::program::NOTHING_REACHED
        ))
    } else {
        None
    };
    if asked && let Some(t) = caveat {
        out.push(paint(&format!("  {t}"), "33", color));
    }
    let uncounted = c.uncounted(false);
    if let Some(why) = c.uncounted(true) {
        out.push(paint(&format!("  {}", uncounted_line(&why)), "33", color));
    }
    out.push(format!("  {}", figures_line(c)));
    if let Some(l) = &c.left {
        out.push(paint(&format!("  {}", left_line(l)), "33", color));
    }
    if asked {
        out.extend(kinds_moved(c));
    }
    out.extend(match again {
        Some(a) => changed_lines(a, &since.findings, uncounted.as_deref(), color),
        None => change_lines(&since.findings, uncounted.as_deref(), color),
    });
    out
}

/// The kinds of request that moved between two runs, the largest change
/// first, in a table; the rest counted.
fn kinds_moved(c: &RunChange) -> Vec<String> {
    const ROWS: usize = 10;
    let changed: Vec<&sonde_analysis::profile::GroupChange> = c
        .groups
        .iter()
        .filter(|g| {
            g.requests.0 != g.requests.1
                || (c.waiting_known && (g.waiting_ms.0 - g.waiting_ms.1).abs() >= 1.0)
        })
        .collect();
    let rows: Vec<Vec<String>> = changed
        .iter()
        .take(ROWS)
        .map(|g| {
            vec![
                if g.missed {
                    format!("{} (not found)", g.kind)
                } else {
                    g.kind.clone()
                },
                format!(
                    "{} → {}",
                    output::grouped(g.requests.0 as u64),
                    output::grouped(g.requests.1 as u64)
                ),
                if c.waiting_known {
                    format!("{} → {}", ms_text(g.waiting_ms.0), ms_text(g.waiting_ms.1))
                } else {
                    "—".into()
                },
            ]
        })
        .collect();
    if rows.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = output::table(&["kind", "requests", "waiting"], &rows)
        .to_string()
        .lines()
        .map(|l| format!("  {l}"))
        .collect();
    out.extend(more_kinds(
        changed.len() - rows.len(),
        c.groups.len() - changed.len(),
    ));
    out
}

/// What the table of kinds that moved leaves out: `cut` that changed past
/// its rows, `same` that did not change.
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

/// A run to compare with (`show --since`), read, reckoned and let go
/// before the run shown is read: two runs cost what the larger does.
pub(super) struct Other {
    path: PathBuf,
    header: RunHeader,
    run: sonde_analysis::profile::Run,
}

impl Other {
    pub(super) fn read(s: &Sonde, path: &Path) -> anyhow::Result<Other> {
        let (_, opened) = super::runs::open_lean(s, &path.to_path_buf())?;
        let prices = request_prices_for(s, &opened.header.endpoint);
        let run = sonde_analysis::profile::Run::analysed(&opened.header, &opened.records, &prices);
        Ok(Other {
            path: path.to_path_buf(),
            header: opened.header,
            run,
        })
    }
}

/// `path` (a run, its `header` and `records`, analysed as `a`) against
/// `other` (`show --since`): as against the run before, but read only: the
/// program's kept findings say what was fixed below its threshold, and are
/// left as they are. Two programs' figures compare, their findings do not;
/// nor do a failed run's, nor those of a run nothing reached.
pub(super) fn since_other(
    s: &Sonde,
    path: &Path,
    header: &RunHeader,
    records: &[Record],
    a: &Analysis,
    other: Other,
) -> SinceLast {
    let Other {
        path: other,
        header: hb,
        run: before,
    } = other;
    let traces = sonde_analysis::episode::traces_of(header, records, &a.episodes);
    let mut after = sonde_analysis::profile::Run::later(
        records,
        a.profile.clone(),
        a.observations.clone(),
        traces,
    );
    after.measure(records, &before);
    let (kb, ka) = (
        sonde_analysis::program::key_of(&hb),
        sonde_analysis::program::key_of(header),
    );
    let mut c = sonde_analysis::profile::compare_runs(&before, &after);
    c.waiting_known = !hb.whole_seconds && !header.whole_seconds;
    c.endpoints =
        (hb.endpoint != header.endpoint).then(|| (hb.endpoint.clone(), header.endpoint.clone()));
    c.same_program = ka.is_some() && ka == kb;
    if c.same_program {
        c = c.of_commands(hb.command.as_deref(), header.command.as_deref());
        let kept = sonde_analysis::fixloop::read(&history_of(s, path));
        if let Some(program) = ka.as_ref().and_then(|k| kept.get(k)) {
            program.mark_gone(&mut c.findings, &before, &after);
            program.predict(&mut c.findings);
            c.appeared = sonde_analysis::fixloop::said(&c.findings, &[State::New]);
            c.resolved = sonde_analysis::fixloop::said(&c.findings, &[State::Fixed]);
            // the later run's time against how much the program's runs
            // before it vary, as a run again says it
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let noise =
                sonde_analysis::fixloop::noise_ms(program, &header.created.to_rfc3339(), &name);
            sonde_analysis::fixloop::within_noise(&mut c, noise);
        }
    } else {
        c = c.of_programs(None, None);
    }
    c = c.of_exits((
        sonde_analysis::fixloop::failed(&other),
        sonde_analysis::fixloop::failed(path),
    ));
    // a run nothing reached: what it did not see is not fixed, nor new
    if c.failed.is_none() && (before.requests == 0 || after.requests == 0) {
        c.resolved.clear();
        c.appeared.clear();
        c.findings.clear();
    }
    SinceLast {
        program: ka.unwrap_or_default(),
        failed: None,
        empty: false,
        previous: Some(other.display().to_string()),
        findings: c.findings.clone(),
        change: Some(c),
        unsaved: None,
    }
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
    a.since_last(&history_of(s, path), path, header, records, &|endpoint| {
        request_prices_for(s, endpoint)
    })
}

/// Where a run's program keeps its history: the runs folder, for a run in
/// it or written with `-o` and listed with it, so a run finds its
/// program's runs before it the same way with `-o` or without; else the
/// folder the run is in.
pub(super) fn history_of(s: &Sonde, path: &Path) -> PathBuf {
    let runs = s.paths().runs();
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let own = sonde_analysis::fixloop::folder_of(path);
    let listed = || {
        sonde_analysis::index::elsewhere(&runs)
            .iter()
            .any(|p| canon(p) == canon(path))
    };
    if canon(&own) == canon(&runs) || listed() {
        runs
    } else {
        own
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonde_analysis::fixloop::Saving;

    #[test]
    fn what_a_run_leaves_otherwise_than_the_run_before_is_said_in_a_line() {
        let left = |fewer, more, absent, key: Option<&str>| sonde_analysis::profile::LeftChange {
            fewer,
            more,
            absent,
            key: key.map(Into::into),
        };
        assert_eq!(
            left_line(&left(2, 0, 1, Some("b/out/_SUCCESS"))),
            "leaves 2 objects fewer, 1 of a kind it no longer writes than the run before (b/out/_SUCCESS)"
        );
        assert_eq!(
            left_line(&left(1, 1500, 0, None)),
            "leaves 1 object fewer and 1,500 objects more than the run before"
        );
    }

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
            saved_requests: None,
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
                    Some("1.2 s less (predicted 1.5 s)"),
                ),
                change(State::Less, "41 one after another", Some("within noise")),
                change(State::New, "listed 5 times", None),
                change(State::Open, "small uploads", None),
                change(State::Open, "hot key", None),
            ],
            None,
            false,
        );
        assert_eq!(
            lines,
            [
                "  ✓ fixed  HEAD before GET · 1.2 s less (predicted 1.5 s)",
                "  ↓ less   41 one after another · within noise",
                "  ! new    listed 5 times",
                // each listed above, with its line and fix
                "  2 still open, as above",
            ]
        );
        // nothing moved: no "still open" alone
        assert!(change_lines(&[change(State::Open, "x", None)], None, false).is_empty());
    }

    #[test]
    fn the_kinds_that_moved_say_how_many_their_table_leaves_out() {
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
    fn a_finding_that_grew_says_more_with_its_counts() {
        let mut grew = change(State::More, "b/logs/ was listed 216 times", None);
        grew.counts = Some([92, 216]);
        assert_eq!(
            change_line(&grew, None, false),
            "  ↑ more   b/logs/ was listed 216 times · 92 → 216"
        );
    }
}
