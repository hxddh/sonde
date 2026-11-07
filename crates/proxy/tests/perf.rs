//! A large session (50,000 requests of a busy program) opens and analyses in
//! well under the time a person waits for a window. The bound is loose for
//! debug builds and slow CI machines; `--release` shows the real figure.

use std::time::Instant;

use chrono::DateTime;
use reqlog::{Record, Sink};
use sonde_proxy::episode::{episodes, observations};
use sonde_proxy::session::{Session, SessionHeader, SessionWriter};
use sonde_proxy::summary::summarize;

pub fn busy(n: u64) -> Vec<Record> {
    (0..n)
        .map(|i| {
            let (method, op, key) = match i % 10 {
                0 => ("PUT", "PutObject", format!("db/wal/{:020}.sst", i)),
                1 => (
                    "HEAD",
                    "HeadObject",
                    format!("data/part-{}.parquet", i % 900),
                ),
                2..=6 => ("GET", "GetObject", format!("data/part-{}.parquet", i % 900)),
                7 => ("GET", "ListObjectsV2", String::new()),
                8 => (
                    "PUT",
                    "PutObject",
                    format!("t/metadata/{:05}-x.metadata.json", i / 10),
                ),
                _ => ("DELETE", "DeleteObject", format!("tmp/{i}")),
            };
            Record {
                id: i + 1,
                time: DateTime::from_timestamp_millis(1_700_000_000_000 + (i as i64) * 7).unwrap(),
                method: method.into(),
                operation: op.into(),
                bucket: Some("b".into()),
                key: (!key.is_empty()).then_some(key),
                status: Some(if i % 97 == 0 { 503 } else { 200 }),
                latency_ms: 3.0 + (i % 50) as f64,
                request_bytes: Some(if method == "PUT" { 4096 } else { 0 }),
                response_bytes: Some(if method == "GET" { 65536 } else { 0 }),
                ..Default::default()
            }
        })
        .collect()
}

#[test]
fn fifty_thousand_requests_open_in_reasonable_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.sonde");
    let w = SessionWriter::create(
        &path,
        &SessionHeader::new("ep", "http://s3", "", Some("busy".into())),
    )
    .unwrap();
    for r in busy(50_000) {
        w.write(&r);
    }
    drop(w);

    let t = Instant::now();
    let s = Session::open(&path).unwrap();
    let read = t.elapsed();
    let eps = episodes(&s.records);
    let analysed = t.elapsed() - read;
    let obs = observations(&s.records, &eps);
    let sum = summarize(&s.records);
    let total = t.elapsed();
    eprintln!(
        "50k: read {read:?}, episodes {analysed:?}, total {total:?} ({} operations, {} findings)",
        eps.len(),
        obs.len()
    );
    assert_eq!(s.records.len(), 50_000);
    assert_eq!(sum.requests, 50_000);
    assert!(total.as_secs() < 30, "{total:?}");
}
