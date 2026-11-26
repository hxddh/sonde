//! `sonde share --redact`, end to end: nothing that names the data, or the
//! file the run is compared with, is anywhere in the page.

use std::path::Path;
use std::process::Command;

use reqlog::{Record, Sink};
use sonde_analysis::run::{RunHeader, RunWriter};

/// Ids as keys hold them: an account, an address, people's numbers.
const IDS: &[&str] = &[
    "123456789012",
    "10.1.2.3",
    "550-12-3456",
    "4111-1111-1111-1111",
    "0f8fad5b-d9cb-469f-a165-70867728950e",
];

fn run_file(path: &Path, requests: usize) {
    let header = RunHeader::new(
        "prod",
        "https://s3.amazonaws.com",
        "us-east-1",
        Some("python etl.py --account 123456789012".into()),
    );
    let w = RunWriter::create(path, &header).unwrap();
    let mut id = 0;
    for n in 0..requests {
        for held in IDS {
            for key in [
                format!("AWSLogs/{held}/CloudTrail/2026/01/02/{held}_{n}.json"),
                format!("hosts/{held}"),
                format!("by-id/{held}.json"),
            ] {
                id += 1;
                let mut r = Record {
                    id,
                    time: chrono::DateTime::from_timestamp_millis(
                        1_750_000_000_000 + id as i64 * 20,
                    )
                    .unwrap(),
                    method: "GET".into(),
                    kind: "GetObject".into(),
                    bucket: Some("acme-payroll".into()),
                    key: Some(key.as_str().into()),
                    status: Some(200),
                    latency_ms: 5.0,
                    ..Default::default()
                };
                r.rare_mut().url = format!("https://acme-payroll.s3.amazonaws.com/{key}");
                r.lists_mut().query = vec![("prefix".into(), format!("accounts/{held}/").into())];
                w.write(&r);
            }
        }
    }
    w.flush().unwrap();
}

#[test]
fn a_redacted_page_holds_no_id_from_a_key_and_no_name_of_the_run_before() {
    let t = tempfile::tempdir().unwrap();
    let before = t.path().join("acct-123456789012-alice.sonde");
    let after = t.path().join("after-10.1.2.3.sonde");
    run_file(&before, 1);
    run_file(&after, 2);
    let page = t.path().join("page.html");
    let o = Command::new(env!("CARGO_BIN_EXE_sonde"))
        .args(["share", "--redact", "-o"])
        .arg(&page)
        .arg(&before)
        .arg(&after)
        .env("SONDE_CONFIG_DIR", t.path().join("config"))
        .env("SONDE_DATA_DIR", t.path().join("data"))
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&o.stderr);
    // a build without the viewer makes no page (the document it would hold
    // is tested in sonde-analysis's report.rs)
    if said.contains("no web viewer") {
        assert!(!page.exists());
        return;
    }
    assert_eq!(o.status.code(), Some(0), "{said}");
    let html = std::fs::read_to_string(&page).unwrap();
    assert!(
        html.contains("\"compare\":{"),
        "the comparison is in the page"
    );
    assert!(
        html.contains("another run · recorded "),
        "the run before, unnamed"
    );
    for held in IDS.iter().chain(&[
        "acct-",
        "alice",
        "acme",
        "payroll",
        "--account",
        "amazonaws",
    ]) {
        assert!(!html.contains(held), "{held} is in the page");
    }
}
