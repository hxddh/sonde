//! The proxy end to end: an application signing with a dummy key, the proxy,
//! and an upstream that checks every SigV4 signature against the real secret.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use chrono::{NaiveDateTime, TimeZone, Utc};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use parking_lot::Mutex;
use reqlog::RequestLog;
use s3raw::sigv4::{self, SignInput};
use s3raw::{Body, ClientConfig, Credentials, S3Client, S3Request, StaticProvider};
use sonde_proxy::{Policy, Proxy, ProxyConfig};
use tokio::net::TcpListener;

const REAL_KEY: &str = "AKIDREALKEY";
const REAL_SECRET: &str = "real-secret-only-sonde-knows";

#[derive(Default)]
struct Upstream {
    objects: Mutex<HashMap<String, (Vec<u8>, Vec<(String, String)>)>>,
    calls: Mutex<Vec<String>>,
}

/// Verifies the request's SigV4 signature; `Err` says why it is wrong.
fn verify(req: &http::request::Parts, body_hash: &str) -> Result<(), String> {
    let auth = req
        .headers
        .get("authorization")
        .ok_or("no authorization")?
        .to_str()
        .unwrap();
    let field = |name: &str| {
        auth.split([',', ' '])
            .find_map(|p| p.trim().strip_prefix(name))
            .unwrap_or("")
            .to_string()
    };
    let credential = field("Credential=");
    let signed = field("SignedHeaders=");
    let signature = field("Signature=");
    let mut scope = credential.split('/');
    let key = scope.next().unwrap_or("");
    if key != REAL_KEY {
        return Err(format!("signed with key {key:?}"));
    }
    let _date = scope.next();
    let region = scope.next().unwrap_or("").to_string();
    let amz_date = req.headers["x-amz-date"].to_str().unwrap();
    let time =
        Utc.from_utc_datetime(&NaiveDateTime::parse_from_str(amz_date, "%Y%m%dT%H%M%SZ").unwrap());
    let headers: Vec<(String, String)> = signed
        .split(';')
        .map(|h| {
            (
                h.to_string(),
                req.headers
                    .get(h)
                    .map(|v| v.to_str().unwrap().to_string())
                    .unwrap_or_default(),
            )
        })
        .collect();
    let query = sonde_proxy::parse_query(req.uri.query());
    let payload = req.headers["x-amz-content-sha256"].to_str().unwrap();
    if payload != body_hash && payload != sigv4::UNSIGNED_PAYLOAD {
        return Err(format!(
            "payload hash {payload} but the body hashes to {body_hash}"
        ));
    }
    let creds = Credentials::new(REAL_KEY, REAL_SECRET, None);
    let sig = sigv4::sign(
        &SignInput {
            method: req.method.as_str(),
            canonical_uri: req.uri.path(),
            query: &query,
            headers: &headers,
            payload_hash: payload,
            region: &region,
            service: "s3",
            time,
        },
        &creds,
    );
    if sig.signature == signature {
        Ok(())
    } else {
        Err(format!(
            "bad signature; canonical request:\n{}",
            sig.canonical_request
        ))
    }
}

async fn serve(
    up: Arc<Upstream>,
    req: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let (parts, body) = req.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    up.calls
        .lock()
        .push(format!("{} {}", parts.method, parts.uri));
    if let Err(why) = verify(&parts, &sigv4::sha256_hex(&body)) {
        return Ok(Response::builder()
            .status(403)
            .header("x-amz-request-id", "UP403")
            .body(Full::new(Bytes::from(format!(
                "<Error><Code>SignatureDoesNotMatch</Code><Message>{why}</Message></Error>"
            ))))
            .unwrap());
    }
    let path = parts.uri.path().to_string();
    let resp = match parts.method.as_str() {
        "PUT" => {
            let checksums: Vec<(String, String)> = parts
                .headers
                .iter()
                .filter(|(k, _)| {
                    k.as_str().starts_with("x-amz-checksum-") || k.as_str() == "content-encoding"
                })
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string()))
                .collect();
            up.objects.lock().insert(path, (body.to_vec(), checksums));
            Response::builder().status(200).header("etag", "\"e\"")
        }
        "GET" => match up.objects.lock().get(&path) {
            Some((data, _)) => {
                return Ok(Response::builder()
                    .status(200)
                    .header("x-amz-request-id", "UPGET")
                    .body(Full::new(Bytes::from(data.clone())))
                    .unwrap());
            }
            None => Response::builder().status(404),
        },
        _ => Response::builder().status(204),
    };
    Ok(resp
        .header("x-amz-request-id", "UP200")
        .body(Full::new(Bytes::new()))
        .unwrap())
}

async fn upstream() -> (String, Arc<Upstream>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let up = Arc::new(Upstream::default());
    let state = up.clone();
    tokio::spawn(async move {
        loop {
            let (s, _) = listener.accept().await.unwrap();
            let state = state.clone();
            tokio::spawn(async move {
                let svc = service_fn(move |r| serve(state.clone(), r));
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(s), svc)
                    .await;
            });
        }
    });
    (format!("http://{addr}"), up)
}

fn client(endpoint: &str, key: &str, secret: &str, log: RequestLog) -> S3Client {
    let cfg = ClientConfig::new("target", endpoint, "eu-west-1").unwrap();
    S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        log,
    )
    .unwrap()
}

async fn setup(policy: Policy) -> (Proxy, S3Client, RequestLog, Arc<Upstream>) {
    let (url, up) = upstream().await;
    let log = RequestLog::default();
    let target = client(&url, REAL_KEY, REAL_SECRET, RequestLog::default());
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
    // the application: another region, the keys sonde gives it
    let cfg = ClientConfig::new("app", &proxy.url(), "us-east-1").unwrap();
    let (key, secret) = proxy.client_keys();
    let app = S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        RequestLog::default(),
    )
    .unwrap();
    (proxy, app, log, up)
}

/// An `Authorization` (and its date) for a request sent by hand to the proxy.
fn signed(
    proxy: &Proxy,
    method: &str,
    path: &str,
    query: &[(String, String)],
    extra: &[(&str, &str)],
    payload: &str,
) -> (String, String) {
    let (key, secret) = proxy.client_keys();
    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let mut headers: Vec<(String, String)> = vec![
        ("host".into(), proxy.addr().to_string()),
        ("x-amz-date".into(), amz_date.clone()),
        ("x-amz-content-sha256".into(), payload.into()),
    ];
    headers.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    let sig = sigv4::sign(
        &SignInput {
            method,
            canonical_uri: path,
            query,
            headers: &headers,
            payload_hash: payload,
            region: "us-east-1",
            service: "s3",
            time: now,
        },
        &Credentials::new(key, secret, None),
    );
    (amz_date, sig.authorization(key))
}

#[tokio::test]
async fn put_get_list_are_resigned_and_recorded() {
    let (_proxy, app, log, up) = setup(Policy::Allow).await;
    let put = S3Request::new("PutObject", http::Method::PUT)
        .bucket("data")
        .key("dir/a b+c.txt")
        .header("content-type", "text/plain")
        .body(Body::from("hello through sonde"));
    let r = app.send_raw(put).await.unwrap();
    assert_eq!(r.status, 200, "{}", r.text());

    let get = S3Request::new("GetObject", http::Method::GET)
        .bucket("data")
        .key("dir/a b+c.txt");
    let r = app.send_raw(get).await.unwrap();
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(&r.body[..], b"hello through sonde");

    let list = S3Request::new("ListObjectsV2", http::Method::GET)
        .bucket("data")
        .query("list-type", "2")
        .query("prefix", "dir/");
    let r = app.send_raw(list).await.unwrap();
    assert_ne!(r.status, 403, "{}", r.text());

    let records = log.list(&reqlog::Filter::default(), 100);
    let ops: Vec<&str> = records.iter().rev().map(|r| r.operation.as_str()).collect();
    assert_eq!(ops, ["PutObject", "GetObject", "ListObjectsV2"]);
    let put = records.last().unwrap();
    assert_eq!(put.key.as_deref(), Some("dir/a b+c.txt"));
    assert_eq!(put.request_bytes, Some(19));
    assert_eq!(put.endpoint, "target");
    // what the application sent is kept; its signature is masked
    let auth = put
        .request_headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("authorization"))
        .unwrap();
    assert!(!auth.value.contains("dummy"));
    assert!(up.calls.lock().len() == 3);
    // a body read to its end: logged with its first byte inside its whole time
    let get = records.iter().find(|r| r.operation == "GetObject").unwrap();
    let first = get.first_byte_ms.expect("first byte measured");
    assert!(first <= get.latency_ms, "{first} > {}", get.latency_ms);
    assert_eq!(get.response_bytes, Some(19));
}

#[tokio::test]
async fn signed_aws_chunked_bodies_are_decoded() {
    let (proxy, _app, log, up) = setup(Policy::Allow).await;
    let sig = "0".repeat(64);
    let body = format!(
        "a;chunk-signature={sig}\r\n0123456789\r\n5;chunk-signature={sig}\r\nabcde\r\n0;chunk-signature={sig}\r\nx-amz-checksum-crc32:Ufg6kA==\r\nx-amz-trailer-signature:{sig}\r\n\r\n"
    );
    let (date, auth) = signed(
        &proxy,
        "PUT",
        "/data/chunked",
        &[],
        &[],
        "STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER",
    );
    let resp = reqwest::Client::new()
        .put(format!("{}/data/chunked", proxy.url()))
        .header(
            "x-amz-content-sha256",
            "STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER",
        )
        .header("x-amz-date", date)
        .header("content-encoding", "aws-chunked")
        .header("x-amz-decoded-content-length", "15")
        .header("x-amz-trailer", "x-amz-checksum-crc32")
        .header("authorization", auth)
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "{}", resp.text().await.unwrap());
    let (data, headers) = up.objects.lock().get("/data/chunked").cloned().unwrap();
    assert_eq!(data, b"0123456789abcde");
    assert!(headers.contains(&("x-amz-checksum-crc32".into(), "Ufg6kA==".into())));
    assert!(!headers.iter().any(|(k, _)| k == "content-encoding"));
    let r = &log.list(&reqlog::Filter::default(), 1)[0];
    assert_eq!(r.operation, "PutObject");
    assert_eq!(r.request_bytes, Some(15));
}

#[tokio::test]
async fn presigned_urls_are_resigned() {
    let (proxy, app, _log, _up) = setup(Policy::Allow).await;
    let put = S3Request::new("PutObject", http::Method::PUT)
        .bucket("data")
        .key("p")
        .body(Body::from("x"));
    app.send(put).await.unwrap();
    let url = app.presign("GET", "data", "p", 60, &[]).await.unwrap();
    assert!(url.starts_with(&proxy.url()));
    let resp = reqwest::get(&url).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "x");
}

#[tokio::test]
async fn no_delete_refuses_without_calling_upstream() {
    let (_proxy, app, log, up) = setup(Policy::NoDelete).await;
    let del = S3Request::new("DeleteObject", http::Method::DELETE)
        .bucket("data")
        .key("k");
    let r = app.send_raw(del).await.unwrap();
    assert_eq!(r.status, 403);
    assert!(r.text().contains("no-delete"));
    assert!(up.calls.lock().is_empty());
    let rec = &log.list(&reqlog::Filter::default(), 1)[0];
    assert_eq!(rec.operation, "DeleteObject");
    assert_eq!(rec.status, Some(403));
}

#[tokio::test]
async fn upstream_errors_keep_their_body() {
    let (url, _up) = upstream().await;
    let log = RequestLog::default();
    // sonde holds the wrong secret: the upstream refuses, the record says why
    let target = client(&url, REAL_KEY, "wrong", RequestLog::default());
    let proxy = Proxy::start(target, log.clone(), ProxyConfig::default())
        .await
        .unwrap();
    let (date, auth) = signed(&proxy, "GET", "/data/k", &[], &[], "UNSIGNED-PAYLOAD");
    let resp = reqwest::Client::new()
        .get(format!("{}/data/k", proxy.url()))
        .header("x-amz-date", date)
        .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
        .header("authorization", auth)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    let rec = &log.list(&reqlog::Filter::default(), 1)[0];
    assert_eq!(rec.request_id.as_deref(), Some("UP403"));
    assert!(
        rec.error_body
            .as_deref()
            .unwrap()
            .contains("SignatureDoesNotMatch")
    );
}

#[tokio::test]
async fn a_control_document_sent_chunked_is_kept_for_replay() {
    let (proxy, _app, log, _up) = setup(Policy::Allow).await;
    let doc = "<Delete><Object><Key>a</Key></Object></Delete>";
    // a body streamed without a length: plain chunked transfer
    let stream = futures::stream::iter([Ok::<_, std::io::Error>(Bytes::from(doc))]);
    let (date, auth) = signed(
        &proxy,
        "POST",
        "/data",
        &[("delete".into(), "".into())],
        &[],
        "UNSIGNED-PAYLOAD",
    );
    let resp = reqwest::Client::new()
        .post(format!("{}/data?delete", proxy.url()))
        .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
        .header("x-amz-date", date)
        .header("authorization", auth)
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), 400, "{}", resp.text().await.unwrap());
    let r = &log.list(&reqlog::Filter::default(), 1)[0];
    assert_eq!(r.operation, "DeleteObjects");
    assert_eq!(r.request_body.as_deref(), Some(doc));
}

#[tokio::test]
async fn only_the_program_given_the_keys_uses_the_credentials() {
    let (proxy, _app, log, up) = setup(Policy::Allow).await;
    let calls = || up.calls.lock().len();
    // not signed at all
    let r = reqwest::get(format!("{}/data/k", proxy.url()))
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    // signed with a key of its own
    let cfg = ClientConfig::new("other", &proxy.url(), "us-east-1").unwrap();
    let other = S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(
            "sonde",
            "sonde-proxy-local-secret",
            None,
        )))),
        RequestLog::default(),
    )
    .unwrap();
    let r = other
        .send_raw(
            S3Request::new("GetObject", http::Method::GET)
                .bucket("data")
                .key("k"),
        )
        .await
        .unwrap();
    assert_eq!(r.status, 403, "{}", r.text());
    // the right signature from a page that rebound its own name to 127.0.0.1
    let (date, auth) = signed(&proxy, "GET", "/data/k", &[], &[], "UNSIGNED-PAYLOAD");
    let r = reqwest::Client::new()
        .get(format!("{}/data/k", proxy.url()))
        .header("host", "evil.example")
        .header("x-amz-date", date)
        .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
        .header("authorization", auth)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    assert_eq!(calls(), 0, "nothing reached the store");
    // each refusal is in the session, saying why
    let refused = log.list(&reqlog::Filter::default(), 10);
    assert_eq!(refused.len(), 3);
    assert!(refused.iter().all(|r| r.status == Some(403)));
    assert!(refused.iter().any(|r| {
        r.error_body
            .as_deref()
            .is_some_and(|b| b.contains("not this machine's"))
    }));
}

#[test]
fn a_sigv2_presigned_url_with_the_keys_is_admitted_and_one_without_is_not() {
    use base64::Engine;
    use hmac::{Hmac, Mac};
    let cfg = ProxyConfig::default();
    let sign = |secret: &str, resource: &str, expires: &str| {
        let mut mac = Hmac::<sha1::Sha1>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("GET\n\n\n{expires}\n{resource}").as_bytes());
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    };
    let enc = |s: &str| {
        s.replace('+', "%2B")
            .replace('/', "%2F")
            .replace('=', "%3D")
    };
    // an hour from now: within the week a presigned URL may last
    let expires = (Utc::now().timestamp() + 3600).to_string();
    let request_until = |sig: &str, key: &str, host: &str, path: &str, expires: &str| {
        let uri = format!(
            "{path}?AWSAccessKeyId={key}&Signature={}&Expires={expires}",
            enc(sig)
        );
        let (parts, _) = Request::builder()
            .uri(uri)
            .header("host", host)
            .body(())
            .unwrap()
            .into_parts();
        sonde_proxy::admit(&cfg, &parts, host)
    };
    let request = |sig: &str, key: &str, host: &str, path: &str| {
        request_until(sig, key, host, path, &expires)
    };
    // path style, a key with an encoded space
    let good = sign(&cfg.client_secret, "/data/a%20b.txt", &expires);
    assert_eq!(
        request(&good, &cfg.client_key, "127.0.0.1:9000", "/data/a%20b.txt"),
        Ok(())
    );
    // virtual-hosted: the bucket is in the Host, and in what was signed
    let vh = sign(&cfg.client_secret, "/data/k", &expires);
    assert_eq!(
        request(&vh, &cfg.client_key, "data.localhost:9000", "/k"),
        Ok(())
    );
    // the right key with another secret, or another key
    let bad = sign("not-the-secret", "/data/a%20b.txt", &expires);
    assert!(request(&bad, &cfg.client_key, "127.0.0.1:9000", "/data/a%20b.txt").is_err());
    assert!(request(&good, "AKIDOTHER", "127.0.0.1:9000", "/data/a%20b.txt").is_err());
    // rightly signed, but good for years: refused, as SigV4 refuses past a week
    let forever = sign(&cfg.client_secret, "/data/k", "1900000000");
    let r = request_until(
        &forever,
        &cfg.client_key,
        "127.0.0.1:9000",
        "/data/k",
        "1900000000",
    );
    assert!(r.unwrap_err().contains("more than a week"));
}

#[test]
fn an_expired_presigned_url_or_a_stale_signature_is_refused() {
    let cfg = ProxyConfig::default();
    let creds = Credentials::new(&cfg.client_key, &cfg.client_secret, None);
    let host = "127.0.0.1:9000";
    let v4 = |at: chrono::DateTime<Utc>, expires: u64| {
        let (query, _) = sigv4::presign(
            "GET",
            "/data/k",
            host,
            &[],
            "us-east-1",
            "s3",
            at,
            expires,
            &creds,
        );
        let uri = format!("/data/k?{}", sigv4::canonical_query(&query));
        let (parts, _) = Request::builder()
            .uri(uri)
            .header("host", host)
            .body(())
            .unwrap()
            .into_parts();
        sonde_proxy::admit(&cfg, &parts, host)
    };
    assert_eq!(v4(Utc::now(), 60), Ok(()));
    // made a minute ago for ten seconds
    let old = v4(Utc::now() - chrono::Duration::seconds(60), 10);
    assert!(old.unwrap_err().contains("expired"));
    // a header signature an hour old
    let stale = Utc::now() - chrono::Duration::hours(1);
    let amz_date = stale.format("%Y%m%dT%H%M%SZ").to_string();
    let headers: Vec<(String, String)> = vec![
        ("host".into(), host.into()),
        ("x-amz-date".into(), amz_date.clone()),
        (
            "x-amz-content-sha256".into(),
            sigv4::UNSIGNED_PAYLOAD.into(),
        ),
    ];
    let sig = sigv4::sign(
        &SignInput {
            method: "GET",
            canonical_uri: "/data/k",
            query: &[],
            headers: &headers,
            payload_hash: sigv4::UNSIGNED_PAYLOAD,
            region: "us-east-1",
            service: "s3",
            time: stale,
        },
        &creds,
    );
    let (parts, _) = Request::builder()
        .uri("/data/k")
        .header("host", host)
        .header("x-amz-date", amz_date)
        .header("x-amz-content-sha256", sigv4::UNSIGNED_PAYLOAD)
        .header("authorization", sig.authorization(&cfg.client_key))
        .body(())
        .unwrap()
        .into_parts();
    assert!(
        sonde_proxy::admit(&cfg, &parts, host)
            .unwrap_err()
            .contains("15 minutes")
    );
    // SigV2: expired in 2020
    let (parts, _) = Request::builder()
        .uri(format!(
            "/data/k?AWSAccessKeyId={}&Signature=x&Expires=1577836801",
            cfg.client_key
        ))
        .header("host", host)
        .body(())
        .unwrap()
        .into_parts();
    assert!(
        sonde_proxy::admit(&cfg, &parts, host)
            .unwrap_err()
            .contains("expired")
    );
}

#[test]
fn every_way_of_writing_this_machine_is_this_machine() {
    let cfg = ProxyConfig::default();
    let host_refused = |host: &str| {
        let (parts, _) = Request::builder()
            .uri("/b/k")
            .header("host", host)
            .body(())
            .unwrap()
            .into_parts();
        sonde_proxy::admit(&cfg, &parts, host)
            .unwrap_err()
            .contains("not this machine")
    };
    for ok in [
        "localhost:9000",
        "localhost.:9000",
        "b.localhost:9000",
        "127.0.0.1:9000",
        "[::1]:9000",
        "[0:0:0:0:0:0:0:1]:9000",
    ] {
        assert!(!host_refused(ok), "{ok} was refused as another machine");
    }
    for other in ["evil.com", "evil.com:9000", "10.0.0.1:9000", "[::]:9000"] {
        assert!(host_refused(other), "{other} was taken");
    }
}

/// An upstream that answers every request with `pieces` bytes, one every
/// `gap`, after the headers.
async fn dripping(pieces: usize, gap: std::time::Duration) -> String {
    dripping_after(0, pieces, gap).await
}

/// [`dripping`], with the first `at_once` bytes sent with the headers.
async fn dripping_after(at_once: usize, pieces: usize, gap: std::time::Duration) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut seen = Vec::new();
                let mut buf = [0u8; 1024];
                while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut buf).await {
                        Ok(n) if n > 0 => seen.extend_from_slice(&buf[..n]),
                        _ => return,
                    }
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
                    at_once + pieces,
                    "x".repeat(at_once)
                );
                let _ = s.write_all(head.as_bytes()).await;
                for _ in 0..pieces {
                    tokio::time::sleep(gap).await;
                    if s.write_all(b"x").await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    format!("http://{addr}")
}

/// A proxy whose store client has `timeout`, and a GET of `/data/k` sent to it.
async fn get_with_timeout(
    upstream: &str,
    timeout: std::time::Duration,
) -> (Proxy, RequestLog, reqwest::Response) {
    let mut cfg = ClientConfig::new("target", upstream, "eu-west-1").unwrap();
    cfg.timeout = timeout;
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
    let proxy = Proxy::start(target, log.clone(), ProxyConfig::default())
        .await
        .unwrap();
    let (date, auth) = signed(&proxy, "GET", "/data/k", &[], &[], "UNSIGNED-PAYLOAD");
    let resp = reqwest::Client::new()
        .get(format!("{}/data/k", proxy.url()))
        .header("x-amz-date", date)
        .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
        .header("authorization", auth)
        .send()
        .await
        .unwrap();
    (proxy, log, resp)
}

#[tokio::test]
async fn a_body_that_takes_longer_than_the_timeout_but_keeps_moving_goes_through() {
    use std::time::Duration;
    // 8 pieces 100 ms apart: 800 ms in all, against a timeout of 400 ms
    let url = dripping(8, Duration::from_millis(100)).await;
    let (_proxy, _log, resp) = get_with_timeout(&url, Duration::from_millis(400)).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().len(), 8);
}

#[tokio::test]
async fn a_body_that_stops_moving_is_cut_after_the_timeout() {
    use std::time::Duration;
    // one byte at once, the next never (in this test's time)
    let url = dripping_after(1, 2, Duration::from_secs(30)).await;
    let started = std::time::Instant::now();
    let (_proxy, log, resp) = get_with_timeout(&url, Duration::from_millis(300)).await;
    assert_eq!(resp.status(), 200);
    assert!(resp.bytes().await.is_err());
    assert!(started.elapsed() < Duration::from_secs(10));
    // the record is not a clean 200 of one byte: it says the body was cut, and where
    let mut records = log.snapshot();
    for _ in 0..40 {
        if !records.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        records = log.snapshot();
    }
    let r = records.first().expect("the request is recorded");
    assert_eq!((r.status, r.response_bytes), (Some(200), Some(1)));
    let why = r
        .transport_error
        .as_deref()
        .expect("a cut body is an error");
    assert!(
        why.starts_with("the response body was cut after 1 byte: ")
            && why.contains("stopped moving for 0.3 s"),
        "{why}"
    );
}

#[tokio::test]
async fn an_upload_slower_than_the_timeout_but_still_moving_goes_through() {
    use std::time::Duration;
    let (url, up) = upstream().await;
    let mut cfg = ClientConfig::new("target", &url, "eu-west-1").unwrap();
    cfg.timeout = Duration::from_millis(400);
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
    let proxy = Proxy::start(target, RequestLog::default(), ProxyConfig::default())
        .await
        .unwrap();
    let pieces = futures::stream::unfold(0, |i| async move {
        if i == 8 {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        Some((Ok::<_, std::io::Error>(Bytes::from_static(b"x")), i + 1))
    });
    let (date, auth) = signed(&proxy, "PUT", "/data/slow", &[], &[], "UNSIGNED-PAYLOAD");
    let resp = reqwest::Client::new()
        .put(format!("{}/data/slow", proxy.url()))
        .header("x-amz-date", date)
        .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
        .header("authorization", auth)
        .header("content-length", "8")
        .body(reqwest::Body::wrap_stream(pieces))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "{}", resp.text().await.unwrap());
    assert_eq!(up.objects.lock()["/data/slow"].0, b"xxxxxxxx");
}

/// A request written by hand: reqwest would tidy the path.
async fn raw(proxy: &Proxy, method: &str, path: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let canonical = path.split('?').next().unwrap();
    let query = sonde_proxy::parse_query(path.split_once('?').map(|(_, q)| q));
    let (date, auth) = signed(proxy, method, canonical, &query, &[], "UNSIGNED-PAYLOAD");
    let mut s = tokio::net::TcpStream::connect(proxy.addr()).await.unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nhost: {}\r\nx-amz-date: {date}\r\nx-amz-content-sha256: UNSIGNED-PAYLOAD\r\nauthorization: {auth}\r\nconnection: close\r\n\r\n",
        proxy.addr()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

#[tokio::test]
async fn a_path_with_an_empty_first_segment_is_refused_under_every_policy() {
    for policy in [Policy::Allow, Policy::NoDelete, Policy::ReadOnly] {
        let (proxy, _app, log, up) = setup(policy).await;
        for method in ["DELETE", "GET"] {
            let answer = raw(&proxy, method, "//data/k").await;
            assert!(answer.starts_with("HTTP/1.1 400"), "{policy:?}: {answer}");
        }
        assert!(up.calls.lock().is_empty(), "{policy:?}");
        assert_eq!(log.list(&reqlog::Filter::default(), 1)[0].status, Some(400));
    }
}

#[tokio::test]
async fn no_delete_refuses_a_delete_it_cannot_name() {
    let (proxy, _app, _log, up) = setup(Policy::NoDelete).await;
    // no bucket in the path: named Unknown, still a DELETE
    for (method, path) in [
        ("DELETE", "/"),
        ("POST", "/?delete"),
        ("PUT", "/?lifecycle"),
        ("POST", "/data?unheard-of"),
    ] {
        let answer = raw(&proxy, method, path).await;
        assert!(
            answer.starts_with("HTTP/1.1 403"),
            "{method} {path}: {answer}"
        );
        assert!(answer.contains("no-delete"), "{answer}");
    }
    assert!(up.calls.lock().is_empty());
}

#[tokio::test]
async fn a_request_in_flight_when_the_proxy_stops_is_still_recorded() {
    use std::time::Duration;
    // the body takes 600 ms; the proxy is told to stop as soon as it starts
    let url = dripping(6, Duration::from_millis(100)).await;
    let (mut proxy, log, resp) = get_with_timeout(&url, Duration::from_secs(30)).await;
    assert!(log.list(&reqlog::Filter::default(), 10).is_empty());
    let reading = tokio::spawn(async move { resp.bytes().await.map(|b| b.len()) });
    assert!(proxy.shutdown(Duration::from_secs(10)).await);
    let records = log.list(&reqlog::Filter::default(), 10);
    assert_eq!(records.len(), 1, "logged before shutdown returns");
    assert_eq!(records[0].response_bytes, Some(6));
    assert_eq!(reading.await.unwrap().unwrap(), 6);
    // and nothing new is taken
    assert!(reqwest::get(proxy.url()).await.is_err());
}

async fn setup_faults(scenario: &str) -> (Proxy, S3Client, RequestLog, Arc<Upstream>) {
    setup_scenario(&sonde_proxy::fault::scenario(scenario).unwrap()).await
}

async fn setup_scenario(
    scenario: &sonde_proxy::fault::Scenario,
) -> (Proxy, S3Client, RequestLog, Arc<Upstream>) {
    let (url, up) = upstream().await;
    let log = RequestLog::default();
    let target = client(&url, REAL_KEY, REAL_SECRET, RequestLog::default());
    let faults = sonde_proxy::fault::Faults::new(scenario);
    let proxy = Proxy::start(
        target,
        log.clone(),
        ProxyConfig {
            faults: Some(Arc::new(faults)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let cfg = ClientConfig::new("app", &proxy.url(), "us-east-1").unwrap();
    let (key, secret) = proxy.client_keys();
    let app = S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        RequestLog::default(),
    )
    .unwrap();
    (proxy, app, log, up)
}

fn put(key: &str) -> S3Request {
    S3Request::new("PutObject", http::Method::PUT)
        .bucket("data")
        .key(key)
        .body(Body::from("payload"))
        .no_retry()
}

#[tokio::test]
async fn a_request_held_past_the_programs_patience_is_kept_as_given_up() {
    let stall = sonde_proxy::fault::project_scenario(
        "stall",
        "delay 3s",
        &["GetObject".to_string()],
        "",
        Some(1),
    )
    .unwrap();
    let (proxy, _app, log, up) = setup_scenario(&stall).await;
    let mut cfg = ClientConfig::new("app", &proxy.url(), "us-east-1").unwrap();
    cfg.timeout = std::time::Duration::from_millis(300);
    let (key, secret) = proxy.client_keys();
    let impatient = S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        RequestLog::default(),
    )
    .unwrap();
    let get = || {
        S3Request::new("GetObject", http::Method::GET)
            .bucket("data")
            .key("k")
            .no_retry()
    };
    let started = std::time::Instant::now();
    assert!(impatient.send_raw(get()).await.is_err(), "it timed out");
    // its retry is let through at once
    let r = impatient.send_raw(get()).await.unwrap();
    assert!(r.status < 500, "{}", r.status);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    // the attempt it gave up on is kept, as given up, and never reached the
    // store
    let mut held = None;
    for _ in 0..50 {
        let recs = log.list(&reqlog::Filter::default(), 10);
        held = recs.into_iter().find(|r| r.injected.is_some());
        if held.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let held = held.expect("the given-up attempt is recorded");
    assert!(held.transport_error.is_some(), "{held:?}");
    assert!(sonde_proxy::trial::faulted(&held));
    assert!(
        held.response_headers
            .iter()
            .any(|h| h.name == sonde_proxy::fault::GIVEN_UP_HEADER),
        "{held:?}"
    );
    assert_eq!(up.calls.lock().len(), 1, "only the retry reached the store");
}

#[tokio::test]
async fn a_fault_is_answered_without_the_store_and_the_retry_goes_through() {
    let (_proxy, app, log, up) = setup_faults("slowdown").await;
    let r = app.send_raw(put("k")).await.unwrap();
    assert_eq!(r.status, 503);
    assert!(r.text().contains("<Code>SlowDown</Code>"), "{}", r.text());
    assert!(up.calls.lock().is_empty(), "the store never saw it");
    let r = app.send_raw(put("k")).await.unwrap();
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(up.calls.lock().len(), 1);
    let recs = log.list(&reqlog::Filter::default(), 10);
    let injected: Vec<_> = recs.iter().filter(|r| r.injected.is_some()).collect();
    assert_eq!(injected.len(), 1);
    assert_eq!(injected[0].status, Some(503));
    assert_eq!(injected[0].injected.as_deref(), Some("slowdown"));
}

#[tokio::test]
async fn a_lost_answer_is_a_write_the_store_made_and_the_program_was_not_told() {
    let (_proxy, app, log, up) = setup_faults("lost-answer").await;
    let r = app.send_raw(put("k")).await.unwrap();
    assert_eq!(r.status, 500);
    assert_eq!(up.calls.lock().len(), 1, "the store took the write");
    let rec = &log.list(&reqlog::Filter::default(), 1)[0];
    assert_eq!(rec.injected.as_deref(), Some("lost-answer"));
    assert!(
        rec.response_headers
            .iter()
            .any(|h| h.name == "x-sonde-store-status" && h.value == "200")
    );
    // reads are not writes: untouched
    let get = S3Request::new("GetObject", http::Method::GET)
        .bucket("data")
        .key("k")
        .no_retry();
    assert_eq!(app.send_raw(get).await.unwrap().status, 200);
}

#[tokio::test]
async fn a_key_with_a_dot_segment_is_refused_and_never_reaches_another_key() {
    let (proxy, _app, log, up) = setup(Policy::Allow).await;
    for path in [
        "/data/a/../g1",
        "/data/a/./g1",
        "/data/a/%2e%2e/g1",
        "/data/..",
    ] {
        for method in ["GET", "DELETE"] {
            let answer = raw(&proxy, method, path).await;
            assert!(answer.starts_with("HTTP/1.1 400"), "{path}: {answer}");
            assert!(answer.contains(". or .. segment"), "{answer}");
        }
    }
    assert!(up.calls.lock().is_empty(), "{:?}", up.calls.lock());
    let rec = &log.list(&reqlog::Filter::default(), 1)[0];
    assert_eq!(rec.status, Some(400));
    assert!(
        rec.response_headers
            .iter()
            .any(|h| h.name == "x-sonde-proxy" && h.value == "refused"),
        "recorded as refused"
    );
    // a dot in a name is a name
    let answer = raw(&proxy, "GET", "/data/a/..b/.c").await;
    assert!(!answer.starts_with("HTTP/1.1 400"), "{answer}");
}

#[tokio::test]
async fn two_batches_of_deletes_each_meet_the_fault_and_each_has_its_retry() {
    let (_proxy, app, log, up) = setup_faults("slowdown").await;
    let batch = |keys: &[&str]| {
        let objects: String = keys
            .iter()
            .map(|k| format!("<Object><Key>{k}</Key></Object>"))
            .collect();
        S3Request::new("DeleteObjects", http::Method::POST)
            .bucket("data")
            .query("delete", "")
            .body(Body::from(format!("<Delete>{objects}</Delete>")))
            .no_retry()
    };
    let send = |keys: &'static [&'static str]| {
        let (app, req) = (app.clone(), batch(keys));
        async move { app.send_raw(req).await.unwrap().status }
    };
    assert_eq!(send(&["a", "b"]).await, 503);
    assert_eq!(
        send(&["c", "d"]).await,
        503,
        "the second batch is a request of its own, not the first one's retry"
    );
    assert!(up.calls.lock().is_empty());
    assert_eq!(send(&["c", "d"]).await, 204);
    assert_eq!(send(&["a", "b"]).await, 204);
    assert_eq!(up.calls.lock().len(), 2);
    // and the records are told apart as the faults told the requests apart
    let recs = log.list(&reqlog::Filter::default(), 10);
    let mut names: Vec<String> = recs.iter().map(sonde_proxy::fault::identity_of).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(
        recs.iter().all(|r| r.request_body.is_some()),
        "kept on a failed one too"
    );
    // read as a trial: both failed, both sent again
    let clean: Vec<reqlog::Record> = recs
        .iter()
        .filter(|r| r.injected.is_none())
        .cloned()
        .collect();
    let t = sonde_proxy::trial::judge(
        &sonde_proxy::fault::scenario("slowdown").unwrap(),
        &clean,
        0,
        &recs,
        0,
    );
    assert_eq!((t.faults, t.retried), (2, 2), "{}", t.line());
}

#[tokio::test]
async fn a_lost_answer_on_a_large_read_is_not_held_to_be_thrown_away() {
    // 64 MiB dripped out: the proxy answers its 500 once the store answered,
    // having read 8 MiB of it at most
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let written = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let count = written.clone();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = listener.accept().await.unwrap();
            let count = count.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                if s.read(&mut buf).await.unwrap_or(0) == 0 {
                    return;
                }
                let total: u64 = 64 << 20;
                let head = format!("HTTP/1.1 200 OK\r\ncontent-length: {total}\r\n\r\n");
                if s.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let piece = vec![7u8; 1 << 20];
                for _ in 0..64 {
                    if s.write_all(&piece).await.is_err() {
                        return;
                    }
                    count.fetch_add(1 << 20, std::sync::atomic::Ordering::SeqCst);
                }
            });
        }
    });
    let log = RequestLog::default();
    let target = client(
        &format!("http://{addr}"),
        REAL_KEY,
        REAL_SECRET,
        RequestLog::default(),
    );
    let all = sonde_proxy::fault::Scenario {
        name: "lose-all".into(),
        about: String::new(),
        rules: vec![sonde_proxy::fault::Rule {
            on: Vec::new(),
            prefix: String::new(),
            effect: sonde_proxy::fault::Effect::LostAnswer,
            times: 1,
            conditional: false,
            most: 0,
        }],
    };
    let proxy = Proxy::start(
        target,
        log.clone(),
        ProxyConfig {
            faults: Some(Arc::new(sonde_proxy::fault::Faults::new(&all))),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let cfg = ClientConfig::new("app", &proxy.url(), "us-east-1").unwrap();
    let (key, secret) = proxy.client_keys();
    let app = S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        RequestLog::default(),
    )
    .unwrap();
    let get = S3Request::new("GetObject", http::Method::GET)
        .bucket("data")
        .key("big")
        .no_retry();
    let r = app.send_raw(get).await.unwrap();
    assert_eq!(r.status, 500);
    let sent = written.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        sent < 40 << 20,
        "the store had sent {sent} bytes: the whole body was read"
    );
    let rec = &log.list(&reqlog::Filter::default(), 1)[0];
    assert_eq!(rec.injected.as_deref(), Some("lose-all"));
}

#[tokio::test]
async fn a_dot_segment_in_the_buckets_place_is_refused_too() {
    let (proxy, _app, log, up) = setup(Policy::Allow).await;
    // the store would fold these: `/%2e%2e/data` is a listing of `data`
    for path in [
        "/%2e%2e/data",
        "/../data",
        "/./data/k",
        "/%2E/data/k",
        "/..",
    ] {
        let answer = raw(&proxy, "GET", path).await;
        assert!(answer.starts_with("HTTP/1.1 400"), "{path}: {answer}");
        assert!(answer.contains(". or .. segment"), "{answer}");
    }
    assert!(up.calls.lock().is_empty(), "{:?}", up.calls.lock());
    for rec in log.list(&reqlog::Filter::default(), 10) {
        assert_eq!(rec.status, Some(400));
        assert!(
            rec.response_headers
                .iter()
                .any(|h| h.name == "x-sonde-proxy" && h.value == "refused")
        );
    }
    // a bucket named with a dot in it is a bucket
    let answer = raw(&proxy, "GET", "/.data/k").await;
    assert!(!answer.starts_with("HTTP/1.1 400"), "{answer}");
}

#[tokio::test]
async fn a_body_hash_the_program_sends_is_not_the_requests_name() {
    let (_proxy, app, log, up) = setup_faults("slowdown").await;
    let batch = |key: &str| {
        S3Request::new("DeleteObjects", http::Method::POST)
            .bucket("data")
            .query("delete", "")
            // every batch claims the same body
            .header(sonde_proxy::fault::BODY_HASH_HEADER, "same")
            .body(Body::from(format!(
                "<Delete><Object><Key>{key}</Key></Object></Delete>"
            )))
            .no_retry()
    };
    assert_eq!(app.send_raw(batch("a")).await.unwrap().status, 503);
    assert_eq!(
        app.send_raw(batch("b")).await.unwrap().status,
        503,
        "another batch, whatever it says of itself"
    );
    let recs = log.list(&reqlog::Filter::default(), 10);
    let names: std::collections::HashSet<String> =
        recs.iter().map(sonde_proxy::fault::identity_of).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    for r in &recs {
        let kept: Vec<&str> = r
            .request_headers
            .iter()
            .filter(|h| {
                h.name
                    .eq_ignore_ascii_case(sonde_proxy::fault::BODY_HASH_HEADER)
            })
            .map(|h| h.value.as_str())
            .collect();
        assert_eq!(kept.len(), 1, "sonde's own, and only it: {kept:?}");
        assert_ne!(kept[0], "same");
    }
    assert!(up.calls.lock().is_empty());
}

#[tokio::test]
async fn a_body_is_cut_half_way_once_and_the_record_says_where() {
    let (proxy, app, log, _up) = setup_faults("cut-body").await;
    // (writes are not this scenario's)
    let big = S3Request::new("PutObject", http::Method::PUT)
        .bucket("data")
        .key("big")
        .body(Body::from("x".repeat(1000)))
        .no_retry();
    assert_eq!(app.send_raw(big).await.unwrap().status, 200);
    let small = S3Request::new("PutObject", http::Method::PUT)
        .bucket("data")
        .key("small")
        .body(Body::from("tiny"))
        .no_retry();
    assert_eq!(app.send_raw(small).await.unwrap().status, 200);

    // the status and the whole length go out; the body stops at its half
    let answer = raw(&proxy, "GET", "/data/big").await;
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let (head, body) = answer.split_once("\r\n\r\n").unwrap();
    assert!(
        head.to_ascii_lowercase().contains("content-length: 1000"),
        "{head}"
    );
    assert!(head.contains("x-sonde-fault: cut-body"), "{head}");
    assert_eq!(body.len(), 500);
    let rec = log
        .list(&reqlog::Filter::default(), 10)
        .into_iter()
        .find(|r| r.injected.is_some())
        .expect("recorded as injected");
    assert_eq!(rec.status, Some(200));
    assert_eq!(rec.response_bytes, Some(500));
    assert_eq!(rec.injected.as_deref(), Some("cut-body"));
    let header = |n: &str| {
        rec.response_headers
            .iter()
            .find(|h| h.name == n)
            .map(|h| h.value.as_str())
    };
    assert_eq!(header("x-sonde-fault"), Some("cut-body"));
    assert_eq!(header(sonde_proxy::CUT_HEADER), Some("500"));
    assert_eq!(header("x-sonde-store-status"), Some("200"));
    assert_eq!(
        rec.transport_error.as_deref(),
        Some("sonde cut the response body after 500 of 1000 bytes on purpose (scenario cut-body)")
    );
    assert!(sonde_proxy::trial::faulted(&rec));

    // the next read of it is whole: the cut is met once
    let get = |key: &str| {
        S3Request::new("GetObject", http::Method::GET)
            .bucket("data")
            .key(key)
            .no_retry()
    };
    let again = app.send_raw(get("big")).await.unwrap();
    assert_eq!((again.status, again.body.len()), (200, 1000));
    // a body too small to cut goes through, and was no fault
    let r = app.send_raw(get("small")).await.unwrap();
    assert_eq!((r.status, r.text().as_str()), (200, "tiny"));
    // nor is an error: the 404 is the store's, and the key's first read is
    // still to come
    assert_eq!(app.send_raw(get("absent")).await.unwrap().status, 404);
    let injected = log
        .list(&reqlog::Filter::default(), 20)
        .iter()
        .filter(|r| r.injected.is_some())
        .count();
    assert_eq!(injected, 1);
}

#[tokio::test]
async fn a_lost_race_is_a_412_to_a_write_the_store_made_or_never_saw() {
    let (_proxy, app, log, up) = setup_faults("lost-race").await;
    let log_key = |n: u32| format!("t/_delta_log/{n:020}.json");
    let create = |key: &str| put(key).header("if-none-match", "*");
    // no condition, no race
    assert_eq!(
        app.send_raw(put("t/part-1.parquet")).await.unwrap().status,
        200
    );
    // If-None-Match: * — the program's own write goes to the store, and it
    // is told it lost
    let r = app.send_raw(create(&log_key(1))).await.unwrap();
    assert_eq!(r.status, 412);
    assert!(
        r.text().contains("<Code>PreconditionFailed</Code>"),
        "{}",
        r.text()
    );
    assert_eq!(up.calls.lock().len(), 2, "the store took the write");
    assert!(
        up.objects
            .lock()
            .contains_key(&format!("/data/{}", log_key(1)))
    );
    // the next number is that commit's retry: not failed again
    assert_eq!(app.send_raw(create(&log_key(2))).await.unwrap().status, 200);
    // If-Match: answered without the store
    let before = up.calls.lock().len();
    let swap = || put("pointer").header("if-match", "\"e\"");
    assert_eq!(app.send_raw(swap()).await.unwrap().status, 412);
    assert_eq!(up.calls.lock().len(), before, "the store never saw it");
    assert_eq!(app.send_raw(swap()).await.unwrap().status, 200);

    let recs = log.list(&reqlog::Filter::default(), 20);
    let lost: Vec<&reqlog::Record> = recs.iter().filter(|r| r.injected.is_some()).collect();
    assert_eq!(lost.len(), 2);
    for r in &lost {
        assert_eq!(r.status, Some(412));
        assert!(sonde_proxy::trial::faulted(r));
    }
    let forwarded = lost
        .iter()
        .find(|r| r.key.as_deref() == Some(log_key(1).as_str()))
        .unwrap();
    assert_eq!(sonde_proxy::trial::store_status(forwarded), Some(200));
    let unsent = lost
        .iter()
        .find(|r| r.key.as_deref() == Some("pointer"))
        .unwrap();
    assert_eq!(sonde_proxy::trial::store_status(unsent), None);
    // what sonde answered itself is not on the store's bill, and is not one
    // of the store's races
    assert!(sonde_proxy::profile::reached_store(forwarded));
    assert!(!sonde_proxy::profile::reached_store(unsent));
    let eps = sonde_proxy::episode::episodes(&recs);
    let obs = sonde_proxy::episode::observations(&recs, &eps);
    assert!(
        !obs.iter()
            .any(|o| o.phrases.first().is_some_and(|p| p.key == "obs.races")),
        "{obs:?}"
    );
}
