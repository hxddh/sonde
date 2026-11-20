//! A run file holds its records as they finished (the proxy numbers and
//! writes each when its answer is in), and its times are when each was sent:
//! what a run is read as is the order they were sent, whatever the file's.

use chrono::DateTime;
use reqlog::{Header, Record, Sink};
use sonde_analysis::episode::{episodes, traces};
use sonde_analysis::run::{RunFile, RunHeader, RunWriter};

fn rec(id: u64, at_ms: i64, method: &str, op: &str, key: &str) -> Record {
    Record {
        id,
        time: DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
        method: method.into(),
        kind: op.into(),
        bucket: Some("b".into()),
        key: Some(key.into()),
        status: Some(if method == "DELETE" { 204 } else { 200 }),
        latency_ms: 5.0,
        request_bytes: Some(if method == "PUT" { 100 } else { 0 }).into(),
        response_bytes: Some(100).into(),
        ..Default::default()
    }
}

fn ranged(mut r: Record, range: &str) -> Record {
    r.lists_mut()
        .request_headers
        .push(Header::new("Range", range));
    r
}

/// `records` written in the order given (as they finished), read back.
fn read(records: &[Record]) -> RunFile {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.sonde");
    let w = RunWriter::create(&path, &RunHeader::new("e", "http://e", "r", None)).unwrap();
    for r in records {
        w.write(r);
    }
    w.flush().unwrap();
    RunFile::open(&path).unwrap()
}

fn kinds(run: &RunFile) -> Vec<String> {
    traces(&run.records, &episodes(&run.records))
        .into_iter()
        .map(|o| o.kind)
        .collect()
}

#[test]
fn a_run_is_read_in_the_order_it_was_sent() {
    // 2 was sent first and finished last
    let run = read(&[
        rec(1, 10, "GET", "GetObject", "a"),
        rec(3, 20, "GET", "GetObject", "c"),
        rec(2, 0, "GET", "GetObject", "b"),
    ]);
    let ids: Vec<u64> = run.records.iter().map(|r| r.id).collect();
    assert_eq!(ids, [2, 1, 3]);
}

#[test]
fn a_range_read_again_after_a_write_between_is_not_read_again() {
    // read at 0 (finished last), written at 10, read again at 20
    let run = read(&[
        rec(1, 10, "PUT", "PutObject", "t.parquet"),
        ranged(rec(2, 20, "GET", "GetObject", "t.parquet"), "bytes=0-99"),
        ranged(rec(3, 0, "GET", "GetObject", "t.parquet"), "bytes=0-99"),
    ]);
    assert!(
        !kinds(&run).contains(&"obs.rangeagain".into()),
        "{:?}",
        kinds(&run)
    );
}

#[test]
fn a_get_sent_before_a_head_is_no_head_then_get() {
    // the GET at 0 finished after the HEAD sent at 10
    let run = read(&[
        rec(1, 10, "HEAD", "HeadObject", "k"),
        rec(2, 0, "GET", "GetObject", "k"),
    ]);
    assert!(
        !kinds(&run).contains(&"obs.headget".into()),
        "{:?}",
        kinds(&run)
    );
}

#[test]
fn a_write_sent_before_a_read_is_no_read_modify_write() {
    // the PUT at 0 finished after the GET sent at 10
    let run = read(&[
        rec(1, 10, "GET", "GetObject", "k"),
        rec(2, 0, "PUT", "PutObject", "k"),
    ]);
    assert!(
        !kinds(&run).contains(&"obs.rmw".into()),
        "{:?}",
        kinds(&run)
    );
}

#[test]
fn what_a_run_leaves_is_of_the_order_it_sent() {
    // written at 0 (finished last), deleted at 50
    let run = read(&[
        rec(1, 50, "DELETE", "DeleteObject", "k"),
        rec(2, 0, "PUT", "PutObject", "k"),
    ]);
    assert_eq!(sonde_analysis::stored::stored(&run.records).bytes(), 0);
}

#[test]
fn a_get_sent_before_its_head_is_no_pair_in_any_order() {
    // as the records come, unsorted: a HEAD after the GET is not before it
    let rs = [
        rec(1, 10, "HEAD", "HeadObject", "k"),
        rec(2, 0, "GET", "GetObject", "k"),
    ];
    let kinds: Vec<String> = traces(&rs, &episodes(&rs))
        .into_iter()
        .map(|o| o.kind)
        .collect();
    assert!(!kinds.contains(&"obs.headget".into()), "{kinds:?}");
}
