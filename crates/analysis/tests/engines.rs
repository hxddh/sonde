//! Fixes in the engine's own words: a finding whose requests are an
//! engine's gets that engine's setting, from requests as the engines send
//! them (the SlateDB recording in tests/fixtures, and each tool's user
//! agent), and never an SDK's code the program does not call.

use std::path::Path;

use chrono::{DateTime, Duration};
use reqlog::{Header, Record};
use sonde_analysis::episode::{Observation, episodes, observations};
use sonde_analysis::fixes::{Fix, for_sdk};
use sonde_analysis::run::RunFile;

fn load(name: &str) -> RunFile {
    RunFile::open(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn findings(recs: &[Record]) -> Vec<Observation> {
    observations(recs, &episodes(recs))
}

fn of<'a>(obs: &'a [Observation], key: &str) -> &'a Observation {
    obs.iter()
        .find(|o| o.phrases[0].key == key)
        .unwrap_or_else(|| panic!("no {key}: {obs:#?}"))
}

/// The fix `show` gives for a finding: the one for its SDK.
fn shown(o: &Observation) -> &Fix {
    for_sdk(&o.fixes, o.sdk.as_deref()).unwrap_or_else(|| panic!("no fix: {o:#?}"))
}

/// A request of the SlateDB recording, sent again `n` times `every_ms` apart
/// from `from_ms` on, ids from `id`.
fn again(r: &Record, n: usize, every_ms: i64, from_ms: i64, id: u64) -> Vec<Record> {
    (0..n)
        .map(|i| {
            let mut c = r.clone();
            c.id = id + i as u64;
            c.time = r.time + Duration::milliseconds(from_ms + i as i64 * every_ms);
            c
        })
        .collect()
}

/// The SlateDB recording's manifest listing and its read of a manifest.
fn slate_manifest() -> (Record, Record) {
    let s = load("slatedb.sonde");
    let list = s
        .records
        .iter()
        .find(|r| {
            r.kind == "ListObjectsV2"
                && r.query()
                    .iter()
                    .any(|(k, v)| &**k == "prefix" && v.contains("manifest"))
        })
        .unwrap()
        .clone();
    let read = s
        .records
        .iter()
        .find(|r| {
            r.kind == "GetObject"
                && r.status == Some(200)
                && r.key.as_deref().is_some_and(|k| k.ends_with(".manifest"))
        })
        .unwrap()
        .clone();
    (list, read)
}

#[test]
fn a_slatedb_reader_polling_its_manifest_is_told_its_poll_interval() {
    // a DbReader: every 100 ms it lists the manifests and reads the newest
    let (list, read) = slate_manifest();
    let mut rs = again(&list, 30, 100, 0, 1);
    rs.extend(again(&read, 30, 100, 5, 100));
    let obs = findings(&rs);
    for key in ["obs.listed", "obs.hot"] {
        let o = of(&obs, key);
        assert_eq!(o.sdk.as_deref(), Some("object_store 0.14"));
        let fix = shown(o);
        let s = fix.setting.as_ref().expect("a setting");
        assert_eq!(
            (s.engine.as_str(), s.key.as_str(), s.value.as_str()),
            ("SlateDB DbReader", "manifest_poll_interval", "10s"),
            "{key}"
        );
        assert!(
            fix.code
                .starts_with("// SlateDB DbReader polls the manifest every 100ms"),
            "{}",
            fix.code
        );
        assert!(fix.code.contains("DbReaderOptions {"), "{}", fix.code);
        // no paginator, no cache in Rust: the program calls SlateDB, not object_store
        assert!(o.fixes.iter().all(|f| f.sdk == "config"), "{:#?}", o.fixes);
    }
    // the JSON an agent reads: the setting whole
    let j = serde_json::to_value(shown(of(&obs, "obs.listed"))).unwrap();
    assert_eq!(j["setting"]["key"], "manifest_poll_interval");
    assert_eq!(j["setting"]["value"], "10s");
    assert!(j["setting"].get("where").is_none(), "{j}");
}

#[test]
fn a_slatedb_writer_is_told_the_dbs_own_interval_and_none_at_its_default() {
    let s = load("slatedb.sonde");
    let wal = s
        .records
        .iter()
        .find(|r| r.kind == "PutObject" && r.key.as_deref().is_some_and(|k| k.contains("/wal/")))
        .unwrap();
    let (list, read) = slate_manifest();
    // a Db (it writes its WAL) whose manifest is polled every 200 ms
    let mut rs = again(&list, 20, 200, 0, 1);
    rs.extend(again(&read, 20, 200, 5, 100));
    rs.extend(again(wal, 1, 0, 2, 200).into_iter().map(|mut r| {
        r.key = Some("db/wal/00000000000000000099.sst".into());
        r
    }));
    let obs = findings(&rs);
    let s = shown(of(&obs, "obs.hot")).setting.clone().unwrap();
    assert_eq!((s.engine.as_str(), s.value.as_str()), ("SlateDB Db", "1s"));
    // polled every 10 s already, a reader's default: no value to give it
    let mut rs = again(&list, 12, 10_000, 0, 1);
    rs.extend(again(&read, 12, 10_000, 5, 100));
    let obs = findings(&rs);
    let o = of(&obs, "obs.listed");
    assert!(
        o.fixes
            .iter()
            .all(|f| f.setting.is_none() && f.sdk == "config"),
        "{:#?}",
        o.fixes
    );
}

#[test]
fn a_slatedb_wal_flushed_often_is_told_its_flush_interval() {
    let s = load("slatedb.sonde");
    let wal = s
        .records
        .iter()
        .find(|r| {
            r.kind == "PutObject"
                && r.key.as_deref().is_some_and(|k| k.contains("/wal/"))
                && r.request_bytes.get().unwrap_or(0) > 0
        })
        .unwrap();
    let rs: Vec<Record> = again(wal, 80, 100, 0, 1)
        .into_iter()
        .enumerate()
        .map(|(i, mut r)| {
            r.key = Some(format!("db/wal/{:020}.sst", i + 10).into());
            r
        })
        .collect();
    let obs = findings(&rs);
    let fix = shown(of(&obs, "obs.small"));
    let s = fix.setting.as_ref().unwrap();
    assert_eq!(
        (s.engine.as_str(), s.key.as_str(), s.value.as_str()),
        ("SlateDB Db", "flush_interval", "1s")
    );
    assert!(fix.code.contains("every 100ms"), "{}", fix.code);
}

/// A request as a tool sends it: its user agent.
fn req(id: u64, at_ms: i64, op: &str, key: &str, status: u16, ua: &str) -> Record {
    let method = match op {
        "HeadObject" => "HEAD",
        "PutObject" => "PUT",
        _ => "GET",
    };
    Record {
        id,
        time: DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
        method: method.into(),
        kind: op.into(),
        bucket: Some("lake".into()),
        key: Some(key.into()),
        status: Some(status),
        latency_ms: 5.0,
        request_bytes: Some(0).into(),
        response_bytes: Some(10).into(),
        lists: reqlog::Lists {
            request_headers: vec![Header::new("user-agent", ua)],
            ..Default::default()
        }
        .into(),
        ..Default::default()
    }
}

/// HEAD then GET of each of 6 objects.
fn head_get(ua: &str) -> Vec<Record> {
    let mut rs = Vec::new();
    for i in 0..6u64 {
        let k = format!("in/{i}.csv");
        rs.push(req(i * 2 + 1, i as i64 * 100, "HeadObject", &k, 200, ua));
        rs.push(req(
            i * 2 + 2,
            i as i64 * 100 + 10,
            "GetObject",
            &k,
            200,
            ua,
        ));
    }
    rs
}

/// 40 requests at once, 10 of them throttled.
fn throttled(ua: &str) -> Vec<Record> {
    (0..40u64)
        .map(|i| {
            let mut r = req(
                i + 1,
                (i / 20) as i64 * 2,
                "GetObject",
                &format!("in/{i}"),
                200,
                ua,
            );
            r.latency_ms = 50.0;
            if i % 4 == 0 {
                r.status = Some(503);
            }
            r
        })
        .collect()
}

fn setting_of(o: &Observation) -> (String, String, String) {
    let s = shown(o)
        .setting
        .clone()
        .unwrap_or_else(|| panic!("{:#?}", o.fixes));
    (s.engine, s.key, s.value)
}

#[test]
fn duckdb_is_told_its_own_settings() {
    let ua = "duckdb/v1.2.1(linux_amd64) python/3.12";
    let obs = findings(&head_get(ua));
    assert_eq!(
        setting_of(of(&obs, "obs.headget")),
        (
            "DuckDB".into(),
            "enable_http_metadata_cache".into(),
            "true".into()
        )
    );
    assert!(shown(of(&obs, "obs.headget")).code.starts_with("-- DuckDB"));
}

#[test]
fn rclone_is_told_its_flags() {
    let ua = "rclone/v1.68.2";
    let obs = findings(&head_get(ua));
    assert_eq!(
        setting_of(of(&obs, "obs.headget")),
        ("rclone".into(), "--s3-no-head-object".into(), "true".into())
    );
    let obs = findings(&throttled(ua));
    let o = obs
        .iter()
        .find(|o| o.phrases[0].key.starts_with("obs.throttled"))
        .unwrap();
    let (engine, key, value) = setting_of(o);
    assert_eq!((engine.as_str(), key.as_str()), ("rclone", "--tpslimit"));
    // half of the 40 a second it sent
    assert_eq!(value, "20");
}

#[test]
fn s5cmd_and_trino_are_told_theirs_when_throttled() {
    let obs = findings(&throttled(
        "aws-sdk-go/1.44.298 (go1.22.1; linux; amd64) S3Manager s5cmd/v2.2.2",
    ));
    let (engine, key, value) = setting_of(of(&obs, "obs.throttled"));
    assert_eq!((engine.as_str(), key.as_str()), ("s5cmd", "--numworkers"));
    let safe = of(&obs, "obs.throttled").phrases[0].args["safe"].to_string();
    assert_eq!(value, safe);
    let obs = findings(&throttled(
        "aws-sdk-java/2.29.6 md/io#sync ua/2.1 os/Linux lang/java#23 app/Trino",
    ));
    assert_eq!(
        setting_of(of(&obs, "obs.throttled")),
        ("Trino".into(), "s3.retry-mode".into(), "ADAPTIVE".into())
    );
}

#[test]
fn an_object_store_program_of_its_own_gets_rusts_code() {
    // no engine's files, no tool named: the program calls object_store itself
    let obs = findings(&head_get("object_store/0.12.3"));
    let fix = shown(of(&obs, "obs.headget"));
    assert_eq!(fix.sdk, "rust");
    assert!(fix.code.contains("store.get(&path)"), "{}", fix.code);
}

#[test]
fn an_iceberg_tables_orphans_get_icebergs_cleanup_not_deltas() {
    let s = load("iceberg.sonde");
    let obs = findings(&s.records);
    let o = of(&obs, "obs.orphans");
    assert!(
        o.fixes.iter().all(|f| !f.code.contains("deltalake")),
        "{:#?}",
        o.fixes
    );
    let fix = shown(o);
    assert!(fix.code.contains("remove_orphan_files"), "{}", fix.code);
    // the table's place, from its requests: no bucket to guess
    assert!(fix.code.contains("s3://"), "{}", fix.code);
    // and its small files sent by pyarrow (an engine's): no SDK's code
    assert!(
        of(&obs, "obs.small.parts")
            .fixes
            .iter()
            .all(|f| f.sdk == "config"),
        "{:#?}",
        of(&obs, "obs.small.parts").fixes
    );
}

#[test]
fn where_a_setting_is_set_is_found_in_the_programs_folder() {
    use sonde_analysis::engines::{Setting, locate};
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::create_dir_all(d.join("target/debug")).unwrap();
    std::fs::write(
        d.join("src/main.rs"),
        "use slatedb::*;\n\nfn main() {\n    let o = DbReaderOptions { manifest_poll_interval: Duration::from_millis(100), ..Default::default() };\n}\n",
    )
    .unwrap();
    // a build's copy, and a longer name that holds the key: not it
    std::fs::write(d.join("target/debug/x.rs"), "manifest_poll_interval\n").unwrap();
    std::fs::write(d.join("notes.txt"), "my_manifest_poll_interval_x\n").unwrap();
    let s = Setting {
        engine: "SlateDB DbReader".into(),
        key: "manifest_poll_interval".into(),
        value: "10s".into(),
        at: None,
        also: vec![],
    };
    assert_eq!(locate(d, &s).as_deref(), Some("src/main.rs:4"));
    // a config file at the top comes before a source deeper down
    std::fs::write(
        d.join("SlateDb.toml"),
        "flush_interval = \"100ms\"\nmanifest_poll_interval = \"100ms\"\n",
    )
    .unwrap();
    assert_eq!(locate(d, &s).as_deref(), Some("SlateDb.toml:2"));
    // a dotted key under Spark's prefix is the key
    std::fs::write(
        d.join("spark-defaults.conf"),
        "spark.hadoop.fs.s3a.directory.marker.retention=delete\n",
    )
    .unwrap();
    let markers = Setting {
        key: "fs.s3a.directory.marker.retention".into(),
        ..s.clone()
    };
    assert_eq!(
        locate(d, &markers).as_deref(),
        Some("spark-defaults.conf:1")
    );
    let none = Setting {
        key: "s3.retry-mode".into(),
        ..s
    };
    assert_eq!(locate(d, &none), None);
}
