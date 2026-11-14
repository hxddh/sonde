//! What reading a large session may cost, held by a test: sessions of any
//! size are a promise only while something fails when it is broken.
//!
//! Each test holds its peak memory to a budget: the same from run to run of
//! the same code, so a miss is a change, never the runner. Its time, which a
//! shared runner moves by half and more, is said (a `budget` line each) and
//! held only at several times what it takes, so a test fails on a time
//! that is wrong, not on a slow machine. docs/performance.md has the figures.
//!
//! Ignored by default (they make sessions of up to a million requests); CI
//! runs them optimised, each in a process of its own:
//! `cargo test --release -p sonde-proxy --test budgets -- --ignored --exact NAME`.

mod support;

use std::time::{Duration, Instant};

use sonde_proxy::profile::RequestPrices;
use sonde_proxy::session::Session;
use sonde_proxy::view::Analysis;

/// One at a time: the peak of memory is the process's, and the clock is
/// kinder to a test that has the cores to itself. The million goes first
/// (the others give it a moment to take its turn): its peak is then its own,
/// not what the tests before it left behind.
static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn alone(first: bool) -> std::sync::MutexGuard<'static, ()> {
    if !first {
        std::thread::sleep(Duration::from_millis(500));
    }
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Starts the count of [`peak_mib`] again from what is held now (Linux):
/// a test's peak is then its own, not the one before it.
fn peak_from_now() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

/// The most memory this process has held at once, in MiB (Linux; none elsewhere).
fn peak_mib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// Says what a test cost (`budget NAME: …`, the line CI puts in its
/// summary) and holds it: the peak (since [`peak_from_now`]) under `mib`,
/// the time under `room` (several times what it takes: a time past it is a
/// change, not a busy runner).
fn held(name: &str, took: Duration, room: Duration, mib: u64) {
    let peak = peak_mib();
    eprintln!(
        "budget {name}: peak {} MiB of {mib}; {:.2} s (held at {:.0} s)",
        peak.map_or("?".into(), |p| p.to_string()),
        took.as_secs_f64(),
        room.as_secs_f64()
    );
    if let Some(peak) = peak {
        assert!(peak < mib, "{name}: peak memory {peak} MiB, past its {mib}");
    }
    assert!(took < room, "{name}: took {took:?}, past {room:?}");
}

/// What `sonde show` does with a session: read it (lean: what the analyses
/// read), then count it, group it into operations, find what stands out,
/// reckon the four signals and where the time went, and what acting on the
/// findings would leave ([`Analysis::of`]: `show`, `check` and the app's
/// view all go through it), and the run against its program's run before
/// it ([`Analysis::since_last`]). Returns how long it took, and the analysis.
fn read_as_show(path: &std::path::Path) -> (Duration, usize, Analysis) {
    let started = Instant::now();
    let s = Session::open_lean(path).expect("the session opens");
    let opened = started.elapsed();
    let a = Analysis::of(&s, &RequestPrices::default());
    // and, a run of a program, against the run before it (none here)
    let since = a.since_last(path, &s.header, &s.records, &|_| RequestPrices::default());
    assert!(since.is_some_and(|c| c.previous.is_none() && !c.findings.is_empty()));
    let took = started.elapsed();
    eprintln!(
        "  read {:.2} s, analysed {:.2} s, peak {} MiB",
        opened.as_secs_f64(),
        (took - opened).as_secs_f64(),
        peak_mib().unwrap_or(0)
    );
    assert_eq!(a.summary.requests as usize, s.records.len());
    assert!(!a.episodes.is_empty() && a.signals.rate.requests > 0);
    (took, s.records.len(), a)
}

/// What a million requests of the lake job may hold at once: what is
/// measured (docs/performance.md) and a tenth more for another machine's
/// threads and allocator, not for a request held whole again.
const MILLION_MIB: u64 = 1500;

#[test]
#[ignore = "a million requests: run in release, as CI does"]
fn a_million_requests_are_read_within_their_memory() {
    let _alone = alone(true);
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("million.sonde");
    let n = support::lake(1_000_000, &path, 1);
    assert!(n >= 1_000_000);
    let before = peak_mib();
    peak_from_now();
    let (took, read, analysis) = read_as_show(&path);
    assert_eq!(read as u64, n);
    eprintln!(
        "a million requests: {:.1} s, peak {:?} MiB (before reading: {before:?}), {} operations, {} findings",
        took.as_secs_f64(),
        peak_mib(),
        analysis.episodes.len(),
        analysis.observations.len()
    );
    // the lake job has what a real one has: the findings are there to be found
    assert!(analysis.observations.len() >= 4 && analysis.what_if.is_some());
    held("show of a million", took, Duration::from_secs(45), MILLION_MIB);
}

/// What `sonde compare` does with two runs: each read (lean), reckoned and
/// let go before the other is read ([`Run::analysed`]), then compared.
#[test]
#[ignore = "two runs of a million requests: run in release, as CI does"]
fn two_runs_of_a_million_requests_compare_within_what_one_costs() {
    use sonde_proxy::profile::{Run, compare_runs};
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let (a, b) = (dir.path().join("a.sonde"), dir.path().join("b.sonde"));
    support::lake(1_000_000, &a, 1);
    support::lake(1_000_000, &b, 2);
    peak_from_now();
    let started = Instant::now();
    let run = |p: &std::path::Path| {
        let s = Session::open_lean(p).expect("the session opens");
        Run::analysed(&s.header, &s.records, &RequestPrices::default())
    };
    let (ra, rb) = (run(&a), run(&b));
    let c = compare_runs(&ra, &rb);
    let took = started.elapsed();
    eprintln!(
        "two runs of a million compared: {:.1} s, peak {:?} MiB",
        took.as_secs_f64(),
        peak_mib()
    );
    assert!(c.requests.0 >= 1_000_000 && c.requests.1 >= 1_000_000);
    assert!(!c.groups.is_empty() && !c.prefixes.is_empty());
    // the second run is read once the first is let go: one run's memory
    held("compare of two millions", took, Duration::from_secs(120), MILLION_MIB);
}

#[test]
#[ignore = "three hundred thousand requests: run in release, as CI does"]
fn an_outage_of_three_hundred_thousand_requests_is_read_within_its_memory() {
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("outage.sonde");
    support::outage(300_000, &path);
    peak_from_now();
    let (took, read, _) = read_as_show(&path);
    assert_eq!(read, 300_000);
    held("show of an outage of 300 thousand", took, Duration::from_secs(30), 1500);
}

#[test]
#[ignore = "three hundred thousand requests: run in release, as CI does"]
fn small_uploads_of_three_hundred_thousand_requests_are_read_within_their_memory() {
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("uploads.sonde");
    support::small_uploads(300_000, &path);
    peak_from_now();
    let (took, read, analysis) = read_as_show(&path);
    // every upload is in the finding, and acting on it leaves one PUT each
    let small = analysis
        .observations
        .iter()
        .find(|o| o.phrases[0].key == "obs.small")
        .expect("small files uploaded in parts");
    assert_eq!(small.saves_requests, 200_000);
    assert_eq!(read, 300_000);
    held("show of 300 thousand small uploads", took, Duration::from_secs(30), 1500);
}

#[test]
#[ignore = "a hundred thousand requests: run in release, as CI does"]
fn the_apps_first_view_of_a_hundred_thousand_requests_is_built_within_its_memory() {
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("view.sonde");
    let n = support::lake(100_000, &path, 2);
    peak_from_now();
    // what the bridge does when a window opens a session: read it, analyse
    // it, and build the view that is sent (the first page and the totals)
    let started = Instant::now();
    let s = Session::open(&path).expect("the session opens");
    let a = Analysis::of(&s, &RequestPrices::default());
    let view = serde_json::Value::Object(a.view(&path.to_string_lossy(), &s.header));
    let took = started.elapsed();
    let sent = serde_json::to_string(&view)
        .expect("the view as text")
        .len();
    eprintln!(
        "the first view of {n} requests: {:.2} s, {} KiB sent for {} operations",
        took.as_secs_f64(),
        sent / 1024,
        a.episodes.len()
    );
    held("the app's first view of 100 thousand", took, Duration::from_secs(6), 1500);
    // and it does not grow with the session: a page of operations, not all
    assert_eq!(
        view["operations"].as_array().map(Vec::len),
        Some(sonde_proxy::view::PAGE)
    );
    assert_eq!(view["operationsTotal"], a.episodes.len());
    assert!(sent < 2 << 20, "{sent} bytes sent");
    // the next page is a lookup, not another analysis
    let started = Instant::now();
    let q: sonde_proxy::view::Query = serde_json::from_value(
        serde_json::json!({ "offset": 50_000, "sort": "duration", "descending": true }),
    )
    .expect("a query");
    let page = a.page(&q);
    assert_eq!(
        page["rows"].as_array().map(Vec::len),
        Some(sonde_proxy::view::PAGE)
    );
    let first = started.elapsed();
    let started = Instant::now();
    let _ = a.page(&sonde_proxy::view::Query {
        offset: 50_500,
        ..q
    });
    eprintln!(
        "a sorted page: {:.0} ms the first time, {:.1} ms the next",
        first.as_secs_f64() * 1000.0,
        started.elapsed().as_secs_f64() * 1000.0
    );
    // (a lookup: held at a few times what it takes, as the view is)
    assert!(first < Duration::from_millis(1500), "a page took {first:?}");
}
