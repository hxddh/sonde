//! 33.0's review: a run's file is redacted where it is written, so no
//! writer (an importer, a replay, one not written yet) can skip it.

use reqlog::{Header, Record, Sink};
use sonde_analysis::run::{RunFile, RunHeader, RunWriter};

/// What S3 answers a request signed wrong: it echoes the signature, the
/// string to sign and every signed header's value.
const MISMATCH: &str = "<Error><Code>SignatureDoesNotMatch</Code><AWSAccessKeyId>ASIAIOSFODNN7EXAMPLE</AWSAccessKeyId><StringToSign>AWS4-HMAC-SHA256\n20130524T000000Z\nscope\n7344ae5b7ee6c3e7</StringToSign><SignatureProvided>f0e8bdb87c964420</SignatureProvided><StringToSignBytes>41 57 53 34</StringToSignBytes><CanonicalRequest>GET\n/k\n\nx-amz-s3session-token:EXPRESSTOKEN\n\nhost\nUNSIGNED-PAYLOAD</CanonicalRequest><CanonicalRequestBytes>50 55 54 0a</CanonicalRequestBytes><RequestId>R1</RequestId></Error>";

const SECRETS: &[&str] = &[
    "ASIAIOSFODNN7EXAMPLE",
    "f0e8bdb87c964420",
    "7344ae5b7ee6c3e7",
    "41 57 53 34",
    "50 55 54 0a",
    "EXPRESSTOKEN",
    "SESSIONTOKEN",
    "deadbeefsignature",
    "sigv2secret",
    "c3NlLWMta2V5",
];

/// The text of a run's file, gzipped or not.
fn text_of(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).unwrap();
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        sonde_analysis::import::gunzip(&bytes, 1 << 30).unwrap()
    } else {
        bytes
    };
    String::from_utf8(bytes).unwrap()
}

fn holds_no_secret(text: &str) {
    for secret in SECRETS {
        assert!(
            !text.contains(secret),
            "{secret} in the run's file:\n{text}"
        );
    }
}

#[test]
fn the_writer_redacts_a_record_whoever_made_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.sonde");
    let mut r = Record {
        id: 1,
        method: "GET".into(),
        kind: "GetObject".into(),
        bucket: Some("b".into()),
        key: Some("k".into()),
        status: Some(403),
        ..Default::default()
    };
    r.rare_mut().url = "https://b.s3.amazonaws.com/k?X-Amz-Credential=ASIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Signature=deadbeefsignature&versionId=7".into();
    r.rare_mut().error_body = Some(MISMATCH.into());
    r.rare_mut().transport_error = Some(
        "error for url (https://h/k?AWSAccessKeyId=ASIAIOSFODNN7EXAMPLE&Signature=sigv2secret)"
            .into(),
    );
    r.lists_mut().query = vec![
        ("X-Amz-Signature".into(), "deadbeefsignature".into()),
        ("versionId".into(), "7".into()),
    ];
    r.lists_mut().request_headers = vec![
        Header::new(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=ASIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=deadbeefsignature",
        ),
        Header::new("x-amz-s3session-token", "EXPRESSTOKEN"),
        Header::new("x-amz-security-token", "SESSIONTOKEN"),
        Header::new("x-amz-server-side-encryption-customer-key", "c3NlLWMta2V5"),
        Header::new("range", "bytes=0-9"),
    ];
    assert!(!r.is_redacted());
    // one that holds no secret is written as it is
    let plain = Record {
        id: 2,
        method: "GET".into(),
        kind: "GetObject".into(),
        bucket: Some("b".into()),
        key: Some("k2".into()),
        status: Some(200),
        ..Default::default()
    };
    assert!(plain.is_redacted());
    let w = RunWriter::create(&path, &RunHeader::new("e", "https://e", "r", None)).unwrap();
    w.write(&r);
    w.write(&plain);
    w.flush().unwrap();
    drop(w);
    let text = text_of(&path);
    holds_no_secret(&text);
    let run = RunFile::open(&path).unwrap();
    // as the recording itself redacts one, and what is no secret is all there
    assert_eq!(run.records[0], r.clone().redacted());
    assert!(run.records[0].is_redacted());
    assert_eq!(run.records[1], plain);
    assert!(
        text.contains("versionId=7") && text.contains("bytes=0-9"),
        "{text}"
    );
    assert!(
        text.contains("<Code>SignatureDoesNotMatch</Code>"),
        "{text}"
    );
}

#[test]
fn an_imported_har_is_redacted_as_a_recording_is() {
    let har = serde_json::json!({"log": {"entries": [{
        "startedDateTime": "2026-11-09T10:00:00Z",
        "time": 5,
        "request": {
            "method": "GET",
            // (a presigned URL as a proxy or a script lowercased it, and SigV2's)
            "url": "https://lake.s3.amazonaws.com/k?x-amz-signature=deadbeefsignature&x-amz-security-token=SESSIONTOKEN&signature=sigv2secret&versionId=7",
            "headers": [
                {"name": "x-amz-date", "value": "20261109T100000Z"},
                {"name": "x-amz-s3session-token", "value": "EXPRESSTOKEN"},
            ],
        },
        "response": {
            "status": 403,
            "statusText": "Forbidden",
            "headers": [{"name": "x-amz-request-id", "value": "R1"}],
            "content": {"text": MISMATCH},
        },
    }]}})
    .to_string();
    let (source, run) = sonde_analysis::import::import(&har, "page").unwrap();
    assert_eq!(source, sonde_analysis::import::Source::Har);
    assert_eq!(run.records.len(), 1);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("page.sonde");
    sonde_analysis::import::save(&run, &path).unwrap();
    let saved = std::fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = text_of(&saved);
    holds_no_secret(&text);
    assert!(
        text.contains("<Code>SignatureDoesNotMatch</Code>"),
        "{text}"
    );
    assert!(text.contains("versionId"), "{text}");
}

#[test]
fn an_imported_trail_is_written_redacted_too() {
    // CloudTrail names no signature, but what it is given is written by the same writer
    let trail = r#"{"Records":[{"eventTime":"2026-02-06T00:00:01Z","eventSource":"s3.amazonaws.com","eventName":"GetObject","requestParameters":{"bucketName":"lake","key":"b","X-Amz-Signature":"deadbeefsignature","X-Amz-Security-Token":"SESSIONTOKEN"},"errorCode":"SignatureDoesNotMatch"}]}"#;
    let (_, run) = sonde_analysis::import::import(trail, "trail").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trail.sonde");
    sonde_analysis::import::save(&run, &path).unwrap();
    let saved = std::fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    holds_no_secret(&text_of(&saved));
}
