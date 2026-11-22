//! How the analysis scales: a synthetic run of N requests (default a
//! million) written, compressed, read back and analysed, each step timed.
//!
//!     cargo run --release -p sonde-analysis --example scale -- 1000000
//!
//! A path after the count keeps the run there (the nightly run gives it to
//! sonde-nsh's timed test of the bridge, `SONDE_BIG_RUN`).

use std::time::Instant;

use chrono::DateTime;
use reqlog::Record;
use sonde_analysis::episode::{episodes, observations};
use sonde_analysis::fixes::{Reckoning, diagnose, rank_by_saving, what_if};
use sonde_analysis::profile::{RequestPrices, profile};
use sonde_analysis::run::{RunFile, RunHeader, RunWriter, compress};

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1_000_000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.sonde");
    let t = Instant::now();
    {
        let w = RunWriter::create(
            &path,
            &RunHeader::new("e", "http://e", "us-east-1", Some("etl".into())),
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
            let mut r = Record {
                id: i as u64 + 1,
                time: DateTime::from_timestamp_micros(1_700_000_000_000_000 + i as i64 * 300)
                    .unwrap(),
                method: method.into(),
                kind: op.into(),
                bucket: Some("lake".into()),
                // a key read again and again, a HEAD before each GET of the
                // next key: findings for the plan to weigh
                key: Some(match i % 10 {
                    9 => "conf/settings.json".into(),
                    1 => format!("t/part={}/f-{:06}.parquet", (i + 2) % 97, (i + 2) / 7).into(),
                    _ => format!("t/part={}/f-{:06}.parquet", i % 97, i / 7).into(),
                }),
                status: Some(if i % 501 == 0 { 404 } else { 200 }),
                latency_ms: 2.0 + (i % 13) as f64,
                response_bytes: Some(4096).into(),
                ..Default::default()
            };
            if i % 10 == 9 {
                r.lists_mut().response_headers = vec![reqlog::Header::new("ETag", "\"e1\"")];
            }
            w.write(&r);
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
    let s = RunFile::open(&path).unwrap();
    println!("open: {:.2?}", t.elapsed());
    if let Some(keep) = &keep {
        std::fs::copy(&path, keep).unwrap();
        println!("kept: {keep}");
    }
    let t = Instant::now();
    let eps = episodes(&s.records);
    println!("episodes: {:.2?} ({})", t.elapsed(), eps.len());
    let t = Instant::now();
    let mut obs = observations(&s.records, &eps);
    println!("observations: {:.2?} ({})", t.elapsed(), obs.len());
    let t = Instant::now();
    let p = profile(&s.records, &RequestPrices::default());
    println!(
        "profile: {:.2?} ({} groups, {} chains)",
        t.elapsed(),
        p.groups.len(),
        p.chains.len()
    );
    // the plan: each finding's saving, theirs in all, the diagnoses
    let t = Instant::now();
    let r = Reckoning::of(&s.records, &p, true);
    rank_by_saving(&mut obs, &r);
    let all = what_if(&r, &obs);
    let plan = diagnose(&mut obs, &eps, &r, None);
    println!(
        "plan: {:.2?} ({} diagnoses, waiting {:?})",
        t.elapsed(),
        plan.len(),
        all.and_then(|w| w.waiting_ms)
    );
}
