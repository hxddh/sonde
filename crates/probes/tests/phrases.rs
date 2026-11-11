//! What a probe says is phrases beside its English: the catalog, each
//! verdict's summary and each observation.

use std::path::Path;

use probes::phrase::{Phrase, en, say, template, templates, untranslated};
use probes::store::Fixture;
use probes::{ProbeMeta, Steps};

fn fixtures() -> Vec<Fixture> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut out = vec![];
    for probe in std::fs::read_dir(&dir).unwrap().flatten() {
        for f in std::fs::read_dir(probe.path()).unwrap().flatten() {
            out.push(serde_json::from_str(&std::fs::read_to_string(f.path()).unwrap()).unwrap());
        }
    }
    out
}

#[test]
fn every_catalog_entry_is_phrases_that_say_its_english() {
    let all = probes::catalog();
    assert!(all.len() >= 29, "{}", all.len());
    for m in &all {
        assert_eq!(en(&m.title_text), m.title, "{}", m.id);
        assert_eq!(en(&m.description_text), m.description, "{}", m.id);
        assert_eq!(en(&m.aws_behavior_text), m.aws_behavior, "{}", m.id);
        assert_eq!(m.title_text[0].key, format!("probe.{}.title", m.id));
    }
    let json = serde_json::to_value(&all[0]).unwrap();
    assert!(json["titleText"][0]["key"].is_string());
    assert!(json["descriptionText"].is_array() && json["awsBehaviorText"].is_array());
}

#[test]
fn every_judgement_is_phrases_that_say_its_english() {
    let fx = fixtures();
    assert!(fx.len() > 50);
    let mut keys = std::collections::BTreeSet::new();
    for f in fx {
        let j = probes::probe(&f.probe)
            .unwrap()
            .judge(&Steps(f.steps.clone()));
        assert!(!j.summary.is_empty(), "{}", f.probe);
        assert_eq!(en(&j.summary_text), j.summary, "{}", f.probe);
        for p in &j.summary_text {
            // a key the table lacks would say nothing
            assert!(template(&p.key).is_some() || p.key == "raw", "{}", p.key);
            // and every argument its template names was given
            let said = say(p);
            let args: String = p.args.values().cloned().collect();
            assert!(
                args.contains('{') || !said.contains('{'),
                "{}: {said}",
                p.key
            );
            keys.insert(p.key.clone());
        }
        assert_eq!(j.observations.len(), j.observations_text.len());
        for (o, t) in j.observations.iter().zip(&j.observations_text) {
            assert_eq!(&en(t), o);
        }
    }
    assert!(keys.len() > 40, "{} keys judged", keys.len());
}

#[test]
fn a_summary_can_be_prefaced_and_an_observation_added() {
    let mut j = probes::Judgement::say(
        probes::Verdict::Pass,
        Phrase::new("judge.range.bounds.pass", &[]),
        &Steps(vec![]),
    );
    j.preface(Phrase::new("judge.timedOut", &[("secs", "30".into())]));
    assert_eq!(
        j.summary,
        "timed out after 30 s; every boundary as AWS answers it"
    );
    assert_eq!(j.summary_text.len(), 2);
    j.observe(Phrase::new("obs.leftBehind", &[("what", "k".into())]));
    assert_eq!(j.observations, vec!["left behind: k"]);
    let v = serde_json::to_value(&j).unwrap();
    assert_eq!(v["summaryText"][0]["key"], "judge.timedOut");
    assert_eq!(v["observationsText"][0][0]["args"]["what"], "k");
}

#[test]
fn a_run_and_a_catalog_saved_before_phrases_still_load() {
    let meta: ProbeMeta = serde_json::from_str(
        r#"{"id":"x","category":"range","title":"T","description":"D","awsBehavior":"A","needs":[]}"#,
    )
    .unwrap();
    assert!(meta.title_text.is_empty());
    let r: probes::ProbeResult = serde_json::from_str(
        r#"{"id":"x","category":"range","verdict":"pass","summary":"s","observations":["o"],
            "steps":[],"exchanges":[],"durationMs":1.0}"#,
    )
    .unwrap();
    assert!(r.summary_text.is_empty() && r.observations_text.is_empty());
    assert!(
        serde_json::to_value(&r)
            .unwrap()
            .get("summaryText")
            .is_none()
    );
}

#[test]
fn no_key_is_listed_twice_or_is_another_crates() {
    let all = templates();
    for (i, (k, _)) in all.iter().enumerate() {
        assert!(all[..i].iter().all(|(o, _)| o != k), "{k} is listed twice");
        assert!(sonde_core::phrase::template(k).is_none(), "{k}");
    }
}

#[test]
fn every_key_has_a_chinese_template() {
    let ts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/corephrases.ts");
    let ts = std::fs::read_to_string(ts).unwrap();
    assert_eq!(untranslated(&ts, &templates()), Vec::<String>::new());
}
