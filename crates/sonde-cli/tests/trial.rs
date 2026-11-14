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
    // the key it is given: an argument `E2E_KEY=<key>` after its own (which
    // the test harness takes for a filter that matches nothing), so that
    // `{trial}` in it is the command's, on every system
    let given = || {
        std::env::args()
            .find_map(|a| a.strip_prefix("E2E_KEY=").map(str::to_string))
            .unwrap_or_default()
    };
    let mut cfg = ClientConfig::new("app", &env("AWS_ENDPOINT_URL"), &env("AWS_REGION")).unwrap();
    // a request held past two seconds is given up on and sent again; a
    // patient program waits as an SDK's minute does
    if !what.starts_with("patient") {
        cfg.timeout = std::time::Duration::from_secs(2);
    }
    // it says it is an SDK that signs a skewed request again (as the AWS
    // SDK for Java does), so that `skew` is offered to it
    if what == "retrying" {
        cfg.user_agent = "aws-sdk-java/2.25.1 sonde-e2e".into();
    }
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
        // an upload in parts, every answer checked; `multipart-tolerant`
        // takes a completion's NoSuchUpload as a question, and looks
        "multipart" | "multipart-tolerant" => {
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
            match done {
                Ok(r) if r.status < 300 => 0,
                // its retry found the upload gone: completed, or never was
                Ok(r) if what == "multipart-tolerant" && r.text().contains("NoSuchUpload") => {
                    let head = S3Request::new("HeadObject", http::Method::HEAD)
                        .bucket("data")
                        .key("big");
                    if ok(app.send_raw(head).await) { 0 } else { 1 }
                }
                _ => 1,
            }
        }
        // writes the key it is given; `never-key` says all is well whatever
        "key" | "never-key" => {
            if ok(app.send_raw(put(&given(), "x")).await) || !retries {
                0
            } else {
                1
            }
        }
        // marks its output done, if no one has: a 412 is "it is there"
        "marker" => {
            let marker = put(&format!("{}/_SUCCESS", given()), "").header("if-none-match", "*");
            match app.send_raw(marker).await.map(|r| r.status) {
                Ok(200 | 412) => 0,
                _ => 1,
            }
        }
        // writes two parts, lists them and reads each it is shown;
        // `lists-again` lists until it is shown what it wrote
        "lists" | "lists-again" => {
            for k in ["out/1", "out/2"] {
                if !ok(app.send_raw(put(k, "rows")).await) {
                    return 1;
                }
            }
            let mut listed: Vec<String> = Vec::new();
            for _ in 0..3 {
                let list = S3Request::new("ListObjectsV2", http::Method::GET)
                    .bucket("data")
                    .query("list-type", "2")
                    .query("prefix", "out/");
                let Ok(r) = app.send_raw(list).await else {
                    return 1;
                };
                listed = r
                    .text()
                    .split("<Key>")
                    .skip(1)
                    .filter_map(|k| k.split("</Key>").next())
                    .map(str::to_string)
                    .collect();
                if what == "lists" || listed.len() >= 2 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            for k in &listed {
                if !ok(app.send_raw(get(k)).await) {
                    return 1;
                }
            }
            0
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
        // writes how much it read on its input
        "stdin-size" => {
            use std::io::Read;
            let mut said = Vec::new();
            std::io::stdin().read_to_end(&mut said).unwrap();
            if ok(app.send_raw(put("size", &said.len().to_string())).await) {
                0
            } else {
                1
            }
        }
        // reads the demo store's README, asking first whether it is there
        "demo" => {
            let on = |op: &str, method: http::Method| {
                S3Request::new(op, method).bucket("demo").key("README.md")
            };
            let head = ok(app.send_raw(on("HeadObject", http::Method::HEAD)).await);
            if head && ok(app.send_raw(on("GetObject", http::Method::GET)).await) {
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
        // writes its state twice, a moment apart, under the place it is
        // given (E2E_KEY) or the one its environment names (SONDE_TRIAL);
        // `never-placed` never retries and never looks at an answer
        "placed" | "never-placed" => {
            let place = match given() {
                k if k.is_empty() => format!("etl-{}", env("SONDE_TRIAL")),
                k => k,
            };
            let key = format!("{place}/state");
            let first = ok(app.send_raw(put(&key, "started")).await);
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let second = ok(app.send_raw(put(&key, "done")).await);
            if retries && !(first && second) { 1 } else { 0 }
        }
        // a write, and a batch of deletes it does not hold the store to
        "tolerant-batches" => {
            if !ok(app.send_raw(put("a", "1")).await) {
                return 1;
            }
            let _ = app.send_raw(batch(&["a"])).await;
            0
        }
        // polls a key for half a second, every 100 ms, the SDK retrying
        "polls-for" => {
            if !ok(app.send_raw(put("job", "queued")).await) {
                return 1;
            }
            let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
            while std::time::Instant::now() < until {
                if !ok(app.send_raw(get("job")).await) {
                    return 1;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            0
        }
        // polls a key until it has had three answers, never a retry
        "never-polls" => {
            let mut answers = 0;
            for _ in 0..20 {
                if ok(app.send_raw(get("job")).await) {
                    answers += 1;
                    if answers == 3 {
                        return 0;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            }
            1
        }
        // downloads `big` and holds it to its length; `never-downloads`
        // takes whatever came; `patient-downloads` waits as long as it takes
        "downloads" | "never-downloads" | "patient-downloads" => {
            match app.send_raw(get("big")).await {
                Ok(r) if r.status == 200 && r.body.len() == BIG => 0,
                _ if retries => 1,
                _ => 0,
            }
        }
        // a commit to a log by conditional create (E2E_KEY names the log's
        // folder). `commits` reads the log again on a 412 and writes the
        // next number; `never-commits` takes the 412 for an answer;
        // `overwrites` writes the same key without its condition; `fenced`
        // stops there, as a SlateDB writer does
        "commits" | "never-commits" | "never-overwrites" | "never-fenced" => {
            let dir = given();
            let name = |n: u32| {
                if dir.ends_with("wal") {
                    format!("{dir}/{n:020}.sst")
                } else {
                    format!("{dir}/{n:020}.json")
                }
            };
            let create = |n: u32| put(&name(n), "commit").header("if-none-match", "*");
            if !ok(app
                .send_raw(put(&format!("{dir}-data/part-1"), "rows"))
                .await)
            {
                return 1;
            }
            for n in 1..5 {
                match app.send_raw(create(n)).await.map(|r| r.status) {
                    Ok(200) => return 0,
                    Ok(412) => match what {
                        "commits" => {
                            let _ = app.send_raw(get(&name(n))).await;
                        }
                        "never-overwrites" => {
                            let _ = app.send_raw(put(&name(n), "commit")).await;
                            return 0;
                        }
                        "never-fenced" => return 3,
                        _ => return 0,
                    },
                    _ => return 1,
                }
            }
            1
        }
        // starts a process of its own and waits for it, under a trial
        "spawns-under-trial" => {
            let _ = app.send_raw(put("a", "1")).await;
            if env("SONDE_TRIAL") != "clean" {
                let mut child = std::process::Command::new("sleep")
                    .arg("300")
                    .spawn()
                    .unwrap();
                std::fs::write(env("E2E_PIDFILE"), child.id().to_string()).unwrap();
                let _ = child.wait();
            }
            0
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

/// The size of the object `big` a bench's store holds.
const BIG: usize = 1000;

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

    /// A bench whose store holds `big`, an object large enough to cut.
    async fn with_big() -> Bench {
        let b = Bench::new().await;
        b.store
            .state
            .lock()
            .buckets
            .entry("data".into())
            .or_default()
            .insert("big".into(), (vec![b'x'; BIG], "\"big\"".into()));
        b
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
    sonde_proxy::trial::load(session)
        .iter()
        .map(|t| serde_json::to_value(t).unwrap())
        .collect()
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
    assert_eq!(
        all.len(),
        5,
        "slowdown, error, skew, lost-answer, slow-first-byte: {all:#?}"
    );
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
    // a skewed clock: each request signed again and sent
    assert_eq!(trial(&all, "skew")["retried"], 3);
    // a first byte held: its timeout cut it short, and it asked again
    let late = trial(&all, "slow-first-byte");
    assert_eq!(
        (&late["held"], &late["retried"]),
        (&1.into(), &1.into()),
        "{late:#}"
    );
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
    // the trial's faults are the trial's notice, no finding (18.0)
    assert!(!shown.stdout.contains("obs.injected"), "{}", shown.stdout);
    assert!(
        !shown.stdout.contains("obs.throttled"),
        "the store asked for nothing: {}",
        shown.stdout
    );
    // and the summary alone is the summary: no list of requests
    assert!(doc.get("requests").is_none() && doc["operations"].is_number());
    assert!(doc["waiting"]["p95Ms"].is_number() && doc["waiting"]["peakInFlight"].is_number());
    assert!(doc["waiting"]["byKind"].is_array() && doc["cost"]["amount"].is_number());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scenario_pressed_harder_fails_more_attempts_in_a_row() {
    // the program makes four attempts at a request: three failures in a
    // row it gets through, four it does not
    let b = Bench::new().await;
    let session = b.path("run.sonde");
    let run = |fault: &'static str| {
        let (b, session) = (&b, session.clone());
        async move {
            b.run(
                Some("retrying"),
                &[
                    "run",
                    "--allow-writes",
                    "--fault",
                    fault,
                    "-o",
                    session.to_str().unwrap(),
                ],
            )
            .await
        }
    };
    let o = run("slowdown:3").await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    // the row says how it was asked for; the session keeps the scenario's name
    assert!(o.stderr.contains("slowdown:3"), "{}", o.stderr);
    let all = trials(&session);
    let t = trial(&all, "slowdown");
    assert_eq!(t["standing"], "survived", "{t:#}");
    assert_eq!(t["times"], 3, "{t:#}");
    assert_eq!(t["attempts"], 4, "{t:#}");
    assert_eq!(t["faults"], 9, "three requests, three attempts each: {t:#}");
    assert_eq!(t["session"], "run.slowdown-3x.sonde");
    assert_eq!(
        t["about"],
        "503 SlowDown on each request's first 3 attempts"
    );
    let o = run("slowdown:4").await;
    let all = trials(&session);
    let t = trial(&all, "slowdown");
    // its first write fails four times: three retries, then it gives up and exits
    assert_eq!(t["standing"], "broke", "{t:#}\n{}", o.stderr);
    assert_eq!(t["faults"], 4, "{t:#}");
    assert_eq!(t["retried"], 3, "{t:#}");
    assert_eq!(t["droppedWrites"], 1, "{t:#}");
    // asked for once, without a count: nothing says "times"
    let o = run("slowdown").await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    assert!(trial(&trials(&session), "slowdown").get("times").is_none());
    // a count out of range, and a scenario that takes none, are usage errors
    for (fault, says) in [
        ("slowdown:0", "a count from 1 to 5"),
        ("lost-answer:2", "only slowdown and error take a count"),
    ] {
        let o = run(fault).await;
        assert_eq!(o.code, Some(64), "{fault}: {}", o.stderr);
        assert!(o.stderr.contains(says), "{fault}: {}", o.stderr);
    }
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
    // (the inner one is of the budget's rows, and none of those failed)
    assert_eq!(doc["check"]["passed"], true);
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
        .run(
            Some("retrying"),
            &["run", "--allow-writes", "--trial", "-o", name],
        )
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
            "run.skew.sonde",
            "run.slow-first-byte.sonde",
            "run.slowdown.sonde"
        ]
    );
    // again, under one scenario: only that one's session, no -2, no orphan
    let second = b
        .run(
            Some("retrying"),
            &["run", "--allow-writes", "--fault", "error", "-o", name],
        )
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
                "--allow-writes",
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
                "--allow-writes",
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
            &[
                "run",
                "--allow-writes",
                "--trial",
                "-o",
                session.to_str().unwrap(),
            ],
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
                "--allow-writes",
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
    let run = |args: &'static [&'static str]| {
        let mut c = b.sonde(Some("key"));
        c.args(args)
            .arg("--")
            .args(the_program_command())
            .arg("E2E_KEY=k-{trial}");
        async move { out(c.output().await.unwrap()) }
    };
    // no trial: the command is run as it was written
    let o = run(&["run", "-o", "plain.sonde"]).await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    assert_eq!(b.store.keys("data"), ["k-{trial}"]);
    assert!(!o.stderr.contains("warning"), "{}", o.stderr);
    // a trial: each run its own place, and no warning that they share one
    let o = run(&[
        "run",
        "--allow-writes",
        "--fault",
        "slowdown",
        "-o",
        "tried.sonde",
    ])
    .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    assert_eq!(b.store.keys("data"), ["k-clean", "k-slowdown", "k-{trial}"]);
    assert!(!o.stderr.contains("warning"), "{}", o.stderr);
    // a command that names no place is warned of once
    let o = b
        .run(
            Some("retrying"),
            &[
                "run",
                "--allow-writes",
                "--fault",
                "slowdown",
                "-o",
                "w.sonde",
            ],
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
        (
            "[[scenario]]\nname = \"clean\"\nfault = \"500\"\n",
            "the run without faults",
        ),
        (
            "[[scenario]]\nname = \"x\"\nfault = \"503 Slow Down extra\"\n",
            "one word too many",
        ),
        ("[program]\nname = \"\"\n", "a program's name"),
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
            .run(
                Some("retrying"),
                &["run", "--allow-writes", "--trial", "-o", "run.sonde"],
            )
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
        .run(
            Some("retrying"),
            &["run", "--allow-writes", "--trial", "-o", "run.sonde"],
        )
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
        c.args(["run", "--"]).args(the_program_command()).arg(key);
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
    c.args(["run", "--name", "other", "-o", "o.sonde", "--"])
        .args(the_program_command())
        .arg("E2E_KEY=third");
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

/// `sonde run <args> -- <the program> E2E_KEY=<key>`.
async fn run_keyed(b: &Bench, program: &str, key: &str, args: &[&str]) -> Out {
    let mut c = b.sonde(Some(program));
    c.args(args)
        .arg("--")
        .args(the_program_command())
        .arg(format!("E2E_KEY={key}"));
    out(c.output().await.unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_placed_key_that_holds_the_clean_runs_name_or_the_scenarios_is_still_matched() {
    let b = Bench::new().await;
    // `etl-clean/clean/state` and `etl-slowdown/clean/state`: the second
    // `clean` is the program's own folder; `etl-error/error/state` the same
    for (fault, key) in [
        ("slowdown", "etl-{trial}/clean"),
        ("error", "etl-{trial}/error"),
    ] {
        let o = run_keyed(
            &b,
            "never-placed",
            key,
            &[
                "run",
                "--allow-writes",
                "--fault",
                fault,
                "-o",
                "never.sonde",
            ],
        )
        .await;
        assert_eq!(o.code, Some(1), "{key}: {}", o.stderr);
        let t = trial(&trials(&b.path("never.sonde")), fault).clone();
        assert_eq!(t["standing"], "broke", "{key}: {t:#}");
        assert_eq!(
            (&t["retried"], &t["droppedWrites"]),
            (&0.into(), &1.into()),
            "{key}: {t:#}"
        );
        let o = run_keyed(
            &b,
            "placed",
            key,
            &[
                "run",
                "--allow-writes",
                "--fault",
                fault,
                "-o",
                "retries.sonde",
            ],
        )
        .await;
        assert_eq!(o.code, Some(0), "{key}: {}", o.stderr);
        let t = trial(&trials(&b.path("retries.sonde")), fault).clone();
        assert_eq!(t["standing"], "survived", "{key}: {t:#}");
    }
    assert!(
        b.store
            .keys("data")
            .contains(&"etl-slowdown/clean/state".to_string()),
        "{:?}",
        b.store.keys("data")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_400_request_timeout_is_owed_a_retry_and_a_dropped_write_broke() {
    let b = Bench::new().await;
    std::fs::write(
        b.path("sonde.toml"),
        "[[scenario]]\nname = \"timeout\"\nfault = \"400 RequestTimeout\"\non = [\"PutObject\"]\n",
    )
    .unwrap();
    let o = run_keyed(
        &b,
        "never-key",
        "k-{trial}",
        &["run", "--allow-writes", "--trial", "-o", "never.sonde"],
    )
    .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let t = trial(&trials(&b.path("never.sonde")), "timeout").clone();
    assert_eq!(t["standing"], "broke", "{t:#}");
    assert!(t.get("notRetryable").is_none(), "{t:#}");
    assert_eq!(t["droppedWrites"], 1, "{t:#}");
    // sonde's own client sends it again, as an SDK does
    let o = run_keyed(
        &b,
        "key",
        "k-{trial}",
        &["run", "--allow-writes", "--trial", "-o", "retries.sonde"],
    )
    .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let t = trial(&trials(&b.path("retries.sonde")), "timeout").clone();
    assert_eq!(t["standing"], "survived", "{t:#}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_marker_created_if_absent_is_not_offered_a_race_and_survives_one() {
    let b = Bench::new().await;
    // not chosen: a create of a marker loses nothing to a race
    let o = run_keyed(
        &b,
        "marker",
        "out-{trial}",
        &["run", "--trial", "-o", "chosen.sonde"],
    )
    .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    assert!(
        trials(&b.path("chosen.sonde"))
            .iter()
            .all(|t| t["scenario"] != "lost-race"),
        "{}",
        o.stderr
    );
    // asked for: it took the 412 as "it is there", and it is
    let o = run_keyed(
        &b,
        "marker",
        "out-{trial}",
        &[
            "run",
            "--allow-writes",
            "--fault",
            "lost-race",
            "-o",
            "marker.sonde",
        ],
    )
    .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let t = trial(&trials(&b.path("marker.sonde")), "lost-race").clone();
    assert_eq!(t["standing"], "survived", "{t:#}");
    assert_eq!(t["yielded"], 1, "{t:#}");
    assert!(!o.stderr.contains("never made"), "{}", o.stderr);
    assert!(
        b.store
            .keys("data")
            .contains(&"out-lost-race/_SUCCESS".to_string())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_completion_answered_500_though_made_is_looked_for_or_reported_wrongly() {
    let b = Bench::new().await;
    // its retry is answered NoSuchUpload, and it reports a failure: the
    // object is in the store all the same
    let o = b
        .run(
            Some("multipart"),
            &[
                "run",
                "--allow-writes",
                "--fault",
                "complete-500",
                "-o",
                "strict.sonde",
            ],
        )
        .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let t = trial(&trials(&b.path("strict.sonde")), "complete-500").clone();
    assert_eq!(t["standing"], "broke", "{t:#}");
    assert!(
        o.stderr
            .contains("the retried CompleteMultipartUpload was answered 404 NoSuchUpload: the upload had completed"),
        "{}",
        o.stderr
    );
    // one that takes NoSuchUpload as a question and looks: survived
    let o = b
        .run(
            Some("multipart-tolerant"),
            &["run", "--allow-writes", "--trial", "-o", "tolerant.sonde"],
        )
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let all = trials(&b.path("tolerant.sonde"));
    let t = trial(&all, "complete-500");
    assert_eq!(t["standing"], "survived", "{t:#}");
    assert_eq!(t["leftUploads"], 0, "{t:#}");
    // chosen with fail-complete for a program that uploads in parts
    assert_eq!(trial(&all, "fail-complete")["standing"], "survived");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_first_byte_waited_out_is_degraded() {
    let b = Bench::with_big().await;
    let started = std::time::Instant::now();
    let o = b
        .run(
            Some("patient-downloads"),
            &[
                "run",
                "--allow-writes",
                "--fault",
                "slow-first-byte",
                "-o",
                "patient.sonde",
            ],
        )
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let t = trial(&trials(&b.path("patient.sonde")), "slow-first-byte").clone();
    assert_eq!(t["standing"], "degraded", "{t:#}");
    assert_eq!((&t["held"], &t["waited"]), (&1.into(), &1.into()), "{t:#}");
    assert_eq!(t["advice"]["key"], "trial.advice.timeout", "{t:#}");
    assert!(
        o.stderr
            .contains("waited out 1 stall of 10.0 s before the first byte"),
        "{}",
        o.stderr
    );
    // the stall is held once, and no longer than it says
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_listing_that_does_not_show_a_write_yet_is_listed_again_or_broke() {
    let b = Bench::new().await;
    // it reads what it is shown, and goes on without the rest
    let o = b
        .run(
            Some("lists"),
            &["run", "--allow-writes", "--trial", "-o", "lists.sonde"],
        )
        .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let t = trial(&trials(&b.path("lists.sonde")), "stale-list").clone();
    assert_eq!(t["standing"], "broke", "{t:#}");
    assert_eq!((&t["stale"], &t["missed"]), (&2.into(), &2.into()), "{t:#}");
    assert_eq!(t["where"]["operation"], "ListObjectsV2", "{t:#}");
    // its session says which keys were left out
    let shown = b
        .run(
            None,
            &[
                "show",
                b.path("trials/lists.stale-list.sonde").to_str().unwrap(),
                "--requests",
                "-f",
                "json",
            ],
        )
        .await;
    assert!(shown.stdout.contains("x-sonde-hidden"), "{}", shown.stdout);
    // it lists until it is shown what it wrote
    let o = b
        .run(
            Some("lists-again"),
            &[
                "run",
                "--allow-writes",
                "--fault",
                "stale-list",
                "-o",
                "again.sonde",
            ],
        )
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let t = trial(&trials(&b.path("again.sonde")), "stale-list").clone();
    assert_eq!(t["standing"], "survived", "{t:#}");
    assert_eq!(t["relisted"], 2, "{t:#}");
}

#[tokio::test(flavor = "multi_thread")]
async fn with_the_runs_name_in_its_key_a_program_that_never_retries_still_broke() {
    let b = Bench::new().await;
    // the docs' own form: a place for each run, by {trial}
    let o = run_keyed(
        &b,
        "never-placed",
        "etl-{trial}",
        &[
            "run",
            "--allow-writes",
            "--fault",
            "slowdown",
            "-o",
            "placed.sonde",
        ],
    )
    .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let all = trials(&b.path("placed.sonde"));
    let t = trial(&all, "slowdown");
    assert_eq!(t["standing"], "broke", "{t:#}");
    assert_eq!(
        (&t["faults"], &t["retried"], &t["droppedWrites"]),
        (&1.into(), &0.into(), &1.into()),
        "its second write of the key is not the retry of its first: {t:#}"
    );
    assert!(t.get("backoffMs").is_none(), "its own sleep is no backoff");
    assert!(!o.stderr.contains("survived"), "{}", o.stderr);
    assert_eq!(
        b.store.keys("data"),
        ["etl-clean/state", "etl-slowdown/state"]
    );
    // the same with a fixed key
    let o = run_keyed(
        &b,
        "never-placed",
        "etl-fixed",
        &[
            "run",
            "--allow-writes",
            "--fault",
            "slowdown",
            "-o",
            "fixed.sonde",
        ],
    )
    .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    assert_eq!(
        trial(&trials(&b.path("fixed.sonde")), "slowdown")["standing"],
        "broke"
    );
    // and one that only reads SONDE_TRIAL from its environment
    let o = b
        .run(
            Some("never-placed"),
            &[
                "run",
                "--allow-writes",
                "--fault",
                "error",
                "-o",
                "env.sonde",
            ],
        )
        .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let all = trials(&b.path("env.sonde"));
    let t = trial(&all, "error");
    assert_eq!(t["standing"], "broke", "{t:#}");
    assert_eq!((&t["retried"], &t["droppedWrites"]), (&0.into(), &1.into()));
    assert!(
        b.store
            .keys("data")
            .contains(&"etl-error/state".to_string())
    );
    // the scenario's word standing by itself in the key (`error/state`) is
    // the key's own: the two runs write the same key, and it is matched
    let o = run_keyed(
        &b,
        "never-placed",
        "error",
        &[
            "run",
            "--allow-writes",
            "--fault",
            "error",
            "-o",
            "word.sonde",
        ],
    )
    .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    assert_eq!(
        trial(&trials(&b.path("word.sonde")), "error")["standing"],
        "broke"
    );
    // a program that retries survives in a place of its own
    let o = run_keyed(
        &b,
        "placed",
        "ok-{trial}",
        &["run", "--trial", "-o", "ok.sonde"],
    )
    .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    for t in trials(&b.path("ok.sonde")) {
        assert_eq!(t["standing"], "survived", "{t:#}");
        assert_eq!(t["faults"], t["retried"], "{t:#}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn what_the_store_itself_refuses_in_the_clean_run_is_not_held_against_the_program() {
    let b = Bench::new().await;
    // a store without DeleteObjects: 501, clean run and trial
    b.store.state.lock().no_batch_delete = true;
    let o = b
        .run(
            Some("tolerant-batches"),
            &[
                "run",
                "--allow-writes",
                "--fault",
                "slowdown",
                "-o",
                "run.sonde",
            ],
        )
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let all = trials(&b.path("run.sonde"));
    let t = trial(&all, "slowdown");
    assert_eq!(t["standing"], "survived", "{t:#}");
    assert_eq!(t["droppedWrites"], 0, "{t:#}");
    assert!(!o.stderr.contains("never sent again"), "{}", o.stderr);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_poll_is_judged_by_its_answers_and_called_a_retry_only_when_it_is_known_to_be() {
    let b = Bench::new().await;
    // a loop that polls for a fixed time, the SDK retrying: backoff uses
    // some of the time, and nothing was given up
    let o = b
        .run(
            Some("polls-for"),
            &[
                "run",
                "--allow-writes",
                "--fault",
                "slowdown",
                "-o",
                "timed.sonde",
            ],
        )
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let all = trials(&b.path("timed.sonde"));
    let t = trial(&all, "slowdown");
    assert_eq!(t["standing"], "survived", "{t:#}");
    assert_eq!(t["droppedReads"], 0, "{t:#}");
    assert!(!o.stderr.contains("gave up"), "{}", o.stderr);
    // a loop that polls until it has its answers, never retrying: the poll
    // after the failed one is the loop's own, and is not called a retry
    let o = b
        .run(
            Some("never-polls"),
            &[
                "run",
                "--allow-writes",
                "--fault",
                "slowdown",
                "-o",
                "counted.sonde",
            ],
        )
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let all = trials(&b.path("counted.sonde"));
    let t = trial(&all, "slowdown");
    assert_eq!(
        (&t["faults"], &t["retried"], &t["readAgain"]),
        (&1.into(), &0.into(), &1.into()),
        "{t:#}"
    );
    assert!(t.get("backoffMs").is_none(), "{t:#}");
    assert_eq!(t["standing"], "survived");
    assert!(o.stderr.contains("read again later"), "{}", o.stderr);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_body_cut_short_is_read_again_or_taken_for_the_whole() {
    let b = Bench::with_big().await;
    // the SDK retries a body that stopped: the whole of it again
    let o = b
        .run(
            Some("downloads"),
            &["run", "--trial", "-o", "retries.sonde"],
        )
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let all = trials(&b.path("retries.sonde"));
    // chosen by itself: the clean run downloaded a body large enough
    let t = trial(&all, "cut-body");
    assert_eq!(t["standing"], "survived", "{t:#}");
    assert_eq!(
        (&t["faults"], &t["retried"], &t["cut"], &t["reread"]),
        (&1.into(), &1.into(), &1.into(), &1.into()),
        "{t:#}"
    );
    assert!(o.stderr.contains("1 read again whole"), "{}", o.stderr);
    // the trial's session keeps the 200, marked as sonde's, and where it stopped
    let shown = b
        .run(
            None,
            &[
                "show",
                b.path("trials/retries.cut-body.sonde").to_str().unwrap(),
                "-f",
                "json",
            ],
        )
        .await;
    let doc: serde_json::Value = serde_json::from_str(&shown.stdout).expect(&shown.stdout);
    let cut = doc["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["injected"] == "cut-body")
        .expect("the cut request");
    assert_eq!(
        (&cut["status"], &cut["responseBytes"]),
        (&200.into(), &500.into())
    );
    let header = |name: &str| {
        cut["responseHeaders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["name"] == name)
            .map(|h| h["value"].as_str().unwrap().to_string())
    };
    assert_eq!(header("x-sonde-fault").as_deref(), Some("cut-body"));
    assert_eq!(header("x-sonde-cut-at").as_deref(), Some("500"));

    // one that takes what came, and exits as its clean run did
    let o = b
        .run(
            Some("never-downloads"),
            &[
                "run",
                "--allow-writes",
                "--fault",
                "cut-body",
                "-o",
                "never.sonde",
            ],
        )
        .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let all = trials(&b.path("never.sonde"));
    let t = trial(&all, "cut-body");
    assert_eq!(t["standing"], "broke", "{t:#}");
    assert_eq!((&t["retried"], &t["cut"]), (&0.into(), &1.into()));
    assert_eq!(t["where"]["operation"], "GetObject");
    assert_eq!(t["where"]["key"], "data/big");
    assert_eq!(t["advice"]["key"], "trial.advice.cut");
    assert!(
        o.stderr
            .contains("the half was taken as the whole (exit 0, as the clean run) · at GetObject data/big · check the bytes read against Content-Length"),
        "{}",
        o.stderr
    );
    // `check` says where, in every format
    let c = b.run(None, &["check", "never.sonde"]).await;
    assert_eq!(c.code, Some(1), "{}", c.stdout);
    assert!(c.stdout.contains("at GetObject data/big"), "{}", c.stdout);
    for format in ["markdown", "github", "json"] {
        let c = b.run(None, &["check", "never.sonde", "-f", format]).await;
        assert_eq!(c.code, Some(1), "{format}: {}", c.stdout);
        assert!(c.stdout.contains("data/big"), "{format}: {}", c.stdout);
    }
}

/// `curl` that signs as the SDKs do, when this machine has one.
#[cfg(unix)]
fn curl_signs() -> bool {
    std::process::Command::new("curl")
        .args(["--aws-sigv4", "aws:amz:us-east-1:s3", "--version"])
        .output()
        .is_ok_and(|o| o.status.success())
}

// (a shell script around curl: a Unix test)
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn curl_resumes_a_cut_download_rereads_it_or_keeps_the_half() {
    if !curl_signs() {
        eprintln!("skipped: no curl with --aws-sigv4 here");
        return;
    }
    let b = Bench::with_big().await;
    const CURL: &str = "curl -sS --aws-sigv4 aws:amz:us-east-1:s3 --user \"$AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY\"";
    let run = |name: &'static str, flags: &'static str, then: &'static str| {
        let get =
            format!("{CURL} {flags} -o out-{name}-$SONDE_TRIAL \"$AWS_ENDPOINT_URL/data/big\"");
        let script = format!(
            "rm -f out-{name}-$SONDE_TRIAL; {get}{}",
            then.replace("AGAIN", &get)
        );
        let mut c = b.sonde(None);
        c.args(["run", "--fault", "cut-body", "-o"])
            .arg(format!("{name}.sonde"))
            .args(["--", "sh", "-c", &script]);
        async move { out(c.output().await.unwrap()) }
    };
    let size = |file: &str| {
        std::fs::metadata(b.path(file))
            .map(|m| m.len())
            .unwrap_or(0)
    };

    // -C -: when the first try leaves half a file, the rest is asked for by
    // Range, from where the body stopped
    let o = run("resume", "-C -", " || AGAIN").await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let all = trials(&b.path("resume.sonde"));
    let t = trial(&all, "cut-body");
    assert_eq!(t["standing"], "survived", "{t:#}");
    assert_eq!((&t["cut"], &t["resumed"]), (&1.into(), &1.into()), "{t:#}");
    assert!(o.stderr.contains("1 resumed from the cut"), "{}", o.stderr);
    assert_eq!(size("out-resume-cut-body"), BIG as u64);

    // --retry alone: the whole of it again
    let o = run("again", "--retry 3 --retry-all-errors --retry-delay 0", "").await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let all = trials(&b.path("again.sonde"));
    let t = trial(&all, "cut-body");
    assert_eq!(t["standing"], "survived", "{t:#}");
    assert_eq!((&t["cut"], &t["reread"]), (&1.into(), &1.into()), "{t:#}");
    assert_eq!(size("out-again-cut-body"), BIG as u64);

    // no retry, and a script that does not look at curl's exit: half a file
    let o = run("half", "", "; exit 0").await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let all = trials(&b.path("half.sonde"));
    let t = trial(&all, "cut-body");
    assert_eq!(t["standing"], "broke", "{t:#}");
    assert_eq!(size("out-half-cut-body"), BIG as u64 / 2);
    assert_eq!(t["where"]["key"], "data/big");

    // no retry, and curl's own exit (18, a partial file): broke by the exit
    let o = run("fails", "", "").await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let all = trials(&b.path("fails.sonde"));
    let t = trial(&all, "cut-body");
    assert_eq!(
        (&t["standing"], &t["exit"]),
        (&"broke".into(), &18.into()),
        "{t:#}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_race_is_run_again_given_up_written_over_or_stops_a_fenced_writer() {
    let b = Bench::new().await;
    // reads the log again and commits the next number
    let o = run_keyed(
        &b,
        "commits",
        "t-{trial}/_delta_log",
        &["run", "--trial", "-o", "commits.sonde"],
    )
    .await;
    let all = trials(&b.path("commits.sonde"));
    // chosen by itself: the clean run wrote on a condition
    let t = trial(&all, "lost-race");
    // (and the lost answer of a conditional create is this program's
    // undoing: its retry finds its own write there and takes it for
    // another's, commit after commit)
    assert_eq!(
        trial(&all, "lost-answer")["standing"],
        "broke",
        "{}",
        o.stderr
    );
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    assert_eq!(t["standing"], "survived", "{t:#}");
    assert_eq!(
        (&t["faults"], &t["races"], &t["retried"]),
        (&1.into(), &1.into(), &1.into()),
        "{t:#}"
    );
    // the program's own write went to the store, and sonde wrote nothing
    let keys = b.store.keys("data");
    for n in [1, 2] {
        assert!(
            keys.contains(&format!("t-lost-race/_delta_log/{n:020}.json")),
            "{keys:?}"
        );
    }
    assert!(!keys.contains(&format!("t-clean/_delta_log/{:020}.json", 2)));
    // the 412 was sonde's: the session does not call it the store's race
    let shown = b
        .run(
            None,
            &[
                "show",
                b.path("trials/commits.lost-race.sonde").to_str().unwrap(),
                "--summary",
                "-f",
                "json",
            ],
        )
        .await;
    assert!(!shown.stdout.contains("obs.races"), "{}", shown.stdout);

    // takes the 412 for an answer and exits 0: the commit was never made
    let o = run_keyed(
        &b,
        "never-commits",
        "n-{trial}/_delta_log",
        &[
            "run",
            "--allow-writes",
            "--fault",
            "lost-race",
            "-o",
            "never.sonde",
        ],
    )
    .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let all = trials(&b.path("never.sonde"));
    let t = trial(&all, "lost-race");
    assert_eq!(t["standing"], "broke", "{t:#}");
    assert_eq!(t["where"]["operation"], "PutObject");
    assert_eq!(
        t["where"]["key"],
        format!("data/n-lost-race/_delta_log/{:020}.json", 1)
    );
    assert_eq!(t["advice"]["key"], "trial.advice.race");
    assert!(
        o.stderr
            .contains("lost its race: no commit followed it and nothing read back what won, and the program exited 0"),
        "{}",
        o.stderr
    );

    // writes the contested key without its condition
    let o = run_keyed(
        &b,
        "never-overwrites",
        "o-{trial}/_delta_log",
        &[
            "run",
            "--allow-writes",
            "--fault",
            "lost-race",
            "-o",
            "over.sonde",
        ],
    )
    .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    let all = trials(&b.path("over.sonde"));
    let t = trial(&all, "lost-race");
    assert_eq!(t["standing"], "broke", "{t:#}");
    assert!(o.stderr.contains("without a condition"), "{}", o.stderr);

    // a SlateDB writer that is fenced and stops: what the format asks
    let o = run_keyed(
        &b,
        "never-fenced",
        "db-{trial}/wal",
        &[
            "run",
            "--allow-writes",
            "--fault",
            "lost-race",
            "-o",
            "fenced.sonde",
        ],
    )
    .await;
    assert_eq!(o.code, Some(0), "stopping is not breaking: {}", o.stderr);
    let all = trials(&b.path("fenced.sonde"));
    let t = trial(&all, "lost-race");
    assert_eq!(t["standing"], "survived", "{t:#}");
    assert_eq!(
        (&t["fenced"], &t["exit"]),
        (&true.into(), &3.into()),
        "{t:#}"
    );
    assert!(o.stderr.contains("fenced"), "{}", o.stderr);
    // the same stop on any other log is giving up
    let o = run_keyed(
        &b,
        "never-fenced",
        "f-{trial}/_delta_log",
        &[
            "run",
            "--allow-writes",
            "--fault",
            "lost-race",
            "-o",
            "gaveup.sonde",
        ],
    )
    .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    assert_eq!(
        trial(&trials(&b.path("gaveup.sonde")), "lost-race")["standing"],
        "broke"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scenario_named_twice_runs_once() {
    let b = Bench::new().await;
    let o = b
        .run(
            Some("retrying"),
            &[
                "run",
                "--fault",
                "slowdown",
                "--fault",
                "slowdown",
                "-o",
                "run.sonde",
            ],
        )
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    assert_eq!(trials(&b.path("run.sonde")).len(), 1);
    assert!(
        o.stderr.contains("runs 1 more time,"),
        "once, not twice into one file: {}",
        o.stderr
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_interrupt_ends_what_the_program_started_too() {
    let b = Bench::new().await;
    let pidfile = b.path("grandchild.pid");
    let mut c = b.sonde(Some("spawns-under-trial"));
    c.env("E2E_PIDFILE", &pidfile)
        .args(["run", "--trial", "-o", "run.sonde", "--"])
        .args(the_program_command())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = c.spawn().unwrap();
    let pid = child.id().unwrap();
    for _ in 0..600 {
        if pidfile.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let grandchild = std::fs::read_to_string(&pidfile).expect("the program started its own");
    let alive = || {
        std::process::Command::new("kill")
            .args(["-0", grandchild.trim()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    assert!(alive(), "the grandchild runs");
    // as a supervisor does: the signal to sonde alone, not to a terminal's group
    let sent = std::process::Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
    let o = tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
        .await
        .expect("sonde ended the trial")
        .unwrap();
    assert_eq!(out(o).code, Some(130));
    for _ in 0..100 {
        if !alive() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(!alive(), "the program's own process went with it");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_input_too_large_to_hold_is_fed_to_every_run_all_the_same() {
    use tokio::io::AsyncWriteExt;
    let b = Bench::new().await;
    let mut c = b.sonde(Some("stdin-size"));
    c.args(["run", "--fault", "slowdown", "-o", "run.sonde", "--"])
        .args(the_program_command())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    // past the 8 MiB sonde keeps in memory: kept in a file for the trial
    const SIZE: usize = (9 << 20) + 5;
    let piece = vec![b'x'; 1 << 20];
    let mut left = SIZE;
    while left > 0 {
        let n = left.min(piece.len());
        stdin.write_all(&piece[..n]).await.unwrap();
        left -= n;
    }
    drop(stdin);
    let o = out(child.wait_with_output().await.unwrap());
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let all = trials(&b.path("run.sonde"));
    let t = trial(&all, "slowdown");
    assert_eq!(
        (&t["exit"], &t["standing"]),
        (&0.into(), &"survived".into()),
        "{t:#}"
    );
    // the trial's run wrote last: it read every byte the clean run did
    assert_eq!(
        b.store.state.lock().buckets["data"]["size"].0,
        SIZE.to_string().as_bytes()
    );
    assert_eq!(
        b.store.sent("PUT /data/size").len(),
        2,
        "clean run and trial"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn by_default_a_run_writes_again_only_what_it_wrote_and_deletes_nothing() {
    let b = Bench::new().await;
    // a trial under the default policy: each run writes again what the
    // clean run wrote
    let o = b
        .run(Some("retrying"), &["run", "--trial", "-o", "run.sonde"])
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    assert!(o.stderr.contains("did not break"), "{}", o.stderr);
    // run again: the program writes over what its last run wrote
    let again = b.run(Some("retrying"), &["run", "-o", "again.sonde"]).await;
    assert_eq!(again.code, Some(0), "{}", again.stderr);
    assert!(b.path("written.json").is_file(), "kept beside its runs");
    // another program may not: what is there is not its own
    let other = b
        .run(
            Some("retrying"),
            &["run", "--name", "other", "-o", "other.sonde"],
        )
        .await;
    assert_eq!(other.code, Some(1), "{}", other.stderr);
    let s = sonde_proxy::session::Session::open(&b.path("other.sonde")).unwrap();
    let refused = s.records.iter().find(|r| r.status == Some(403)).unwrap();
    assert!(
        refused
            .error_body
            .as_deref()
            .is_some_and(|e| e.contains("no-delete") && e.contains("data/a")),
        "{refused:?}"
    );
    assert_eq!(b.store.state.lock().buckets["data"]["a"].0, b"1");
    // the project may let it, and sonde says so; a flag is over the file
    std::fs::write(b.path("sonde.toml"), "[record]\npolicy = \"allow\"\n").unwrap();
    let allowed = b
        .run(
            Some("retrying"),
            &["run", "--name", "other", "-o", "o.sonde"],
        )
        .await;
    assert_eq!(allowed.code, Some(0), "{}", allowed.stderr);
    assert!(
        allowed.stderr.contains("[record] policy"),
        "{}",
        allowed.stderr
    );
    let flag = b
        .run(
            Some("retrying"),
            &[
                "run",
                "--policy",
                "no-delete",
                "--name",
                "third",
                "-o",
                "again.sonde",
            ],
        )
        .await;
    assert_eq!(flag.code, Some(1), "{}", flag.stderr);
    // deletes are refused by default
    std::fs::remove_file(b.path("sonde.toml")).unwrap();
    let deletes = b
        .run(Some("batches"), &["run", "-o", "deletes.sonde"])
        .await;
    assert_eq!(deletes.code, Some(1), "{}", deletes.stderr);
    std::fs::write(b.path("sonde.toml"), "[record]\npolicy = \"everything\"\n").unwrap();
    let wrong = b.run(Some("retrying"), &["run", "-o", "again.sonde"]).await;
    assert_eq!(wrong.code, Some(64), "{}", wrong.stderr);
    assert!(wrong.stderr.contains("one of allow"), "{}", wrong.stderr);
}

/// The newcomer's walk: a program that rewrites its own outputs is run
/// again, and a trial is tried again, under the default policy.
#[tokio::test(flavor = "multi_thread")]
async fn a_program_rewrites_its_own_outputs_and_a_trial_is_tried_again_under_no_delete() {
    let b = Bench::new().await;
    // a plain second run of a program that writes the same key
    for n in 0..2 {
        let o = run_keyed(&b, "key", "out/state", &["run"]).await;
        assert_eq!(o.code, Some(0), "run {n}: {}", o.stderr);
    }
    // a trial, then the same trial again: `{trial}` keys are the same each
    // time, and the second trial's clean run is not refused
    for n in 0..2 {
        let o = run_keyed(&b, "key", "t{trial}/x", &["run", "--trial"]).await;
        assert_eq!(o.code, Some(0), "trial {n}: {}", o.stderr);
        assert!(o.stderr.contains("did not break"), "{}", o.stderr);
    }
    // with nothing remembered (a trial of before), the `{trial}` place
    // alone is the trial's own
    std::fs::remove_file(b.path("data/sessions/written.json")).unwrap();
    let o = run_keyed(&b, "key", "t{trial}/x", &["run", "--trial"]).await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    // but not a key outside it, which this program never wrote
    b.store
        .state
        .lock()
        .buckets
        .entry("data".into())
        .or_default()
        .insert("theirs".into(), (b"keep".to_vec(), "\"t\"".into()));
    let o = run_keyed(&b, "key", "theirs", &["run"]).await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    assert_eq!(b.store.state.lock().buckets["data"]["theirs"].0, b"keep");
}

/// A scenario the policy refused is "refused", never "broke", and is tried
/// again.
#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_by_the_policy_under_a_trial_is_its_own_outcome() {
    let b = Bench::new().await;
    // what the trial's run will write is there, and not the program's
    b.store
        .state
        .lock()
        .buckets
        .entry("data".into())
        .or_default()
        .insert("etl-slowdown/state".into(), (b"x".to_vec(), "\"x\"".into()));
    let o = b
        .run(
            Some("placed"),
            &["run", "--fault", "slowdown", "-o", "run.sonde"],
        )
        .await;
    assert_eq!(o.code, Some(0), "not broke: {}", o.stderr);
    let all = trials(&b.path("run.sonde"));
    let t = trial(&all, "slowdown");
    assert_eq!(t["standing"], "refused", "{t:#}");
    // its keys carry the trial's name already: not told to add it
    assert_eq!(t["advice"]["key"], "trial.advice.policy.placed");
    assert!(o.stderr.contains("refused   slowdown"), "{}", o.stderr);
    assert!(
        o.stderr
            .contains("refused by the recording policy (no-delete)"),
        "{}",
        o.stderr
    );
    let c = b.run(None, &["check", "run.sonde"]).await;
    assert_eq!(c.code, Some(0), "{}", c.stdout);
    assert!(
        c.stdout
            .contains("warn  refused by the policy under slowdown"),
        "{}",
        c.stdout
    );
}

/// Broke, changed, tried again: only what broke runs, and each verdict says
/// what it was ("broke on …, survived since"), in `check` and `show` too.
#[tokio::test(flavor = "multi_thread")]
async fn trying_again_runs_only_what_broke_and_says_it_survived_since() {
    let b = Bench::new().await;
    let o = b
        .run(
            Some("never-twice"),
            &["run", "--trial", "--name", "etl", "-o", "run.sonde"],
        )
        .await;
    assert_eq!(o.code, Some(1), "{}", o.stderr);
    assert!(
        o.stderr
            .contains("sonde run --trial --again --name etl -o run.sonde -- "),
        "the way to try again is said: {}",
        o.stderr
    );
    let first = trials(&b.path("run.sonde"));
    let broke: Vec<String> = first
        .iter()
        .filter(|t| t["standing"] != "survived" && t["standing"] != "untested")
        .map(|t| t["scenario"].as_str().unwrap().to_string())
        .collect();
    assert!(
        !broke.is_empty() && broke.len() < first.len() + 1,
        "{first:#?}"
    );
    // the program changed: it retries now
    let again = b
        .run(
            Some("retrying"),
            &[
                "run",
                "--trial",
                "--again",
                "--name",
                "etl",
                "-o",
                "run.sonde",
            ],
        )
        .await;
    assert_eq!(again.code, Some(0), "{}", again.stderr);
    let now = trials(&b.path("run.sonde"));
    let mut tried: Vec<String> = now
        .iter()
        .map(|t| t["scenario"].as_str().unwrap().to_string())
        .collect();
    tried.sort();
    let mut want = broke.clone();
    want.sort();
    assert_eq!(tried, want, "only what broke: {}", again.stderr);
    assert!(
        again.stderr.contains(", survived since"),
        "{}",
        again.stderr
    );
    // check and show say it, and their JSON has the history
    let c = b.run(None, &["check", "run.sonde"]).await;
    assert_eq!(c.code, Some(0), "{}{}", c.stdout, c.stderr);
    assert!(c.stdout.contains(", survived since"), "{}", c.stdout);
    let j = b.run(None, &["check", "run.sonde", "-f", "json"]).await;
    let doc: serde_json::Value = serde_json::from_str(&j.stdout).expect(&j.stdout);
    let history = doc["trials"][0]["history"].as_array().unwrap();
    assert_eq!(history.len(), 2, "{history:#?}");
    assert_eq!(history[1]["standing"], "survived");
    assert_eq!(history[1]["run"], "run.sonde");
    let s = b.run(None, &["show", "run.sonde", "--summary"]).await;
    assert!(s.stdout.contains("Under faults:"), "{}", s.stdout);
    assert!(s.stdout.contains(", survived since"), "{}", s.stdout);
    let j = b
        .run(None, &["show", "run.sonde", "--summary", "-f", "json"])
        .await;
    let doc: serde_json::Value = serde_json::from_str(&j.stdout).expect(&j.stdout);
    assert_eq!(doc["trials"][0]["history"].as_array().unwrap().len(), 2);
    // recorded again to the same file: the verdicts are the program's, kept
    let plain = b
        .run(
            Some("retrying"),
            &["run", "--name", "etl", "-o", "run.sonde"],
        )
        .await;
    assert_eq!(plain.code, Some(0), "{}", plain.stderr);
    assert!(b.path("verdicts.json").is_file());
    // nothing left to try
    let none = b
        .run(
            Some("retrying"),
            &[
                "run",
                "--trial",
                "--again",
                "--name",
                "etl",
                "-o",
                "run.sonde",
            ],
        )
        .await;
    assert_eq!(none.code, Some(0), "{}", none.stderr);
    assert!(
        none.stderr.contains("Nothing to try again"),
        "{}",
        none.stderr
    );
    // a program never tried: a usage error
    let never = b
        .run(
            Some("retrying"),
            &[
                "run", "--trial", "--again", "--name", "new", "-o", "n.sonde",
            ],
        )
        .await;
    assert_eq!(never.code, Some(64), "{}", never.stderr);
}

/// `run --demo`: the program against the built-in store, no endpoint
/// configured and no secrets; said to be the demo's, its times not meaningful.
#[tokio::test(flavor = "multi_thread")]
async fn run_demo_records_a_program_against_the_built_in_store() {
    let dir = tempfile::tempdir().unwrap();
    let sonde = |args: &[&str], program: bool| {
        let mut c = tokio::process::Command::new(env!("CARGO_BIN_EXE_sonde"));
        c.current_dir(dir.path())
            .env("SONDE_CONFIG_DIR", dir.path().join("config"))
            .env("SONDE_DATA_DIR", dir.path().join("data"))
            .env("AWS_EC2_METADATA_DISABLED", "true")
            .env_remove("AWS_ACCESS_KEY_ID")
            .env_remove("AWS_SECRET_ACCESS_KEY")
            .env_remove("AWS_PROFILE")
            .env("SONDE_E2E", "demo")
            .args(args)
            .stdin(Stdio::null());
        if program {
            c.arg("--").args(the_program_command());
        }
        c
    };
    let session = dir.path().join("demo.sonde");
    let o = out(
        sonde(&["run", "--demo", "-o", session.to_str().unwrap()], true)
            .output()
            .await
            .unwrap(),
    );
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    assert!(
        o.stderr.contains("built-in demo store") && o.stderr.contains("times are not meaningful"),
        "{}",
        o.stderr
    );
    let shown = out(
        sonde(&["show", session.to_str().unwrap(), "-f", "json"], false)
            .output()
            .await
            .unwrap(),
    );
    let v: serde_json::Value = serde_json::from_str(&shown.stdout).unwrap();
    assert_eq!(v["summary"]["requests"], 2, "{v}");
    assert_eq!(v["summary"]["errors"], 0, "{v}");
    // one store at a time
    for other in [&["--url", "http://127.0.0.1:9"][..], &["-e", "mine"][..]] {
        let mut args = vec!["run", "--demo"];
        args.extend_from_slice(other);
        let o = out(sonde(&args, true).output().await.unwrap());
        assert_eq!(o.code, Some(64), "{other:?}: {}", o.stderr);
    }
}

/// Two strengths of one scenario stand broke: each is run again, into a
/// session of its own; the name is trimmed as it was when kept.
#[tokio::test(flavor = "multi_thread")]
async fn trying_again_runs_every_strength_that_broke() {
    let b = Bench::new().await;
    let broke = serde_json::json!([{
        "at": "2026-11-01T00:00:00Z", "standing": "broke", "run": "run.sonde"
    }]);
    let kept = serde_json::json!({ "etl": { "slowdown": broke, "slowdown:3": broke } });
    std::fs::write(b.path("verdicts.json"), kept.to_string()).unwrap();
    let o = b
        .run(
            Some("retrying"),
            &[
                "run",
                "--trial",
                "--again",
                "--name",
                " etl ",
                "-o",
                "run.sonde",
            ],
        )
        .await;
    assert_eq!(o.code, Some(0), "{}", o.stderr);
    let mut sessions: Vec<String> = trials(&b.path("run.sonde"))
        .iter()
        .map(|t| t["session"].as_str().unwrap().to_string())
        .collect();
    sessions.sort();
    assert_eq!(
        sessions,
        ["run.slowdown-3x.sonde", "run.slowdown.sonde"],
        "{}",
        o.stderr
    );
}
