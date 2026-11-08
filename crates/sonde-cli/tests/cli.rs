//! The CLI's contract: what each command prints on stdout, in which
//! format, and the exit codes docs/cli.md promises.

use std::path::Path;
use std::process::{Command, Output};

fn sonde(dirs: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonde"))
        .args(args)
        .env("SONDE_CONFIG_DIR", dirs.join("config"))
        .env("SONDE_DATA_DIR", dirs.join("data"))
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn a_mistake_in_the_command_is_a_usage_error_and_help_is_not() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(
        sonde(t.path(), &["no-such-command"]).status.code(),
        Some(64)
    );
    assert_eq!(
        sonde(t.path(), &["check", "--tolerance", "x"])
            .status
            .code(),
        Some(64)
    );
    let help = sonde(t.path(), &["--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(stdout(&help).contains("Usage"));
}

#[test]
fn the_demo_records_a_session_that_every_command_reads() {
    let t = tempfile::tempdir().unwrap();
    let demo = sonde(t.path(), &["demo"]);
    assert_eq!(
        demo.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&demo.stderr)
    );
    // the answer is on stdout, the hints on stderr
    let out = stdout(&demo);
    assert!(
        out.contains("checked with HEAD and then read with GET"),
        "{out}"
    );
    assert!(String::from_utf8_lossy(&demo.stderr).contains("sonde run --"));

    let json = sonde(t.path(), &["show", "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["records"].as_array().unwrap().len(), 53);
    assert!(!v["observations"].as_array().unwrap().is_empty());

    let csv = stdout(&sonde(t.path(), &["sessions", "-f", "csv"]));
    let mut lines = csv.lines();
    assert_eq!(lines.next(), Some("session,recorded,program,endpoint,size"));
    let name = lines.next().unwrap().split(',').next().unwrap().to_string();
    assert!(lines.next().is_none());

    // a session by its name alone
    let by_name = sonde(
        t.path(),
        &["show", name.trim_end_matches(".sonde"), "--summary"],
    );
    assert_eq!(by_name.status.code(), Some(0));
    assert!(stdout(&by_name).contains("53 requests"));

    // the same run against itself: the latency table compares each kind
    let path = t.path().join("data/sessions").join(&name);
    let p = path.to_str().unwrap();
    let diff = sonde(t.path(), &["diff", p, p, "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&diff.stdout).unwrap();
    assert!(v["latency"].as_array().unwrap().len() >= 4, "{v}");
}

#[test]
fn check_holds_a_run_to_its_programs_baseline() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    // nothing to hold it to yet
    assert_eq!(
        sonde(t.path(), &["check", "--baseline", "auto"])
            .status
            .code(),
        Some(64)
    );
    assert_eq!(sonde(t.path(), &["check"]).status.code(), Some(64));
    let set = sonde(
        t.path(),
        &["check", "--set-baseline", "--max-requests", "100"],
    );
    assert_eq!(
        set.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    assert!(t.path().join("data/sessions/baselines.json").is_file());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    let c = sonde(t.path(), &["check", "--baseline", "auto", "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&c.stdout).unwrap();
    assert_eq!(c.status.code(), Some(0), "{v}");
    assert!(
        v["check"]["verdicts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["against"] == "baseline")
    );
    // over a budget is exit 1
    assert_eq!(
        sonde(t.path(), &["check", "--max-requests", "10"])
            .status
            .code(),
        Some(1)
    );
    // the baseline survives pruning
    sonde(t.path(), &["sessions", "prune", "--keep", "0", "--yes"]);
    assert_eq!(
        stdout(&sonde(t.path(), &["sessions", "-f", "csv"]))
            .lines()
            .count(),
        2
    );
}
