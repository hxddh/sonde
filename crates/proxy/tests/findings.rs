//! Findings added in 5.0, and how findings are ranked.

use chrono::DateTime;
use reqlog::Record;
use sonde_proxy::episode::{Observation, episodes, observations};

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
        operation: op.into(),
        bucket: Some("b".into()),
        key: key.map(Into::into),
        status: Some(status),
        latency_ms: latency,
        request_bytes: Some(0),
        response_bytes: Some(10),
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
        "4 objects were checked with HEAD and then read with GET: the GET alone answers both"
    );
    assert_eq!(
        f.saving.as_ref().unwrap().en(),
        "4 fewer requests (50% of the session)"
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
    use sonde_proxy::profile::{RequestPrices, Run, compare_runs, profile};
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
        r.query = kv.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect();
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
        .find(|o| key(o) == "obs.small")
        .expect("small uploads");
    // three uploads of three requests each: every create counted once
    assert_eq!(small.impact, 9, "{small:#?}");
}

#[test]
fn a_prefix_near_s3s_rate_per_prefix_is_named() {
    // 3,000 reads of one folder in a second: 54% of 5,500, no finding
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
    assert!(findings(&reads(3000)).is_empty());
    // 4,500: 81%, a warning
    let obs = findings(&reads(4500));
    let f = obs
        .iter()
        .find(|o| key(o) == "obs.prefixrate.reads")
        .expect("a hot prefix");
    assert_eq!(f.level, "warn");
    assert!(
        f.text
            .starts_with("b/logs/2026/ took up to 4500 reads a second, 81% of the 5500"),
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
    let p = sonde_proxy::profile::profile(&recs, &Default::default());
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
    let p = sonde_proxy::profile::profile(&recs, &Default::default());
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
    assert!(
        f.saving_text
            .as_deref()
            .unwrap()
            .starts_with("about 700 ms sooner"),
        "{:?}",
        f.saving_text
    );
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
        sonde_proxy::profile::profile(&together, &Default::default())
            .chains
            .is_empty()
    );
}

#[test]
fn the_pages_of_one_listing_are_not_the_prefix_listed_again() {
    // ListObjects (v1) pages with a marker, as ListObjectsV2's with a token
    let page = |id: u64, op: &str, param: Option<(&str, &str)>| {
        let mut r = rec(id, id as i64 * 10, op, None, 200, 5.0);
        r.query = vec![("prefix".into(), "t/".into())];
        if let Some((k, v)) = param {
            r.query.push((k.into(), v.into()));
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
