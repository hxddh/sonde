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

#[test]
fn every_judge_calls_a_bucket_it_could_not_write_an_error() {
    let numbered = |n: usize, extra: &[(&str, u16)]| -> Steps {
        let mut v: Vec<Step> = (0..n).map(|i| step(&format!("put-{i}"), 403, "")).collect();
        v.extend(extra.iter().map(|(l, s)| step(l, *s, "")));
        Steps(v)
    };
    let mut special = numbered(14, &[("list", 200)]);
    special
        .0
        .extend((0..14).map(|i| step(&format!("get-{i}"), 404, "")));
    let cases: Vec<(&str, Steps)> = vec![
        (
            "range.bounds",
            read_only_bucket(&[
                ("put", 403),
                ("first", 404),
                ("last", 404),
                ("suffix", 404),
                ("open", 404),
                ("past-end", 404),
            ]),
        ),
        (
            "range.multi",
            read_only_bucket(&[("put", 403), ("get", 404)]),
        ),
        (
            "range.unsatisfiable",
            read_only_bucket(&[
                ("put", 403),
                ("past", 404),
                ("put-empty", 403),
                ("empty", 404),
            ]),
        ),
        (
            "range.unsatisfiable",
            // the empty object is half of the test
            read_only_bucket(&[
                ("put", 200),
                ("past", 416),
                ("put-empty", 403),
                ("empty", 404),
            ]),
        ),
        ("list.token-stability", numbered(30, &[("page-0", 200)])),
        (
            "list.token-stability",
            // the 30 keys are there, the two written between pages are not
            Steps(
                (0..30)
                    .map(|i| step(&format!("put-{i}"), 200, ""))
                    .chain([
                        step("page-0", 200, ""),
                        step("insert-before", 403, ""),
                        step("insert-after", 403, ""),
                        step("keys", 200, "[]"),
                    ])
                    .collect(),
            ),
        ),
        ("keys.special", special),
        (
            "delete.batch-limit",
            read_only_bucket(&[("batch-1000", 403), ("batch-1001", 403)]),
        ),
        (
            "checksum.crc32.single",
            read_only_bucket(&[("good", 403), ("head", 404), ("bad", 403)]),
        ),
        (
            "checksum.crc32.mpu-composite",
            read_only_bucket(&[("create", 403)]),
        ),
        (
            "checksum.crc32.mpu-composite",
            // the parts were refused: the Complete's 400 says nothing of checksums
            read_only_bucket(&[
                ("create", 200),
                ("part-1", 403),
                ("part-2", 403),
                ("complete", 400),
                ("head", 404),
            ]),
        ),
        (
            "checksum.crc64nvme.mpu-full-object",
            read_only_bucket(&[("create", 403)]),
        ),
        (
            "checksum.trailing",
            read_only_bucket(&[("put", 403), ("head", 404)]),
        ),
        ("mpu.min-part-size", read_only_bucket(&[("create", 403)])),
        (
            "mpu.min-part-size",
            read_only_bucket(&[
                ("create", 200),
                ("small-part", 403),
                ("last-part", 403),
                ("complete-small", 400),
            ]),
        ),
        ("mpu.out-of-order", read_only_bucket(&[("create", 403)])),
        (
            "mpu.duplicate-complete",
            read_only_bucket(&[("create", 403)]),
        ),
        (
            "mpu.concurrent-same-key",
            read_only_bucket(&[("create-a", 403), ("create-b", 403)]),
        ),
        (
            "presign.clock-skew",
            read_only_bucket(&[
                ("put", 403),
                ("ahead-5m", 404),
                ("ahead-20m", 403),
                ("behind-20m", 403),
                ("ahead-20m-get", 403),
            ]),
        ),
        (
            "meta.size-limit",
            read_only_bucket(&[("under", 403), ("head", 404), ("over", 403)]),
        ),
        (
            "sse-c",
            read_only_bucket(&[
                ("put", 403),
                ("no-key", 404),
                ("wrong-key", 404),
                ("right-key", 404),
            ]),
        ),
        (
            "express.append",
            read_only_bucket(&[
                ("create", 403),
                ("append", 403),
                ("wrong-offset", 403),
                ("head", 404),
            ]),
        ),
        (
            "cond.put-if-none-match",
            read_only_bucket(&[("first", 403), ("second", 403), ("read", 404)]),
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
fn a_refusal_that_is_about_the_feature_is_still_a_verdict() {
    // 403 for a signature the store does not know is not "may not write"
    let mut put = step("put", 403, "");
    put.code = Some("SignatureDoesNotMatch".into());
    let j = probes::probe("checksum.trailing")
        .unwrap()
        .judge(&Steps(vec![put, step("head", 404, "")]));
    assert_eq!(j.verdict, Verdict::Unsupported, "{}", j.summary);
    // a store without multipart: unsupported, not "could not run"
    let mut create = step("create", 501, "");
    create.code = Some("NotImplemented".into());
    let j = probes::probe("checksum.crc32.mpu-composite")
        .unwrap()
        .judge(&Steps(vec![create]));
    assert_eq!(j.verdict, Verdict::Unsupported, "{}", j.summary);
}

/// The special keys as a store that keeps every key answers them, the two
/// with dot segments as sonde answers them itself.
fn special_keys(refuse: &[usize]) -> Steps {
    let mut v = vec![];
    let mut listed = vec![];
    for (i, k) in probes::suite::listing::SPECIAL.iter().enumerate() {
        if s3raw::has_dot_segment(k) {
            for l in ["put", "get"] {
                v.push(Step {
                    label: format!("{l}-{i}"),
                    body: s3raw::client::DOT_SEGMENT.into(),
                    ..Default::default()
                });
            }
        } else if refuse.contains(&i) {
            v.push(step(&format!("put-{i}"), 400, ""));
            v.push(step(&format!("get-{i}"), 404, ""));
        } else {
            v.push(step(&format!("put-{i}"), 200, ""));
            v.push(step(&format!("get-{i}"), 200, &format!("body-{i}")));
            listed.push(k.to_string());
        }
    }
    v.push(step("list", 200, ""));
    v.push(Step {
        label: "listed".into(),
        body: serde_json::to_string(&listed).unwrap(),
        ..Default::default()
    });
    Steps(v)
}

#[test]
fn keys_sonde_cannot_send_are_said_and_not_held_against_the_store() {
    let p = probes::probe("keys.special").unwrap();
    // judged by the keys sent: a store that round-trips them all passes
    let j = p.judge(&special_keys(&[]));
    assert_eq!(j.verdict, Verdict::Pass, "{}", j.summary);
    assert_eq!(j.summary, "all 12 keys round-trip");
    let said = j.observations.join(" ");
    assert!(
        said.contains("\"./dot\" and \"a/../b\" not tested"),
        "{said}"
    );
    assert!(said.contains("sonde cannot send"), "{said}");
    // what the store does refuse is still counted, of the keys it was sent
    let j = p.judge(&special_keys(&[0]));
    assert_eq!(j.verdict, Verdict::Differs, "{}", j.summary);
    assert!(
        j.summary
            .starts_with("1 of 12 keys do not round-trip: \"a b\""),
        "{}",
        j.summary
    );
    assert!(!j.summary.contains("a/../b"), "{}", j.summary);
    assert!(j.observations.iter().any(|o| o.contains("not tested")));
}
