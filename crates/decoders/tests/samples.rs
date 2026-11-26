//! Each decoder on a layout written by the real system (`samples/`, made by
//! `tools/`: SlateDB's crate, pyiceberg, lakeFS, Woodpecker's codec).

use std::path::Path;

use decoders::{DirSource, Format, Severity, analyze, detect};

fn samples(sub: &str) -> DirSource {
    DirSource::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("samples")
            .join(sub),
    )
}

#[tokio::test]
async fn iceberg_table() {
    let src = samples("iceberg");
    let found = detect(&src, "").await.unwrap();
    let d = found
        .iter()
        .find(|d| d.format == Format::Iceberg)
        .expect("detected");
    assert_eq!(d.root, "warehouse/demo.db/events/");
    let l = analyze(&src, Format::Iceberg, &d.root).await.unwrap();
    assert!(l.decoded, "{:?}", l.issues);
    assert_eq!(l.format_version.as_deref(), Some("format-version 2"));
    assert!(!l.has_errors(), "{:?}", l.issues);
    let fact = |n: &str| {
        l.facts
            .iter()
            .find(|f| f.label == n)
            .map(|f| f.value.clone())
            .unwrap()
    };
    assert_eq!(fact("Snapshots"), "5");
    // pyiceberg's overwrite writes a delete then an append; the delete of id 5 rewrites the file
    assert_eq!(fact("Live records"), "29");
    assert!(
        fact("Orphan data files").starts_with("1 "),
        "{}",
        fact("Orphan data files")
    );
    assert!(l.issues.iter().any(|i| i.message.contains("orphans")));
    assert!(
        l.issues
            .iter()
            .any(|i| i.message.contains("expire_snapshots")),
        "{:?}",
        l.issues
    );
    assert_eq!(l.snapshots.len(), 5);
    // replay: the first snapshot holds the first append only
    let first = l.state_at(l.snapshots[0].time.unwrap()).unwrap();
    assert!(
        first
            .facts
            .iter()
            .any(|f| f.label == "Records" && f.value == "100"),
        "{:?}",
        first.facts
    );
}

#[tokio::test]
async fn lakefs_namespace() {
    let src = samples("lakefs");
    let found = detect(&src, "").await.unwrap();
    let d = found
        .iter()
        .find(|d| d.format == Format::Lakefs)
        .expect("detected");
    assert_eq!(d.root, "sample/");
    let l = analyze(&src, Format::Lakefs, &d.root).await.unwrap();
    assert!(!l.has_errors(), "{:?}", l.issues);
    let fact = |n: &str| {
        l.facts
            .iter()
            .find(|f| f.label == n)
            .map(|f| f.value.clone())
            .unwrap()
    };
    // two commits on main/dev, the merge reuses dev's tree
    assert_eq!(fact("Metaranges (committed trees)"), "2");
    assert_eq!(fact("Ranges"), "2");
    assert_eq!(fact("Distinct paths"), "3");
    let trees = &l.tables[0];
    assert!(trees.rows.iter().any(|r| r[3] == "2"), "{:?}", trees.rows);
    let paths: Vec<&Vec<String>> = l
        .snapshots
        .iter()
        .flat_map(|s| s.tables[0].rows.iter())
        .collect();
    assert!(
        paths
            .iter()
            .any(|r| r[0] == "tables/b/f3.csv" && r[1].starts_with("data/")),
        "{paths:?}"
    );
}

#[tokio::test]
async fn woodpecker_log() {
    let src = samples("woodpecker");
    let found = detect(&src, "").await.unwrap();
    let d = found
        .iter()
        .find(|d| d.format == Format::Woodpecker)
        .expect("detected");
    let l = analyze(&src, Format::Woodpecker, &d.root).await.unwrap();
    assert!(!l.has_errors(), "{:?}", l.issues);
    assert_eq!(l.format_version.as_deref(), Some("v6"));
    let fact = |n: &str| {
        l.facts
            .iter()
            .find(|f| f.label == n)
            .map(|f| f.value.clone())
            .unwrap()
    };
    assert_eq!(fact("Segments"), "4");
    assert_eq!(fact("Sealed"), "3");
    assert_eq!(fact("Active"), "1");
    assert_eq!(fact("Entries"), (120 + 50 + 20 + 5).to_string());
    let seg = &l.tables[0].rows;
    assert!(
        seg.iter()
            .any(|r| r[0] == "1" && r[1] == "0" && r[5] == "119"),
        "LAC of log 1 segment 0: {seg:?}"
    );
}

#[tokio::test]
async fn woodpecker_corruption_is_reported() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples");
    let mut mem = decoders::MemSource::default();
    for e in walk(&base.join("woodpecker")) {
        let key = e
            .strip_prefix(&base)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let mut bytes = std::fs::read(&e).unwrap();
        if key == "woodpecker/1/0/footer.blk" {
            let n = bytes.len();
            bytes[n - 20] ^= 0xff;
        }
        if key == "woodpecker/1/2/1.blk" {
            continue; // a lost block in the active segment
        }
        mem.put(&key, bytes, None);
    }
    let l = analyze(&mem, Format::Woodpecker, "woodpecker/")
        .await
        .unwrap();
    assert!(
        l.issues
            .iter()
            .any(|i| i.severity == Severity::Error && i.message.contains("CRC32")),
        "{:?}",
        l.issues
    );
    assert!(
        l.issues
            .iter()
            .any(|i| i.severity == Severity::Error && i.message.contains("gaps"))
            || l.tables[0].rows.iter().any(|r| r[1] == "2" && r[3] == "1"),
        "{:?}",
        l.issues
    );
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut v = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().is_dir() {
            v.extend(walk(&e.path()));
        } else {
            v.push(e.path());
        }
    }
    v
}

#[tokio::test]
async fn slatedb_database() {
    let src = samples("slatedb");
    let found = detect(&src, "").await.unwrap();
    let d = found
        .iter()
        .find(|d| d.format == Format::Slatedb)
        .expect("detected");
    assert_eq!(d.root, "db/");
    let l = analyze(&src, Format::Slatedb, &d.root).await.unwrap();
    assert!(l.decoded);
    assert!(!l.has_errors(), "{:?}", l.issues);
    let fact = |n: &str| {
        l.facts
            .iter()
            .find(|f| f.label == n)
            .map(|f| f.value.clone())
            .unwrap()
    };
    assert!(
        fact("Fencing events").parse::<u32>().unwrap() >= 1,
        "two runs fence once: {}",
        fact("Fencing events")
    );
    assert!(fact("Writer epoch").parse::<u32>().unwrap() >= 2);
    assert_eq!(fact("Checkpoints"), "1");
    assert!(l.timeline.iter().any(|e| e.kind == "fence"));
    assert!(l.snapshots.len() >= 3);
    // the SSTs the latest manifest lists exist
    assert!(!l.tables[0].rows.iter().any(|r| r[4] == "MISSING"));
}

#[tokio::test]
async fn slatedb_unknown_version_and_orphans() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples/slatedb");
    let mut mem = decoders::MemSource::default();
    for e in walk(&base) {
        let key = e
            .strip_prefix(&base)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        mem.put(&key, std::fs::read(&e).unwrap(), None);
    }
    mem.put(
        "db/compacted/01ARZ3NDEKTSV4RRFFQ69G5FAV.sst",
        vec![0u8; 100],
        None,
    );
    mem.put(
        "db/manifest/00000000000000000099.manifest",
        vec![0, 7, 0, 0, 0, 0, 0, 0],
        None,
    );
    let l = analyze(&mem, Format::Slatedb, "db/").await.unwrap();
    assert!(
        l.issues
            .iter()
            .any(|i| i.message.contains("unknown manifest format version 7")),
        "{:?}",
        l.issues
    );
    assert!(
        l.issues.iter().any(|i| i.message.contains("orphans")),
        "{:?}",
        l.issues
    );
}

#[tokio::test]
async fn cadence_on_any_prefix() {
    let mut mem = decoders::MemSource::default();
    let t0 = chrono::Utc::now() - chrono::Duration::hours(1);
    for i in 0..100 {
        mem.put(
            &format!("warpstream/_segments/seg-{i:04}"),
            vec![0u8; 1000],
            Some(t0 + chrono::Duration::seconds(i * 2)),
        );
    }
    let found = detect(&mem, "").await.unwrap();
    assert!(found.iter().any(|d| d.format == Format::Warpstream));
    let l = analyze(&mem, Format::Warpstream, "warpstream/")
        .await
        .unwrap();
    assert!(!l.decoded);
    assert!(l.issues[0].message.contains("not published"));
    assert!(
        l.facts
            .iter()
            .any(|f| f.label == "Median interval" && f.value == "2000 ms")
    );
}

/// A sample whose GETs of some keys fail, and whose listing leaves out the
/// keys holding `dropped`, said cut or not.
struct Flaky {
    inner: DirSource,
    fail: Vec<&'static str>,
    dropped: &'static str,
    cut: bool,
}

#[async_trait::async_trait]
impl decoders::Source for Flaky {
    async fn list(&self, prefix: &str) -> Result<decoders::Listing, decoders::SourceError> {
        let mut l = self.inner.list(prefix).await?;
        if !self.dropped.is_empty() {
            l.objects.retain(|o| !o.key.contains(self.dropped));
        }
        l.cut = self.cut;
        Ok(l)
    }
    async fn get(
        &self,
        key: &str,
        range: Option<(u64, u64)>,
    ) -> Result<bytes::Bytes, decoders::SourceError> {
        if self.fail.iter().any(|f| key.ends_with(f)) {
            return Err(decoders::SourceError("503 SlowDown".into()));
        }
        self.inner.get(key, range).await
    }
}

#[tokio::test]
async fn a_read_that_fails_is_an_issue_not_the_end() {
    for (sample, root, format, fail) in [
        (
            "slatedb",
            "db/",
            Format::Slatedb,
            vec!["manifest/00000000000000000003.manifest"],
        ),
        (
            "lakefs",
            "sample/",
            Format::Lakefs,
            vec!["_lakefs/c3e92946de4eef6f941cf2b5268876ab501598df8d5ecb411267be73cb9e767e"],
        ),
        (
            "woodpecker",
            "",
            Format::Woodpecker,
            vec!["1/0/footer.blk", "1/2/0.blk"],
        ),
    ] {
        let n = fail.len();
        let src = Flaky {
            inner: samples(sample),
            fail,
            dropped: "",
            cut: false,
        };
        let l = analyze(&src, format, root)
            .await
            .unwrap_or_else(|e| panic!("{sample}: {e}"));
        let said = l
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Error && i.message.contains("cannot read"))
            .count();
        assert_eq!(said, n, "{sample}: {:?}", l.issues);
    }
}

#[tokio::test]
async fn a_cut_listing_says_so_and_calls_nothing_missing() {
    for (sample, root, format, dropped) in [
        (
            "iceberg",
            "warehouse/demo.db/events/",
            Format::Iceberg,
            "/data/",
        ),
        ("slatedb", "db/", Format::Slatedb, "/compacted/"),
    ] {
        let missing = |l: &decoders::Layout| {
            l.issues
                .iter()
                .any(|i| i.message.contains("but missing") || i.message.contains("orphans"))
        };
        let flaky = |cut| Flaky {
            inner: samples(sample),
            fail: vec![],
            dropped,
            cut,
        };
        // listed to its end, what is not there is missing
        let whole = analyze(&flaky(false), format, root).await.unwrap();
        assert!(missing(&whole), "{sample}: {:?}", whole.issues);
        let cut = analyze(&flaky(true), format, root).await.unwrap();
        assert!(!missing(&cut), "{sample}: {:?}", cut.issues);
        assert!(
            cut.issues
                .iter()
                .any(|i| i.severity == Severity::Warning && i.message.contains("listing stopped")),
            "{sample}: {:?}",
            cut.issues
        );
    }
}

/// An Avro file of `rows` under `schema` (uncompressed).
fn avro(schema: &str, rows: Vec<apache_avro::types::Value>) -> Vec<u8> {
    let schema = apache_avro::Schema::parse_str(schema).expect("a schema");
    let mut w = apache_avro::Writer::new(&schema, Vec::new()).expect("a writer");
    for row in rows {
        w.append_value(row).expect("a row of the schema");
    }
    w.into_inner().expect("the file")
}

/// A format-version 3 table at `wh/t/`, as its spec lays it out (no
/// engine sonde's tools run writes v3 yet): two data files, one Puffin file
/// holding a deletion vector for each (the vector of `b.parquet` at
/// `second`, its offset and size), statistics files listed by the table
/// metadata (one beside the data), and a Puffin file nothing names.
fn iceberg_v3(second: (i64, i64)) -> decoders::MemSource {
    use apache_avro::types::Value as V;
    const LIST: &str = r#"{"type":"record","name":"manifest_file","fields":[
        {"name":"manifest_path","type":"string"},
        {"name":"manifest_length","type":"long"},
        {"name":"partition_spec_id","type":"int"},
        {"name":"content","type":"int"},
        {"name":"sequence_number","type":"long"},
        {"name":"added_snapshot_id","type":"long"},
        {"name":"first_row_id","type":["null","long"]}]}"#;
    const MANIFEST: &str = r#"{"type":"record","name":"manifest_entry","fields":[
        {"name":"status","type":"int"},
        {"name":"snapshot_id","type":["null","long"]},
        {"name":"data_file","type":{"type":"record","name":"r2","fields":[
            {"name":"content","type":"int"},
            {"name":"file_path","type":"string"},
            {"name":"file_format","type":"string"},
            {"name":"record_count","type":"long"},
            {"name":"file_size_in_bytes","type":"long"},
            {"name":"first_row_id","type":["null","long"]},
            {"name":"referenced_data_file","type":["null","string"]},
            {"name":"content_offset","type":["null","long"]},
            {"name":"content_size_in_bytes","type":["null","long"]}]}}]}"#;
    let at = "s3://b/wh/t";
    let some = |v: V| V::Union(1, Box::new(v));
    let none = || V::Union(0, Box::new(V::Null));
    let rec = |fields: Vec<(&str, V)>| {
        V::Record(
            fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    };
    let entry = |file: V| {
        rec(vec![
            ("status", V::Int(1)),
            ("snapshot_id", some(V::Long(1))),
            ("data_file", file),
        ])
    };
    let data = |name: &str, records: i64, bytes: i64, first: i64| {
        entry(rec(vec![
            ("content", V::Int(0)),
            ("file_path", V::String(format!("{at}/data/{name}"))),
            ("file_format", V::String("PARQUET".into())),
            ("record_count", V::Long(records)),
            ("file_size_in_bytes", V::Long(bytes)),
            ("first_row_id", some(V::Long(first))),
            ("referenced_data_file", none()),
            ("content_offset", none()),
            ("content_size_in_bytes", none()),
        ]))
    };
    let vector = |of: &str, deleted: i64, (offset, size): (i64, i64)| {
        entry(rec(vec![
            ("content", V::Int(1)),
            ("file_path", V::String(format!("{at}/data/dv-1.puffin"))),
            ("file_format", V::String("PUFFIN".into())),
            ("record_count", V::Long(deleted)),
            ("file_size_in_bytes", V::Long(200)),
            ("first_row_id", none()),
            (
                "referenced_data_file",
                some(V::String(format!("{at}/data/{of}"))),
            ),
            ("content_offset", some(V::Long(offset))),
            ("content_size_in_bytes", some(V::Long(size))),
        ]))
    };
    let listed = |name: &str, content: i32| {
        rec(vec![
            ("manifest_path", V::String(format!("{at}/metadata/{name}"))),
            ("manifest_length", V::Long(1)),
            ("partition_spec_id", V::Int(0)),
            ("content", V::Int(content)),
            ("sequence_number", V::Long(1)),
            ("added_snapshot_id", V::Long(1)),
            (
                "first_row_id",
                if content == 0 {
                    some(V::Long(0))
                } else {
                    none()
                },
            ),
        ])
    };
    let metadata = serde_json::json!({
        "format-version": 3,
        "table-uuid": "6c5e0a1b-1f2e-4d3c-8b7a-0123456789ab",
        "location": at,
        "last-updated-ms": chrono::Utc::now().timestamp_millis(),
        "next-row-id": 150,
        "current-snapshot-id": 1,
        "refs": { "main": { "snapshot-id": 1, "type": "branch" } },
        "snapshots": [{
            "snapshot-id": 1,
            "sequence-number": 1,
            "first-row-id": 0,
            "timestamp-ms": chrono::Utc::now().timestamp_millis(),
            "summary": { "operation": "delete" },
            "manifest-list": format!("{at}/metadata/snap-1.avro"),
        }],
        "statistics": [{
            "snapshot-id": 1,
            "statistics-path": format!("{at}/data/stats-1.puffin"),
            "file-size-in-bytes": 64,
            "file-footer-size-in-bytes": 16,
            "blob-metadata": [],
        }],
        "partition-statistics": [{
            "snapshot-id": 1,
            "statistics-path": format!("{at}/metadata/partition-stats-1.parquet"),
            "file-size-in-bytes": 32,
        }],
    });
    let mut s = decoders::MemSource::default();
    s.put("wh/t/metadata/v1.metadata.json", metadata.to_string(), None);
    s.put(
        "wh/t/metadata/snap-1.avro",
        avro(LIST, vec![listed("m0.avro", 0), listed("m1.avro", 1)]),
        None,
    );
    s.put(
        "wh/t/metadata/m0.avro",
        avro(
            MANIFEST,
            vec![
                data("a.parquet", 100, 1000, 0),
                data("b.parquet", 50, 500, 100),
            ],
        ),
        None,
    );
    s.put(
        "wh/t/metadata/m1.avro",
        avro(
            MANIFEST,
            vec![
                vector("a.parquet", 7, (4, 40)),
                vector("b.parquet", 3, second),
            ],
        ),
        None,
    );
    s.put(
        "wh/t/metadata/partition-stats-1.parquet",
        vec![0u8; 32],
        None,
    );
    s.put("wh/t/data/a.parquet", vec![0u8; 1000], None);
    s.put("wh/t/data/b.parquet", vec![0u8; 500], None);
    s.put("wh/t/data/dv-1.puffin", vec![0u8; 200], None);
    s.put("wh/t/data/stats-1.puffin", vec![0u8; 64], None);
    s.put("wh/t/data/stray.puffin", vec![0u8; 30], None);
    s
}

/// Iceberg v3: deletion vectors are blobs of one Puffin file (one file, its
/// bytes once), the rows they delete are not live, and the statistics files
/// the metadata lists are the table's, not orphans.
#[tokio::test]
async fn iceberg_v3_deletion_vectors_and_statistics() {
    let src = iceberg_v3((44, 60));
    let found = detect(&src, "").await.unwrap();
    assert!(
        found
            .iter()
            .any(|d| d.format == Format::Iceberg && d.root == "wh/t/"),
        "{found:?}"
    );
    let l = analyze(&src, Format::Iceberg, "wh/t/").await.unwrap();
    assert!(l.decoded && !l.has_errors(), "{:?}", l.issues);
    assert_eq!(l.format_version.as_deref(), Some("format-version 3"));
    let fact = |n: &str| {
        l.facts
            .iter()
            .find(|f| f.label == n)
            .map(|f| f.value.clone())
            .unwrap_or_else(|| panic!("no fact {n}"))
    };
    assert_eq!(fact("Live data files"), "2");
    assert_eq!(fact("Live delete files"), "1", "one Puffin file");
    assert_eq!(fact("Deletion vectors"), "2");
    assert_eq!(fact("Deleted rows"), "10");
    assert_eq!(fact("Live records"), "140", "150 less the 10 deleted");
    // the Puffin file's 200 bytes once, not once a vector
    assert_eq!(fact("Live bytes"), sonde_fmt::bytes(1000 + 500 + 200));
    assert_eq!(fact("Statistics files"), "2");
    // the stray file alone is an orphan: not the vectors' file, not the statistics
    let orphans = &l
        .tables
        .iter()
        .find(|t| t.title == "Orphan data files")
        .unwrap()
        .rows;
    assert_eq!(orphans.len(), 1, "{orphans:?}");
    assert_eq!(orphans[0][0], "wh/t/data/stray.puffin");
    assert!(fact("Orphan data files").starts_with("1 "));
    assert!(l.headline.contains("3 live files"), "{}", l.headline);
    // the snapshot's state says the same
    let snap = &l.snapshots[0];
    let said = |n: &str| {
        snap.facts
            .iter()
            .find(|f| f.label == n)
            .map(|f| f.value.as_str())
    };
    assert_eq!(said("Records"), Some("140"));
    assert_eq!(said("Deleted rows"), Some("10"));
    assert_eq!(said("Data files"), Some("3"));
    let files = &snap.tables[0].rows;
    assert_eq!(files.len(), 4);
    let vectors: Vec<_> = files.iter().filter(|r| r[1] == "deletion vector").collect();
    assert_eq!(vectors.len(), 2, "{files:?}");
    assert_eq!(vectors[1][3], "3");
    assert_eq!(vectors[1][4], sonde_fmt::bytes(60), "the blob's own size");

    // a statistics file the metadata lists and the store lacks is missing
    let mut lost = iceberg_v3((44, 60));
    lost.objects.remove("wh/t/data/stats-1.puffin");
    let l = analyze(&lost, Format::Iceberg, "wh/t/").await.unwrap();
    assert!(
        l.issues.iter().any(
            |i| i.severity == Severity::Error && i.message.contains("wh/t/data/stats-1.puffin")
        ),
        "{:?}",
        l.issues
    );

    // a vector said to lie past its file's end is an error, by name
    let l = analyze(&iceberg_v3((180, 60)), Format::Iceberg, "wh/t/")
        .await
        .unwrap();
    assert!(
        l.issues.iter().any(|i| i.severity == Severity::Error
            && i.message.contains("dv-1.puffin")
            && i.message.contains("180")),
        "{:?}",
        l.issues
    );
}

/// A ULID's 128 bits (Crockford base 32).
fn ulid_bits(s: &str) -> u128 {
    const DIGITS: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    s.bytes().fold(0u128, |v, c| {
        (v << 5) | DIGITS.iter().position(|d| *d == c).expect("a ULID") as u128
    })
}

/// A database whose checkpoint names a manifest older than the newest
/// `MAX_MANIFESTS` the decoder reads: manifest 12 of the sample (its own
/// checkpoint's), then 600 newer ones in which the last SST gave way to
/// another. Returns the store, and the SST only the checkpoint keeps.
fn slatedb_with_an_old_checkpoint() -> (decoders::MemSource, String) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples/slatedb/db");
    let old = std::fs::read(base.join("manifest/00000000000000000012.manifest")).unwrap();
    let before = decoders::slatedb::decode_manifest(&old).unwrap();
    assert_eq!(before.checkpoints[0].manifest_id, 12);
    // the newest L0 SST's id, low half, as the manifest holds it: one bit of it another SST's
    let kept_only = before.l0[0].clone();
    let low = (ulid_bits(&kept_only) as u64).to_le_bytes();
    let places: Vec<usize> = old
        .windows(8)
        .enumerate()
        .filter(|(_, w)| *w == low)
        .map(|(i, _)| i)
        .collect();
    let mut newer = old.clone();
    for at in &places {
        newer[*at] ^= 1;
    }
    let after = decoders::slatedb::decode_manifest(&newer).unwrap();
    assert!(!after.l0.contains(&kept_only), "{:?}", after.l0);
    assert_eq!(after.checkpoints[0].manifest_id, 12);
    let replaced = after.l0[0].clone();

    let mut mem = decoders::MemSource::default();
    for dir in ["compacted", "wal"] {
        for e in walk(&base.join(dir)) {
            let name = e.file_name().unwrap().to_string_lossy().to_string();
            mem.put(
                &format!("db/{dir}/{name}"),
                std::fs::read(&e).unwrap(),
                None,
            );
        }
    }
    mem.put(&format!("db/compacted/{replaced}.sst"), vec![0u8; 64], None);
    mem.put("db/manifest/00000000000000000012.manifest", old, None);
    for id in 13..=612u64 {
        mem.put(
            &format!("db/manifest/{id:020}.manifest"),
            newer.clone(),
            None,
        );
    }
    (mem, kept_only)
}

/// A checkpoint keeps its manifest's SSTs however many manifests came
/// after: the manifest is read though it is older than those decoded, and
/// when it cannot be, the orphans are unknown rather than found.
#[tokio::test]
async fn slatedb_ssts_of_an_old_checkpoint_are_not_orphans() {
    let (mem, kept_only) = slatedb_with_an_old_checkpoint();
    assert!(612 - 12 > decoders::slatedb::MAX_MANIFESTS);
    let state = |l: &decoders::Layout, sst: &str| {
        l.tables[0]
            .rows
            .iter()
            .find(|r| r[0] == sst)
            .map(|r| r[4].clone())
            .expect("the SST's row")
    };
    let orphans = |l: &decoders::Layout| {
        l.issues
            .iter()
            .any(|i| i.message.contains("orphans awaiting GC"))
    };
    let l = analyze(&mem, Format::Slatedb, "db/").await.unwrap();
    assert!(l.decoded && !l.has_errors(), "{:?}", l.issues);
    assert_eq!(state(&l, &kept_only), "checkpoint");
    assert!(!orphans(&l), "{:?}", l.issues);
    assert!(l.tables[0].rows.iter().all(|r| r[4] != "orphan"));

    // the checkpoint's manifest gone: what it kept is not known
    let mut gone = mem.clone();
    gone.objects
        .remove("db/manifest/00000000000000000012.manifest");
    let l = analyze(&gone, Format::Slatedb, "db/").await.unwrap();
    assert_eq!(state(&l, &kept_only), "unknown");
    assert!(!orphans(&l), "{:?}", l.issues);
    let said = l
        .issues
        .iter()
        .find(|i| i.message.contains("manifest 12"))
        .expect("says which manifest");
    assert_eq!(said.severity, Severity::Warning);
    assert!(said.message.contains("1 compacted SST"), "{}", said.message);
    let lag = l.facts.iter().find(|f| f.label == "GC lag").unwrap();
    assert!(lag.value.contains("? orphan"), "{}", lag.value);

    // and an SST no manifest names is still an orphan when all were read
    let mut stray = mem.clone();
    stray.put(
        "db/compacted/01ARZ3NDEKTSV4RRFFQ69G5FAV.sst",
        vec![0u8; 100],
        None,
    );
    let l = analyze(&stray, Format::Slatedb, "db/").await.unwrap();
    assert!(orphans(&l), "{:?}", l.issues);
    assert_eq!(state(&l, "01ARZ3NDEKTSV4RRFFQ69G5FAV"), "orphan");
    assert_eq!(state(&l, &kept_only), "checkpoint");
}
