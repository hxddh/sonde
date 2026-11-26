//! What is read again and kept: never past the program's own write. A key
//! (or a range, or a listing) the program writes between its reads is
//! counted stretch by stretch, and the fix it is given drops what it keeps
//! on each write: 32.0 printed `@lru_cache` / a `Map` kept for the run, which
//! answers what the program itself replaced.

use chrono::DateTime;
use reqlog::{Header, Record};
use sonde_analysis::episode::{Observation, episodes, observations};

fn rec(id: u64, method: &str, kind: &str, key: Option<&str>) -> Record {
    Record {
        id,
        time: DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 10).unwrap(),
        method: method.into(),
        kind: kind.into(),
        bucket: Some("b".into()),
        key: key.map(Into::into),
        status: Some(200),
        latency_ms: 5.0,
        request_bytes: Some(if method == "PUT" { 100 } else { 0 }).into(),
        response_bytes: Some(10).into(),
        ..Default::default()
    }
}

fn get(id: u64, key: &str) -> Record {
    rec(id, "GET", "GetObject", Some(key))
}

fn ranged(id: u64, key: &str) -> Record {
    let mut r = get(id, key);
    r.status = Some(206);
    r.lists_mut().request_headers = vec![Header::new("Range", "bytes=-8")];
    r
}

fn list(id: u64, prefix: &str) -> Record {
    let mut r = rec(id, "GET", "ListObjectsV2", None);
    r.lists_mut().query = vec![
        ("list-type".into(), "2".into()),
        ("prefix".into(), prefix.into()),
    ];
    r
}

/// The program's own writes of `key`, each kind of them.
fn writes(id: u64, key: &str) -> Vec<(&'static str, Record)> {
    let mut copy = rec(id, "PUT", "CopyObject", Some(key));
    copy.lists_mut().request_headers = vec![Header::new("x-amz-copy-source", "b/elsewhere")];
    let mut complete = rec(id, "POST", "CompleteMultipartUpload", Some(key));
    complete.lists_mut().query = vec![("uploadId".into(), "u".into())];
    let mut batch = rec(id, "POST", "DeleteObjects", None);
    batch.rare_mut().request_body = Some(format!(
        "<Delete><Object><Key>{key}</Key></Object></Delete>"
    ));
    vec![
        ("PUT", rec(id, "PUT", "PutObject", Some(key))),
        ("copy onto it", copy),
        ("completed upload", complete),
        ("DELETE", rec(id, "DELETE", "DeleteObject", Some(key))),
        ("batch delete", batch),
    ]
}

fn found(rs: &[Record], kind: &str) -> Option<Observation> {
    observations(rs, &episodes(rs))
        .into_iter()
        .find(|o| o.phrases[0].key == kind)
}

fn code<'a>(o: &'a Observation, sdk: &str) -> &'a str {
    o.fixes
        .iter()
        .find(|f| f.sdk == sdk)
        .unwrap_or_else(|| panic!("no {sdk} fix"))
        .code
        .as_ref()
}

/// Whether every SDK's fix drops what it keeps when the program writes.
fn dropped_on_write(o: &Observation) -> bool {
    let drops = [
        ("python", "def write(key, body):"),
        ("java", "void write(String key, byte[] body)"),
        (
            "go",
            "func write(ctx context.Context, k string, body []byte) error",
        ),
        ("js", "async function write(k, Body)"),
        ("rust-aws", "async fn write("),
    ];
    let n = drops
        .iter()
        .filter(|(sdk, write)| code(o, sdk).contains(write))
        .count();
    assert!(
        n == 0 || n == drops.len(),
        "some SDKs' fixes drop the copy and some keep it: {:#?}",
        o.fixes
    );
    // (a copy kept for the run is Python's lru_cache)
    assert_eq!(n == 0, code(o, "python").contains("lru_cache"));
    n > 0
}

#[test]
fn a_key_the_program_writes_between_its_reads_is_kept_until_the_write() {
    // 12 reads, the program's own write, 12 reads: each kind of write
    for (how, write) in writes(13, "state/conf.json") {
        let mut rs: Vec<Record> = (1..=12).map(|i| get(i, "state/conf.json")).collect();
        rs.push(write);
        if how.contains("delete") || how == "DELETE" {
            // (deleted, then written again: read after a delete alone it is a miss)
            rs.push(rec(14, "PUT", "PutObject", Some("state/conf.json")));
        }
        rs.extend((15..=26).map(|i| get(i, "state/conf.json")));
        let hot = found(&rs, "obs.hot").unwrap_or_else(|| panic!("{how}: no finding"));
        // counted stretch by stretch, as before: each read once
        assert_eq!(
            (hot.phrases[0].args["n"].as_ref(), hot.saves_requests),
            ("24", 22),
            "{how}"
        );
        assert!(
            dropped_on_write(&hot),
            "{how}: a copy kept for the run answers what the program replaced:\n{}",
            code(&hot, "python")
        );
    }
}

#[test]
fn a_key_written_after_its_reads_and_read_once_more_is_not_kept_for_the_run() {
    // 25 reads, a write, one read: one stretch counts, and its copy still
    // answers the 27th read wrongly if it is kept for the run
    let mut rs: Vec<Record> = (1..=25).map(|i| get(i, "conf.json")).collect();
    rs.push(rec(26, "PUT", "PutObject", Some("conf.json")));
    rs.push(get(27, "conf.json"));
    let hot = found(&rs, "obs.hot").unwrap();
    assert_eq!(hot.saves_requests, 24);
    assert!(dropped_on_write(&hot), "{}", code(&hot, "python"));
    // written after its last read: the next loop of the program reads it
    rs.pop();
    assert!(dropped_on_write(&found(&rs, "obs.hot").unwrap()));
}

#[test]
fn a_key_the_program_only_reads_is_kept_for_the_run_as_before() {
    let rs: Vec<Record> = (1..=24).map(|i| get(i, "state/conf.json")).collect();
    let hot = found(&rs, "obs.hot").unwrap();
    assert!(!dropped_on_write(&hot));
    assert_eq!(
        code(&hot, "python"),
        "# read it once, keep it for the run\nfrom functools import lru_cache\n@lru_cache(maxsize=1024)\ndef read(key):\n    return s3.get_object(Bucket=b, Key=key)[\"Body\"].read()"
    );
    // its name is its kind and its key, written or not: the fix loop
    // follows one finding from the run that writes the key to one that does not
    assert_eq!(hot.identity, "obs.hot|b/state/conf.json");
    let mut written = rs.clone();
    written.insert(12, rec(100, "PUT", "PutObject", Some("state/conf.json")));
    written[12].time = rs[11].time + chrono::Duration::milliseconds(5);
    let again = found(&written, "obs.hot").unwrap();
    assert!(dropped_on_write(&again));
    assert_eq!(again.identity, hot.identity);
    assert_eq!(again.saves_requests, 22);
    // written before its first read, or another key written: nothing a
    // copy kept for the run would answer wrongly
    let mut before = vec![rec(1, "PUT", "PutObject", Some("state/conf.json"))];
    before.extend((2..=25).map(|i| get(i, "state/conf.json")));
    assert!(!dropped_on_write(&found(&before, "obs.hot").unwrap()));
    let mut other = rs.clone();
    other.push(rec(13, "PUT", "PutObject", Some("state/conf.json.bak")));
    other.push(rec(14, "DELETE", "DeleteObject", Some("state/conf")));
    assert!(!dropped_on_write(&found(&other, "obs.hot").unwrap()));
}

#[test]
fn a_key_asked_for_again_by_head_is_kept_until_the_write_too() {
    let head = |i| rec(i, "HEAD", "HeadObject", Some("data/part.parquet"));
    let mut rs: Vec<Record> = (1..=12).map(head).collect();
    assert!(!dropped_on_write(&found(&rs, "obs.hot").unwrap()));
    rs.push(rec(13, "PUT", "PutObject", Some("data/part.parquet")));
    rs.extend((14..=25).map(head));
    let hot = found(&rs, "obs.hot").unwrap();
    assert_eq!(hot.saves_requests, 22);
    assert!(dropped_on_write(&hot));
}

#[test]
fn a_range_read_again_across_the_programs_write_is_kept_until_the_write() {
    let footers = |from: u64, to: u64| (from..=to).map(|i| ranged(i, "t/part.parquet"));
    let rs: Vec<Record> = footers(1, 24).collect();
    let kept = found(&rs, "obs.rangeagain").unwrap();
    assert_eq!(kept.saves_requests, 23);
    assert!(!dropped_on_write(&kept));
    assert!(code(&kept, "python").contains("def read_range(key, start, end):"));
    for (how, write) in writes(13, "t/part.parquet") {
        let mut rs: Vec<Record> = footers(1, 12).collect();
        rs.push(write);
        rs.extend(footers(14, 25));
        let again = found(&rs, "obs.rangeagain").unwrap_or_else(|| panic!("{how}: no finding"));
        // the first read after the write is the store's: one fewer saved
        assert_eq!(again.saves_requests, 22, "{how}");
        assert!(
            dropped_on_write(&again),
            "{how}:\n{}",
            code(&again, "python")
        );
        assert!(code(&again, "python").contains("def read_range(key, start, end):"));
        assert_eq!(again.identity, kept.identity, "{how}");
    }
}

/// A listing kept has no write to drop it on in its fix (a set of keys
/// made once): it is found only where nothing was written under the prefix
/// between its listings, by any kind of write. (So it was in 32.0: this
/// holds it there.)
#[test]
fn a_prefix_the_program_writes_under_between_its_listings_is_not_listed_once() {
    let listed: Vec<Record> = (1..=6).map(|i| list(i * 2, "inbox/")).collect();
    assert!(found(&listed, "obs.listed").is_some());
    for (how, write) in writes(7, "inbox/new.csv") {
        let mut rs = listed.clone();
        rs.push(write);
        assert!(found(&rs, "obs.listed").is_none(), "{how}");
    }
    // under another prefix, or before the first listing, or after the last
    for (id, key) in [
        (7, "outbox/new.csv"),
        (1, "inbox/new.csv"),
        (13, "inbox/new.csv"),
    ] {
        let mut rs = listed.clone();
        rs.push(rec(id, "PUT", "PutObject", Some(key)));
        let o = found(&rs, "obs.listed").unwrap_or_else(|| panic!("{id} {key}"));
        assert_eq!(o.saves_requests, 5);
    }
}
