//! Bytes and money (25.0): a run's cost split into requests, bytes out
//! and what it leaves stored; the findings about reading (a range read
//! again, objects read whole that are read by range too, a storm of small
//! objects, prefixes listed one by one), each with what it saves.

use chrono::DateTime;
use reqlog::{Header, Record};
use sonde_analysis::episode::{Observation, episodes, observations};
use sonde_analysis::profile::{Egress, RequestPrices, profile};

const MIB: u64 = 1 << 20;

fn rec(id: u64, op: &str, key: &str, down: u64) -> Record {
    let method = match op {
        "PutObject" => "PUT",
        "DeleteObject" => "DELETE",
        _ => "GET",
    };
    Record {
        id,
        time: DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 10).unwrap(),
        method: method.into(),
        kind: op.into(),
        bucket: Some("lake".into()),
        key: (!key.is_empty()).then(|| key.into()),
        status: Some(match method {
            "DELETE" => 204,
            _ => 200,
        }),
        latency_ms: 5.0,
        request_bytes: Some(0).into(),
        response_bytes: Some(down).into(),
        ..Default::default()
    }
}

/// A read of `range` (`bytes=…`), answered 206.
fn ranged(id: u64, key: &str, range: &str, down: u64) -> Record {
    let mut r = rec(id, "GetObject", key, down);
    r.status = Some(206);
    r.lists_mut()
        .request_headers
        .push(Header::new("Range", range));
    r
}

fn listing(id: u64, prefix: &str) -> Record {
    let mut r = rec(id, "ListObjectsV2", "", 2000);
    r.lists_mut().query = vec![
        ("list-type".into(), "2".into()),
        ("prefix".into(), prefix.into()),
    ];
    r
}

fn findings(rs: &[Record]) -> Vec<Observation> {
    observations(rs, &episodes(rs))
}

fn find<'a>(obs: &'a [Observation], kind: &str) -> Option<&'a Observation> {
    obs.iter().find(|o| o.phrases[0].key == kind)
}

fn charged() -> RequestPrices {
    RequestPrices {
        egress: Some(true),
        ..RequestPrices::default()
    }
}

#[test]
fn a_runs_cost_is_its_requests_and_its_bytes_out_when_they_leave_the_region() {
    // a GiB read, a GiB written and kept
    let mut put = rec(2, "PutObject", "out/a.bin", 0);
    put.request_bytes = Some(1 << 30).into();
    let rs = [rec(1, "GetObject", "in/a.bin", 1 << 30), put];
    // not known where it was read from: the requests alone, the egress said
    let p = profile(&rs, &RequestPrices::default());
    let c = &p.cost;
    assert_eq!(c.transfer.egress, Egress::Unknown);
    assert_eq!(c.transfer.bytes, 1 << 30);
    assert!((c.transfer.amount - 0.09).abs() < 1e-9, "{c:?}");
    assert!((c.amount - c.requests).abs() < 1e-12);
    assert!((c.requests - (0.0004 + 0.005) / 1000.0).abs() < 1e-12);
    // what it left stored, a month: a GiB at S3 Standard's
    assert_eq!(c.storage.bytes, 1 << 30);
    assert!((c.storage.per_month - 0.023).abs() < 1e-9);
    let line = c.line().unwrap();
    assert!(
        line.contains("egress if read from outside the region"),
        "{line}"
    );
    assert!(
        line.contains("left 1.0 GiB stored ≈ 0.02 USD a month"),
        "{line}"
    );
    // read from outside the region: the bytes are on the bill, the kinds' too
    let p = profile(&rs, &charged());
    assert_eq!(p.cost.transfer.egress, Egress::Charged);
    assert!((p.cost.amount - p.cost.requests - 0.09).abs() < 1e-9);
    let get = p.groups.iter().find(|g| g.kind == "GetObject").unwrap();
    assert!(get.cost > 0.09);
    // each slice's money adds up to the run's
    let sum: f64 = p
        .rate
        .iter()
        .map(|b| b.requests_cost + b.transfer_cost)
        .sum();
    assert!((sum - p.cost.amount).abs() < 1e-9);
    let stored: f64 = p.rate.iter().map(|b| b.storage_cost).sum();
    assert!((stored - 0.023).abs() < 1e-9);
    // read in the region, or a store with no egress fee: free, and not said
    let in_region = RequestPrices {
        egress: Some(false),
        ..RequestPrices::default()
    };
    let r2 = RequestPrices {
        per_gib_out: 0.0,
        egress: Some(true),
        ..RequestPrices::default()
    };
    for prices in [in_region, r2] {
        let c = profile(&rs, &prices).cost;
        assert_eq!(c.transfer.egress, Egress::Free);
        assert_eq!(c.transfer.amount, 0.0);
        assert!(!c.line().unwrap().contains("egress"));
    }
}

#[test]
fn a_versioned_buckets_overwrites_say_what_their_old_versions_cost() {
    let v = |mut r: Record, id: &str| {
        r.lists_mut()
            .response_headers
            .push(Header::new("x-amz-version-id", id));
        r
    };
    let mut rs = Vec::new();
    for i in 0..3u64 {
        let mut put = v(rec(i + 1, "PutObject", "state.json", 0), &format!("v{i}"));
        put.request_bytes = Some(512 * MIB).into();
        rs.push(put);
    }
    let c = profile(&rs, &RequestPrices::default()).cost;
    assert!(c.storage.versioned);
    assert_eq!(
        (c.storage.noncurrent, c.storage.noncurrent_bytes),
        (2, 1 << 30)
    );
    assert!((c.storage.noncurrent_per_month - 0.023).abs() < 1e-9);
    assert!(
        c.line().unwrap().contains(
            "the bucket keeps versions: 2 noncurrent versions of 1.0 GiB ≈ 0.02 USD a month"
        ),
        "{:?}",
        c.line()
    );
    // versions under a cent a month are not worth a line (28.0 said
    // "≈ <0.000001 USD a month")
    let mut rs = Vec::new();
    for i in 0..3u64 {
        let mut put = v(rec(i + 1, "PutObject", "state.json", 0), &format!("v{i}"));
        put.request_bytes = Some(1536).into();
        rs.push(put);
    }
    let c = profile(&rs, &RequestPrices::default()).cost;
    assert_eq!(c.storage.noncurrent, 2);
    assert_eq!(c.line(), None);
}

/// DuckDB and pyarrow read a Parquet file's footer for each query.
#[test]
fn a_footer_read_again_by_each_query_is_a_range_read_again() {
    let mut rs = Vec::new();
    let mut id = 0;
    for _query in 0..4 {
        for f in 0..4 {
            id += 1;
            let key = format!("events/date=2026-11-0{f}/part-0.parquet");
            rs.push(ranged(id, &key, "bytes=1048000-1048575", 576));
        }
    }
    let obs = findings(&rs);
    let o = find(&obs, "obs.rangeagain").expect("a range read again");
    assert_eq!(
        o.text,
        "12 ranges of 4 objects under lake/events/ were read again (6.8 KiB): keep each range once read"
    );
    assert_eq!(o.saves_requests, 12);
    assert_eq!(o.saves_bytes, 12 * 576);
    assert_eq!(o.kind, "obs.rangeagain");
    assert_eq!(o.identity, "obs.rangeagain|lake");
    assert!(!o.fixes.is_empty());
    // one object is one object (28.0: "29 ranges of 1 objects")
    let rs: Vec<Record> = (0..12u64)
        .map(|i| ranged(i + 1, "events/one.parquet", "bytes=0-99", 100))
        .collect();
    let one = findings(&rs);
    let o = find(&one, "obs.rangeagain").expect("a range read again");
    assert!(
        o.text.starts_with("11 ranges of 1 object under"),
        "{}",
        o.text
    );
    // a range read again after the object was written is another object
    let mut rs = Vec::new();
    for i in 0..12u64 {
        rs.push(ranged(i * 2 + 1, "t/f.parquet", "bytes=0-99", 100));
        rs.push(rec(i * 2 + 2, "PutObject", "t/f.parquet", 0));
    }
    assert!(find(&findings(&rs), "obs.rangeagain").is_none());
    // different ranges of one object are a read in parts, not again
    let rs: Vec<Record> = (0..20u64)
        .map(|i| {
            let range = format!("bytes={}-{}", i * 100, i * 100 + 99);
            ranged(i + 1, "t/f.parquet", &range, 100)
        })
        .collect();
    assert!(find(&findings(&rs), "obs.rangeagain").is_none());
}

/// Objects read whole where the program reads them by range too: the
/// range reads are the evidence that parts are what it uses.
#[test]
fn objects_read_whole_and_by_range_are_a_whole_read() {
    let mut rs = Vec::new();
    let mut id = 0;
    for f in 0..4 {
        let key = format!("t/part-{f}.parquet");
        id += 1;
        rs.push(ranged(id, &key, "bytes=-65536", 65536));
        id += 1;
        rs.push(rec(id, "GetObject", &key, 64 * MIB));
    }
    let obs = findings(&rs);
    let o = find(&obs, "obs.wholeread").expect("read whole");
    assert_eq!(
        o.text,
        "4 whole reads of 4 objects under lake/t/ that are also read by range (256 MiB): read only the ranges used"
    );
    assert_eq!(o.saves_bytes, 4 * (64 * MIB - 65536));
    assert_eq!(o.saves_requests, 0);
    assert!(o.saving_text.as_deref().unwrap().contains("less read"));
    // what it saves is egress, when the bytes leave the region
    let a = sonde_analysis::view::Analysis::of_records(&header(), &rs, &charged());
    let o = find(&a.observations, "obs.wholeread").unwrap();
    assert!((o.saves_cost - (o.saves_bytes as f64 / (1u64 << 30) as f64) * 0.09).abs() < 1e-9);
    let w = a.what_if.as_ref().expect("a what-if");
    assert_eq!(w.bytes_out.1, w.bytes_out.0 - o.saves_bytes);
    assert!(w.cost.1 < w.cost.0);
    assert!(w.line(None).contains("read 256 MiB → "), "{}", w.line(None));
    // not known where it was read from: the bytes saved, no money
    let a = sonde_analysis::view::Analysis::of_records(&header(), &rs, &RequestPrices::default());
    assert_eq!(
        find(&a.observations, "obs.wholeread").unwrap().saves_cost,
        0.0
    );
    // large objects read whole with no range read of them: no evidence
    let rs: Vec<Record> = (0..8u64)
        .map(|i| rec(i + 1, "GetObject", &format!("t/{i}.bin"), 64 * MIB))
        .collect();
    assert!(find(&findings(&rs), "obs.wholeread").is_none());
}

/// An ML loader reading a JPEG a GET.
#[test]
fn many_small_objects_each_read_whole_are_a_storm_to_shard() {
    let rs: Vec<Record> = (0..600u64)
        .map(|i| {
            rec(
                i + 1,
                "GetObject",
                &format!("train/img-{i:05}.jpg"),
                110_000,
            )
        })
        .collect();
    let obs = findings(&rs);
    let o = find(&obs, "obs.smallreads").expect("small reads");
    assert!(
        o.text
            .starts_with("600 objects of less than 1 MiB under lake/train/ were read one GET each"),
        "{}",
        o.text
    );
    // 63 MiB fits one shard of 64 MiB: one read left of 600
    assert_eq!(o.saves_requests, 599);
    // images: packed in tar shards (records would be lines: JSONL)
    assert!(o.fixes.iter().any(|f| f.code.contains("tarfile")));
    assert!(o.fixes.iter().all(|f| !f.code.contains("jsonl")));
    // fewer than 500, or large objects: no storm
    assert!(find(&findings(&rs[..400]), "obs.smallreads").is_none());
    let big: Vec<Record> = (0..600u64)
        .map(|i| rec(i + 1, "GetObject", &format!("train/{i}.bin"), 2 * MIB))
        .collect();
    assert!(find(&findings(&big), "obs.smallreads").is_none());
}

/// Sharding small objects sends fewer requests, not fewer bytes: the
/// shards hold every sample, and what they read out is as before.
#[test]
fn sharding_small_reads_saves_requests_and_keeps_their_bytes() {
    let rs: Vec<Record> = (0..600u64)
        .map(|i| {
            rec(
                i + 1,
                "GetObject",
                &format!("train/img-{i:05}.jpg"),
                110_000,
            )
        })
        .collect();
    let prices = charged();
    let a = sonde_analysis::view::Analysis::of_records(&header(), &rs, &prices);
    let o = find(&a.observations, "obs.smallreads").expect("small reads");
    assert_eq!(o.saves_requests, 599);
    assert_eq!(o.saves_bytes, 0);
    let w = a.what_if.as_ref().expect("a what-if");
    assert_eq!(w.requests, (600, 1));
    assert_eq!(w.bytes_out, (600 * 110_000, 600 * 110_000));
    // the requests' price, not their egress
    let each = sonde_analysis::profile::request_price(&rs[0], &prices);
    assert!(
        (o.saves_cost - 599.0 * each).abs() < 1e-9,
        "{} vs {}",
        o.saves_cost,
        599.0 * each
    );
    let egress = prices.out(600 * 110_000);
    assert!(
        (w.cost.1 - (egress + each)).abs() < 1e-9,
        "{:?} vs {}",
        w.cost,
        egress + each
    );
}

/// 33.0: sharding small reads was credited with every moment its requests
/// were in flight, the time their bodies took to arrive with it. The shards
/// read the same bytes: only the round trips go (to each first byte).
#[test]
fn sharding_small_reads_saves_their_round_trips_not_the_time_their_bytes_moved() {
    // 600 reads one after another, 10 ms each: 4 ms to the first byte, 6 ms
    // of body
    let rs: Vec<Record> = (0..600u64)
        .map(|i| {
            let mut r = rec(
                i + 1,
                "GetObject",
                &format!("train/img-{i:05}.jpg"),
                110_000,
            );
            r.latency_ms = 10.0;
            r.first_byte_ms = Some(4.0).into();
            r
        })
        .collect();
    let a = sonde_analysis::view::Analysis::of_records(&header(), &rs, &charged());
    let o = find(&a.observations, "obs.smallreads").expect("small reads");
    assert_eq!(o.saves_requests, 599);
    // 599 round trips of 4 ms, not 599 requests of 10 ms (5.99 s)
    assert_eq!(o.saves_ms, 2396.0);
    // and the run without them, reckoned again, waits as much less
    let reckoning = sonde_analysis::fixes::Reckoning::of(&rs, &a.profile, true);
    let w = sonde_analysis::fixes::what_if(&reckoning, std::slice::from_ref(o)).expect("a what-if");
    assert_eq!(w.waiting_ms, Some((6000.0, 3604.0)));
    // a run that does not say when a first byte came: as before, their whole time
    let untold: Vec<Record> = rs
        .iter()
        .cloned()
        .map(|mut r| {
            r.first_byte_ms = None.into();
            r
        })
        .collect();
    let a = sonde_analysis::view::Analysis::of_records(&header(), &untold, &charged());
    let o = find(&a.observations, "obs.smallreads").expect("small reads");
    assert_eq!(o.saves_ms, 5990.0);
}

/// Partition discovery: each partition's folder listed once.
#[test]
fn many_prefixes_listed_one_by_one_are_a_crawl() {
    let rs: Vec<Record> = (0..30u64)
        .map(|i| listing(i + 1, &format!("events/date=2026-10-{:02}/", i + 1)))
        .collect();
    let obs = findings(&rs);
    let o = find(&obs, "obs.crawl").expect("a crawl");
    assert_eq!(
        o.text,
        "30 prefixes under lake/events/ were listed one by one (30 list requests): read a manifest, or the table format's metadata, instead"
    );
    assert_eq!(o.saves_requests, 29);
    assert_eq!(o.identity, "obs.crawl|lake");
    // a few folders listed is no crawl, nor is one prefix listed again and
    // again (that is `obs.listed`)
    assert!(find(&findings(&rs[..10]), "obs.crawl").is_none());
    let again: Vec<Record> = (0..30u64).map(|i| listing(i + 1, "events/")).collect();
    let obs = findings(&again);
    assert!(find(&obs, "obs.crawl").is_none());
    assert!(find(&obs, "obs.listed").is_some());
}

fn header() -> sonde_analysis::run::RunHeader {
    sonde_analysis::run::RunHeader::new("e", "https://e", "us-east-1", None)
}
