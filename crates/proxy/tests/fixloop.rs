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
            "obs.headget|etl-bucket/run/in/",
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
    let head = &kept["etl"].findings["obs.headget|etl-bucket/run/in/"];
    assert_eq!(head.state, State::Fixed);
    assert_eq!(head.since, RUNS[2]);
    assert_eq!(head.measured.map(|m| m.requests), Some(20));
    since(RUNS[3]);
    let undone = since(RUNS[4]);
    assert_eq!(state_of(&undone.findings, "obs.headget"), State::Back);
    assert_eq!(state_of(&undone.findings, "obs.small"), State::Back);
    let kept = fixloop::read(dir.path()).unwrap();
    assert_eq!(
        kept["etl"].findings["obs.headget|etl-bucket/run/in/"].state,
        State::Back
    );
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

fn list(id: u64, prefix: &str) -> Record {
    Record {
        operation: "ListObjectsV2".into(),
        key: None,
        query: vec![("prefix".into(), prefix.to_string().into())],
        ..get(id, "")
    }
}

/// A run of a program whose keys change every run (`seed`): a job folder
/// named by the time it started, a table's metadata version, a Hadoop
/// attempt folder, a run id and a date. Its findings are the same each run.
fn changing_run(seed: u64) -> Vec<Record> {
    let ms = 1_794_558_464_595 + seed * 1_000;
    let uuid = format!(
        "{:08x}-{:04x}-4abc-8def-{:012x}",
        seed * 7_919,
        seed,
        seed * 104_729
    );
    let mut out: Vec<Record> = Vec::new();
    let read = |out: &mut Vec<Record>, n: usize, key: String| {
        for _ in 0..n {
            out.push(get(out.len() as u64 + 1, &key));
        }
    };
    read(&mut out, 16, format!("jobs/{ms}/state.json"));
    read(
        &mut out,
        14,
        format!("tbl/metadata/v{}.metadata.json", seed + 3),
    );
    read(
        &mut out,
        12,
        format!(
            "out/_temporary/{seed}/attempt_2026111310{seed:02}_0001_m_000001_{seed}/part-00000"
        ),
    );
    for _ in 0..6 {
        let prefix = format!("runs/{uuid}/at=2026-11-{:02}T10:{seed:02}:00Z/", 10 + seed);
        out.push(list(out.len() as u64 + 1, &prefix));
    }
    out
}

fn run_of(rs: &[Record]) -> Run {
    let eps = sonde_proxy::episode::episodes(rs);
    Run::of(
        rs,
        sonde_proxy::profile::profile(rs, &RequestPrices::default()),
        sonde_proxy::episode::observations(rs, &eps),
    )
}

/// 18.0's false ✓: keys that carry a run's own time or id made each run's
/// findings new, and the run before's fixed, though nothing changed.
#[test]
fn two_runs_of_an_unchanged_program_whose_keys_change_every_run_read_no_change() {
    let (ra, rb) = (changing_run(1), changing_run(2));
    let a = run_of(&ra);
    let mut b = run_of(&rb);
    b.measure(&rb, &a);
    let ids = kinds(&a);
    for id in [
        "obs.hot|b/jobs/{n}/state.json",
        "obs.hot|b/tbl/metadata/v{n}.metadata.json",
        "obs.hot|b/out/_temporary/{n}/attempt_{n}_{n}_m_{n}_{n}/part-{n}",
        "obs.listed|b/runs/{uuid}/at={v}/",
    ] {
        assert!(ids.contains(&id.to_string()), "{id} in {ids:?}");
    }
    assert_eq!(ids, kinds(&b));
    let c = compare_runs(&a, &b);
    assert!(
        c.findings.iter().all(|f| f.state == State::Open),
        "{:#?}",
        c.findings
    );
    assert!(c.resolved.is_empty() && c.appeared.is_empty());
}

/// A session of `records` written to `path`, as `sonde run` leaves one.
fn write_session(path: &Path, header: &sonde_proxy::session::SessionHeader, records: &[Record]) {
    use reqlog::Sink;
    let w = sonde_proxy::session::SessionWriter::create(path, header).unwrap();
    for r in records {
        w.write(r);
    }
    w.flush().unwrap();
}

/// A run of the program `job` recorded `minute` minutes in, kept in `dir`;
/// what `sonde run` says of it against the run before.
fn ran(dir: &Path, minute: i64, records: &[Record], exit: Option<i32>) -> fixloop::SinceLast {
    let (path, header) = job(dir, minute);
    write_session(&path, &header, records);
    if let Some(x) = exit {
        sonde_proxy::index::set(&path, sonde_proxy::index::Change::Exit(x));
    }
    shown(dir, minute, records)
}

/// The run of `job` recorded `minute` minutes in: its path and header.
fn job(dir: &Path, minute: i64) -> (PathBuf, sonde_proxy::session::SessionHeader) {
    let mut header =
        sonde_proxy::session::SessionHeader::new("e", "http://e", "", Some("python job.py".into()));
    header.name = Some("job".into());
    header.created = DateTime::from_timestamp_millis(1_700_000_000_000 + minute * 60_000).unwrap();
    (dir.join(format!("run-{minute:02}.sonde")), header)
}

/// What `sonde show` says of a run of `job` kept already.
fn shown(dir: &Path, minute: i64, records: &[Record]) -> fixloop::SinceLast {
    let (path, header) = job(dir, minute);
    fixloop::since_last(dir, &path, &header, records, run_of(records), &|_: &str| {
        RequestPrices::default()
    })
    .expect("a run of a program")
}

/// Kept over runs: the program's findings do not grow with keys that change.
#[test]
fn a_programs_kept_findings_do_not_grow_with_keys_that_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut kept = Vec::new();
    for seed in 1..=3u64 {
        let since = ran(dir.path(), seed as i64, &changing_run(seed), Some(0));
        assert!(
            since.findings.iter().all(|f| f.state == State::Open),
            "run {seed}: {:#?}",
            since.findings
        );
        kept.push(fixloop::read(dir.path()).unwrap()["job"].findings.len());
    }
    assert_eq!(kept[0], kept[2], "{kept:?}");
}

fn hot(reads: u64) -> Vec<Record> {
    (1..=reads).map(|i| get(i, "hot")).collect()
}

/// 18.0 said a crash fixed what it did not get to, and the next run back.
#[test]
fn a_run_that_failed_is_not_compared_and_changes_nothing_kept() {
    let dir = tempfile::tempdir().unwrap();
    let first = ran(dir.path(), 1, &hot(12), Some(0));
    assert!(first.failed.is_none());
    let kept = std::fs::read(dir.path().join("findings.json")).unwrap();
    // it crashed after one request
    let crashed = ran(dir.path(), 2, &hot(1), Some(3));
    assert_eq!(crashed.failed, Some(3));
    assert!(crashed.change.is_none() && crashed.findings.is_empty());
    assert_eq!(
        std::fs::read(dir.path().join("findings.json")).unwrap(),
        kept
    );
    // the next is compared with the last that did not fail
    let next = ran(dir.path(), 3, &hot(12), Some(0));
    assert!(next.previous.as_deref().unwrap().ends_with("run-01.sonde"));
    assert_eq!(state_of(&next.findings, "obs.hot"), State::Open);
}

/// `show` of an older run says what it was against the run before it, not
/// what later runs made of its findings.
#[test]
fn back_is_judged_by_the_runs_compared_not_by_later_ones() {
    let dir = tempfile::tempdir().unwrap();
    let good = || hot(1);
    ran(dir.path(), 1, &good(), Some(0));
    let bad = ran(dir.path(), 2, &hot(12), Some(0));
    assert_eq!(state_of(&bad.findings, "obs.hot"), State::New);
    let fixed = ran(dir.path(), 3, &good(), Some(0));
    assert_eq!(state_of(&fixed.findings, "obs.hot"), State::Fixed);
    // the bad run shown again: new then, whatever came after
    let again = shown(dir.path(), 2, &hot(12));
    assert_eq!(state_of(&again.findings, "obs.hot"), State::New);
    let ch = again.change.unwrap();
    assert_eq!(ch.appeared.len(), 1);
    // and back once it was fixed before
    let back = ran(dir.path(), 4, &hot(12), Some(0));
    assert_eq!(state_of(&back.findings, "obs.hot"), State::Back);
}

/// Less is kept: over its threshold again it is back, not new.
#[test]
fn less_is_kept_and_over_its_threshold_again_is_back() {
    let dir = tempfile::tempdir().unwrap();
    ran(dir.path(), 1, &hot(12), Some(0));
    let less = ran(dir.path(), 2, &hot(3), Some(0));
    assert_eq!(state_of(&less.findings, "obs.hot"), State::Less);
    let kept = |dir: &Path| fixloop::read(dir).unwrap()["job"].findings["obs.hot|b/hot"].state;
    assert_eq!(kept(dir.path()), State::Less);
    let over = ran(dir.path(), 3, &hot(12), Some(0));
    assert_eq!(state_of(&over.findings, "obs.hot"), State::Back);
    assert_eq!(kept(dir.path()), State::Back);
}

/// `n` requests one after another, each `ms` long.
fn chain(n: u64, ms: i64) -> Vec<Record> {
    (1..=n)
        .map(|i| Record {
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + i as i64 * (ms + 1)).unwrap(),
            latency_ms: ms as f64,
            ..get(i, &format!("k{i}"))
        })
        .collect()
}

/// A finding of time alone is judged by its requests: timing that drops it
/// under its threshold leaves it open, and what it saved is its own waiting.
#[test]
fn a_finding_of_time_is_judged_by_its_requests_not_its_timing() {
    let serial = |c: &[fixloop::FindingChange]| {
        c.iter()
            .find(|f| f.identity == "obs.serial|")
            .cloned()
            .unwrap_or_else(|| panic!("obs.serial in {c:#?}"))
    };
    let a = run_of(&chain(20, 10));
    assert!(a.observations.iter().any(|o| o.identity == "obs.serial|"));
    // the same 20 requests, faster: under its threshold, still there
    let quick = chain(20, 5);
    let mut b = run_of(&quick);
    assert!(b.observations.is_empty());
    b.measure(&quick, &a);
    let c = compare_runs(&a, &b);
    assert_eq!(serial(&c.findings).state, State::Open);
    assert!(c.resolved.is_empty() && c.appeared.is_empty());
    // half of them: less, and saved its own waiting, not the whole drop
    let fewer = chain(10, 5);
    let mut b = run_of(&fewer);
    b.measure(&fewer, &a);
    let c = compare_runs(&a, &b);
    let s = serial(&c.findings);
    assert_eq!(s.state, State::Less);
    let ms = s.measured.unwrap().ms;
    assert!((ms - 130.0).abs() < 1.0, "{ms}");
    assert!(ms < a.profile.waiting_ms - b.profile.waiting_ms);
}

/// A run reading keys laid out as tables and jobs lay them out, each named
/// anew every run (`seed`): Hive partitions, a date-hour value, Delta's log,
/// Iceberg's metadata and snapshots, Spark's part files, a short job id, a
/// counter and a time folder. Two prefixes that differ by a number alone
/// (`t00/`, `t01/`) are listed again and again: two findings. A run reads
/// three of the keys (`part`): three keys read hot are what a run shows.
fn layouts_run(seed: u64, part: usize) -> Vec<Record> {
    let uuid = |n: u64| {
        format!(
            "{:08x}-{:04x}-4abc-8def-{:012x}",
            n * 7_919 + seed,
            seed,
            n * 104_729 + seed
        )
    };
    let keys = [
        format!(
            "in/year=2026/month=11/day=13/hour={:02}/conf.json",
            10 + seed
        ),
        format!("ev/dt=2026-11-13-{:02}/manifest.json", 20 + seed),
        format!("t/_delta_log/{:020}.json", seed + 1),
        format!("t/metadata/{:05}-{}.metadata.json", seed + 2, uuid(1)),
        format!(
            "t/metadata/snap-{}-1-{}.avro",
            6_894_157_311_395_042_398 + seed,
            uuid(2)
        ),
        format!("o/part-00000-{}-c000.snappy.parquet", uuid(3)),
        format!("jobs/job-{:08x}/state.json", 0x3fa2_b1c9 + seed),
        format!("runs/run-{}/out.json", 40 + seed),
        format!("logs/22-{:02}-01/x.log", 40 + seed),
    ];
    let mut out: Vec<Record> = Vec::new();
    for (i, key) in keys.iter().enumerate().skip(part * 3).take(3) {
        for _ in 0..(12 + i) {
            out.push(get(out.len() as u64 + 1, key));
        }
    }
    for prefix in ["t00/", "t01/"] {
        for _ in 0..6 {
            out.push(list(out.len() as u64 + 1, prefix));
        }
    }
    out
}

/// 19.0's false ✓ and !: an unchanged job whose keys carry an hour, a
/// partition, a version or an id read "no change"; and what differs by a
/// number alone in one run stays two findings.
#[test]
fn an_unchanged_job_over_real_layouts_reads_no_change() {
    let expected = [
        "obs.hot|b/in/year={v}/month={v}/day={v}/hour={v}/conf.json",
        "obs.hot|b/ev/dt={v}/manifest.json",
        "obs.hot|b/t/_delta_log/{n}.json",
        "obs.hot|b/t/metadata/{n}-{uuid}.metadata.json",
        "obs.hot|b/t/metadata/snap-{n}-{n}-{uuid}.avro",
        "obs.hot|b/o/part-{n}-{uuid}-c{n}.snappy.parquet",
        "obs.hot|b/jobs/job-{id}/state.json",
        "obs.hot|b/runs/run-{n}/out.json",
        "obs.hot|b/logs/{n}-{n}-{n}/x.log",
    ];
    for part in 0..3 {
        let (ra, rb) = (layouts_run(1, part), layouts_run(2, part));
        let a = run_of(&ra);
        let mut b = run_of(&rb);
        b.measure(&rb, &a);
        let ids = kinds(&a);
        // two prefixes alike once their numbers are out: their own keys
        for id in expected[part * 3..part * 3 + 3]
            .iter()
            .chain(&["obs.listed|b/t00/", "obs.listed|b/t01/"])
        {
            assert!(ids.contains(&id.to_string()), "{id} in {ids:?}");
        }
        assert_eq!(ids, kinds(&b));
        let c = compare_runs(&a, &b);
        assert!(
            c.findings.iter().all(|f| f.state == State::Open),
            "{:#?}",
            c.findings
        );
        assert!(c.resolved.is_empty() && c.appeared.is_empty());
    }
}

/// 19.0 called a prefix listed 92 → 216 times "less": it had fallen out of
/// the findings shown, among more prefixes listed more. By its counts it is
/// more; and throttling that grew is among the findings.
#[test]
fn a_finding_that_grew_is_more_never_less() {
    let lists = |out: &mut Vec<Record>, prefix: &str, n: usize| {
        for _ in 0..n {
            out.push(list(out.len() as u64 + 1, prefix));
        }
    };
    let throttled = |out: &mut Vec<Record>, n: usize| {
        for _ in 0..n {
            let mut r = get(out.len() as u64 + 1, "busy");
            r.status = Some(503);
            r.error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
            out.push(r);
        }
    };
    let mut ra: Vec<Record> = Vec::new();
    lists(&mut ra, "logs/", 92);
    throttled(&mut ra, 64);
    let mut rb: Vec<Record> = Vec::new();
    lists(&mut rb, "logs/", 216);
    for p in ["alpha/", "beta/", "gamma/"] {
        lists(&mut rb, p, 300);
    }
    throttled(&mut rb, 145);
    let a = run_of(&ra);
    let mut b = run_of(&rb);
    b.measure(&rb, &a);
    // the run after shows three prefixes: logs/ is not among them
    assert!(
        !b.observations
            .iter()
            .any(|o| o.identity == "obs.listed|b/logs/")
    );
    let c = compare_runs(&a, &b);
    let logs = c
        .findings
        .iter()
        .find(|f| f.identity == "obs.listed|b/logs/")
        .expect("logs/");
    assert_eq!(logs.state, State::More, "{logs:#?}");
    assert_eq!(logs.counts, Some([92, 216]));
    assert!(logs.saved.is_none());
    let slow = c
        .findings
        .iter()
        .find(|f| f.identity.starts_with("obs.throttled"))
        .expect("throttling");
    assert_eq!((slow.state, slow.counts), (State::More, Some([64, 145])));
    // fewer of them is less
    let c = compare_runs(&b, &a);
    let slow = c
        .findings
        .iter()
        .find(|f| f.identity.starts_with("obs.throttled"))
        .unwrap();
    assert_eq!(slow.state, State::Less);
}

/// Every kind of failure is compared: one that moved is a finding of the
/// change, new, more, less or fixed.
#[test]
fn each_kind_of_failure_is_compared() {
    let failing = |status: u16, code: &str, n: usize, from: u64| -> Vec<Record> {
        (0..n as u64)
            .map(|i| Record {
                status: Some(status),
                error_body: Some(format!("<Error><Code>{code}</Code></Error>")),
                ..get(from + i, "x")
            })
            .collect()
    };
    let mut ra = failing(500, "InternalError", 3, 1);
    ra.extend(failing(403, "AccessDenied", 2, 10));
    let mut rb = failing(500, "InternalError", 7, 1);
    rb.extend(failing(400, "InvalidArgument", 1, 10));
    let c = compare_runs(&run_of(&ra), &run_of(&rb));
    let of = |id: &str| {
        c.findings
            .iter()
            .find(|f| f.identity == id)
            .unwrap_or_else(|| panic!("{id} in {:#?}", c.findings))
    };
    let internal = of("obs.errors|500 InternalError");
    assert_eq!(internal.state, State::More);
    assert_eq!(internal.text, "7 requests failed with 500 InternalError");
    assert_eq!(internal.counts, Some([3, 7]));
    assert_eq!(of("obs.errors|403 AccessDenied").state, State::Fixed);
    assert_eq!(of("obs.errors|400 InvalidArgument").state, State::New);
    // the same failures: nothing to say
    let same = compare_runs(&run_of(&ra), &run_of(&ra));
    assert!(
        !same
            .findings
            .iter()
            .any(|f| f.identity.starts_with("obs.errors"))
    );
}

/// 19.0 kept 3 of 6 runs' findings when six ran at once: a run that ends
/// after a later one is taken in, and changes nothing the later one said.
#[test]
fn runs_that_end_out_of_order_keep_all_their_findings() {
    let dir = tempfile::tempdir().unwrap();
    let words = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"];
    let runs: Vec<(i64, Vec<Record>)> = words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let (path, header) = job(dir.path(), i as i64 + 1);
            let rs: Vec<Record> = (1..=12).map(|n| get(n, w)).collect();
            write_session(&path, &header, &rs);
            sonde_proxy::index::set(&path, sonde_proxy::index::Change::Exit(0));
            (i as i64 + 1, rs)
        })
        .collect();
    // they end at once, the newest first
    std::thread::scope(|s| {
        for (minute, rs) in runs.iter().rev() {
            let d = dir.path();
            s.spawn(move || shown(d, *minute, rs));
        }
    });
    let kept = fixloop::read(dir.path()).unwrap();
    for w in words {
        let id = format!("obs.hot|b/{w}");
        assert!(
            kept["job"].findings.contains_key(&id),
            "{id} in {:?}",
            kept["job"].findings.keys()
        );
    }
    assert_eq!(
        kept["job"].last.as_ref().map(|(_, run)| run.as_str()),
        Some("run-06.sonde")
    );
}

/// Findings kept under 19.0's rules are made again by these: kept, not
/// read as one fixed and another new; ones told apart by rank let go.
#[test]
fn findings_kept_under_older_rules_are_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let tracked = |text: &str| {
        serde_json::json!({
            "kind": "obs.hot", "text": text, "state": "fixed", "since": "run-01.sonde",
            "predicted": {"requests": 11, "ms": 0.0}, "fixedIn": ["2023-11-14T22:13:20+00:00"]
        })
    };
    let old = serde_json::json!({
        "job": {
            "last": ["2023-11-14T22:13:20+00:00", "run-01.sonde"],
            "findings": {
                "obs.hot|b/day={date}/part-00001": tracked("a"),
                "obs.hot|b/t/{n}/x": tracked("b"),
                "obs.hot|b/t/{n}/x#2": tracked("c"),
            }
        }
    });
    std::fs::write(
        dir.path().join("findings.json"),
        serde_json::to_vec(&old).unwrap(),
    )
    .unwrap();
    let kept = fixloop::read(dir.path()).unwrap();
    let job = &kept["job"];
    assert_eq!(job.rules, sonde_proxy::profile::IDENTITY_RULES);
    let ids: Vec<&String> = job.findings.keys().collect();
    assert_eq!(ids, ["obs.hot|b/day={v}/part-{n}"]);
    // back, as it was fixed before: not new
    let mut c = vec![fixloop::FindingChange {
        identity: "obs.hot|b/day={v}/part-{n}".into(),
        state: State::New,
        level: "info",
        text: String::new(),
        phrases: Vec::new(),
        was: None,
        counts: None,
        predicted: Default::default(),
        measured: None,
        saved: None,
        saved_text: None,
    }];
    job.mark_back(&mut c, "2030-01-01T00:00:00+00:00");
    assert_eq!(c[0].state, State::Back);
}
