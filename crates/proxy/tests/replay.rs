//! Record a workload through the proxy, replay it on other stores, compare.

mod common;

use std::sync::Arc;

use common::MiniS3;
use reqlog::{Filter, RequestLog};
use s3raw::{Body, ClientConfig, Credentials, S3Client, S3Request, StaticProvider};
use sonde_proxy::replay::{ReplayOptions, compare, plan, replay};
use sonde_proxy::{Proxy, ProxyConfig};

fn client(url: &str, log: RequestLog) -> S3Client {
    keyed(url, "k", "s", log)
}

fn keyed(url: &str, key: &str, secret: &str, log: RequestLog) -> S3Client {
    let mut cfg = ClientConfig::new("t", url, "us-east-1").unwrap();
    cfg.retry = s3raw::RetryPolicy::none();
    S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        log,
    )
    .unwrap()
}

/// The application: with the keys the proxy gives it.
fn app(proxy: &Proxy) -> S3Client {
    let (k, s) = proxy.client_keys();
    keyed(&proxy.url(), k, s, RequestLog::default())
}

/// What an application might do: conditional creates (one losing), a
/// multipart upload, a paged listing, a batch delete, lookups.
async fn workload(app: &S3Client) {
    let put = |k: &str, body: &'static str| {
        S3Request::new("PutObject", http::Method::PUT)
            .bucket("data")
            .key(k)
            .body(Body::from(body))
    };
    app.send_raw(S3Request::new("CreateBucket", http::Method::PUT).bucket("data"))
        .await
        .unwrap();
    app.send_raw(put("log/0001.json", "first").header("if-none-match", "*"))
        .await
        .unwrap();
    let lost = app
        .send_raw(put("log/0001.json", "again").header("if-none-match", "*"))
        .await
        .unwrap();
    assert_eq!(lost.status, 412);
    let created = app.send_raw(put("log/0002.json", "second")).await.unwrap();
    let etag = created.header("etag").unwrap().to_string();
    // a compare-and-swap on that ETag
    app.send_raw(put("log/0002.json", "second, edited").header("if-match", &etag))
        .await
        .unwrap();
    // multipart
    let up = app
        .send(
            S3Request::new("CreateMultipartUpload", http::Method::POST)
                .bucket("data")
                .key("big.bin")
                .query("uploads", ""),
        )
        .await
        .unwrap();
    let text = up.text();
    let id = text
        .split("<UploadId>")
        .nth(1)
        .unwrap()
        .split("</UploadId>")
        .next()
        .unwrap()
        .to_string();
    let mut etags = Vec::new();
    for n in 1..=2 {
        let r = app
            .send(
                S3Request::new("UploadPart", http::Method::PUT)
                    .bucket("data")
                    .key("big.bin")
                    .query("partNumber", n.to_string())
                    .query("uploadId", id.clone())
                    .body(Body::from(vec![n as u8; 1000])),
            )
            .await
            .unwrap();
        etags.push(r.header("etag").unwrap().to_string());
    }
    let xml = format!(
        "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part><Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>",
        etags[0], etags[1]
    );
    app.send(
        S3Request::new("CompleteMultipartUpload", http::Method::POST)
            .bucket("data")
            .key("big.bin")
            .query("uploadId", id)
            .body(Body::from(xml)),
    )
    .await
    .unwrap();
    // a listing of two pages
    let page1 = app
        .send(
            S3Request::new("ListObjectsV2", http::Method::GET)
                .bucket("data")
                .query("list-type", "2")
                .query("prefix", "log/")
                .query("max-keys", "1"),
        )
        .await
        .unwrap();
    let t = page1.text();
    let token = t
        .split("<NextContinuationToken>")
        .nth(1)
        .unwrap()
        .split("</NextContinuationToken>")
        .next()
        .unwrap()
        .to_string();
    app.send(
        S3Request::new("ListObjectsV2", http::Method::GET)
            .bucket("data")
            .query("list-type", "2")
            .query("prefix", "log/")
            .query("max-keys", "1")
            .query("continuation-token", token),
    )
    .await
    .unwrap();
    // lookups and a batch delete
    app.send_raw(
        S3Request::new("HeadObject", http::Method::HEAD)
            .bucket("data")
            .key("missing"),
    )
    .await
    .unwrap();
    app.send(
        S3Request::new("GetObject", http::Method::GET)
            .bucket("data")
            .key("big.bin"),
    )
    .await
    .unwrap();
    app.send(
        S3Request::new("DeleteObjects", http::Method::POST)
            .bucket("data")
            .query("delete", "")
            .header("content-type", "application/xml")
            .body(Body::from(
                "<Delete><Object><Key>log/0002.json</Key></Object></Delete>",
            )),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn replay_finds_a_store_that_ignores_conditional_writes() {
    // record against a faithful store
    let original = MiniS3::start(true).await;
    let recorded = RequestLog::new(1000);
    let proxy = Proxy::start(
        client(&original.url, RequestLog::default()),
        recorded.clone(),
        ProxyConfig::default(),
    )
    .await
    .unwrap();
    workload(&app(&proxy)).await;
    let mut records = recorded.list(&Filter::default(), 1000);
    records.reverse();
    assert_eq!(records.len(), 14);
    let delete = records
        .iter()
        .find(|r| r.operation == "DeleteObjects")
        .unwrap();
    assert!(
        delete
            .request_body
            .as_deref()
            .unwrap()
            .contains("<Key>log/0002.json</Key>"),
        "control bodies are recorded"
    );

    let opts = ReplayOptions {
        prefix: "replay-1/".into(),
        buckets: vec![],
    };
    let p = plan(&records, &opts);
    assert_eq!(p.requests, 14);
    assert!(p.skipped.is_empty(), "{:?}", p.skipped);

    // replay on another faithful store: nothing differs
    let twin = MiniS3::start(true).await;
    let log = RequestLog::new(1000);
    let done = replay(&client(&twin.url, log.clone()), &records, &opts, |_, _| {}).await;
    assert!(done.iter().all(|d| d.skipped.is_none()), "{done:?}");
    let replayed = log.list(&Filter::default(), 1000);
    let c = compare(&records, &replayed, &done);
    assert_eq!(c.compared, 14);
    assert_eq!(c.differ(), 0, "{c:#?}");
    // everything landed under the prefix; the batch delete removed its key there
    assert_eq!(
        twin.keys("data"),
        ["replay-1/big.bin", "replay-1/log/0001.json"]
    );

    // replay on a store that ignores If-None-Match and If-Match
    let lax = MiniS3::start(false).await;
    let log = RequestLog::new(1000);
    let done = replay(&client(&lax.url, log.clone()), &records, &opts, |_, _| {}).await;
    let c = compare(&records, &log.list(&Filter::default(), 1000), &done);
    assert_eq!(c.differ(), 1, "{c:#?}");
    let d = &c.differences[0];
    assert_eq!(
        (d.operation.as_str(), d.original.as_str(), d.replay.as_str()),
        ("PutObject", "412 PreconditionFailed", "200")
    );
    assert_eq!(d.conditions, ["if-none-match: *"]);
    assert_eq!(d.example, "data/log/0001.json");
}

#[tokio::test]
async fn a_replay_never_cleans_up_what_it_did_not_write() {
    // record the workload
    let original = MiniS3::start(true).await;
    let recorded = RequestLog::new(1000);
    let proxy = Proxy::start(
        client(&original.url, RequestLog::default()),
        recorded.clone(),
        ProxyConfig::default(),
    )
    .await
    .unwrap();
    workload(&app(&proxy)).await;
    let mut records = recorded.list(&Filter::default(), 1000);
    records.reverse();
    let session = sonde_proxy::session::Session {
        header: sonde_proxy::session::SessionHeader::new("t", &original.url, "us-east-1", None),
        records,
        logs: Vec::new(),
    };
    // a store where the prefix is taken, and a neighbour that only starts with the same letters
    let store = MiniS3::start(true).await;
    let app = client(&store.url, RequestLog::default());
    app.send_raw(S3Request::new("CreateBucket", http::Method::PUT).bucket("data"))
        .await
        .unwrap();
    for k in ["taken/mine.txt", "taken-not/keep.txt"] {
        app.send_raw(
            S3Request::new("PutObject", http::Method::PUT)
                .bucket("data")
                .key(k)
                .body(Body::from("x")),
        )
        .await
        .unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let opts = ReplayOptions {
        prefix: "taken/".into(),
        buckets: vec![],
    };
    let refused = sonde_proxy::replay::replay_to_file(
        &app,
        &session,
        &opts,
        &dir.path().join("r.sonde"),
        false,
        |_, _| {},
    )
    .await;
    assert!(refused.is_err(), "a prefix holding objects is refused");
    assert!(!dir.path().join("r.sonde").exists());
    assert_eq!(store.keys("data"), ["taken-not/keep.txt", "taken/mine.txt"]);

    // "taken" names the folder taken/, never taken-not/
    let opts = ReplayOptions {
        prefix: sonde_proxy::replay::normalize_prefix("fresh").unwrap(),
        buckets: vec![],
    };
    let run = sonde_proxy::replay::replay_to_file(
        &app,
        &session,
        &opts,
        &dir.path().join("r2.sonde"),
        false,
        |_, _| {},
    )
    .await
    .unwrap();
    assert!(run.removed > 0);
    assert_eq!(
        store.keys("data"),
        ["taken-not/keep.txt", "taken/mine.txt"],
        "only the replay's own writes went"
    );
}
