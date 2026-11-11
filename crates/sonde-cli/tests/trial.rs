//! `sonde run --trial` end to end: the real binary, a small S3 in this
//! process, and small programs that retry or do not. Each program is this
//! test binary run again (`the_program`, told what to be by `SONDE_E2E`), so
//! nothing outside the workspace is needed.

#[path = "../../proxy/tests/common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use common::MiniS3;
use s3raw::{Body, ClientConfig, Credentials, RetryPolicy, S3Client, S3Request, StaticProvider};

/// The program under trial. What it does is `SONDE_E2E`'s; without it this
/// is a test that passes.
#[test]
fn the_program() {
    let Ok(what) = std::env::var("SONDE_E2E") else {
        return;
    };
    let code = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(program(&what));
    std::process::exit(code);
}

fn put(key: &str, body: &str) -> S3Request {
    S3Request::new("PutObject", http::Method::PUT)
        .bucket("data")
        .key(key)
        .body(Body::from(body.to_string()))
}

fn get(key: &str) -> S3Request {
    S3Request::new("GetObject", http::Method::GET)
        .bucket("data")
        .key(key)
}

fn batch(keys: &[&str]) -> S3Request {
    let objects: String = keys
        .iter()
        .map(|k| format!("<Object><Key>{k}</Key></Object>"))
        .collect();
    S3Request::new("DeleteObjects", http::Method::POST)
        .bucket("data")
        .query("delete", "")
        .body(Body::from(format!("<Delete>{objects}</Delete>")))
}

async fn program(what: &str) -> i32 {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let mut cfg = ClientConfig::new("app", &env("AWS_ENDPOINT_URL"), &env("AWS_REGION")).unwrap();
    let retries = !what.starts_with("never");
    cfg.retry = if retries {
        RetryPolicy {
            max_attempts: 4,
            base_delay_ms: 20,
            max_delay_ms: 200,
        }
    } else {
        RetryPolicy::none()
    };
    let app = S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(
            env("AWS_ACCESS_KEY_ID"),
            env("AWS_SECRET_ACCESS_KEY"),
            None,
        )))),
        reqlog::RequestLog::default(),
    )
    .unwrap();
    // a program that checks what it is told, and one that does not
    let ok = |r: Result<s3raw::S3Response, s3raw::S3Error>| r.is_ok_and(|r| r.status < 300);
    match what {
        // writes, reads and writes: every request retried
        "retrying" => {
            let all = ok(app.send_raw(put("a", "1")).await)
                && ok(app.send_raw(get("a")).await)
                && ok(app.send_raw(put("b", "2")).await);
            if all { 0 } else { 1 }
        }
        // writes its state, polls, writes its state again: never a retry,
        // never a look at an answer
        "never-twice" => {
            let _ = app.send_raw(put("state", "started")).await;
            let _ = app.send_raw(get("job")).await;
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let _ = app.send_raw(put("state", "done")).await;
            0
        }
        // two batches of deletes
        "batches" | "never-batches" => {
            let first = ok(app.send_raw(batch(&["a", "b"])).await);
            let second = ok(app.send_raw(batch(&["c", "d"])).await);
            if retries && !(first && second) { 1 } else { 0 }
        }
        // an upload in parts, every answer checked
        "multipart" => {
            let created = app
                .send_raw(
                    S3Request::new("CreateMultipartUpload", http::Method::POST)
                        .bucket("data")
                        .key("big")
                        .query("uploads", ""),
                )
                .await;
            let Ok(created) = created else { return 1 };
            let text = created.text();
            let Some(id) = text
                .split("<UploadId>")
                .nth(1)
                .and_then(|t| t.split("</UploadId>").next())
            else {
                return 1;
            };
            let part = app
                .send_raw(
                    S3Request::new("UploadPart", http::Method::PUT)
                        .bucket("data")
                        .key("big")
                        .query("partNumber", "1")
                        .query("uploadId", id)
                        .body(Body::from("part one")),
                )
                .await;
            let Some(etag) = part.ok().and_then(|r| r.header("etag").map(str::to_string)) else {
                return 1;
            };
            let done = app
                .send_raw(
                    S3Request::new("CompleteMultipartUpload", http::Method::POST)
                        .bucket("data")
                        .key("big")
                        .query("uploadId", id)
                        .body(Body::from(format!(
                            "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{etag}</ETag></Part></CompleteMultipartUpload>"
                        ))),
                )
                .await;
            if ok(done) { 0 } else { 1 }
        }
        // writes the key it is given
        "key" => {
            if ok(app.send_raw(put(&env("E2E_KEY"), "x")).await) {
                0
            } else {
                1
            }
        }
        // writes what it reads on its input
        "stdin" => {
            use std::io::Read;
            let mut said = String::new();
            std::io::stdin().read_to_string(&mut said).unwrap();
            if said.is_empty() {
                return 9;
            }
            if ok(app.send_raw(put("said", &said)).await) {
                0
            } else {
                1
            }
        }
        // one write, then fails
        "fails" => {
            let _ = app.send_raw(put("a", "1")).await;
            3
        }
        // a trial's run that does not end
        "hangs-under-trial" => {
            let _ = app.send_raw(put("a", "1")).await;
            if env("SONDE_TRIAL") != "clean" {
                tokio::time::sleep(std::time::Duration::from_secs(120)).await;
            }
            0
        }
        other => panic!("no program {other}"),
    }
}

/// One place for a test: its store, sonde's directories, a folder to run in.
struct Bench {
    store: MiniS3,
    dir: tempfile::TempDir,
}

struct Out {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Bench {
    async fn new() -> Bench {
        let store = MiniS3::start(true).await;
        store.state.lock().buckets.entry("data".into()).or_default();
        Bench {
            store,
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn sonde(&self, program: Option<&str>) -> tokio::process::Command {
        let mut c = tokio::process::Command::new(env!("CARGO_BIN_EXE_sonde"));
        c.current_dir(self.dir.path())
            .env("SONDE_CONFIG_DIR", self.path("config"))
            .env("SONDE_DATA_DIR", self.path("data"))
            .env("AWS_EC2_METADATA_DISABLED", "true")
            .env("AWS_ACCESS_KEY_ID", "k")
            .env("AWS_SECRET_ACCESS_KEY", "s")
            .env_remove("SONDE_E2E")
            .args(["--url", &self.store.url, "--path-style"])
            .stdin(Stdio::null());
        if let Some(p) = program {
            c.env("SONDE_E2E", p);
        }
        c
    }

    /// `sonde <args>`, the program (if any) after `--`.
    async fn run(&self, program: Option<&str>, args: &[&str]) -> Out {
        let mut c = self.sonde(program);
        c.args(args);
        if program.is_some() {
            c.arg("--").args(the_program_command());
        }
        out(c.output().await.unwrap())
    }
}

fn out(o: std::process::Output) -> Out {
    Out {
        code: o.status.code(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// This binary, run as the program.
fn the_program_command() -> Vec<String> {
    vec![
        std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        "the_program".into(),
        "--exact".into(),
        "--nocapture".into(),
    ]
}

fn trials(session: &Path) -> Vec<serde_json::Value> {
    let file = session.with_file_name(format!(
        "{}.trials.json",
        session.file_stem().unwrap().to_string_lossy()
    ));
    std::fs::read(&file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn trial<'a>(all: &'a [serde_json::Value], scenario: &str) -> &'a serde_json::Value {
    all.iter()
        .find(|t| t["scenario"] == scenario)
        .unwrap_or_else(|| panic!("no trial under {scenario} in {all:#?}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_program_that_retries_survives_every_scenario() {
    let b = Bench::new().await;
    let session = b.path("run.sonde");
    let o = b
        .run(
            Some("retrying"),
            &["run", "--trial", "-o", session.to_str().unwrap()],
        )
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    assert!(
        o.stderr.contains("The program did not break."),
        "{}",
        o.stderr
    );
    let all = trials(&session);
    assert_eq!(all.len(), 3, "slowdown, error, lost-answer: {all:#?}");
    for t in &all {
        assert_eq!(t["standing"], "survived", "{t:#}");
        assert_eq!(t["faults"], t["retried"], "{t:#}");
        // each verdict names a session that is there, by a name and not a path
        let name = t["session"].as_str().unwrap();
        assert!(!name.contains('/'), "{name}");
        let file = b.path("trials").join(name);
        assert!(file.is_file(), "{}", file.display());
        assert!(
            o.stderr.contains(&format!("sonde show {}", file.display())),
            "the hint names the file: {}",
            o.stderr
        );
    }
    assert_eq!(trial(&all, "slowdown")["faults"], 3);
    // two writes whose answer was lost and which were made again
    assert_eq!(trial(&all, "lost-answer")["wroteTwice"], 2);
    assert!(trial(&all, "slowdown")["wallMs"].as_f64().unwrap() > 0.0);
    // a trial's session says first that its faults were sonde's
    let shown = b
        .run(
            None,
            &[
                "show",
                b.path("trials/run.slowdown.sonde").to_str().unwrap(),
                "--summary",
                "-f",
                "json",
            ],
        )
        .await;
    let doc: serde_json::Value = serde_json::from_str(&shown.stdout).expect(&shown.stdout);
    assert_eq!(doc["observations"][0]["phrases"][0]["key"], "obs.injected");
    assert!(
        !shown.stdout.contains("obs.throttled"),
        "the store asked for nothing: {}",
        shown.stdout
    );
    // and the summary alone is the summary: no list of requests
    assert!(doc.get("records").is_none() && doc.get("episodes").is_none());
    assert!(doc["waiting"]["p95Ms"].is_number() && doc["waiting"]["peakInFlight"].is_number());
    assert!(doc["waiting"]["byKind"].is_array() && doc["cost"]["amount"].is_number());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_program_that_never_retries_and_writes_a_key_twice_did_not_survive() {
    let b = Bench::new().await;
    let session = b.path("run.sonde");
    let o = b
        .run(
            Some("never-twice"),
            &[
                "run",
                "--fault",
                "slowdown",
                "-o",
                session.to_str().unwrap(),
            ],
        )
        .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let all = trials(&session);
    let t = trial(&all, "slowdown");
    assert_eq!(t["standing"], "broke", "{t:#}");
    // its own second write of the key is not the retry of the first
    // (two requests met the fault: the first write and the poll)
    assert_eq!(
        (&t["faults"], &t["retried"]),
        (&2.into(), &0.into()),
        "{t:#}"
    );
    assert_eq!(
        (&t["droppedWrites"], &t["droppedReads"]),
        (&1.into(), &1.into())
    );
    assert!(t.get("backoffMs").is_none(), "its own sleep is no backoff");
    assert!(!o.stderr.contains("retried, after"), "{}", o.stderr);
    assert!(
        o.stderr.contains("The program broke under 1 of 1."),
        "{}",
        o.stderr
    );

    // `check` fails the run on its trial, with no budget given
    let name = session.to_str().unwrap();
    let c = b.run(None, &["check", name]).await;
    assert_eq!(c.code, Some(1), "{}{}", c.stdout, c.stderr);
    assert!(
        c.stdout.contains("FAIL  breaks under slowdown"),
        "{}",
        c.stdout
    );
    assert!(
        c.stdout.trim_end().ends_with("broke under a trial"),
        "{}",
        c.stdout
    );
    assert!(!c.stdout.contains("over budget"), "{}", c.stdout);
    let j = b.run(None, &["check", name, "-f", "json"]).await;
    assert_eq!(j.code, Some(1));
    let doc: serde_json::Value = serde_json::from_str(&j.stdout).expect(&j.stdout);
    assert_eq!(doc["passed"], false);
    assert_eq!(doc["check"]["passed"], false);
    assert_eq!(doc["trials"][0]["standing"], "broke");
    for format in ["markdown", "github"] {
        let m = b.run(None, &["check", name, "-f", format]).await;
        assert_eq!(m.code, Some(1));
        assert!(m.stdout.contains("broke under a trial"), "{}", m.stdout);
        assert!(!m.stdout.contains("over budget"), "{}", m.stdout);
    }
    // within a budget and broken is said as both, and json's passed is true to it
    let w = b
        .run(
            None,
            &["check", name, "--max-requests", "100", "-f", "json"],
        )
        .await;
    let doc: serde_json::Value = serde_json::from_str(&w.stdout).expect(&w.stdout);
    assert_eq!(doc["passed"], false);
    assert_eq!(doc["verdict"], "within budget, and broke under a trial");
    assert!(doc["check"]["verdicts"][0]["ok"].as_bool().unwrap());

    // the program fixed and recorded again to the same file, with no trial:
    // the old verdict is not this run's
    let again = b.run(Some("retrying"), &["run", "-o", name]).await;
    assert_eq!(again.code, Some(0), "{}", again.stderr);
    assert!(trials(&session).is_empty());
    assert!(!b.path("run.trials.json").exists());
    assert!(
        !b.path("trials").exists(),
        "its trial sessions went with the verdicts"
    );
    let c = b.run(None, &["check", name]).await;
    assert_eq!(c.code, Some(64), "nothing to check now: {}", c.stdout);
    let c = b.run(None, &["check", name, "--max-requests", "100"]).await;
    assert_eq!(c.code, Some(0), "{}{}", c.stdout, c.stderr);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trial_run_again_leaves_no_session_of_the_one_before() {
    let b = Bench::new().await;
    let session = b.path("run.sonde");
    let name = session.to_str().unwrap();
    let first = b
        .run(Some("retrying"), &["run", "--trial", "-o", name])
        .await;
    assert_eq!(first.code, Some(0), "{}", first.stderr);
    let files = |dir: PathBuf| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(
        files(b.path("trials")),
        [
            "run.error.sonde",
            "run.lost-answer.sonde",
            "run.slowdown.sonde"
        ]
    );
    // again, under one scenario: only that one's session, no -2, no orphan
    let second = b
        .run(Some("retrying"), &["run", "--fault", "error", "-o", name])
        .await;
    assert_eq!(second.code, Some(0), "{}", second.stderr);
    assert_eq!(files(b.path("trials")), ["run.error.sonde"]);
    assert_eq!(trials(&session).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn two_batches_of_deletes_are_each_failed_and_each_judged() {
    let b = Bench::new().await;
    // a program that retries: both batches reach the store in the trial too
    let session = b.path("retries.sonde");
    let o = b
        .run(
            Some("batches"),
            &[
                "run",
                "--fault",
                "slowdown",
                "-o",
                session.to_str().unwrap(),
            ],
        )
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let all = trials(&session);
    let t = trial(&all, "slowdown");
    assert_eq!(
        (&t["faults"], &t["retried"]),
        (&2.into(), &2.into()),
        "{t:#}"
    );
    assert_eq!(t["standing"], "survived");
    let sent = b.store.sent("POST /data");
    // clean run and trial: each batch twice at the store
    for key in ["<Key>a</Key>", "<Key>c</Key>"] {
        assert_eq!(
            sent.iter().filter(|(_, body)| body.contains(key)).count(),
            2,
            "{sent:?}"
        );
    }
    // one that does not: neither batch is the other's retry, and the
    // objects of both were never deleted
    b.store.state.lock().log.clear();
    let session = b.path("never.sonde");
    let o = b
        .run(
            Some("never-batches"),
            &[
                "run",
                "--fault",
                "slowdown",
                "-o",
                session.to_str().unwrap(),
            ],
        )
        .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let all = trials(&session);
    let t = trial(&all, "slowdown");
    assert_eq!(
        (&t["faults"], &t["retried"]),
        (&2.into(), &0.into()),
        "{t:#}"
    );
    assert_eq!(t["droppedWrites"], 2);
    assert_eq!(t["standing"], "broke");
    assert_eq!(
        b.store.sent("POST /data").len(),
        2,
        "the clean run's two: the trial's never reached the store"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_completion_whose_answer_was_lost_says_why_the_program_broke() {
    let b = Bench::new().await;
    let session = b.path("run.sonde");
    let o = b
        .run(
            Some("multipart"),
            &[
                "run",
                "--fault",
                "lost-answer",
                "-o",
                session.to_str().unwrap(),
            ],
        )
        .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let all = trials(&session);
    let t = trial(&all, "lost-answer");
    assert_eq!(t["standing"], "broke", "{t:#}");
    assert_eq!(t["leftUploads"], 0, "the store completed it: {t:#}");
    let reasons: Vec<&str> = t["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r.as_str())
        .collect();
    assert!(
        reasons.contains(
            &"the retried CompleteMultipartUpload was answered 404 NoSuchUpload: the upload had completed"
        ),
        "{reasons:?}"
    );
    // the part was written twice; the completion was not
    assert_eq!(t["wroteTwice"], 1, "{t:#}");
    assert!(
        o.stderr.contains("the upload had completed"),
        "{}",
        o.stderr
    );
    // and the object is whole in the store
    assert!(b.store.keys("data").contains(&"big".to_string()));
    // the trial's session does not call the upload aborted or unfinished
    let shown = b
        .run(
            None,
            &[
                "show",
                b.path("trials/run.lost-answer.sonde").to_str().unwrap(),
            ],
        )
        .await;
    assert!(
        !shown.stdout.contains("(aborted)") && !shown.stdout.contains("(not completed)"),
        "{}",
        shown.stdout
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_clean_run_that_fails_is_not_tried_and_a_scenario_that_meets_nothing_is_untested() {
    let b = Bench::new().await;
    let session = b.path("fails.sonde");
    let o = b
        .run(
            Some("fails"),
            &["run", "--trial", "-o", session.to_str().unwrap()],
        )
        .await;
    assert_eq!(o.code, Some(3), "{}", o.stderr);
    assert!(
        o.stderr.contains("No trial: the clean run exited 3"),
        "{}",
        o.stderr
    );
    assert!(trials(&session).is_empty() && !b.path("trials").exists());

    let session = b.path("run.sonde");
    let o = b
        .run(
            Some("retrying"),
            &[
                "run",
                "--fault",
                "fail-complete",
                "-o",
                session.to_str().unwrap(),
            ],
        )
        .await;
    assert_eq!(o.code, Some(0), "untested is not broken: {}", o.stderr);
    assert!(o.stderr.contains("untested"), "{}", o.stderr);
    assert!(o.stderr.contains("Nothing was tried"), "{}", o.stderr);
    assert!(!o.stderr.contains("did not break"), "{}", o.stderr);
    let c = b.run(None, &["check", session.to_str().unwrap()]).await;
    assert_eq!(c.code, Some(0), "{}", c.stdout);
    assert!(c.stdout.contains("untested: fail-complete"), "{}", c.stdout);
    assert!(
        c.stdout.trim_end().ends_with("nothing was tried"),
        "{}",
        c.stdout
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_place_of_a_run_is_named_only_when_a_trial_runs() {
    let b = Bench::new().await;
    let env = "env";
    let run = |args: &'static [&'static str]| {
        let mut c = b.sonde(Some("key"));
        c.args(args)
            .args(["--", env, "E2E_KEY=k-{trial}"])
            .args(the_program_command());
        async move { out(c.output().await.unwrap()) }
    };
    // no trial: the command is run as it was written
    let o = run(&["run", "-o", "plain.sonde"]).await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    assert_eq!(b.store.keys("data"), ["k-{trial}"]);
    assert!(!o.stderr.contains("warning"), "{}", o.stderr);
    // a trial: each run its own place, and no warning that they share one
    let o = run(&["run", "--fault", "slowdown", "-o", "tried.sonde"]).await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    assert_eq!(b.store.keys("data"), ["k-clean", "k-slowdown", "k-{trial}"]);
    assert!(!o.stderr.contains("warning"), "{}", o.stderr);
    // a command that names no place is warned of once
    let o = b
        .run(
            Some("retrying"),
            &["run", "--fault", "slowdown", "-o", "w.sonde"],
        )
        .await;
    assert_eq!(
        o.stderr
            .matches("warning: the command has no {trial}")
            .count(),
        1,
        "{}",
        o.stderr
    );
    let help = b.run(None, &["run", "--help"]).await;
    assert!(
        help.stdout.contains("{trial}") && help.stdout.contains("SONDE_TRIAL"),
        "{}",
        help.stdout
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_program_fed_by_a_pipe_reads_the_same_in_every_run() {
    use tokio::io::AsyncWriteExt;
    let b = Bench::new().await;
    let mut c = b.sonde(Some("stdin"));
    c.args([
        "run",
        "--fault",
        "slowdown",
        "--fault",
        "error",
        "-o",
        "run.sonde",
        "--",
    ])
    .args(the_program_command())
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"what it was told").await.unwrap();
    drop(stdin);
    let o = out(child.wait_with_output().await.unwrap());
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let all = trials(&b.path("run.sonde"));
    assert_eq!(all.len(), 2);
    for t in &all {
        assert_eq!(t["exit"], 0, "it read its input again: {t:#}");
        assert_eq!(t["standing"], "survived", "{t:#}");
    }
    assert_eq!(
        b.store.state.lock().buckets["data"]["said"].0,
        b"what it was told"
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_interrupt_ends_the_trial_and_writes_no_verdict() {
    for (signal, code) in [("INT", 130), ("TERM", 143)] {
        let b = Bench::new().await;
        let mut c = b.sonde(Some("hangs-under-trial"));
        c.args(["run", "--trial", "-o", "run.sonde", "--"])
            .args(the_program_command())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = c.spawn().unwrap();
        let pid = child.id().unwrap();
        // the first scenario's run has begun once its session is there
        let trial_session = b.path("trials/run.slowdown.sonde");
        for _ in 0..600 {
            if trial_session.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(trial_session.exists(), "the trial never began");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let sent = std::process::Command::new("kill")
            .args([&format!("-{signal}"), &pid.to_string()])
            .status()
            .unwrap();
        assert!(sent.success());
        let o = tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
            .await
            .expect("sonde ended the trial")
            .unwrap();
        let o = out(o);
        assert_eq!(o.code, Some(code), "{signal}: {}", o.stderr);
        assert!(
            o.stderr.contains("Interrupted under slowdown"),
            "{}",
            o.stderr
        );
        // one scenario was begun, none after it; nothing of it is kept
        assert!(!o.stderr.contains("  broke"), "{}", o.stderr);
        assert!(!b.path("run.trials.json").exists(), "{signal}");
        assert!(!trial_session.exists(), "the half-written session is gone");
        assert!(!b.path("trials").exists());
        // and `check` has no broken trial to fail the run on
        let c = b
            .run(None, &["check", "run.sonde", "--max-requests", "10"])
            .await;
        assert_eq!(c.code, Some(0), "{}{}", c.stdout, c.stderr);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sonde_toml_that_says_what_sonde_does_not_know_is_an_error() {
    let b = Bench::new().await;
    let cases = [
        (
            "[[scenarios]]\nname = \"x\"\nfault = \"500\"\n",
            "[[scenario]]",
        ),
        (
            "[[scenario]]\nname = \"x\"\nfault = \"500\"\non = [\"PutObjekt\"]\n",
            "PutObjekt",
        ),
        (
            "[[scenario]]\nname = \"slowdown\"\nfault = \"500\"\n",
            "one of sonde's own",
        ),
        ("[chek]\nmax-requests = 3\n", "chek"),
        (
            "[check]\nmax-waiting = \"soon\"\nmax-requests = 3\n",
            "soon",
        ),
    ];
    for (toml, says) in cases {
        std::fs::write(b.path("sonde.toml"), toml).unwrap();
        // before anything runs
        let o = b
            .run(Some("retrying"), &["run", "--trial", "-o", "run.sonde"])
            .await;
        let code = if says == "soon" {
            // (a budget is `check`'s to read)
            assert_eq!(o.code, Some(0), "{}", o.stderr);
            b.run(None, &["check", "run.sonde"]).await
        } else {
            assert!(!b.path("run.sonde").exists() || says == "soon", "{toml}");
            o
        };
        assert_eq!(code.code, Some(64), "{toml}: {}", code.stderr);
        assert!(code.stderr.contains(says), "{toml}: {}", code.stderr);
        assert!(code.stderr.contains("sonde.toml"), "{}", code.stderr);
        let _ = std::fs::remove_file(b.path("run.sonde"));
    }
    // a pattern that matches an operation is taken
    std::fs::write(
        b.path("sonde.toml"),
        "[[scenario]]\nname = \"flaky-puts\"\nfault = \"503 SlowDown\"\non = [\"Put*\"]\n",
    )
    .unwrap();
    let o = b
        .run(Some("retrying"), &["run", "--trial", "-o", "run.sonde"])
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    assert!(o.stderr.contains("survived  flaky-puts"), "{}", o.stderr);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_named_program_keeps_its_baseline_when_its_arguments_change() {
    let b = Bench::new().await;
    std::fs::write(b.path("sonde.toml"), "[program]\nname = \"nightly\"\n").unwrap();
    let run = |key: &'static str| {
        let mut c = b.sonde(Some("key"));
        c.args(["run", "--", "env", key])
            .args(the_program_command());
        async move { out(c.output().await.unwrap()) }
    };
    assert_eq!(run("E2E_KEY=first").await.code, Some(0));
    let set = b.run(None, &["check", "--set-baseline"]).await;
    assert_eq!(set.code, Some(0), "{}", set.stderr);
    assert!(
        set.stderr.contains("baseline for `nightly`"),
        "{}",
        set.stderr
    );
    // another argument: the same program, held to that baseline
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(run("E2E_KEY=second").await.code, Some(0));
    let c = b.run(None, &["check", "--baseline", "auto"]).await;
    assert_eq!(c.code, Some(0), "{}{}", c.stdout, c.stderr);
    assert!(
        c.stdout.contains("program nightly (its name)"),
        "{}",
        c.stdout
    );
    assert!(!c.stderr.contains("the baseline ran"), "{}", c.stderr);
    let list = b.run(None, &["sessions", "-f", "json"]).await;
    let rows: serde_json::Value = serde_json::from_str(&list.stdout).unwrap();
    assert_eq!(rows[0]["program"], "nightly");
    assert_eq!(rows[0]["requests"], 1);
    // --name wins over the file, and a run without either is its command
    let mut c = b.sonde(Some("key"));
    c.args([
        "run",
        "--name",
        "other",
        "-o",
        "o.sonde",
        "--",
        "env",
        "E2E_KEY=third",
    ])
    .args(the_program_command());
    assert_eq!(out(c.output().await.unwrap()).code, Some(0));
    let c = b
        .run(None, &["check", "o.sonde", "--max-requests", "5"])
        .await;
    assert!(
        c.stdout.contains("program other (its name)"),
        "{}",
        c.stdout
    );
}
