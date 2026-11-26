//! Record a workload through the proxy, replay it on other stores, compare.

#[path = "../../proxy/tests/common/mod.rs"]
mod common;

use std::sync::Arc;

use common::MiniS3;
use reqlog::{Filter, RequestLog};
use s3raw::{Body, ClientConfig, Credentials, S3Client, S3Request, StaticProvider};
use sonde_analysis::replay::{ReplayOptions, compare, plan, replay};
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
    let delete = records.iter().find(|r| r.kind == "DeleteObjects").unwrap();
    assert!(
        delete
            .rare()
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
    assert_eq!(p.requests, 13);
    // the bucket is not created again: it is outside the prefix
    assert_eq!(
        p.skipped,
        [(
            "creates or deletes a bucket, outside the prefix".to_string(),
            1
        )]
        .into()
    );

    // replay on another faithful store: nothing differs
    let twin = MiniS3::start(true).await;
    let log = RequestLog::new(1000);
    let done = replay(&client(&twin.url, log.clone()), &records, &opts, |_, _| {}).await;
    let skipped: Vec<_> = done.iter().filter_map(|d| d.skipped.as_deref()).collect();
    assert_eq!(skipped, ["creates or deletes a bucket, outside the prefix"]);
    let replayed = log.list(&Filter::default(), 1000);
    assert!(replayed.iter().all(|r| r.kind != "CreateBucket"));
    let c = compare(&records, &replayed, &done);
    assert_eq!(c.compared, 13);
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
        (d.kind.as_str(), d.original.as_str(), d.replay.as_str()),
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
    let run = sonde_analysis::run::RunFile {
        header: sonde_analysis::run::RunHeader::new("t", &original.url, "us-east-1", None),
        records,
        logs: Vec::new(),
        afresh: false,
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
    let refused = sonde_analysis::replay::replay_to_file(
        &app,
        &run,
        &opts,
        &dir.path().join("r.sonde"),
        |_, _| {},
    )
    .await;
    assert!(refused.is_err(), "a prefix holding objects is refused");
    assert!(!dir.path().join("r.sonde").exists());
    assert_eq!(store.keys("data"), ["taken-not/keep.txt", "taken/mine.txt"]);
    // never on the recorded keys: a prefix of its own, always
    let in_place = ReplayOptions {
        prefix: String::new(),
        buckets: vec![],
    };
    let refused = sonde_analysis::replay::replay_to_file(
        &app,
        &run,
        &in_place,
        &dir.path().join("r0.sonde"),
        |_, _| {},
    )
    .await;
    assert!(refused.is_err(), "an empty prefix is refused");
    assert!(!dir.path().join("r0.sonde").exists());

    // "taken" names the folder taken/, never taken-not/
    let opts = ReplayOptions {
        prefix: sonde_analysis::replay::normalize_prefix("fresh").unwrap(),
        buckets: vec![],
    };
    let run = sonde_analysis::replay::replay_to_file(
        &app,
        &run,
        &opts,
        &dir.path().join("r2.sonde"),
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

/// A store that answers every request with `status` and `code`.
async fn answering(status: u16, code: &'static str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                // one request a connection: none of the replay's has a body
                // larger than a read
                if s.read(&mut buf).await.unwrap_or(0) == 0 {
                    return;
                }
                let body = format!("<Error><Code>{code}</Code><Message>{code}</Message></Error>");
                let answer = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(answer.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

fn one_put() -> sonde_analysis::run::RunFile {
    let r = reqlog::Record {
        id: 1,
        kind: "PutObject".into(),
        method: "PUT".into(),
        bucket: Some("data".into()),
        key: Some("k".into()),
        status: Some(200),
        request_bytes: Some(1).into(),
        ..Default::default()
    };
    sonde_analysis::run::RunFile {
        header: sonde_analysis::run::RunHeader::new("t", "http://x", "us-east-1", None),
        records: vec![r],
        logs: Vec::new(),
        afresh: false,
    }
}

#[tokio::test]
async fn a_replay_that_cannot_list_its_prefix_does_not_start() {
    let url = answering(403, "AccessDenied").await;
    let log = RequestLog::new(100);
    let store = client(&url, log.clone());
    let dir = tempfile::tempdir().unwrap();
    let opts = ReplayOptions {
        prefix: "fresh/".into(),
        buckets: vec![],
    };
    let out = dir.path().join("r.sonde");
    let run =
        sonde_analysis::replay::replay_to_file(&store, &one_put(), &opts, &out, |_, _| {}).await;
    let why = run.expect_err("an unknown prefix is not replayed into");
    assert!(why.contains("could not check"), "{why}");
    assert!(!out.exists());
    // the listing only: nothing written, nothing deleted
    let sent = log.list(&Filter::default(), 100);
    assert!(sent.iter().all(|r| r.method == "GET"), "{sent:?}");
}

#[tokio::test]
async fn a_bucket_that_is_not_there_is_not_created_and_nothing_is_sent() {
    let url = answering(404, "NoSuchBucket").await;
    let store = client(&url, RequestLog::default());
    let dir = tempfile::tempdir().unwrap();
    let opts = ReplayOptions {
        prefix: "fresh/".into(),
        buckets: vec![],
    };
    let out = dir.path().join("r.sonde");
    let Err(why) =
        sonde_analysis::replay::replay_to_file(&store, &one_put(), &opts, &out, |_, _| {}).await
    else {
        panic!("replayed into a bucket that is not there");
    };
    assert!(why.contains("a replay creates no bucket"), "{why}");
    assert!(!out.exists());
}

#[tokio::test]
async fn a_replay_on_a_bucket_that_keeps_versions_removes_the_versions_it_wrote() {
    let store = MiniS3::start(true).await;
    store.state.lock().versioned = true;
    let target = client(&store.url, RequestLog::default());
    target
        .send_raw(S3Request::new("CreateBucket", http::Method::PUT).bucket("data"))
        .await
        .unwrap();
    // something of the bucket's own, with a version, outside the prefix
    target
        .send_raw(
            S3Request::new("PutObject", http::Method::PUT)
                .bucket("data")
                .key("mine.txt")
                .body(Body::from("x")),
        )
        .await
        .unwrap();
    let put = |id: u64, key: &str| reqlog::Record {
        id,
        kind: "PutObject".into(),
        method: "PUT".into(),
        bucket: Some("data".into()),
        key: Some(key.into()),
        status: Some(200),
        request_bytes: Some(4).into(),
        ..Default::default()
    };
    let run = sonde_analysis::run::RunFile {
        header: sonde_analysis::run::RunHeader::new("t", "http://x", "us-east-1", None),
        // one key written twice: two versions of it
        records: vec![put(1, "a"), put(2, "a"), put(3, "dir/b")],
        logs: Vec::new(),
        afresh: false,
    };
    let dir = tempfile::tempdir().unwrap();
    let opts = ReplayOptions {
        prefix: "rp1/".into(),
        buckets: vec![],
    };
    let run = sonde_analysis::replay::replay_to_file(
        &target,
        &run,
        &opts,
        &dir.path().join("r.sonde"),
        |_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(run.removed, 2, "the two objects");
    // three versions written, and a delete marker for each object deleted
    assert_eq!(run.removed_versions, 5);
    assert_eq!(run.remaining, 0);
    let left: Vec<String> = store
        .state
        .lock()
        .versions
        .iter()
        .map(|(_, k, _, _)| k.clone())
        .collect();
    assert_eq!(
        left,
        ["mine.txt"],
        "nothing under the prefix, all outside it"
    );
}

fn put_record(id: u64, key: &str) -> reqlog::Record {
    reqlog::Record {
        id,
        kind: "PutObject".into(),
        method: "PUT".into(),
        bucket: Some("data".into()),
        key: Some(key.into()),
        status: Some(200),
        request_bytes: Some(4).into(),
        ..Default::default()
    }
}

async fn replayed(
    store: &MiniS3,
    records: Vec<reqlog::Record>,
) -> sonde_analysis::replay::ReplayRun {
    let target = client(&store.url, RequestLog::default());
    target
        .send_raw(S3Request::new("CreateBucket", http::Method::PUT).bucket("data"))
        .await
        .unwrap();
    let run = sonde_analysis::run::RunFile {
        header: sonde_analysis::run::RunHeader::new("t", "http://x", "us-east-1", None),
        records,
        logs: Vec::new(),
        afresh: false,
    };
    let dir = tempfile::tempdir().unwrap();
    let opts = ReplayOptions {
        prefix: "rp1/".into(),
        buckets: vec![],
    };
    sonde_analysis::replay::replay_to_file(
        &target,
        &run,
        &opts,
        &dir.path().join("r.sonde"),
        |_, _| {},
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_store_without_delete_objects_is_cleaned_up_key_by_key() {
    for versioned in [false, true] {
        let store = MiniS3::start(true).await;
        {
            let mut s = store.state.lock();
            s.no_batch_delete = true;
            s.versioned = versioned;
        }
        let run = replayed(
            &store,
            vec![
                put_record(1, "a"),
                put_record(2, "a"),
                put_record(3, "dir/b"),
            ],
        )
        .await;
        assert_eq!(run.removed, 2, "versioned {versioned}");
        assert_eq!(run.removed_versions, if versioned { 5 } else { 0 });
        assert_eq!(run.remaining, 0, "versioned {versioned}");
        assert!(store.keys("data").is_empty());
        assert!(store.state.lock().versions.is_empty());
        // the batch was tried once and no more
        let batches = store
            .sent("POST /data")
            .iter()
            .filter(|(line, _)| line.contains("delete"))
            .count();
        assert_eq!(batches, 1, "{:?}", store.sent("POST"));
    }
}

#[tokio::test]
async fn what_a_cleanup_removed_is_what_the_store_says_and_what_remains_is_counted_once() {
    let store = MiniS3::start(true).await;
    {
        let mut s = store.state.lock();
        s.versioned = true;
        s.undeletable = vec!["rp1/a".into()];
        s.no_abort = true;
    }
    let mut create = put_record(4, "big");
    create.kind = "CreateMultipartUpload".into();
    create.method = "POST".into();
    create.lists_mut().query = vec![("uploads".into(), "".into())];
    create.request_bytes = None.into();
    let run = replayed(
        &store,
        vec![
            put_record(1, "a"),
            put_record(2, "a"),
            put_record(3, "dir/b"),
            create,
        ],
    )
    .await;
    // the batch asked for two keys; the store's answer says one was not deleted
    assert_eq!(run.removed, 1);
    // `dir/b`: its version and its marker. `a` keeps its two versions.
    assert_eq!(run.removed_versions, 2);
    // one object (its newest version) and one older version: two things,
    // not the object counted again among the versions
    assert_eq!(
        (
            run.remaining,
            run.remaining_objects,
            run.remaining_versions,
            run.remaining_markers
        ),
        (2, 1, 1, 0)
    );
    assert_eq!(run.unaborted_uploads, 1);
}

fn create_upload_record(id: u64, key: &str) -> reqlog::Record {
    let mut create = put_record(id, key);
    create.kind = "CreateMultipartUpload".into();
    create.method = "POST".into();
    create.lists_mut().query = vec![("uploads".into(), "".into())];
    create.request_bytes = None.into();
    create
}

/// A store lists 1,000 uploads a page: the ones past the first page are
/// aborted too, and counted when they would not abort.
#[tokio::test]
async fn a_cleanup_aborts_the_uploads_of_every_page() {
    let creates = || -> Vec<reqlog::Record> {
        (0..1001)
            .map(|i| create_upload_record(i + 1, &format!("up/{i:04}")))
            .collect()
    };
    let store = MiniS3::start(true).await;
    let run = replayed(&store, creates()).await;
    assert_eq!(store.state.lock().open_uploads(), 0, "none left billed");
    assert_eq!(run.unaborted_uploads, 0);
    // and where none aborts, every one is counted, not the first page's
    let store = MiniS3::start(true).await;
    store.state.lock().no_abort = true;
    let run = replayed(&store, creates()).await;
    assert_eq!(run.unaborted_uploads, 1001);
}

/// A store that answers every page of uploads with the same marker is not
/// asked for ever.
#[tokio::test]
async fn a_listing_of_uploads_that_never_ends_ends_the_cleanup() {
    let store = MiniS3::start(true).await;
    store.state.lock().same_uploads_page = true;
    let run = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        replayed(&store, vec![create_upload_record(1, "big")]),
    )
    .await
    .expect("the cleanup ended");
    assert_eq!(store.state.lock().open_uploads(), 0);
    assert_eq!(run.unaborted_uploads, 0);
}

/// A program that reads input it did not write: its replay writes
/// stand-ins of what it read first, so nothing differs.
#[tokio::test]
async fn a_run_that_reads_existing_input_replays_with_no_difference() {
    let original = MiniS3::start(true).await;
    let direct = client(&original.url, RequestLog::default());
    let put = |k: &str, body: &'static str| {
        S3Request::new("PutObject", http::Method::PUT)
            .bucket("data")
            .key(k)
            .body(Body::from(body))
    };
    // what was there before the run
    direct
        .send_raw(S3Request::new("CreateBucket", http::Method::PUT).bucket("data"))
        .await
        .unwrap();
    for (k, body) in [
        ("in/a.json", "aaaaaaaaaa"),
        ("in/b.json", "bbbbbbbbbbbbbbbbbbbb"),
        ("old/1", "x"),
        ("old/2", "y"),
        ("old/3", "z"),
    ] {
        direct.send_raw(put(k, body)).await.unwrap();
    }
    let recorded = RequestLog::new(1000);
    let proxy = Proxy::start(
        client(&original.url, RequestLog::default()),
        recorded.clone(),
        ProxyConfig::default(),
    )
    .await
    .unwrap();
    let app = app(&proxy);
    let get = |k: &str| {
        S3Request::new("GetObject", http::Method::GET)
            .bucket("data")
            .key(k)
    };
    let list = |p: &str| {
        S3Request::new("ListObjectsV2", http::Method::GET)
            .bucket("data")
            .query("list-type", "2")
            .query("prefix", p)
    };
    app.send_raw(list("in/")).await.unwrap();
    let head = app
        .send_raw(
            S3Request::new("HeadObject", http::Method::HEAD)
                .bucket("data")
                .key("in/a.json"),
        )
        .await
        .unwrap();
    let etag = head.header("etag").unwrap().to_string();
    app.send_raw(get("in/a.json")).await.unwrap();
    app.send_raw(get("in/b.json").header("range", "bytes=0-4"))
        .await
        .unwrap();
    app.send_raw(list("old/")).await.unwrap();
    // a compare-and-swap on what it read
    app.send_raw(put("in/a.json", "edited").header("if-match", &etag))
        .await
        .unwrap();
    let _ = app.send_raw(get("in/missing.json")).await;
    app.send_raw(put("out/result.json", "done")).await.unwrap();
    let mut records = recorded.list(&Filter::default(), 1000);
    records.reverse();
    let opts = ReplayOptions {
        prefix: "rp/".into(),
        buckets: vec![],
    };
    // the two it read, of the sizes it read, and three stand-ins for what it listed
    let seeds = sonde_analysis::replay::seeds(&records, &opts);
    let named: Vec<(&str, u64)> = seeds.iter().map(|s| (s.key.as_str(), s.size)).collect();
    assert_eq!(
        named,
        [
            ("in/a.json", 10),
            ("in/b.json", 20),
            ("old/sonde-seed-1", 0),
            ("old/sonde-seed-2", 0),
            ("old/sonde-seed-3", 0)
        ]
    );
    // ("in/" listed two keys before the run read them: they are the two read)
    let p = plan(&records, &opts);
    assert_eq!((p.seeds, p.seed_bytes), (5, 30));

    let twin = MiniS3::start(true).await;
    let target = client(&twin.url, RequestLog::default());
    target
        .send_raw(S3Request::new("CreateBucket", http::Method::PUT).bucket("data"))
        .await
        .unwrap();
    let run = sonde_analysis::run::RunFile {
        header: sonde_analysis::run::RunHeader::new("t", "http://x", "us-east-1", None),
        records,
        logs: Vec::new(),
        afresh: false,
    };
    let dir = tempfile::tempdir().unwrap();
    let done = sonde_analysis::replay::replay_to_file(
        &target,
        &run,
        &opts,
        &dir.path().join("r.sonde"),
        |_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(done.comparison.differ(), 0, "{:#?}", done.comparison);
    assert_eq!(done.comparison.compared, 8);
    // the stand-ins went with the rest
    assert!(twin.keys("data").is_empty(), "{:?}", twin.keys("data"));
}
