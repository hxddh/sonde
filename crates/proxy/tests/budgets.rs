//! What reading a large session may cost, held by a test: sessions of any
//! size are a promise only while something fails when it is broken.
//!
//! Ignored by default (they make sessions of up to a million requests); CI
//! runs them optimised: `cargo test --release -p sonde-proxy --test budgets -- --ignored`.

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

/// The most memory this process has held at once, in MiB (Linux; none elsewhere).
fn peak_mib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// What `sonde show` does with a session: read it (lean: what the analyses
/// read), then count it, group it into operations, find what stands out,
/// reckon the four signals and where the time went, and what acting on the
/// findings would leave ([`Analysis::of`]: `show`, `check` and the app's
/// view all go through it). Returns how long it took, and the analysis.
fn read_as_show(path: &std::path::Path) -> (Duration, usize, Analysis) {
    let started = Instant::now();
    let s = Session::open_lean(path).expect("the session opens");
    let opened = started.elapsed();
    let a = Analysis::of(&s, &RequestPrices::default());
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

#[test]
#[ignore = "a million requests: run in release, as CI does"]
fn a_million_requests_are_read_in_fifteen_seconds_and_two_gigabytes() {
    let _alone = alone(true);
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("million.sonde");
    let n = support::lake(1_000_000, &path, 1);
    assert!(n >= 1_000_000);
    let before = peak_mib();
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
    assert!(took < Duration::from_secs(15), "took {took:?}");
    // (the generator writes as it goes: it holds a few thousand records)
    if let Some(peak) = peak_mib() {
        assert!(peak < 2048, "peak memory {peak} MiB");
    }
}

#[test]
#[ignore = "three hundred thousand requests: run in release, as CI does"]
fn an_outage_of_three_hundred_thousand_requests_is_read_in_ten_seconds() {
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("outage.sonde");
    support::outage(300_000, &path);
    let (took, read, _) = read_as_show(&path);
    assert_eq!(read, 300_000);
    eprintln!("an outage of 300 thousand: {:.1} s", took.as_secs_f64());
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

#[test]
#[ignore = "three hundred thousand requests: run in release, as CI does"]
fn small_uploads_of_three_hundred_thousand_requests_are_read_in_ten_seconds() {
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("uploads.sonde");
    support::small_uploads(300_000, &path);
    let (took, read, analysis) = read_as_show(&path);
    // every upload is in the finding, and acting on it leaves one PUT each
    let small = analysis
        .observations
        .iter()
        .find(|o| o.phrases[0].key == "obs.small")
        .expect("small files uploaded in parts");
    assert_eq!(small.saves_requests, 200_000);
    assert_eq!(read, 300_000);
    eprintln!("small uploads, 300 thousand: {:.1} s", took.as_secs_f64());
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

#[test]
#[ignore = "a hundred thousand requests: run in release, as CI does"]
fn the_apps_first_view_of_a_hundred_thousand_requests_is_built_in_two_seconds() {
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("view.sonde");
    let n = support::lake(100_000, &path, 2);
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
    assert!(took < Duration::from_secs(2), "took {took:?}");
    // and it does not grow with the session: a page of operations, not all
    assert_eq!(
        view["episodes"].as_array().map(Vec::len),
        Some(sonde_proxy::view::PAGE)
    );
    assert_eq!(view["episodesTotal"], a.episodes.len());
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
    assert!(first < Duration::from_millis(500), "a page took {first:?}");
}
