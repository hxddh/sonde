//! What each recording policy lets through to a store: no-delete refuses
//! deletes, bucket policies, versioning suspended, and a write over an
//! object the recording did not write itself.

mod common;

use std::sync::Arc;

use common::MiniS3;
use reqlog::RequestLog;
use s3raw::{Body, ClientConfig, Credentials, S3Client, S3Request, StaticProvider};
use sonde_proxy::{Policy, Proxy, ProxyConfig, Written};

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
async fn bench(
    policy: Policy,
    honor: bool,
    written: Written,
) -> (MiniS3, Proxy, S3Client, RequestLog) {
    let store = MiniS3::start(honor).await;
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
            written: Arc::new(written),
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

#[tokio::test]
async fn no_delete_writes_new_objects_and_its_own_again_but_never_over_one_there_before() {
    let (store, _proxy, app, log) = bench(Policy::NoDelete, true, Written::default()).await;
    // new, then its own again: a program that writes a key twice
    assert_eq!(app.send_raw(put("new", "1")).await.unwrap().status, 200);
    assert_eq!(app.send_raw(put("new", "2")).await.unwrap().status, 200);
    assert_eq!(body_of(&store, "new").as_deref(), Some("2"));
    // there before: the store's 412 is sonde's refusal
    let over = app.send_raw(put("old", "after")).await.unwrap();
    assert_eq!(over.status, 403, "{}", over.text());
    assert!(over.text().contains("no-delete"), "{}", over.text());
    assert!(over.text().contains("--policy allow"), "{}", over.text());
    assert_eq!(body_of(&store, "old").as_deref(), Some("before"));
    // the If-None-Match was sonde's: not on the record, which says refused
    let rec = log
        .list(&reqlog::Filter::default(), 100)
        .into_iter()
        .find(|r| r.status == Some(403))
        .unwrap();
    assert!(
        !rec.request_headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("if-none-match")),
        "{rec:?}"
    );
    assert!(
        rec.response_headers
            .iter()
            .any(|h| h.name == "x-sonde-proxy" && h.value == "refused")
    );
    // a program's own condition is its own: a 412 stays a 412
    let lost = app
        .send_raw(put("old", "x").header("if-none-match", "*"))
        .await
        .unwrap();
    assert_eq!(lost.status, 412);
    // If-Match names an object there: refused before the store is asked
    let sent = store.sent("PUT").len();
    let cas = app
        .send_raw(put("old", "x").header("if-match", "\"e\""))
        .await
        .unwrap();
    assert_eq!(cas.status, 403);
    assert_eq!(store.sent("PUT").len(), sent);
}

#[tokio::test]
async fn no_delete_refuses_a_completion_or_a_copy_over_an_object_there_before() {
    let (store, _proxy, app, _log) = bench(Policy::NoDelete, true, Written::default()).await;
    assert_eq!(upload(&app, "parts").await, 200);
    assert_eq!(upload(&app, "parts").await, 200, "its own, again");
    assert_eq!(upload(&app, "old").await, 403);
    assert_eq!(body_of(&store, "old").as_deref(), Some("before"));
    // a copy's target is looked for first
    assert_eq!(
        app.send_raw(copy("old", "copied")).await.unwrap().status,
        200
    );
    assert_eq!(
        app.send_raw(copy("old", "copied")).await.unwrap().status,
        200
    );
    let over = app.send_raw(copy("new-ish", "old")).await.unwrap();
    assert_eq!(over.status, 403, "{}", over.text());
    assert_eq!(body_of(&store, "old").as_deref(), Some("before"));
}

#[tokio::test]
async fn a_trial_may_write_again_what_its_clean_run_wrote() {
    let mut clean = reqlog::Record {
        operation: "PutObject".into(),
        bucket: Some("data".into()),
        key: Some("old".into()),
        status: Some(200),
        ..Default::default()
    };
    let refused = reqlog::Record {
        key: Some("other".into()),
        status: Some(403),
        ..clean.clone()
    };
    clean.id = 1;
    let (store, _proxy, app, _log) =
        bench(Policy::NoDelete, true, Written::of(&[clean, refused])).await;
    assert_eq!(app.send_raw(put("old", "again")).await.unwrap().status, 200);
    assert_eq!(body_of(&store, "old").as_deref(), Some("again"));
}

#[tokio::test]
async fn allow_writes_over_anything_and_a_store_without_conditions_is_not_guarded() {
    let (store, _proxy, app, _log) = bench(Policy::Allow, true, Written::default()).await;
    assert_eq!(app.send_raw(put("old", "after")).await.unwrap().status, 200);
    assert_eq!(body_of(&store, "old").as_deref(), Some("after"));
    // a store that ignores If-None-Match writes over: docs/security.md says so
    let (store, _proxy, app, _log) = bench(Policy::NoDelete, false, Written::default()).await;
    assert_eq!(app.send_raw(put("old", "after")).await.unwrap().status, 200);
    assert_eq!(body_of(&store, "old").as_deref(), Some("after"));
}

#[tokio::test]
async fn no_delete_refuses_a_bucket_policy_and_versioning_suspended() {
    let (store, _proxy, app, _log) = bench(Policy::NoDelete, true, Written::default()).await;
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
    assert_eq!(Policy::default(), Policy::NoDelete);
    assert_eq!(Policy::Allow.stricter(Policy::NoDelete), Policy::NoDelete);
    assert_eq!(Policy::ReadOnly.stricter(Policy::Allow), Policy::ReadOnly);
}
