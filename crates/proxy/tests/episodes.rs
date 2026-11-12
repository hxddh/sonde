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
            e.title(),
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
    let titles: Vec<String> = slate.iter().map(|e| e.title()).collect();
    assert_eq!(
        titles
            .iter()
            .filter(|t| t.starts_with("manifest ") && t.ends_with(" written"))
            .count(),
        12
    );
    assert!(
        titles
            .iter()
            .any(|t| t == "WAL 1 written empty (fencing marker)"),
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
        .find(|e| e.title() == "manifest 3 written")
        .unwrap();
    assert!(manifest.notes.iter().any(|n| n.contains("If-None-Match")));
    assert!(manifest.notes.iter().any(|n| n.starts_with("after ")));
    assert!(eps.iter().all(|e| e.outcome == Outcome::Ok));
    // a format's shared habit is said once, among the operations' notes (no finding)
    let shared: Vec<(usize, String)> = sonde_proxy::episode::shared_notes(&eps)
        .into_iter()
        .map(|(_, n, with, _)| (with, n))
        .collect();
    assert!(
        shared
            .iter()
            .any(|(n, note)| *n == 27 && note.starts_with("conditional create")),
        "{shared:?}"
    );

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
            mpu[0].title().contains("3 parts · 12.0 MiB"),
            "{}",
            mpu[0].title()
        );
        assert!(!mpu[0].title().contains("not completed"));
        assert_eq!(mpu[0].requests(), 5);
        assert_eq!(eps.len(), s.records.len() - 4, "{name}");
        assert!(observations(&s.records, &eps).is_empty(), "{name}");
    }
}

#[test]
fn pyiceberg_run_reads_as_table_commits() {
    let s = load("iceberg.sonde");
    let eps = episodes(&s.records);
    let titles: Vec<String> = eps.iter().map(|e| e.title()).collect();
    // create, append, append, overwrite, delete — then a file nothing commits
    assert_eq!(eps.len(), 6, "{titles:#?}");
    assert!(titles[0].starts_with("commit: table metadata v0"));
    for (i, t) in titles[1..5].iter().enumerate() {
        assert!(
            t.starts_with(&format!("commit: table metadata v{} · ", i + 1)),
            "{t}"
        );
    }
    assert_eq!(titles[5], "1 file written, no commit followed (72 B)");
    assert!(eps.iter().all(|e| e.format == Some(Format::Iceberg)));
    let obs = observations(&s.records, &eps);
    assert!(
        obs.iter()
            .any(|o| o.level == "warn" && o.text.starts_with("files were written with no commit"))
    );
    // pyarrow uploads every file in parts
    assert!(
        obs.iter().any(|o| o
            .text
            .starts_with("21 files under 5 MiB were uploaded as multipart uploads")),
        "{obs:?}"
    );
}

#[test]
fn delta_rs_run_reads_as_commits() {
    let s = load("delta.sonde");
    let eps = episodes(&s.records);
    let titles: Vec<String> = eps.iter().map(|e| e.title()).collect();
    assert_eq!(
        &titles[..4],
        [
            "commit 0 · 1 file (1.2 KiB)",
            "commit 1 · 1 file (1004 B)",
            "commit 2 · 1 file (883 B)",
            "commit 3 · 1 file (904 B)"
        ]
    );
    assert!(
        eps[..4]
            .iter()
            .all(|e| e.format == Some(Format::Delta) && e.outcome == Outcome::Ok)
    );
    // a format's shared habit is said once, among the operations' notes (no finding)
    let shared: Vec<(usize, String)> = sonde_proxy::episode::shared_notes(&eps)
        .into_iter()
        .map(|(_, n, with, _)| (with, n))
        .collect();
    assert!(
        shared
            .iter()
            .any(|(n, note)| *n == 4 && note.starts_with("conditional create (If-None-Match: *)")),
        "{shared:?}"
    );
    let obs = observations(&s.records, &eps);
    assert!(
        obs.iter()
            .any(|o| o.text.ends_with("_delta_log/ was listed 15 times"))
    );
}

/// A recording of pylance (`tools/gen_lance.py`): three appends, a delete, an
/// index, a compaction and two reads are eight operations, not 63 requests.
#[test]
fn lance_run_reads_as_versions() {
    let s = load("lance.sonde");
    assert_eq!(s.records.len(), 63);
    let eps = episodes(&s.records);
    let titles: Vec<String> = eps.iter().map(|e| e.title()).collect();
    assert_eq!(
        titles,
        [
            "version 1 committed · 1 data file (126 KiB)",
            "version 2 committed · 1 data file (142 KiB)",
            "version 3 committed · 1 data file (135 KiB)",
            "version 4 committed · rows deleted",
            "version 5 committed · an index (33.5 KiB)",
            "version 6 committed · 1 data file (439 KiB)",
            "version 7 committed · an index (36.6 KiB)",
            "read version 7 · 3 ranges of 1 file (112 KiB)"
        ]
    );
    assert!(
        eps.iter()
            .all(|e| e.format == Some(Format::Lance) && e.outcome == Outcome::Ok)
    );
    // a format's shared habit is said once, among the operations' notes (no finding)
    let shared: Vec<(usize, String)> = sonde_proxy::episode::shared_notes(&eps)
        .into_iter()
        .map(|(_, n, with, _)| (with, n))
        .collect();
    assert!(
        shared
            .iter()
            .any(|(n, note)| *n == 7 && note.starts_with("conditional create (If-None-Match: *)")),
        "{shared:?}"
    );
}

/// Every title, note and observation of the recorded sessions is said by
/// phrases with an English template, and those phrases say exactly the
/// English text (the app says them in Chinese from the same keys).
#[test]
fn every_text_is_phrases() {
    use sonde_proxy::phrase::en;
    for name in [
        "slatedb.sonde",
        "iceberg.sonde",
        "delta.sonde",
        "lance.sonde",
        "sdk-python.sonde",
        "sdk-java.sonde",
    ] {
        let s = load(name);
        let eps = episodes(&s.records);
        for e in &eps {
            assert!(!e.title_text.is_empty(), "{name}: {}", e.title());
            assert_eq!(en(&e.title_text), e.title(), "{name}");
            assert_eq!(e.notes_text.len(), e.notes.len(), "{name}: {}", e.title());
            for (n, t) in e.notes.iter().zip(&e.notes_text) {
                assert_eq!(&t.en(), n, "{name}");
            }
            for t in e.title_text.iter().chain(&e.notes_text) {
                assert!(
                    !t.en().is_empty() || t.args.is_empty(),
                    "{name}: no template for {}",
                    t.key
                );
            }
        }
        for o in observations(&s.records, &eps) {
            assert_eq!(en(&o.phrases), o.text, "{name}");
        }
    }
}
