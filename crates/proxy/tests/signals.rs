//! The four signals of real recordings (tests/fixtures) and of small made-up
//! sessions: what they say adds up to what the profile and the summary say.

use std::path::Path;
use std::time::Instant;

use chrono::DateTime;
use reqlog::Record;
use sonde_proxy::profile::{RequestPrices, profile};
use sonde_proxy::session::Session;
use sonde_proxy::signals::{SPARK_COLUMNS, Signals, signals, sparkline, text};
use sonde_proxy::summary::summarize;

const FIXTURES: [&str; 6] = [
    "delta.sonde",
    "iceberg.sonde",
    "lance.sonde",
    "sdk-java.sonde",
    "sdk-python.sonde",
    "slatedb.sonde",
];

fn load(name: &str) -> Session {
    Session::open(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn rec(id: u64, at_ms: i64, op: &str, status: u16, latency: f64) -> Record {
    let method = match op {
        "PutObject" | "UploadPart" => "PUT",
        "HeadObject" => "HEAD",
        "DeleteObject" => "DELETE",
        _ => "GET",
    };
    Record {
        id,
        time: DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
        method: method.into(),
        operation: op.into(),
        bucket: Some("b".into()),
        key: Some(format!("data/{}.bin", id % 7).into()),
        status: Some(status),
        latency_ms: latency,
        ..Default::default()
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn sum(v: &[f64]) -> f64 {
    v.iter().sum()
}

#[test]
fn the_signals_of_every_recording_add_up_to_its_profile_and_summary() {
    let prices = RequestPrices::default();
    for name in FIXTURES {
        let s = load(name);
        let recs = &s.records;
        let (sig, prof, sum_) = (
            signals(recs, &prices),
            profile(recs, &prices),
            summarize(recs),
        );
        let bin_s = sig.bin_ms / 1000.0;

        // one time axis with the profile's charts
        assert_eq!(sig.bin_ms, prof.bin_ms, "{name}");
        assert_eq!(sig.bins, prof.rate.len(), "{name}");
        assert_eq!(sig.wall_ms, prof.wall_ms, "{name}");
        assert_eq!(sig.timed, prof.timed, "{name}");
        for series in [
            &sig.rate.series,
            &sig.errors.series,
            &sig.errors.client,
            &sig.errors.server,
            &sig.errors.transport,
            &sig.errors.injected,
            &sig.errors.retries,
            &sig.latency.series,
            &sig.latency.p95,
            &sig.latency.p99,
            &sig.bytes.series,
            &sig.bytes.up,
            &sig.bytes.down,
            &sig.in_flight.series,
        ] {
            assert_eq!(series.len(), sig.bins, "{name}");
        }

        // rate: the slices hold every request
        assert_eq!(sig.rate.requests, recs.len(), "{name}");
        assert!(
            near(sum(&sig.rate.series) * bin_s, recs.len() as f64),
            "{name}"
        );
        // a run under a second has its total and no rate
        assert_eq!(sig.short, sig.wall_ms < 1000.0, "{name}");
        assert_eq!(sig.rate.value.is_none(), sig.short, "{name}");
        if let Some(rate) = sig.rate.value {
            assert!(
                near(rate * sig.wall_ms / 1000.0, recs.len() as f64),
                "{name}"
            );
        }

        // errors: the summary's failures, each in one class, each in one slice
        let c = &sig.errors.counts;
        assert_eq!(sig.errors.count as u64, sum_.errors, "{name}");
        assert_eq!(
            c.client + c.server + c.transport + c.injected,
            sig.errors.count,
            "{name}"
        );
        assert_eq!(sum(&sig.errors.series), sig.errors.count as f64, "{name}");
        assert_eq!(sum(&sig.errors.client), c.client as f64, "{name}");
        assert_eq!(sum(&sig.errors.server), c.server as f64, "{name}");
        assert_eq!(sum(&sig.errors.transport), c.transport as f64, "{name}");
        assert_eq!(sum(&sig.errors.injected), c.injected as f64, "{name}");
        assert_eq!(
            sum(&sig.errors.retries),
            sig.errors.retries_total as f64,
            "{name}"
        );
        assert!(
            sig.errors.retries_total >= recs.iter().map(|r| r.retries as u64).sum::<u64>(),
            "{name}"
        );
        // what the profile draws as client and server errors, and no more
        let drawn: usize = prof.rate.iter().map(|b| b.client + b.server).sum();
        assert_eq!(sig.errors.count, drawn, "{name}");

        // latency: the profile's figures, and bands that never cross
        assert_eq!(sig.latency.p50_ms, Some(prof.p50_ms), "{name}");
        assert_eq!(sig.latency.p95_ms, Some(prof.p95_ms), "{name}");
        assert_eq!(sig.latency.p99_ms, Some(prof.p99_ms), "{name}");
        assert_eq!(sig.latency.value, sig.latency.p50_ms, "{name}");
        assert!(prof.p50_ms <= prof.p95_ms && prof.p95_ms <= prof.p99_ms);
        for (i, b) in prof.rate.iter().enumerate() {
            let (p50, p95, p99) = (
                sig.latency.series[i],
                sig.latency.p95[i],
                sig.latency.p99[i],
            );
            assert!(p50 <= p95 && p95 <= p99, "{name} slice {i}");
            assert_eq!((p50, p95, p99), (b.p50_ms, b.p95_ms, b.p99_ms), "{name}");
        }

        // bytes: each way, slice by slice, what the profile counted
        let up: u64 = recs.iter().map(|r| r.request_bytes.unwrap_or(0)).sum();
        let down: u64 = recs.iter().map(|r| r.response_bytes.unwrap_or(0)).sum();
        assert_eq!(
            (sig.bytes.up_bytes, sig.bytes.down_bytes),
            (up, down),
            "{name}"
        );
        assert!(near(sum(&sig.bytes.up) * bin_s, up as f64), "{name}");
        assert!(near(sum(&sig.bytes.down) * bin_s, down as f64), "{name}");
        assert!(
            near(sum(&sig.bytes.series) * bin_s, (up + down) as f64),
            "{name}"
        );
        for (i, b) in prof.rate.iter().enumerate() {
            assert!(near(sig.bytes.up[i] * bin_s, b.bytes_up as f64), "{name}");
            assert!(
                near(sig.bytes.down[i] * bin_s, b.bytes_down as f64),
                "{name}"
            );
        }
        assert_eq!(sig.bytes.value.is_none(), sig.short, "{name}");
        if let Some(both) = sig.bytes.value {
            assert!(
                near(
                    sig.bytes.up_per_sec.unwrap() + sig.bytes.down_per_sec.unwrap(),
                    both
                ),
                "{name}"
            );
        }

        // in flight: the profile's peak, reached in some slice
        assert_eq!(sig.in_flight.peak, prof.peak_in_flight, "{name}");
        assert_eq!(sig.in_flight.peak_at_ms, prof.peak_at_ms, "{name}");
        assert_eq!(
            sig.in_flight.series.iter().copied().fold(0.0, f64::max),
            prof.peak_in_flight as f64,
            "{name}"
        );

        // cost: the profile's total, split by operation
        assert_eq!(sig.cost.amount, prof.cost.amount, "{name}");
        let by_op: f64 = sig.cost.operations.iter().map(|r| r.cost).sum();
        assert!(near(by_op, prof.cost.amount), "{name}: {by_op}");
        assert_eq!(
            sig.cost
                .operations
                .iter()
                .map(|r| r.requests)
                .sum::<usize>(),
            recs.len(),
            "{name}"
        );
        assert_eq!(
            sig.cost.operations.iter().map(|r| r.billed).sum::<usize>(),
            prof.cost.writes + prof.cost.reads + prof.cost.free,
            "{name}"
        );
        if prof.cost.amount > 0.0 {
            let shares: f64 = sig.cost.operations.iter().map(|r| r.share).sum();
            assert!(near(shares, 1.0), "{name}: {shares}");
        }

        // prefixes: the profile's, each against S3's limit
        assert_eq!(sig.prefixes.len(), prof.prefixes.len(), "{name}");
        for (a, b) in sig.prefixes.iter().zip(&prof.prefixes) {
            assert_eq!(
                (
                    &a.prefix,
                    a.folders,
                    a.requests,
                    a.peak_reads,
                    a.peak_writes
                ),
                (
                    &b.prefix,
                    b.folders,
                    b.requests,
                    b.peak_reads,
                    b.peak_writes
                ),
                "{name}"
            );
            assert!(near(a.read_share * 5500.0, b.peak_reads as f64), "{name}");
            assert!(near(a.write_share * 3500.0, b.peak_writes as f64), "{name}");
        }

        // statuses and sizes: every request once
        assert_eq!(
            sig.statuses.iter().map(|r| r.count).sum::<usize>(),
            recs.len(),
            "{name}"
        );
        for row in &sig.statuses {
            assert!(row.first_ms <= row.last_ms && row.last_ms <= sig.wall_ms);
        }
        assert_eq!(
            sig.sizes.request.iter().map(|b| b.count).sum::<usize>(),
            recs.iter().filter(|r| r.request_bytes.is_some()).count(),
            "{name}"
        );
        assert_eq!(sig.sizes.request.iter().map(|b| b.bytes).sum::<u64>(), up);
        assert_eq!(
            sig.sizes.response.iter().map(|b| b.bytes).sum::<u64>(),
            down
        );
        for h in [&sig.sizes.request, &sig.sizes.response] {
            assert!(
                h.windows(2).all(|w| w[0].le_bytes < w[1].le_bytes),
                "{name}"
            );
        }

        // first byte: no more of the time than all of it
        if let Some(fb) = &sig.first_byte {
            assert!((0.0..=1.0).contains(&fb.share), "{name}: {}", fb.share);
            assert_eq!(fb.kinds.iter().map(|k| k.count).sum::<usize>(), fb.count);
        }

        // the terminal's reading: four lines with their sparklines, and short
        let t = text(&sig, false);
        eprintln!("--- {name}\n{t}");
        let lines: Vec<&str> = t.lines().collect();
        assert!(
            lines.len() >= 4 && lines.len() <= 20,
            "{name}: {}",
            lines.len()
        );
        for (line, label) in lines.iter().zip(["rate", "errors", "latency", "bytes"]) {
            assert!(line.starts_with(label), "{name}: {line}");
            // as wide as its slices, never wider than the line; none for no errors
            let bars = line.chars().filter(|c| ('▁'..='█').contains(c)).count();
            let none = label == "errors" && sig.errors.count == 0;
            let want = if none { 0 } else { sig.bins.min(SPARK_COLUMNS) };
            assert_eq!(bars, want, "{name}: {line}");
        }
        assert!(!t.contains('\x1b'), "{name}");
        assert!(text(&sig, true).contains("\x1b[2mrate"), "{name}");

        // the same records, the same signals
        assert_eq!(sig, signals(recs, &prices), "{name}");
        serde_json::to_string(&sig).unwrap();
    }
}

#[test]
fn a_session_without_times_has_no_latency_and_no_first_byte() {
    // a CloudTrail file: when each request was made, not how long it took
    let recs: Vec<Record> = (0..20)
        .map(|i| rec(i + 1, i as i64 * 500, "GetObject", 200, 0.0))
        .collect();
    let s = signals(&recs, &RequestPrices::default());
    assert!(!s.timed);
    assert_eq!(s.latency.value, None);
    assert_eq!(
        (s.latency.p50_ms, s.latency.p95_ms, s.latency.p99_ms),
        (None, None, None)
    );
    assert!(s.latency.series.is_empty() && s.latency.p95.is_empty() && s.latency.p99.is_empty());
    assert_eq!(s.first_byte, None);
    assert_eq!(s.in_flight.value, None);
    assert_eq!(s.sizes.mib_per_sec, None);
    // what was asked and when is still said
    assert!(near(s.rate.value.unwrap(), 20.0 / 9.5));
    assert_eq!(s.cost.operations[0].requests, 20);
    let t = text(&s, false);
    assert!(t.contains("latency  not recorded"), "{t}");
    assert!(!t.contains("first byte"), "{t}");

    // and of no session at all, nothing
    let none = signals(&[], &RequestPrices::default());
    assert_eq!(
        (none.bins, none.rate.value, none.errors.value),
        (0, None, None)
    );
    assert_eq!(text(&none, false), "");
    let v = serde_json::to_value(&none).unwrap();
    assert!(v["latency"]["p50Ms"].is_null() && v["firstByte"].is_null());
}

#[test]
fn an_injected_error_is_counted_as_injected_not_as_the_stores() {
    let mut recs: Vec<Record> = (0..10)
        .map(|i| rec(i + 1, i as i64 * 100, "GetObject", 200, 5.0))
        .collect();
    // (ten requests, none of them another's retry)
    for r in &mut recs {
        r.key = Some(format!("data/{}.bin", r.id).into());
    }
    // a fault scenario answered 503 in the store's place; the store's own 503
    recs[2].status = Some(503);
    recs[2].injected = Some("slow-down".into());
    recs[2].error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
    recs[3].status = Some(503);
    recs[3].error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
    // a scenario lost an answer: injected too, not a transport error
    recs[4].status = None;
    recs[4].transport_error = Some("connection reset".into());
    recs[4].injected = Some("lost-answer".into());
    // a scenario that only delayed a request failed nothing
    recs[5].injected = Some("delay".into());
    // the rest of the classes
    recs[6].status = Some(403);
    recs[6].error_body = Some("AccessDenied: no".into());
    recs[7].status = None;
    recs[7].transport_error = Some("timed out".into());
    recs[7].retries = 3;
    // a lookup that found nothing is no error
    recs[8].status = Some(404);
    recs[8].error_body = Some("<Error><Code>NoSuchKey</Code></Error>".into());

    let s = signals(&recs, &RequestPrices::default());
    let c = &s.errors.counts;
    assert_eq!((c.injected, c.server, c.client, c.transport), (2, 1, 1, 1));
    assert_eq!(s.errors.count, 5);
    assert_eq!(s.errors.value, Some(0.5));
    assert_eq!(s.errors.retries_total, 3);
    assert_eq!(s.errors.injected.iter().sum::<f64>(), 2.0);
    assert_eq!(s.errors.server.iter().sum::<f64>(), 1.0);

    // each answer with its code, the injected ones marked
    let row = |status: Option<u16>, code: Option<&str>| {
        s.statuses
            .iter()
            .find(|r| r.status == status && r.code.as_deref() == code)
            .unwrap_or_else(|| panic!("{status:?} {code:?} in {:?}", s.statuses))
    };
    let slow = row(Some(503), Some("SlowDown"));
    assert_eq!((slow.count, slow.injected), (2, 1));
    assert_eq!((slow.first_ms, slow.last_ms), (200.0, 300.0));
    assert_eq!(row(Some(403), Some("AccessDenied")).count, 1);
    assert_eq!(row(Some(404), Some("NoSuchKey")).count, 1);
    assert_eq!(row(None, None).count, 2);
    assert_eq!(row(None, None).injected, 1);
    let ok = row(Some(200), None);
    assert_eq!((ok.count, ok.injected), (4, 1));
    assert_eq!(s.statuses[0].status, Some(200));

    let t = text(&s, false);
    assert!(
        t.contains("50% (5)")
            && t.contains("1 client, 1 server, 1 no answer, 2 injected, 3 retries"),
        "{t}"
    );
    assert!(text(&s, true).contains("\x1b[31m50% (5)\x1b[0m"));
}

#[test]
fn the_first_byte_is_told_from_the_rest_by_kind_of_request() {
    let mut recs = Vec::new();
    // reads: the body is most of the time; writes: the answer is all of it
    for i in 0..5u64 {
        let mut r = rec(i + 1, i as i64 * 10, "GetObject", 200, 100.0);
        r.first_byte_ms = Some(20.0 + i as f64);
        recs.push(r);
    }
    for i in 5..8u64 {
        let mut r = rec(i + 1, i as i64 * 10, "PutObject", 200, 50.0);
        r.first_byte_ms = Some(50.0);
        recs.push(r);
    }
    // one whose first byte was not timed is in neither
    recs.push(rec(9, 90, "HeadObject", 200, 7.0));
    let s = signals(&recs, &RequestPrices::default());
    let fb = s.first_byte.as_ref().unwrap();
    assert_eq!(fb.count, 8);
    assert_eq!(fb.kinds.len(), 2);
    let get = &fb.kinds[0];
    assert_eq!(
        (
            get.operation.as_str(),
            get.count,
            get.first_byte_ms,
            get.rest_ms
        ),
        ("GetObject", 5, 22.0, 78.0)
    );
    let put = &fb.kinds[1];
    assert_eq!(
        (put.operation.as_str(), put.first_byte_ms, put.rest_ms),
        ("PutObject", 50.0, 0.0)
    );
    // 110 + 150 of 500 + 150 ms
    assert!(near(fb.share, 260.0 / 650.0));
    let t = text(&s, false);
    assert!(t.contains("first byte  40% of the time"), "{t}");
    assert!(t.contains("first byte  then the rest"), "{t}");
    assert!(t.contains("GetObject          5       22 ms  78 ms"), "{t}");
}

#[test]
fn sizes_fall_in_buckets_that_double_and_transfers_are_timed_against_them() {
    const MIB: u64 = 1024 * 1024;
    let mut recs = Vec::new();
    let mut put = |bytes: u64, ms: f64| {
        let mut r = rec(
            recs.len() as u64 + 1,
            recs.len() as i64 * 10,
            "PutObject",
            200,
            ms,
        );
        r.request_bytes = Some(bytes);
        r.response_bytes = Some(0);
        recs.push(r);
    };
    // 8 MiB in one, two and four seconds: 8, 4 and 2 MiB/s
    put(8 * MIB, 1000.0);
    put(8 * MIB, 2000.0);
    put(8 * MIB, 4000.0);
    // just past 8 MiB: the next bucket
    put(8 * MIB + 1, 1000.0);
    // small bodies say nothing of throughput
    put(1000, 10.0);
    put(1024, 10.0);
    put(1, 10.0);
    put(0, 10.0);
    let mut get = rec(9, 90, "GetObject", 200, 500.0);
    get.response_bytes = Some(64 * 1024);
    recs.push(get);
    // a transfer that failed moved nothing whole
    let mut cut = rec(10, 100, "GetObject", 200, 500.0);
    cut.response_bytes = Some(32 * MIB);
    cut.transport_error = Some("cut".into());
    recs.push(cut);
    // a size not known is in no bucket
    recs.push(rec(11, 110, "HeadObject", 200, 1.0));

    let s = signals(&recs, &RequestPrices::default());
    let req: Vec<(u64, usize)> = s
        .sizes
        .request
        .iter()
        .map(|b| (b.le_bytes, b.count))
        .collect();
    // the empty body, then 1 B, then from 1 KiB to 16 MiB with the gaps kept
    assert_eq!(req[0], (0, 1));
    assert_eq!(req[1], (1, 1));
    assert_eq!(req[2], (2, 0));
    assert!(req.contains(&(1024, 2)), "{req:?}");
    assert!(req.contains(&(8 * MIB, 3)), "{req:?}");
    assert_eq!(*req.last().unwrap(), (16 * MIB, 1));
    assert_eq!(req.iter().map(|b| b.1).sum::<usize>(), 8);
    let eight = s
        .sizes
        .request
        .iter()
        .find(|b| b.le_bytes == 8 * MIB)
        .unwrap();
    assert_eq!(eight.bytes, 24 * MIB);
    // responses: the empty ones and then only where there are bodies
    let resp: Vec<(u64, usize)> = s
        .sizes
        .response
        .iter()
        .map(|b| (b.le_bytes, b.count))
        .collect();
    assert_eq!(resp[0], (0, 8));
    assert_eq!(resp[1], (64 * 1024, 1));
    assert_eq!(*resp.last().unwrap(), (32 * MIB, 1));

    let rows: Vec<(&str, u64, usize, f64)> = s
        .sizes
        .throughput
        .iter()
        .map(|t| (t.operation.as_str(), t.bucket, t.count, t.mib_per_sec))
        .collect();
    assert_eq!(
        rows,
        [
            ("GetObject", 64 * 1024, 1, 0.125),
            ("PutObject", 8 * MIB, 3, 4.0),
            ("PutObject", 16 * MIB, 1, 8.0 + 1.0 / MIB as f64),
        ]
    );
    assert_eq!(s.sizes.mib_per_sec, Some(4.0));
}

#[test]
fn requests_in_flight_run_on_through_the_slices_they_span() {
    // one request for ten seconds; in its fourth second, three more at once
    let mut recs = vec![rec(1, 0, "GetObject", 200, 10_000.0)];
    for i in 0..3 {
        recs.push(rec(i + 2, 3000, "GetObject", 200, 100.0));
    }
    let s = signals(&recs, &RequestPrices::default());
    let p = profile(&recs, &RequestPrices::default());
    assert_eq!(s.in_flight.peak, 4);
    assert_eq!(s.in_flight.peak, p.peak_in_flight);
    assert_eq!(s.in_flight.peak_at_ms, 3000.0);
    let at = |ms: f64| s.in_flight.series[(ms / s.bin_ms) as usize];
    assert_eq!(at(3000.0), 4.0);
    // before and after the burst the long request is still there
    assert_eq!(at(1000.0), 1.0);
    assert_eq!(at(9000.0), 1.0);
    assert!(s.in_flight.series.iter().all(|n| *n >= 1.0));
}

#[test]
fn cost_by_operation_is_the_profiles_cost_and_a_refused_request_costs_nothing() {
    let mut recs = Vec::new();
    for i in 0..1000u64 {
        let op = ["PutObject", "GetObject", "ListObjectsV2", "DeleteObject"][(i % 4) as usize];
        recs.push(rec(i + 1, i as i64, op, 200, 2.0));
    }
    // no answer: not on the store's bill
    recs[0].status = None;
    let prices = RequestPrices {
        per_1000_lists: Some(0.004),
        ..RequestPrices::default()
    };
    let s = signals(&recs, &prices);
    let p = profile(&recs, &prices);
    assert_eq!(s.cost.amount, p.cost.amount);
    assert!(near(
        s.cost.operations.iter().map(|r| r.cost).sum(),
        p.cost.amount
    ));
    let row = |op: &str| {
        s.cost
            .operations
            .iter()
            .find(|r| r.operation == op)
            .unwrap()
    };
    assert_eq!(
        (row("PutObject").class, row("PutObject").billed),
        ("write", 249)
    );
    assert!(near(row("PutObject").cost, 249.0 * 0.005 / 1000.0));
    assert_eq!(row("ListObjectsV2").class, "list");
    assert!(near(row("ListObjectsV2").cost, 250.0 * 0.004 / 1000.0));
    assert_eq!(row("GetObject").class, "read");
    assert_eq!(
        (row("DeleteObject").class, row("DeleteObject").cost),
        ("free", 0.0)
    );
    // dearest first
    assert_eq!(s.cost.operations[0].operation, "PutObject");
    let t = text(&s, false);
    assert!(t.contains("cost  ") && !t.contains("DeleteObject"), "{t}");
}

#[test]
fn a_prefix_past_half_of_s3s_limit_is_said_in_the_terminal() {
    // 3,000 reads of one prefix within a second, and a quiet prefix
    let mut recs: Vec<Record> = (0..3000u64)
        .map(|i| Record {
            key: Some("hot/part.parquet".into()),
            ..rec(i + 1, (i / 4) as i64, "GetObject", 200, 1.0)
        })
        .collect();
    recs.push(Record {
        key: Some("cold/x".into()),
        ..rec(3001, 5000, "PutObject", 200, 1.0)
    });
    let s = signals(&recs, &RequestPrices::default());
    let hot = s.prefixes.iter().find(|p| p.prefix == "b/hot/").unwrap();
    assert_eq!((hot.peak_reads, hot.peak_writes), (3000, 0));
    assert!(near(hot.read_share, 3000.0 / 5500.0));
    let cold = s.prefixes.iter().find(|p| p.prefix == "b/cold/").unwrap();
    assert!(near(cold.write_share, 1.0 / 3500.0));
    let t = text(&s, false);
    assert!(t.contains("prefixes past half of S3's limit"), "{t}");
    assert!(t.contains("b/hot/") && !t.contains("b/cold/"), "{t}");
    assert!(t.contains("3,000 reads/s   55%"), "{t}");
}

#[test]
fn a_sparkline_keeps_a_burst_and_shows_anything_above_nothing() {
    assert_eq!(sparkline(&[], 32), "");
    assert_eq!(sparkline(&[0.0, 0.0], 4), "▁▁");
    // fewer values than columns: a bar each, none drawn wide
    assert_eq!(sparkline(&[0.0, 8.0], 4), "▁█");
    // more: a column is the most of what it stands for
    let mut long = vec![0.0; 320];
    long[5] = 100.0;
    long[200] = 1.0;
    let s: Vec<char> = sparkline(&long, 32).chars().collect();
    assert_eq!(s.len(), 32);
    assert_eq!(s[0], '█');
    // one in a hundred is still not nothing
    assert_eq!(s[20], '▂');
    assert!(
        s.iter()
            .enumerate()
            .all(|(i, c)| i == 0 || i == 20 || *c == '▁')
    );
}

fn million() -> Vec<Record> {
    (0..1_000_000u64)
        .map(|i| {
            let (method, op, key) = match i % 10 {
                0 => (
                    "PUT",
                    "PutObject",
                    format!("db/wal/{:04}/{i}.sst", i % 3000),
                ),
                1 => (
                    "HEAD",
                    "HeadObject",
                    format!("data/dt={}/p.parquet", i % 400),
                ),
                2..=6 => ("GET", "GetObject", format!("data/dt={}/p.parquet", i % 400)),
                7 => ("GET", "ListObjectsV2", String::new()),
                8 => ("PUT", "UploadPart", format!("t/metadata/{}.json", i / 10)),
                _ => ("DELETE", "DeleteObject", format!("tmp/{i}")),
            };
            let failed = i % 97 == 0;
            Record {
                id: i + 1,
                time: DateTime::from_timestamp_micros(1_700_000_000_000_000 + i as i64 * 700)
                    .unwrap(),
                method: method.into(),
                operation: op.into(),
                bucket: Some("b".into()),
                key: (!key.is_empty()).then(|| key.into()),
                status: Some(if failed { 503 } else { 200 }),
                error_body: failed.then(|| "<Error><Code>SlowDown</Code></Error>".into()),
                latency_ms: 3.0 + (i % 50) as f64,
                first_byte_ms: Some(1.0 + (i % 7) as f64),
                request_bytes: Some(if method == "PUT" {
                    100_000 + i % 5_000_000
                } else {
                    0
                }),
                response_bytes: Some(if op == "GetObject" { i % 300_000 } else { 200 }),
                retries: u32::from(i % 211 == 0),
                ..Default::default()
            }
        })
        .collect()
}

fn check_million(s: &Signals, n: usize) {
    assert_eq!(s.rate.requests, n);
    assert!(near(sum(&s.rate.series) * s.bin_ms / 1000.0, n as f64));
    assert_eq!(s.errors.counts.server, n.div_ceil(97));
    assert!(s.bins <= 91 && s.in_flight.peak > 1);
    assert!(text(s, false).lines().count() <= 20);
}

/// `cargo test -p sonde-proxy --test signals -- --ignored --nocapture` says
/// how long. Two seconds is the bound of a build that is optimised; a debug
/// build on a busy machine is given three times that.
#[test]
#[ignore = "a million records: slow to build in a debug run"]
fn a_million_requests_are_read_in_under_two_seconds() {
    let recs = million();
    let prices = RequestPrices::default();
    let t = Instant::now();
    let s = signals(&recs, &prices);
    let took = t.elapsed();
    eprintln!("signals of {} requests: {took:?}", recs.len());
    check_million(&s, recs.len());
    let bound = if cfg!(debug_assertions) { 6.0 } else { 2.0 };
    assert!(took.as_secs_f64() < bound, "{took:?}");
}

#[test]
fn a_retry_sent_as_a_request_of_its_own_is_a_retry() {
    // an SDK's retry through the proxy is another record, its own `retries` zero
    let mut recs = vec![
        rec(1, 0, "PutObject", 503, 2.0),
        rec(2, 300, "PutObject", 200, 2.0),
        rec(3, 400, "GetObject", 200, 2.0),
        rec(4, 500, "GetObject", 200, 2.0),
    ];
    for r in &mut recs {
        r.key = Some("state".into());
    }
    let s = signals(&recs, &RequestPrices::default());
    // the PUT again after its 503; the second GET follows a success and is no retry
    assert_eq!(s.errors.retries_total, 1);
    assert_eq!(s.errors.retries.iter().sum::<f64>(), 1.0);
    assert!(text(&s, false).contains("1 retry"), "{}", text(&s, false));
}
