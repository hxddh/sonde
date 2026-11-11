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
        ("Read a session:", "\n  sessions ", "\n  export "),
        ("Replay on another store:", "\n  replay ", "\n  diff "),
        ("The store itself:", "\n  probe ", "\n  endpoints "),
    ] {
        assert!(at(heading) < at(first) && at(first) < at(last), "{help}");
    }
    assert!(at("\n  import ") < at("Read a session:"), "{help}");
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
    assert_eq!(v["records"].as_array().unwrap().len(), 28);
    assert!(v["observations"].as_array().unwrap().len() >= 2, "{v}");

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

    // the same run against itself is no session and its replay: diff says
    // so and names the command that compares two runs, which says "no change"
    let path = t.path().join("data/sessions").join(&name);
    let p = path.to_str().unwrap();
    for format in ["table", "json"] {
        let diff = sonde(t.path(), &["diff", p, p, "-f", format]);
        let err = String::from_utf8_lossy(&diff.stderr).into_owned();
        assert_eq!(diff.status.code(), Some(64), "{err}");
        assert!(diff.stdout.is_empty(), "{}", stdout(&diff));
        assert!(
            err.contains("not a session and its replay") && err.contains("sonde compare "),
            "{err}"
        );
    }
    let same = stdout(&sonde(t.path(), &["compare", p, p]));
    assert!(same.starts_with("no change"), "{same}");
    assert!(same.contains("requests   28 (no change)"), "{same}");
    assert!(!same.contains("0%"), "{same}");
}

#[test]
fn the_demo_reads_as_operations_not_as_a_list_of_requests() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["demo"]).status.code(), Some(0));
    let out = stdout(&sonde(t.path(), &["show"]));
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
        "1 request repeated after a retryable failure",
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
    assert!(
        md.contains("| **FAIL** +2700% | requests | 28 | ≤ 1 |"),
        "{md}"
    );
    assert!(md.contains("| pass | waiting |"), "{md}");
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
        err.contains("1 request from a S3 server access log"),
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
    assert!(shown.contains("to lake-logs"), "{shown}");

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
        &["du", "-b", "b", "-f", "markdown"][..],
        &["mpu", "scan", "-b", "b", "-f", "github"],
        &["mpu", "abort", "-b", "b", "-f", "json"],
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
    // compare and diff by name, as show and check take one
    let c = sonde(t.path(), &["compare", &name, &name]);
    assert_eq!(c.status.code(), Some(0), "{}", stderr(&c));
    assert!(stdout(&c).contains("no change"), "{}", stdout(&c));
    let d = sonde(t.path(), &["diff", &name, &name]);
    assert_eq!(d.status.code(), Some(64), "two runs are not a replay");
    assert!(stderr(&d).contains("sonde compare"), "{}", stderr(&d));
    // replay opens it too (and stops at the confirmation: nothing is sent)
    let r = sonde(
        t.path(),
        &[
            "--url",
            "http://127.0.0.1:9",
            "--path-style",
            "replay",
            &name,
        ],
    );
    assert_eq!(r.status.code(), Some(77), "{}", stderr(&r));
    assert!(stderr(&r).contains("Replay 28 requests"), "{}", stderr(&r));
    // a name that is no session is said, with where it was looked for
    for args in [
        vec!["compare", "nosuch", name.as_str()],
        vec!["diff", name.as_str(), "nosuch"],
        vec!["replay", "nosuch", "--url", "http://127.0.0.1:9"],
        vec!["show", "nosuch"],
        vec!["check", "nosuch", "--max-requests", "1"],
        vec!["export", "nosuch"],
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
    // --log-out would be written empty: refused, with where the requests are
    for args in [
        vec!["--log-out", "x.har", "run", "--", "true"],
        vec!["--log-out", "x.har", "proxy"],
        vec!["--log-out", "x.har", "watch"],
    ] {
        let o = sonde(t.path(), &args);
        assert_eq!(o.status.code(), Some(64), "{args:?}");
        assert!(stderr(&o).contains("session file"), "{}", stderr(&o));
        assert!(!Path::new("x.har").exists());
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
    // export has one form: it needs no flag to say so
    let o = sonde(t.path(), &["export"]);
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
    assert!(help(&["du"]).contains("table, json or csv"));
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
    for cmd in [
        "show", "sessions", "compare", "check", "diff", "share", "export", "import",
    ] {
        let h = help(&[cmd]);
        assert!(h.contains("--data-dir"), "{cmd}: {h}");
        assert!(
            !h.contains("--url") && !h.contains("--rate") && !h.contains("--log-out"),
            "{cmd}: {h}"
        );
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
