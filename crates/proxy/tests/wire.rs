//! A key on the wire: what the program wrote in its request's path reaches
//! the store byte for byte (an encoded slash, a `%25`, a plus, bytes that
//! are no UTF-8, empty and dot segments), signed as a store reads it, and
//! the record holds the key it names. The store here is a socket that keeps
//! each request's target as it was written, and checks its SigV4 signature
//! the way a store does: over the path decoded, then encoded once.

use std::sync::Arc;
use std::time::Duration;

use chrono::{NaiveDateTime, TimeZone, Utc};
use parking_lot::Mutex;
use reqlog::RequestLog;
use s3raw::sigv4::{self, SignInput};
use s3raw::{ClientConfig, Credentials, S3Client, StaticProvider};
use sonde_proxy::{Policy, Proxy, ProxyConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const REAL_KEY: &str = "AKIDREALKEY";
const REAL_SECRET: &str = "real-secret-only-sonde-knows";

/// A request as the store was sent it.
#[derive(Clone, Debug)]
struct Seen {
    method: String,
    /// The request target, as written.
    target: String,
    headers: Vec<(String, String)>,
}

impl Seen {
    fn header(&self, name: &str) -> &str {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map_or("", |(_, v)| v.as_str())
    }
}

/// The canonical URI a store makes of a request's path: every escape read
/// as its byte, then each byte outside the unreserved set (and `/`) written
/// `%XX`. Written here apart from sonde's own.
fn as_a_store_reads(path: &str) -> String {
    percent_encoding::percent_decode_str(path)
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// Checks a request's SigV4 signature against the real secret.
fn verify(seen: &Seen) -> Result<(), String> {
    let auth = seen.header("authorization");
    let field = |name: &str| {
        auth.split([',', ' '])
            .find_map(|p| p.trim().strip_prefix(name))
            .unwrap_or("")
            .to_string()
    };
    let credential = field("Credential=");
    let mut scope = credential.split('/');
    if scope.next() != Some(REAL_KEY) {
        return Err(format!("signed with {credential:?}"));
    }
    let region = scope.nth(1).unwrap_or("").to_string();
    let time = Utc.from_utc_datetime(
        &NaiveDateTime::parse_from_str(seen.header("x-amz-date"), "%Y%m%dT%H%M%SZ")
            .map_err(|e| e.to_string())?,
    );
    let headers: Vec<(String, String)> = field("SignedHeaders=")
        .split(';')
        .map(|h| (h.to_string(), seen.header(h).to_string()))
        .collect();
    let (path, query) = seen.target.split_once('?').unwrap_or((&seen.target, ""));
    let sig = sigv4::sign(
        &SignInput {
            method: &seen.method,
            canonical_uri: &as_a_store_reads(path),
            query: &sonde_proxy::parse_query(Some(query)),
            headers: &headers,
            payload_hash: seen.header("x-amz-content-sha256"),
            region: &region,
            service: "s3",
            time,
        },
        &Credentials::new(REAL_KEY, REAL_SECRET, None),
    );
    if sig.signature == field("Signature=") {
        Ok(())
    } else {
        Err(format!(
            "bad signature; canonical request:\n{}",
            sig.canonical_request
        ))
    }
}

/// A store that keeps what it is sent and answers 200.
async fn store() -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let kept = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            let kept = kept.clone();
            tokio::spawn(async move {
                let mut got = Vec::new();
                let mut buf = [0u8; 4096];
                let end = loop {
                    if let Some(i) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i;
                    }
                    match s.read(&mut buf).await {
                        Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
                        _ => return,
                    }
                };
                // (a request line is bytes: one that is no UTF-8 would show)
                let head = String::from_utf8(got[..end].to_vec()).expect("the head, as text");
                let mut lines = head.split("\r\n");
                let mut line = lines.next().unwrap().splitn(3, ' ');
                let method = line.next().unwrap().to_string();
                let target = line.next().unwrap().to_string();
                let headers: Vec<(String, String)> = lines
                    .filter_map(|l| l.split_once(':'))
                    .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                    .collect();
                let seen = Seen {
                    method,
                    target,
                    headers,
                };
                let length: usize = seen.header("content-length").parse().unwrap_or(0);
                let mut body = got.len() - (end + 4);
                while body < length {
                    match s.read(&mut buf).await {
                        Ok(n) if n > 0 => body += n,
                        _ => return,
                    }
                }
                kept.lock().push(seen);
                let _ = s
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    .await;
            });
        }
    });
    (format!("http://{addr}"), seen)
}

async fn setup() -> (Proxy, RequestLog, String, Arc<Mutex<Vec<Seen>>>) {
    setup_under(Policy::Allow).await
}

async fn setup_under(policy: Policy) -> (Proxy, RequestLog, String, Arc<Mutex<Vec<Seen>>>) {
    let (url, seen) = store().await;
    let cfg = ClientConfig::new("target", &url, "eu-west-1").unwrap();
    let target = S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(
            REAL_KEY,
            REAL_SECRET,
            None,
        )))),
        RequestLog::default(),
    )
    .unwrap();
    let log = RequestLog::default();
    let proxy = Proxy::start(
        target,
        log.clone(),
        ProxyConfig {
            policy,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    (proxy, log, url, seen)
}

/// Sends `method target` to the proxy on a socket, as written, signed with
/// the keys the proxy gave (over the path as written, as an SDK signs), to
/// the Host `host`; a PUT carries one byte.
async fn send(proxy: &Proxy, host: &str, method: &str, target: &str) -> String {
    let body: &[u8] = if method == "PUT" { b"x" } else { b"" };
    send_with(proxy, host, method, target, "UNSIGNED-PAYLOAD", &[], body).await
}

/// [`send`], with the payload hash `sha`, `extra` headers (unsigned, as
/// written) and `body`.
async fn send_with(
    proxy: &Proxy,
    host: &str,
    method: &str,
    target: &str,
    sha: &str,
    extra: &[(&str, String)],
    body: &[u8],
) -> String {
    let (key, secret) = proxy.client_keys();
    let now = Utc::now();
    let date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let headers = vec![
        ("host".to_string(), host.to_string()),
        ("x-amz-content-sha256".to_string(), sha.to_string()),
        ("x-amz-date".to_string(), date.clone()),
    ];
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
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
    let mut req = format!(
        "{method} {target} HTTP/1.1\r\nhost: {host}\r\nx-amz-date: {date}\r\nx-amz-content-sha256: {sha}\r\nauthorization: {}\r\ncontent-length: {}\r\nconnection: close\r\n",
        sig.authorization(key),
        body.len()
    );
    for (k, v) in extra {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    let mut req = req.into_bytes();
    req.extend_from_slice(body);
    let exchange = async {
        let mut s = TcpStream::connect(proxy.addr()).await.unwrap();
        s.write_all(&req).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    };
    tokio::time::timeout(Duration::from_secs(30), exchange)
        .await
        .expect("the exchange hangs")
}

/// Paths a program may write, and the key each names.
const KEYS: &[(&str, &str)] = &[
    // an encoded slash is a slash of the key, and stays encoded on the wire
    ("a%2Fb", "a/b"),
    ("a%2fb%2F", "a/b/"),
    ("100%25", "100%"),
    ("%2525", "%25"),
    ("a+b", "a+b"),
    ("a%2Bb", "a+b"),
    ("a%20b", "a b"),
    // bytes that are no UTF-8: no other key is named in their place
    ("%FF%fe/x", "%FF%FE/x"),
    ("caf%E9", "caf%E9"),
    ("%E6%97%A5%e6%9c%ac", "日本"),
    // empty segments, at either end too
    ("a//b", "a//b"),
    ("/a/", "/a/"),
    ("a///", "a///"),
    // dot segments, written either way
    ("a/../b/./c", "a/../b/./c"),
    ("a%2Fb/%2e%2E/x", "a/b/../x"),
    ("..", ".."),
    ("a/.", "a/."),
    // what a URL library would write otherwise
    ("x\\y|z^", "x\\y|z^"),
    ("q(1)*'!~", "q(1)*'!~"),
];

/// Path-style, as a recorded program sends: each key reaches the store as
/// it was written, with a signature the store accepts, and is recorded as
/// the key it names. Before 33.0 the path was decoded and encoded again:
/// `a%2Fb` reached the store as `a/b`, `a+b` as `a%2Bb`, a dot segment as
/// `%2E`, and `%FF` as `%EF%BF%BD`, another key.
#[tokio::test]
async fn a_key_reaches_the_store_as_the_program_wrote_it() {
    let (proxy, log, url, seen) = setup().await;
    let host = proxy.addr().to_string();
    for (written, key) in KEYS {
        for method in ["PUT", "GET", "DELETE"] {
            let target = format!("/data/{written}");
            let answer = send(&proxy, &host, method, &target).await;
            assert!(
                answer.starts_with("HTTP/1.1 200"),
                "{method} {target}: {answer}"
            );
            let got = seen.lock().last().cloned().expect("it reached the store");
            assert_eq!(
                (got.method.as_str(), got.target.as_str()),
                (method, target.as_str())
            );
            verify(&got).unwrap_or_else(|e| panic!("{method} {target}: {e}"));
            let r = &log.list(&reqlog::Filter::default(), 1)[0];
            assert_eq!(r.bucket.as_deref(), Some("data"), "{target}");
            assert_eq!(r.key.as_deref(), Some(*key), "{target}");
            assert_eq!(r.rare().url, format!("{url}{target}"));
        }
    }
    assert_eq!(seen.lock().len(), KEYS.len() * 3);
}

/// Virtual-hosted style, as a program told so sends (`data.localhost`): the
/// whole path is the key, a leading slash and all.
#[tokio::test]
async fn a_key_under_a_bucket_in_the_host_reaches_the_store_as_written() {
    let (proxy, log, _url, seen) = setup().await;
    let host = format!("data.localhost:{}", proxy.addr().port());
    for (written, key) in KEYS {
        let answer = send(&proxy, &host, "PUT", &format!("/{written}")).await;
        assert!(answer.starts_with("HTTP/1.1 200"), "{written}: {answer}");
        let got = seen.lock().last().cloned().expect("it reached the store");
        assert_eq!(got.target, format!("/data/{written}"));
        verify(&got).unwrap_or_else(|e| panic!("{written}: {e}"));
        let r = &log.list(&reqlog::Filter::default(), 1)[0];
        assert_eq!(
            (r.bucket.as_deref(), r.key.as_deref()),
            (Some("data"), Some(*key))
        );
    }
}

/// A query goes with it as before: the path's fidelity is the path's.
#[tokio::test]
async fn a_key_written_its_own_way_keeps_its_query() {
    let (proxy, _log, _url, seen) = setup().await;
    let host = proxy.addr().to_string();
    let (key, secret) = proxy.client_keys();
    let now = Utc::now();
    let date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let query = vec![("versionId".to_string(), "v 1/+".to_string())];
    let headers = vec![
        ("host".to_string(), host.clone()),
        (
            "x-amz-content-sha256".to_string(),
            "UNSIGNED-PAYLOAD".into(),
        ),
        ("x-amz-date".to_string(), date.clone()),
    ];
    let sig = sigv4::sign(
        &SignInput {
            method: "GET",
            canonical_uri: "/data/a%2Fb/../c",
            query: &query,
            headers: &headers,
            payload_hash: "UNSIGNED-PAYLOAD",
            region: "us-east-1",
            service: "s3",
            time: now,
        },
        &Credentials::new(key, secret, None),
    );
    let req = format!(
        "GET /data/a%2Fb/../c?versionId=v%201%2F%2B HTTP/1.1\r\nhost: {host}\r\nx-amz-date: {date}\r\nx-amz-content-sha256: UNSIGNED-PAYLOAD\r\nauthorization: {}\r\nconnection: close\r\n\r\n",
        sig.authorization(key)
    );
    let exchange = async {
        let mut s = TcpStream::connect(proxy.addr()).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    };
    let answer = tokio::time::timeout(Duration::from_secs(30), exchange)
        .await
        .expect("the exchange hangs");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let got = seen.lock().last().cloned().unwrap();
    assert_eq!(got.target, "/data/a%2Fb/../c?versionId=v%201%2F%2B");
    verify(&got).unwrap();
}

/// Under no-delete, a key that is not UTF-8 (`%FF`) and the key of text
/// its record reads as (`%25FF`, the three characters `%FF`) are two keys:
/// what the run made of one is not the other's to delete. Before 34.1 a
/// create of either let the other be deleted.
#[tokio::test]
async fn a_key_of_bytes_and_the_text_its_record_reads_as_are_two_keys_to_no_delete() {
    let (proxy, _log, _url, seen) = setup_under(Policy::NoDelete).await;
    let host = proxy.addr().to_string();
    let create = [("if-none-match", "*".to_string())];
    let send = |method, target, extra| {
        let body: &[u8] = if method == "PUT" { b"x" } else { b"" };
        send_with(
            &proxy,
            &host,
            method,
            target,
            "UNSIGNED-PAYLOAD",
            extra,
            body,
        )
    };
    for (made, other) in [("/data/%FF", "/data/%25FF"), ("/data/%25FE", "/data/%FE")] {
        let answer = send("PUT", made, &create).await;
        assert!(answer.starts_with("HTTP/1.1 200"), "{made}: {answer}");
        let sent = seen.lock().len();
        let answer = send("DELETE", other, &[]).await;
        assert!(answer.starts_with("HTTP/1.1 403"), "{other}: {answer}");
        assert!(answer.contains("no-delete"), "{answer}");
        assert_eq!(
            seen.lock().len(),
            sent,
            "{other}: no delete reached the store"
        );
    }
    // a key of text the run created is its own, as before
    let answer = send("DELETE", "/data/%25FE", &[]).await;
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
}

/// A signed `aws-chunked` body read whole (a control document) sends on as
/// headers only the checksums its `x-amz-trailer` names: a trailer that sets
/// a governance retention aside, or names a header sonde sets, is never
/// judged as a header is, and goes nowhere. Before 34.1 every trailer went
/// on, `x-amz-bypass-governance-retention` signed with the store's keys.
#[tokio::test]
async fn a_trailer_of_a_body_read_whole_cannot_carry_a_header_past_no_delete() {
    let (proxy, _log, _url, seen) = setup_under(Policy::NoDelete).await;
    let host = proxy.addr().to_string();
    let sha = "STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER";
    let sig = "0".repeat(64);
    let framed = |doc: &str, trailers: &str| {
        format!(
            "{:x};chunk-signature={sig}\r\n{doc}\r\n0;chunk-signature={sig}\r\n{trailers}x-amz-trailer-signature:{sig}\r\n\r\n",
            doc.len()
        )
        .into_bytes()
    };
    let smuggled = "x-amz-checksum-crc32:AAAAAA==\r\nx-amz-bypass-governance-retention:true\r\nhost:elsewhere.example\r\ncontent-length:999\r\nx-amz-meta-extra:1\r\n";
    let headers = |doc: &str, named: &str| {
        vec![
            ("content-encoding", "aws-chunked".to_string()),
            ("x-amz-decoded-content-length", doc.len().to_string()),
            ("x-amz-trailer", named.to_string()),
        ]
    };
    // what the run makes, for a batch of deletes of its own
    let create = [("if-none-match", "*".to_string())];
    let answer = send_with(
        &proxy,
        &host,
        "PUT",
        "/data/mine",
        "UNSIGNED-PAYLOAD",
        &create,
        b"x",
    )
    .await;
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let retention = "<Retention><Mode>GOVERNANCE</Mode></Retention>";
    let batch = "<Delete><Object><Key>mine</Key></Object></Delete>";
    for (target, doc, named) in [
        ("/data/old?retention", retention, "x-amz-checksum-crc32"),
        // named, and still no checksum: not sent on
        (
            "/data/old?retention",
            retention,
            "x-amz-checksum-crc32,x-amz-bypass-governance-retention,host",
        ),
        ("/data?delete", batch, "x-amz-checksum-crc32"),
    ] {
        let method = if target.ends_with("delete") {
            "POST"
        } else {
            "PUT"
        };
        let body = framed(doc, smuggled);
        let answer = send_with(
            &proxy,
            &host,
            method,
            target,
            sha,
            &headers(doc, named),
            &body,
        )
        .await;
        assert!(
            answer.starts_with("HTTP/1.1 200"),
            "{target} {named}: {answer}"
        );
        let got = seen.lock().last().cloned().expect("it reached the store");
        let sub = target.split('?').nth(1).unwrap();
        assert!(got.target.contains(sub), "{}", got.target);
        assert_eq!(
            got.header("x-amz-bypass-governance-retention"),
            "",
            "{target}: {got:?}"
        );
        assert_eq!(got.header("x-amz-meta-extra"), "", "{target}: {got:?}");
        assert_eq!(
            got.header("x-amz-checksum-crc32"),
            "AAAAAA==",
            "{target}: {got:?}"
        );
        assert_eq!(
            got.header("content-length"),
            doc.len().to_string(),
            "{got:?}"
        );
        assert!(!got.header("host").contains("elsewhere"), "{got:?}");
        verify(&got).unwrap_or_else(|e| panic!("{target}: {e}"));
    }
    // a checksum it names and does not send is a body not as said
    let sent = seen.lock().len();
    let body = framed(retention, "");
    let answer = send_with(
        &proxy,
        &host,
        "PUT",
        "/data/old?retention",
        sha,
        &headers(retention, "x-amz-checksum-crc32"),
        &body,
    )
    .await;
    assert!(answer.starts_with("HTTP/1.1 400"), "{answer}");
    assert!(answer.contains("<Code>InvalidRequest</Code>"), "{answer}");
    assert_eq!(seen.lock().len(), sent, "it did not reach the store");
}
