//! The boto3 fixes sonde prints, run for real: a small program with the
//! pattern under `sonde run --demo`, then the same program with sonde's fix
//! pasted in as `show -f json` gives it, run again: the finding gone, the
//! fault survived. Needs python3 with boto3: skipped without, unless
//! SONDE_BOTO3 is set (CI's record job, which installs it).

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const PRELUDE: &str =
    "import boto3, os, sys, threading, time\ns3 = boto3.client(\"s3\")\nb = \"demo\"\n";

fn boto3() -> bool {
    let ok = Command::new("python3")
        .args(["-c", "import boto3"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        assert!(
            std::env::var_os("SONDE_BOTO3").is_none(),
            "no python3 with boto3"
        );
        eprintln!("skipped: no python3 with boto3 here");
    }
    ok
}

fn sonde(dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sonde"));
    c.current_dir(dir)
        .env("SONDE_CONFIG_DIR", dir.join("config"))
        .env("SONDE_DATA_DIR", dir.join("data"));
    for k in ["AWS_PROFILE", "AWS_ENDPOINT_URL", "AWS_ENDPOINT_URL_S3"] {
        c.env_remove(k);
    }
    c
}

/// `program` (`name`.py) under `sonde run --demo` with `args`; its `show -f
/// json`.
fn run(dir: &Path, name: &str, program: &str, args: &[&str]) -> Value {
    std::fs::write(dir.join(format!("{name}.py")), program).unwrap();
    // a file a run: the first run's, then its fix's
    let file = (1..)
        .map(|n| format!("{name}.{n}.sonde"))
        .find(|f| !dir.join(f).exists())
        .unwrap();
    let o = sonde(dir)
        .args(["run", "--demo", "--name", name, "-o", &file])
        .args(args)
        .args(["--", "python3", &format!("{name}.py")])
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&o.stderr);
    let shown = sonde(dir)
        .args(["show", &file, "-f", "json"])
        .output()
        .unwrap();
    serde_json::from_slice(&shown.stdout).unwrap_or_else(|e| panic!("{e}: {said}"))
}

/// The fix the run's diagnosis of `kind` prints for boto3.
fn fix(j: &Value, kind: &str) -> String {
    j["diagnoses"]
        .as_array()
        .and_then(|ds| ds.iter().find(|d| d["kind"] == kind))
        .and_then(|d| d["fix"]["code"].as_str())
        .unwrap_or_else(|| panic!("no {kind} with a fix: {j}"))
        .to_string()
}

fn kinds(j: &Value) -> Vec<String> {
    j["diagnoses"]
        .as_array()
        .map(|ds| ds.iter().map(|d| d["kind"].to_string()).collect())
        .unwrap_or_default()
}

fn standing(j: &Value) -> String {
    j["trials"][0]["standing"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

/// A body cut short raises as boto3 reads it: the fix reads the rest.
#[test]
fn a_body_cut_short_is_read_to_its_end_by_the_fix() {
    if !boto3() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let put = "k = \"fx/big\"\ndata = os.urandom(256 * 1024)\ns3.put_object(Bucket=b, Key=k, Body=data)\n";
    let bad = format!(
        "{PRELUDE}{put}body = s3.get_object(Bucket=b, Key=k)[\"Body\"].read()\nassert body == data\n"
    );
    let j = run(t.path(), "cut", &bad, &["--fault", "cut-body"]);
    assert_eq!(standing(&j), "broke", "{j}");
    let fixed = format!("{PRELUDE}{put}{}\nassert body == data\n", fix(&j, "trial"));
    let j = run(t.path(), "cut", &fixed, &["--fault", "cut-body"]);
    assert_eq!(standing(&j), "survived", "{fixed}\n{j}");
}

/// A first byte 10 s late: the fix's timeout cuts it short and sends it
/// again, well before the stall ends.
#[test]
fn a_stalled_first_byte_is_sent_again_by_the_fixs_timeout() {
    if !boto3() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let rest = "s3.put_object(Bucket=b, Key=\"k\", Body=b\"x\" * 1000)\nassert len(s3.get_object(Bucket=b, Key=\"k\")[\"Body\"].read()) == 1000\n";
    let j = run(
        t.path(),
        "stall",
        &format!("{PRELUDE}{rest}"),
        &["--fault", "slow-first-byte"],
    );
    assert_eq!(standing(&j), "degraded", "{j}");
    let fixed = format!("{PRELUDE}{}\n{rest}", fix(&j, "trial"));
    let j = run(t.path(), "stall", &fixed, &["--fault", "slow-first-byte"]);
    assert_eq!(standing(&j), "survived", "{fixed}\n{j}");
    let wall = j["trials"][0]["wallMs"].as_f64().unwrap();
    assert!(wall < 8000.0, "the stall was waited out: {wall} ms");
}

/// A program that never sends a request again: the fix turns boto3's
/// retries on, and a 503 SlowDown is survived.
#[test]
fn retries_turned_on_as_the_fix_says_survive_a_slowdown() {
    if !boto3() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let never = "import boto3\nfrom botocore.config import Config\ns3 = boto3.client(\"s3\", config=Config(retries={\"total_max_attempts\": 1}))\nb = \"demo\"\n";
    let rest = "s3.put_object(Bucket=b, Key=\"k\", Body=b\"x\")\n";
    let j = run(
        t.path(),
        "retry",
        &format!("{never}{rest}"),
        &["--fault", "slowdown"],
    );
    assert_eq!(standing(&j), "broke", "{j}");
    let fixed = format!("import boto3\n{}\nb = \"demo\"\n{rest}", fix(&j, "trial"));
    let j = run(t.path(), "retry", &fixed, &["--fault", "slowdown"]);
    assert_eq!(standing(&j), "survived", "{fixed}\n{j}");
}

/// Keys deleted one by one: the fix deletes them a thousand a request, and
/// a key the store did not delete (in `Errors`, not raised) is not passed
/// over in silence.
#[test]
fn deletes_in_batches_as_the_fix_says_and_a_key_not_deleted_raises() {
    if !boto3() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let put = "keys = [f\"tmp/{i}\" for i in range(40)]\nfor k in keys:\n    s3.put_object(Bucket=b, Key=k, Body=b\"x\")\n";
    let bad = format!("{PRELUDE}{put}for k in keys:\n    s3.delete_object(Bucket=b, Key=k)\n");
    let j = run(t.path(), "deletes", &bad, &[]);
    let code = fix(&j, "obs.deletes");
    let fixed = format!(
        "{PRELUDE}{put}{code}\nassert \"Contents\" not in s3.list_objects_v2(Bucket=b, Prefix=\"tmp/\")\n"
    );
    let j = run(t.path(), "deletes", &fixed, &[]);
    assert!(
        !kinds(&j).contains(&"\"obs.deletes\"".into()),
        "{fixed}\n{j}"
    );
    // the store's answer names a key it did not delete
    let indented: String = code.lines().map(|l| format!("    {l}\n")).collect();
    let stubbed = format!(
        "import boto3, sys\nfrom botocore.stub import Stubber\n\
         s3 = boto3.client(\"s3\", region_name=\"us-east-1\", aws_access_key_id=\"x\", aws_secret_access_key=\"x\")\n\
         stub = Stubber(s3)\n\
         stub.add_response(\"delete_objects\", {{\"Errors\": [{{\"Key\": \"a\", \"Code\": \"AccessDenied\", \"Message\": \"no\"}}]}})\n\
         stub.activate()\nb, keys = \"demo\", [\"a\", \"b\"]\ntry:\n{indented}except Exception as e:\n    print(\"raised\", e)\nelse:\n    sys.exit(\"a key not deleted was passed over\")\n"
    );
    let o = Command::new("python3")
        .args(["-c", &stubbed])
        .output()
        .unwrap();
    assert!(
        o.status.success() && String::from_utf8_lossy(&o.stdout).contains("raised"),
        "{stubbed}\n{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// A key polled every fifth of a second until its writer writes it: the fix
/// backs off, asks fewer times, and sees the key soon after it is there.
#[test]
fn a_poll_backs_off_as_the_fix_says_and_is_not_much_later() {
    if !boto3() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let writer = "k = \"jobs/run-1/_SUCCESS\"\nthreading.Timer(1.5, lambda: s3.put_object(Bucket=b, Key=k, Body=b\"\")).start()\n";
    let bad = format!(
        "{PRELUDE}{writer}while True:\n    try:\n        s3.head_object(Bucket=b, Key=k)\n        break\n    except s3.exceptions.ClientError:\n        time.sleep(0.2)\n"
    );
    let j = run(t.path(), "poll", &bad, &[]);
    let before = j["time"]["wallMs"].as_f64().unwrap();
    let fixed = format!("{PRELUDE}{writer}{}\n", fix(&j, "obs.polled"));
    let j = run(t.path(), "poll", &fixed, &[]);
    assert!(
        !kinds(&j).contains(&"\"obs.polled\"".into()),
        "{fixed}\n{j}"
    );
    let after = j["time"]["wallMs"].as_f64().unwrap();
    assert!(
        after < before + 2500.0,
        "{after} ms, {before} ms before: the fix waits too long"
    );
}
