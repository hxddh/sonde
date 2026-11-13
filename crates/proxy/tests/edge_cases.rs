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
    r.query = q.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect();
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
        !eps.iter().any(|e| e.title().contains("no commit followed")),
        "{:#?}",
        eps.iter().map(|e| e.title()).collect::<Vec<_>>()
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
            .map(|e| (&e.records, e.title()))
            .collect::<Vec<_>>()
    );
    assert!(
        !uploads[0].title().contains("not completed"),
        "{}",
        uploads[0].title()
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

#[test]
fn records_joined_from_two_recordings_get_ids_of_their_own() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("joined.sonde");
    let w = SessionWriter::create(&path, &SessionHeader::new("r2", "u", "auto", None)).unwrap();
    for (id, key) in [(1, "a"), (2, "b"), (1, "c")] {
        w.write(&rec(id, id as i64, "GET", "GetObject", Some(key), 200));
    }
    w.flush().unwrap();
    drop(w);
    let s = Session::open(&path).unwrap();
    let ids: Vec<u64> = s.records.iter().map(|r| r.id).collect();
    assert_eq!(ids, [1, 2, 3]);
    assert_eq!(s.records[2].key.as_deref(), Some("c"));
}

#[test]
fn a_small_multipart_upload_counts_its_create_too() {
    let mut recs = Vec::new();
    for i in 0..3u64 {
        let k = format!("f{i}");
        let at = i as i64 * 100;
        recs.push(q(
            rec(
                i * 3 + 1,
                at,
                "POST",
                "CreateMultipartUpload",
                Some(&k),
                200,
            ),
            &[("uploads", "")],
        ));
        recs.push(q(
            rec(i * 3 + 2, at + 10, "PUT", "UploadPart", Some(&k), 200),
            &[("partNumber", "1"), ("uploadId", &format!("u{i}"))],
        ));
        recs.push(q(
            rec(
                i * 3 + 3,
                at + 20,
                "POST",
                "CompleteMultipartUpload",
                Some(&k),
                200,
            ),
            &[("uploadId", &format!("u{i}"))],
        ));
    }
    let eps = episodes(&recs);
    let obs = observations(&recs, &eps);
    let small = obs
        .iter()
        .find(|o| o.phrases[0].key == "obs.small")
        .expect("small uploads");
    assert_eq!(small.impact, 9);
    assert_eq!(
        small.saving_text.as_deref(),
        Some("6 fewer requests (66% of the session)")
    );
}

#[test]
fn a_head_before_ranged_reads_is_not_a_request_too_many() {
    let mut recs = Vec::new();
    for i in 0..4u64 {
        let k = format!("big{i}");
        recs.push(rec(
            i * 2 + 1,
            i as i64 * 100,
            "HEAD",
            "HeadObject",
            Some(&k),
            200,
        ));
        let mut g = rec(
            i * 2 + 2,
            i as i64 * 100 + 5,
            "GET",
            "GetObject",
            Some(&k),
            206,
        );
        g.request_headers.push(reqlog::Header {
            name: "Range".into(),
            value: "bytes=0-99".into(),
        });
        recs.push(g);
    }
    let obs = observations(&recs, &episodes(&recs));
    assert!(
        !obs.iter().any(|o| o.phrases[0].key == "obs.headget"),
        "{obs:#?}"
    );
}

#[test]
fn a_copy_of_a_version_and_a_cut_body_are_not_replayed() {
    use sonde_proxy::replay::{ReplayOptions, plan};
    let mut copy = rec(1, 0, "PUT", "CopyObject", Some("dst"), 200);
    copy.request_headers.push(reqlog::Header {
        name: "x-amz-copy-source".into(),
        value: "/b/src?versionId=v1".into(),
    });
    let mut del = q(
        rec(2, 10, "POST", "DeleteObjects", None, 200),
        &[("delete", "")],
    );
    del.request_body = Some("x".repeat(reqlog::MAX_REQUEST_BODY));
    let p = plan(
        &[copy, del],
        &ReplayOptions {
            prefix: "p/".into(),
            buckets: Vec::new(),
        },
    );
    assert_eq!(
        p.skipped.get("copies a version of the original endpoint"),
        Some(&1),
        "{:?}",
        p.skipped
    );
    assert_eq!(
        p.skipped
            .get("its body was too large to keep when recorded"),
        Some(&1),
        "{:?}",
        p.skipped
    );
}

/// A session that names `PUT /bucket?inventory` CreateBucket:
/// a replay confined to a prefix goes by the request, not by that name.
#[test]
fn a_misnamed_bucket_configuration_is_not_replayed_under_a_prefix() {
    use sonde_proxy::replay::{ReplayOptions, plan};
    let old = q(
        rec(1, 0, "PUT", "CreateBucket", None, 200),
        &[("inventory", ""), ("id", "daily")],
    );
    let p = plan(
        &[old],
        &ReplayOptions {
            prefix: "p/".into(),
            buckets: Vec::new(),
        },
    );
    assert_eq!(p.requests, 0);
    assert_eq!(
        p.skipped.get("changes the bucket itself"),
        Some(&1),
        "{:?}",
        p.skipped
    );
}

#[test]
fn throttling_counts_never_underflow() {
    let mut a = rec(1, 0, "GET", "GetObject", Some("k"), 503);
    a.latency_ms = -5.0;
    let b = rec(2, 0, "GET", "GetObject", Some("k"), 200);
    let obs = observations(&[a.clone(), b.clone()], &episodes(&[a, b]));
    assert!(
        obs.iter()
            .any(|o| o.phrases[0].key.starts_with("obs.throttled"))
    );
}

#[test]
fn a_finished_session_is_compressed_and_still_opens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("done.sonde");
    let w = SessionWriter::create(&path, &SessionHeader::new("r2", "u", "auto", None)).unwrap();
    for i in 0..500u64 {
        w.write(&rec(
            i + 1,
            i as i64,
            "GET",
            "GetObject",
            Some(&format!("data/part-{i}.parquet")),
            200,
        ));
    }
    w.flush().unwrap();
    drop(w);
    let plain = std::fs::metadata(&path).unwrap().len();
    sonde_proxy::session::compress(&path).unwrap();
    let packed = std::fs::metadata(&path).unwrap().len();
    assert!(packed * 5 < plain, "{packed} of {plain}");
    assert!(sonde_proxy::session::is_compressed(&path));
    let s = Session::open(&path).unwrap();
    assert_eq!(s.records.len(), 500);
    // compressing again changes nothing
    sonde_proxy::session::compress(&path).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), packed);
}

#[test]
fn a_growing_session_is_read_in_steps_without_repeats() {
    use reqlog::Sink;
    use sonde_proxy::session::{Session, SessionHeader, SessionWriter, complete_len};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sonde");
    let w = SessionWriter::create(&path, &SessionHeader::new("e", "http://e", "r", None)).unwrap();
    let rec = |id| reqlog::Record {
        id,
        operation: "GetObject".into(),
        ..Default::default()
    };
    for id in 1..=3 {
        w.write(&rec(id));
    }
    w.flush().unwrap();
    // the offset is taken before the read, so it may lag behind it
    let at = complete_len(&path).unwrap();
    w.write(&rec(4));
    w.flush().unwrap();
    let mut s = Session::open(&path).unwrap();
    assert_eq!(s.records.len(), 4);
    let at = s.read_more(&path, at).unwrap();
    assert_eq!(s.records.len(), 4, "record 4 is not read twice");
    for id in 5..=7 {
        w.write(&rec(id));
    }
    w.flush().unwrap();
    // half a line at the end waits for the rest
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, b"{\"id\":8,"))
        .unwrap();
    let at2 = s.read_more(&path, at).unwrap();
    assert_eq!(
        s.records.iter().map(|r| r.id).collect::<Vec<_>>(),
        (1..=7).collect::<Vec<_>>()
    );
    assert_eq!(at2, std::fs::metadata(&path).unwrap().len() - 8);
}

#[test]
fn a_record_written_after_a_newer_one_is_still_read() {
    use reqlog::Sink;
    use sonde_proxy::session::{Session, SessionHeader, SessionWriter, complete_len};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sonde");
    let w = SessionWriter::create(&path, &SessionHeader::new("e", "http://e", "r", None)).unwrap();
    let rec = |id| reqlog::Record {
        id,
        operation: "GetObject".into(),
        ..Default::default()
    };
    for id in [1, 2, 4] {
        w.write(&rec(id));
    }
    w.flush().unwrap();
    let mut s = Session::open(&path).unwrap();
    let at = complete_len(&path).unwrap();
    // 3 finished after 4
    w.write(&rec(3));
    w.write(&rec(5));
    w.flush().unwrap();
    s.read_more(&path, at).unwrap();
    let mut ids: Vec<u64> = s.records.iter().map(|r| r.id).collect();
    ids.sort();
    assert_eq!(ids, vec![1, 2, 3, 4, 5]);
}
