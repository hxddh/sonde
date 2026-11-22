//! The plan's numbers (30.0): the time a change saves on a run of requests at
//! once, requests sent at once weighed on their own, what is wrong before
//! what is slow, and a run's time said as whose it was.

use chrono::DateTime;
use reqlog::{Header, Record};
use sonde_analysis::profile::RequestPrices;
use sonde_analysis::run::RunHeader;
use sonde_analysis::view::Analysis;

/// A request started `at_us` after the run began, taking `ms`.
fn rec(id: u64, at_us: i64, ms: f64, op: &str, key: &str) -> Record {
    let method = match op {
        "PutObject" => "PUT",
        "HeadObject" => "HEAD",
        _ => "GET",
    };
    Record {
        id,
        time: DateTime::from_timestamp_micros(1_700_000_000_000_000 + at_us).unwrap(),
        method: method.into(),
        kind: op.into(),
        bucket: Some("lake".into()),
        key: Some(key.into()),
        status: Some(200),
        latency_ms: ms,
        request_bytes: Some(if method == "PUT" { 600 } else { 0 }).into(),
        response_bytes: Some(if method == "PUT" { 0 } else { 2000 }).into(),
        ..Default::default()
    }
}

fn analysed_as(command: Option<&str>, rs: &[Record]) -> Analysis {
    let h = RunHeader::new("e", "http://e", "us-east-1", command.map(String::from));
    Analysis::of_records(&h, rs, &RequestPrices::default())
}

fn analysed(rs: &[Record]) -> Analysis {
    analysed_as(Some("python3 job.py"), rs)
}

/// The head finding's kind of each diagnosis, in order.
fn heads(a: &Analysis) -> Vec<&str> {
    a.diagnoses
        .iter()
        .map(|d| a.observations[d.findings[0]].kind.as_str())
        .collect()
}

/// Requests one after another, 2 ms of the program's own between them.
struct Serial(Vec<Record>, i64);

impl Serial {
    fn add(&mut self, op: &str, key: &str) -> &mut Record {
        let id = self.0.len() as u64 + 1;
        self.0.push(rec(id, self.1, 30.0, op, key));
        self.1 += 32_000;
        self.0.last_mut().unwrap()
    }
}

#[test]
fn a_change_on_a_run_of_requests_at_once_saves_their_part_of_its_waiting() {
    // eight at once, a pool kept full: 1,000 reads in waves of 10 ms, one
    // read in eleven of the same key. 29.0 took them out and found the
    // waiting as long (others still in flight): it saved "nothing"
    let rs: Vec<Record> = (0..1000u64)
        .map(|i| {
            let key = match i % 11 {
                0 => "conf/settings.json".to_string(),
                _ => format!("in/part-{i}.parquet"),
            };
            rec(i + 1, (i / 8) as i64 * 10_000, 10.0, "GetObject", &key)
        })
        .collect();
    let a = analysed(&rs);
    let hot = a
        .observations
        .iter()
        .find(|o| o.kind == "obs.hot")
        .unwrap_or_else(|| panic!("{:#?}", a.observations));
    assert_eq!(hot.saves_requests, 90);
    // each read is an eighth of its 10 ms of the waiting: 90 of them, 112.5 ms
    assert!(
        (hot.saves_ms - 112.5).abs() < 1.0,
        "{} of {}",
        hot.saves_ms,
        a.profile.waiting_ms
    );
}

#[test]
fn requests_sent_at_once_are_weighed_on_their_own_and_first_when_they_save_most() {
    // an ETL: each object read, summed, its sum written to an object of its
    // own, one after another. Writing fewer objects saves the writes' time;
    // sending them at once saves most of all of it (29.0 counted only the
    // reads, those no other finding took, and put the format change first)
    let mut job = Serial(Vec::new(), 0);
    for i in 0..120 {
        job.add("GetObject", &format!("events/dt=1/{i:03}.json"));
        job.add("PutObject", &format!("out/sums/dt=1/{i:03}.json.sum"));
    }
    let a = analysed(&job.0);
    assert_eq!(
        heads(&a)[..2],
        ["obs.serial", "obs.small"],
        "{:#?}",
        a.diagnoses
    );
    let (first, second) = (&a.diagnoses[0], &a.diagnoses[1]);
    // 240 requests of 30 ms, eight at once: 7.2 s → 30 waves, 0.9 s
    let at_once = first
        .at_once
        .as_ref()
        .expect("its requests at once, counted");
    assert_eq!((at_once.requests, at_once.concurrency), (240, 8));
    assert!((at_once.saves_ms - 6300.0).abs() < 1.0, "{at_once:?}");
    assert_eq!(first.saves_ms, at_once.saves_ms);
    assert!(second.saves_ms < first.saves_ms, "{:#?}", a.diagnoses);
    // both at once: never their sum; the writes gone, the reads at once
    let w = a.what_if.as_ref().unwrap();
    let (before, after) = w.waiting_ms.unwrap();
    assert!(before - after < first.saves_ms + second.saves_ms, "{w:?}");
    assert!(before - after > first.saves_ms, "{w:?}");
}

#[test]
fn a_key_each_time_round_a_loop_holds_back_its_own_requests_not_the_loop_s() {
    // each item read, then a manifest read and written back on condition:
    // the manifest's requests need the one before; the item reads do not
    // (29.0 ended the run of requests at each manifest read: none was said)
    let mut job = Serial(Vec::new(), 0);
    for i in 0..30 {
        job.add("GetObject", &format!("kv/item-{i}"));
        job.add("GetObject", "kv/manifest.json");
        job.add("PutObject", "kv/manifest.json")
            .lists_mut()
            .request_headers = vec![Header::new("If-Match", "\"e1\"")];
    }
    let a = analysed(&job.0);
    let serial = a
        .observations
        .iter()
        .find(|o| o.kind == "obs.serial")
        .unwrap_or_else(|| panic!("{:#?}", a.observations));
    // the 30 item reads and the manifest's first read go at once
    assert_eq!(serial.impact, 31, "{}", serial.text);
}

#[test]
fn what_is_wrong_comes_before_what_is_slow() {
    // a lost update (a read written back with no condition) and a key read
    // again and again: the lost update is the next step, though it saves no time
    let mut job = Serial(Vec::new(), 0);
    for _ in 0..5 {
        job.add("GetObject", "job/config.json");
        job.add("PutObject", "job/config.json");
    }
    for i in 0..60 {
        job.add("GetObject", "conf/settings.json");
        job.add("GetObject", &format!("in/{i}"));
    }
    let a = analysed(&job.0);
    let kinds = heads(&a);
    assert_eq!(kinds[0], "obs.rmw", "{kinds:?}");
    assert!(kinds.contains(&"obs.hot"), "{kinds:?}");
    // the findings in the diagnoses' order
    let placed: Vec<usize> = a
        .diagnoses
        .iter()
        .flat_map(|d| d.findings.clone())
        .collect();
    assert_eq!(placed, (0..a.observations.len()).collect::<Vec<_>>());
}

#[test]
fn a_run_s_time_is_said_as_whose_it_was() {
    let mut job = Serial(Vec::new(), 0);
    for i in 0..20 {
        job.add("GetObject", &format!("in/{i}"));
    }
    // a program's run: its own time between requests
    let t = analysed(&job.0).time.expect("a program's time");
    assert!(t.own_ms > 0.0 && t.injected_ms == 0.0, "{t:?}");
    // an endpoint that stood waiting (`run --listen`), a log source's
    // import: what was idle is no program's own, and is not said so
    assert!(analysed_as(None, &job.0).time.is_none());
    assert!(
        analysed_as(Some("logs of s3://b/logs/"), &job.0)
            .time
            .is_none()
    );
    // a trial: the time sonde held a request is sonde's, not S3's
    for r in job.0.iter_mut().take(5) {
        r.latency_ms += 200.0;
        r.rare_mut().injected = Some("slowdown".into());
    }
    // (later requests start later: the run as the program saw it)
    let mut at = 0;
    for r in job.0.iter_mut() {
        r.time = DateTime::from_timestamp_micros(1_700_000_000_000_000 + at).unwrap();
        at += (r.latency_ms * 1000.0) as i64 + 2_000;
    }
    let t = analysed(&job.0).time.unwrap();
    assert!((t.injected_ms - 5.0 * 230.0).abs() < 1.0, "{t:?}");
    assert!((t.waiting_ms - 15.0 * 30.0).abs() < 1.0, "{t:?}");
    assert!(t.text.contains("held or answered by sonde"), "{}", t.text);
}

#[test]
fn a_poll_saves_requests_and_money_not_time() {
    // a loop that asks for a marker every 200 ms until it is there: a backoff
    // asks less often over the same time, the program waits as long (29.0
    // said "≈636 ms" of a poll that already slept between tries)
    let mut rs: Vec<Record> = (0..25u64)
        .map(|i| {
            let mut r = rec(
                i + 1,
                i as i64 * 230_000,
                30.0,
                "HeadObject",
                "jobs/run-17/_SUCCESS",
            );
            r.status = Some(404);
            r
        })
        .collect();
    rs.push(rec(
        26,
        25 * 230_000,
        30.0,
        "GetObject",
        "jobs/run-17/out.json",
    ));
    let a = analysed(&rs);
    let polled = a
        .observations
        .iter()
        .find(|o| o.kind == "obs.polled")
        .unwrap_or_else(|| panic!("{:#?}", a.observations));
    assert!(
        polled.saves_requests > 0 && polled.saves_cost > 0.0,
        "{polled:?}"
    );
    assert_eq!(polled.saves_ms, 0.0);
    let d = &a.diagnoses[heads(&a).iter().position(|k| *k == "obs.polled").unwrap()];
    assert_eq!(d.saves_ms, 0.0);
    let w = a.what_if.as_ref().unwrap();
    assert!(w.requests.1 < w.requests.0, "{w:?}");
    assert_eq!(w.waiting_ms.map(|(a, b)| a - b), Some(0.0), "{w:?}");
}

#[test]
fn the_parts_of_the_waiting_add_up_to_it() {
    // requests overlapping every which way, out of order, one of no time
    let rs: Vec<Record> = (0..500u64)
        .map(|i| {
            let at = ((i * 7919) % 500) as i64 * 1_300;
            rec(
                i + 1,
                at,
                ((i * 31) % 17) as f64,
                "GetObject",
                &format!("k{i}"),
            )
        })
        .collect();
    let parts = sonde_analysis::profile::waits_ms(&rs);
    let sum: f64 = parts.iter().sum();
    let waiting = sonde_analysis::profile::waiting_ms(&rs);
    assert!((sum - waiting).abs() < 1e-6, "{sum} of {waiting}");
    assert!(parts.iter().all(|&p| p >= 0.0));
}
