//! A request body that is not the one its headers said: cut short, never
//! ended, longer than said, or stopped part way. The program is answered
//! with S3's own 400, the store keeps nothing of it, and nothing waits on
//! the store for bytes that will never come. Each request is written by
//! hand on a socket, so its framing is the test's to get wrong, and each
//! test ends within [`BOUND`]: a hang fails, it does not stall.

mod common;

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use common::MiniS3;
use reqlog::RequestLog;
use s3raw::sigv4::{self, SignInput};
use s3raw::{ClientConfig, Credentials, S3Client, StaticProvider};
use sonde_proxy::{Proxy, ProxyConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// The longest a test may take.
const BOUND: Duration = Duration::from_secs(30);

/// The idle limit of a proxy that is to give up on a body that stopped.
const IDLE: Duration = Duration::from_millis(400);

const SIGNED: &str = "STREAMING-AWS4-HMAC-SHA256-PAYLOAD";
const SIGNED_TRAILER: &str = "STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER";
const UNSIGNED_TRAILER: &str = "STREAMING-UNSIGNED-PAYLOAD-TRAILER";

async fn bounded<T>(what: impl Future<Output = T>) -> T {
    tokio::time::timeout(BOUND, what)
        .await
        .expect("the exchange hangs")
}

/// A store, and a proxy in front of it that waits `idle` for a byte.
async fn setup(idle: Option<Duration>) -> (MiniS3, Proxy, RequestLog) {
    let store = MiniS3::start(true).await;
    let mut cfg = ClientConfig::new("target", &store.url, "eu-west-1").unwrap();
    if let Some(idle) = idle {
        cfg.timeout = idle;
    }
    let target = S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(
            "AKID", "secret", None,
        )))),
        RequestLog::default(),
    )
    .unwrap();
    let log = RequestLog::default();
    let proxy = Proxy::start(target, log.clone(), ProxyConfig::default())
        .await
        .unwrap();
    (store, proxy, log)
}

/// A request's head, signed with the keys the proxy gave: `extra` headers
/// go unsigned, as written.
fn head(proxy: &Proxy, method: &str, target: &str, sha: &str, extra: &[(&str, String)]) -> String {
    let (key, secret) = proxy.client_keys();
    let now = Utc::now();
    let date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers = vec![
        ("host".to_string(), proxy.addr().to_string()),
        ("x-amz-content-sha256".to_string(), sha.to_string()),
        ("x-amz-date".to_string(), date.clone()),
    ];
    let sig = sigv4::sign(
        &SignInput {
            method,
            canonical_uri: path,
            query: &sonde_proxy::parse_query(Some(query)),
            headers: &headers,
            payload_hash: sha,
            region: "us-east-1",
            service: "s3",
            time: now,
        },
        &Credentials::new(key, secret, None),
    );
    let mut head = format!(
        "{method} {target} HTTP/1.1\r\nhost: {}\r\nx-amz-date: {date}\r\nx-amz-content-sha256: {sha}\r\nauthorization: {}\r\nconnection: close\r\n",
        proxy.addr(),
        sig.authorization(key)
    );
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head + "\r\n"
}

/// What the program does once it has written what it will write.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Then {
    /// Reads the answer, its connection open.
    Waits,
    /// Closes its side, and reads the answer if one comes.
    Closes,
    /// Goes: the connection is dropped.
    Goes,
}

/// Writes `bytes` to the proxy, and gives the answer read to its end.
async fn exchange(proxy: &Proxy, bytes: &[u8], then: Then) -> String {
    let mut s = TcpStream::connect(proxy.addr()).await.unwrap();
    s.write_all(bytes).await.unwrap();
    match then {
        Then::Goes => return String::new(),
        Then::Closes => s.shutdown().await.unwrap(),
        Then::Waits => {}
    }
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out).await;
    String::from_utf8_lossy(&out).into_owned()
}

/// `payload` framed as `aws-chunked`, in chunks of 64 KiB: signed chunks
/// or not, the last (empty) chunk and the trailer when `ended`.
fn framed(payload: &[u8], sha: &str, ended: bool) -> Vec<u8> {
    let sig = if sha == UNSIGNED_TRAILER {
        String::new()
    } else {
        format!(";chunk-signature={}", "0".repeat(64))
    };
    let mut body = Vec::new();
    for c in payload.chunks(64 << 10) {
        body.extend(format!("{:x}{sig}\r\n", c.len()).as_bytes());
        body.extend(c);
        body.extend(b"\r\n");
    }
    if ended {
        body.extend(end_of(sha));
    }
    body
}

/// The last chunk and the trailer of a body of `sha`'s kind.
fn end_of(sha: &str) -> Vec<u8> {
    let sig = "0".repeat(64);
    match sha {
        SIGNED => format!("0;chunk-signature={sig}\r\n\r\n"),
        SIGNED_TRAILER => format!(
            "0;chunk-signature={sig}\r\nx-amz-checksum-crc32:AAAAAA==\r\nx-amz-trailer-signature:{sig}\r\n\r\n"
        ),
        _ => "0\r\nx-amz-checksum-crc32:AAAAAA==\r\n\r\n".to_string(),
    }
    .into_bytes()
}

/// An `aws-chunked` PUT of `key`: its payload said to be `declared` bytes,
/// its body `body`, said to be `length` bytes long.
fn put(
    proxy: &Proxy,
    key: &str,
    sha: &str,
    declared: usize,
    body: &[u8],
    length: usize,
) -> Vec<u8> {
    let mut extra = vec![
        ("content-encoding", "aws-chunked".to_string()),
        ("x-amz-decoded-content-length", declared.to_string()),
        ("content-length", length.to_string()),
    ];
    if sha != SIGNED {
        extra.push(("x-amz-trailer", "x-amz-checksum-crc32".to_string()));
    }
    let mut req = head(proxy, "PUT", &format!("/data/{key}"), sha, &extra).into_bytes();
    req.extend(body);
    req
}

fn payload(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

/// The answer is S3's error `code`, a 400 of sonde's.
#[track_caller]
fn refused(answer: &str, code: &str, what: &str) {
    assert!(answer.starts_with("HTTP/1.1 400"), "{what}: {answer}");
    assert!(
        answer.contains(&format!("<Code>{code}</Code>")),
        "{what}: {answer}"
    );
}

/// The record of the request for `key`, once it is there.
async fn record_of(log: &RequestLog, key: &str) -> reqlog::Record {
    let mut rx = log.subscribe();
    loop {
        if let Some(r) = log
            .snapshot()
            .into_iter()
            .find(|r| r.key.as_deref() == Some(key))
        {
            return r;
        }
        let _ = rx.recv().await;
    }
}

const KINDS: [(&str, &str); 3] = [
    ("signed", SIGNED),
    ("trailed", SIGNED_TRAILER),
    ("unsigned", UNSIGNED_TRAILER),
];

/// The payload said and the payload sent are the same, or the upload is
/// refused: fewer bytes are a body cut short, more are not the request the
/// program signed. Before 33.0 a signed body with more went to the store cut
/// to the length said and was answered 200, and an unsigned one with fewer
/// went as it was.
#[tokio::test]
async fn an_aws_chunked_payload_is_as_long_as_it_said_or_no_object_is_made() {
    bounded(async {
        let (store, proxy, log) = setup(None).await;
        let said = 300 << 10;
        for (kind, sha) in KINDS {
            // as said: it goes through
            let key = format!("{kind}-exact");
            let body = framed(&payload(said), sha, true);
            let answer = exchange(
                &proxy,
                &put(&proxy, &key, sha, said, &body, body.len()),
                Then::Waits,
            )
            .await;
            assert!(answer.starts_with("HTTP/1.1 200"), "{key}: {answer}");
            assert!(store.keys("data").contains(&key), "{key}");
            assert_eq!(record_of(&log, &key).await.request_bytes, Some(said as u64));

            for (how, sent, code) in [
                ("fewer", said - 5, "IncompleteBody"),
                ("more", said + 5, "InvalidRequest"),
                // (a whole chunk more, and one byte in all)
                ("far-more", said + (200 << 10), "InvalidRequest"),
                ("one", 1, "IncompleteBody"),
            ] {
                let key = format!("{kind}-{how}");
                let body = framed(&payload(sent), sha, true);
                let answer = exchange(
                    &proxy,
                    &put(&proxy, &key, sha, said, &body, body.len()),
                    Then::Waits,
                )
                .await;
                refused(&answer, code, &key);
                assert!(
                    answer.contains("x-amz-decoded-content-length"),
                    "{key}: {answer}"
                );
                assert!(
                    !store.keys("data").contains(&key),
                    "{key}: an object was made"
                );
                let r = record_of(&log, &key).await;
                assert!(r.rare().transport_error.is_some(), "{key}: {r:?}");
            }
        }
    })
    .await;
}

/// A body whose last chunk never comes: the program sent the whole payload
/// and then nothing. The proxy gives up after its idle limit with S3's
/// `RequestTimeout`, and the store, which was kept waiting for the body's
/// last bytes, has made no object. Before 33.0 a signed body's payload had
/// all gone on by then, and the object was made of an upload never ended.
#[tokio::test]
async fn a_body_whose_last_chunk_never_comes_is_given_up_on_and_makes_no_object() {
    bounded(async {
        let (store, proxy, log) = setup(Some(IDLE)).await;
        let said = 100 << 10;
        for (kind, sha) in KINDS {
            let key = format!("{kind}-open");
            let body = framed(&payload(said), sha, false);
            let length = body.len() + end_of(sha).len();
            let answer = exchange(
                &proxy,
                &put(&proxy, &key, sha, said, &body, length),
                Then::Waits,
            )
            .await;
            refused(&answer, "RequestTimeout", &key);
            assert!(
                !store.keys("data").contains(&key),
                "{key}: an object was made"
            );
            let r = record_of(&log, &key).await;
            assert!(
                r.rare()
                    .transport_error
                    .as_deref()
                    .is_some_and(|e| e.contains("sent no more of it")),
                "{key}: {r:?}"
            );
        }
    })
    .await;
}

/// The program goes in the middle of its body (its payload sent, the last
/// chunk not), or closes its side: no object is made, and the request is
/// recorded as one that failed.
#[tokio::test]
async fn a_program_that_goes_mid_body_makes_no_object() {
    bounded(async {
        let (store, proxy, log) = setup(None).await;
        let said = 100 << 10;
        for (kind, sha) in KINDS {
            for then in [Then::Goes, Then::Closes] {
                let key = format!("{kind}-{then:?}");
                let body = framed(&payload(said), sha, false);
                let length = body.len() + end_of(sha).len();
                let answer =
                    exchange(&proxy, &put(&proxy, &key, sha, said, &body, length), then).await;
                // a program still reading is told
                if !answer.is_empty() {
                    refused(&answer, "IncompleteBody", &key);
                }
                let r = record_of(&log, &key).await;
                assert!(r.rare().transport_error.is_some(), "{key}: {r:?}");
                assert!(!(200..300).contains(&r.status.unwrap_or(0)), "{key}: {r:?}");
                assert!(
                    !store.keys("data").contains(&key),
                    "{key}: an object was made"
                );
            }
        }
    })
    .await;
}

/// A plain body shorter than its `Content-Length`: the program closes its
/// side, or sends nothing more. Either way it is answered (once, with the
/// idle limit's `RequestTimeout` when it only stopped) and nothing is made.
#[tokio::test]
async fn a_plain_body_shorter_than_its_content_length_is_answered_and_makes_no_object() {
    bounded(async {
        let (store, proxy, log) = setup(Some(IDLE)).await;
        for (key, then, code) in [
            ("plain-closed", Then::Closes, "IncompleteBody"),
            ("plain-open", Then::Waits, "RequestTimeout"),
        ] {
            let extra = [("content-length", "100".to_string())];
            let mut req = head(
                &proxy,
                "PUT",
                &format!("/data/{key}"),
                "UNSIGNED-PAYLOAD",
                &extra,
            )
            .into_bytes();
            req.extend([b'x'; 50]);
            let answer = exchange(&proxy, &req, then).await;
            if !answer.is_empty() {
                refused(&answer, code, key);
            }
            assert!(
                then == Then::Closes || !answer.is_empty(),
                "{key}: no answer"
            );
            let r = record_of(&log, key).await;
            assert!(r.rare().transport_error.is_some(), "{key}: {r:?}");
            assert!(!store.keys("data").contains(&key.to_string()), "{key}");
        }
    })
    .await;
}

/// A body sonde reads whole before it sends anything (a control document,
/// a body with no length) and that stops part way: before 33.0 the request
/// waited for the rest for as long as the program kept its connection, and
/// left no record. It is answered after the idle limit.
#[tokio::test]
async fn a_body_read_whole_that_stops_part_way_is_answered_not_waited_for() {
    bounded(async {
        let (store, proxy, log) = setup(Some(IDLE)).await;
        // DeleteObjects, 100 bytes said, 50 sent
        let extra = [("content-length", "100".to_string())];
        let mut req = head(&proxy, "POST", "/data?delete", "UNSIGNED-PAYLOAD", &extra).into_bytes();
        req.extend([b'x'; 50]);
        let answer = exchange(&proxy, &req, Then::Waits).await;
        refused(&answer, "RequestTimeout", "DeleteObjects");
        // a PUT sent chunked, its last chunk never sent
        let extra = [("transfer-encoding", "chunked".to_string())];
        let mut req = head(&proxy, "PUT", "/data/te-open", "UNSIGNED-PAYLOAD", &extra).into_bytes();
        req.extend(b"5\r\nhello\r\n");
        let answer = exchange(&proxy, &req, Then::Waits).await;
        refused(&answer, "RequestTimeout", "chunked");
        assert!(store.sent("POST").is_empty() && store.sent("PUT").is_empty());
        let records = log.snapshot();
        assert_eq!(records.len(), 2, "{records:?}");
        assert!(records.iter().all(|r| r.rare().transport_error.is_some()));
    })
    .await;
}

/// A signed body read whole (its trailer is none sonde knows the length of)
/// is held to the payload it said too.
#[tokio::test]
async fn a_signed_body_read_whole_is_as_long_as_it_said() {
    bounded(async {
        let (store, proxy, _log) = setup(None).await;
        let sig = "0".repeat(64);
        for (key, sent, status) in [("whole-exact", 10, "200"), ("whole-fewer", 7, "400"), ("whole-more", 12, "400")] {
            let data = &b"0123456789ab"[..sent];
            let body = format!(
                "{sent:x};chunk-signature={sig}\r\n{}\r\n0;chunk-signature={sig}\r\nx-custom:1\r\nx-amz-trailer-signature:{sig}\r\n\r\n",
                std::str::from_utf8(data).unwrap()
            );
            let extra = [
                ("content-encoding", "aws-chunked".to_string()),
                ("x-amz-decoded-content-length", "10".to_string()),
                ("x-amz-trailer", "x-custom".to_string()),
                ("content-length", body.len().to_string()),
            ];
            let mut req =
                head(&proxy, "PUT", &format!("/data/{key}"), SIGNED_TRAILER, &extra).into_bytes();
            req.extend(body.as_bytes());
            let answer = exchange(&proxy, &req, Then::Waits).await;
            assert!(answer.starts_with(&format!("HTTP/1.1 {status}")), "{key}: {answer}");
            assert_eq!(store.keys("data").contains(&key.to_string()), status == "200", "{key}");
        }
    })
    .await;
}

/// A signed body that says it carries nothing (`x-amz-decoded-content-length:
/// 0`) is held to its framing too: the store, told a length of 0, would never
/// read it, so it is read whole first. Its last chunk, and nothing past it,
/// goes through as an empty object; anything else is refused and makes
/// none. Before 34.1 any body at all went through as an empty object.
#[tokio::test]
async fn a_signed_body_of_no_payload_is_held_to_its_framing() {
    bounded(async {
        let (store, proxy, _log) = setup(None).await;
        let sig = "0".repeat(64);
        let last = format!("0;chunk-signature={sig}\r\n\r\n");
        for (key, body, code) in [
            ("empty-exact", last.clone(), None),
            // the last chunk with no blank line after it, as SDKs end
            ("empty-bare", format!("0;chunk-signature={sig}\r\n"), None),
            (
                "empty-garbled",
                "zz;chunk-signature=x\r\n\r\n".to_string(),
                Some("InvalidRequest"),
            ),
            (
                "empty-cut",
                format!("0;chunk-signature={}", &sig[..10]),
                Some("IncompleteBody"),
            ),
            ("empty-none", String::new(), Some("IncompleteBody")),
            (
                "empty-more",
                format!("5;chunk-signature={sig}\r\nhello\r\n{last}"),
                Some("InvalidRequest"),
            ),
        ] {
            let answer = exchange(
                &proxy,
                &put(&proxy, key, SIGNED, 0, body.as_bytes(), body.len()),
                Then::Waits,
            )
            .await;
            match code {
                None => {
                    assert!(answer.starts_with("HTTP/1.1 200"), "{key}: {answer}");
                    assert!(store.keys("data").contains(&key.to_string()), "{key}");
                }
                Some(code) => {
                    refused(&answer, code, key);
                    assert!(
                        !store.keys("data").contains(&key.to_string()),
                        "{key}: an object was made"
                    );
                }
            }
        }
    })
    .await;
}
