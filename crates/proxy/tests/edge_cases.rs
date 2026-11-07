//! Edge cases found reviewing 4.0: retries around commits and multipart
//! uploads, a session cut off mid-character, a replay's root listing.

use chrono::DateTime;
use reqlog::{Record, Sink};
use sonde_proxy::episode::{Kind, episodes, observations};
use sonde_proxy::session::{Session, SessionHeader, SessionWriter};

fn rec(id: u64, at_ms: i64, method: &str, op: &str, key: Option<&str>, status: u16) -> Record {
    Record {
        id,
        time: DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
        method: method.into(),
        operation: op.into(),
        bucket: Some("b".into()),
        key: key.map(Into::into),
        status: Some(status),
        latency_ms: 5.0,
        request_bytes: Some(if method == "PUT" { 100 } else { 0 }),
        response_bytes: Some(10),
        ..Default::default()
    }
}

fn q(mut r: Record, q: &[(&str, &str)]) -> Record {
    r.query = q
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    r
}

fn slow_down(mut r: Record) -> Record {
    r.error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
    r
}

#[test]
fn a_retried_commit_is_still_a_commit() {
    let recs = [
        rec(
            1,
            0,
            "PUT",
            "PutObject",
            Some("t/data/00000-0-a.parquet"),
            200,
        ),
        rec(2, 10, "PUT", "PutObject", Some("t/metadata/a-m0.avro"), 200),
        slow_down(rec(
            3,
            30,
            "PUT",
            "PutObject",
            Some("t/metadata/00001-x.metadata.json"),
            503,
        )),
        rec(
            4,
            300,
            "PUT",
            "PutObject",
            Some("t/metadata/00001-x.metadata.json"),
            200,
        ),
        rec(5, 10_000, "GET", "GetObject", Some("readme"), 200),
    ];
    let eps = episodes(&recs);
    let obs = observations(&recs, &eps);
    assert!(
        !eps.iter().any(|e| e.title.contains("no commit followed")),
        "{:#?}",
        eps.iter().map(|e| &e.title).collect::<Vec<_>>()
    );
    assert!(
        !obs.iter().any(|o| o.text.contains("no commit after them")),
        "{obs:#?}"
    );
    let commit = eps.iter().find(|e| e.records.contains(&4)).unwrap();
    assert!(
        commit.records.contains(&1) && commit.records.contains(&3),
        "the attempt and the files are the commit's"
    );
}

#[test]
fn a_retried_create_does_not_leave_an_upload_open() {
    let recs = [
        slow_down(rec(1, 0, "POST", "CreateMultipartUpload", Some("big"), 503)),
        rec(2, 100, "POST", "CreateMultipartUpload", Some("big"), 200),
        q(
            rec(3, 200, "PUT", "UploadPart", Some("big"), 200),
            &[("partNumber", "1"), ("uploadId", "u1")],
        ),
        q(
            rec(4, 300, "POST", "CompleteMultipartUpload", Some("big"), 200),
            &[("uploadId", "u1")],
        ),
    ];
    let eps = episodes(&recs);
    let obs = observations(&recs, &eps);
    let uploads: Vec<_> = eps.iter().filter(|e| e.kind == Kind::Multipart).collect();
    assert_eq!(
        uploads.len(),
        1,
        "{:#?}",
        eps.iter()
            .map(|e| (&e.records, &e.title))
            .collect::<Vec<_>>()
    );
    assert!(
        !uploads[0].title.contains("not completed"),
        "{}",
        uploads[0].title
    );
    assert!(
        !obs.iter().any(|o| o.text.contains("never completed")),
        "{obs:#?}"
    );
}

#[test]
fn a_session_cut_off_mid_character_still_opens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.sonde");
    let w = SessionWriter::create(&path, &SessionHeader::new("r2", "u", "auto", None)).unwrap();
    w.write(&Record {
        id: 1,
        operation: "GetObject".into(),
        ..Default::default()
    });
    w.flush().unwrap();
    drop(w);
    let mut bytes = std::fs::read(&path).unwrap();
    let line = "{\"log\":{\"time\":\"2026-01-01T00:00:00Z\",\"text\":\"世界\"}}";
    bytes.extend_from_slice(&line.as_bytes()[..line.find('世').unwrap() + 1]);
    std::fs::write(&path, &bytes).unwrap();
    let s = Session::open(&path).expect("the records before the torn line");
    assert_eq!(s.records.len(), 1);
}

#[test]
fn a_replays_root_listing_reads_as_the_original() {
    let mut a = rec(1, 0, "GET", "ListObjectsV2", None, 200);
    a.query = vec![("list-type".into(), "2".into())];
    let mut b = a.clone();
    b.query
        .push(("prefix".into(), "replay-20261107-015048/".into()));
    assert_eq!(
        sonde_proxy::replay::transcript(&[a]),
        sonde_proxy::replay::transcript(&[b])
    );
}
