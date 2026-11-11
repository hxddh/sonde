//! What reading a large session may cost, held by a test: sessions of any
//! size are a promise only while something fails when it is broken.
//!
//! Ignored by default (they make sessions of up to a million requests); CI
//! runs them optimised: `cargo test --release -p sonde-proxy --test budgets -- --ignored`.

mod support;

use std::time::{Duration, Instant};

use sonde_proxy::episode::{episodes, observations};
use sonde_proxy::profile::{RequestPrices, profile};
use sonde_proxy::session::Session;
use sonde_proxy::summary::summarize;

/// The most memory this process has held at once, in MiB (Linux; none elsewhere).
fn peak_mib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// What `sonde show` does with a session: read it, count it, group it into
/// operations, find what stands out, the four signals, and what acting on
/// the findings would leave. Returns how long it took and how many requests.
fn read_as_show(path: &std::path::Path) -> (Duration, usize) {
    let started = Instant::now();
    let mut phase = Instant::now();
    let mut lap = |what: &str| {
        eprintln!(
            "  {what:<14} {:>6.2} s  peak {} MiB",
            phase.elapsed().as_secs_f64(),
            peak_mib().unwrap_or(0)
        );
        phase = Instant::now();
    };
    let s = Session::open_lean(path).expect("the session opens");
    lap("open");
    let prices = RequestPrices::default();
    let sum = summarize(&s.records);
    lap("summarize");
    let eps = episodes(&s.records);
    lap("episodes");
    let obs = observations(&s.records, &eps);
    lap("observations");
    let sig = sonde_proxy::signals::signals(&s.records, &prices);
    lap("signals");
    let prof = profile(&s.records, &prices);
    lap("profile");
    let what_if = sonde_proxy::fixes::what_if(&s.records, &prof, &obs);
    lap("what_if");
    assert_eq!(sum.requests as usize, s.records.len());
    assert!(!eps.is_empty() && sig.rate.requests > 0);
    std::hint::black_box((&obs, &what_if));
    (started.elapsed(), s.records.len())
}

#[test]
#[ignore = "a million requests: run in release, as CI does"]
fn a_million_requests_are_read_in_fifteen_seconds_and_two_gigabytes() {
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("million.sonde");
    let n = support::lake(1_000_000, &path, 1);
    assert!(n >= 1_000_000);
    let before = peak_mib();
    let (took, read) = read_as_show(&path);
    assert_eq!(read as u64, n);
    eprintln!(
        "a million requests: {:.1} s, peak {:?} MiB (before: {before:?})",
        took.as_secs_f64(),
        peak_mib()
    );
    assert!(took < Duration::from_secs(15), "took {took:?}");
    // (the generator writes as it goes: it holds a few thousand records)
    if let Some(peak) = peak_mib() {
        assert!(peak < 2048, "peak memory {peak} MiB");
    }
}

#[test]
#[ignore = "three hundred thousand requests: run in release, as CI does"]
fn an_outage_of_three_hundred_thousand_requests_is_read_in_ten_seconds() {
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("outage.sonde");
    support::outage(300_000, &path);
    let (took, read) = read_as_show(&path);
    assert_eq!(read, 300_000);
    eprintln!("an outage of 300 thousand: {:.1} s", took.as_secs_f64());
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

#[test]
#[ignore = "three hundred thousand requests: run in release, as CI does"]
fn small_uploads_of_three_hundred_thousand_requests_are_read_in_ten_seconds() {
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("uploads.sonde");
    support::small_uploads(300_000, &path);
    let (took, read) = read_as_show(&path);
    assert_eq!(read, 300_000);
    eprintln!("small uploads, 300 thousand: {:.1} s", took.as_secs_f64());
    assert!(took < Duration::from_secs(10), "took {took:?}");
}
