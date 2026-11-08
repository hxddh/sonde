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
    let request = |sig: &str, key: &str, host: &str, path: &str| {
        let uri = format!(
            "{path}?AWSAccessKeyId={key}&Signature={}&Expires=1900000000",
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
    // path style, a key with an encoded space
    let good = sign(&cfg.client_secret, "/data/a%20b.txt", "1900000000");
    assert_eq!(
        request(&good, &cfg.client_key, "127.0.0.1:9000", "/data/a%20b.txt"),
        Ok(())
    );
    // virtual-hosted: the bucket is in the Host, and in what was signed
    let vh = sign(&cfg.client_secret, "/data/k", "1900000000");
    assert_eq!(
        request(&vh, &cfg.client_key, "data.localhost:9000", "/k"),
        Ok(())
    );
    // the right key with another secret, or another key
    let bad = sign("not-the-secret", "/data/a%20b.txt", "1900000000");
    assert!(request(&bad, &cfg.client_key, "127.0.0.1:9000", "/data/a%20b.txt").is_err());
    assert!(request(&good, "AKIDOTHER", "127.0.0.1:9000", "/data/a%20b.txt").is_err());
}
