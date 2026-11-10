//! Every probe's judge against recorded responses (`fixtures/<probe>/<vendor>.json`).
//!
//! `ceph-rgw.json` and `minio.json` (RustFS, a MinIO-compatible server) were
//! recorded from live runs (`SONDE_RECORD_FIXTURES`); `aws.json` files are
//! written from AWS's documented behavior and must pass.

use std::path::Path;

use probes::store::Fixture;
use probes::{Step, Steps, Verdict};

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

fn step(label: &str, status: u16, body: &str) -> Step {
    Step {
        label: label.into(),
        status: Some(status),
        code: (status == 403).then(|| "AccessDenied".into()),
        body: body.into(),
        ..Default::default()
    }
}

/// The steps a probe records in a scratch bucket that can be read but not
/// written: every write is 403, reads are 404, listings are empty.
fn read_only_bucket(labels: &[(&str, u16)]) -> Steps {
    Steps(labels.iter().map(|(l, s)| step(l, *s, "")).collect())
}

#[test]
fn a_probe_whose_setup_failed_is_an_error_not_a_verdict() {
    let puts: Vec<(String, u16)> = (0..20).map(|i| (format!("put-{i}"), 403)).collect();
    let mut law: Vec<(&str, u16)> = puts.iter().map(|(l, s)| (l.as_str(), *s)).collect();
    law.push(("list", 200));
    let cases: Vec<(&str, Steps)> = vec![
        ("consistency.list-after-write", read_only_bucket(&law)),
        (
            "consistency.read-after-delete",
            read_only_bucket(&[("put", 403), ("delete", 403), ("get", 404), ("head", 404)]),
        ),
        (
            "consistency.read-after-overwrite",
            read_only_bucket(&[
                ("put-0", 403),
                ("get-0", 404),
                ("put-1", 403),
                ("get-1", 404),
            ]),
        ),
        (
            "cond.put-if-match",
            read_only_bucket(&[
                ("create", 403),
                ("stale", 403),
                ("current", 403),
                ("missing", 403),
            ]),
        ),
        (
            "cond.copy-if-match",
            read_only_bucket(&[
                ("source", 403),
                ("wrong-source", 404),
                ("right-source", 404),
            ]),
        ),
        (
            "cond.complete-if-none-match",
            read_only_bucket(&[("create-upload", 403)]),
        ),
        (
            "cond.complete-if-none-match",
            // only the racing PUT failed: the upload completing is no broken guarantee
            read_only_bucket(&[
                ("create-upload", 200),
                ("part", 200),
                ("racing-put", 403),
                ("complete", 200),
                ("head", 200),
            ]),
        ),
        (
            "mpu.upload-part-copy",
            read_only_bucket(&[("source", 403), ("create", 403)]),
        ),
        (
            "presign.expiry",
            read_only_bucket(&[("put", 403), ("fresh", 404), ("expired", 403)]),
        ),
        (
            "list.start-after",
            read_only_bucket(&[
                ("put-a", 403),
                ("put-b", 403),
                ("put-c", 403),
                ("put-d", 403),
            ]),
        ),
    ];
    for (id, steps) in cases {
        let j = probes::probe(id).unwrap().judge(&steps);
        assert_eq!(j.verdict, Verdict::Error, "{id}: {}", j.summary);
        assert!(
            j.summary.starts_with("could not run"),
            "{id}: {}",
            j.summary
        );
    }
}

#[test]
fn if_match_on_a_missing_key_that_writes_is_a_fail() {
    let steps = Steps(vec![
        step("create", 200, ""),
        step("stale", 412, ""),
        step("current", 200, ""),
        step("missing", 200, ""),
    ]);
    let j = probes::probe("cond.put-if-match").unwrap().judge(&steps);
    assert_eq!(j.verdict, Verdict::Fail, "{}", j.summary);
}
