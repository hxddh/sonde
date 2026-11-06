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
        "two sessions fence once: {}",
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
