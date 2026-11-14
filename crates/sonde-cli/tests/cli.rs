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
fn help_lists_the_commands_by_what_they_are_for() {
    let t = tempfile::tempdir().unwrap();
    let help = stdout(&sonde(t.path(), &["--help"]));
    let at = |what: &str| {
        help.find(what)
            .unwrap_or_else(|| panic!("no {what:?} in:\n{help}"))
    };
    // each heading, then its commands, each with its own line
    for (heading, first, last) in [
        ("Record:", "\n  run ", "\n  import "),
        ("Read a run:", "\n  sessions ", "\n  share "),
        ("Buckets:", "\n  bucket ", "\n  endpoints "),
    ] {
        assert!(at(heading) < at(first) && at(first) < at(last), "{help}");
    }
    assert!(at("\n  import ") < at("Read a run:"), "{help}");
    assert!(
        help.contains("run        Run a command with its S3 requests recorded"),
        "{help}"
    );
    assert!(
        at("Options:") < at("--config-dir") && at("--config-dir") < at("--version"),
        "{help}"
    );
    // what sonde is now, not what it was before it recorded anything
    assert!(
        help.starts_with("See what your software does to S3"),
        "{help}"
    );
    // a command's own help is clap's, as before
    let run = stdout(&sonde(t.path(), &["run", "--help"]));
    assert!(run.contains("Usage: sonde run"), "{run}");
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
    assert_eq!(v["requests"].as_array().unwrap().len(), 28);
    assert!(v["findings"].as_array().unwrap().len() >= 2, "{v}");

    let csv = stdout(&sonde(t.path(), &["sessions", "-f", "csv"]));
    let mut lines = csv.lines();
    assert_eq!(
        lines.next(),
        Some("session,recorded,program,endpoint,requests,took,errors,trials,size")
    );
    let name = lines.next().unwrap().split(',').next().unwrap().to_string();
    assert!(lines.next().is_none());

    // a session by its name alone
    let by_name = sonde(
        t.path(),
        &["show", name.trim_end_matches(".sonde"), "--summary"],
    );
    assert_eq!(by_name.status.code(), Some(0));
    assert!(stdout(&by_name).contains("28 requests"));

    // the same run against itself: no change
    let path = t.path().join("data/sessions").join(&name);
    let p = path.to_str().unwrap();
    let same = stdout(&sonde(t.path(), &["compare", p, p]));
    assert!(same.starts_with("no change"), "{same}");
    assert!(same.contains("requests   28 (no change)"), "{same}");
    assert!(!same.contains("0%"), "{same}");
}

#[test]
fn the_demo_reads_as_operations_not_as_a_list_of_requests() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    let out = stdout(&sonde(t.path(), &["show", "--all"]));
    // what sonde reads from requests, each once in the tour
    for line in [
        "4 lookups in demo/tour/, none found",
        "multipart upload demo/tour/export.csv · 5 parts",
        "commit 0 · 3 files",
        "PutObject tour/summary.json retried 1× after 503 SlowDown",
        "listed demo/tour/ in 3 pages",
        "28 requests (11 operations)",
    ] {
        assert!(out.contains(line), "no {line:?} in:\n{out}");
    }
    // and at least two findings to act on
    for finding in [
        "1 request throttled (503 SlowDown or 429)",
        "3 objects were checked with HEAD and then read with GET",
    ] {
        assert!(out.contains(finding), "no {finding:?} in:\n{out}");
    }
}

#[test]
fn damaged_baselines_are_said_and_neither_written_over_nor_read_as_none() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    let file = t.path().join("data/sessions/baselines.json");
    let damaged = "{\"other program\": \"kept.so";
    std::fs::write(&file, damaged).unwrap();
    for args in [
        &["check", "--set-baseline", "--max-requests", "100"][..],
        &["check", "--baseline", "auto"],
        &["sessions", "prune", "--keep", "0", "--yes"],
    ] {
        let o = sonde(t.path(), args);
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(64), "{args:?}: {err}");
        assert!(
            err.contains("baselines.json cannot be read"),
            "{args:?}: {err}"
        );
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), damaged);
    // the session a damaged file may have named as a baseline is still there
    assert_eq!(sonde(t.path(), &["show"]).status.code(), Some(0));
}

#[test]
fn a_probe_run_named_on_the_command_line_is_not_held_to_read_only_mode() {
    let t = tempfile::tempdir().unwrap();
    // read-only is the default; the run starts (and fails here on the connection, not refused)
    let args = ["--url", "http://127.0.0.1:9", "probe", "-b", "scratch"];
    assert_ne!(sonde(t.path(), &args).status.code(), Some(77));
}

#[test]
fn check_holds_a_run_to_its_programs_baseline() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    // no baseline yet is no mistake (a first run in CI has none): it is
    // said, with the program it is of, and the run is not failed for it
    let first = sonde(t.path(), &["check", "--baseline", "auto"]);
    let err = String::from_utf8_lossy(&first.stderr).into_owned();
    assert_eq!(first.status.code(), Some(0), "{err}");
    assert!(err.contains("no baseline yet for program `"), "{err}");
    assert!(
        stdout(&first).contains("nothing checked"),
        "{}",
        stdout(&first)
    );
    // with a budget, the budget is what decides
    let over = sonde(
        t.path(),
        &["check", "--baseline", "auto", "--max-requests", "1"],
    );
    assert_eq!(over.status.code(), Some(1));
    assert!(
        stdout(&over).contains("over budget (no baseline yet)"),
        "{}",
        stdout(&over)
    );
    // said in every format: a comment, an annotation, a field
    let md = sonde(t.path(), &["check", "--baseline", "auto", "-f", "markdown"]);
    assert_eq!(md.status.code(), Some(0));
    assert!(
        stdout(&md).contains("> ⚠️ no baseline yet for program"),
        "{}",
        stdout(&md)
    );
    let gh = sonde(t.path(), &["check", "--baseline", "auto", "-f", "github"]);
    assert_eq!(gh.status.code(), Some(0));
    assert!(
        stdout(&gh).starts_with("::warning title=sonde check::no baseline yet"),
        "{}",
        stdout(&gh)
    );
    let json = sonde(t.path(), &["check", "--baseline", "auto", "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert!(v["noBaseline"].is_string(), "{v}");
    assert_eq!(v["verdict"], "nothing checked (no baseline yet)");
    // nothing at all to hold it to is still a mistake in the command
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
    // the mechanism, not this machine's timing: two runs of the demo may differ by more than 10% in waiting
    let c = sonde(
        t.path(),
        &[
            "check",
            "--baseline",
            "auto",
            "--tolerance",
            "200",
            "-f",
            "json",
        ],
    );
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

#[test]
fn show_by_splits_the_operations_table_and_names_the_sdk() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    let o = sonde(t.path(), &["show", "--by", "kind"]);
    assert_eq!(o.status.code(), Some(0));
    let out = stdout(&o);
    let lines: Vec<&str> = out.lines().collect();
    // the head line names the SDK; then one table, split
    assert!(lines[1].contains(" from sonde "), "{out}");
    assert!(
        lines[2].starts_with("kind") && lines[2].contains("p99") && lines[2].contains("cost"),
        "{out}"
    );
    assert!(lines.iter().any(|l| l.starts_with("HeadObject")), "{out}");
    // the summary's head line names it too
    let summary = stdout(&sonde(t.path(), &["show", "--summary"]));
    assert!(summary.contains(" from sonde "), "{summary}");
    // a dimension it does not know, and a flag it would ignore
    assert_eq!(
        sonde(t.path(), &["show", "--by", "bucket"]).status.code(),
        Some(64)
    );
    assert_eq!(
        sonde(t.path(), &["show", "--by", "size", "--requests"])
            .status
            .code(),
        Some(64)
    );
}

#[test]
fn a_shared_page_of_two_runs_is_their_comparison() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    let mut names: Vec<String> = std::fs::read_dir(t.path().join("data/sessions"))
        .unwrap()
        .flatten()
        .map(|e| e.path().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".sonde"))
        .collect();
    names.sort();
    let page = t.path().join("page.html");
    let o = sonde(
        t.path(),
        &[
            "share",
            "--html",
            &names[0],
            &names[1],
            "-o",
            page.to_str().unwrap(),
        ],
    );
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let html = std::fs::read_to_string(&page).unwrap();
    let against = html.find("<h2>Against session ").expect("the comparison");
    assert!(against < html.find("<h2>This run</h2>").unwrap());
    // the same as --baseline FILE
    let o = sonde(
        t.path(),
        &["share", "--html", &names[1], "--baseline", &names[0]],
    );
    assert!(stdout(&o).contains("<h2>Against session "));
    // a comparison is a page; and two runs or a baseline, not both
    assert_eq!(
        sonde(t.path(), &["share", &names[0], &names[1]])
            .status
            .code(),
        Some(64)
    );
    assert_eq!(
        sonde(
            t.path(),
            &[
                "share",
                "--html",
                &names[0],
                &names[1],
                "--baseline",
                "auto"
            ]
        )
        .status
        .code(),
        Some(64)
    );
    // a link in a comment is a comment's
    let o = sonde(
        t.path(),
        &[
            "check",
            "--max-requests",
            "1000",
            "--report-url",
            "https://x",
        ],
    );
    assert_eq!(o.status.code(), Some(64));
    let o = sonde(
        t.path(),
        &[
            "check",
            &names[1],
            "--baseline",
            &names[0],
            "-f",
            "markdown",
            "--report-url",
            "https://x.test/r",
        ],
    );
    let md = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{md}");
    assert!(md.contains("**Against the baseline**"), "{md}");
    assert!(md.contains("[The report](https://x.test/r)"), "{md}");
    // the program's status heads it; JSON says it too
    let o = sonde(
        t.path(),
        &[
            "check",
            &names[1],
            "--max-requests",
            "1000",
            "-f",
            "markdown",
            "--program-exit",
            "3",
        ],
    );
    let md = stdout(&o);
    assert!(md.contains("> ❌ **The program failed** (exit 3)"), "{md}");
    let o = sonde(
        t.path(),
        &[
            "check",
            &names[1],
            "--max-requests",
            "1000",
            "-f",
            "json",
            "--program-exit",
            "3",
        ],
    );
    assert!(stdout(&o).contains("\"programExit\": 3"), "{}", stdout(&o));
}

#[test]
fn check_names_the_run_it_judged_and_says_by_how_much_it_failed() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    let csv = stdout(&sonde(t.path(), &["sessions", "-f", "csv"]));
    let name = csv.lines().nth(1).unwrap().split(',').next().unwrap();
    let c = sonde(t.path(), &["check", "--max-requests", "20"]);
    assert_eq!(c.status.code(), Some(1));
    let out = stdout(&c);
    let first = out.lines().next().unwrap();
    assert!(
        first.starts_with(&format!("session {name} · recorded 20")),
        "{out}"
    );
    // 28 requests against 20
    let fail = out.lines().find(|l| l.starts_with("FAIL")).unwrap();
    assert!(
        fail.contains("requests") && fail.trim_end().ends_with("+40%"),
        "{out}"
    );

    // the demo warns (a retry); against itself none of it is new, and the row says so
    let same = sonde(
        t.path(),
        &["check", "--baseline", name, "--no-new-warnings"],
    );
    let out = stdout(&same);
    assert_eq!(same.status.code(), Some(0), "{out}");
    assert!(out.contains(&format!("against session {name}")), "{out}");
    assert!(out.contains("new warnings  0"), "{out}");
}

#[test]
fn a_baseline_set_alone_succeeds_by_name_or_relative_path() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    let dir = t.path().join("data/sessions");
    let name = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.ends_with(".sonde"))
        .unwrap();
    // from inside the sessions directory, by a relative path
    let set = Command::new(env!("CARGO_BIN_EXE_sonde"))
        .args(["check", &name, "--set-baseline"])
        .current_dir(&dir)
        .env("SONDE_CONFIG_DIR", t.path().join("config"))
        .env("SONDE_DATA_DIR", t.path().join("data"))
        .output()
        .unwrap();
    assert_eq!(
        set.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    // a baseline named, not a path
    let by_name = sonde(
        t.path(),
        &["check", "--baseline", name.trim_end_matches(".sonde")],
    );
    assert_eq!(
        by_name.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&by_name.stderr)
    );
    // what goes to a file has no colour in it
    assert!(!stdout(&sonde(t.path(), &["show", "--summary"])).contains('\u{1b}'));
}

#[test]
fn a_projects_sonde_toml_is_its_budget_and_markdown_is_a_pr_comment() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    let cfg = t.path().join("sonde.toml");
    // the budget kept with the code: too few requests allowed
    std::fs::write(&cfg, "[check]\nmax-requests = 1\nmax-waiting = \"60s\"\n").unwrap();
    let c = sonde(
        t.path(),
        &["check", "--config", cfg.to_str().unwrap(), "-f", "markdown"],
    );
    assert_eq!(
        c.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&c.stderr)
    );
    let md = stdout(&c);
    assert!(md.starts_with("### sonde: over budget"), "{md}");
    // (the table's columns line up as text: cells are padded)
    let cells = md
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        cells.contains("| **FAIL** +2700% | requests | 28 | ≤ 1 |"),
        "{md}"
    );
    // a row that passed is not repeated in a comment
    assert!(!cells.contains("| pass |"), "{md}");
    // a flag still wins over the file
    let c = sonde(
        t.path(),
        &[
            "check",
            "--config",
            cfg.to_str().unwrap(),
            "--max-requests",
            "100000",
        ],
    );
    assert_eq!(c.status.code(), Some(0), "{}", stdout(&c));
    // a mistake in the file says where
    std::fs::write(&cfg, "[check]\nmax-requets = 1\n").unwrap();
    let c = sonde(t.path(), &["check", "--config", cfg.to_str().unwrap()]);
    assert_ne!(c.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&c.stderr).contains("sonde.toml"));
}

#[test]
fn check_init_writes_a_budget_the_run_passes_and_github_annotates_it() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    let cfg = t.path().join("sonde.toml");
    let c = sonde(
        t.path(),
        &["check", "--init", "--config", cfg.to_str().unwrap()],
    );
    assert_eq!(
        c.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&c.stderr)
    );
    let text = std::fs::read_to_string(&cfg).unwrap();
    assert!(
        text.contains("[check]") && text.contains("max-requests ="),
        "{text}"
    );
    // the run it was made from passes it
    let c = sonde(
        t.path(),
        &["check", "--config", cfg.to_str().unwrap(), "-f", "github"],
    );
    assert_eq!(c.status.code(), Some(0), "{}", stdout(&c));
    assert!(
        stdout(&c).contains("sonde: within budget"),
        "{}",
        stdout(&c)
    );
    // never over a file there is
    let again = sonde(
        t.path(),
        &["check", "--init", "--config", cfg.to_str().unwrap()],
    );
    assert_eq!(again.status.code(), Some(64));
    // a budget it is past: an error annotation on the run
    std::fs::write(&cfg, "[check]\nmax-requests = 1\n").unwrap();
    let c = sonde(
        t.path(),
        &["check", "--config", cfg.to_str().unwrap(), "-f", "github"],
    );
    assert_eq!(c.status.code(), Some(1));
    assert!(
        stdout(&c).contains("::error title=sonde check::requests"),
        "{}",
        stdout(&c)
    );
}

#[test]
fn a_gzipped_log_is_imported_and_its_odd_lines_are_counted() {
    use std::io::Write;
    let t = tempfile::tempdir().unwrap();
    let log = concat!(
        "79a5 lake [06/Feb/2026:00:00:38 +0000] 192.0.2.3 - 3E57 REST.GET.OBJECT t/a \"GET /lake/t/a HTTP/1.1\" 200 - 4096 4096 12 10 \"-\" \"boto3/1.34\" - x SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -\n",
        "79a5 lake [06/Feb/2026:00:00:41 +0000] - AmazonS3 5E57 S3.EXPIRE.OBJECT t/old - \"-\" - - - 4096 - - \"-\" \"-\" - x - - - - - - -\n",
        "79a5 lake [06/Feb/2026:00:00:44 +0000] 192.0.2.3 arn:aws\n",
    );
    let file = t.path().join("2026-02-06.log.gz");
    let mut gz = flate2::write::GzEncoder::new(
        std::fs::File::create(&file).unwrap(),
        flate2::Compression::default(),
    );
    gz.write_all(log.as_bytes()).unwrap();
    gz.finish().unwrap();
    let out = t.path().join("out.sonde");
    let o = sonde(
        t.path(),
        &[
            "import",
            file.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ],
    );
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert!(
        err.contains("1 request from an S3 server access log"),
        "{err}"
    );
    assert!(err.contains("1 line could not be read"), "{err}");
    assert!(
        err.contains("1 line left out: the store's own work"),
        "{err}"
    );
    assert!(out.is_file());
}

#[test]
fn a_folder_of_logs_is_imported_as_one_session() {
    let t = tempfile::tempdir().unwrap();
    let logs = t.path().join("lake-logs");
    std::fs::create_dir_all(logs.join("2026-02-06")).unwrap();
    let line = |second: u32, id: &str, key: &str| {
        format!(
            r#"79a5 lake [06/Feb/2026:00:00:{second:02} +0000] 192.0.2.3 arn:aws:iam::1:user/etl {id} REST.GET.OBJECT {key} "GET /lake/{key} HTTP/1.1" 200 - 4096 4096 12 10 "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#
        )
    };
    // named out of time order: the session is in time order all the same
    std::fs::write(logs.join("2026-02-06/b"), line(1, "A1", "t/one")).unwrap();
    std::fs::write(logs.join("2026-02-06/a"), line(2, "A2", "t/two")).unwrap();
    std::fs::write(logs.join("c"), line(3, "A3", "t/three")).unwrap();
    std::fs::write(logs.join("notes.txt"), "kept by hand").unwrap();
    let out = t.path().join("logs.sonde");
    let o = sonde(
        t.path(),
        &[
            "import",
            logs.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ],
    );
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert!(
        err.contains("3 requests from 3 files of S3 server access log"),
        "{err}"
    );
    assert!(
        err.contains("1 file left out:") && err.contains("notes.txt: not an S3"),
        "{err}"
    );
    let shown = stdout(&sonde(
        t.path(),
        &["show", out.to_str().unwrap(), "--requests"],
    ));
    let at = |k: &str| shown.find(k).unwrap_or_else(|| panic!("no {k} in {shown}"));
    assert!(
        at("t/one") < at("t/two") && at("t/two") < at("t/three"),
        "{shown}"
    );
    // named by the store the log is of, not by the folder it was in
    assert!(shown.contains("to s3.amazonaws.com"), "{shown}");

    // a folder with no log in it is a mistake in the command
    let none = t.path().join("none");
    std::fs::create_dir(&none).unwrap();
    assert_eq!(
        sonde(t.path(), &["import", none.to_str().unwrap()])
            .status
            .code(),
        Some(64)
    );
}

#[test]
fn every_command_refuses_a_format_it_does_not_write() {
    let t = tempfile::tempdir().unwrap();
    // before anything is asked of a store: none of these has one to ask
    for args in [
        &["bucket", "size", "-b", "b", "-f", "markdown"][..],
        &["bucket", "uploads", "-b", "b", "-f", "github"],
        &["bucket", "abort-uploads", "-b", "b", "-f", "json"],
        &["probe", "list", "-f", "csv"],
        &["probe", "-b", "scratch", "-f", "github"],
        &["layout", "inspect", "-b", "b", "-f", "csv"],
        &["endpoints", "list", "-f", "csv"],
        &["sessions", "-f", "github"],
        &["sessions", "prune", "--keep", "5", "-f", "json"],
    ] {
        let o = sonde(t.path(), args);
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        assert_eq!(o.status.code(), Some(64), "{args:?}: {err}");
        assert!(err.contains("does not write -f"), "{args:?}: {err}");
        assert!(o.stdout.is_empty(), "{args:?}: {}", stdout(&o));
    }
    // what each does write is still written
    let list = sonde(t.path(), &["endpoints", "list", "-f", "json"]);
    assert_eq!(list.status.code(), Some(0));
    assert!(
        stdout(&list).trim_start().starts_with('['),
        "{}",
        stdout(&list)
    );
}

#[test]
fn a_flag_that_would_do_nothing_is_a_usage_error() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    // a format the command does not write
    for args in [
        &["show", "-f", "csv"][..],
        &["show", "-f", "github"],
        &["check", "--max-requests", "100000", "-f", "csv"],
    ] {
        let o = sonde(t.path(), args);
        assert_eq!(o.status.code(), Some(64), "{args:?}");
        assert!(
            String::from_utf8_lossy(&o.stderr).contains("does not write -f"),
            "{args:?}"
        );
        assert!(o.stdout.is_empty(), "{args:?}");
    }
    // held to a baseline, with no baseline
    for flag in [&["--no-new-warnings"][..], &["--tolerance", "5"]] {
        let mut args = vec!["check", "--max-requests", "100000"];
        args.extend_from_slice(flag);
        let o = sonde(t.path(), &args);
        assert_eq!(o.status.code(), Some(64), "{args:?}");
        assert!(
            String::from_utf8_lossy(&o.stderr).contains("needs a baseline"),
            "{args:?}"
        );
    }
    // with one, both are taken
    assert_eq!(
        sonde(
            t.path(),
            &["check", "--set-baseline", "--max-requests", "100000"]
        )
        .status
        .code(),
        Some(0)
    );
    let o = sonde(
        t.path(),
        &[
            "check",
            "--baseline",
            "auto",
            "--no-new-warnings",
            "--tolerance",
            "5",
        ],
    );
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

#[cfg(unix)]
#[test]
fn run_exits_as_a_shell_would_when_a_signal_ends_the_program() {
    let t = tempfile::tempdir().unwrap();
    // the program sends nothing: the endpoint is never called
    let o = sonde(
        t.path(),
        &[
            "--url",
            "http://127.0.0.1:9",
            "run",
            "--",
            "sh",
            "-c",
            "kill -TERM $$",
        ],
    );
    assert_eq!(
        o.status.code(),
        Some(143),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    // the run that failed is kept as one: compared with nothing
    let s = sonde(t.path(), &["show"]);
    assert!(
        stdout(&s).contains("the run failed (exit 143); not compared"),
        "{}",
        stdout(&s)
    );
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The name of the one session of the demo.
fn demo_session(dirs: &Path) -> String {
    assert_eq!(sonde(dirs, &["demo"]).status.code(), Some(0));
    std::fs::read_dir(dirs.join("data/sessions"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.ends_with(".sonde"))
        .unwrap()
        .trim_end_matches(".sonde")
        .to_string()
}

const A_LOG_LINE: &str = "79a5 lake [06/Feb/2031:00:00:38 +0000] 192.0.2.3 - 3E57 REST.GET.OBJECT t/a \"GET /lake/t/a HTTP/1.1\" 200 - 4096 4096 12 10 \"-\" \"boto3/1.34\" - x SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -\n";

#[test]
fn every_command_takes_a_session_by_its_name_and_says_where_it_looked() {
    let t = tempfile::tempdir().unwrap();
    let name = demo_session(t.path());
    // compare by name, as show and check take one
    let c = sonde(t.path(), &["compare", &name, &name]);
    assert_eq!(c.status.code(), Some(0), "{}", stderr(&c));
    assert!(stdout(&c).contains("no change"), "{}", stdout(&c));
    // a replay opens it too (and stops at the confirmation: nothing is sent)
    let r = sonde(
        t.path(),
        &[
            "--url",
            "http://127.0.0.1:9",
            "--path-style",
            "compare",
            &name,
            "--replay",
            "adhoc",
        ],
    );
    assert_eq!(r.status.code(), Some(77), "{}", stderr(&r));
    assert!(stderr(&r).contains("Replay 28 requests"), "{}", stderr(&r));
    // a name that is no session is said, with where it was looked for
    for args in [
        vec!["compare", "nosuch", name.as_str()],
        vec![
            "compare",
            "nosuch",
            "--replay",
            "adhoc",
            "--url",
            "http://127.0.0.1:9",
        ],
        vec!["show", "nosuch"],
        vec!["check", "nosuch", "--max-requests", "1"],
        vec!["share", "--otlp", "nosuch"],
    ] {
        let o = sonde(t.path(), &args);
        assert_eq!(o.status.code(), Some(64), "{args:?}: {}", stderr(&o));
        assert!(
            stderr(&o).contains("no session nosuch") && stderr(&o).contains("sessions"),
            "{args:?}: {}",
            stderr(&o)
        );
    }
    let o = sonde(t.path(), &["import", "nosuchdir"]);
    assert_eq!(o.status.code(), Some(64));
    assert!(stderr(&o).contains("nosuchdir"), "{}", stderr(&o));
}

#[test]
fn the_newest_session_is_the_newest_recording_not_an_import() {
    let t = tempfile::tempdir().unwrap();
    let name = demo_session(t.path());
    // an import made later, and of a later time
    let log = t.path().join("later.log");
    std::fs::write(&log, A_LOG_LINE).unwrap();
    let o = sonde(t.path(), &["import", log.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let shown = sonde(t.path(), &["show", "--summary", "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(v["summary"]["requests"], 28, "the demo's, not the import's");
    let checked = sonde(t.path(), &["check", "--max-requests", "100"]);
    assert!(stdout(&checked).contains(&name), "{}", stdout(&checked));
    // the import is still a session, read by its name
    let named = sonde(t.path(), &["show", "later", "--summary", "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&named.stdout).unwrap();
    assert_eq!(v["summary"]["requests"], 1);
    // and the list says what each holds
    let list = stdout(&sonde(t.path(), &["sessions"]));
    assert!(list.contains("requests") && list.contains("took") && list.contains("errors"));
    let rows: serde_json::Value =
        serde_json::from_slice(&sonde(t.path(), &["sessions", "-f", "json"]).stdout).unwrap();
    let demo = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"].as_str().unwrap().starts_with(&name))
        .unwrap();
    assert_eq!(demo["requests"], 28);
    assert!(demo["seconds"].is_number() && demo["errors"].is_number());
}

#[test]
fn what_a_command_cannot_do_is_said_with_what_does_it() {
    let t = tempfile::tempdir().unwrap();
    // flags and commands of earlier versions are unknown, as any other
    for args in [
        vec!["--log-out", "x.har", "endpoints", "list"],
        vec!["proxy", "--no-delete"],
        vec!["proxy", "--read-only"],
        vec!["share", "--json"],
        vec!["export"],
        vec!["watch"],
        vec!["replay", "x.sonde"],
        vec!["diff", "a.sonde", "b.sonde"],
        vec!["du", "-b", "b"],
        vec!["mpu", "scan", "-b", "b"],
        vec!["bucket", "scan", "-b", "b"],
        vec!["compare", "x.sonde", "--replay", "e", "--in-place"],
    ] {
        let o = sonde(t.path(), &args);
        assert_eq!(o.status.code(), Some(64), "{args:?}: {}", stderr(&o));
        assert!(
            stderr(&o).contains("unexpected argument")
                || stderr(&o).contains("unrecognized subcommand"),
            "{args:?}: {}",
            stderr(&o)
        );
    }
    // no endpoint: how to add one, both ways
    let o = sonde(t.path(), &["run", "--", "true"]);
    let e = stderr(&o);
    assert!(
        e.contains("sonde endpoints add ID URL") && e.contains("--url URL --path-style"),
        "{e}"
    );
    // a time on the command line reads as sonde.toml's does
    demo_session(t.path());
    let o = sonde(t.path(), &["check", "--max-waiting", "2s", "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["check"]["verdicts"][0]["limit"], 2000.0, "{v}");
    let o = sonde(t.path(), &["check", "--max-p95", "250ms", "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["check"]["verdicts"][0]["limit"], 250.0);
    assert_eq!(
        sonde(t.path(), &["check", "--max-waiting", "soon"])
            .status
            .code(),
        Some(64)
    );
    // traces are a form of sharing a run
    let o = sonde(t.path(), &["share", "--otlp"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).contains("resourceSpans"), "{}", stdout(&o));
    // a session without a failure says so, when only failures are asked for
    let o = sonde(t.path(), &["show", "--errors", "--requests"]);
    let shown = stdout(&o);
    if !shown.contains(" 5") && !shown.contains("ERR") {
        assert!(shown.contains("no errors"), "{shown}");
    }
}

#[test]
fn a_clean_session_asked_for_its_errors_says_there_are_none() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("clean.log");
    std::fs::write(&log, A_LOG_LINE).unwrap();
    let out = t.path().join("clean.sonde");
    let o = sonde(
        t.path(),
        &["import", log.to_str().unwrap(), "-o", out.to_str().unwrap()],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    // (grammar: one of each)
    assert!(
        stdout(&o).contains("1 request (1 operation)"),
        "{}",
        stdout(&o)
    );
    for flags in [vec!["--errors"], vec!["--errors", "--requests"]] {
        let mut args = vec!["show", out.to_str().unwrap()];
        args.extend(flags);
        let shown = stdout(&sonde(t.path(), &args));
        assert!(shown.contains("no errors: none of its"), "{shown}");
    }
}

#[test]
fn a_commands_help_lists_the_formats_it_writes_and_the_options_it_uses() {
    let t = tempfile::tempdir().unwrap();
    let help = |cmd: &[&str]| {
        let mut args = cmd.to_vec();
        args.push("--help");
        stdout(&sonde(t.path(), &args))
    };
    let show = help(&["show"]);
    assert!(show.contains("The format written: table or json"), "{show}");
    assert!(!show.contains("csv") && !show.contains("github"), "{show}");
    assert!(help(&["check"]).contains("markdown") && help(&["check"]).contains("github"));
    assert!(help(&["bucket", "size"]).contains("table, json or csv"));
    // the options of every command under their own heading
    let run = help(&["run"]);
    let at = |text: &str, what: &str| {
        text.find(what)
            .unwrap_or_else(|| panic!("no {what}: {text}"))
    };
    assert!(
        at(&run, "Global options:") < at(&run, "--url <URL>"),
        "{run}"
    );
    assert!(at(&run, "--trial") < at(&run, "Global options:"), "{run}");
    // and a command that reads a file is not offered a store
    for cmd in ["show", "sessions", "check", "share"] {
        let h = help(&[cmd]);
        assert!(h.contains("--data-dir"), "{cmd}: {h}");
        assert!(!h.contains("--url") && !h.contains("--rate"), "{cmd}: {h}");
    }
    // (still taken where it was always taken: it is hidden, not gone)
    assert_eq!(
        sonde(t.path(), &["sessions", "--region", "eu-west-1"])
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn check_init_leaves_the_limits_of_time_for_a_person_to_set() {
    let t = tempfile::tempdir().unwrap();
    demo_session(t.path());
    let cfg = t.path().join("sonde.toml");
    let c = sonde(
        t.path(),
        &["check", "--init", "--config", cfg.to_str().unwrap()],
    );
    assert_eq!(c.status.code(), Some(0), "{}", stderr(&c));
    let text = std::fs::read_to_string(&cfg).unwrap();
    let waiting = text
        .lines()
        .find(|l| l.contains("max-waiting"))
        .expect("the line is there");
    assert!(waiting.starts_with("# max-waiting = "), "{waiting}");
    assert!(waiting.contains("measured"), "{waiting}");
    assert!(
        text.lines().any(|l| l.starts_with("# max-p95 = ")),
        "{text}"
    );
    assert!(
        text.lines().any(|l| l.starts_with("max-requests = ")),
        "{text}"
    );
    // the file it wrote is one sonde reads
    let c = sonde(t.path(), &["check", "--config", cfg.to_str().unwrap()]);
    assert_eq!(c.status.code(), Some(0), "{}{}", stdout(&c), stderr(&c));
    // and one it does not read is a configuration error, as documented
    std::fs::write(&cfg, "[check]\nmax-request = 3\n").unwrap();
    let c = sonde(t.path(), &["check", "--config", cfg.to_str().unwrap()]);
    assert_eq!(c.status.code(), Some(64), "{}", stderr(&c));
    assert!(stderr(&c).contains("sonde.toml"), "{}", stderr(&c));
}

/// The demo's session, and where it is.
fn demo_file(dirs: &Path) -> std::path::PathBuf {
    let name = demo_session(dirs);
    dirs.join("data/sessions").join(format!("{name}.sonde"))
}

/// A recorded session of `command` in the sessions folder, made of a
/// fixture's requests.
fn session_of(dirs: &Path, name: &str, command: &str, created: &str) -> std::path::PathBuf {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../proxy/tests/fixtures/sdk-python.sonde");
    let text = std::fs::read_to_string(fixture).unwrap();
    let (header, records) = text.split_once('\n').unwrap();
    let mut header: serde_json::Value = serde_json::from_str(header).unwrap();
    header["command"] = command.into();
    header["created"] = created.into();
    let dir = dirs.join("data/sessions");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    std::fs::write(&file, format!("{header}\n{records}")).unwrap();
    file
}

/// `sonde check | head`: the reader gone before a line is written, the
/// command still ends with its own exit code, not 0.
#[cfg(unix)]
#[test]
fn an_exit_code_survives_a_closed_pipe() {
    let t = tempfile::tempdir().unwrap();
    demo_file(t.path());
    let closed = |args: &[&str]| {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let o = Command::new(env!("CARGO_BIN_EXE_sonde"))
            .args(args)
            .env("SONDE_CONFIG_DIR", t.path().join("config"))
            .env("SONDE_DATA_DIR", t.path().join("data"))
            .stdout(writer)
            .stderr(std::process::Stdio::piped())
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        assert!(!err.contains("panicked"), "{err}");
        o.status.code()
    };
    for _ in 0..3 {
        assert_eq!(closed(&["check", "--max-requests", "10"]), Some(1));
        assert_eq!(
            closed(&["check", "--max-requests", "10", "-f", "json"]),
            Some(1)
        );
        assert_eq!(closed(&["show", "--all"]), Some(0));
        assert_eq!(closed(&["show", "-f", "json"]), Some(0));
    }
}

#[test]
fn sessions_are_listed_from_what_was_counted_in_the_folders_index() {
    let t = tempfile::tempdir().unwrap();
    let file = demo_file(t.path());
    // counted when it was recorded: in the folder's one index, no file beside it
    // (a program's own state, one file per folder, is no session's)
    let counted = || sonde_proxy::index::get(&file).counted.is_some();
    assert!(counted());
    let per_program = [
        "findings.json",
        "verdicts.json",
        "written.json",
        "baselines.json",
    ];
    let beside: Vec<String> = std::fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.') && !n.ends_with(".sonde"))
        .filter(|n| !per_program.contains(&n.as_str()))
        .collect();
    assert!(beside.is_empty(), "{beside:?}");
    let listed: serde_json::Value =
        serde_json::from_str(&stdout(&sonde(t.path(), &["sessions", "-f", "json"]))).unwrap();
    assert_eq!(listed[0]["requests"], 28, "{listed}");
    assert_eq!(listed[0]["program"], "sonde demo");
    assert!(listed[0]["seconds"].as_f64().unwrap() > 0.0);
    // one that is missing is made by the listing
    sonde_proxy::index::set(&file, sonde_proxy::index::Change::NoCounted);
    assert!(!counted());
    assert!(stdout(&sonde(t.path(), &["sessions"])).contains("sonde demo"));
    assert!(counted());
    // nothing past the newest five: said, not silence
    let kept = sonde(t.path(), &["sessions", "prune", "--keep", "5", "--yes"]);
    assert!(
        stdout(&kept).contains("nothing to remove: 1 session here"),
        "{}",
        stdout(&kept)
    );
    assert!(file.is_file());
    // a dry run that would delete: 4, and nothing deleted
    let dry = sonde(t.path(), &["sessions", "prune", "--keep", "0"]);
    assert_eq!(dry.status.code(), Some(4), "{}", stdout(&dry));
    assert!(file.is_file());
    assert_eq!(kept.status.code(), Some(0));
    // removed, and what was counted of it with it
    let gone = sonde(t.path(), &["sessions", "prune", "--keep", "0", "--yes"]);
    assert!(stdout(&gone).contains("deleted "), "{}", stdout(&gone));
    assert!(!file.exists() && !counted());
}

#[test]
fn a_session_with_a_line_no_record_could_be_is_bad_data_to_every_reader() {
    let t = tempfile::tempdir().unwrap();
    let file = session_of(
        t.path(),
        "bad.sonde",
        "python etl.py",
        "2026-11-01T10:00:00Z",
    );
    let text = std::fs::read_to_string(&file).unwrap();
    let (header, records) = text.split_once('\n').unwrap();
    std::fs::write(&file, format!("{header}\n{{\"id\": \"x\"}}\n{records}")).unwrap();
    // show refuses it as bad data, not as an operation that failed
    let shown = sonde(t.path(), &["show", "bad"]);
    assert_eq!(shown.status.code(), Some(65), "{}", stderr(&shown));
    assert!(stderr(&shown).contains("line 2: "), "{}", stderr(&shown));
    for args in [
        &["check", "bad", "--max-requests", "100"][..],
        &["compare", "bad", "bad"],
    ] {
        assert_eq!(sonde(t.path(), args).status.code(), Some(65), "{args:?}");
    }
    // and the list says so, rather than listing it as a session that reads
    let listed = sonde(t.path(), &["sessions", "-f", "json"]);
    let rows: serde_json::Value = serde_json::from_str(&stdout(&listed)).unwrap();
    assert!(
        rows[0]["unreadable"]
            .as_str()
            .unwrap()
            .starts_with("line 2: "),
        "{rows}"
    );
    let table = sonde(t.path(), &["sessions"]);
    assert!(
        stderr(&table).contains("bad.sonde is not a session: line 2: "),
        "{}",
        stderr(&table)
    );
}

#[test]
fn compare_says_the_kinds_it_leaves_out_and_waiting_as_show_does() {
    let t = tempfile::tempdir().unwrap();
    session_of(t.path(), "a.sonde", "python etl.py", "2026-11-01T10:00:00Z");
    let b = session_of(t.path(), "b.sonde", "python etl.py", "2026-11-01T11:00:00Z");
    // the second run made one upload part fewer
    let text = std::fs::read_to_string(&b).unwrap();
    let mut lines: Vec<&str> = text.lines().collect();
    let part = lines
        .iter()
        .position(|l| l.contains("\"UploadPart\""))
        .unwrap();
    lines.remove(part);
    std::fs::write(&b, lines.join("\n") + "\n").unwrap();
    let out = sonde(t.path(), &["compare", "a", "b"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("UploadPart"), "{text}");
    // ten kinds, one changed: the nine others are said, not dropped
    assert!(
        text.contains("… and 9 more kinds of request, unchanged"),
        "{text}"
    );
    // a kind's waiting is its share of the run's, as show's table says it
    let json: serde_json::Value = serde_json::from_str(&stdout(&sonde(
        t.path(),
        &["compare", "a", "b", "-f", "json"],
    )))
    .unwrap();
    let shown: serde_json::Value = serde_json::from_str(&stdout(&sonde(
        t.path(),
        &["show", "a", "--summary", "-f", "json"],
    )))
    .unwrap();
    let kinds = shown["waiting"]["byKind"].as_array().unwrap();
    for g in json["groups"].as_array().unwrap() {
        let s = kinds
            .iter()
            .find(|k| k["kind"] == g["kind"] && k["missed"] == g["missed"])
            .unwrap();
        assert_eq!(g["waitingMs"][0], s["waitedMs"], "{g} {s}");
    }
}

#[test]
fn a_compressed_session_cut_short_is_read_up_to_its_cut() {
    use std::io::Write;
    let t = tempfile::tempdir().unwrap();
    let file = session_of(
        t.path(),
        "cut.sonde",
        "python etl.py",
        "2026-11-01T10:00:00Z",
    );
    let text = std::fs::read(&file).unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&text).unwrap();
    let whole = gz.finish().unwrap();
    std::fs::write(&file, &whole[..whole.len() * 3 / 4]).unwrap();
    let shown = sonde(t.path(), &["show", "cut", "--summary"]);
    assert_eq!(shown.status.code(), Some(0), "{}", stderr(&shown));
    assert!(
        stderr(&shown).contains("its compressed text stops short"),
        "{}",
        stderr(&shown)
    );
}

#[test]
fn a_long_command_is_cut_in_its_middle_to_fit_the_terminal() {
    let t = tempfile::tempdir().unwrap();
    let long = format!(
        "python /srv/jobs/etl/load.py --table warehouse.events {}--day 2026-11-10",
        "--partition p ".repeat(12)
    );
    session_of(t.path(), "long.sonde", &long, "2026-11-10T02:00:00Z");
    let narrow = Command::new(env!("CARGO_BIN_EXE_sonde"))
        .arg("sessions")
        .env("SONDE_CONFIG_DIR", t.path().join("config"))
        .env("SONDE_DATA_DIR", t.path().join("data"))
        .env("COLUMNS", "110")
        .output()
        .unwrap();
    let table = stdout(&narrow);
    let row = table.lines().find(|l| l.starts_with("long.sonde")).unwrap();
    assert!(
        row.chars().count() <= 110,
        "{} columns:\n{table}",
        row.chars().count()
    );
    // its start and its end: the arguments tell two runs apart
    assert!(
        row.contains("python /srv") && row.contains("…") && row.contains("2026-11-10"),
        "{row}"
    );
    // not a terminal, and no width given: whole (and whole as JSON always)
    let piped = Command::new(env!("CARGO_BIN_EXE_sonde"))
        .arg("sessions")
        .env("SONDE_CONFIG_DIR", t.path().join("config"))
        .env("SONDE_DATA_DIR", t.path().join("data"))
        .env_remove("COLUMNS")
        .output()
        .unwrap();
    assert!(stdout(&piped).contains(&long), "{}", stdout(&piped));
}

#[test]
fn a_trials_session_is_found_by_its_name() {
    let t = tempfile::tempdir().unwrap();
    let file = demo_file(t.path());
    let trials = file.with_file_name("trials");
    std::fs::create_dir_all(&trials).unwrap();
    std::fs::copy(&file, trials.join("run.slowdown.sonde")).unwrap();
    for name in [
        "run.slowdown",
        "run.slowdown.sonde",
        "trials/run.slowdown.sonde",
    ] {
        let o = sonde(t.path(), &["show", name, "--summary"]);
        assert_eq!(o.status.code(), Some(0), "{name}: {}", stderr(&o));
        assert!(stdout(&o).contains("28 requests"), "{name}");
    }
}

#[test]
fn show_writes_only_what_failed_as_json_too_and_the_ends_of_a_long_session() {
    let t = tempfile::tempdir().unwrap();
    // 2,400 requests a second apart, every hundredth refused
    let mut log = String::new();
    for i in 0..2400 {
        let (status, code) = if i % 100 == 7 {
            ("403", "AccessDenied")
        } else {
            ("200", "-")
        };
        log.push_str(&format!(
            "79a5 lake [06/Feb/2031:{:02}:{:02}:{:02} +0000] 192.0.2.3 - ID{i} REST.PUT.OBJECT t/k{i} \"PUT /lake/t/k{i} HTTP/1.1\" {status} {code} - 4096 12 10 \"-\" \"boto3/1.34\" - x SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -\n",
            i / 3600,
            i / 60 % 60,
            i % 60
        ));
    }
    let file = t.path().join("day.log");
    std::fs::write(&file, log).unwrap();
    let out = t.path().join("day.sonde");
    let o = sonde(
        t.path(),
        &[
            "import",
            file.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let out = out.to_str().unwrap();
    // only what failed, in every format
    let failed: serde_json::Value = serde_json::from_str(&stdout(&sonde(
        t.path(),
        &["show", out, "--errors", "-f", "json"],
    )))
    .unwrap();
    assert_eq!(failed["requests"].as_array().unwrap().len(), 24);
    assert_eq!(failed["operations"].as_array().unwrap().len(), 24);
    assert_eq!(
        failed["summary"]["requests"], 2400,
        "the summary is of the session"
    );
    let all: serde_json::Value =
        serde_json::from_str(&stdout(&sonde(t.path(), &["show", out, "-f", "json"]))).unwrap();
    assert_eq!(all["requests"].as_array().unwrap().len(), 2400);
    assert_eq!(all["requests"][7]["status"], 403);
    // a long session: its first and last hundred, and how many are between
    let shown = stdout(&sonde(t.path(), &["show", out, "--requests"]));
    assert!(
        shown.contains("… 2,200 requests between these: `--all` prints them"),
        "{}",
        &shown[..shown.len().min(2000)]
    );
    let has = |k: &str| {
        shown
            .lines()
            .any(|l| l.split_whitespace().any(|w| w.ends_with(k)))
    };
    assert!(has("t/k99") && has("t/k2300") && !has("t/k1200"), "{shown}");
    // the summary is always there, each figure once
    assert_eq!(shown.matches("2,400 requests").count(), 1, "{shown}");
    assert!(shown.contains("PutObject"));
    let whole = stdout(&sonde(t.path(), &["show", out, "--all"]));
    assert!(whole.contains("t/k1200 ") && !whole.contains("between these"));
    // its times are a log's, to the second: said, with what is not known
    assert!(shown.contains("its times are to the second"), "{shown}");
    assert!(!shown.contains("at once at most"), "{shown}");
}

#[test]
fn a_missing_baseline_names_the_program_and_the_one_its_other_arguments_have() {
    let t = tempfile::tempdir().unwrap();
    let monday = session_of(
        t.path(),
        "monday.sonde",
        "python etl.py --day 1",
        "2026-11-09T02:00:00Z",
    );
    let tuesday = session_of(
        t.path(),
        "tuesday.sonde",
        "python etl.py --day 2",
        "2026-11-10T02:00:00Z",
    );
    let set = sonde(
        t.path(),
        &["check", monday.to_str().unwrap(), "--set-baseline"],
    );
    assert_eq!(set.status.code(), Some(0), "{}", stderr(&set));
    // the next day's run is another command: no baseline of its own, and the
    // budget is what decides
    let c = sonde(
        t.path(),
        &[
            "check",
            tuesday.to_str().unwrap(),
            "--baseline",
            "auto",
            "--max-requests",
            "100",
            "-f",
            "json",
        ],
    );
    let err = stderr(&c);
    assert_eq!(c.status.code(), Some(0), "{err}");
    assert!(
        err.contains("no baseline yet for program `python etl.py --day 2`"),
        "{err}"
    );
    assert!(
        err.contains("there is one for `python etl.py --day 1`")
            && err.contains("name = \"etl\"")
            && err.contains("sonde run --name etl"),
        "{err}"
    );
    let doc: serde_json::Value = serde_json::from_str(&stdout(&c)).unwrap();
    assert_eq!(doc["passed"], true);
    assert_eq!(doc["verdict"], "within budget (no baseline yet)");
    assert!(
        doc["noBaseline"]
            .as_str()
            .unwrap()
            .contains("no baseline yet")
    );
    // held to it by name: past it is "past its baseline", not "over budget"
    let c = sonde(
        t.path(),
        &[
            "check",
            tuesday.to_str().unwrap(),
            "--baseline",
            monday.to_str().unwrap(),
            "--tolerance",
            "0",
            "-f",
            "github",
        ],
    );
    assert!(
        stdout(&c).contains("sonde: within its baseline"),
        "{}",
        stdout(&c)
    );
    // and `check --init` gives the program the name its runs then share
    let cfg = t.path().join("sonde.toml");
    let init = sonde(
        t.path(),
        &[
            "check",
            tuesday.to_str().unwrap(),
            "--init",
            "--config",
            cfg.to_str().unwrap(),
        ],
    );
    assert_eq!(init.status.code(), Some(0), "{}", stderr(&init));
    assert!(
        stderr(&init).contains("[program] name = \"etl\""),
        "{}",
        stderr(&init)
    );
    assert!(
        std::fs::read_to_string(&cfg)
            .unwrap()
            .contains("[program]\nname = \"etl\"\n")
    );
}

#[test]
fn a_check_of_trials_alone_has_no_failed_budget_inside_it() {
    let t = tempfile::tempdir().unwrap();
    let file = session_of(
        t.path(),
        "run.sonde",
        "python etl.py",
        "2026-11-09T02:00:00Z",
    );
    // a verdict kept in the folder's index
    let broke: Vec<sonde_proxy::trial::Trial> = serde_json::from_str(
        r#"[{"scenario":"slowdown","about":"503 SlowDown","standing":"broke","exit":1,"cleanExit":0,
            "faults":2,"retried":0,"droppedReads":0,"droppedWrites":2,"attempts":1,"requests":10,
            "cleanRequests":13,"leftUploads":0,"wroteTwice":0,"reasons":["exited 1"]}]"#,
    )
    .unwrap();
    sonde_proxy::trial::save(&file, &broke).unwrap();
    let c = sonde(t.path(), &["check", file.to_str().unwrap(), "-f", "json"]);
    assert_eq!(c.status.code(), Some(1), "{}{}", stdout(&c), stderr(&c));
    let doc: serde_json::Value = serde_json::from_str(&stdout(&c)).unwrap();
    assert_eq!(doc["passed"], false, "{doc}");
    assert_eq!(doc["check"]["verdicts"], serde_json::json!([]));
    assert_eq!(
        doc["check"]["passed"], true,
        "nothing was checked: nothing failed there"
    );
}

#[cfg(unix)]
#[test]
fn spark_submit_is_given_s3as_endpoint_and_tls_its_certificate() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    // a spark-submit that says what it was given
    let bin = t.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let said = t.path().join("said.txt");
    let fake = bin.join("spark-submit");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do echo \"arg $a\"; done > {0}\n\
             echo \"url $AWS_ENDPOINT_URL\" >> {0}\necho \"ca $AWS_CA_BUNDLE\" >> {0}\n\
             echo \"py $PYSPARK_SUBMIT_ARGS\" >> {0}\n",
            said.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let o = sonde(
        t.path(),
        &[
            "--url",
            "http://127.0.0.1:9",
            "run",
            "--tls",
            "--",
            fake.to_str().unwrap(),
            "--master",
            "local[2]",
            "job.py",
        ],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stderr(&o).contains("is given its endpoint with --conf spark.hadoop.fs.s3a.*"));
    let said = std::fs::read_to_string(&said).unwrap();
    let args: Vec<&str> = said
        .lines()
        .filter_map(|l| l.strip_prefix("arg "))
        .collect();
    // sonde's settings first, the command's own after them, as it gave them
    assert_eq!(args[0], "--conf");
    assert!(
        args[1].starts_with("spark.hadoop.fs.s3a.endpoint=http://127.0.0.1:"),
        "{said}"
    );
    assert!(args.contains(&"spark.hadoop.fs.s3a.path.style.access=true"));
    assert_eq!(&args[args.len() - 3..], ["--master", "local[2]", "job.py"]);
    assert!(!said.contains("secret.key"), "no key on a command line");
    // the SDKs get TLS and a bundle; the JVM the plain endpoint beside it
    assert!(said.contains("url https://127.0.0.1:"), "{said}");
    let ca = said.lines().find_map(|l| l.strip_prefix("ca ")).unwrap();
    assert!(
        std::fs::read_to_string(ca)
            .unwrap()
            .contains("BEGIN CERTIFICATE")
    );
    assert!(said.contains("py --conf spark.hadoop.fs.s3a.endpoint=http://"));
    let key = t.path().join("data/tls/key.pem");
    assert_eq!(
        std::fs::metadata(key).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn the_help_lists_fourteen_commands_at_most() {
    let t = tempfile::tempdir().unwrap();
    let help = stdout(&sonde(t.path(), &["--help"]));
    let listed: Vec<&str> = help
        .lines()
        .filter(|l| l.starts_with("  ") && !l.trim_start().starts_with('-'))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|n| *n != "help")
        .collect();
    assert!(listed.len() <= 14, "{listed:?}");
}

/// The fix loop on recorded runs of one boto3 program: `show` says each
/// finding's saving, line and fix, and the change since the run before;
/// `compare` alone takes the last two runs.
#[test]
fn a_fix_is_confirmed_with_what_it_saved_and_an_undone_one_is_back() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("data/sessions");
    std::fs::create_dir_all(&dir).unwrap();
    let runs = [
        "etl-1-base",
        "etl-2-again",
        "etl-3-no-head",
        "etl-4-no-small",
        "etl-5-undone",
    ];
    for r in runs {
        let from = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../proxy/tests/fixtures")
            .join(format!("{r}.sonde"));
        std::fs::copy(from, dir.join(format!("{r}.sonde"))).unwrap();
    }
    let shown: Vec<String> = runs
        .iter()
        .map(|r| stdout(&sonde(t.path(), &["show", r])))
        .collect();
    // the head line names the SDK; each finding its saving, line and fix
    let first = &shown[0];
    assert!(
        first.contains("91 requests (73 operations) from boto3 1.44 to moto"),
        "{first}"
    );
    for said in [
        "    saving: 20 fewer requests (21% of the session)",
        "    at etl.py:19 <module> (20 requests)",
        "    fix for boto3:",
        "      obj = s3.get_object(Bucket=b, Key=k)",
        "    at etl.py:33 <module> (18 requests)",
    ] {
        assert!(first.contains(said), "no {said:?} in:\n{first}");
    }
    // findings first, then one table: no other table by default
    assert!(first.find("saving:").unwrap() < first.find("kind ").unwrap());
    assert!(
        !first.contains("first byte") && !first.contains("waiting on"),
        "{first}"
    );
    assert!(!first.contains("since the run before"), "{first}");
    assert!(shown[1].contains("since the run before (etl-1-base)"));
    let third = &shown[2];
    assert!(
        third.contains("✓ fixed  20 objects were checked with HEAD and then read with GET: the GET alone answers both · saved 20 requests (predicted 20)"),
        "{third}"
    );
    assert!(third.contains("  requests 91 → 71 (-22%)"), "{third}");
    assert!(
        shown[3].contains("✓ fixed  6 files under 5 MiB"),
        "{}",
        shown[3]
    );
    assert!(
        shown[4].contains("! back   20 objects were checked"),
        "{}",
        shown[4]
    );

    // compare alone: the last two runs, an undone fix back
    let c = stdout(&sonde(t.path(), &["compare"]));
    assert!(
        c.contains("etl-4-no-small.sonde → etl-5-undone.sonde"),
        "{c}"
    );
    assert!(c.contains("! back   6 files under 5 MiB"), "{c}");
    // one run: it and the run before it
    let c = stdout(&sonde(t.path(), &["compare", "etl-3-no-head"]));
    assert!(c.contains("etl-2-again.sonde → etl-3-no-head.sonde"), "{c}");
    assert!(c.contains("✓ fixed  20 objects"), "{c}");
    let j: serde_json::Value = serde_json::from_str(&stdout(&sonde(
        t.path(),
        &["compare", "etl-3-no-head", "-f", "json"],
    )))
    .unwrap();
    let fixed = &j["findings"][0];
    assert_eq!(fixed["state"], "fixed", "{j}");
    assert_eq!(fixed["predicted"]["requests"], 20);
    assert_eq!(fixed["measured"]["requests"], 20);
    // a program's first run has nothing before it
    let none = sonde(t.path(), &["compare", "etl-1-base"]);
    assert_eq!(none.status.code(), Some(64));
    assert!(
        stderr(&none).contains("the first run of `etl`"),
        "{}",
        stderr(&none)
    );
    // show says in JSON what it says in text: the change since the run before
    let j: serde_json::Value = serde_json::from_str(&stdout(&sonde(
        t.path(),
        &["show", "etl-3-no-head", "-f", "json"],
    )))
    .unwrap();
    assert_eq!(
        j["since"]["findings"][0]["state"], "fixed",
        "{}",
        j["since"]
    );
    let j: serde_json::Value = serde_json::from_str(&stdout(&sonde(
        t.path(),
        &["show", "etl-3-no-head", "--summary", "-f", "json"],
    )))
    .unwrap();
    assert_eq!(
        j["since"]["findings"][0]["state"], "fixed",
        "{}",
        j["since"]
    );

    // a run that crashed is compared with nothing, and the next with the run before it
    let mut fixture = String::new();
    std::io::Read::read_to_string(
        &mut flate2::read::MultiGzDecoder::new(
            std::fs::File::open(dir.join("etl-3-no-head.sonde")).unwrap(),
        ),
        &mut fixture,
    )
    .unwrap();
    let (header, records) = fixture.split_once('\n').unwrap();
    let mut header: serde_json::Value = serde_json::from_str(header).unwrap();
    header["created"] = "2030-01-01T00:00:00Z".into();
    let crashed = dir.join("etl-6-crashed.sonde");
    std::fs::write(&crashed, format!("{header}\n{records}")).unwrap();
    sonde_proxy::index::set(&crashed, sonde_proxy::index::Change::Exit(3));
    let s = stdout(&sonde(t.path(), &["show", "etl-6-crashed"]));
    assert!(s.contains("the run failed (exit 3); not compared"), "{s}");
    assert!(!s.contains("fixed"), "{s}");
    let c = stdout(&sonde(
        t.path(),
        &["compare", "etl-5-undone", "etl-6-crashed"],
    ));
    assert!(
        c.contains("the run failed (exit 3); its findings are not compared"),
        "{c}"
    );
    assert!(!c.contains("✓ fixed"), "{c}");
}

/// `check --init` names a program: what was kept under its command goes
/// with it, its baseline and its trials' verdicts.
#[test]
fn check_init_carries_the_baseline_and_verdicts_to_the_programs_name() {
    let t = tempfile::tempdir().unwrap();
    let run = session_of(
        t.path(),
        "run.sonde",
        "python etl.py",
        "2026-11-09T02:00:00Z",
    );
    let set = sonde(
        t.path(),
        &["check", run.to_str().unwrap(), "--set-baseline"],
    );
    assert_eq!(set.status.code(), Some(0), "{}", stderr(&set));
    let dir = t.path().join("data/sessions");
    std::fs::write(
        dir.join("verdicts.json"),
        r#"{"python etl.py":{"slowdown":[{"at":"2026-11-09T02:00:00Z","standing":"broke","run":"run.sonde"}]}}"#,
    )
    .unwrap();
    let cfg = t.path().join("sonde.toml");
    let init = sonde(
        t.path(),
        &[
            "check",
            run.to_str().unwrap(),
            "--init",
            "--config",
            cfg.to_str().unwrap(),
        ],
    );
    assert_eq!(init.status.code(), Some(0), "{}", stderr(&init));
    assert!(
        stderr(&init).contains(
            "what was kept for `python etl.py` (baselines.json, verdicts.json) now goes by \"etl\""
        ),
        "{}",
        stderr(&init)
    );
    let read = |f: &str| -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(dir.join(f)).unwrap()).unwrap()
    };
    assert_eq!(
        read("baselines.json"),
        serde_json::json!({"etl": "run.sonde"})
    );
    assert_eq!(
        read("verdicts.json")["etl"]["slowdown"][0]["standing"],
        "broke"
    );
    assert!(read("verdicts.json").get("python etl.py").is_none());
}

/// `[check] expect` names findings by identity: words of a finding's text,
/// or a kind sonde does not find, are a mistake in sonde.toml.
#[test]
fn an_expect_that_is_no_findings_identity_is_a_usage_error() {
    let t = tempfile::tempdir().unwrap();
    let run = session_of(
        t.path(),
        "run.sonde",
        "python etl.py",
        "2026-11-09T02:00:00Z",
    );
    let cfg = t.path().join("sonde.toml");
    for (expect, ok) in [
        ("obs.hot", true),
        ("obs.hot|b/jobs/1794558464595/x", true),
        ("read again", false),
        ("obs.retried", false),
    ] {
        std::fs::write(
            &cfg,
            format!("[check]\nmax-requests = 100000\nexpect = [{expect:?}]\n"),
        )
        .unwrap();
        let c = sonde(
            t.path(),
            &[
                "check",
                run.to_str().unwrap(),
                "--config",
                cfg.to_str().unwrap(),
            ],
        );
        if ok {
            assert_eq!(c.status.code(), Some(0), "{expect}: {}", stderr(&c));
        } else {
            assert_eq!(c.status.code(), Some(64), "{expect}");
            assert!(
                stderr(&c).contains("is not a finding's identity"),
                "{}",
                stderr(&c)
            );
        }
    }
}

#[test]
fn an_import_says_what_it_left_out_and_no_time_it_does_not_have() {
    let t = tempfile::tempdir().unwrap();
    // CloudTrail, one event a line: a cut line and another service's event
    let trail = t.path().join("trail.json");
    std::fs::write(
        &trail,
        [
            r#"{"eventTime":"2031-02-06T00:00:01Z","eventSource":"s3.amazonaws.com","eventName":"GetObject","requestParameters":{"bucketName":"lake","key":"a"}}"#,
            r#"{"eventTime":"2031-02-06T00:00:02Z","eventSource":"ec2.amazonaws.com","eventName":"RunInstances"}"#,
            r#"{"eventTime":"2031-02-06T00:00:03Z","eventSource":"s3.amaz"#,
        ]
        .join("\n"),
    )
    .unwrap();
    let o = sonde(t.path(), &["import", trail.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let err = stderr(&o);
    assert!(err.contains("1 line could not be read"), "{err}");
    assert!(err.contains("1 event left out"), "{err}");
    // no latency in CloudTrail: no p95, no waiting
    let table = stdout(&o);
    let row = table.lines().find(|l| l.contains("GetObject")).unwrap();
    assert!(row.contains('—') && !row.contains(" ms"), "{table}");
    // an access log to the second: its waiting would be sonde's spreading
    let log = t.path().join("two.log");
    std::fs::write(&log, A_LOG_LINE.repeat(2)).unwrap();
    let o = sonde(t.path(), &["import", log.to_str().unwrap()]);
    let shown = stdout(&o);
    assert!(!shown.contains(", waiting"), "{shown}");
    assert!(shown.contains("so the waiting, is not known"), "{shown}");
}

#[test]
fn a_layout_asked_for_by_kind_and_not_there_is_said() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("files");
    std::fs::create_dir_all(d.join("logs")).unwrap();
    std::fs::write(d.join("logs/a.txt"), "x").unwrap();
    let dir = d.to_str().unwrap();
    let o = sonde(
        t.path(),
        &["layout", "inspect", "--dir", dir, "--kind", "slatedb"],
    );
    assert_eq!(o.status.code(), Some(4), "{}", stderr(&o));
    assert!(
        stderr(&o).contains("is not a SlateDB layout"),
        "{}",
        stderr(&o)
    );
    // any prefix has a write cadence
    let o = sonde(
        t.path(),
        &["layout", "inspect", "--dir", dir, "--kind", "generic"],
    );
    assert_ne!(o.status.code(), Some(64), "{}", stderr(&o));
    assert!(!stderr(&o).contains("is not a"), "{}", stderr(&o));
}

#[test]
fn an_endpoint_id_taken_is_refused_and_test_says_how_to_name_one() {
    let t = tempfile::tempdir().unwrap();
    let add = |id: &str, more: &[&str]| {
        let mut args = vec!["endpoints", "add", id, "http://127.0.0.1:9"];
        args.extend(more);
        sonde(t.path(), &args)
    };
    assert_eq!(add("a", &[]).status.code(), Some(0));
    let again = add("a", &[]);
    assert_eq!(again.status.code(), Some(64));
    assert!(stderr(&again).contains("--replace"), "{}", stderr(&again));
    assert_eq!(add("a", &["--replace"]).status.code(), Some(0));
    assert_eq!(add("b", &[]).status.code(), Some(0));
    // two: named by its argument, not --endpoint
    let o = sonde(t.path(), &["endpoints", "test"]);
    assert_eq!(o.status.code(), Some(64));
    assert!(
        stderr(&o).contains("`sonde endpoints test ID` (a, b)"),
        "{}",
        stderr(&o)
    );
    let o = sonde(t.path(), &["endpoints", "test", "c"]);
    assert_eq!(o.status.code(), Some(64));
    assert!(
        stderr(&o).contains("the configured ones are a, b"),
        "{}",
        stderr(&o)
    );
}
