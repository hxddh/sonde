//! What each recording policy lets through to a store: no-delete refuses
//! deletes, aborts, lifecycle rules, bucket policies, ACLs and versioning
//! suspended, lets every write through, and lets a recording delete only
//! what stood nowhere before it.

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
    // no store
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
    let rec = |id: u64, method: &str, kind: &str, key: &str, status: u16| {
        let mut r = reqlog::Record {
            id,
            method: method.into(),
            kind: kind.into(),
            bucket: Some("data".into()),
            key: Some(key.into()),
            status: Some(status),
            ..Default::default()
        };
        r.time += chrono::Duration::milliseconds(id as i64);
        r
    };
    let mut created = rec(1, "PUT", "PutObject", "tmp/part", 200);
    created
        .lists_mut()
        .request_headers
        .push(reqlog::Header::new("If-None-Match", "*"));
    let mut kept = rec(5, "PUT", "PutObject", "kept", 200);
    kept.lists_mut()
        .response_headers
        .push(reqlog::Header::new("x-amz-version-id", "v7"));
    let made = sonde_proxy::Made::of(&[
        created,
        // nothing there, then written: made
        rec(2, "HEAD", "HeadObject", "tmp/looked", 404),
        rec(3, "PUT", "PutObject", "tmp/looked", 200),
        // written over what may have stood there: not the run's to delete
        rec(4, "PUT", "PutObject", "old", 200),
        kept,
        // a 404 after the write says nothing of what stood before it
        rec(6, "PUT", "PutObject", "late", 200),
        rec(7, "GET", "GetObject", "late", 404),
    ]);
    let removes = |k: &str| made.may_remove("DeleteObject", Some("data"), Some(k), &[], None);
    assert!(removes("tmp/part") && removes("tmp/looked"));
    assert!(!removes("old") && !removes("late") && !removes("never"));
    // a bucket that keeps versions: a delete adds a marker, and the
    // version written is the run's own
    assert!(removes("kept"));
    let version = |v: &str| {
        let q = vec![("versionId".to_string(), v.to_string())];
        made.may_remove("DeleteObject", Some("data"), Some("kept"), &q, None)
    };
    assert!(version("v7") && !version("v6"));
}

fn delete(key: &str) -> S3Request {
    S3Request::new("DeleteObject", http::Method::DELETE)
        .bucket("data")
        .key(key)
}

fn batch(keys: &[&str]) -> S3Request {
    let objects: String = keys
        .iter()
        .map(|k| format!("<Object><Key>{k}</Key></Object>"))
        .collect();
    S3Request::new("DeleteObjects", http::Method::POST)
        .bucket("data")
        .query("delete", "")
        .body(Body::from(format!("<Delete>{objects}</Delete>")))
}

/// On a bucket without versions, writing over a key that stood before the
/// run does not make it the run's to delete: by PutObject, CopyObject or a
/// completed upload, then DeleteObject or a batch, `old` is still there.
#[tokio::test]
async fn no_delete_never_lets_a_key_that_stood_before_the_run_be_deleted() {
    let (store, _proxy, app, _log) = bench(Policy::NoDelete).await;
    assert_eq!(app.send_raw(put("src", "1")).await.unwrap().status, 200);
    for step in 0..3 {
        let status = match step {
            0 => app.send_raw(put("old", "after")).await.unwrap().status,
            1 => app.send_raw(copy("src", "old")).await.unwrap().status,
            _ => upload(&app, "old").await,
        };
        assert_eq!(status, 200, "the write goes through ({step})");
        let sent = store.sent("DELETE").len() + store.sent("POST /data?delete").len();
        let refused = app.send_raw(delete("old")).await.unwrap();
        assert_eq!(refused.status, 403, "{step}: {}", refused.text());
        assert!(refused.text().contains("no-delete"), "{}", refused.text());
        assert_eq!(app.send_raw(batch(&["old"])).await.unwrap().status, 403);
        assert_eq!(
            store.sent("DELETE").len() + store.sent("POST /data?delete").len(),
            sent,
            "no delete reached the store"
        );
        assert!(body_of(&store, "old").is_some(), "old stands ({step})");
    }
    // `src` was written blind as well: sonde asked the store nothing, so it
    // is not known to be the run's
    assert_eq!(app.send_raw(delete("src")).await.unwrap().status, 403);
    assert!(store.sent("HEAD").is_empty(), "sonde looked for nothing");
    // what the run made it removes: a create, and a key the store had
    // answered 404 for
    let create = put("tmp/made", "x").header("if-none-match", "*");
    assert_eq!(app.send_raw(create).await.unwrap().status, 200);
    let head = S3Request::new("HeadObject", http::Method::HEAD)
        .bucket("data")
        .key("tmp/looked");
    assert_eq!(app.send_raw(head).await.unwrap().status, 404);
    assert_eq!(
        app.send_raw(put("tmp/looked", "x")).await.unwrap().status,
        200
    );
    assert_eq!(
        app.send_raw(batch(&["tmp/made", "tmp/looked"]))
            .await
            .unwrap()
            .status,
        200
    );
    assert!(body_of(&store, "tmp/made").is_none());
    // a create the store refused (the key stood) made nothing
    let lost = put("old", "x").header("if-none-match", "*");
    assert_eq!(app.send_raw(lost).await.unwrap().status, 412);
    assert_eq!(app.send_raw(delete("old")).await.unwrap().status, 403);
}

/// What guards the data is not lifted either: ACLs, the public access
/// block, ownership controls, a legal hold, a governance retention.
#[tokio::test]
async fn no_delete_refuses_what_lifts_a_guard() {
    let (store, _proxy, app, _log) = bench(Policy::NoDelete).await;
    let bucket = |method: http::Method, sub: &str| {
        S3Request::new("BucketSetting", method)
            .bucket("data")
            .query(sub, "")
            .body(Body::from("<x/>"))
    };
    let object = |key: &str, sub: &str, body: &str| {
        S3Request::new("ObjectSetting", http::Method::PUT)
            .bucket("data")
            .key(key)
            .query(sub, "")
            .body(Body::from(body.to_string()))
    };
    let create = put("mine", "x").header("if-none-match", "*");
    assert_eq!(app.send_raw(create).await.unwrap().status, 200);
    let sent = store.state.lock().log.len();
    let hold = |status: &str| format!("<LegalHold><Status>{status}</Status></LegalHold>");
    let refused = [
        bucket(http::Method::PUT, "acl"),
        bucket(http::Method::PUT, "publicAccessBlock"),
        bucket(http::Method::DELETE, "publicAccessBlock"),
        bucket(http::Method::PUT, "ownershipControls"),
        bucket(http::Method::PUT, "lifecycle"),
        bucket(http::Method::DELETE, "lifecycle"),
        bucket(http::Method::PUT, "policy"),
        bucket(http::Method::DELETE, "policy"),
        // an object that stood before the run: its ACL, its hold
        object("old", "acl", "<AccessControlPolicy/>"),
        object("old", "legal-hold", &hold("OFF")),
        object("old", "legal-hold", "<LegalHold/>"),
        // a retention set aside, on what the run made too
        object("old", "retention", "<Retention/>")
            .header("x-amz-bypass-governance-retention", "true"),
        delete("mine").header("x-amz-bypass-governance-retention", "true"),
        batch(&["mine"]).header("x-amz-bypass-governance-retention", "True"),
    ];
    for r in refused {
        let line = format!("{:?}", r);
        let answer = app.send_raw(r).await.unwrap();
        assert_eq!(answer.status, 403, "{line}: {}", answer.text());
        assert!(answer.text().contains("no-delete"), "{}", answer.text());
    }
    assert_eq!(store.state.lock().log.len(), sent, "none reached the store");
    // what keeps more goes through, and the run's own object is its own
    for r in [
        object("old", "legal-hold", &hold("ON")),
        object("old", "retention", "<Retention/>"),
        object("old", "retention", "<Retention/>")
            .header("x-amz-bypass-governance-retention", "false"),
        object("mine", "acl", "<AccessControlPolicy/>"),
        bucket(http::Method::PUT, "cors"),
    ] {
        let line = format!("{:?}", r);
        assert_ne!(app.send_raw(r).await.unwrap().status, 403, "{line}");
    }
    assert_eq!(store.state.lock().log.len(), sent + 5);
}

/// The table no-delete goes by names every operation the proxy can name:
/// one added to `op.rs` is refused until it is given a row here.
#[test]
fn no_delete_has_a_row_for_every_operation() {
    use sonde_proxy::NoDelete::{self, *};
    let passes = [
        "PutObject",
        "CopyObject",
        "CreateMultipartUpload",
        "UploadPart",
        "UploadPartCopy",
        "CompleteMultipartUpload",
        "PutObjectTagging",
        "PutObjectRetention",
        "RestoreObject",
        "SelectObjectContent",
        "UpdateObjectEncryption",
        "CreateBucket",
        "CreateSession",
        "PutBucketCors",
        "PutBucketTagging",
        "PutBucketEncryption",
        "PutObjectLockConfiguration",
        "PutBucketNotificationConfiguration",
        "PutBucketReplication",
        "PutBucketWebsite",
        "PutBucketLogging",
        "PutBucketAccelerateConfiguration",
        "PutBucketRequestPayment",
        "PutBucketAnalyticsConfiguration",
        "PutBucketMetricsConfiguration",
        "PutBucketInventoryConfiguration",
        "PutBucketIntelligentTieringConfiguration",
    ];
    let own = [
        "DeleteObject",
        "DeleteObjects",
        "DeleteBucket",
        "AbortMultipartUpload",
        "PutObjectAcl",
    ];
    let document = ["PutBucketVersioning", "PutObjectLegalHold"];
    let refused = [
        "RenameObject",
        "DeleteObjectTagging",
        "PutBucketLifecycleConfiguration",
        "PutBucketPolicy",
        "PutBucketAcl",
        "PutPublicAccessBlock",
        "PutBucketOwnershipControls",
        "Unknown",
        "PutSomethingNew",
    ];
    let expected = |op: &str| -> NoDelete {
        let read = ["Get", "List", "Head"].iter().any(|v| op.starts_with(v));
        if read || passes.contains(&op) {
            Passes
        } else if own.contains(&op) {
            Own
        } else if document.contains(&op) {
            Document
        } else {
            // every Delete of a bucket's setting, and the rows above
            assert!(
                refused.contains(&op) || (op.starts_with("Delete") && op.len() > 12),
                "{op} has no row"
            );
            Refused
        }
    };
    let mut names = sonde_proxy::op::operation_names();
    names.extend(refused.iter().map(|n| n.to_string()));
    for op in &names {
        assert_eq!(sonde_proxy::no_delete(op), expected(op), "{op}");
        // and the policy goes by it: a read is never refused
        let method = match op.as_str() {
            o if o.starts_with("Get") || o.starts_with("List") || o == "CreateSession" => "GET",
            o if o.starts_with("Head") => "HEAD",
            o if o.starts_with("Delete") && o != "DeleteObjects" => "DELETE",
            "AbortMultipartUpload" => "DELETE",
            o if o.starts_with("Put") || o.starts_with("Upload") || o == "CopyObject" => "PUT",
            "CreateBucket" | "RenameObject" | "UpdateObjectEncryption" => "PUT",
            _ => "POST",
        };
        let refuses = Policy::NoDelete.refuses(method, op, &[]).is_some();
        let by_table = matches!(sonde_proxy::no_delete(op), Refused | Own);
        assert_eq!(refuses, by_table && method != "GET", "{method} {op}");
        assert!(Policy::Allow.refuses(method, op, &[]).is_none());
    }
    for name in passes.iter().chain(&own).chain(&document) {
        assert!(names.iter().any(|n| n == name), "{name} is no operation");
    }
}

/// A trial's run kept to its own place (`SONDE_TRIAL` in its keys, seen in
/// the clean run's) writes nowhere else, under any policy.
#[tokio::test]
async fn a_run_kept_to_its_place_writes_nowhere_else() {
    let (store, proxy, app, _log) = bench(Policy::Allow).await;
    let _ = proxy;
    assert_eq!(app.send_raw(put("etl/x", "1")).await.unwrap().status, 200);
    let made = sonde_proxy::Made::default();
    made.keep_to(Some("slowdown"));
    let strays = |method: &str, op: &str, key: Option<&str>, doc: Option<&str>| {
        made.strays(method, op, Some("data"), key, doc).is_some()
    };
    assert!(!strays(
        "PUT",
        "PutObject",
        Some("etl-slowdown/state"),
        None
    ));
    assert!(!strays("DELETE", "DeleteObject", Some("slowdown/x"), None));
    assert!(strays("PUT", "PutObject", Some("etl/state"), None));
    assert!(strays(
        "PUT",
        "PutObject",
        Some("etl-slowdowns/state"),
        None
    ));
    assert!(strays("PUT", "PutBucketCors", None, None));
    assert!(!strays("GET", "GetObject", Some("etl/state"), None));
    let doc = |keys: &[&str]| {
        let objects: String = keys
            .iter()
            .map(|k| format!("<Object><Key>{k}</Key></Object>"))
            .collect();
        format!("<Delete>{objects}</Delete>")
    };
    let all = doc(&["etl-slowdown/a", "etl-slowdown/b"]);
    let some = doc(&["etl-slowdown/a", "etl/b"]);
    assert!(!strays("POST", "DeleteObjects", None, Some(&all)));
    assert!(strays("POST", "DeleteObjects", None, Some(&some)));
    assert!(strays("POST", "DeleteObjects", None, None));
    made.keep_to(None);
    assert!(!strays("PUT", "PutObject", Some("etl/state"), None));

    // through the proxy: refused as the policy's, and the store not asked
    let log = RequestLog::default();
    let made = Arc::new(sonde_proxy::Made::default());
    made.keep_to(Some("slowdown"));
    let kept = Proxy::start(
        keyed(&store.url, "k", "s"),
        log.clone(),
        ProxyConfig {
            made,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (k, s) = kept.client_keys();
    let app = keyed(&kept.url(), k, s);
    let sent = store.sent("PUT").len();
    let refused = app.send_raw(put("etl/x", "2")).await.unwrap();
    assert_eq!(refused.status, 403, "{}", refused.text());
    assert!(refused.text().contains("slowdown"), "{}", refused.text());
    assert_eq!(body_of(&store, "etl/x").as_deref(), Some("1"));
    assert_eq!(store.sent("PUT").len(), sent);
    let own = app.send_raw(put("etl-slowdown/x", "2")).await.unwrap();
    assert_eq!(own.status, 200);
    let rec = &log.list(&reqlog::Filter::default(), 10);
    assert!(rec.iter().any(sonde_proxy::refused_by_policy));
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
fn no_delete_lets_a_legal_hold_through_only_when_its_document_reads_on() {
    let refuses = |body: Option<&str>| {
        Policy::NoDelete
            .refuses_document("PutObjectLegalHold", body)
            .is_some()
    };
    let ns = "http://s3.amazonaws.com/doc/2006-03-01/";
    assert!(!refuses(Some("<LegalHold><Status>ON</Status></LegalHold>")));
    let spaced = format!("<LegalHold xmlns=\"{ns}\">\n  <Status>ON</Status>\n</LegalHold>");
    assert!(!refuses(Some(&spaced)));
    for no in [
        "<LegalHold><Status>OFF</Status></LegalHold>",
        "<LegalHold><Status>O&#70;F</Status></LegalHold>",
        "<LegalHold><Status>ON</Status><Status>OFF</Status></LegalHold>",
        "<LegalHold><Status><![CDATA[OFF]]></Status></LegalHold>",
        "<LegalHold><Status>on</Status></LegalHold>",
        "<LegalHold/>",
        "",
    ] {
        assert!(refuses(Some(no)), "{no}");
    }
    assert!(refuses(None));
    assert!(
        Policy::Allow
            .refuses_document("PutObjectLegalHold", Some("<LegalHold/>"))
            .is_none()
    );
    assert!(
        Policy::NoDelete
            .refuses_document("PutObject", None)
            .is_none()
    );
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
