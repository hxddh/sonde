//! How the analysis scales: a synthetic session of N requests (default a
//! million) written, compressed, read back and analysed, each step timed.
//!
//!     cargo run --release -p sonde-proxy --example scale -- 1000000
//!
//! A path after the count keeps the session there (the nightly run gives it to
//! sonde-nsh's timed test of the bridge, `SONDE_BIG_SESSION`).

use std::time::Instant;

use chrono::DateTime;
use reqlog::Record;
use sonde_proxy::episode::{episodes, observations};
use sonde_proxy::profile::{RequestPrices, profile};
use sonde_proxy::session::{Session, SessionHeader, SessionWriter, compress};

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1_000_000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.sonde");
    let t = Instant::now();
    {
        let w = SessionWriter::create(
            &path,
            &SessionHeader::new("e", "http://e", "us-east-1", Some("etl".into())),
        )
        .unwrap();
        use reqlog::Sink;
        for i in 0..n {
            let (op, method) = match i % 10 {
                0 => ("PutObject", "PUT"),
                1 => ("HeadObject", "HEAD"),
                2 => ("ListObjectsV2", "GET"),
                _ => ("GetObject", "GET"),
            };
            w.write(&Record {
                id: i as u64 + 1,
                time: DateTime::from_timestamp_micros(1_700_000_000_000_000 + i as i64 * 300)
                    .unwrap(),
                method: method.into(),
                operation: op.into(),
                bucket: Some("lake".into()),
                key: Some(format!("t/part={}/f-{:06}.parquet", i % 97, i / 7).into()),
                status: Some(if i % 501 == 0 { 404 } else { 200 }),
                latency_ms: 2.0 + (i % 13) as f64,
                response_bytes: Some(4096),
                ..Default::default()
            });
        }
        w.flush().unwrap();
    }
    let keep = std::env::args().nth(2);
    let raw = std::fs::metadata(&path).unwrap().len();
    println!("write {n}: {:.2?} ({} MiB)", t.elapsed(), raw >> 20);
    let t = Instant::now();
    compress(&path).unwrap();
    let gz = std::fs::metadata(&path).unwrap().len();
    println!(
        "compress: {:.2?} ({} MiB, {:.0}×)",
        t.elapsed(),
        gz >> 20,
        raw as f64 / gz as f64
    );
    let t = Instant::now();
    let s = Session::open(&path).unwrap();
    println!("open: {:.2?}", t.elapsed());
    if let Some(keep) = &keep {
        std::fs::copy(&path, keep).unwrap();
        println!("kept: {keep}");
    }
    let t = Instant::now();
    let eps = episodes(&s.records);
    println!("episodes: {:.2?} ({})", t.elapsed(), eps.len());
    let t = Instant::now();
    let obs = observations(&s.records, &eps);
    println!("observations: {:.2?} ({})", t.elapsed(), obs.len());
    let t = Instant::now();
    let p = profile(&s.records, &RequestPrices::default());
    println!(
        "profile: {:.2?} ({} groups, {} chains)",
        t.elapsed(),
        p.groups.len(),
        p.chains.len()
    );
}
