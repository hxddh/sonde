//! What each recording policy lets through to a store: no-delete refuses
//! deletes, aborts, lifecycle rules, bucket policies and versioning
//! suspended, and lets every write through.

mod common;

use std::sync::Arc;

use common::MiniS3;
use reqlog::RequestLog;
use s3raw::{Body, ClientConfig, Credentials, S3Client, S3Request, StaticProvider};
use sonde_proxy::{Policy, Proxy, ProxyConfig};

fn keyed(url: &str, key: &str, secret: &str) -> S3Client {
    let mut cfg = ClientConfig::new("t", url, "us-east-1").unwrap();
    cfg.retry = s3raw::RetryPolicy::none();
    S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        RequestLog::default(),
    )
    .unwrap()
}

/// A store holding `data/old` (written before the recording), a proxy in
/// front of it under `policy`, and the program's client.
async fn bench(policy: Policy) -> (MiniS3, Proxy, S3Client, RequestLog) {
    let store = MiniS3::start(true).await;
    let direct = keyed(&store.url, "k", "s");
    direct
        .send_raw(S3Request::new("CreateBucket", http::Method::PUT).bucket("data"))
        .await
        .unwrap();
    direct.send_raw(put("old", "before")).await.unwrap();
    let log = RequestLog::default();
    let proxy = Proxy::start(
        keyed(&store.url, "k", "s"),
        log.clone(),
        ProxyConfig {
            policy,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (k, s) = proxy.client_keys();
    let app = keyed(&proxy.url(), k, s);
    (store, proxy, app, log)
}

fn put(key: &str, body: &str) -> S3Request {
    S3Request::new("PutObject", http::Method::PUT)
        .bucket("data")
        .key(key)
        .body(Body::from(body.to_string()))
}

fn body_of(store: &MiniS3, key: &str) -> Option<String> {
    let s = store.state.lock();
    s.buckets
        .get("data")
        .and_then(|o| o.get(key))
        .map(|(d, _)| String::from_utf8_lossy(d).into_owned())
}

async fn upload(app: &S3Client, key: &str) -> u16 {
    let created = app
        .send_raw(
            S3Request::new("CreateMultipartUpload", http::Method::POST)
                .bucket("data")
                .key(key)
                .query("uploads", ""),
        )
        .await
        .unwrap();
    let text = created.text();
    let id = text
        .split("<UploadId>")
        .nth(1)
        .and_then(|t| t.split("</UploadId>").next())
        .unwrap()
        .to_string();
    let part = app
        .send_raw(
            S3Request::new("UploadPart", http::Method::PUT)
                .bucket("data")
                .key(key)
                .query("partNumber", "1")
                .query("uploadId", &id)
                .body(Body::from("part")),
        )
        .await
        .unwrap();
    let etag = part.header("etag").unwrap().to_string();
    app.send_raw(
        S3Request::new("CompleteMultipartUpload", http::Method::POST)
            .bucket("data")
            .key(key)
            .query("uploadId", &id)
            .body(Body::from(format!(
                "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{etag}</ETag></Part></CompleteMultipartUpload>"
            ))),
    )
    .await
    .unwrap()
    .status
}

fn copy(from: &str, to: &str) -> S3Request {
    S3Request::new("CopyObject", http::Method::PUT)
        .bucket("data")
        .key(to)
        .header("x-amz-copy-source", format!("/data/{from}"))
}

/// A program that rewrites yesterday's output runs on the first try: under
/// no-delete every write goes through, over an object there before or not,
/// as the program sent it (no condition added, no lookup of sonde's own).
#[tokio::test]
async fn no_delete_lets_every_write_through_as_it_was_sent() {
    let (store, _proxy, app, log) = bench(Policy::NoDelete).await;
    assert_eq!(app.send_raw(put("new", "1")).await.unwrap().status, 200);
    assert_eq!(app.send_raw(put("old", "after")).await.unwrap().status, 200);
    assert_eq!(body_of(&store, "old").as_deref(), Some("after"));
    assert_eq!(upload(&app, "old").await, 200);
    assert_eq!(app.send_raw(copy("new", "old")).await.unwrap().status, 200);
    assert!(store.sent("HEAD").is_empty(), "sonde looked for nothing");
    // a program's own condition is its own: a 412 stays a 412
    let lost = app
        .send_raw(put("old", "x").header("if-none-match", "*"))
        .await
        .unwrap();
    assert_eq!(lost.status, 412);
    assert!(
        log.list(&reqlog::Filter::default(), 100)
            .iter()
            .all(|r| r.status != Some(403)),
        "nothing refused"
    );
    // a delete of what the recording did not make is refused, and reaches
    // no store (what it wrote, `old` now among it, it may remove)
    let sent = store.sent("DELETE").len();
    let del = S3Request::new("DeleteObject", http::Method::DELETE)
        .bucket("data")
        .key("theirs");
    let refused = app.send_raw(del).await.unwrap();
    assert_eq!(refused.status, 403, "{}", refused.text());
    assert!(refused.text().contains("no-delete"), "{}", refused.text());
    assert_eq!(store.sent("DELETE").len(), sent);
}

#[tokio::test]
async fn a_trial_may_remove_what_its_clean_run_made() {
    let clean = reqlog::Record {
        kind: "PutObject".into(),
        bucket: Some("data".into()),
        key: Some("tmp/part".into()),
        status: Some(200),
        ..Default::default()
    };
    let made = sonde_proxy::Made::of(&[clean]);
    let removes = |k: &str| made.may_remove("DeleteObject", Some("data"), Some(k), &[], None);
    assert!(removes("tmp/part") && !removes("old"));
}

#[tokio::test]
async fn allow_writes_over_anything() {
    let (store, _proxy, app, _log) = bench(Policy::Allow).await;
    assert_eq!(app.send_raw(put("old", "after")).await.unwrap().status, 200);
    assert_eq!(body_of(&store, "old").as_deref(), Some("after"));
}

#[tokio::test]
async fn no_delete_refuses_a_bucket_policy_and_versioning_suspended() {
    let (store, _proxy, app, _log) = bench(Policy::NoDelete).await;
    let setting = |sub: &str, body: &str| {
        S3Request::new("PutBucket", http::Method::PUT)
            .bucket("data")
            .query(sub, "")
            .body(Body::from(body.to_string()))
    };
    let sent = store.sent("PUT").len();
    let policy = app
        .send_raw(setting("policy", r#"{"Statement":[{"Effect":"Deny"}]}"#))
        .await
        .unwrap();
    assert_eq!(policy.status, 403, "{}", policy.text());
    let suspend = "<VersioningConfiguration><Status>Suspended</Status></VersioningConfiguration>";
    assert_eq!(
        app.send_raw(setting("versioning", suspend))
            .await
            .unwrap()
            .status,
        403
    );
    let delete_policy = S3Request::new("DeleteBucketPolicy", http::Method::DELETE)
        .bucket("data")
        .query("policy", "");
    assert_eq!(app.send_raw(delete_policy).await.unwrap().status, 403);
    assert_eq!(store.sent("PUT").len(), sent, "none reached the store");
    // versioning turned on keeps more, not less
    let enable = "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>";
    app.send_raw(setting("versioning", enable)).await.unwrap();
    assert_eq!(store.sent("PUT").len(), sent + 1);
}

#[test]
fn policies_by_name_and_the_stricter_of_two() {
    for p in [Policy::Allow, Policy::NoDelete, Policy::ReadOnly] {
        assert_eq!(Policy::named(p.name()), Some(p));
    }
    assert_eq!(Policy::named("readonly"), None);
    assert_eq!(Policy::default(), Policy::Allow);
    assert_eq!(Policy::Allow.stricter(Policy::NoDelete), Policy::NoDelete);
    assert_eq!(Policy::ReadOnly.stricter(Policy::Allow), Policy::ReadOnly);
}

#[test]
fn no_delete_lets_versioning_through_only_when_its_document_reads_enabled() {
    let refuses = |body: &str| {
        Policy::NoDelete
            .refuses_document("PutBucketVersioning", Some(body))
            .is_some()
    };
    let ns = "http://s3.amazonaws.com/doc/2006-03-01/";
    for ok in [
        "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>".to_string(),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<VersioningConfiguration xmlns=\"{ns}\">\n  <Status>Enabled</Status>\n  <MfaDelete>Disabled</MfaDelete>\n</VersioningConfiguration>\n"
        ),
    ] {
        assert!(!refuses(&ok), "{ok}");
    }
    let utf16: String = String::from_utf8_lossy(
        &"<VersioningConfiguration><Status>Suspended</Status></VersioningConfiguration>"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<u8>>(),
    )
    .into_owned();
    for no in [
        "<VersioningConfiguration><Status>Suspended</Status></VersioningConfiguration>",
        "<VersioningConfiguration><Status>Suspen&#100;ed</Status></VersioningConfiguration>",
        "<VersioningConfiguration><Status>Suspen<!-- -->ded</Status></VersioningConfiguration>",
        "<VersioningConfiguration><Status><![CDATA[Suspended]]></Status></VersioningConfiguration>",
        "<VersioningConfiguration><Status>Enabled</Status><Status>Suspended</Status></VersioningConfiguration>",
        "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration><X/>",
        "<VersioningConfiguration><Status>Enabled</Status>",
        "<s3:VersioningConfiguration xmlns:s3=\"x\"><s3:Status>Suspended</s3:Status></s3:VersioningConfiguration>",
        "<VersioningConfiguration><Status>enabled</Status></VersioningConfiguration>",
        "",
        &utf16,
    ] {
        assert!(refuses(no), "{no}");
    }
    assert!(
        Policy::NoDelete
            .refuses_document("PutBucketVersioning", None)
            .is_some()
    );
    assert!(
        Policy::Allow
            .refuses_document("PutBucketVersioning", Some(&utf16))
            .is_none()
    );
}
