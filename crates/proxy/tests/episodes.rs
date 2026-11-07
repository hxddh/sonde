//! Episodes from real recordings (tests/fixtures, made with `sonde run`).

use std::path::Path;

use sonde_proxy::episode::{Format, Kind, Outcome, episodes, observations};
use sonde_proxy::session::Session;

fn load(name: &str) -> Session {
    Session::open(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn slatedb_run_reads_as_slatedb_steps() {
    let s = load("slatedb.sonde");
    let eps = episodes(&s.records);
    for e in &eps {
        eprintln!(
            "{:>3}  {:?}  {}  {:?}",
            e.requests(),
            e.format,
            e.title,
            e.notes
        );
    }
    // every request lands in exactly one episode
    let mut ids: Vec<u64> = eps.iter().flat_map(|e| e.records.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), s.records.len());
    assert!(
        eps.len() * 3 < s.records.len(),
        "{} episodes for {} requests",
        eps.len(),
        s.records.len()
    );

    let slate: Vec<_> = eps
        .iter()
        .filter(|e| e.format == Some(Format::SlateDB))
        .collect();
    let titles: Vec<&str> = slate.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(
        titles
            .iter()
            .filter(|t| t.starts_with("manifest ") && t.ends_with(" written"))
            .count(),
        12
    );
    assert!(
        titles.contains(&"WAL 1 written empty (fencing marker)"),
        "{titles:?}"
    );
    assert!(
        titles
            .iter()
            .any(|t| t.starts_with("WAL ") && t.contains("flushed"))
    );
    assert_eq!(
        titles
            .iter()
            .filter(|t| t.starts_with("SST flushed to L0"))
            .count(),
        4
    );
    assert_eq!(
        titles
            .iter()
            .filter(|t| t.starts_with("compaction state"))
            .count(),
        5
    );
    assert!(
        titles
            .iter()
            .any(|t| t.starts_with("looked for the newest WAL: probed ids 1…")),
        "{titles:?}"
    );
    let manifest = slate
        .iter()
        .find(|e| e.title == "manifest 3 written")
        .unwrap();
    assert!(manifest.notes.iter().any(|n| n.contains("If-None-Match")));
    assert!(manifest.notes.iter().any(|n| n.starts_with("after ")));
    assert!(eps.iter().all(|e| e.outcome == Outcome::Ok));

    let obs = observations(&s.records, &eps);
    assert!(
        obs.iter()
            .any(|o| o.text.contains("gc/manifest.boundary was read")),
        "{obs:?}"
    );
}

#[test]
fn sdk_runs_show_one_multipart_upload() {
    for name in ["sdk-python.sonde", "sdk-java.sonde"] {
        let s = load(name);
        let eps = episodes(&s.records);
        let mpu: Vec<_> = eps.iter().filter(|e| e.kind == Kind::Multipart).collect();
        assert_eq!(mpu.len(), 1, "{name}: {eps:#?}");
        assert!(
            mpu[0].title.contains("3 parts · 12.0 MiB"),
            "{}",
            mpu[0].title
        );
        assert!(!mpu[0].title.contains("not completed"));
        assert_eq!(mpu[0].requests(), 5);
        assert_eq!(eps.len(), s.records.len() - 4, "{name}");
        assert!(observations(&s.records, &eps).is_empty(), "{name}");
    }
}
