//! Proxy overhead: the same small GET, direct and through the proxy.
//!
//!     OVERHEAD_URL=http://127.0.0.1:9100 OVERHEAD_BUCKET=b \
//!     AWS_ACCESS_KEY_ID=… AWS_SECRET_ACCESS_KEY=… \
//!     cargo run --release -p sonde-proxy --example overhead
//!
//! The bucket must exist; the example writes and reads `sonde-overhead/obj`.

use std::sync::Arc;
use std::time::Instant;

use reqlog::RequestLog;
use s3raw::{Body, ClientConfig, Credentials, S3Client, S3Request, StaticProvider};
use sonde_proxy::{Proxy, ProxyConfig};

fn client(url: &str, key: &str, secret: &str) -> S3Client {
    let mut cfg = ClientConfig::new("bench", url, "us-east-1").unwrap();
    cfg.retry = s3raw::RetryPolicy::none();
    S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        RequestLog::new(16),
    )
    .unwrap()
}

async fn measure(c: &S3Client, bucket: &str, n: usize) -> Vec<f64> {
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let req = S3Request::new("GetObject", http::Method::GET)
            .bucket(bucket)
            .key("sonde-overhead/obj");
        let t = Instant::now();
        let r = c.send(req).await.unwrap();
        assert_eq!(r.body.len(), 1024);
        v.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v
}

fn pct(v: &[f64], p: f64) -> f64 {
    v[((v.len() - 1) as f64 * p) as usize]
}

#[tokio::main]
async fn main() {
    let url = std::env::var("OVERHEAD_URL").expect("OVERHEAD_URL");
    let bucket = std::env::var("OVERHEAD_BUCKET").expect("OVERHEAD_BUCKET");
    let key = std::env::var("AWS_ACCESS_KEY_ID").expect("AWS_ACCESS_KEY_ID");
    let secret = std::env::var("AWS_SECRET_ACCESS_KEY").expect("AWS_SECRET_ACCESS_KEY");
    let n: usize = std::env::var("OVERHEAD_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let direct = client(&url, &key, &secret);
    direct
        .send(
            S3Request::new("PutObject", http::Method::PUT)
                .bucket(&bucket)
                .key("sonde-overhead/obj")
                .body(Body::from(vec![7u8; 1024])),
        )
        .await
        .unwrap();
    let log = RequestLog::new(1024);
    let proxy = Proxy::start(direct.clone(), log, ProxyConfig::default())
        .await
        .unwrap();
    let (key, secret) = proxy.client_keys();
    let via = client(&proxy.url(), key, secret);
    // warm both paths, then interleave rounds so drift hits both alike
    measure(&direct, &bucket, 100).await;
    measure(&via, &bucket, 100).await;
    let (mut d, mut p) = (Vec::new(), Vec::new());
    for _ in 0..4 {
        d.extend(measure(&direct, &bucket, n / 4).await);
        p.extend(measure(&via, &bucket, n / 4).await);
    }
    d.sort_by(|a, b| a.total_cmp(b));
    p.sort_by(|a, b| a.total_cmp(b));
    println!("{n} GETs of 1 KiB each way");
    println!(
        "direct      p50 {:.3} ms  p90 {:.3} ms  p99 {:.3} ms",
        pct(&d, 0.5),
        pct(&d, 0.9),
        pct(&d, 0.99)
    );
    println!(
        "via sonde   p50 {:.3} ms  p90 {:.3} ms  p99 {:.3} ms",
        pct(&p, 0.5),
        pct(&p, 0.9),
        pct(&p, 0.99)
    );
    println!(
        "overhead    p50 {:+.3} ms  p90 {:+.3} ms  p99 {:+.3} ms",
        pct(&p, 0.5) - pct(&d, 0.5),
        pct(&p, 0.9) - pct(&d, 0.9),
        pct(&p, 0.99) - pct(&d, 0.99)
    );
}
