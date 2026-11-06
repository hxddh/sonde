//! Every probe's judge against recorded responses (`fixtures/<probe>/<vendor>.json`).
//!
//! `ceph-rgw.json` and `minio.json` (RustFS, a MinIO-compatible server) were
//! recorded from live runs (`SONDE_RECORD_FIXTURES`); `aws.json` files are
//! written from AWS's documented behavior and must pass.

use std::path::Path;

use probes::store::Fixture;
use probes::{Steps, Verdict};

fn fixtures() -> Vec<(String, Fixture)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut out = vec![];
    for probe in std::fs::read_dir(&dir).unwrap().flatten() {
        for f in std::fs::read_dir(probe.path()).unwrap().flatten() {
            let text = std::fs::read_to_string(f.path()).unwrap();
            let fx: Fixture = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", f.path().display()));
            out.push((f.path().display().to_string(), fx));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn every_probe_has_recorded_fixtures() {
    let fx = fixtures();
    for m in probes::catalog() {
        let n = fx.iter().filter(|(_, f)| f.probe == m.id).count();
        assert!(n >= 2, "{} has {n} fixtures", m.id);
    }
}

#[test]
fn judges_match_fixtures() {
    let mut failures = vec![];
    for (path, f) in fixtures() {
        let p =
            probes::probe(&f.probe).unwrap_or_else(|| panic!("{path}: unknown probe {}", f.probe));
        let j = p.judge(&Steps(f.steps.clone()));
        if j.verdict != f.expect {
            failures.push(format!(
                "{path}: expected {:?}, judged {:?} ({})",
                f.expect, j.verdict, j.summary
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn aws_reference_fixtures_pass() {
    let aws: Vec<_> = fixtures()
        .into_iter()
        .filter(|(_, f)| f.vendor == "aws")
        .collect();
    assert!(aws.len() >= 10);
    for (path, f) in aws {
        assert_eq!(f.expect, Verdict::Pass, "{path}");
    }
}
