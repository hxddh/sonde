//! The CLI's contract: what each command prints on stdout, in which
//! format, and the exit codes docs/cli.md promises.

use std::path::Path;
use std::process::{Command, Output};

fn sonde(dirs: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonde"))
        .args(args)
        .env("SONDE_CONFIG_DIR", dirs.join("config"))
        .env("SONDE_DATA_DIR", dirs.join("data"))
        .no_aws(dirs)
        .output()
        .unwrap()
}

/// No AWS configuration of the machine's: no credentials in the
/// environment, no `~/.aws` files, no instance role.
trait NoAws {
    fn no_aws(&mut self, dirs: &Path) -> &mut Self;
}

impl NoAws for Command {
    fn no_aws(&mut self, dirs: &Path) -> &mut Self {
        for k in [
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
            "AWS_PROFILE",
            "AWS_REGION",
            "AWS_DEFAULT_REGION",
            "AWS_ENDPOINT_URL",
            "AWS_ENDPOINT_URL_S3",
            "AWS_WEB_IDENTITY_TOKEN_FILE",
            "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
            "AWS_CONTAINER_CREDENTIALS_FULL_URI",
        ] {
            self.env_remove(k);
        }
        self.env("AWS_CONFIG_FILE", dirs.join("aws-config"))
            .env("AWS_SHARED_CREDENTIALS_FILE", dirs.join("aws-credentials"))
            .env("AWS_EC2_METADATA_DISABLED", "true")
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// `sonde` alone says where to start, in three lines: the tour, a real
/// run, where to read more.
#[test]
fn sonde_alone_prints_where_to_start() {
    let t = tempfile::tempdir().unwrap();
    let o = sonde(t.path(), &[]);
    assert_eq!(o.status.code(), Some(0));
    let lines: Vec<String> = stdout(&o).lines().map(String::from).collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[0].starts_with("sonde run --demo "));
    assert!(lines[1].starts_with("sonde run -- <your command> "));
    assert!(lines[2].contains("docs"));
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
    // the usage of a missing argument names what is required, not the
    // directories the environment gives (20.0 named them as required)
    let missing = sonde(t.path(), &["bucket", "size"]);
    assert_eq!(missing.status.code(), Some(64));
    assert!(
        stderr(&missing).contains("Usage: sonde bucket size --bucket <BUCKET>\n"),
        "{}",
        stderr(&missing)
    );
}

#[test]
fn completions_and_man_pages_are_written_and_never_listed() {
    let t = tempfile::tempdir().unwrap();
    for (shell, says) in [
        ("bash", "_sonde()"),
        ("zsh", "#compdef sonde"),
        ("fish", "complete -c sonde"),
        ("powershell", "Register-ArgumentCompleter"),
    ] {
        let o = sonde(t.path(), &["completions", shell]);
        assert_eq!(o.status.code(), Some(0), "{shell}");
        let text = stdout(&o);
        assert!(text.contains(says), "{shell}: {text}");
        assert!(text.contains("replay"), "{shell}");
        // neither generator is a command they complete
        for word in [
            "'completions'",
            "\"completions\"",
            " completions ",
            "'man'",
            " man ",
        ] {
            assert!(!text.contains(word), "{shell} completes {word}");
        }
    }
    assert_eq!(
        sonde(t.path(), &["completions", "tcsh"]).status.code(),
        Some(64)
    );
    let man = t.path().join("man/man1");
    let o = sonde(t.path(), &["man", man.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0));
    let page = std::fs::read_to_string(man.join("sonde.1")).unwrap();
    assert!(page.starts_with(".ie \\n(.g .ds Aq"), "{page}");
    assert!(page.contains("sonde\\-run(1)"), "{page}");
    assert!(man.join("sonde-run.1").exists() && man.join("sonde-endpoints-test.1").exists());
    assert!(!man.join("sonde-man.1").exists() && !man.join("sonde-completions.1").exists());
    // neither is a command the help lists
    let help = stdout(&sonde(t.path(), &["--help"]));
    assert!(
        !help.contains("completions") && !help.contains("  man "),
        "{help}"
    );
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
        ("Read a run:", "\n  runs ", "\n  share "),
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
fn the_demo_records_a_run_that_every_command_reads() {
    let t = tempfile::tempdir().unwrap();
    let demo = sonde(t.path(), &["run", "--demo"]);
    assert_eq!(
        demo.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&demo.stderr)
    );
    // the whole loop on stdout: a finding, its line of code, its fix, and
    // the job fixed, measured against the prediction
    let out = stdout(&demo);
    for said in [
        "checked with HEAD and then read with GET",
        "    at export.py:18 read_reports (3 requests)",
        "    fix for boto3:",
        "The same job with that fix",
    ] {
        assert!(out.contains(said), "no {said:?} in {out}");
    }
    let last = out.lines().last().unwrap();
    // its own requests saved, and the time its plan said it saves
    assert!(
        last.starts_with("  ✓ fixed") && last.contains(" fewer requests (predicted "),
        "{out}"
    );
    // the job it names is there, the line it names holds the HEAD
    let job = std::fs::read_to_string(t.path().join("data/tour/export.py")).unwrap();
    assert!(
        job.lines().nth(17).unwrap().contains("s3.head_object"),
        "{job}"
    );

    // the newest run is the fixed one: no finding left
    let json = sonde(t.path(), &["show", "--requests", "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["requests"].as_array().unwrap().len(), 9);
    assert!(v["diagnoses"].as_array().unwrap().is_empty(), "{v}");

    let table = stdout(&sonde(t.path(), &["runs"]));
    assert!(
        table.starts_with("run   ") && table.contains("recorded") && table.contains("trials"),
        "{table}"
    );
    let rows = runs_json(t.path());
    assert_eq!(rows.len(), 2, "{rows:?}");
    let name = rows[0]["name"].as_str().unwrap().to_string();
    assert!(
        rows[1]["name"].as_str().unwrap().contains("-tour.sonde"),
        "the first run"
    );

    // a run by its name alone
    let by_name = sonde(t.path(), &["show", name.trim_end_matches(".sonde")]);
    assert_eq!(by_name.status.code(), Some(0));
    assert!(stdout(&by_name).contains("9 requests"));

    // the same run against itself: no change
    let path = t.path().join("data/runs").join(&name);
    let p = path.to_str().unwrap();
    let same = stdout(&sonde(t.path(), &["show", p, "--since", p]));
    let block = &same[same.find("\nsince ").unwrap_or_else(|| panic!("{same}"))..];
    assert!(
        block.contains("requests 9 (no change) · waiting"),
        "{block}"
    );
    assert!(!block.contains("0%"), "{block}");
}

#[test]
fn the_demo_reads_as_operations_not_as_a_list_of_requests() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
    let first = tour_first(t.path());
    let out = stdout(&sonde(t.path(), &["show", &first, "--all"]));
    // what sonde reads from requests: four lookups are one operation
    for line in [
        "4 lookups in demo/tour/, none found",
        "12 requests (9 operations) from boto3 1.35",
        // the costs of a few requests are under a cent: their shares, in the table
        "cost",
        "3 objects under demo/reports/ were checked with HEAD and then read with GET",
    ] {
        assert!(out.contains(line), "no {line:?} in:\n{out}");
    }
}

#[test]
fn a_probe_run_named_on_the_command_line_is_not_held_to_read_only_mode() {
    let t = tempfile::tempdir().unwrap();
    // read-only is the default; the run starts (and fails here on the connection, not refused)
    let args = [
        "--url",
        "http://127.0.0.1:9",
        "endpoints",
        "test",
        "adhoc",
        "--probes",
        "scratch",
    ];
    let o = sonde(t.path(), &args);
    assert_ne!(o.status.code(), Some(77), "{}", stderr(&o));
    assert_ne!(o.status.code(), Some(64), "{}", stderr(&o));
}

#[test]
fn check_holds_a_run_to_its_programs_baseline() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
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
    assert!(!sonde_analysis::program::baselines(&t.path().join("data/runs")).is_empty());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
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
        sonde(t.path(), &["check", "--max-requests", "5"])
            .status
            .code(),
        Some(1)
    );
    // the baseline survives pruning
    let pruned = sonde(t.path(), &["runs", "prune", "--keep", "0", "--yes"]);
    assert_eq!(pruned.status.code(), Some(0), "{}", stderr(&pruned));
    assert_eq!(runs_json(t.path()).len(), 1);
}

#[test]
fn show_by_splits_the_operations_table_and_names_the_sdk() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
    let o = sonde(t.path(), &["show", "--by", "kind"]);
    assert_eq!(o.status.code(), Some(0));
    let out = stdout(&o);
    let lines: Vec<&str> = out.lines().collect();
    // the head line names the SDK; then one table, split
    assert!(lines[1].contains(" from boto3 "), "{out}");
    assert!(
        lines[2].starts_with("kind") && lines[2].contains("p99") && lines[2].contains("cost"),
        "{out}"
    );
    assert!(lines.iter().any(|l| l.starts_with("HeadObject")), "{out}");
    // the summary's head line names it too
    let summary = stdout(&sonde(t.path(), &["show"]));
    assert!(summary.contains(" from boto3 "), "{summary}");
    // a dimension it does not know (nor the SDK, which the head line
    // names, nor the status, which the error lines count), and a flag it
    // would ignore
    for by in ["bucket", "sdk", "status"] {
        let o = sonde(t.path(), &["show", "--by", by]);
        assert_eq!(o.status.code(), Some(64), "{by}");
        assert!(
            stderr(&o).contains("not one of kind, prefix, size, caller, key"),
            "{}",
            stderr(&o)
        );
    }
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
    assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
    let mut names: Vec<String> = std::fs::read_dir(t.path().join("data/runs"))
        .unwrap()
        .flatten()
        .map(|e| e.path().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".sonde"))
        .collect();
    names.sort();
    let page = t.path().join("page.html");
    let o = sonde(
        t.path(),
        &["share", &names[0], &names[1], "-o", page.to_str().unwrap()],
    );
    // a build without the viewer says how to make it, and leaves no file
    if stderr(&o).contains("no web viewer") {
        assert_ne!(o.status.code(), Some(0));
        assert!(!page.exists());
    } else {
        assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
        let html = std::fs::read_to_string(&page).unwrap();
        // one file: the viewer and the run, the run before it compared first
        assert!(html.contains("id=\"nsh-params\">{\"compare\":{\"change\":"));
        assert!(!html.contains("<\\/script") && html.matches("</script>").count() >= 3);
    }
    // two runs or a baseline, not both
    assert_eq!(
        sonde(
            t.path(),
            &["share", &names[0], &names[1], "--baseline", "auto"]
        )
        .status
        .code(),
        Some(64)
    );
    // the link a comment ends with is CI's: SONDE_REPORT_URL, which the
    // Action sets (no flag carries it)
    assert_eq!(
        sonde(t.path(), &["check", "--report-url", "https://x"])
            .status
            .code(),
        Some(64)
    );
    let o = Command::new(env!("CARGO_BIN_EXE_sonde"))
        .args([
            "check",
            &names[1],
            "--baseline",
            &names[0],
            // the demo store's times are not meaningful (a loaded runner doubles them): the
            // comment's shape is under test here, not a verdict on its waiting
            "--tolerance",
            "1000",
            "-f",
            "markdown",
        ])
        .env("SONDE_CONFIG_DIR", t.path().join("config"))
        .env("SONDE_DATA_DIR", t.path().join("data"))
        .env("SONDE_REPORT_URL", "https://x.test/r")
        .no_aws(t.path())
        .output()
        .unwrap();
    let md = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{md}");
    assert!(md.contains("**Against the baseline**"), "{md}");
    assert!(md.contains("[The report](https://x.test/r)"), "{md}");
}

/// The demo store's times are not meaningful (`run --demo` says so): no
/// row of `check` holds a demo run's waiting, to a budget or a baseline.
#[test]
fn a_demo_run_is_never_held_to_its_waiting() {
    let t = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
    }
    let rows = runs_json(t.path());
    let (newest, older) = (
        rows[0]["name"].as_str().unwrap(),
        rows[1]["name"].as_str().unwrap(),
    );
    let c = sonde(
        t.path(),
        &[
            "check",
            newest,
            "--baseline",
            older,
            "--max-waiting",
            "0",
            "--tolerance",
            "0",
            "-f",
            "json",
        ],
    );
    let doc: serde_json::Value = serde_json::from_str(&stdout(&c)).unwrap();
    let rows = doc["check"]["verdicts"].as_array().unwrap();
    assert!(rows.iter().all(|v| v["what"] != "waiting"), "{doc:#}");
    assert!(rows.iter().any(|v| v["what"] == "requests"), "{doc:#}");
    assert!(
        stderr(&c).contains("its times are not meaningful, so max-waiting is not checked"),
        "{}",
        stderr(&c)
    );
    // held to its waiting alone: nothing checked, and no mistake (a demo in CI
    // with a sonde.toml that sets max-waiting passes)
    let c = sonde(t.path(), &["check", newest, "--max-waiting", "0"]);
    assert_eq!(c.status.code(), Some(0), "{}", stderr(&c));
    assert!(
        stdout(&c).contains("nothing checked: a demo run's waiting is not held to a limit"),
        "{}",
        stdout(&c)
    );
}

#[test]
fn check_names_the_run_it_judged_and_says_by_how_much_it_failed() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
    let rows = runs_json(t.path());
    let name = rows[0]["name"].as_str().unwrap();
    let c = sonde(t.path(), &["check", "--max-requests", "6"]);
    assert_eq!(c.status.code(), Some(1));
    let out = stdout(&c);
    let first = out.lines().next().unwrap();
    assert!(
        first.starts_with(&format!("run {name} · recorded 20")),
        "{out}"
    );
    // 9 requests against 6
    let fail = out.lines().find(|l| l.starts_with("over")).unwrap();
    assert!(
        fail.contains("requests") && fail.trim_end().ends_with("+50%"),
        "{out}"
    );

    // against itself no warning is new, and the row says so (a baseline
    // always holds the run to no new warning)
    let same = sonde(t.path(), &["check", "--baseline", name]);
    let out = stdout(&same);
    assert_eq!(same.status.code(), Some(0), "{out}");
    assert!(out.contains(&format!("against run {name}")), "{out}");
    assert!(out.contains("new warnings  0"), "{out}");
}

#[test]
fn a_baseline_set_alone_succeeds_by_name_or_relative_path() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
    let dir = t.path().join("data/runs");
    let name = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.ends_with(".sonde"))
        .unwrap();
    // from inside the runs directory, by a relative path
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
    assert!(!stdout(&sonde(t.path(), &["show"])).contains('\u{1b}'));
}

#[test]
fn a_projects_sonde_toml_is_its_budget_and_markdown_is_a_pr_comment() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
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
        cells.contains("| **over** +800% | requests | 9 | ≤ 1 |"),
        "{md}"
    );
    // a row that passed is not repeated in a comment
    assert!(!cells.contains("| within |"), "{md}");
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
    assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
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
fn a_folder_of_logs_is_imported_as_one_run() {
    let t = tempfile::tempdir().unwrap();
    let logs = t.path().join("lake-logs");
    std::fs::create_dir_all(logs.join("2026-02-06")).unwrap();
    let line = |second: u32, id: &str, key: &str| {
        format!(
            r#"79a5 lake [06/Feb/2026:00:00:{second:02} +0000] 192.0.2.3 arn:aws:iam::1:user/etl {id} REST.GET.OBJECT {key} "GET /lake/{key} HTTP/1.1" 200 - 4096 4096 12 10 "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#
        )
    };
    // named out of time order: the run is in time order all the same
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

    // a folder with nothing in it is nothing to import: bad data
    let none = t.path().join("none");
    std::fs::create_dir(&none).unwrap();
    assert_eq!(
        sonde(t.path(), &["import", none.to_str().unwrap()])
            .status
            .code(),
        Some(65)
    );
}

/// 21.0 wrote five formats, a different set per command: now text and
/// json everywhere, and check's two for CI.
#[test]
fn every_command_writes_text_and_json_and_refuses_another_format() {
    let t = tempfile::tempdir().unwrap();
    // refused before anything is asked of a store: none of these has one to ask
    for args in [
        &["bucket", "size", "-b", "b", "-f", "markdown"][..],
        &["bucket", "uploads", "-b", "b", "-f", "csv"],
        &["endpoints", "test", "--probes", "scratch", "-f", "markdown"],
        &["layout", "-b", "b", "-f", "csv"],
        &["endpoints", "list", "-f", "table"],
        &["runs", "-f", "github"],
        &["show", "-f", "markdown"],
        &["replay", "x.sonde", "-f", "csv"],
        &["check", "--max-requests", "1", "-f", "csv"],
    ] {
        let o = sonde(t.path(), args);
        let err = stderr(&o);
        assert_eq!(o.status.code(), Some(64), "{args:?}: {err}");
        assert!(err.contains("invalid value"), "{args:?}: {err}");
        assert!(o.stdout.is_empty(), "{args:?}: {}", stdout(&o));
    }
    // a command that writes only what a person reads has no -f at all
    for args in [
        &["bucket", "abort-uploads", "-b", "b", "-f", "json"][..],
        &["bucket", "lifecycle", "-b", "b", "-f", "text"],
        &["runs", "prune", "--keep", "5", "-f", "json"],
        &["endpoints", "add", "x", "http://x", "-f", "json"],
    ] {
        let o = sonde(t.path(), args);
        assert_eq!(o.status.code(), Some(64), "{args:?}: {}", stderr(&o));
        assert!(stderr(&o).contains("unexpected argument"), "{args:?}");
    }
    for (args, says) in [
        (&["runs", "prune", "--help"][..], None),
        (&["endpoints", "add", "--help"], None),
        (
            &["endpoints", "test", "--help"],
            Some("written: text or json ["),
        ),
        (&["runs", "--help"], Some("written: text or json [")),
        (
            &["bucket", "uploads", "--help"],
            Some("written: text or json ["),
        ),
        (
            &["check", "--help"],
            Some("written: text, json, markdown (a pull request's comment) or github"),
        ),
    ] {
        let help = stdout(&sonde(t.path(), args));
        match says {
            None => assert!(!help.contains("--format"), "{args:?}: {help}"),
            Some(s) => assert!(help.contains(s), "{args:?}: {help}"),
        }
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
    assert_eq!(sonde(t.path(), &["run", "--demo"]).status.code(), Some(0));
    // held to a baseline, with no baseline
    let o = sonde(
        t.path(),
        &["check", "--max-requests", "100000", "--tolerance", "5"],
    );
    assert_eq!(o.status.code(), Some(64));
    assert!(stderr(&o).contains("needs a baseline"), "{}", stderr(&o));
    // with one, it is taken
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
        &["check", "--baseline", "auto", "--tolerance", "5"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    // what 21.0 had and 22.0 does not is no flag
    for args in [
        &["check", "--max-cost", "1"][..],
        &["check", "--max-p95", "1s"],
        &["check", "--no-new-warnings"],
        &["share", "--html"],
        &["share", "--otlp"],
        &["share", "--serve"],
        &["import", "s3://b/p", "--follow"],
        &["runs", "prune", "--keep", "1", "--confirm", "1"],
        // nor what 29.0 took out: sonde's own pace, a request printed as it
        // happens (`show --requests` lists them), a cached scan, the link a
        // comment ends with (SONDE_REPORT_URL)
        &["--rate", "5", "runs"],
        &["--concurrency", "2", "runs"],
        &["run", "--demo", "--live"],
        &["bucket", "size", "-b", "b", "--cached"],
        &["bucket", "size", "-b", "b", "--no-multipart"],
        &["check", "--max-requests", "1", "--report-url", "https://x"],
    ] {
        let o = sonde(t.path(), args);
        assert_eq!(o.status.code(), Some(64), "{args:?}: {}", stderr(&o));
        assert!(
            stderr(&o).contains("unexpected argument") || stderr(&o).contains("unknown option"),
            "{args:?}: {}",
            stderr(&o)
        );
    }
}

/// 29.0 folded commands into others: `compare` into `show --since` and
/// `replay`, `proxy` into `run --listen`, `probe` into `endpoints test
/// --probes`, `endpoints import` into `endpoints add --profile`.
#[test]
fn a_command_folded_into_another_is_no_command() {
    let t = tempfile::tempdir().unwrap();
    for args in [
        &["compare"][..],
        &["proxy"],
        &["probe", "list"],
        &["endpoints", "import"],
    ] {
        let o = sonde(t.path(), args);
        assert_eq!(o.status.code(), Some(64), "{args:?}: {}", stderr(&o));
        assert!(
            stderr(&o).contains("unrecognized subcommand"),
            "{args:?}: {}",
            stderr(&o)
        );
    }
}

/// `run --listen` serves until stopped, with no command; a command, a
/// trial or the demo store with it is a mistake, as is an address other
/// machines reach without --allow-remote.
#[test]
fn run_listen_takes_no_command_and_stays_on_loopback() {
    let t = tempfile::tempdir().unwrap();
    for args in [
        &["run", "--listen", "--", "true"][..],
        &["run", "--listen", "--trial"],
        &["run", "--listen", "--demo"],
        &["run", "--allow-remote", "--", "true"],
        &[
            "run",
            "--listen",
            "0.0.0.0:9000",
            "--url",
            "http://127.0.0.1:9",
        ],
    ] {
        let o = sonde(t.path(), args);
        assert_eq!(o.status.code(), Some(64), "{args:?}: {}", stderr(&o));
    }
    let o = sonde(
        t.path(),
        &[
            "run",
            "--listen",
            "0.0.0.0:9000",
            "--url",
            "http://127.0.0.1:9",
        ],
    );
    assert!(stderr(&o).contains("--allow-remote"), "{}", stderr(&o));
    assert!(runs_in(t.path()) == 0, "nothing recorded");
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
    // and listed with its exit, not as a clean run
    let listed = sonde(t.path(), &["runs", "-f", "json"]);
    let doc: serde_json::Value = serde_json::from_str(&stdout(&listed)).unwrap();
    assert_eq!(doc[0]["exit"], 143, "{doc}");
    let table = stdout(&sonde(t.path(), &["runs"]));
    let (head, row) = (table.lines().next().unwrap(), table.lines().nth(2).unwrap());
    let at = head.find("exit").expect("an exit column");
    assert!(row[at..].starts_with("143"), "{table}");
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The name of the tour's newest run (its fixed run, the one a command
/// without one takes).
fn demo_run(dirs: &Path) -> String {
    assert_eq!(sonde(dirs, &["run", "--demo"]).status.code(), Some(0));
    tour_runs(dirs)[0].clone()
}

/// `show -f json` is the answer an agent reads, lean: each diagnosis
/// carries what to change and where (its text, its callers as file, line and
/// function, the fix for the run's SDK alone, what it saves), the command
/// that records it again; no display phrases, no operation ids, times to a
/// tenth of a millisecond, and no `since` for a program's first run.
#[test]
fn show_json_is_lean_and_each_diagnosis_names_what_to_change_and_where() {
    let t = tempfile::tempdir().unwrap();
    demo_run(t.path());
    // the tour's first run: the job before its fix
    let name = tour_runs(t.path())[1].clone();
    let shown = sonde(t.path(), &["show", &name, "-f", "json"]);
    assert_eq!(shown.status.code(), Some(0), "{}", stderr(&shown));
    let text = stdout(&shown);
    let j: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(j.get("operations").is_none(), "only with --all");
    assert_eq!(j["verify"], "sonde run --demo");
    let d = &j["diagnoses"][0];
    assert_eq!(d["kind"], "obs.headget", "{d}");
    assert!(
        d["text"]
            .as_str()
            .is_some_and(|t| t.contains("checked with HEAD")),
        "{d}"
    );
    assert_eq!(d["at"][0]["file"], "export.py", "{d}");
    assert!(d["at"][0]["line"].as_u64().is_some(), "{d}");
    assert_eq!(d["at"][0]["function"], "read_reports", "{d}");
    assert_eq!(d["fix"]["sdk"], "python", "{d}");
    assert!(d["fix"]["code"].is_string(), "{d}");
    assert!(
        d["savesMs"].is_number() && d["savingText"].is_string(),
        "{d}"
    );
    // no join to make: the findings, their places and display phrases are --all's
    for gone in ["findings", "serial", "saving", "rest"] {
        assert!(d.get(gone).is_none(), "{gone}: {d}");
    }
    assert!(j.get("findings").is_none(), "{text}");
    for gone in [
        "\"phrases\"",
        "\"impact\"",
        "\"level\"",
        "\"kinds\"",
        "\"sumMs\"",
    ] {
        assert!(!text.contains(gone), "{gone} in {text}");
    }
    // a program's first run: nothing to compare
    assert!(j["since"].is_null(), "{}", j["since"]);
    // a time to a tenth of a millisecond, a figure to six digits
    fn walk(v: &serde_json::Value, key: &str, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => m.iter().for_each(|(k, x)| walk(x, k, out)),
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, key, out)),
            serde_json::Value::Number(n) if n.is_f64() => {
                let shown = n.to_string();
                let digits = shown.trim_start_matches(['-', '0', '.']).replace('.', "");
                let digits = digits.split(['e', 'E']).next().unwrap_or("").len();
                let tenth = shown.split_once('.').is_none_or(|(_, f)| f.len() <= 1);
                let ms = key.ends_with("Ms") || key == "ms";
                if (ms && !tenth) || (!ms && digits > 6) {
                    out.push(format!("{key}: {shown}"));
                }
            }
            _ => {}
        }
    }
    let mut long = Vec::new();
    walk(&j, "", &mut long);
    assert!(long.is_empty(), "{long:?}");
    // --all: the findings whole (every SDK's fix, every operation), and
    // each diagnosis's by place
    let all: serde_json::Value =
        serde_json::from_slice(&sonde(t.path(), &["show", &name, "--all", "-f", "json"]).stdout)
            .unwrap();
    assert!(all["findings"][0]["fixes"].as_array().unwrap().len() > 1);
    assert_eq!(
        all["findings"][0]["operationsTotal"],
        all["findings"][0]["operations"].as_array().unwrap().len()
    );
    assert_eq!(all["diagnoses"][0]["findings"][0], 0);
    assert_eq!(
        all["operations"].as_array().unwrap().len() as u64,
        all["operationsTotal"].as_u64().unwrap()
    );
    // the fixed run against it: its finding fixed, what it saved beside
    // what was predicted, said in one place (`since.findings`)
    let fixed = tour_runs(t.path())[0].clone();
    let j: serde_json::Value =
        serde_json::from_slice(&sonde(t.path(), &["show", &fixed, "-f", "json"]).stdout).unwrap();
    let f = &j["since"]["findings"][0];
    assert_eq!(f["state"], "fixed", "{}", j["since"]);
    // its own requests saved, beside the time its plan predicted; the
    // run's time is the run's, said once in the change
    assert!(f["savedRequests"].as_u64().unwrap() > 0, "{f}");
    assert!(f["predicted"]["ms"].as_f64().unwrap() > 0.0, "{f}");
    assert!(
        f.get("saved").is_none() && f.get("withinNoise").is_none(),
        "{f}"
    );
    assert!(j["since"]["change"]["wallMs"].is_array(), "{}", j["since"]);
    let change = &j["since"]["change"];
    for gone in ["findings", "resolved", "appeared", "latency"] {
        assert!(change.get(gone).is_none(), "{gone}: {change}");
    }
}

/// A program run again says what changed, not its whole plan again: each
/// finding fixed with what it saved, each still open on a line with its line
/// of code, a new one in full; `sonde show` has the whole plan.
#[test]
fn a_run_again_says_only_what_changed() {
    let t = tempfile::tempdir().unwrap();
    demo_run(t.path());
    let tour = t.path().join("data/tour");
    let run = |job: &str| {
        std::fs::copy(tour.join(job), t.path().join("job.py")).unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_sonde"))
            .args(["run", "--demo", "--name", "job", "--"])
            .arg(env!("CARGO_BIN_EXE_sonde"))
            .args(["demo-job", "job.py"])
            .current_dir(t.path())
            .env("SONDE_CONFIG_DIR", t.path().join("config"))
            .env("SONDE_DATA_DIR", t.path().join("data"))
            .no_aws(t.path())
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
        stderr(&o)
    };
    let plan = ["    fix for boto3:", "kind ", "    saving: "];
    // a program's first run: the whole plan
    let first = run("export.py");
    for said in plan {
        assert!(first.contains(said), "no {said:?} in:\n{first}");
    }
    // again, unchanged: each finding still open, a line each with its code line
    let again = run("export.py");
    for not in plan {
        assert!(!again.contains(not), "{not:?} in:\n{again}");
    }
    assert!(again.contains("since the run before"), "{again}");
    assert!(again.contains("  · open   "), "{again}");
    assert!(again.contains(" (at job.py:"), "{again}");
    assert!(again.contains("(the whole plan: sonde show)"), "{again}");
    assert!(again.contains("  time "), "{again}");
    // fixed: what it saved, and nothing of the plan
    let fixed = run("export-fixed.py");
    assert!(fixed.contains("  ✓ fixed  "), "{fixed}");
    for not in plan {
        assert!(!fixed.contains(not), "{not:?} in:\n{fixed}");
    }
    // undone: new again, in full as the plan says it
    let undone = run("export.py");
    assert!(undone.contains("  ! new    "), "{undone}");
    assert!(undone.contains("    fix for boto3:"), "{undone}");
    assert!(!undone.contains("kind "), "{undone}");
    assert!(
        undone.split_whitespace().count() * 2 < first.split_whitespace().count() * 3,
        "{undone}"
    );
    // show: the whole plan
    let shown = Command::new(env!("CARGO_BIN_EXE_sonde"))
        .args(["show"])
        .current_dir(t.path())
        .env("SONDE_CONFIG_DIR", t.path().join("config"))
        .env("SONDE_DATA_DIR", t.path().join("data"))
        .no_aws(t.path())
        .output()
        .unwrap();
    let shown = stdout(&shown);
    for said in plan {
        assert!(shown.contains(said), "no {said:?} in:\n{shown}");
    }
}

/// The runs listed, newest first (`runs -f json`).
fn runs_json(dirs: &Path) -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(&stdout(&sonde(dirs, &["runs", "-f", "json"])))
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

/// The tour's runs by name, newest first.
fn tour_runs(dirs: &Path) -> Vec<String> {
    let listed: serde_json::Value =
        serde_json::from_str(&stdout(&sonde(dirs, &["runs", "-f", "json"]))).unwrap();
    listed
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            r["name"]
                .as_str()
                .unwrap()
                .trim_end_matches(".sonde")
                .to_string()
        })
        .collect()
}

/// The newest tour's first run: the job before its fix, with its finding.
fn tour_first(dirs: &Path) -> String {
    tour_runs(dirs)[1].clone()
}

const A_LOG_LINE: &str = "79a5 lake [06/Feb/2031:00:00:38 +0000] 192.0.2.3 - 3E57 REST.GET.OBJECT t/a \"GET /lake/t/a HTTP/1.1\" 200 - 4096 4096 12 10 \"-\" \"boto3/1.34\" - x SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -\n";

#[test]
fn every_command_takes_a_run_by_its_name_and_says_where_it_looked() {
    let t = tempfile::tempdir().unwrap();
    let name = demo_run(t.path());
    // since a run by name, as show and check take one
    let c = sonde(t.path(), &["show", &name, "--since", &name]);
    assert_eq!(c.status.code(), Some(0), "{}", stderr(&c));
    assert!(stdout(&c).contains("(no change)"), "{}", stdout(&c));
    // a replay opens it too (and stops at the confirmation: nothing is sent)
    let r = sonde(
        t.path(),
        &[
            "--url",
            "http://127.0.0.1:9",
            "--path-style",
            "replay",
            &name,
            "-e",
            "adhoc",
        ],
    );
    // a dry run: what it would send, where, and how to send it
    assert_eq!(r.status.code(), Some(4), "{}", stderr(&r));
    assert!(stderr(&r).contains("Replay 9 requests"), "{}", stderr(&r));
    assert!(
        stderr(&r).contains("run again with --yes"),
        "{}",
        stderr(&r)
    );
    // a name that is no run is said, with where it was looked for
    for args in [
        vec!["show", name.as_str(), "--since", "nosuch"],
        vec![
            "replay",
            "nosuch",
            "-e",
            "adhoc",
            "--url",
            "http://127.0.0.1:9",
        ],
        vec!["show", "nosuch"],
        vec!["check", "nosuch", "--max-requests", "1"],
        vec!["share", "nosuch"],
    ] {
        let o = sonde(t.path(), &args);
        assert_eq!(o.status.code(), Some(64), "{args:?}: {}", stderr(&o));
        assert!(
            stderr(&o).contains("no run nosuch") && stderr(&o).contains("runs"),
            "{args:?}: {}",
            stderr(&o)
        );
    }
    let o = sonde(t.path(), &["import", "nosuchdir"]);
    assert_eq!(o.status.code(), Some(64));
    assert!(stderr(&o).contains("nosuchdir"), "{}", stderr(&o));
}

/// 20.0 compared two imports of one log as "two different programs" and
/// then "the same … findings", with a waiting `show` says is not known.
#[test]
fn two_imports_compare_without_contradicting_themselves() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("access.log");
    std::fs::write(&log, A_LOG_LINE.repeat(3)).unwrap();
    let (a, b) = (t.path().join("a.sonde"), t.path().join("b.sonde"));
    for f in [&a, &b] {
        let o = sonde(
            t.path(),
            &["import", log.to_str().unwrap(), "-o", f.to_str().unwrap()],
        );
        assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    }
    let (a, b) = (a.to_str().unwrap(), b.to_str().unwrap());
    let text = stdout(&sonde(t.path(), &["show", b, "--since", a]));
    assert!(text.contains("not two runs of one program"), "{text}");
    assert!(
        text.contains("requests 3 (no change) · waiting — (not known) · cost"),
        "{text}"
    );
    let doc: serde_json::Value = serde_json::from_str(&stdout(&sonde(
        t.path(),
        &["show", b, "--since", a, "-f", "json"],
    )))
    .unwrap();
    assert_eq!(doc["since"]["change"]["waitingKnown"], false, "{doc}");
}

#[test]
fn the_newest_run_is_the_newest_recording_not_an_import() {
    let t = tempfile::tempdir().unwrap();
    let name = demo_run(t.path());
    // an import made later, and of a later time
    let log = t.path().join("later.log");
    std::fs::write(&log, A_LOG_LINE).unwrap();
    let o = sonde(t.path(), &["import", log.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    // its summary on stderr, as `run` says its own (27.0: on stdout)
    assert_eq!(stdout(&o), "");
    assert!(stderr(&o).contains("1 request"), "{}", stderr(&o));
    let shown = sonde(t.path(), &["show", "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(v["summary"]["requests"], 9, "the demo's, not the import's");
    let checked = sonde(t.path(), &["check", "--max-requests", "100"]);
    assert!(stdout(&checked).contains(&name), "{}", stdout(&checked));
    // the import is still a run, read by its name
    let named = sonde(t.path(), &["show", "later", "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&named.stdout).unwrap();
    assert_eq!(v["summary"]["requests"], 1);
    // and the list says what each holds
    let list = stdout(&sonde(t.path(), &["runs"]));
    assert!(list.contains("requests") && list.contains("took") && list.contains("errors"));
    let rows: serde_json::Value =
        serde_json::from_slice(&sonde(t.path(), &["runs", "-f", "json"]).stdout).unwrap();
    let demo = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"].as_str().unwrap().starts_with(&name))
        .unwrap();
    assert_eq!(demo["requests"], 9);
    assert!(demo["seconds"].is_number() && demo["errors"].is_number());
}

#[test]
fn what_a_command_cannot_do_is_said_with_what_does_it() {
    let t = tempfile::tempdir().unwrap();
    // flags and commands of earlier versions are unknown, as any other
    for args in [
        vec!["--log-out", "x.har", "endpoints", "list"],
        vec!["run", "--no-delete", "--", "true"],
        vec!["run", "--read-only", "--", "true"],
        vec!["run", "--allow-writes", "--", "true"],
        vec!["share", "--json"],
        vec!["export"],
        vec!["demo"],
        vec!["watch"],
        vec!["replay", "x.sonde", "--replay", "e"],
        vec!["diff", "a.sonde", "b.sonde"],
        vec!["du", "-b", "b"],
        vec!["mpu", "scan", "-b", "b"],
        vec!["bucket", "scan", "-b", "b"],
        vec!["replay", "x.sonde", "-e", "e", "--in-place"],
    ] {
        let o = sonde(t.path(), &args);
        assert_eq!(o.status.code(), Some(64), "{args:?}: {}", stderr(&o));
        assert!(
            stderr(&o).contains("unexpected argument")
                || stderr(&o).contains("unrecognized subcommand")
                || stderr(&o).contains("unknown option"),
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
    // a time on the command line reads as sonde.toml's does (of a run on a
    // store: a demo run's waiting is held to nothing)
    run_of(
        t.path(),
        "timed.sonde",
        "python etl.py",
        "2026-11-09T02:00:00Z",
    );
    let o = sonde(t.path(), &["check", "--max-waiting", "2s", "-f", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["check"]["verdicts"][0]["limit"], 2000.0, "{v}");
    assert_eq!(
        sonde(t.path(), &["check", "--max-waiting", "soon"])
            .status
            .code(),
        Some(64)
    );
    // a run without a failure says so, when only failures are asked for
    let o = sonde(t.path(), &["show", "--errors", "--requests"]);
    let shown = stdout(&o);
    if !shown.contains(" 5") && !shown.contains("ERR") {
        assert!(shown.contains("no errors"), "{shown}");
    }
}

#[test]
fn a_clean_run_asked_for_its_errors_says_there_are_none() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("clean.log");
    std::fs::write(&log, A_LOG_LINE).unwrap();
    let out = t.path().join("clean.sonde");
    let o = sonde(
        t.path(),
        &["import", log.to_str().unwrap(), "-o", out.to_str().unwrap()],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    // (one request is one operation: said once)
    assert!(stderr(&o).contains("1 request from"), "{}", stderr(&o));
    assert!(!stderr(&o).contains("operation"), "{}", stderr(&o));
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
    assert!(show.contains("The format written: text or json"), "{show}");
    assert!(!show.contains("csv") && !show.contains("github"), "{show}");
    assert!(help(&["check"]).contains("markdown") && help(&["check"]).contains("github"));
    assert!(help(&["bucket", "size"]).contains("text or json"));
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
    for cmd in ["show", "runs", "check", "share"] {
        let h = help(&[cmd]);
        assert!(h.contains("--data-dir"), "{cmd}: {h}");
        assert!(!h.contains("--url"), "{cmd}: {h}");
    }
    // (still taken where it was always taken: it is hidden, not gone)
    assert_eq!(
        sonde(t.path(), &["runs", "--region", "eu-west-1"])
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn check_init_leaves_the_limits_of_time_for_a_person_to_set() {
    let t = tempfile::tempdir().unwrap();
    demo_run(t.path());
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
    // what follows from the others is no limit of its own
    assert!(
        !text.contains("max-p95") && !text.contains("max-cost"),
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

/// The demo's run, and where it is.
fn demo_file(dirs: &Path) -> std::path::PathBuf {
    let name = demo_run(dirs);
    dirs.join("data/runs").join(format!("{name}.sonde"))
}

/// A recorded run of `command` in the runs folder, made of a
/// fixture's requests.
/// The folder [`sonde`] runs in, as a run's `cwd` says it.
fn here() -> String {
    std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn run_of(dirs: &Path, name: &str, command: &str, created: &str) -> std::path::PathBuf {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../analysis/tests/fixtures/sdk-python.sonde");
    let text = std::fs::read_to_string(fixture).unwrap();
    let (header, records) = text.split_once('\n').unwrap();
    let mut header: serde_json::Value = serde_json::from_str(header).unwrap();
    header["command"] = command.into();
    header["created"] = created.into();
    // recorded where `sonde` runs: the run a command takes when none is named
    header["cwd"] = here().into();
    let dir = dirs.join("data/runs");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    std::fs::write(&file, format!("{header}\n{records}")).unwrap();
    file
}

/// With nothing configured, a recording goes where an AWS SDK would: the
/// default credential chain and region, said; without credentials either,
/// the usage error says both ways.
#[test]
fn with_no_endpoint_a_run_goes_where_the_aws_default_chain_says() {
    let t = tempfile::tempdir().unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_sonde"))
        .args(["run", "--", "true"])
        .env("SONDE_CONFIG_DIR", t.path().join("config"))
        .env("SONDE_DATA_DIR", t.path().join("data"))
        .no_aws(t.path())
        .env("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE")
        .env("AWS_SECRET_ACCESS_KEY", "secret")
        .env("AWS_REGION", "eu-west-1")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(
        stderr(&o).contains(
            "no endpoint configured: the AWS default credentials and region eu-west-1 (AWS_REGION) → https://s3.eu-west-1.amazonaws.com"
        ),
        "{}",
        stderr(&o)
    );
    let o = sonde(t.path(), &["run", "--", "true"]);
    assert_eq!(o.status.code(), Some(64), "{}", stderr(&o));
    assert!(
        stderr(&o).contains("no credentials found"),
        "{}",
        stderr(&o)
    );
}

/// A run of no request is a program whose S3 client went around sonde:
/// `check` fails it, against a baseline or a budget, and `run` says so.
#[test]
fn nothing_reached_sonde_fails_the_check_and_run_says_so() {
    let t = tempfile::tempdir().unwrap();
    let base = run_of(
        t.path(),
        "base.sonde",
        "python etl.py",
        "2026-11-01T10:00:00Z",
    );
    let header = std::fs::read_to_string(&base).unwrap();
    let empty = t.path().join("data/runs/empty.sonde");
    std::fs::write(&empty, format!("{}\n", header.lines().next().unwrap())).unwrap();
    let base = base.to_string_lossy().into_owned();
    let empty = empty.to_string_lossy().into_owned();
    for args in [
        vec!["check", &empty, "--baseline", &base],
        vec!["check", &empty, "--max-requests", "100"],
        vec!["check", &empty, "--baseline", &base, "-f", "markdown"],
    ] {
        let o = sonde(t.path(), &args);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", stdout(&o));
        assert!(
            stdout(&o).contains("nothing reached sonde"),
            "{}",
            stdout(&o)
        );
        assert!(stderr(&o).contains("recorded no request"), "{}", stderr(&o));
    }
    let gh = sonde(
        t.path(),
        &["check", &empty, "--max-errors", "0", "-f", "github"],
    );
    assert_eq!(gh.status.code(), Some(1));
    assert!(stdout(&gh).starts_with("::error title=sonde check::nothing reached sonde"));
    let json = sonde(
        t.path(),
        &["check", &empty, "--max-errors", "0", "-f", "json"],
    );
    let doc: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        (doc["passed"].as_bool(), doc["check"]["empty"].as_bool()),
        (Some(false), Some(true))
    );
    // no figures to hold another run to
    let set = sonde(t.path(), &["check", &empty, "--set-baseline"]);
    assert_eq!(set.status.code(), Some(64));
    let ran = sonde(t.path(), &["run", "--demo", "--", "true"]);
    assert!(
        stderr(&ran).contains("Nothing reached sonde"),
        "{}",
        stderr(&ran)
    );
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
        assert_eq!(closed(&["check", "--max-requests", "5"]), Some(1));
        assert_eq!(
            closed(&["check", "--max-requests", "5", "-f", "json"]),
            Some(1)
        );
        assert_eq!(closed(&["show", "--all"]), Some(0));
        assert_eq!(closed(&["show", "-f", "json"]), Some(0));
    }
}

#[test]
fn runs_are_listed_from_what_was_counted_in_the_folders_index() {
    let t = tempfile::tempdir().unwrap();
    let file = demo_file(t.path());
    // counted when it was recorded: in the folder's one index, with all else
    // kept beside the runs (a program's findings too): no other file there
    let counted = || sonde_analysis::index::get(&file).counted.is_some();
    assert!(counted());
    let beside: Vec<String> = std::fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.ends_with(".sonde"))
        .collect();
    assert_eq!(beside.len(), 2, "{beside:?}");
    assert!(
        beside
            .iter()
            .all(|n| n == ".index.jsonl" || n == ".index.lock"),
        "{beside:?}"
    );
    let listed: serde_json::Value =
        serde_json::from_str(&stdout(&sonde(t.path(), &["runs", "-f", "json"]))).unwrap();
    assert_eq!(listed[0]["requests"], 9, "{listed}");
    assert_eq!(listed[0]["program"], "tour");
    assert!(listed[0]["seconds"].as_f64().unwrap() > 0.0);
    // one that is missing is made by the listing
    sonde_analysis::index::set(&file, sonde_analysis::index::Change::NoCounted);
    assert!(!counted());
    assert!(stdout(&sonde(t.path(), &["runs"])).contains("tour"));
    assert!(counted());
    // nothing past the newest five: said, not silence
    let kept = sonde(t.path(), &["runs", "prune", "--keep", "5", "--yes"]);
    assert!(
        stdout(&kept).contains("nothing to remove: 2 runs here"),
        "{}",
        stdout(&kept)
    );
    assert!(file.is_file());
    assert_eq!(kept.status.code(), Some(0));
    // a dry run that would delete: 4, and nothing deleted
    let dry = sonde(t.path(), &["runs", "prune", "--keep", "0"]);
    assert_eq!(dry.status.code(), Some(4), "{}", stdout(&dry));
    assert!(
        stderr(&dry).contains("run again with --yes"),
        "{}",
        stderr(&dry)
    );
    assert!(file.is_file());
    // removed, and what was counted of it with it
    let gone = sonde(t.path(), &["runs", "prune", "--keep", "0", "--yes"]);
    assert!(stdout(&gone).contains("deleted "), "{}", stdout(&gone));
    assert!(!file.exists() && !counted());
}

#[test]
fn a_run_with_a_line_no_record_could_be_is_bad_data_to_every_reader() {
    let t = tempfile::tempdir().unwrap();
    let file = run_of(
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
        &["show", "bad", "--since", "bad"],
    ] {
        assert_eq!(sonde(t.path(), args).status.code(), Some(65), "{args:?}");
    }
    // and the list says so, rather than listing it as a run that reads
    let listed = sonde(t.path(), &["runs", "-f", "json"]);
    let rows: serde_json::Value = serde_json::from_str(&stdout(&listed)).unwrap();
    assert!(
        rows[0]["unreadable"]
            .as_str()
            .unwrap()
            .starts_with("line 2: "),
        "{rows}"
    );
    let table = sonde(t.path(), &["runs"]);
    assert!(
        stderr(&table).contains("bad.sonde is not a run: line 2: "),
        "{}",
        stderr(&table)
    );
}

#[test]
fn show_since_says_the_kinds_it_leaves_out_and_waiting_as_show_does() {
    let t = tempfile::tempdir().unwrap();
    run_of(t.path(), "a.sonde", "python etl.py", "2026-11-01T10:00:00Z");
    let b = run_of(t.path(), "b.sonde", "python etl.py", "2026-11-01T11:00:00Z");
    // the second run made one upload part fewer
    let text = std::fs::read_to_string(&b).unwrap();
    let mut lines: Vec<&str> = text.lines().collect();
    let part = lines
        .iter()
        .position(|l| l.contains("\"UploadPart\""))
        .unwrap();
    lines.remove(part);
    std::fs::write(&b, lines.join("\n") + "\n").unwrap();
    let out = sonde(t.path(), &["show", "b", "--since", "a"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    let text = &text[text.find("\nsince a").unwrap_or_else(|| panic!("{text}"))..];
    assert!(text.contains("UploadPart"), "{text}");
    // ten kinds, one changed: the nine others are said, not dropped
    assert!(
        text.contains("… and 9 more kinds of request, unchanged"),
        "{text}"
    );
    // a kind's waiting is its share of the run's, as show's table says it
    let json: serde_json::Value = serde_json::from_str(&stdout(&sonde(
        t.path(),
        &["show", "b", "--since", "a", "-f", "json"],
    )))
    .unwrap();
    let shown: serde_json::Value =
        serde_json::from_str(&stdout(&sonde(t.path(), &["show", "a", "-f", "json"]))).unwrap();
    let kinds = shown["waiting"]["byKind"].as_array().unwrap();
    for g in json["since"]["change"]["groups"].as_array().unwrap() {
        let s = kinds
            .iter()
            .find(|k| k["kind"] == g["kind"] && k["missed"] == g["missed"])
            .unwrap();
        assert_eq!(g["waitingMs"][0], s["waitedMs"], "{g} {s}");
    }
}

#[test]
fn a_compressed_run_cut_short_is_read_up_to_its_cut() {
    use std::io::Write;
    let t = tempfile::tempdir().unwrap();
    let file = run_of(
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
    let shown = sonde(t.path(), &["show", "cut"]);
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
    run_of(t.path(), "long.sonde", &long, "2026-11-10T02:00:00Z");
    let narrow = Command::new(env!("CARGO_BIN_EXE_sonde"))
        .arg("runs")
        .env("SONDE_CONFIG_DIR", t.path().join("config"))
        .env("SONDE_DATA_DIR", t.path().join("data"))
        .env("COLUMNS", "120")
        .output()
        .unwrap();
    let table = stdout(&narrow);
    let row = table.lines().find(|l| l.starts_with("long.sonde")).unwrap();
    assert!(
        row.chars().count() <= 120,
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
        .arg("runs")
        .env("SONDE_CONFIG_DIR", t.path().join("config"))
        .env("SONDE_DATA_DIR", t.path().join("data"))
        .env_remove("COLUMNS")
        .output()
        .unwrap();
    assert!(stdout(&piped).contains(&long), "{}", stdout(&piped));
}

#[test]
fn a_trials_run_is_found_by_its_name() {
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
        let o = sonde(t.path(), &["show", name]);
        assert_eq!(o.status.code(), Some(0), "{name}: {}", stderr(&o));
        assert!(stdout(&o).contains("9 requests"), "{name}");
    }
}

#[test]
fn show_writes_only_what_failed_as_json_too_and_the_ends_of_a_long_run() {
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
        &["show", out, "--errors", "--requests", "-f", "json"],
    )))
    .unwrap();
    assert_eq!(failed["requests"].as_array().unwrap().len(), 24);
    assert_eq!(failed["operations"].as_array().unwrap().len(), 24);
    assert_eq!(
        failed["summary"]["requests"], 2400,
        "the summary is of the run"
    );
    let all: serde_json::Value = serde_json::from_str(&stdout(&sonde(
        t.path(),
        &["show", out, "--requests", "-f", "json"],
    )))
    .unwrap();
    assert_eq!(all["requests"].as_array().unwrap().len(), 2400);
    // the requests only when asked for
    let lean: serde_json::Value =
        serde_json::from_str(&stdout(&sonde(t.path(), &["show", out, "-f", "json"]))).unwrap();
    assert!(lean.get("requests").is_none() && lean["operationsTotal"].is_number());
    // its operations every one with --all
    let whole: serde_json::Value = serde_json::from_str(&stdout(&sonde(
        t.path(),
        &["show", out, "--all", "-f", "json"],
    )))
    .unwrap();
    assert_eq!(
        whole["operations"].as_array().unwrap().len() as u64,
        whole["operationsTotal"].as_u64().unwrap()
    );
    assert_eq!(all["requests"][7]["status"], 403);
    // a long run: its first and last hundred, and how many are between
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
fn a_baseline_holds_a_program_whose_argument_changes_between_runs() {
    let t = tempfile::tempdir().unwrap();
    let monday = run_of(
        t.path(),
        "monday.sonde",
        "python etl.py --date 2026-11-09",
        "2026-11-09T02:00:00Z",
    );
    let tuesday = run_of(
        t.path(),
        "tuesday.sonde",
        "python etl.py --date 2026-11-10",
        "2026-11-10T02:00:00Z",
    );
    let set = sonde(
        t.path(),
        &["check", monday.to_str().unwrap(), "--set-baseline"],
    );
    assert_eq!(set.status.code(), Some(0), "{}", stderr(&set));
    // the next day's run is the same program, `etl`: Monday's baseline holds it
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
    assert!(!err.contains("no baseline yet"), "{err}");
    let doc: serde_json::Value = serde_json::from_str(&stdout(&c)).unwrap();
    assert_eq!(doc["passed"], true, "{doc}");
    assert!(doc["noBaseline"].is_null(), "{doc}");
    // held to it by name: over it is "over its baseline", not "over budget"
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
    // a program is what its command runs: `check --init` writes no name
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
    assert!(!std::fs::read_to_string(&cfg).unwrap().contains("[program]"));
}

#[test]
fn a_check_of_trials_alone_has_no_failed_budget_inside_it() {
    let t = tempfile::tempdir().unwrap();
    let file = run_of(
        t.path(),
        "run.sonde",
        "python etl.py",
        "2026-11-09T02:00:00Z",
    );
    // a verdict kept in the folder's index
    let broke: Vec<sonde_analysis::trial::Trial> = serde_json::from_str(
        r#"[{"scenario":"slowdown","about":"503 SlowDown","standing":"broke","exit":1,"cleanExit":0,
            "faults":2,"retried":0,"droppedReads":0,"droppedWrites":2,"attempts":1,"requests":10,
            "cleanRequests":13,"leftUploads":0,"wroteTwice":0,"reasons":["exited 1"]}]"#,
    )
    .unwrap();
    sonde_analysis::trial::save(&file, &broke).unwrap();
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
fn the_help_lists_eleven_commands_at_most() {
    let t = tempfile::tempdir().unwrap();
    let help = stdout(&sonde(t.path(), &["--help"]));
    let listed: Vec<&str> = help
        .lines()
        .filter(|l| l.starts_with("  ") && !l.trim_start().starts_with('-'))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|n| *n != "help")
        .collect();
    assert!(listed.len() <= 11, "{listed:?}");
}

/// The fix loop on recorded runs of one boto3 program: `show` says each
/// finding's saving, line and fix, and the change since the run before
/// (or since the run named with `--since`).
#[test]
fn a_fix_is_confirmed_with_what_it_saved_and_an_undone_one_is_new_again() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("data/runs");
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
            .join("../analysis/tests/fixtures")
            .join(format!("{r}.sonde"));
        // recorded where `sonde` runs, so that `compare` alone takes them
        let mut text = String::new();
        std::io::Read::read_to_string(
            &mut flate2::read::MultiGzDecoder::new(std::fs::File::open(from).unwrap()),
            &mut text,
        )
        .unwrap();
        let (header, records) = text.split_once('\n').unwrap();
        let mut header: serde_json::Value = serde_json::from_str(header).unwrap();
        header["cwd"] = here().into();
        let mut gz = flate2::write::GzEncoder::new(
            std::fs::File::create(dir.join(format!("{r}.sonde"))).unwrap(),
            flate2::Compression::default(),
        );
        std::io::Write::write_all(&mut gz, format!("{header}\n{records}").as_bytes()).unwrap();
        gz.finish().unwrap();
    }
    let shown: Vec<String> = runs
        .iter()
        .map(|r| stdout(&sonde(t.path(), &["show", r])))
        .collect();
    // the head line names the SDK; where the time went; the first diagnosis
    // its saving (time of the run), line and fix; the others a line each
    let first = &shown[0];
    assert!(
        first.contains("91 requests (73 operations) from boto3 1.44 to moto"),
        "{first}"
    );
    for said in [
        "  time 739 ms: your program's own 250 ms (34%) · S3 round trips 489 ms (91 requests, median 3.3 ms)",
        // sent at once saves more than dropping the HEADs: its own change, first
        "! 51 requests ran one after another for 450 ms",
        "    saving: ≈160 ms of 739 ms (22%)",
        "    at etl.py:14 <module> (20 requests)",
        "    fix for boto3:",
        "      with ThreadPoolExecutor(8) as pool:",
        "the GET alone answers both · ≈65 ms of 739 ms (9%) (at etl.py:19 <module>)",
        " · ≈50 ms of 739 ms (7%) (at etl.py:33 <module>)",
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
        third.contains("✓ fixed  20 objects under etl-bucket/run/in/ were checked with HEAD and then read with GET: the GET alone answers both · 20 fewer requests (predicted "),
        "{third}"
    );
    assert!(third.contains("  requests 91 → 71 (-22%)"), "{third}");
    // the run's time said once, for the run: the two runs before it took
    // 397 ms apart, so what it moved by is not told from that
    assert!(
        third.contains("since the run before (etl-2-again) · time ")
            && third.contains(" · its runs vary by 397 ms: within noise"),
        "{third}"
    );
    assert!(
        shown[3].contains("✓ fixed  6 files under 5 MiB"),
        "{}",
        shown[3]
    );
    assert!(
        shown[4].contains("! new    20 objects under etl-bucket/run/in/ were checked"),
        "{}",
        shown[4]
    );

    // show alone: the newest run against the one before, an undone fix new again
    let c = stdout(&sonde(t.path(), &["show"]));
    assert!(c.contains("since the run before (etl-4-no-small)"), "{c}");
    assert!(c.contains("! new    6 files under 5 MiB"), "{c}");
    // since a run named: the kinds that moved too
    let c = stdout(&sonde(
        t.path(),
        &["show", "etl-3-no-head", "--since", "etl-2-again"],
    ));
    assert!(c.contains("since etl-2-again · time "), "{c}");
    assert!(c.contains("✓ fixed  20 objects"), "{c}");
    assert!(c.contains("  HeadObject "), "{c}");
    let j: serde_json::Value = serde_json::from_str(&stdout(&sonde(
        t.path(),
        &[
            "show",
            "etl-3-no-head",
            "--since",
            "etl-2-again",
            "-f",
            "json",
        ],
    )))
    .unwrap();
    let fixed = &j["since"]["findings"][0];
    assert_eq!(fixed["state"], "fixed", "{j}");
    assert_eq!(fixed["predicted"]["requests"], 20);
    assert_eq!(fixed["savedRequests"], 20);
    assert!(j["since"]["change"].get("findings").is_none(), "{j}");
    // a program's first run has nothing before it
    let none = sonde(t.path(), &["show", "etl-1-base"]);
    assert_eq!(none.status.code(), Some(0));
    assert!(
        !stdout(&none).contains("since the run before"),
        "{}",
        stdout(&none)
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
        &["show", "etl-3-no-head", "-f", "json"],
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
    sonde_analysis::index::set(&crashed, sonde_analysis::index::Change::Exit(3));
    let s = stdout(&sonde(t.path(), &["show", "etl-6-crashed"]));
    assert!(s.contains("the run failed (exit 3); not compared"), "{s}");
    assert!(!s.contains("fixed"), "{s}");
    let c = stdout(&sonde(
        t.path(),
        &["show", "etl-6-crashed", "--since", "etl-5-undone"],
    ));
    assert!(
        c.contains("a run compared failed (exit 3); their findings are not compared"),
        "{c}"
    );
    assert!(!c.contains("✓ fixed"), "{c}");
}

/// A finding the project keeps on purpose carries why in every output of
/// `show` and `check`: an agent and a reviewer read it and leave it.
#[test]
fn an_expected_finding_says_why_in_every_output() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("data/runs");
    std::fs::create_dir_all(&dir).unwrap();
    let run = dir.join("etl-1-base.sonde");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../analysis/tests/fixtures/etl-1-base.sonde"),
        &run,
    )
    .unwrap();
    let why = "one at a time on purpose: the store limits us";
    std::fs::write(
        t.path().join("sonde.toml"),
        format!(
            "[program]\nname = \"etl\"\nruns = \"etl\"\n[check]\nmax-requests = 100000\nexpect = {{ \"obs.serial\" = {why:?} }}\n"
        ),
    )
    .unwrap();
    let here = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_sonde"))
            .args(args)
            .current_dir(t.path())
            .env("SONDE_CONFIG_DIR", t.path().join("config"))
            .env("SONDE_DATA_DIR", t.path().join("data"))
            .no_aws(t.path())
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", stderr(&o));
        stdout(&o)
    };
    let file = run.to_str().unwrap();
    let shown: serde_json::Value =
        serde_json::from_str(&here(&["show", file, "-f", "json"])).unwrap();
    // on its diagnosis, by identity: only that one
    let expected: Vec<&serde_json::Value> = shown["diagnoses"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d.get("expected"))
        .collect();
    assert_eq!(expected.len(), 1, "{}", shown["diagnoses"]);
    assert_eq!(expected[0].as_object().unwrap().len(), 1, "{expected:?}");
    assert_eq!(expected[0]["obs.serial|"], why);
    assert!(here(&["show", file]).contains(&format!("expected: {why}")));
    let checked: serde_json::Value =
        serde_json::from_str(&here(&["check", file, "-f", "json"])).unwrap();
    assert_eq!(checked["check"]["expected"][0]["expected"], why);
    assert_eq!(checked["check"]["expected"][0]["identity"], "obs.serial|");
    assert!(here(&["check", file]).contains(&format!("(expected: {why})")));
    let md = here(&["check", file, "-f", "markdown"]);
    assert!(md.contains("**Expected** (sonde.toml)"), "{md}");
    assert!(md.contains(&format!("expected: {why}")), "{md}");
    // a warning kept on purpose is a notice, never a warning
    let gh = here(&["check", file, "-f", "github"]);
    let line = gh.lines().find(|l| l.contains("(expected: ")).unwrap();
    assert!(line.starts_with("::notice "), "{gh}");
    assert!(line.contains(why), "{gh}");
}

/// `[check] expect` names findings by identity: words of a finding's text,
/// or a kind sonde does not find, are a mistake in sonde.toml.
#[test]
fn an_expect_that_is_no_findings_identity_is_a_usage_error() {
    let t = tempfile::tempdir().unwrap();
    let run = run_of(
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
            format!("[check]\nmax-requests = 100000\nexpect = {{ {expect:?} = \"kept\" }}\n"),
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
    // no latency in CloudTrail: no p95, no waiting (a run of a few
    // requests has its table with --all)
    let run = std::fs::read_dir(t.path().join("data/runs"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "sonde"))
        .unwrap();
    let table = stdout(&sonde(t.path(), &["show", run.to_str().unwrap(), "--all"]));
    let row = table.lines().find(|l| l.starts_with("GetObject ")).unwrap();
    assert!(row.contains('—') && !row.contains(" ms"), "{table}");
    // an access log to the second: its waiting would be sonde's spreading
    let log = t.path().join("two.log");
    std::fs::write(&log, A_LOG_LINE.repeat(2)).unwrap();
    let o = sonde(t.path(), &["import", log.to_str().unwrap()]);
    let shown = stderr(&o);
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
    let o = sonde(t.path(), &["layout", "--dir", dir, "--kind", "slatedb"]);
    assert_eq!(o.status.code(), Some(4), "{}", stderr(&o));
    assert!(
        stderr(&o).contains("is not a SlateDB layout"),
        "{}",
        stderr(&o)
    );
    // any prefix has a write cadence
    let o = sonde(t.path(), &["layout", "--dir", dir, "--kind", "generic"]);
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

/// `sonde` run from the folder `cwd`, its data and settings in `dirs`.
/// A program in `cwd` that sends one request through sonde, signed as
/// sonde told it (`sh get.sh`): a run that reached sonde.
fn one_request(cwd: &Path) -> [&'static str; 2] {
    std::fs::write(
        cwd.join("get.sh"),
        "curl -s -o /dev/null --aws-sigv4 aws:amz:us-east-1:s3 --user \"$AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY\" \"$AWS_ENDPOINT_URL/demo/reports/2026-q1.csv\"\n",
    )
    .unwrap();
    ["sh", "get.sh"]
}

fn sonde_in(dirs: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonde"))
        .args(args)
        .current_dir(cwd)
        .env("SONDE_CONFIG_DIR", dirs.join("config"))
        .env("SONDE_DATA_DIR", dirs.join("data"))
        .no_aws(dirs)
        .output()
        .unwrap()
}

/// 24.0's `check` alone passed over a run written with `-o` from this very
/// folder and checked another program's run: a run the runs folder lists
/// is a run like its own.
#[test]
fn check_alone_takes_a_run_written_elsewhere_from_this_folder() {
    let t = tempfile::tempdir().unwrap();
    let (r, b, out) = (t.path().join("r"), t.path().join("b"), t.path().join("out"));
    for d in [&r, &b, &out] {
        std::fs::create_dir_all(d).unwrap();
    }
    // another program's run, in the runs folder
    let o = sonde_in(t.path(), &b, &["run", "--demo", "--", "sh", "-c", "exit 0"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let mv = out.join("mv.sonde");
    let o = sonde_in(
        t.path(),
        &r,
        &[
            &["run", "--demo", "-o", mv.to_str().unwrap(), "--"][..],
            &one_request(&r),
        ]
        .concat(),
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let ck = sonde_in(t.path(), &r, &["check", "--max-requests", "100"]);
    assert_eq!(ck.status.code(), Some(0), "{}{}", stdout(&ck), stderr(&ck));
    assert!(
        stderr(&ck).contains("the newest run of program `get` recorded in this folder"),
        "{}",
        stderr(&ck)
    );
    // and by its name in sonde.toml
    std::fs::write(r.join("sonde.toml"), "[program]\nname = \"get\"\n").unwrap();
    let shown = sonde_in(t.path(), &r, &["show", "-f", "json"]);
    assert_eq!(shown.status.code(), Some(0), "{}", stderr(&shown));
    let doc: serde_json::Value = serde_json::from_str(&stdout(&shown)).unwrap();
    assert_eq!(doc["run"]["command"], "sh get.sh", "{doc}");
}

/// 19.0 passed `check --program-exit 3` "within budget": a run whose
/// program failed fails its check in every format, by the exit the run
/// kept; and it is no baseline, nor `--init`'s source.
#[test]
fn check_fails_a_run_whose_program_failed() {
    let t = tempfile::tempdir().unwrap();
    let w = t.path().join("w");
    std::fs::create_dir_all(&w).unwrap();
    let ok = sonde_in(t.path(), &w, &["run", "--demo", "--", "true"]);
    assert_eq!(ok.status.code(), Some(0), "{}", stderr(&ok));
    let budget = ["--max-requests", "100"];
    // the flag is gone: the run keeps the exit
    let c = sonde_in(
        t.path(),
        &w,
        &[&["check", "--program-exit", "3"][..], &budget].concat(),
    );
    assert_eq!(c.status.code(), Some(64), "{}", stdout(&c));
    // the run kept that its program failed
    let failed = sonde_in(t.path(), &w, &["run", "--demo", "--", "sh", "-c", "exit 3"]);
    assert_eq!(failed.status.code(), Some(3), "{}", stderr(&failed));
    for format in ["text", "json", "markdown", "github"] {
        let c = sonde_in(
            t.path(),
            &w,
            &[&["check", "-f", format][..], &budget].concat(),
        );
        assert_eq!(c.status.code(), Some(1), "{format}: {}", stdout(&c));
        assert!(
            stdout(&c).contains("the program failed (exit 3)"),
            "{format}: {}",
            stdout(&c)
        );
        if format == "markdown" {
            assert!(
                stdout(&c).contains("> ❌ **The program failed** (exit 3)"),
                "{}",
                stdout(&c)
            );
        }
        if format == "json" {
            let j: serde_json::Value = serde_json::from_str(&stdout(&c)).unwrap();
            assert_eq!(j["passed"], false);
            assert_eq!(j["programExit"], 3);
            assert!(j["reason"].as_str().unwrap().contains("exit 3"), "{j}");
        }
    }
    // without a budget too: the exit is what it is held to
    let c = sonde_in(t.path(), &w, &["check"]);
    assert_eq!(c.status.code(), Some(1), "{}{}", stdout(&c), stderr(&c));
    for refused in [&["check", "--set-baseline"][..], &["check", "--init"]] {
        let c = sonde_in(t.path(), &w, refused);
        assert_eq!(c.status.code(), Some(64), "{refused:?}: {}", stderr(&c));
        assert!(
            stderr(&c).contains("the run failed (exit 3)"),
            "{}",
            stderr(&c)
        );
    }
    assert!(!w.join("sonde.toml").exists());
}

/// 19.0's `check` alone took the newest run of any program: another
/// program's run passed this one's budget. It takes the newest run of
/// `[program] name`, else of a program recorded in this folder, and says
/// which; never another's.
#[test]
fn check_alone_takes_this_programs_newest_run_never_anothers() {
    let t = tempfile::tempdir().unwrap();
    let (a, b, c) = (t.path().join("a"), t.path().join("b"), t.path().join("c"));
    for d in [&a, &b, &c] {
        std::fs::create_dir_all(d).unwrap();
    }
    let r = sonde_in(
        t.path(),
        &a,
        &[&["run", "--demo", "--"][..], &one_request(&a)].concat(),
    );
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    // another program, later, elsewhere
    let r = sonde_in(t.path(), &b, &["run", "--demo", "--", "sh", "-c", "exit 3"]);
    assert_eq!(r.status.code(), Some(3));
    let ck = sonde_in(t.path(), &a, &["check", "--max-requests", "100"]);
    assert_eq!(ck.status.code(), Some(0), "{}{}", stdout(&ck), stderr(&ck));
    assert!(
        stderr(&ck).contains("the newest run of program `get` recorded in this folder"),
        "{}",
        stderr(&ck)
    );
    // (named once, first in what check writes)
    let taken = stdout(&ck)
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("run "))
        .and_then(|l| l.split(" · ").next())
        .unwrap()
        .to_string();
    assert!(
        !stderr(&ck).contains(&taken),
        "the path said twice: {}",
        stderr(&ck)
    );
    // show and share take the same run: this folder's, not the newest
    let shown = sonde_in(t.path(), &a, &["show", "-f", "json"]);
    let doc: serde_json::Value = serde_json::from_str(&stdout(&shown)).unwrap();
    assert_eq!(doc["run"]["command"], "sh get.sh", "{doc}");
    // nothing recorded here: none taken (27.0 took the newest of any
    // program, said only on stderr), and what to do
    for cmd in [&["check", "--max-requests", "100"][..], &["show"]] {
        let o = sonde_in(t.path(), &c, cmd);
        assert_eq!(o.status.code(), Some(64), "{cmd:?}: {}", stdout(&o));
        let err = stderr(&o);
        assert!(
            err.contains("no run recorded in this folder")
                && err.contains("program `sh`")
                && err.contains("`sonde runs` lists them"),
            "{err}"
        );
    }
    // and this folder's sonde.toml is not the budget of another program's run
    std::fs::write(c.join("sonde.toml"), "[check]\nmax-requests = 0\n").unwrap();
    let runs: serde_json::Value =
        serde_json::from_str(&stdout(&sonde_in(t.path(), &c, &["runs", "-f", "json"]))).unwrap();
    let other = runs
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["program"] == "sh")
        .and_then(|r| r["path"].as_str())
        .unwrap_or_else(|| panic!("{runs}"))
        .to_string();
    let ck = sonde_in(t.path(), &c, &["check", &other, "--max-requests", "100"]);
    let err = stderr(&ck);
    assert!(
        err.contains("recorded outside the folder of")
            && err.contains("its [check] is not applied"),
        "{err}"
    );
    assert!(!stdout(&ck).contains("≤ 0"), "{}", stdout(&ck));
    assert_eq!(ck.status.code(), Some(1), "the run failed: {}", stdout(&ck));
    std::fs::remove_file(c.join("sonde.toml")).unwrap();
    // a project's name for its program: given to the commands that run it,
    // not to another command run here
    std::fs::write(
        c.join("sonde.toml"),
        "[program]\nname = \"fetch\"\nruns = \"get\"\n",
    )
    .unwrap();
    let r = sonde_in(
        t.path(),
        &c,
        &[&["run", "--demo", "--"][..], &one_request(&c)].concat(),
    );
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    let r = sonde_in(t.path(), &c, &["run", "--demo", "--", "sh", "-c", "exit 0"]);
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    let listed: serde_json::Value =
        serde_json::from_str(&stdout(&sonde(t.path(), &["runs", "-f", "json"]))).unwrap();
    assert_eq!(
        (listed[0]["program"].as_str(), listed[1]["program"].as_str()),
        (Some("sh"), Some("fetch")),
        "{listed}"
    );
    let ck = sonde_in(t.path(), &c, &["check", "--max-requests", "100"]);
    assert_eq!(ck.status.code(), Some(0), "{}", stderr(&ck));
    assert!(
        stderr(&ck).contains("the newest run of program \"fetch\" ([program] name in sonde.toml)"),
        "{}",
        stderr(&ck)
    );
    std::fs::write(
        c.join("sonde.toml"),
        "[program]\nname = \"etl\"\nruns = \"etl\"\n",
    )
    .unwrap();
    let ck = sonde_in(t.path(), &c, &["check", "--max-requests", "100"]);
    assert_eq!(ck.status.code(), Some(64));
    assert!(
        stderr(&ck).contains("no run of program \"etl\""),
        "{}",
        stderr(&ck)
    );
}

/// `--init` names the action at this sonde's major version.
#[test]
fn check_init_names_the_action_at_this_major_version() {
    let t = tempfile::tempdir().unwrap();
    let r = sonde_in(
        t.path(),
        t.path(),
        &[&["run", "--demo", "--"][..], &one_request(t.path())].concat(),
    );
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    let c = sonde_in(t.path(), t.path(), &["check", "--init"]);
    assert_eq!(c.status.code(), Some(0), "{}", stderr(&c));
    let major = env!("CARGO_PKG_VERSION").split('.').next().unwrap();
    assert!(
        stderr(&c).contains(&format!("uses: hxddh/sonde@v{major}\n")),
        "{}",
        stderr(&c)
    );
}

/// The program that reads one hour's partition, as an hourly job does.
const HOURLY: &str = r#"
import os, boto3
s3 = boto3.client("s3")
hour = os.environ["H"]
key = f"in/year=2026/month=11/day=13/hour={hour}/conf.json"
s3.put_object(Bucket="demo", Key=key, Body=b"{}")
for _ in range(20):
    s3.get_object(Bucket="demo", Key=key)["Body"].read()
"#;

/// An hourly job run twice, unchanged but for its hour: "no change", no
/// finding fixed or new.
#[test]
fn an_hourly_job_run_twice_unchanged_reads_no_change() {
    let has_boto3 = Command::new("python3")
        .args(["-c", "import boto3"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_boto3 {
        eprintln!("skipped: no python3 with boto3");
        return;
    }
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("hourly.py"), HOURLY).unwrap();
    let run = |hour: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_sonde"))
            .args(["run", "--demo", "--", "python3", "hourly.py"])
            .current_dir(t.path())
            .env("SONDE_CONFIG_DIR", t.path().join("config"))
            .env("SONDE_DATA_DIR", t.path().join("data"))
            .env("AWS_EC2_METADATA_DISABLED", "true")
            .env("H", hour)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
        format!("{}{}", stdout(&o), stderr(&o))
    };
    let first = run("10");
    assert!(
        first.contains("hour=10/conf.json was read 20 times"),
        "{first}"
    );
    let second = run("11");
    assert!(second.contains("since the run before"), "{second}");
    assert!(second.contains("requests 21 (no change)"), "{second}");
    for not in ["✓ fixed", "! new", "↓ less", "↑ more"] {
        assert!(!second.contains(not), "{not} in:\n{second}");
    }
}

/// A key planted in a log with ESC and BEL (a window title set) reaches no
/// terminal as they are: every command writes them out.
#[test]
fn a_keys_control_characters_are_written_out_never_sent() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("planted.log");
    let line = A_LOG_LINE
        .replace("t/a", "t/%1B%5D0%3Bpwned%07a")
        .replace("boto3/1.34", "boto3/1.34\u{1b}[2J");
    std::fs::write(&log, line.repeat(3)).unwrap();
    let run = t.path().join("planted.sonde");
    let s = run.to_str().unwrap();
    let mut seen = vec![sonde(t.path(), &["import", log.to_str().unwrap(), "-o", s])];
    for args in [
        vec!["show", s],
        vec!["show", "--requests", s],
        vec!["show", "--all", s],
        vec!["show", "--by", "prefix", s],
        vec!["runs"],
        vec!["show", s, "--since", s],
        vec!["check", s],
    ] {
        seen.push(sonde(t.path(), &args));
    }
    let mut written_out = false;
    for o in &seen {
        for text in [stdout(o), stderr(o)] {
            assert!(
                !text.contains('\u{1b}') && !text.contains('\u{7}'),
                "a control character reached the terminal: {text:?}"
            );
            written_out |= text.contains("\\e]0;pwned\\x07a");
        }
    }
    assert!(written_out, "the key is shown, written out");
}

fn runs_in(dirs: &Path) -> usize {
    std::fs::read_dir(dirs.join("data/runs"))
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".sonde"))
                .count()
        })
        .unwrap_or(0)
}

/// What `run` cannot do is a usage error before anything runs: a flag of
/// its own mistyped is never taken for the program, and no endpoint is the
/// configuration's mistake. A command that cannot start leaves no run.
#[test]
fn run_refuses_a_mistyped_flag_and_no_endpoint_and_records_nothing_it_did_not_run() {
    let t = tempfile::tempdir().unwrap();
    let o = sonde(t.path(), &["run", "--demo", "--typo", "--", "true"]);
    assert_eq!(o.status.code(), Some(64), "{}", stderr(&o));
    assert!(
        stderr(&o).contains("unknown option --typo"),
        "{}",
        stderr(&o)
    );
    let o = sonde(t.path(), &["run", "--", "true"]);
    assert_eq!(o.status.code(), Some(64), "{}", stderr(&o));
    assert!(
        stderr(&o).contains("no endpoint configured"),
        "{}",
        stderr(&o)
    );
    let o = sonde(t.path(), &["run", "--demo", "--", "/no/such/program"]);
    assert_eq!(o.status.code(), Some(70), "{}", stderr(&o));
    assert!(stderr(&o).contains("cannot start"), "{}", stderr(&o));
    assert_eq!(runs_in(t.path()), 0, "no run of what never ran");
    // a program's own flags after its name are its own
    let o = sonde(t.path(), &["run", "--demo", "--", "sh", "-c", "exit 0"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
}

/// `-o FILE` refuses a file that is there unless `--force`, and two runs
/// given one file each print their own report and keep their own run.
#[test]
fn run_o_refuses_a_file_there_and_two_runs_at_once_keep_their_own() {
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("run.sonde");
    std::fs::write(&out, "mine").unwrap();
    let o_ = out.to_str().unwrap();
    let o = sonde(t.path(), &["run", "--demo", "-o", o_, "--", "true"]);
    assert_eq!(o.status.code(), Some(64), "{}", stderr(&o));
    assert!(stderr(&o).contains("--force"), "{}", stderr(&o));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "mine");
    let o = sonde(
        t.path(),
        &["run", "--demo", "-o", o_, "--force", "--", "true"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_ne!(std::fs::read(&out).unwrap(), b"mine");
    // the same for every command that writes a file it is given
    let log = t.path().join("access.log");
    std::fs::write(&log, A_LOG_LINE).unwrap();
    let mine = t.path().join("mine.txt");
    let m_ = mine.to_str().unwrap();
    for args in [
        vec!["import", log.to_str().unwrap(), "-o", m_],
        vec!["share", o_, "-o", m_],
    ] {
        std::fs::write(&mine, "mine").unwrap();
        let o = sonde(t.path(), &args);
        assert_eq!(o.status.code(), Some(64), "{args:?}: {}", stderr(&o));
        assert!(
            stderr(&o).contains("--force writes over it"),
            "{}",
            stderr(&o)
        );
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), "mine", "{args:?}");
        let forced = [args.clone(), vec!["--force"]].concat();
        let o = sonde(t.path(), &forced);
        // (a build without the viewer makes no page: it leaves the file as it was)
        if stderr(&o).contains("no web viewer") {
            assert_eq!(std::fs::read_to_string(&mine).unwrap(), "mine");
            continue;
        }
        assert_eq!(o.status.code(), Some(0), "{forced:?}: {}", stderr(&o));
        assert_ne!(std::fs::read_to_string(&mine).unwrap_or_default(), "mine");
    }
    // an import with nothing in it is bad data
    let empty = t.path().join("empty.log");
    std::fs::write(&empty, "\n").unwrap();
    let o = sonde(t.path(), &["import", empty.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(65), "{}", stderr(&o));
    // two at once, one file
    let both = t.path().join("both.sonde");
    let b_ = both.to_str().unwrap().to_string();
    let runs: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|word| {
            let b_ = b_.clone();
            let dirs = t.path().to_path_buf();
            std::thread::spawn(move || {
                let script = format!("sleep 0.5; echo {word}");
                sonde(
                    &dirs,
                    &["run", "--demo", "-o", &b_, "--", "sh", "-c", &script],
                )
            })
        })
        .collect();
    let seen: Vec<String> = runs
        .into_iter()
        .map(|r| {
            let o = r.join().unwrap();
            assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
            let err = stderr(&o);
            err.lines()
                .find_map(|l| l.strip_prefix("run: "))
                .unwrap()
                .to_string()
        })
        .collect();
    assert_ne!(seen[0], seen[1], "each its own run: {seen:?}");
    for s in &seen {
        assert!(Path::new(s).is_file(), "{s}");
    }
    let left: Vec<String> = std::fs::read_dir(t.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".recording"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// sonde told to end (SIGTERM) ends its program and says the code it exits
/// with: one code, 143, in the message and the exit.
#[cfg(unix)]
#[test]
fn a_run_ended_by_sigterm_says_the_code_it_exits_with() {
    let t = tempfile::tempdir().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_sonde"))
        .args(["run", "--demo", "--", "sleep", "30"])
        .env("SONDE_CONFIG_DIR", t.path().join("config"))
        .env("SONDE_DATA_DIR", t.path().join("data"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(143), "{}", stderr(&o));
    assert!(
        stderr(&o).contains("sonde was sent SIGTERM: the program was stopped (exit 143)"),
        "{}",
        stderr(&o)
    );
    // (not killed: no exit 137 said; the run's name holds the time, its digits any)
    assert!(!stderr(&o).contains("exit 137"), "{}", stderr(&o));
}

/// `run --listen`: the endpoint, standing, until Ctrl-C; then its run.
#[cfg(unix)]
#[test]
fn run_listen_serves_until_ctrl_c_and_keeps_its_run() {
    use std::io::Read;
    let t = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr = format!("127.0.0.1:{port}");
    let mut child = Command::new(env!("CARGO_BIN_EXE_sonde"))
        .args(["run", "--listen", &addr, "--url", "http://127.0.0.1:9"])
        .env("SONDE_CONFIG_DIR", t.path().join("config"))
        .env("SONDE_DATA_DIR", t.path().join("data"))
        .no_aws(t.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // listening: it says how to point a program at it
    let mut said = String::new();
    let mut err = child.stderr.take().unwrap();
    let mut buf = [0u8; 4096];
    while !said.contains("Ctrl-C stops") {
        let n = err.read(&mut buf).unwrap();
        assert!(n > 0, "{said}");
        said.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    assert!(
        said.contains(&format!("listening on http://{addr}")),
        "{said}"
    );
    assert!(said.contains("export AWS_ENDPOINT_URL="), "{said}");
    std::net::TcpStream::connect(&addr).expect("it listens");
    let killed = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    assert_eq!(runs_in(t.path()), 1);
}

/// A CloudTrail file cut short is bad data (65), said where it broke.
#[test]
fn a_cloudtrail_file_cut_short_is_bad_data_said_where() {
    let t = tempfile::tempdir().unwrap();
    let trail = t.path().join("trail.json");
    std::fs::write(
        &trail,
        r#"{"Records":[{"eventTime":"2031-02-06T00:00:01Z","eventSource":"s3.amazonaws.com","eventName":"GetObject","requestParameters":{"bucketName":"lake","key":"a"}},{"eventTime":"2031-02-06T00:00:02Z","eventSo"#,
    )
    .unwrap();
    let o = sonde(t.path(), &["import", trail.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(65), "{}", stderr(&o));
    assert!(stderr(&o).contains("at line 1, column"), "{}", stderr(&o));
}

/// An imported log's what-if says no waiting: the import says it is not known.
#[test]
fn an_imported_logs_what_if_says_no_waiting_it_does_not_know() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("headget.log");
    let mut text = String::new();
    for i in 1..=8 {
        for (op, method) in [("REST.HEAD.OBJECT", "HEAD"), ("REST.GET.OBJECT", "GET")] {
            text.push_str(&format!(
                "79a5 lake [06/Feb/2031:00:00:3{i} +0000] 192.0.2.3 - {method}{i} {op} t/k{i} \"{method} /lake/t/k{i} HTTP/1.1\" 200 - 4096 4096 12 10 \"-\" \"boto3/1.34\" - x SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -\n"
            ));
        }
    }
    std::fs::write(&log, text).unwrap();
    let run = t.path().join("headget.sonde");
    let s = run.to_str().unwrap();
    let o = sonde(t.path(), &["import", log.to_str().unwrap(), "-o", s]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    // one diagnosis: its saving is the run's, said as requests (no times)
    let shown = stdout(&sonde(t.path(), &["show", s]));
    let line = shown
        .lines()
        .find(|l| l.starts_with("    saving:"))
        .unwrap_or_else(|| panic!("{shown}"));
    assert_eq!(line, "    saving: 8 fewer requests (50% of the run)");
    assert!(!shown.contains(" ms of "), "{shown}");
    let j: serde_json::Value =
        serde_json::from_str(&stdout(&sonde(t.path(), &["show", s, "-f", "json"]))).unwrap();
    assert!(j["whatIf"]["waitingMs"].is_null(), "{}", j["whatIf"]);
    assert_eq!(j["whatIf"]["requests"], serde_json::json!([16, 8]));
}

/// A run written elsewhere with `-o` (as agents.md advises) is listed with
/// the data directory's, by its path: people see an agent's runs. It is the
/// writer's to keep (never pruned), and let go once it is gone.
#[test]
fn runs_written_elsewhere_are_listed_by_their_path_and_never_pruned() {
    let t = tempfile::tempdir().unwrap();
    let agent = t.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let out = agent.join("job.sonde");
    // (--force: the run is written whole at its end, then renamed into place)
    std::fs::write(&out, b"").unwrap();
    let o = sonde(
        t.path(),
        &["run", "--demo", "-o", out.to_str().unwrap(), "--force"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let listed = runs_json(t.path());
    // the tour's two runs: the job and the job fixed, beside it
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert!(listed.iter().all(|r| r["elsewhere"] == true), "{listed:?}");
    let written = out.canonicalize().unwrap();
    assert!(
        listed
            .iter()
            .any(|r| r["path"] == written.to_str().unwrap() && r["name"] == r["path"]),
        "{listed:?}"
    );
    assert!(listed.iter().all(|r| r["requests"].as_u64() > Some(0)));
    // each named by the client that sent it (the tour plays boto3)
    assert!(
        listed
            .iter()
            .all(|r| r["clients"] == serde_json::json!(["boto3"])),
        "{listed:?}"
    );
    let table = stdout(&sonde(t.path(), &["runs"]));
    assert!(table.contains(written.to_str().unwrap()), "{table}");
    assert!(
        table.lines().next().is_some_and(|h| h.contains(" client ")),
        "{table}"
    );
    // a program's run, recorded beside its file and put in its place at the end
    let two = agent.join("two.sonde");
    let o = sonde(
        t.path(),
        &[
            "run",
            "--demo",
            "-o",
            two.to_str().unwrap(),
            "--force",
            "--",
            "true",
        ],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let listed = runs_json(t.path());
    assert_eq!(listed.len(), 3, "{listed:?}");
    let two = two.canonicalize().unwrap();
    assert!(
        listed.iter().any(|r| r["path"] == two.to_str().unwrap()),
        "{listed:?}"
    );
    std::fs::remove_file(&two).unwrap();
    // never pruned: the files are the agent's
    let pruned = sonde(t.path(), &["runs", "prune", "--keep", "0", "--yes"]);
    assert_eq!(pruned.status.code(), Some(0), "{}", stderr(&pruned));
    assert!(out.is_file());
    // gone from the disk: gone from the list
    std::fs::remove_file(&out).unwrap();
    assert_eq!(runs_json(t.path()).len(), 1);
}

/// An import from several principals says how many callers sent it, not the
/// SDK most of them used; one from a single principal names its SDK.
#[test]
fn an_import_from_several_callers_says_how_many() {
    let t = tempfile::tempdir().unwrap();
    let line = |second: u32, who: &str, ua: &str| {
        format!(
            "79a5 lake [06/Feb/2026:00:00:{second:02} +0000] 192.0.2.3 arn:aws:iam::1:role/{who} A{second} REST.GET.OBJECT t/k \"GET /lake/t/k HTTP/1.1\" 200 - 4096 4096 12 10 \"-\" \"{ua}\" - x SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -\n"
        )
    };
    let log = t.path().join("access.log");
    let many: String = [
        line(1, "etl", "aws-sdk-java/2.29"),
        line(2, "etl", "aws-sdk-java/2.29"),
        line(3, "dashboard", "Boto3/1.44"),
        line(4, "ingest", "aws-sdk-go-v2/1.32"),
    ]
    .concat();
    std::fs::write(&log, many).unwrap();
    let out = t.path().join("many.sonde");
    let o = sonde(
        t.path(),
        &["import", log.to_str().unwrap(), "-o", out.to_str().unwrap()],
    );
    let said = format!("{}{}", stdout(&o), stderr(&o));
    assert_eq!(o.status.code(), Some(0), "{said}");
    assert!(said.contains("4 requests from 3 callers to "), "{said}");
    let shown = stdout(&sonde(t.path(), &["show", out.to_str().unwrap()]));
    assert!(shown.contains("4 requests from 3 callers to "), "{shown}");
    let split = stdout(&sonde(
        t.path(),
        &["show", out.to_str().unwrap(), "--by", "kind"],
    ));
    assert!(split.contains(" from 3 callers to "), "{split}");
    std::fs::write(&log, line(1, "etl", "aws-sdk-java/2.29")).unwrap();
    let one = t.path().join("one.sonde");
    let o = sonde(
        t.path(),
        &["import", log.to_str().unwrap(), "-o", one.to_str().unwrap()],
    );
    let said = format!("{}{}", stdout(&o), stderr(&o));
    assert!(
        said.contains("1 request from aws-sdk-java 2.29 to "),
        "{said}"
    );
}

/// 27.0: a baseline not there, or at http://, was exit 70 with nothing to
/// do; `--set-baseline` refused a run written with -o, though `no baseline
/// yet` sends one there; a negative tolerance was taken; `--baseline none`
/// was a file of that name.
#[test]
fn check_s_baseline_and_tolerance_mistakes_are_usage_errors() {
    let t = tempfile::tempdir().unwrap();
    let run = t.path().join("x.sonde");
    let x = run.to_str().unwrap();
    let r = sonde_in(
        t.path(),
        t.path(),
        &[
            &["run", "--demo", "-o", x, "--"][..],
            &one_request(t.path()),
        ]
        .concat(),
    );
    assert_eq!(r.status.code(), Some(0), "{}", stderr(&r));
    let missing = sonde_in(
        t.path(),
        t.path(),
        &["check", x, "--baseline", "nope.sonde"],
    );
    assert_eq!(missing.status.code(), Some(64), "{}", stderr(&missing));
    assert!(
        stderr(&missing).contains("no run nope.sonde")
            && stderr(&missing).contains("`sonde runs` lists them"),
        "{}",
        stderr(&missing)
    );
    let http = sonde_in(
        t.path(),
        t.path(),
        &["check", x, "--baseline", "http://example.test/b.sonde"],
    );
    assert_eq!(http.status.code(), Some(64), "{}", stderr(&http));
    assert!(stderr(&http).contains("only https"), "{}", stderr(&http));
    for bad in ["-5", "NaN", "inf"] {
        let o = sonde_in(
            t.path(),
            t.path(),
            &["check", x, "--baseline", x, &format!("--tolerance={bad}")],
        );
        assert_eq!(o.status.code(), Some(64), "{bad}: {}", stderr(&o));
    }
    std::fs::write(t.path().join("sonde.toml"), "[check]\ntolerance = -1\n").unwrap();
    let o = sonde_in(t.path(), t.path(), &["check", x, "--baseline", x]);
    assert_eq!(o.status.code(), Some(64), "{}", stderr(&o));
    assert!(stderr(&o).contains("0 or more"), "{}", stderr(&o));
    std::fs::remove_file(t.path().join("sonde.toml")).unwrap();
    // a run written with -o is a baseline as one in the runs folder is
    let set = sonde_in(
        t.path(),
        t.path(),
        &["check", x, "--set-baseline", "--max-requests", "100"],
    );
    assert_eq!(set.status.code(), Some(0), "{}", stderr(&set));
    std::fs::write(
        t.path().join("sonde.toml"),
        "[check]\nbaseline = \"auto\"\nmax-requests = 100\n",
    )
    .unwrap();
    let against = |extra: &[&str]| -> serde_json::Value {
        let o = sonde_in(
            t.path(),
            t.path(),
            &[&["check", x, "-f", "json"][..], extra].concat(),
        );
        assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
        serde_json::from_slice(&o.stdout).unwrap()
    };
    let held = against(&[]);
    assert!(held["noBaseline"].is_null(), "{held}");
    let baseline_rows = |v: &serde_json::Value| {
        v["check"]["verdicts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["against"] == "baseline")
            .count()
    };
    assert!(baseline_rows(&held) > 0, "{held}");
    // `none`: no baseline, sonde.toml's neither
    let alone = against(&["--baseline", "none"]);
    assert_eq!(baseline_rows(&alone), 0, "{alone}");
    assert!(alone["noBaseline"].is_null(), "{alone}");
}

/// 27.0's tour wrote over `x-fixed.sonde` beside `-o x.sonde` unasked.
#[test]
fn the_tour_refuses_a_fixed_run_that_is_there() {
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("x.sonde");
    std::fs::write(t.path().join("x-fixed.sonde"), "mine").unwrap();
    let o = sonde(t.path(), &["run", "--demo", "-o", out.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(64), "{}", stderr(&o));
    assert!(stderr(&o).contains("x-fixed.sonde"), "{}", stderr(&o));
    assert_eq!(
        std::fs::read_to_string(t.path().join("x-fixed.sonde")).unwrap(),
        "mine"
    );
    assert!(!out.exists());
    let o = sonde(
        t.path(),
        &["run", "--demo", "-o", out.to_str().unwrap(), "--force"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(out.is_file());
}

/// 27.0 stored us-east-1 for every endpoint added without --region.
#[test]
fn an_endpoint_added_takes_its_url_s_region_else_the_environment_s() {
    let t = tempfile::tempdir().unwrap();
    let region_of = |id: &str| -> String {
        let v: serde_json::Value =
            serde_json::from_slice(&sonde(t.path(), &["endpoints", "list", "-f", "json"]).stdout)
                .unwrap();
        v.as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == id)
            .and_then(|e| e["region"].as_str())
            .unwrap_or_else(|| panic!("{v}"))
            .to_string()
    };
    let add = |args: &[&str], env: Option<&str>| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sonde"));
        c.args(args)
            .env("SONDE_CONFIG_DIR", t.path().join("config"))
            .env("SONDE_DATA_DIR", t.path().join("data"))
            .no_aws(t.path());
        if let Some(r) = env {
            c.env("AWS_REGION", r);
        }
        let o = c.output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    };
    add(
        &[
            "endpoints",
            "add",
            "eu",
            "https://s3.eu-west-1.amazonaws.com",
        ],
        Some("ap-south-1"),
    );
    assert_eq!(region_of("eu"), "eu-west-1");
    add(
        &["endpoints", "add", "lab", "http://localhost:9000"],
        Some("ap-south-1"),
    );
    assert_eq!(region_of("lab"), "ap-south-1");
    add(
        &["endpoints", "add", "plain", "http://localhost:9000"],
        None,
    );
    assert_eq!(region_of("plain"), "us-east-1");
    add(
        &[
            "endpoints",
            "add",
            "given",
            "https://s3.eu-west-1.amazonaws.com",
            "--region",
            "eu-north-1",
        ],
        None,
    );
    assert_eq!(region_of("given"), "eu-north-1");
    // a profile alone (what `endpoints import` did): its endpoint and region
    std::fs::write(
        t.path().join("aws-config"),
        "[profile r2]\nregion = auto\nendpoint_url = https://acct.r2.cloudflarestorage.com\n",
    )
    .unwrap();
    add(&["endpoints", "add", "r2", "--profile", "r2"], None);
    assert_eq!(region_of("r2"), "auto");
    let o = sonde(t.path(), &["endpoints", "add", "x", "--profile", "nosuch"]);
    assert_eq!(o.status.code(), Some(64), "{}", stderr(&o));
    // and neither a URL nor a profile is a usage error
    let o = sonde(t.path(), &["endpoints", "add", "y"]);
    assert_eq!(o.status.code(), Some(64), "{}", stderr(&o));
}

/// 27.0's `runs prune --keep 0 --yes` deleted a run still being recorded:
/// the recording then ended on a file that was not there.
#[test]
fn prune_keeps_a_run_still_being_recorded() {
    let t = tempfile::tempdir().unwrap();
    let finished = t.path().join("data/runs").join(demo_run(t.path()));
    // not compressed yet, and written to just now: a recording's
    let live = run_of(
        t.path(),
        "live.sonde",
        "python etl.py",
        "2030-01-01T00:00:00Z",
    );
    let o = sonde(t.path(), &["runs", "prune", "--keep", "0", "--yes"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(!finished.exists() && live.is_file(), "{}", stdout(&o));
    // one left so a day ago was a recording that was killed: let go
    std::fs::File::options()
        .write(true)
        .open(&live)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 86400))
        .unwrap();
    let o = sonde(t.path(), &["runs", "prune", "--keep", "0", "--yes"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(!live.exists(), "{}", stdout(&o));
}

/// 29.0's eval: `verify` of a run written with `-o` did not write it beside
/// it again, and a run without `-o` after runs with it had `since: null`:
/// a program's runs are found the same way with `-o` or without.
#[test]
fn a_run_written_elsewhere_is_in_its_programs_history_and_verify_writes_beside_it() {
    let t = tempfile::tempdir().unwrap();
    demo_run(t.path());
    std::fs::copy(
        t.path().join("data/tour/export.py"),
        t.path().join("job.py"),
    )
    .unwrap();
    let out = t.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let run = |o: Option<&Path>| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sonde"));
        c.args(["run", "--demo", "--name", "job"]);
        if let Some(o) = o {
            c.arg("-o").arg(o);
        }
        let o = c
            .arg("--")
            .arg(env!("CARGO_BIN_EXE_sonde"))
            .args(["demo-job", "job.py"])
            .current_dir(t.path())
            .env("SONDE_CONFIG_DIR", t.path().join("config"))
            .env("SONDE_DATA_DIR", t.path().join("data"))
            .no_aws(t.path())
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    };
    let first = out.join("job1.sonde");
    run(Some(&first));
    let shown = sonde(t.path(), &["show", first.to_str().unwrap(), "-f", "json"]);
    let j: serde_json::Value = serde_json::from_str(&stdout(&shown)).unwrap();
    let verify = j["verify"].as_str().unwrap();
    assert!(
        verify.starts_with("sonde run --demo --name job -o "),
        "{verify}"
    );
    // (quoted where the shell would need it: a Windows path)
    assert!(
        verify.contains("job2.sonde -- ") || verify.contains("job2.sonde\" -- "),
        "{verify}"
    );
    // again, into the runs folder: the run written elsewhere is the one before
    run(None);
    let newest = std::fs::read_dir(t.path().join("data/runs"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sonde"))
        .max_by_key(|p| std::fs::metadata(p).unwrap().modified().unwrap())
        .unwrap();
    let shown = sonde(t.path(), &["show", newest.to_str().unwrap(), "-f", "json"]);
    let j: serde_json::Value = serde_json::from_str(&stdout(&shown)).unwrap();
    let previous = j["since"]["previous"].as_str().unwrap_or_default();
    assert!(previous.ends_with("job1.sonde"), "{}", j["since"]);
}
