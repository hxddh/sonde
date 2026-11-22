//! A request's time as the program sees it: the proxy adds no wait of its
//! own to an answer, and a record's time ends when the program has the body.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use chrono::Utc;
use futures::StreamExt;
use http_body_util::StreamBody;
use hyper::body::Frame;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use reqlog::RequestLog;
use s3raw::sigv4::{self, SignInput};
use s3raw::{ClientConfig, Credentials, S3Client, StaticProvider};
use sonde_proxy::{Proxy, ProxyConfig};
use tokio::net::TcpListener;

/// A store answering `GET /data/<n>` with `n` bytes, sent in pieces of
/// 16 KiB as a store's do (each its own write). It checks no signature.
async fn store() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((s, _)) = listener.accept().await {
            let _ = s.set_nodelay(true);
            tokio::spawn(async move {
                let svc = service_fn(|r: Request<hyper::body::Incoming>| async move {
                    let n: usize = r.uri().path().rsplit('/').next().unwrap().parse().unwrap();
                    let pieces = futures::stream::iter(
                        (0..n)
                            .step_by(16 << 10)
                            .map(move |at| Bytes::from(vec![b'x'; (16 << 10).min(n - at)])),
                    )
                    .then(|piece| async move {
                        tokio::task::yield_now().await;
                        Ok::<_, std::convert::Infallible>(Frame::data(piece))
                    });
                    Ok::<_, std::convert::Infallible>(
                        Response::builder()
                            .header("content-length", n.to_string())
                            .body(StreamBody::new(pieces))
                            .unwrap(),
                    )
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(s), svc)
                    .await;
            });
        }
    });
    format!("http://{addr}")
}

async fn proxy(store: &str, log: RequestLog) -> Proxy {
    let cfg = ClientConfig::new("target", store, "us-east-1").unwrap();
    let creds = Arc::new(StaticProvider(Some(Credentials::new("k", "s", None))));
    let target = S3Client::new(cfg, creds, RequestLog::default()).unwrap();
    Proxy::start(target, log, ProxyConfig::default())
        .await
        .unwrap()
}

/// A GET of `path` as a program sends it, signed with the proxy's keys
/// when there is one.
fn get(host: &str, path: &str, keys: Option<(&str, &str)>) -> String {
    let mut req = format!("GET {path} HTTP/1.1\r\nhost: {host}\r\n");
    if let Some((key, secret)) = keys {
        let now = Utc::now();
        let date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let headers = [
            ("host".to_string(), host.to_string()),
            (
                "x-amz-content-sha256".into(),
                sigv4::UNSIGNED_PAYLOAD.into(),
            ),
            ("x-amz-date".into(), date.clone()),
        ];
        let sig = sigv4::sign(
            &SignInput {
                method: "GET",
                canonical_uri: path,
                query: &[],
                headers: &headers,
                payload_hash: sigv4::UNSIGNED_PAYLOAD,
                region: "us-east-1",
                service: "s3",
                time: now,
            },
            &Credentials::new(key, secret, None),
        );
        req.push_str(&format!(
            "x-amz-content-sha256: {}\r\nx-amz-date: {date}\r\nauthorization: {}\r\n",
            sigv4::UNSIGNED_PAYLOAD,
            sig.authorization(key)
        ));
    }
    req.push_str("\r\n");
    req
}

/// Reads one answer's head and its body, `pace` between reads of `read`
/// bytes; the body's length.
fn answer(conn: &mut BufReader<TcpStream>, read: usize, pace: Duration) -> usize {
    let mut length = 0;
    loop {
        let mut line = String::new();
        conn.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap();
        }
    }
    let mut buf = vec![0; read];
    let mut got = 0;
    while got < length {
        let n = conn.read(&mut buf[..read.min(length - got)]).unwrap();
        assert!(n > 0, "the answer stopped after {got} of {length} bytes");
        got += n;
        if !pace.is_zero() {
            std::thread::sleep(pace);
        }
    }
    length
}

/// The median time of GETs of `sizes` on one kept-alive connection, as the
/// program waits for each: from sending the request to the body's last byte.
async fn round_trips(addr: String, sizes: Vec<usize>, keys: Option<(String, String)>) -> Duration {
    tokio::task::spawn_blocking(move || {
        let tcp = TcpStream::connect(&addr).unwrap();
        // the program's own side sends at once, as SDKs' do
        tcp.set_nodelay(true).unwrap();
        let mut conn = BufReader::new(tcp);
        let mut times = Vec::new();
        for n in sizes {
            let req = get(
                &addr,
                &format!("/data/{n}"),
                keys.as_ref().map(|(k, s)| (k.as_str(), s.as_str())),
            );
            let t = Instant::now();
            conn.get_mut().write_all(req.as_bytes()).unwrap();
            assert_eq!(answer(&mut conn, 64 << 10, Duration::ZERO), n);
            times.push(t.elapsed());
        }
        times.sort();
        times[times.len() / 2]
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_proxy_adds_no_wait_to_an_answer_of_a_few_pieces() {
    let store = store().await;
    let log = RequestLog::default();
    let proxy = proxy(&store, log.clone()).await;
    let keys = proxy.client_keys();
    let keys = Some((keys.0.to_string(), keys.1.to_string()));
    // bodies of 8 to 64 KiB, the size of a file's footer, a page, a manifest
    let sizes: Vec<usize> = (0..24).map(|i| (8 + (i % 8) * 8) << 10).collect();
    let direct = round_trips(
        store.trim_start_matches("http://").into(),
        sizes.clone(),
        None,
    )
    .await;
    let via = round_trips(proxy.addr().to_string(), sizes.clone(), keys).await;
    assert!(
        via < direct + Duration::from_millis(10),
        "through sonde {via:?} a request, directly {direct:?}"
    );
    // and each record's time is the program's wait, not less
    let records = log.snapshot();
    assert_eq!(records.len(), sizes.len());
    let mut recorded: Vec<f64> = records.iter().map(|r| r.latency_ms).collect();
    recorded.sort_by(f64::total_cmp);
    let recorded = recorded[recorded.len() / 2];
    let waited = via.as_secs_f64() * 1000.0;
    assert!(
        (waited - recorded).abs() < 5.0,
        "the program waited {waited:.1} ms, the record says {recorded:.1} ms"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_records_time_covers_the_body_going_to_a_slow_reader() {
    let store = store().await;
    let log = RequestLog::default();
    let proxy = proxy(&store, log.clone()).await;
    let addr = proxy.addr().to_string();
    let (key, secret) = proxy.client_keys();
    let (key, secret) = (key.to_string(), secret.to_string());
    let n = 32usize << 20;
    let waited = tokio::task::spawn_blocking(move || {
        let tcp = TcpStream::connect(&addr).unwrap();
        tcp.set_nodelay(true).unwrap();
        let mut conn = BufReader::new(tcp);
        let req = get(&addr, &format!("/data/{n}"), Some((&key, &secret)));
        let t = Instant::now();
        conn.get_mut().write_all(req.as_bytes()).unwrap();
        // 64 KiB every millisecond or so: well over half a second
        assert_eq!(answer(&mut conn, 64 << 10, Duration::from_millis(1)), n);
        t.elapsed()
    })
    .await
    .unwrap();
    let records = log.snapshot();
    let record = &records[0];
    let waited = waited.as_secs_f64() * 1000.0;
    // (the last few MiB wait in the sockets' buffers for the program to read)
    assert!(
        record.latency_ms > waited * 0.75,
        "the program read for {waited:.0} ms, the record says {:.0} ms",
        record.latency_ms
    );
    assert_eq!(record.response_bytes, Some(n as u64));
}
