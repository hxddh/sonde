//! `sonde run -- <command>`: a program's requests recorded, and its trials.

use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Args;
use sonde_core::Sonde;
use sonde_proxy::session::Session;

use crate::Cli;

use super::record::*;

#[derive(Args)]
pub struct RunArgs {
    #[command(flatten)]
    rec: Recording,
    /// Send the requests to the built-in demo store (buckets `demo`,
    /// `demo-large`, `demo-layouts`): no account or secrets needed. It lives
    /// in this process and starts afresh each run; its times are not
    /// meaningful. Without a command: a tour, a small program of sonde's own
    /// recorded and explained.
    #[arg(long, conflicts_with = "endpoint")]
    demo: bool,
    /// Print each request on stderr as it happens.
    #[arg(long)]
    live: bool,
    /// Do not name the place in the program's code that sent each request.
    /// It is named by default for Python's boto3, the AWS SDK for
    /// JavaScript v3 and the AWS SDK for Java 2 (hooks loaded with
    /// PYTHONPATH, NODE_OPTIONS, JAVA_TOOL_OPTIONS), so that each finding
    /// says its line. Also `[record] name-callers = false` in sonde.toml.
    #[arg(long)]
    no_callers: bool,
    /// Then run the command again under faults chosen from what it did (a
    /// 503 on each request's first attempt, a skewed clock if it signs, a
    /// lost answer on each write, a failed completion if it uploads in
    /// parts, a body cut short or a late first byte if it downloads, a lost
    /// race if it commits on a condition, a stale listing if it lists what it
    /// wrote), and say whether it survived each. The command runs several times, so give each run a place of
    /// its own: `{trial}` in the command is replaced by `clean`, then by
    /// each scenario's name, and the variable SONDE_TRIAL is set to the same
    /// (`-- python etl.py s3://scratch/etl-{trial}/`).
    #[arg(long)]
    trial: bool,
    /// A fault scenario to try by name (repeatable): slowdown, error, skew,
    /// lost-answer, fail-complete, complete-500, cut-body, slow-first-byte,
    /// lost-race, stale-list, slow, s3-limits, or one of sonde.toml's. `slowdown:3` and `error:3` fail each request's
    /// first three attempts rather than its first (1 to 5).
    /// `{trial}` and SONDE_TRIAL are as with --trial.
    #[arg(long = "fault", value_name = "SCENARIO")]
    faults: Vec<String>,
    /// A trial of only what did not survive (--trial is implied): a fresh
    /// clean run, then the scenarios that broke, degraded or were refused by
    /// the policy in this program's last trials (`sonde run --again -- …`).
    #[arg(long, conflicts_with = "faults")]
    again: bool,
    /// The program's name: its runs and its baseline go by it, whatever the
    /// command's arguments are this time (default: `name` under `[program]`
    /// in sonde.toml, else the command itself).
    #[arg(long)]
    name: Option<String>,
    /// With -o: write over the file if it is there (else a run refuses it).
    #[arg(long, requires = "out")]
    force: bool,
    /// The command, after `--` (none with --demo: the tour).
    #[arg(
        required_unless_present = "demo",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    command: Vec<String>,
}

/// The signal that asked sonde to stop, once one has: 2 (Ctrl-C) or 15
/// (SIGTERM). Listened for from the start of a run to its end, so that none
/// is lost between one run of a trial and the next.
#[derive(Clone)]
struct Stop(tokio::sync::watch::Receiver<i32>);

impl Stop {
    fn listen() -> Stop {
        let (tx, rx) = tokio::sync::watch::channel(0);
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let (Ok(mut int), Ok(mut term)) = (
                    signal(SignalKind::interrupt()),
                    signal(SignalKind::terminate()),
                ) else {
                    return;
                };
                loop {
                    let n = tokio::select! {
                        _ = int.recv() => 2,
                        _ = term.recv() => 15,
                    };
                    if tx.send(n).is_err() {
                        return;
                    }
                }
            }
            #[cfg(not(unix))]
            while tokio::signal::ctrl_c().await.is_ok() {
                if tx.send(2).is_err() {
                    return;
                }
            }
        });
        Stop(rx)
    }

    /// The signal received, if one was.
    fn signal(&self) -> Option<i32> {
        let n = *self.0.borrow();
        (n != 0).then_some(n)
    }

    /// Waits for the next signal.
    async fn asked(&mut self) -> i32 {
        if self.0.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
        *self.0.borrow()
    }
}

/// What every run of one `sonde run` shares.
struct RunCtx {
    /// A trial follows the clean run: `{trial}` and SONDE_TRIAL say which
    /// run each is.
    trying: bool,
    /// The program's name, when it has one.
    name: Option<String>,
    /// What the clean run read on its input, when that is not a terminal
    /// and a trial follows: every run is fed the same.
    input: Option<Arc<std::sync::Mutex<KeptInput>>>,
    stop: Stop,
    /// How it is recorded: the command line's, and sonde.toml's `[record]`.
    rec: Recording,
    /// The runs name the code that sent each request: unless
    /// `--no-callers` or `[record] name-callers = false`.
    callers: bool,
    /// What the runs wrote: a trial's runs may write again what the clean
    /// run wrote, under no-delete as under allow.
    written: Arc<sonde_proxy::Written>,
}

/// The most of a clean run's input kept in memory for the runs after it:
/// past it, it is kept in a file.
const INPUT_IN_MEMORY: usize = 8 << 20;

/// What the clean run read on its input, kept so that every run of a trial
/// is fed the same: in memory while it is small, then in a file of its own
/// (unnamed, gone when sonde ends), so that a job fed gigabytes by a pipe
/// does not hold them all.
struct KeptInput {
    limit: usize,
    head: Vec<u8>,
    file: Option<std::fs::File>,
    /// The file could not be written: the runs after are fed nothing, and
    /// are told so once.
    lost: bool,
}

impl Default for KeptInput {
    fn default() -> Self {
        KeptInput {
            limit: INPUT_IN_MEMORY,
            head: Vec::new(),
            file: None,
            lost: false,
        }
    }
}

/// A kept input, to be read once more.
enum Again {
    Bytes(Vec<u8>),
    File(std::fs::File),
    Lost,
}

impl KeptInput {
    fn push(&mut self, chunk: &[u8]) {
        use std::io::Write;
        if self.lost {
            return;
        }
        if self.file.is_none() && self.head.len() + chunk.len() > self.limit {
            match tempfile::tempfile().and_then(|mut f| f.write_all(&self.head).map(|_| f)) {
                Ok(f) => {
                    self.file = Some(f);
                    self.head = Vec::new();
                }
                Err(e) => return self.lose(e),
            }
        }
        match &mut self.file {
            Some(f) => {
                if let Err(e) = f.write_all(chunk) {
                    self.lose(e);
                }
            }
            None => self.head.extend_from_slice(chunk),
        }
    }

    fn lose(&mut self, why: std::io::Error) {
        eprintln!(
            "sonde: the program's input could not be kept for the trial's runs ({why}): they read none"
        );
        self.lost = true;
        self.file = None;
        self.head = Vec::new();
    }

    /// The input from its start: its bytes, or its file opened anew.
    fn again(&mut self) -> Again {
        use std::io::{Seek, SeekFrom};
        if self.lost {
            return Again::Lost;
        }
        match &mut self.file {
            // (runs come one after the other: the place in the file is one run's)
            Some(f) => match f.try_clone().and_then(|mut c| {
                c.seek(SeekFrom::Start(0))?;
                Ok(c)
            }) {
                Ok(c) => Again::File(c),
                Err(_) => Again::Lost,
            },
            None => Again::Bytes(self.head.clone()),
        }
    }
}

/// One run of the command.
struct Ran {
    file: PathBuf,
    /// The command's exit code.
    code: i32,
    /// How long the command ran.
    wall_ms: f64,
    /// The signal sonde was sent while it ran.
    signal: Option<i32>,
}

/// Whether the command gives each run of a trial a place of its own, as far
/// as its words say: `{trial}` in an argument, or SONDE_TRIAL named in one.
fn names_its_place(command: &[String]) -> bool {
    command
        .iter()
        .any(|w| w.contains("{trial}") || w.contains("SONDE_TRIAL"))
}

pub async fn run(s: &Arc<Sonde>, cli: &Cli, a: &RunArgs) -> anyhow::Result<i32> {
    // a word like a flag where the command starts is a flag of run's
    // mistyped, not a program (none is named `--typo`)
    if let Some(first) = a
        .command
        .first()
        .filter(|w| w.starts_with('-') && *w != "-")
    {
        return Err(super::usage(format!(
            "run: unknown option {first} (the command goes after `--`: sonde run [OPTIONS] -- COMMAND)"
        )));
    }
    if let Some(out) = &a.rec.out {
        refuse_existing(out, a.force)?;
    }
    // `--demo` alone: the tour, a program of sonde's own
    if a.demo && a.command.is_empty() {
        if cli.url.is_some() {
            return Err(super::usage("--demo and --url name two stores: give one"));
        }
        if a.trial || a.again || !a.faults.is_empty() || a.name.is_some() {
            return Err(super::usage(
                "the tour is not tried or named: give a command after -- (`sonde run --demo --trial -- python etl.py`)",
            ));
        }
        return super::demo::tour(s, a.rec.out.clone()).await;
    }
    // sonde.toml and the scenarios asked for by name are checked before
    // anything runs
    let project = load_project(None)?;
    let mut rec = a.rec.clone();
    if a.demo {
        if cli.url.is_some() {
            return Err(super::usage("--demo and --url name two stores: give one"));
        }
        let ep = sonde_core::demo::install(s)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        rec.endpoint = Some(ep.id);
        eprintln!(
            "sonde: --demo: the requests go to the built-in demo store (buckets demo, demo-large, demo-layouts), in this process. Its counts and findings are the program's; its times are not meaningful."
        );
    }
    let mut asked: Vec<sonde_proxy::fault::Scenario> = Vec::new();
    // one scenario at two strengths is two runs, each with a session of its own
    let same = |a: &sonde_proxy::fault::Scenario, b: &sonde_proxy::fault::Scenario| {
        a.name == b.name && sonde_proxy::fault::pressed(a) == sonde_proxy::fault::pressed(b)
    };
    for name in &a.faults {
        let scenario = named_scenario(name, &project.scenarios)?;
        // named twice, run once: the second run would write over the first
        if !asked.iter().any(|s| same(s, &scenario)) {
            asked.push(scenario);
        }
    }
    // what to try again is known before anything runs
    if a.again {
        // (trimmed as the name is when kept)
        let program = a
            .name
            .clone()
            .or(project.file.program.name.clone())
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .or_else(|| {
                sonde_proxy::program::program_key(Some(&sonde_proxy::program::shell_join(
                    &a.command,
                )))
            })
            .unwrap_or_default();
        let dir = match &a.rec.out {
            Some(out) => runs_dir(out),
            None => s.paths().sessions(),
        };
        let names = sonde_proxy::trial::to_try_again(&dir, &program);
        if names.is_empty() {
            let tried = sonde_proxy::trial::read_verdicts(&dir).contains_key(&program);
            if tried {
                eprintln!(
                    "Nothing to try again: every scenario of `{program}` survived its last trial, or met nothing."
                );
                return Ok(0);
            }
            return Err(super::usage(format!(
                "--again: `{program}` has no trial in {} yet (`sonde run --trial`)",
                dir.display()
            )));
        }
        for name in &names {
            match named_scenario(name, &project.scenarios) {
                Ok(scenario) if asked.iter().any(|s| same(s, &scenario)) => {}
                Ok(scenario) => asked.push(scenario),
                // a project's scenario since taken out of sonde.toml
                Err(e) => eprintln!("warning: not tried again: {e}"),
            }
        }
        if asked.is_empty() {
            return Ok(64);
        }
    }
    let trying = a.trial || a.again || !asked.is_empty();
    let ctx = RunCtx {
        trying,
        name: a.name.clone().or(project.file.program.name.clone()),
        input: (trying && !std::io::stdin().is_terminal()).then(Default::default),
        stop: Stop::listen(),
        rec: Recording {
            tls: rec.tls || project.file.record.tls,
            ..rec
        },
        callers: !a.no_callers && project.file.record.name_callers != Some(false),
        written: Arc::default(),
    };
    if trying && !names_its_place(&a.command) {
        eprintln!(
            "warning: the command has no {{trial}} and does not name SONDE_TRIAL: every run of the trial writes to the same place"
        );
    }
    let clean = run_once(s, a, None, &ctx).await?;
    let (file, code) = (clean.file.clone(), clean.code);
    if !trying {
        return Ok(if clean.signal == Some(15) { 143 } else { code });
    }
    // a trial is read against a run that worked, to its end
    if let Some(signal) = clean.signal {
        eprintln!("\nNo trial: the clean run was interrupted.");
        return Ok(if code == 0 { 128 + signal } else { code });
    }
    if code != 0 {
        eprintln!(
            "\nNo trial: the clean run exited {code}. A trial is read against a run that worked: nothing was tried."
        );
        return Ok(code);
    }
    let clean_session = Session::open(&file).map_err(|e| anyhow::anyhow!(e))?;
    if clean_session.records.is_empty() {
        eprintln!("\nNo trial: the program sent no requests to fail.");
        return Ok(code);
    }
    let scenarios = if !asked.is_empty() {
        asked
    } else if !project.scenarios.is_empty() {
        project.scenarios
    } else {
        sonde_proxy::trial::choose(&clean_session.records)
    };
    let of = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // trials are kept in a folder of their own: they are not runs of the
    // program (no trend, baseline or "newest" counts them)
    let folder = sonde_proxy::trial::folder_for(&file);
    std::fs::create_dir_all(&folder)?;
    // where the program's runs are, and its verdicts kept
    let dir = runs_dir(&file);
    let color = std::io::stderr().is_terminal();
    eprintln!(
        "\nTrial: the command runs {} more time{}, each under one scenario.",
        scenarios.len(),
        if scenarios.len() == 1 { "" } else { "s" }
    );
    let mut trials = Vec::new();
    let mut interrupted = None;
    // the program's verdicts before this trial, and this one's time: each
    // row says what changed ("broke on 12 Nov, survived since")
    let program = sonde_proxy::program::key_of(&clean_session.header);
    let kept = program
        .as_deref()
        .and_then(|p| sonde_proxy::trial::read_verdicts(&dir).remove(p))
        .unwrap_or_default();
    let now = chrono::Utc::now();
    for scenario in scenarios {
        let under = Under {
            // (the run's last trials were removed when it was recorded: the
            // name is free, and is the one the verdict keeps)
            path: folder.join(format!(
                "{}.sonde",
                sonde_proxy::trial::session_stem(&file, &scenario)
            )),
            of: of.clone(),
            scenario,
        };
        let ran = run_once(s, a, Some(&under), &ctx).await?;
        if let Some(signal) = ran.signal {
            // half a run says nothing of the program: no verdict, no session
            let _ = std::fs::remove_file(&ran.file);
            interrupted = Some((signal, under.scenario.name.clone()));
            break;
        }
        let run = Session::open(&ran.file).map_err(|e| anyhow::anyhow!(e))?;
        let mut t = sonde_proxy::trial::judge(
            &under.scenario,
            &clean_session.records,
            code,
            &run.records,
            ran.code,
        );
        // the command's own time: what a delay costs is not all in requests
        (t.wall_ms, t.clean_wall_ms) = (ran.wall_ms, clean.wall_ms);
        t.session = ran
            .file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        if program.is_some() {
            t.history = kept.get(&t.asked()).cloned().unwrap_or_default();
            t.history.push(sonde_proxy::trial::Seen {
                at: now,
                standing: t.standing,
                run: of.clone(),
            });
        }
        eprintln!("{}", trial_line(&t, color));
        // (the history is the program's, not the run's: kept apart)
        t.history.clear();
        trials.push(t);
    }
    if trials.is_empty() {
        let _ = std::fs::remove_dir(&folder);
    } else {
        sonde_proxy::trial::save(&file, &trials)?;
        if let Some(p) = &program
            && let Err(e) = sonde_proxy::trial::record(&dir, p, &of, now, &trials)
        {
            eprintln!("warning: the program's verdicts could not be kept: {e}");
        }
    }
    if let Some((signal, scenario)) = interrupted {
        eprintln!(
            "\nInterrupted under {scenario}: no verdict for it, and none for the scenarios after it."
        );
        return Ok(128 + signal);
    }
    use sonde_proxy::trial::Standing;
    let broke = trials
        .iter()
        .filter(|t| t.standing == Standing::Broke)
        .count();
    let standings: Vec<Standing> = trials.iter().map(|t| t.standing).collect();
    eprintln!("\n{}", trials_summary(&standings));
    eprintln!("Each trial is a session:");
    for t in &trials {
        if let Some(path) = sonde_proxy::trial::session_path(&file, t) {
            eprintln!("  sonde show {}", path.display());
        }
    }
    if program.is_some()
        && trials.iter().any(|t| {
            matches!(
                t.standing,
                Standing::Broke | Standing::Degraded | Standing::Refused
            )
        })
    {
        eprintln!("Once changed, try again what did not survive:");
        eprintln!("  {}", again_command(cli, a, &trials));
    }
    Ok(if broke > 0 { 1 } else { 0 })
}

/// What a trial's runs came to, in a sentence: "did not break" only when
/// every scenario was judged; the ones that were not are said.
fn trials_summary(standings: &[sonde_proxy::trial::Standing]) -> String {
    use sonde_proxy::trial::Standing;
    let count = |s: Standing| standings.iter().filter(|t| **t == s).count();
    let (broke, refused, untested) = (
        count(Standing::Broke),
        count(Standing::Refused),
        count(Standing::Untested),
    );
    let judged = standings.len() - refused - untested;
    let mut unjudged = Vec::new();
    if refused > 0 {
        unjudged.push(format!("{refused} refused by the recording policy"));
    }
    if untested > 0 {
        unjudged.push(format!("{untested} met no request of the program's"));
    }
    let unjudged = unjudged.join(", ");
    match (judged, broke) {
        (0, _) if refused == 0 => {
            "Nothing was tried: no scenario met a request of the program's.".to_string()
        }
        (0, _) => format!("Nothing was judged: {unjudged}."),
        (_, 0) if unjudged.is_empty() => "The program did not break.".to_string(),
        (_, 0) => {
            format!("The program did not break under the {judged} judged; not judged: {unjudged}.")
        }
        _ if unjudged.is_empty() => format!("The program broke under {broke} of {judged}."),
        _ => format!("The program broke under {broke} of {judged} judged; not judged: {unjudged}."),
    }
}

/// The command that tries a program again under what did not survive, with
/// the flags of the trial it repeats: the same store, policy and recording.
/// A trial of scenarios named (`--fault`) names again the ones that did not
/// survive.
fn again_command(cli: &Cli, a: &RunArgs, trials: &[sonde_proxy::trial::Trial]) -> String {
    use sonde_proxy::trial::Standing;
    let q = |w: &str| sonde_proxy::program::shell_join(&[w.to_string()]);
    let mut words = vec!["sonde run".to_string()];
    if a.faults.is_empty() {
        words.push("--again".into());
    } else {
        for t in trials.iter().filter(|t| {
            matches!(
                t.standing,
                Standing::Broke | Standing::Degraded | Standing::Refused
            )
        }) {
            words.push(format!("--fault {}", q(&t.asked())));
        }
    }
    if let Some(n) = &a.name {
        words.push(format!("--name {}", q(n)));
    }
    if let Some(out) = &a.rec.out {
        // (the file is there now: written over, as asked)
        words.push(format!("-o {} --force", q(&out.to_string_lossy())));
    }
    if a.demo {
        words.push("--demo".into());
    }
    if let Some(url) = &cli.url {
        words.push(format!("--url {}", q(url)));
        if cli.region != "us-east-1" {
            words.push(format!("--region {}", q(&cli.region)));
        }
        if cli.path_style {
            words.push("--path-style".into());
        }
        if let Some(ca) = &cli.ca_file {
            words.push(format!("--ca-file {}", q(&ca.to_string_lossy())));
        }
    }
    if let Some(e) = &a.rec.endpoint
        && !a.demo
    {
        words.push(format!("-e {}", q(e)));
    }
    if let Some(p) = &a.rec.policy {
        words.push(format!("--policy {}", q(p)));
    }
    if a.rec.tls {
        words.push("--tls".into());
    }
    if a.no_callers {
        words.push("--no-callers".into());
    }
    words.push("--".into());
    words.push(sonde_proxy::program::shell_join(&a.command));
    words.join(" ")
}

/// A trial's line: its standing, the scenario, what happened.
pub fn trial_line(t: &sonde_proxy::trial::Trial, color: bool) -> String {
    use sonde_proxy::trial::Standing;
    let code = match t.standing {
        Standing::Survived => "32",
        Standing::Degraded => "33",
        Standing::Broke => "31",
        Standing::Untested => "2",
        Standing::Refused => "33",
    };
    format!(
        "  {}  {:<14} {}",
        paint(&format!("{:<8}", t.standing.label()), code, color),
        t.asked(),
        t.line()
    )
}

fn named_scenario(
    name: &str,
    project: &[sonde_proxy::fault::Scenario],
) -> anyhow::Result<sonde_proxy::fault::Scenario> {
    if let Some(s) = project.iter().find(|s| s.name == name) {
        return Ok(s.clone());
    }
    let asked = sonde_proxy::fault::scenario_asked(name)
        .map_err(|e| super::usage(format!("--fault {e}")))?;
    asked.ok_or_else(|| {
        let known: Vec<String> = sonde_proxy::fault::scenarios()
            .iter()
            .chain(project)
            .map(|s| format!("  {:<14} {}", s.name, s.about))
            .collect();
        super::usage(format!(
            "--fault {name}: no such scenario. The scenarios:\n{}",
            known.join("\n")
        ))
    })
}

/// One run of the command, recorded: the session's file and the program's
/// exit code. Under a scenario, the proxy answers with its faults.
async fn run_once(
    s: &Arc<Sonde>,
    a: &RunArgs,
    under: Option<&Under>,
    ctx: &RunCtx,
) -> anyhow::Result<Ran> {
    use tokio::io::AsyncWriteExt;
    let command = sonde_proxy::program::shell_join(&a.command);
    // `-o FILE` is recorded beside it under a name of its own and put in
    // its place at the end: two runs given one file never write into, or
    // print, each other's
    let target = ctx.rec.out.clone().filter(|_| under.is_none());
    let rec = match &target {
        Some(t) => {
            // (its earlier verdicts go now, as a session recorded again)
            sonde_proxy::trial::remove(t);
            Recording {
                out: Some(recording_path(t)),
                ..ctx.rec.clone()
            }
        }
        None => ctx.rec.clone(),
    };
    let mut started = start_under(
        s,
        &rec,
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Some(command),
        false,
        &Of {
            name: ctx.name.clone(),
            under,
            written: Some(ctx.written.clone()),
        },
    )
    .await?;
    let (mut set, unset) = started.proxy.env();
    if ctx.callers {
        set.extend(sonde_proxy::callers::caller_env(&s.paths().python_hook())?);
    }
    // each run a place of its own, if the command asks for one: `{trial}`
    // in the command (and SONDE_TRIAL) is "clean", then the scenario's name.
    // A run that no trial follows is left as it was written.
    let trial = under.map_or("clean", |u| u.scenario.name.as_str());
    // keys under the run's own place are its own to write over
    if ctx.trying {
        for place in sonde_proxy::trial::places(&a.command, trial) {
            ctx.written.own_place(&place);
        }
    }
    let placed = |word: &String| {
        if ctx.trying {
            word.replace("{trial}", trial)
        } else {
            word.clone()
        }
    };
    let words: Vec<String> = a.command.iter().map(placed).collect();
    // the JVM engines read s3a's endpoint from their command line, not
    // from AWS_ENDPOINT_URL (a Python program that starts Spark, from
    // PYSPARK_SUBMIT_ARGS)
    let plain = started.proxy.plain_url();
    let region = set
        .iter()
        .find(|(k, _)| k == "AWS_REGION")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    set.push((
        "PYSPARK_SUBMIT_ARGS".into(),
        sonde_proxy::jvm::pyspark_submit_args(
            std::env::var("PYSPARK_SUBMIT_ARGS").ok().as_deref(),
            &plain,
            &region,
        ),
    ));
    let words = match sonde_proxy::jvm::with_settings(&words, &plain, &region) {
        Some((engine, argv)) => {
            if under.is_none() {
                eprintln!("{}", engine_note(engine, &words));
            }
            argv
        }
        None => words,
    };
    let program = words[0].clone();
    let mut child = tokio::process::Command::new(&program);
    child.args(&words[1..]);
    if ctx.trying {
        child.env("SONDE_TRIAL", trial);
    }
    for k in &unset {
        child.env_remove(k);
    }
    for (k, v) in &set {
        child.env(k, v);
    }
    if under.is_some() {
        // a trial's output is the clean run's again: the verdict is what is read
        child.stdout(std::process::Stdio::null());
        child.stderr(std::process::Stdio::null());
    }
    // a run is a process group of its own, so that ending it ends what it
    // started too (a supervisor's children, a shell's pipeline). Not the
    // clean run at a terminal: its program may read the keyboard, and
    // Ctrl-C reaches it there by itself.
    let own_group = cfg!(unix) && (under.is_some() || !std::io::stdin().is_terminal());
    #[cfg(unix)]
    if own_group {
        child.process_group(0);
    }
    match (&ctx.input, under.is_some()) {
        // fed by a pipe: every run reads what the first read
        (Some(_), _) => {
            child.stdin(std::process::Stdio::piped());
        }
        // a terminal answers one run, not four
        (None, true) => {
            child.stdin(std::process::Stdio::null());
        }
        (None, false) => {}
    }
    let printer = (a.live && under.is_none()).then(|| follow(&started.log, true));
    let began = std::time::Instant::now();
    let mut child = match child.spawn() {
        Ok(c) => c,
        Err(e) => {
            // nothing ran: no session of it
            if let Some(p) = printer {
                p.abort();
            }
            shut_down(&mut started.proxy, SHUTDOWN_GRACE).await;
            let path = started.path.clone();
            drop(started);
            let _ = std::fs::remove_file(&path);
            return Err(anyhow::anyhow!("cannot start {program}: {e}"));
        }
    };
    let feeder = match (&ctx.input, child.stdin.take()) {
        (Some(kept), Some(mut to)) if under.is_none() => {
            // sonde's input, to the program and kept: read on a thread of
            // its own, which a program that reads none of it leaves waiting
            // (a few chunks ahead at most: a program that reads slowly
            // slows whoever writes to sonde, as a pipe would)
            let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
            std::thread::spawn(move || {
                use std::io::Read;
                let mut buf = vec![0u8; 64 << 10];
                let mut stdin = std::io::stdin().lock();
                while let Ok(n) = stdin.read(&mut buf) {
                    if n == 0 || tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
            let kept = kept.clone();
            Some(tokio::spawn(async move {
                while let Some(chunk) = rx.recv().await {
                    kept.lock().expect("input").push(&chunk);
                    if to.write_all(&chunk).await.is_err() {
                        break;
                    }
                }
                // its end is the program's end of input
                drop(to);
            }))
        }
        (Some(kept), Some(mut to)) => {
            let again = kept.lock().expect("input").again();
            Some(tokio::spawn(async move {
                match again {
                    Again::Bytes(bytes) => {
                        let _ = to.write_all(&bytes).await;
                    }
                    // from the file it was kept in, a piece at a time
                    Again::File(file) => {
                        let mut from = tokio::fs::File::from_std(file);
                        let _ = tokio::io::copy(&mut from, &mut to).await;
                    }
                    Again::Lost => {}
                }
                drop(to);
            }))
        }
        _ => None,
    };
    let mut stop = ctx.stop.clone();
    let mut signal = stop.signal();
    let status = loop {
        tokio::select! {
            st = child.wait() => break st?,
            n = stop.asked() => {
                signal = Some(n);
                // Ctrl-C is the clean run's program's to answer: it is
                // waited for, and what it sent is kept (in a group of its
                // own the signal is passed on to it). A trial's run is
                // ended, and so is any run sonde is told to terminate: the
                // whole of it, not its first process alone.
                if under.is_some() || n == 15 {
                    if !(own_group && signal_group(&child, true)) {
                        let _ = child.start_kill();
                    }
                } else if own_group {
                    signal_group(&child, false);
                }
            }
        }
    };
    let wall_ms = began.elapsed().as_secs_f64() * 1000.0;
    if let Some(f) = feeder {
        f.abort();
    }
    if let Some(p) = printer {
        p.abort();
    }
    // a run sonde ended on SIGTERM ended as sonde does (143), whatever
    // signal took its processes down
    let code = match signal {
        Some(15) => 143,
        _ => exit_code(status),
    };
    // requests still in flight are recorded before the file is read and packed
    shut_down(&mut started.proxy, SHUTDOWN_GRACE).await;
    started.writer.flush()?;
    if under.is_none() {
        // read before it is put in its place: this run's, whatever another does
        let session = Session::open_lean(&started.path).map_err(|e| anyhow::anyhow!(e))?;
        if let Some(t) = &target {
            started.path = place(&started.path, t, a.force)?;
        }
        // kept as the app keeps it: a run that failed is compared with none
        sonde_proxy::index::set(&started.path, sonde_proxy::index::Change::Exit(code));
        let color = std::io::stderr().is_terminal();
        let prices = request_prices_for(s, &session.header.endpoint);
        let analysis = sonde_proxy::view::Analysis::of(&session, &prices);
        // against the program's run before: what the change did
        let since = super::fixloop::since_last(
            s,
            &started.path,
            &session.header,
            &session.records,
            &analysis,
        );
        print_summary_of(
            &analysis,
            &session.header,
            &started.path,
            color,
            false,
            false,
            since.as_ref(),
        );
        // stopped from outside: said as such, not as a program that failed
        // (nor as one that sent nothing)
        match stopped_text(signal) {
            Some(why) => eprintln!("{}", paint(&why, "33", color)),
            None if session.records.is_empty() => eprintln!("{}", NO_REQUESTS),
            None => {}
        }
    }
    // what it wrote is the program's to write over next time
    if let Err(e) = ctx.written.keep() {
        eprintln!("warning: what the program wrote could not be kept for its next run: {e}");
    }
    // finished: compressed to a tenth or less, still a .sonde file
    let file = started.path.clone();
    drop(started);
    let _ = sonde_proxy::session::compress(&file);
    Ok(Ran {
        file,
        code,
        wall_ms,
        signal,
    })
}

/// Where `-o FILE` is recorded until the run ends: beside it, a name of
/// this run's own that no listing takes for a session.
fn recording_path(target: &std::path::Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "run.sonde".into());
    target.with_file_name(format!(
        ".{name}.{}.{}.recording",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ))
}

/// Puts a finished recording at `target`: over a file there only with
/// `--force`. One another run put there meanwhile is kept, and this one
/// goes beside it under a fresh name, said.
fn place(
    recorded: &std::path::Path,
    target: &std::path::Path,
    force: bool,
) -> anyhow::Result<PathBuf> {
    if force {
        std::fs::rename(recorded, target)?;
        return Ok(target.to_path_buf());
    }
    // a link made new is refused when the name is taken, as a rename is not
    match std::fs::hard_link(recorded, target) {
        Ok(()) => {
            let _ = std::fs::remove_file(recorded);
            Ok(target.to_path_buf())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let stem = target
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "run".into());
            let fresh = sonde_proxy::session::fresh_path(&runs_dir(target), &stem);
            std::fs::rename(recorded, &fresh)?;
            eprintln!(
                "sonde: {} was written by another run meanwhile: this run's session is {}",
                target.display(),
                fresh.display()
            );
            Ok(fresh)
        }
        // a file system without links: the name is looked at, then taken
        Err(_) if target.symlink_metadata().is_err() => {
            std::fs::rename(recorded, target)?;
            Ok(target.to_path_buf())
        }
        Err(e) => Err(e.into()),
    }
}

/// What sonde wrote on an engine's command line, and whether the program's
/// own arguments overrule it.
fn engine_note(engine: sonde_proxy::jvm::Engine, command: &[String]) -> String {
    let how = match engine {
        sonde_proxy::jvm::Engine::Spark => "--conf spark.hadoop.fs.s3a.*",
        sonde_proxy::jvm::Engine::Flink => "-Ds3.*",
    };
    match sonde_proxy::jvm::own_endpoint(command) {
        Some(own) => format!(
            "warning: sonde gave {} its endpoint ({how}), but the command names its own ({own}): those requests go there, not through sonde",
            command[0]
        ),
        None => format!("sonde: {} is given its endpoint with {how}", command[0]),
    }
}

/// Signals the process group `child` leads: an interrupt, or (`kill`) its
/// end. Whether it was sent.
#[cfg(unix)]
fn signal_group(child: &tokio::process::Child, kill: bool) -> bool {
    use rustix::process::{Pid, Signal, kill_process_group};
    let signal = if kill { Signal::KILL } else { Signal::INT };
    child
        .id()
        .and_then(|id| Pid::from_raw(id as i32))
        .is_some_and(|pid| kill_process_group(pid, signal).is_ok())
}

#[cfg(not(unix))]
fn signal_group(_child: &tokio::process::Child, _kill: bool) -> bool {
    false
}

/// What a run stopped by a signal sonde was sent says: `sonde was sent
/// SIGTERM: the program was stopped (exit 143)`.
fn stopped_text(signal: Option<i32>) -> Option<String> {
    match signal? {
        15 => Some("sonde was sent SIGTERM: the program was stopped (exit 143)".into()),
        2 => Some("interrupted (Ctrl-C): the program was stopped (exit 130)".into()),
        _ => None,
    }
}

/// The program's exit code as a shell gives it: its own, or 128 and the
/// signal that ended it.
fn exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return 128 + signal;
    }
    status.code().unwrap_or(1)
}

/// What to try when a program sent sonde nothing: most SDKs read
/// AWS_ENDPOINT_URL, some need the endpoint said their own way.
const NO_REQUESTS: &str = "\nThe program sent sonde no requests. sonde set AWS_ENDPOINT_URL, which current SDKs read; some need it said their own way:
  boto3 / botocore    1.28 or newer, or boto3.client(\"s3\", endpoint_url=os.environ[\"AWS_ENDPOINT_URL\"])
  AWS CLI             v2.13 or newer, or --endpoint-url \"$AWS_ENDPOINT_URL\"
  Go SDK v2           1.20 or newer (aws-sdk-go v1 does not read it)
  Java SDK v2         2.21 or newer
  Hadoop s3a          its configuration, not -D: fs.s3a.endpoint, fs.s3a.path.style.access=true (sonde writes them for spark-submit and flink run)
  Trino               the server reads S3, not the trino client: s3.endpoint in the catalog's properties (docs/recording.md)
  DuckDB              SET s3_endpoint='127.0.0.1:PORT'; SET s3_url_style='path'; SET s3_use_ssl=false
  Node (@aws-sdk/*)   3.4xx or newer, or new S3Client({ endpoint: process.env.AWS_ENDPOINT_URL, forcePathStyle: true })
Credentials must be the ones sonde set (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY): a profile or a role bypasses them.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_input_past_its_limit_is_kept_in_a_file_and_read_again_whole() {
        use std::io::Read;
        let mut kept = KeptInput {
            limit: 10,
            ..Default::default()
        };
        kept.push(b"12345");
        kept.push(b"678");
        assert!(kept.file.is_none(), "small: in memory");
        assert!(matches!(kept.again(), Again::Bytes(b) if b == b"12345678"));
        kept.push(b"9abcdef");
        kept.push(b"gh");
        assert!(
            kept.file.is_some() && kept.head.is_empty(),
            "past it: a file"
        );
        // every run reads all of it, from its start
        for _ in 0..2 {
            let Again::File(mut f) = kept.again() else {
                panic!("kept in its file");
            };
            let mut all = String::new();
            f.read_to_string(&mut all).unwrap();
            assert_eq!(all, "123456789abcdefgh");
        }
        assert_eq!(KeptInput::default().limit, 8 << 20);
    }

    #[test]
    fn did_not_break_is_said_only_when_every_scenario_was_judged() {
        use sonde_proxy::trial::Standing::*;
        assert_eq!(
            trials_summary(&[Survived, Degraded]),
            "The program did not break."
        );
        assert_eq!(
            trials_summary(&[Survived, Refused, Refused, Untested]),
            "The program did not break under the 1 judged; not judged: 2 refused by the recording policy, 1 met no request of the program's."
        );
        assert_eq!(
            trials_summary(&[Broke, Survived]),
            "The program broke under 1 of 2."
        );
        assert_eq!(
            trials_summary(&[Broke, Refused]),
            "The program broke under 1 of 1 judged; not judged: 1 refused by the recording policy."
        );
        assert_eq!(
            trials_summary(&[Refused, Untested]),
            "Nothing was judged: 1 refused by the recording policy, 1 met no request of the program's."
        );
        assert_eq!(
            trials_summary(&[Untested]),
            "Nothing was tried: no scenario met a request of the program's."
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_program_ended_by_a_signal_exits_as_a_shell_says_it() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::ExitStatus;
        // a wait status: the signal in its low bits, the exit code above
        assert_eq!(exit_code(ExitStatus::from_raw(9)), 137);
        assert_eq!(exit_code(ExitStatus::from_raw(15)), 143);
        assert_eq!(exit_code(ExitStatus::from_raw(3 << 8)), 3);
        assert_eq!(exit_code(ExitStatus::from_raw(0)), 0);
    }
}
