//! What a decoder says is phrases beside its English, so the app can say it
//! in another language.

use std::path::Path;

use decoders::phrase::{en, templates, untranslated};
use decoders::{DirSource, Layout, analyze, detect};

async fn layouts() -> Vec<Layout> {
    let mut out = vec![];
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples");
    for dir in std::fs::read_dir(&root).unwrap() {
        let dir = dir.unwrap().path();
        if !dir.is_dir() {
            continue;
        }
        let src = DirSource::new(dir);
        for d in detect(&src, "").await.unwrap() {
            out.push(analyze(&src, d.format, &d.root).await.unwrap());
        }
    }
    out
}

#[tokio::test]
async fn every_text_is_phrases() {
    let all = layouts().await;
    assert!(all.len() >= 4, "{}", all.len());
    let (mut issues, mut events) = (0, 0);
    for l in &all {
        assert!(!l.headline.is_empty());
        assert_eq!(en(&l.headline_text), l.headline);
        for f in l
            .facts
            .iter()
            .chain(l.snapshots.iter().flat_map(|s| &s.facts))
        {
            assert_eq!(en(&f.label_text), f.label);
            if !f.value_text.is_empty() {
                assert_eq!(en(&f.value_text), f.value);
            }
        }
        for i in &l.issues {
            issues += 1;
            assert!(!i.message.is_empty());
            assert_eq!(en(&i.message_text), i.message);
        }
        for e in &l.timeline {
            events += 1;
            assert!(!e.label.is_empty());
            assert_eq!(en(&e.label_text), e.label);
            assert_eq!(en(&e.detail_text), e.detail);
        }
    }
    // (a fresh checkout has one issue fewer: SlateDB's GC lag is read from the files' age)
    assert!(
        issues >= 3 && events > 10,
        "{issues} issues, {events} events"
    );
}

#[test]
fn a_layout_saved_before_phrases_still_loads() {
    let old = r#"{"format":"slatedb","root":"db/","formatVersion":null,"decoded":true,
        "headline":"SlateDB: manifest 3","facts":[{"label":"Writer epoch","value":"2"}],
        "issues":[{"severity":"info","message":"unexpected object x (1 B)"}],
        "timeline":[{"time":null,"kind":"wal","label":"WAL 1","detail":"","key":null,"bytes":null}],
        "tables":[],"snapshots":[],"objects":1,"bytes":1}"#;
    let l: Layout = serde_json::from_str(old).unwrap();
    assert_eq!(l.headline, "SlateDB: manifest 3");
    assert!(l.headline_text.is_empty() && l.facts[0].label_text.is_empty());
    // and what has no phrases is written without the fields
    let json = serde_json::to_value(&l).unwrap();
    assert!(json.get("headlineText").is_none());
    assert!(json["issues"][0].get("messageText").is_none());
}

#[tokio::test]
async fn the_phrase_fields_are_named_for_the_app() {
    let all = layouts().await;
    let l = all.iter().find(|l| !l.issues.is_empty()).unwrap();
    let json = serde_json::to_value(l).unwrap();
    assert!(json["headlineText"][0]["key"].is_string());
    assert!(json["facts"][0]["labelText"][0]["key"].is_string());
    assert!(json["issues"][0]["messageText"][0]["args"].is_object());
    assert!(json["timeline"][0]["labelText"].is_array());
}

#[test]
fn every_key_has_a_chinese_template() {
    let ts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/corephrases.ts");
    let ts = std::fs::read_to_string(ts).unwrap();
    assert_eq!(untranslated(&ts, &templates()), Vec::<String>::new());
}
