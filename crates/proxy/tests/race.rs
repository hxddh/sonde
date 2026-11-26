//! A lost race (`--trial lost-race`) against a store that honors
//! conditions: the write the program is told it lost is in no store, so a
//! writer that reads its log again commits once, and one that steps to the
//! next number unread leaves the log no gap.

mod common;

use std::sync::Arc;

use common::MiniS3;
use reqlog::RequestLog;
use s3raw::{Body, ClientConfig, Credentials, S3Client, S3Request, StaticProvider};
use sonde_proxy::{Proxy, ProxyConfig};

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

fn commit_key(n: u64) -> String {
    format!("t/_delta_log/{n:020}.json")
}

/// A store whose table `t` holds commit 0 (another's, made before), the
/// proxy in front of it under `lost-race`, and the writer's client.
async fn bench() -> (MiniS3, Proxy, S3Client, RequestLog) {
    let store = MiniS3::start(true).await;
    store
        .state
        .lock()
        .buckets
        .entry("data".into())
        .or_default()
        .insert(commit_key(0), (b"create table".to_vec(), "\"0\"".into()));
    let log = RequestLog::default();
    let faults =
        sonde_proxy::fault::Faults::new(&sonde_proxy::fault::scenario("lost-race").unwrap());
    let proxy = Proxy::start(
        keyed(&store.url, "k", "s"),
        log.clone(),
        ProxyConfig {
            faults: Some(Arc::new(faults)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (k, s) = proxy.client_keys();
    let app = keyed(&proxy.url(), k, s);
    (store, proxy, app, log)
}

const MINE: &str = r#"{"add":{"path":"part-1.parquet"}}"#;

fn create(n: u64) -> S3Request {
    S3Request::new("PutObject", http::Method::PUT)
        .bucket("data")
        .key(&commit_key(n))
        .header("if-none-match", "*")
        .body(Body::from(MINE))
}

/// The newest commit the log lists: what a Delta writer reads after a 412.
async fn newest(app: &S3Client) -> u64 {
    let list = S3Request::new("ListObjectsV2", http::Method::GET)
        .bucket("data")
        .query("list-type", "2")
        .query("prefix", "t/_delta_log/");
    let text = app.send_raw(list).await.unwrap().text();
    text.split("<Key>")
        .skip(1)
        .filter_map(|k| k.split("</Key>").next())
        .filter_map(|k| k.rsplit('/').next()?.strip_suffix(".json")?.parse().ok())
        .max()
        .expect("the log holds commit 0")
}

/// The commits in the store's log that are the writer's.
fn mine(store: &MiniS3) -> Vec<String> {
    let s = store.state.lock();
    s.buckets["data"]
        .iter()
        .filter(|(k, (body, _))| k.starts_with("t/_delta_log/") && body == MINE.as_bytes())
        .map(|(k, _)| k.clone())
        .collect()
}

/// A Delta writer: `PUT _delta_log/<n>.json If-None-Match: *`, and on a 412
/// it lists the log, reads the commits that won and writes the number after
/// the newest. Told it lost a race no one won, it must not find its own
/// commit there and append its rows a second time over it.
#[tokio::test]
async fn a_writer_that_reads_its_log_again_after_a_lost_race_commits_once() {
    let (store, _proxy, app, log) = bench().await;
    let mut read = newest(&app).await;
    let mut statuses = Vec::new();
    for _ in 0..5 {
        let status = app.send_raw(create(read + 1)).await.unwrap().status;
        statuses.push(status);
        if status == 200 {
            break;
        }
        assert_eq!(status, 412);
        // what won: every commit past the one it read
        let latest = newest(&app).await;
        for n in read + 1..=latest {
            let won = S3Request::new("GetObject", http::Method::GET)
                .bucket("data")
                .key(&commit_key(n));
            assert_eq!(app.send_raw(won).await.unwrap().status, 200);
        }
        read = latest;
    }
    assert_eq!(statuses, [412, 200], "lost once, then committed");
    assert_eq!(mine(&store), [commit_key(1)], "exactly one commit");
    assert_eq!(
        store.keys("data"),
        [commit_key(0), commit_key(1)],
        "a log without a gap, and nothing of sonde's"
    );
    // the 412 was sonde's, and the store never saw that write
    assert_eq!(store.sent("PUT").len(), 1);
    let recs = log.list(&reqlog::Filter::default(), 20);
    let lost: Vec<&reqlog::Record> = recs
        .iter()
        .filter(|r| r.rare().injected.is_some())
        .collect();
    assert_eq!(lost.len(), 1);
    assert_eq!(lost[0].status, Some(412));
    assert!(
        !lost[0]
            .response_headers()
            .iter()
            .any(|h| h.name == "x-sonde-store-status"),
        "no answer of the store's behind it"
    );
}

/// A writer that takes a 412 for "that number is taken" and writes the next
/// unread: the lost number is in no store, so the next is held too (it
/// would leave the log a gap), and nothing is written.
#[tokio::test]
async fn a_writer_that_steps_past_a_lost_commit_unread_leaves_the_log_no_gap() {
    let (store, _proxy, app, _log) = bench().await;
    for n in 1..5 {
        assert_eq!(app.send_raw(create(n)).await.unwrap().status, 412, "{n}");
    }
    assert_eq!(store.keys("data"), [commit_key(0)]);
    assert!(store.sent("PUT").is_empty(), "the store saw none of them");
    // the lost number itself is still its retry
    assert_eq!(app.send_raw(create(1)).await.unwrap().status, 200);
    assert_eq!(app.send_raw(create(2)).await.unwrap().status, 200);
    assert_eq!(mine(&store).len(), 2);
}

/// A race the store itself decides is the store's: a commit that stands
/// (another writer's) is stepped past once the store has shown it.
#[tokio::test]
async fn a_commit_the_store_shows_to_stand_is_stepped_past() {
    let (store, _proxy, app, _log) = bench().await;
    // sonde's 412 for a number that is in fact taken
    assert_eq!(app.send_raw(create(0)).await.unwrap().status, 412);
    assert_eq!(app.send_raw(create(1)).await.unwrap().status, 412, "held");
    let won = S3Request::new("HeadObject", http::Method::HEAD)
        .bucket("data")
        .key(&commit_key(0));
    assert_eq!(app.send_raw(won).await.unwrap().status, 200);
    assert_eq!(app.send_raw(create(1)).await.unwrap().status, 200);
    assert_eq!(mine(&store), [commit_key(1)]);
}
