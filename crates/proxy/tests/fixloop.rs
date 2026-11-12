//! The fix loop on recorded runs of one boto3 program (`etl`, against moto):
//! 1 and 2 the same program twice, 3 without its HEAD before each GET, 4
//! also without small files uploaded in parts, 5 both undone. A finding
//! keeps its identity across runs; fixed is gone at any threshold, less is
//! under it; what a fix saved is measured against what was predicted.

use std::path::{Path, PathBuf};

use chrono::DateTime;
use reqlog::Record;
use sonde_proxy::fixloop::{self, State};
use sonde_proxy::profile::{RequestPrices, Run, compare_runs};
use sonde_proxy::session::Session;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

const RUNS: [&str; 5] = [
    "etl-1-base.sonde",
    "etl-2-again.sonde",
    "etl-3-no-head.sonde",
    "etl-4-no-small.sonde",
    "etl-5-undone.sonde",
];

/// A run as `sonde compare` reads it, counted against `before` when given.
fn run(name: &str, before: Option<&Run>) -> Run {
    let s = Session::open_lean(&fixture(name)).expect("the fixture opens");
    let mut r = Run::analysed(&s.header, &s.records, &RequestPrices::default());
    if let Some(b) = before {
        r.measure(&s.records, b);
    }
    r
}

fn kinds(r: &Run) -> Vec<String> {
    let mut ids: Vec<String> = r.observations.iter().map(|o| o.identity.clone()).collect();
    ids.sort();
    ids
}

fn state_of(c: &[fixloop::FindingChange], kind: &str) -> State {
    c.iter()
        .find(|f| f.identity.starts_with(kind))
        .unwrap_or_else(|| panic!("{kind} in {c:#?}"))
        .state
}

#[test]
fn a_findings_identity_is_the_same_in_two_runs_of_the_same_program() {
    let (a, b) = (run(RUNS[0], None), run(RUNS[1], None));
    // what the runs are about, never their counts or ids
    assert_eq!(
        kinds(&a),
        [
            "obs.headget|",
            "obs.listed|etl-bucket/run/in/",
            "obs.serial|",
            "obs.small|"
        ]
    );
    assert_eq!(kinds(&a), kinds(&b));
    let b = run(RUNS[1], Some(&a));
    let c = compare_runs(&a, &b);
    assert!(c.findings.iter().all(|f| f.state == State::Open), "{c:#?}");
    assert!(c.resolved.is_empty() && c.appeared.is_empty());
}

#[test]
fn a_fixed_finding_says_what_it_saved_against_what_was_predicted() {
    let a = run(RUNS[1], None);
    let b = run(RUNS[2], Some(&a));
    let c = compare_runs(&a, &b);
    let head = &c.findings[0];
    assert_eq!(head.state, State::Fixed, "{c:#?}");
    assert!(head.identity.starts_with("obs.headget"));
    assert_eq!(head.predicted.requests, 20);
    assert_eq!(head.measured.map(|m| m.requests), Some(20));
    assert_eq!(
        head.saved_text.as_deref(),
        Some("saved 20 requests (predicted 20)")
    );
    assert_eq!(c.resolved.len(), 1);
    assert_eq!(state_of(&c.findings, "obs.small"), State::Open);

    let d = run(RUNS[3], Some(&b));
    let c = compare_runs(&b, &d);
    let small = c
        .findings
        .iter()
        .find(|f| f.identity.starts_with("obs.small"))
        .unwrap();
    assert_eq!(small.state, State::Fixed);
    assert_eq!(
        (small.predicted.requests, small.measured.map(|m| m.requests)),
        (12, Some(12))
    );
    // not counted against the run before: nothing measured
    let unmeasured = compare_runs(&b, &run(RUNS[3], None));
    assert!(unmeasured.findings[0].measured.is_none());
}

/// The program's kept findings over the five runs, each taken in as
/// `sonde run` does: open, fixed with its saving, back when undone.
#[test]
fn a_programs_findings_are_kept_open_fixed_and_back_over_its_runs() {
    let dir = tempfile::tempdir().unwrap();
    for name in RUNS {
        std::fs::copy(fixture(name), dir.path().join(name)).unwrap();
    }
    let prices = |_: &str| RequestPrices::default();
    let since = |name: &str| {
        let path = dir.path().join(name);
        let s = Session::open_lean(&path).unwrap();
        let r = Run::analysed(&s.header, &s.records, &RequestPrices::default());
        fixloop::since_last(dir.path(), &path, &s.header, &s.records, r, &prices)
            .expect("a run of a program")
    };
    let first = since(RUNS[0]);
    assert_eq!(first.program, "etl");
    assert!(first.previous.is_none() && first.change.is_none());
    assert!(first.findings.iter().all(|f| f.state == State::Open));
    since(RUNS[1]);
    let third = since(RUNS[2]);
    assert!(third.previous.as_deref().unwrap().ends_with(RUNS[1]));
    assert_eq!(state_of(&third.findings, "obs.headget"), State::Fixed);
    let kept = fixloop::read(dir.path()).unwrap();
    let head = &kept["etl"].findings["obs.headget|"];
    assert_eq!(head.state, State::Fixed);
    assert_eq!(head.since, RUNS[2]);
    assert_eq!(head.measured.map(|m| m.requests), Some(20));
    since(RUNS[3]);
    let undone = since(RUNS[4]);
    assert_eq!(state_of(&undone.findings, "obs.headget"), State::Back);
    assert_eq!(state_of(&undone.findings, "obs.small"), State::Back);
    let kept = fixloop::read(dir.path()).unwrap();
    assert_eq!(kept["etl"].findings["obs.headget|"].state, State::Back);
    assert_eq!(
        kept["etl"].findings["obs.listed|etl-bucket/run/in/"].state,
        State::Open
    );

    // an older run shown again changes nothing kept
    let before = std::fs::read(dir.path().join("findings.json")).unwrap();
    let again = since(RUNS[2]);
    assert_eq!(state_of(&again.findings, "obs.headget"), State::Fixed);
    assert_eq!(
        std::fs::read(dir.path().join("findings.json")).unwrap(),
        before
    );
}

fn get(id: u64, key: &str) -> Record {
    Record {
        id,
        time: DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 10).unwrap(),
        method: "GET".into(),
        operation: "GetObject".into(),
        bucket: Some("b".into()),
        key: Some(key.into()),
        status: Some(200),
        latency_ms: 5.0,
        ..Default::default()
    }
}

/// The walkthrough's false ✓: a finding under its threshold is less, not
/// fixed, and says what it saved so far.
#[test]
fn a_finding_under_its_threshold_is_less_and_one_gone_is_fixed() {
    let before: Vec<Record> = (1..=12).map(|i| get(i, "hot")).collect();
    // read three times: under the ten a finding needs, still read again
    let after: Vec<Record> = (1..=3).map(|i| get(i, "hot")).collect();
    let once: Vec<Record> = vec![get(1, "hot")];
    let of = |rs: &[Record]| {
        let eps = sonde_proxy::episode::episodes(rs);
        Run::of(
            rs,
            sonde_proxy::profile::profile(rs, &RequestPrices::default()),
            sonde_proxy::episode::observations(rs, &eps),
        )
    };
    let a = of(&before);
    let mut b = of(&after);
    b.measure(&after, &a);
    let c = compare_runs(&a, &b);
    assert_eq!(c.findings.len(), 1);
    let hot = &c.findings[0];
    assert_eq!(hot.state, State::Less);
    assert_eq!(hot.text, "b/hot was read 3 times (100% of all requests)");
    assert_eq!(
        hot.was.as_ref().unwrap().text,
        "b/hot was read 12 times (100% of all requests)"
    );
    assert_eq!(hot.measured.map(|m| m.requests), Some(9));
    assert_eq!(
        hot.saved_text.as_deref(),
        Some("saved 9 requests (predicted 11)")
    );
    // less is not resolved
    assert!(c.resolved.is_empty());

    let mut b = of(&once);
    b.measure(&once, &a);
    let c = compare_runs(&a, &b);
    assert_eq!(c.findings[0].state, State::Fixed);
    assert_eq!(c.findings[0].measured.map(|m| m.requests), Some(11));
    assert_eq!(c.resolved.len(), 1);
}
