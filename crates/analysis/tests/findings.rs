//! Findings added in 5.0, and how findings are ranked.

use chrono::DateTime;
use reqlog::Record;
use sonde_analysis::episode::{Observation, episodes, observations};

fn rec(id: u64, at_ms: i64, op: &str, key: Option<&str>, status: u16, latency: f64) -> Record {
    let method = match op {
        "HeadObject" => "HEAD",
        "PutObject" => "PUT",
        _ => "GET",
    };
    Record {
        id,
        time: DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
        method: method.into(),
        kind: op.into(),
        bucket: Some("b".into()),
        key: key.map(Into::into),
        status: Some(status),
        latency_ms: latency,
        request_bytes: Some(0).into(),
        response_bytes: Some(10).into(),
        ..Default::default()
    }
}

fn findings(recs: &[Record]) -> Vec<Observation> {
    observations(recs, &episodes(recs))
}

fn key(o: &Observation) -> &str {
    o.phrases[0].key
}

#[test]
fn a_head_before_each_get_is_a_request_too_many() {
    let mut recs = Vec::new();
    for i in 0..4u64 {
        let k = format!("data/{i}.parquet");
        recs.push(rec(
            i * 2 + 1,
            i as i64 * 1000,
            "HeadObject",
            Some(&k),
            200,
            5.0,
        ));
        recs.push(rec(
            i * 2 + 2,
            i as i64 * 1000 + 10,
            "GetObject",
            Some(&k),
            200,
            5.0,
        ));
    }
    let obs = findings(&recs);
    let f = obs
        .iter()
        .find(|o| key(o) == "obs.headget")
        .expect("HEAD then GET");
    assert_eq!(
        f.text,
        "4 objects under b/data/ were checked with HEAD and then read with GET: the GET alone answers both"
    );
    assert_eq!(
        f.saving.as_ref().unwrap().en(),
        "4 fewer requests (50% of the run)"
    );
}

#[test]
fn throttling_under_load_says_how_many_at_once_was_too_many() {
    // eight reads at once, two answered 503
    let recs: Vec<Record> = (0..8u64)
        .map(|i| {
            rec(
                i + 1,
                i as i64,
                "GetObject",
                Some(&format!("k{i}")),
                if i >= 6 { 503 } else { 200 },
                200.0,
            )
        })
        .collect();
    let obs = findings(&recs);
    let f = obs
        .iter()
        .find(|o| key(o) == "obs.throttled")
        .expect("throttled");
    assert_eq!(f.level, "warn");
    assert!(
        f.text.starts_with(
            "2 requests throttled (503 SlowDown or 429), each with 7 or more in flight"
        ),
        "{}",
        f.text
    );
}

#[test]
fn throttling_without_load_is_about_the_rate() {
    let recs = [
        rec(1, 0, "GetObject", Some("k"), 503, 5.0),
        rec(2, 500, "GetObject", Some("k"), 200, 5.0),
    ];
    let obs = findings(&recs);
    assert!(
        obs.iter().any(|o| key(o) == "obs.throttled.rate"),
        "{obs:#?}"
    );
}

#[test]
fn a_request_far_slower_than_the_rest_is_no_finding() {
    // every run has a tail: the latency charts show it (18.0)
    let mut recs: Vec<Record> = (0..20u64)
        .map(|i| rec(i + 1, i as i64 * 100, "GetObject", Some("k"), 200, 8.0))
        .collect();
    recs.push(rec(21, 5_000, "GetObject", Some("big.bin"), 200, 3_250.0));
    assert!(findings(&recs).iter().all(|o| key(o) == "obs.hot"));
}

#[test]
fn warnings_come_first_then_the_findings_that_bear_on_more_requests() {
    let mut recs = Vec::new();
    let mut id = 0;
    let mut push = |r: fn(u64) -> Record| {
        id += 1;
        recs.push(r(id));
    };
    // a key read 12 times, a listing repeated 5 times, one throttled request
    for _ in 0..12 {
        push(|id| rec(id, id as i64 * 50, "GetObject", Some("hot"), 200, 5.0));
    }
    for _ in 0..5 {
        push(|id| rec(id, id as i64 * 50, "ListObjectsV2", None, 200, 5.0));
    }
    push(|id| rec(id, id as i64 * 50, "PutObject", Some("x"), 503, 5.0));
    let obs = findings(&recs);
    let order: Vec<&str> = obs.iter().map(key).collect();
    assert_eq!(order.first(), Some(&"obs.throttled.rate"), "{order:?}");
    let hot = order.iter().position(|k| *k == "obs.hot").unwrap();
    let listed = order.iter().position(|k| *k == "obs.listed").unwrap();
    assert!(hot < listed, "{order:?}");
    for w in obs.windows(2) {
        assert!(w[0].level == "warn" || w[1].level != "warn");
    }
}

#[test]
fn a_finding_whose_details_change_is_the_same_finding() {
    use sonde_analysis::profile::{RequestPrices, Run, compare_runs, profile};
    let run = |slow: &str| {
        let mut recs: Vec<Record> = (0..20u64)
            .map(|i| rec(i + 1, i as i64 * 100, "GetObject", Some("k"), 200, 8.0))
            .collect();
        recs.push(rec(21, 5_000, "GetObject", Some(slow), 200, 3_000.0));
        recs
    };
    let (a, b) = (run("x"), run("y"));
    let prices = RequestPrices::default();
    let (pa, pb) = (profile(&a, &prices), profile(&b, &prices));
    let (oa, ob) = (findings(&a), findings(&b));
    let c = compare_runs(&Run::of(&a, pa, oa), &Run::of(&b, pb, ob));
    assert!(c.resolved.is_empty() && c.appeared.is_empty(), "{c:?}");
}

#[test]
fn concurrent_uploads_to_one_key_each_get_their_own_create() {
    let q = |mut r: Record, kv: &[(&str, &str)]| {
        r.lists_mut().query = kv.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect();
        r
    };
    let mut recs = Vec::new();
    // create A, create B, then each upload's one part and its completion
    recs.push(q(
        rec(1, 0, "CreateMultipartUpload", Some("k"), 200, 5.0),
        &[("uploads", "")],
    ));
    recs.push(q(
        rec(2, 1, "CreateMultipartUpload", Some("k"), 200, 5.0),
        &[("uploads", "")],
    ));
    for (i, u) in ["a", "b", "c"].iter().enumerate() {
        let base = 10 + i as u64 * 10;
        if i == 2 {
            recs.push(q(
                rec(
                    base,
                    base as i64 - 1,
                    "CreateMultipartUpload",
                    Some("k"),
                    200,
                    5.0,
                ),
                &[("uploads", "")],
            ));
        }
        recs.push(q(
            rec(base + 1, base as i64, "UploadPart", Some("k"), 200, 5.0),
            &[("partNumber", "1"), ("uploadId", u)],
        ));
        recs.push(q(
            rec(
                base + 2,
                base as i64 + 1,
                "CompleteMultipartUpload",
                Some("k"),
                200,
                5.0,
            ),
            &[("uploadId", u)],
        ));
    }
    let obs = findings(&recs);
    let small = obs
        .iter()
        .find(|o| key(o) == "obs.small.parts")
        .expect("small uploads");
    // three uploads of three requests each: every create counted once
    assert_eq!(small.impact, 9, "{small:#?}");
}

#[test]
fn a_prefix_near_s3s_rate_per_prefix_is_named() {
    // 3,000 reads of one folder in a second: 54% of 5,500, no finding of its
    // rate (of so many small objects read one GET each, `obs.smallreads` alone)
    let reads = |n: u64| -> Vec<Record> {
        (0..n)
            .map(|i| {
                rec(
                    i + 1,
                    (i * 1000 / n) as i64,
                    "GetObject",
                    Some(&format!("logs/2026/{i}.json")),
                    200,
                    2.0,
                )
            })
            .collect()
    };
    let quiet = findings(&reads(3000));
    let kinds: Vec<&str> = quiet.iter().map(key).collect();
    assert_eq!(kinds, ["obs.smallreads"]);
    // 4,500: 81%, a warning
    let obs = findings(&reads(4500));
    let f = obs
        .iter()
        .find(|o| key(o) == "obs.prefixrate.reads")
        .expect("a hot prefix");
    assert_eq!(f.level, "warn");
    assert!(
        f.text
            .starts_with("b/logs/2026/ took up to 4,500 reads a second, 81% of the 5500"),
        "{}",
        f.text
    );
    let recs: Vec<Record> = (0..3000u64)
        .map(|i| {
            rec(
                i + 1,
                (i / 3) as i64,
                "GetObject",
                Some(&format!("logs/2026/{i}.json")),
                200,
                2.0,
            )
        })
        .collect();
    let p = sonde_analysis::profile::profile(&recs, &Default::default());
    assert_eq!(p.prefixes[0].prefix, "b/logs/2026/");
    assert_eq!(p.prefixes[0].peak_reads, 3000);
    assert!(
        p.flows
            .iter()
            .any(|f| f.source == "GetObject" && f.target == "b/logs/2026/" && f.value == 3000)
    );
    assert!(
        p.flows
            .iter()
            .any(|f| f.source == "b/logs/2026/" && f.target == "ok")
    );
    assert_eq!(p.heat.iter().map(|c| c.n).sum::<usize>(), 3000);
}

#[test]
fn the_prefix_nearest_its_limit_is_named_first() {
    // three read prefixes at 54% of 5,500, one write prefix at 97% of 3,500
    let mut recs = Vec::new();
    let mut id = 0;
    for p in ["a", "b", "c"] {
        for i in 0..3000u64 {
            id += 1;
            recs.push(rec(
                id,
                (i / 3) as i64,
                "GetObject",
                Some(&format!("{p}/{i}")),
                200,
                1.0,
            ));
        }
    }
    for i in 0..3400u64 {
        id += 1;
        recs.push(rec(
            id,
            (i * 1000 / 3400) as i64,
            "PutObject",
            Some(&format!("w/{i}")),
            200,
            1.0,
        ));
    }
    let obs = findings(&recs);
    let hot: Vec<&Observation> = obs
        .iter()
        .filter(|o| key(o).starts_with("obs.prefixrate"))
        .collect();
    assert_eq!(
        key(hot[0]),
        "obs.prefixrate.writes",
        "{:#?}",
        hot.iter().map(|o| &o.text).collect::<Vec<_>>()
    );
    assert_eq!(hot[0].level, "warn");
}

#[test]
fn requests_one_after_another_are_a_chain_that_requests_at_once_would_shorten() {
    // 40 reads, each 20 ms, each starting as the one before ends
    let recs: Vec<Record> = (0..40u64)
        .map(|i| {
            rec(
                i + 1,
                i as i64 * 21,
                "GetObject",
                Some(&format!("k{i}")),
                200,
                20.0,
            )
        })
        .collect();
    let p = sonde_analysis::profile::profile(&recs, &Default::default());
    assert_eq!(p.chains.len(), 1);
    assert_eq!(p.chains[0].requests, 40);
    assert!(
        (p.chains[0].at_once_ms - 100.0).abs() < 1e-6,
        "{:?}",
        p.chains[0]
    );
    let obs = findings(&recs);
    let f = obs
        .iter()
        .find(|o| key(o) == "obs.serial")
        .expect("a chain");
    assert_eq!(
        f.text,
        "40 requests ran one after another for 839 ms, each waiting for the one before: 8 at once would take about 100 ms"
    );
    // 800 ms waited, 100 ms at once (said as time of the run once ranked)
    assert!((f.saves_ms - 700.0).abs() < 1e-6, "{}", f.saves_ms);
    assert!(f.fixes.iter().any(|x| x.code.contains("get_object")));
    // the same reads 8 at a time make no chain
    let together: Vec<Record> = (0..40u64)
        .map(|i| {
            rec(
                i + 1,
                (i / 8) as i64 * 21,
                "GetObject",
                Some(&format!("k{i}")),
                200,
                20.0,
            )
        })
        .collect();
    assert!(
        sonde_analysis::profile::profile(&together, &Default::default())
            .chains
            .is_empty()
    );
}

#[test]
fn the_pages_of_one_listing_are_not_the_prefix_listed_again() {
    // ListObjects (v1) pages with a marker, as ListObjectsV2's with a token
    let page = |id: u64, op: &str, param: Option<(&str, &str)>| {
        let mut r = rec(id, id as i64 * 10, op, None, 200, 5.0);
        r.lists_mut().query = vec![("prefix".into(), "t/".into())];
        if let Some((k, v)) = param {
            r.lists_mut().query.push((k.into(), v.into()));
        }
        r
    };
    let mut recs = vec![page(1, "ListObjects", None)];
    for i in 2..=8u64 {
        recs.push(page(i, "ListObjects", Some(("marker", "t/k"))));
    }
    recs.push(page(9, "ListObjectsV2", None));
    for i in 10..=16u64 {
        recs.push(page(i, "ListObjectsV2", Some(("continuation-token", "x"))));
    }
    let found = findings(&recs);
    assert!(
        !found.iter().any(|o| key(o) == "obs.listed"),
        "{:?}",
        found.iter().map(key).collect::<Vec<_>>()
    );
    // five listings from the start are
    let again: Vec<Record> = (1..=5).map(|i| page(i, "ListObjects", None)).collect();
    assert!(findings(&again).iter().any(|o| key(o) == "obs.listed"));
}

fn put(id: u64, at_ms: i64, key: &str) -> Record {
    rec(id, at_ms, "PutObject", Some(key), 200, 20.0)
}

/// 28.0 told requests one after another of those another finding takes
/// away already (a HEAD before each GET), and of polls; and its fix was a
/// pool of get_object whatever the requests were.
#[test]
fn requests_one_after_another_are_those_left_once_the_rest_is_fixed() {
    // 40 objects, a HEAD then a GET each, one after another
    let mut recs = Vec::new();
    for i in 0..40u64 {
        let k = format!("in/{i}.csv");
        recs.push(rec(
            i * 2 + 1,
            i as i64 * 42,
            "HeadObject",
            Some(&k),
            200,
            20.0,
        ));
        recs.push(rec(
            i * 2 + 2,
            i as i64 * 42 + 21,
            "GetObject",
            Some(&k),
            200,
            20.0,
        ));
    }
    let obs = findings(&recs);
    let serial = obs
        .iter()
        .find(|o| key(o) == "obs.serial")
        .expect("a chain");
    // the 40 GETs: the HEADs go with the GET alone
    assert!(
        serial.text.starts_with("40 requests ran one after another"),
        "{}",
        serial.text
    );
    // a key asked for again and again while not there is a poll, not a chain
    let polls: Vec<Record> = (0..40u64)
        .map(|i| {
            rec(
                i + 1,
                i as i64 * 21,
                "HeadObject",
                Some("out/_READY"),
                404,
                20.0,
            )
        })
        .collect();
    assert!(!findings(&polls).iter().any(|o| key(o) == "obs.serial"));
    // writes one after another: the fix writes at once
    let puts: Vec<Record> = (0..40u64)
        .map(|i| put(i + 1, i as i64 * 21, &format!("big/{i}.bin")))
        .collect();
    let obs = findings(&puts);
    let serial = obs
        .iter()
        .find(|o| key(o) == "obs.serial")
        .expect("a chain");
    let py = serial.fixes.iter().find(|f| f.sdk == "python").unwrap();
    assert!(
        py.code.contains("put_object") && !py.code.contains("get_object"),
        "{}",
        py.code
    );
}

/// One root cause, one diagnosis: 600 tiny objects written one each, then
/// each checked with HEAD and read with GET, one after another (the eval's
/// four findings of 28.0).
#[test]
fn findings_of_one_root_cause_are_one_diagnosis_saving_what_all_at_once_saves() {
    use sonde_analysis::run::RunHeader;
    use sonde_analysis::view::Analysis;
    let mut recs = Vec::new();
    let mut id = 0u64;
    let mut at = 0i64;
    for i in 0..600 {
        id += 1;
        recs.push(put(id, at, &format!("tiny/{i:04}.json")));
        at += 21;
    }
    for i in 0..600 {
        let k = format!("tiny/{i:04}.json");
        id += 1;
        recs.push(rec(id, at, "HeadObject", Some(&k), 200, 20.0));
        at += 21;
        id += 1;
        recs.push(rec(id, at, "GetObject", Some(&k), 200, 20.0));
        at += 21;
    }
    // and a config read again and again: a cause of its own
    for _ in 0..30 {
        id += 1;
        recs.push(rec(id, at, "GetObject", Some("conf.json"), 200, 2.0));
        at += 30;
    }
    let header = RunHeader::new(
        "aws",
        "https://s3",
        "us-east-1",
        Some("python3 job.py".into()),
    );
    let a = Analysis::of_records(&header, &recs, &Default::default());
    let kinds = |d: &sonde_analysis::fixes::Diagnosis| -> Vec<&str> {
        d.findings
            .iter()
            .map(|&i| a.observations[i].kind.as_str())
            .collect()
    };
    let first = &a.diagnoses[0];
    let mut members = kinds(first);
    members.sort_unstable();
    assert_eq!(
        members,
        ["obs.headget", "obs.small", "obs.smallreads"],
        "{:#?}",
        a.diagnoses
    );
    // the members keep their identities; the id is made of them
    assert_eq!(first.identities.len(), 3);
    assert!(first.id.starts_with("dx-"));
    // one saving, of them all at once: never more than the waiting, never their sum
    let sum: f64 = first
        .findings
        .iter()
        .map(|&i| a.observations[i].saves_ms)
        .sum();
    assert!(first.saves_ms > 0.0 && first.saves_ms <= a.profile.waiting_ms + 1e-6);
    assert!(first.saves_ms <= sum + 1e-6, "{} of {sum}", first.saves_ms);
    assert!(
        first.saving_text.starts_with('≈') && first.saving_text.contains(" of "),
        "{}",
        first.saving_text
    );
    // records, packed as lines: no tar shard beside a JSONL batch
    let head = &a.observations[first.findings[0]];
    assert!(head.fixes.iter().all(|f| !f.code.contains("tarfile")));
    // the config read is another diagnosis, after: it saves less time
    assert!(
        a.diagnoses[1..].iter().any(|d| kinds(d) == ["obs.hot"]),
        "{:#?}",
        a.diagnoses
    );
    // the same findings, the same id
    let again = Analysis::of_records(&header, &recs, &Default::default());
    assert_eq!(again.diagnoses[0].id, first.id);
}

/// 28.0 ranked money first: a poll saving a millionth of a dollar above
/// requests one after another saving most of the run.
#[test]
fn diagnoses_are_ranked_by_the_time_they_save() {
    use sonde_analysis::run::RunHeader;
    use sonde_analysis::view::Analysis;
    let mut recs: Vec<Record> = (0..60u64)
        .map(|i| {
            rec(
                i + 1,
                i as i64 * 101,
                "GetObject",
                Some(&format!("data/{i}")),
                200,
                100.0,
            )
        })
        .collect();
    // 200 small writes, quick: cheap in time, dear in money
    for i in 0..200u64 {
        recs.push(put(61 + i, 7000 + i as i64 * 3, &format!("out/{i}.json")));
        recs.last_mut().unwrap().latency_ms = 1.0;
    }
    let header = RunHeader::new(
        "aws",
        "https://s3",
        "us-east-1",
        Some("python3 job.py".into()),
    );
    let a = Analysis::of_records(&header, &recs, &Default::default());
    let head = |d: &sonde_analysis::fixes::Diagnosis| a.observations[d.findings[0]].kind.as_str();
    let order: Vec<&str> = a.diagnoses.iter().map(head).collect();
    assert_eq!(order.first(), Some(&"obs.serial"), "{order:?}");
    assert!(order.contains(&"obs.small"), "{order:?}");
    // the money of the writes is under a cent: not said
    let small = a.diagnoses.iter().find(|d| head(d) == "obs.small").unwrap();
    assert!(!small.saving_text.contains("USD"), "{}", small.saving_text);
}

/// The time account: the program's own time, the round trips, and on the
/// demo store, times said as indicative.
#[test]
fn a_run_says_where_its_time_went() {
    use sonde_analysis::run::RunHeader;
    use sonde_analysis::view::Analysis;
    // 10 requests of 10 ms, 100 ms apart: 100 ms of 910 waiting
    let recs: Vec<Record> = (0..10u64)
        .map(|i| {
            rec(
                i + 1,
                i as i64 * 100,
                "GetObject",
                Some(&format!("k{i}")),
                200,
                10.0,
            )
        })
        .collect();
    let demo = RunHeader::new("demo", "http://demo", "", Some("python3 job.py".into()));
    let t = Analysis::of_records(&demo, &recs, &Default::default())
        .time
        .unwrap();
    assert_eq!((t.wall_ms, t.waiting_ms, t.most), (910.0, 100.0, "program"));
    assert_eq!(
        t.text,
        "time 910 ms: your program's own 810 ms (89%) · S3 round trips 100 ms (10 requests, median 10 ms) · most of the time is your program's own, not S3's (the demo store's times are indicative)"
    );
    // a log's times, to the second: none said
    let mut log = RunHeader::new("aws", "", "", Some("imported".into()));
    log.whole_seconds = true;
    assert!(
        Analysis::of_records(&log, &recs, &Default::default())
            .time
            .is_none()
    );
}
