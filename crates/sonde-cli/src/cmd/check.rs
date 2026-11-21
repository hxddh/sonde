//! `sonde check`: a run held to a budget and a baseline.

use sonde_analysis::program::{money_text, ms_text};
use sonde_proxy::callers::code_place;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Args;
use sonde_analysis::project::Project;
use sonde_analysis::run::{RunFile, RunHeader};
use sonde_analysis::view::Analysis;
use sonde_core::Sonde;

use crate::output;

use super::record::*;

#[derive(Args)]
pub struct CheckArgs {
    /// The run to check (default: the newest run).
    file: Option<PathBuf>,
    /// A run to hold it to: its requests, waiting, cost and errors with
    /// --tolerance of room, and no warning it did not have. `auto` is the
    /// baseline set for the run's program (in the app, or with --set-baseline).
    #[arg(long)]
    baseline: Option<String>,
    /// Make this run its program's baseline, for later `--baseline auto` (and the app).
    #[arg(long)]
    set_baseline: bool,
    /// The project's budget file (default: `sonde.toml` here or in a folder
    /// above): its `[check]` gives the limits and the baseline the flags
    /// leave out.
    #[arg(long)]
    config: Option<PathBuf>,
    /// At most this many requests.
    #[arg(long)]
    max_requests: Option<usize>,
    /// At most this much time waiting on the store: ms, or as sonde.toml
    /// writes it (`2s`, `250ms`).
    #[arg(long, value_parser = duration_arg, value_name = "TIME")]
    max_waiting: Option<f64>,
    /// At most this many requests that failed after their retries (a miss
    /// is no failure).
    #[arg(long)]
    max_errors: Option<usize>,
    /// At most this many bytes read out of the store: bytes, or as
    /// sonde.toml writes it (`10GiB`, `500MB`).
    #[arg(long, value_parser = size_arg, value_name = "SIZE")]
    max_transfer: Option<u64>,
    /// Against the baseline: requests, waiting and cost may grow this many
    /// percent (10 unless sonde.toml says).
    #[arg(long)]
    tolerance: Option<f64>,
    /// Write `sonde.toml` here (or at --config) with a `[check]` that this
    /// run passes: its figures with --tolerance, 10%, of room. Refused when
    /// the file is there. Trials are not written: `sonde run --trial` tries
    /// the program, and a `[[scenario]]` is the project's own to add.
    #[arg(long)]
    init: bool,
    /// With -f markdown: the link to the whole report (`sonde share`'s
    /// page, kept by CI) the comment ends with.
    #[arg(long, value_name = "URL")]
    report_url: Option<String>,
    /// The format written: text, json, markdown (a pull request's comment) or github (annotations).
    #[arg(
        long,
        short = 'f',
        value_enum,
        default_value_t,
        hide_possible_values = true
    )]
    format: Format,
}

/// What `check` writes: text and JSON, as every command, and for CI a pull
/// request's comment and GitHub Actions annotations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Default)]
pub enum Format {
    #[default]
    Text,
    Json,
    /// A pull request's comment.
    Markdown,
    /// GitHub Actions annotations: on the run's summary and, where a finding
    /// names its code, on that line of the diff.
    Github,
}

/// A run as `check` reads it: its figures, profile, findings and header,
/// and the run as `compare` holds it when it is compared.
type Checked = (
    sonde_analysis::program::RunPoint,
    sonde_analysis::profile::Profile,
    Vec<sonde_analysis::episode::Observation>,
    RunHeader,
    Option<sonde_analysis::profile::Run>,
);

/// `sonde check`: a run against a budget and a baseline run; exit 1 when it is past either.
/// A duration on the command line, as `sonde.toml` writes one.
fn size_arg(text: &str) -> Result<u64, String> {
    sonde_analysis::project::size_text(text)
        .ok_or_else(|| format!("{text:?} is not a size (10GiB, 500MB, or bytes as a number)"))
}

fn duration_arg(text: &str) -> Result<f64, String> {
    sonde_analysis::project::duration_text(text)
        .ok_or_else(|| format!("{text:?} is not a time (2s, 250ms, or ms as a number)"))
}

/// Whether a run is the project's (`sonde.toml` at `project`): of its
/// program by name when it names one ([`super::record::of_program`]), else
/// recorded in the project's folder or below it. Another program's run is
/// never held to the project's `[check]`.
fn of_project(h: &RunHeader, project: &Path, name: Option<&str>) -> bool {
    match name {
        Some(n) => super::record::of_program(h, n),
        None => {
            let root = project
                .parent()
                .map(|d| d.canonicalize().unwrap_or(d.into()));
            let cwd = h.cwd.as_deref().map(PathBuf::from);
            let cwd = cwd.map(|c| c.canonicalize().unwrap_or(c));
            matches!((root, cwd), (Some(r), Some(c)) if c.starts_with(&r))
        }
    }
}

/// What the project a run is of keeps on purpose (`[check] expect`, each
/// with why): the sonde.toml of the folder the run ran in (else this one),
/// when the run is that project's ([`of_project`]). A sonde.toml that
/// cannot be read keeps nothing here: `run` and `check` say what is wrong.
pub(super) fn expect_of(h: &RunHeader) -> std::collections::BTreeMap<String, String> {
    let from = h
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
        .or_else(|| std::env::current_dir().ok());
    let Some(Ok(Some(p))) = from.map(|d| sonde_analysis::project::of(&d)) else {
        return Default::default();
    };
    let name = p
        .file
        .program
        .name
        .as_deref()
        .filter(|n| !n.trim().is_empty());
    match &p.path {
        Some(toml) if of_project(h, toml, name) => p.file.check.expect,
        _ => Default::default(),
    }
}

/// `sonde check --init`: a `sonde.toml` whose `[check]` this run passes,
/// with room to grow, and the CI step that holds every run to it.
fn init_project(a: &CheckArgs, run: &sonde_analysis::program::RunPoint) -> anyhow::Result<i32> {
    let path = a
        .config
        .clone()
        .unwrap_or_else(|| PathBuf::from("sonde.toml"));
    if path.exists() {
        eprintln!(
            "{} exists: edit its [check], or remove it first",
            path.display()
        );
        return Ok(64);
    }
    let tolerance = a.tolerance.unwrap_or(10.0);
    let grow = 1.0 + tolerance / 100.0;
    // whole figures a person reads and edits
    let waiting = (run.waiting_ms * grow + sonde_analysis::program::WAITING_SLACK_MS).ceil();
    let text = [
        "# sonde check: what a run of this program may use against S3.".to_string(),
        format!(
            "# Made from a run of {}; edit freely.",
            output::count(run.requests, "request")
        ),
        "[check]".into(),
        format!("max-requests = {}", (run.requests as f64 * grow).ceil() as u64),
        // time moves from one identical run to the next: a limit made from
        // one run fails the next one in two. Left for a person to set
        format!(
            "# max-waiting = \"{waiting}ms\"   # measured {}: time varies run to run, set it with room",
            ms_text(run.waiting_ms)
        ),
        format!("max-errors = {}", run.errors),
        "# against a baseline run: how much more it may use, in percent".into(),
        format!("tolerance = {tolerance}"),
        "# baseline = \"auto\"   # the baseline set for the program (in the app, or --set-baseline)".into(),
        "# baseline = \"https://…/main.sonde\"   # or the main branch's run, kept by CI".into(),
        String::new(),
    ]
    .join("\n");
    std::fs::write(&path, text)?;
    eprintln!(
        "wrote {}: a [check] this run passes ({}, {} of room)",
        path.display(),
        output::count(run.requests, "request"),
        format_args!("{tolerance}%")
    );
    eprintln!("  no trials: `sonde run --trial` tries the program under faults");
    let uses = action_line();
    eprintln!(
        "{}",
        [
            "",
            "In GitHub Actions, the sonde action records the program, checks it against",
            "the base branch's run and keeps one comment on the pull request (docs/ci.md):",
            "",
            uses.as_str(),
            "        with:",
            "          run: <your command>",
            "          run-args: --url <the S3 endpoint>",
            "",
            "Elsewhere: `sonde run -- <your command>`, then `sonde check -f markdown`.",
        ]
        .join("\n")
    );
    Ok(0)
}

/// The workflow step's `uses:` line: the action at this sonde's major
/// version (`hxddh/sonde@v20` for 20.x), the tag each release moves.
fn action_line() -> String {
    let major = env!("CARGO_PKG_VERSION")
        .split('.')
        .next()
        .unwrap_or_default();
    format!("      - uses: hxddh/sonde@v{major}")
}

/// The runs of `key` up to `upto` (oldest first, 30 at most), each with
/// the commit it recorded: what [`sonde_analysis::program::since`] walks.
fn program_runs(
    s: &Sonde,
    key: &str,
    upto: (&Path, &RunHeader, &sonde_analysis::program::RunPoint),
) -> Vec<(
    sonde_analysis::program::RunPoint,
    Option<sonde_analysis::run::GitStamp>,
)> {
    let dir = s.paths().runs();
    let (upto, upto_header, upto_point) = upto;
    let upto_created = upto_header.created;
    let mut found: Vec<(chrono::DateTime<chrono::Utc>, PathBuf)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sonde"))
        .filter_map(|p| {
            let h = sonde_analysis::run::read_header(&p)?;
            (sonde_analysis::program::key_of(&h).as_deref() == Some(key)
                && h.created <= upto_created)
                .then_some((h.created, p))
        })
        .collect();
    found.sort();
    // the run checked is the newest, wherever its file is
    found.retain(|(_, p)| p.canonicalize().ok() != upto.canonicalize().ok());
    let from = found.len().saturating_sub(29);
    // an earlier run's figures are kept in its folder's index once
    // reckoned: thirty runs are thirty entries, not thirty runs read whole
    let mut runs: Vec<_> = found[from..]
        .iter()
        .filter_map(|(_, p)| {
            let endpoint = sonde_analysis::run::read_header(p)?.endpoint;
            let (counted, figures) =
                sonde_analysis::summary::figures(p, &request_prices_for(s, &endpoint))?;
            Some((figures.point, counted.git))
        })
        .collect();
    runs.push((upto_point.clone(), upto_header.git.clone()));
    runs
}

/// [`sonde_analysis::program::Since`] in a line: "over the baseline since
/// a1b2c3d, 2 runs ago; within at 9f8e7d6", or "the run before, at
/// 9f8e7d6, was within it" when this run is the first past it. None when
/// this run is the only one known: the verdict says it all.
fn since_line(s: &sonde_analysis::program::Since) -> Option<String> {
    let short = |g: &Option<sonde_analysis::run::GitStamp>, path: &str| match g {
        Some(g) => format!(
            "{}{}",
            &g.commit[..g.commit.len().min(7)],
            if g.dirty { " (with changes)" } else { "" }
        ),
        None => Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    let within = s
        .last_within
        .as_ref()
        .map(|good| short(&s.last_within_commit, good));
    if s.runs_ago == 0 {
        return within.map(|w| format!("the run before, at {w}, was within it"));
    }
    let mut t = format!(
        "over the baseline since {}, {} ago",
        short(&s.first_past_commit, &s.first_past),
        output::count(s.runs_ago, "run")
    );
    if let Some(w) = within {
        t.push_str(&format!("; within at {w}"));
    }
    Some(t)
}

/// A code place's file as the repository has it: the program ran in `cwd`
/// (perhaps a folder of the repository), annotations name files from its root.
fn repo_path(cwd: Option<&str>, file: &str) -> String {
    let Some(cwd) = cwd else {
        return file.to_string();
    };
    let full = Path::new(cwd).join(file);
    let root = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .or_else(|| std::env::var_os("GITHUB_WORKSPACE").map(PathBuf::from));
    root.and_then(|r| {
        let r = r.canonicalize().unwrap_or(r);
        let f = full.canonicalize().unwrap_or(full.clone());
        f.strip_prefix(&r)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
    })
    .unwrap_or_else(|| file.to_string())
}

/// A workflow command's message: `%`, CR and LF escaped.
fn gh_escape(t: &str) -> String {
    t.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// A workflow command's property value: `:` and `,` escaped too.
fn gh_property(t: &str) -> String {
    gh_escape(t).replace(':', "%3A").replace(',', "%2C")
}

/// A URL as it may be printed: without its query, which may be a presigned
/// URL's signature.
pub(super) fn url_shown(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// A baseline at an https URL (a CI artifact, a presigned URL), fetched to a
/// temporary file: in a minute at most, and 1 GiB at most.
pub(super) async fn fetch_baseline(url: &str) -> anyhow::Result<tempfile::TempPath> {
    const MAX: u64 = 1 << 30;
    let shown = url_shown(url);
    if !url.starts_with("https://") {
        anyhow::bail!("baseline {shown}: only https URLs are fetched");
    }
    let client = s3raw::http_client()
        .timeout(std::time::Duration::from_secs(60))
        // a redirect keeps to https
        .redirect(reqwest::redirect::Policy::custom(|a| {
            if a.url().scheme() == "https" && a.previous().len() < 10 {
                a.follow()
            } else {
                a.stop()
            }
        }))
        .build()?;
    let fail = |e: reqwest::Error| anyhow::anyhow!("baseline {shown}: {}", e.without_url());
    let mut resp = client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(fail)?;
    if resp.content_length().is_some_and(|n| n > MAX) {
        anyhow::bail!("baseline {shown}: larger than 1 GiB");
    }
    let mut f = tempfile::Builder::new().suffix(".sonde").tempfile()?;
    let mut got = 0u64;
    while let Some(chunk) = resp.chunk().await.map_err(fail)? {
        got += chunk.len() as u64;
        if got > MAX {
            anyhow::bail!("baseline {shown}: larger than 1 GiB");
        }
        std::io::Write::write_all(&mut f, &chunk)?;
    }
    Ok(f.into_temp_path())
}

pub async fn check(s: &Arc<Sonde>, a: &CheckArgs) -> anyhow::Result<i32> {
    if a.report_url.is_some() && a.format != Format::Markdown {
        eprintln!("--report-url is for -f markdown: the link a pull request's comment ends with");
        return Ok(64);
    }
    use sonde_analysis::program::{Budget, check, run_point};
    // the project's budget, which the flags override
    // --init writes the file: there is none to read yet
    let loaded = if a.init {
        Project::default()
    } else {
        load_project(a.config.as_ref())?
    };
    let project_path = loaded.path.clone();
    let project_name = loaded
        .file
        .program
        .name
        .clone()
        .filter(|n| !n.trim().is_empty());
    let file = match &a.file {
        Some(f) => super::runs::resolve(s, f),
        None => {
            let (file, why) = super::record::which_run(s, project_name.as_deref())?;
            // (the run is named first in what check writes: its path, not again)
            eprintln!("checking {why}");
            file
        }
    };
    // another program's run is not held to the sonde.toml found here (one
    // given with --config is the one asked for)
    let mut project = loaded.file.check;
    if a.config.is_none()
        && let Some(toml) = &project_path
        && let Some(h) = sonde_analysis::run::read_header(&file)
        && !of_project(&h, toml, project_name.as_deref())
    {
        let whose = match &project_name {
            Some(n) => format!("not of program \"{n}\" ({})", toml.display()),
            None => format!("recorded outside the folder of {}", toml.display()),
        };
        eprintln!(
            "note: the run is of `{}`, {whose}: its [check] is not applied",
            sonde_analysis::program::key_of(&h)
                .as_deref()
                .unwrap_or("no command"),
        );
        project = Default::default();
    }
    // a path in sonde.toml is the project's: relative to the file, not to
    // the folder `sonde check` runs in
    let project_baseline = project.baseline.clone().map(|b| {
        let path_like = b != "auto" && !b.starts_with("https://") && !b.starts_with("http://");
        match project_path.as_ref().and_then(|p| p.parent()) {
            Some(dir)
                if path_like
                    && Path::new(&b).is_relative()
                    && !dir.as_os_str().is_empty()
                    && dir.join(&b).exists() =>
            {
                dir.join(&b).to_string_lossy().into_owned()
            }
            _ => b,
        }
    });
    let baseline_arg = a.baseline.clone().or(project_baseline);
    let mut fetched: Option<tempfile::TempPath> = None;
    if let Some(u) = baseline_arg
        .as_deref()
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
    {
        fetched = Some(fetch_baseline(u).await?);
    }
    // against a baseline its findings are said as `run` and `compare` say
    // them: at any threshold too (less, not fixed)
    let traced = baseline_arg.is_some() && matches!(a.format, Format::Markdown | Format::Json);
    // read once, for what it amounts to: its figures, its findings, its header
    let read = |p: &PathBuf| -> anyhow::Result<Checked> {
        let opened = super::runs::read(p, sonde_analysis::run::Keep::Analysis)?;
        let Analysis {
            profile: pr,
            observations: ob,
            episodes,
            ..
        } = Analysis::of(&opened, &request_prices_for(s, &opened.header.endpoint));
        let point = run_point(
            &p.to_string_lossy(),
            &opened.header.created.to_rfc3339(),
            &opened.records,
            &pr,
            &ob,
        );
        let run = traced.then(|| {
            sonde_analysis::profile::Run::of_run(
                &opened.header,
                &opened.records,
                &episodes,
                pr.clone(),
                ob.clone(),
            )
        });
        // (its records are let go here: two runs are never held at once)
        Ok((point, pr, ob, opened.header, run))
    };
    if !file.exists() {
        return Err(super::usage(format!(
            "no run {}: not a file, and not a name in {} (`sonde runs` lists them)",
            file.display(),
            s.paths().runs().display()
        )));
    }
    let mut run = read(&file)?;
    // what the project keeps on purpose, each finding with why
    sonde_analysis::program::mark_expected(&mut run.2, &project.expect);
    let header = run.3.clone();
    let program = sonde_analysis::program::key_of(&header);
    let dir = s.paths().runs();
    // the program failed: as the run's folder kept it when the program ended
    let failed = sonde_analysis::fixloop::failed(&file);
    // a run that failed holds nothing to: what it did not get to is not there
    if let Some(x) = failed.filter(|_| a.init || a.set_baseline) {
        eprintln!(
            "the run failed (exit {x}): it is no {}; record one that succeeds",
            if a.init {
                "budget's source"
            } else {
                "baseline"
            }
        );
        return Ok(64);
    }
    // a run that sent nothing is no program's figures to hold another to
    if run.0.requests == 0 && (a.init || a.set_baseline) {
        eprintln!(
            "{}: the run recorded no request, so it is no {}. {}",
            sonde_analysis::program::NOTHING_REACHED,
            if a.init {
                "budget's source"
            } else {
                "baseline"
            },
            NOT_THROUGH_SONDE
        );
        return Ok(64);
    }
    if a.init {
        return init_project(a, &run.0);
    }
    if a.set_baseline {
        let Some(key) = &program else {
            eprintln!("this run has no command, so it is no program's baseline");
            return Ok(64);
        };
        // compared whole: `x.sonde` given from inside the directory has an empty parent
        let parent = std::path::absolute(&file)
            .ok()
            .and_then(|f| f.parent().and_then(|p| p.canonicalize().ok()));
        if parent.is_none() || parent != dir.canonicalize().ok() {
            eprintln!(
                "a baseline is a run in {}: record it there (no -o), or pass --baseline FILE",
                dir.display()
            );
            return Ok(64);
        }
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        sonde_analysis::program::set_baseline(&dir, key, Some(&name))?;
        eprintln!("baseline for `{key}`: {}", file.display());
    }
    let mut no_baseline: Option<String> = None;
    // a baseline set for this program is its own, whatever it ran as then
    // (a run of before it had a name)
    let by_program = baseline_arg.as_deref() == Some("auto") && fetched.is_none();
    let base_path = match baseline_arg.as_deref() {
        None => None,
        Some(_) if fetched.is_some() => fetched.as_ref().map(|p| p.to_path_buf()),
        Some("auto") => {
            let all = sonde_analysis::program::baselines(&dir);
            let found = program
                .as_ref()
                .and_then(|k| all.get(k).map(|n| dir.join(n)).filter(|p| p.is_file()));
            if found.is_none() {
                // no baseline yet is not a mistake: a first run in CI has
                // none. The budget is checked, and this is said
                no_baseline = Some(no_baseline_text(program.as_deref()));
            }
            found
        }
        Some(p) => Some(super::runs::resolve(s, &PathBuf::from(p))),
    };
    // the baseline: its figures and its findings
    let base = base_path.as_ref().map(read).transpose()?;
    if let Some(why) = &no_baseline {
        eprintln!("{why}");
    }
    // both hold a run to its baseline: without one they would check nothing
    if base.is_none() && no_baseline.is_none() && a.tolerance.is_some() {
        eprintln!(
            "--tolerance needs a baseline: add --baseline FILE|auto|URL (or `baseline` in sonde.toml)"
        );
        return Ok(64);
    }
    let base_program = match &base {
        Some(_) if by_program => program.clone(),
        Some(b) => sonde_analysis::program::key_of(&b.3),
        None => None,
    };
    if let Some(b) = &base {
        let bp = sonde_analysis::program::key_of(&b.3);
        if !by_program && bp != program {
            eprintln!(
                "note: the baseline ran `{}`, this run `{}`",
                bp.as_deref().unwrap_or("no command"),
                program.as_deref().unwrap_or("no command")
            );
        }
    }
    // a mistake in sonde.toml names the file it is in
    let in_project = |e: String| match &project_path {
        Some(p) => super::usage(format!("{}: {e}", p.display())),
        None => super::usage(e),
    };
    // the built-in demo store's times are not meaningful (`run --demo`
    // says so): no row holds a demo run's waiting, nor a run's against a
    // demo baseline
    let untimed = std::iter::once(&header)
        .chain(base.as_ref().map(|b| &b.3))
        .any(|h| h.endpoint == "demo");
    let budget = Budget {
        max_requests: a.max_requests.or(project.max_requests),
        max_waiting_ms: match a.max_waiting {
            Some(v) => Some(v),
            None => project.max_waiting_ms().map_err(in_project)?,
        },
        max_errors: a.max_errors.or(project.max_errors),
        max_transfer: match a.max_transfer {
            Some(v) => Some(v),
            None => project.max_transfer_bytes().map_err(in_project)?,
        },
        tolerance: base
            .as_ref()
            .map(|_| a.tolerance.or(project.tolerance).unwrap_or(10.0) / 100.0),
        untimed,
    };
    if untimed && budget.max_waiting_ms.is_some() {
        eprintln!(
            "note: a run on the built-in demo store (--demo): its times are not meaningful, so max-waiting is not checked"
        );
    }
    // (a run is held by its figures and its findings: `check` reads no
    // record of it, nor of its baseline)
    let c = check(
        (&run.0, &[][..], &run.1, &run.2),
        base.as_ref().map(|b| (&b.0, &[][..], &b.1, &b.2[..])),
        &budget,
    );
    // what the run's last trial found: a program that broke fails the
    // check, with a budget or without one
    use sonde_analysis::program::{run_closing, run_passed};
    // each scenario with what it was before ("broke on 12 Nov, survived since")
    let trials = sonde_analysis::trial::load_with_history(&file, program.as_deref());
    // `c.passed` is of its rows; the whole check also fails on a trial that
    // broke, and on a program that failed
    let passed = run_passed(&c, &trials, failed);
    let closing = match &no_baseline {
        // said in every format: the rows held to a baseline are not there
        Some(_) => {
            format!("{} (no baseline yet)", run_closing(&c, &trials, failed))
        }
        _ => run_closing(&c, &trials, failed),
    };
    // nothing reached sonde: said on stderr in every format, the verdict says it too
    if c.empty {
        eprintln!(
            "{}: the run recorded no request. {}",
            sonde_analysis::program::NOTHING_REACHED,
            NOT_THROUGH_SONDE
        );
    }
    // setting a baseline alone is a whole command
    if c.verdicts.is_empty() && trials.is_empty() && a.set_baseline {
        return Ok(0);
    }
    // nothing to hold the run to is a mistake in the command, in every
    // format; a program that failed is held to its exit
    if c.verdicts.is_empty() && trials.is_empty() && failed.is_none() && !c.empty {
        // with `baseline = "auto"` and no baseline yet there is nothing to
        // hold the run to, and nothing wrong with it: the first run passes
        // (the other formats say it below, as they say any verdict: a
        // comment, an annotation, a field)
        if no_baseline.is_some() && a.format == Format::Text {
            println!("nothing checked: no budget, and no baseline yet");
            return Ok(0);
        }
        // a demo run held to its waiting alone: asked for, and not meaningful
        // (said above); it passes, as a first run with nothing to hold it to
        let waived = untimed && budget.max_waiting_ms.is_some();
        if waived && a.format == Format::Text {
            println!("nothing checked: a demo run's waiting is not held to a limit");
            return Ok(0);
        }
        if no_baseline.is_none() && !waived {
            eprintln!(
                "nothing to check: give a budget (--max-requests, --max-waiting, --max-errors, --max-transfer, or sonde.toml's [check]) or a --baseline, or try the program (`sonde run --trial`)"
            );
            return Ok(64);
        }
    }
    // which program this is, and what it goes by: a baseline is a program's
    let named = match (&program, header.name.as_deref().map(str::trim)) {
        (Some(p), Some(n)) if !n.is_empty() => format!("program {p} (its name)"),
        (Some(p), _) => format!("program `{p}` (what its command runs)"),
        (None, _) => "no program (the run has no command)".to_string(),
    };
    // past the baseline: since which run, and the commits around it
    let since = match (&base, &program) {
        (Some(b), Some(key)) if !c.passed => {
            let runs = program_runs(s, key, (&file, &header, &run.0));
            sonde_analysis::program::since(&runs, &b.0, budget.tolerance.unwrap_or(0.1))
        }
        _ => None,
    };
    let since_text = since.as_ref().and_then(since_line);
    // the run against its baseline, by kind and by finding (a finding gone
    // is gone, not measured: no record is counted here)
    let change = match (&base, &run.4) {
        (Some(b), Some(r)) => b.4.as_ref().map(|br| {
            sonde_analysis::profile::compare_runs(br, r)
                .of_programs(base_program.as_deref(), program.as_deref())
                .of_exits((
                    base_path
                        .as_deref()
                        .and_then(sonde_analysis::fixloop::failed),
                    sonde_analysis::fixloop::failed(&file),
                ))
        }),
        _ => None,
    };
    if matches!(a.format, Format::Json) {
        let mut doc = serde_json::json!({
            "run": run.0,
            "program": program,
            "programNamed": header.name.is_some(),
            "check": c,
            "trials": trials,
            "passed": passed,
            "verdict": closing,
            "since": since,
        });
        // why it failed, in words: the verdict of a check that did not pass
        if !passed {
            doc["reason"] = serde_json::json!(closing);
        }
        if let Some(ch) = &change {
            doc["change"] = serde_json::json!(ch);
        }
        // `baseline = "auto"` found none: the rows held to one are not there
        if let Some(why) = &no_baseline {
            doc["noBaseline"] = serde_json::json!(why);
        }
        if let Some(code) = failed {
            doc["programExit"] = serde_json::json!(code);
        }
        output::print_json(&doc);
        return Ok(if passed { 0 } else { 1 });
    }
    let color = std::io::stdout().is_terminal();
    let judged = run_line(&file, Some(&header));
    let shown = |what: &str, v: f64| match what {
        "waiting" => ms_text(v),
        "cost" => money_text(v, &run.0.currency),
        "transfer" => sonde_fmt::bytes(v as u64),
        _ => format!("{v:.0}"),
    };
    // GitHub Actions: each verdict that failed on the run, each finding on
    // the line of code that sent its requests
    if matches!(a.format, Format::Github) {
        // a baseline asked for and not found: a warning on the run, not a
        // pass in silence
        if let Some(why) = &no_baseline {
            println!("::warning title=sonde check::{}", gh_escape(why));
        }
        if c.empty {
            println!(
                "::error title=sonde check::{}",
                gh_escape(&format!(
                    "{}: the run recorded no request. {NOT_THROUGH_SONDE}",
                    sonde_analysis::program::NOTHING_REACHED
                ))
            );
        }
        if let Some(x) = failed {
            println!(
                "::error title=sonde check::{}",
                gh_escape(&match run.0.refused.as_deref() {
                    Some(w) => format!("the program failed (exit {x}): {w}"),
                    None => format!("the program failed (exit {x})"),
                })
            );
        }
        // since when it is past its baseline: said once, on the first row
        // the baseline failed
        let mut since_said = false;
        for v in c.verdicts.iter().filter(|v| !v.ok) {
            let since = match &since_text {
                Some(t) if v.against == "baseline" && !since_said => {
                    since_said = true;
                    format!(" · {t}")
                }
                _ => String::new(),
            };
            println!(
                "::error title=sonde check::{}",
                gh_escape(&format!(
                    "{} {} > {} ({}, {}){since}",
                    v.label(),
                    shown(v.what, v.value),
                    shown(v.what, v.limit),
                    v.over().unwrap_or_default(),
                    v.against,
                ))
            );
        }
        for t in trials
            .iter()
            .filter(|t| t.standing == sonde_analysis::trial::Standing::Broke)
        {
            println!(
                "::error title=sonde check::{}",
                gh_escape(&format!("broke under {}: {}", t.scenario, t.line()))
            );
        }
        for t in &trials {
            let said = match t.standing {
                sonde_analysis::trial::Standing::Degraded => "degraded under",
                sonde_analysis::trial::Standing::Refused => "refused by the policy under",
                _ => continue,
            };
            println!(
                "::warning title=sonde check::{}",
                gh_escape(&format!("{said} {}: {}", t.scenario, t.line()))
            );
        }
        for w in &c.new_warnings {
            println!(
                "::warning title=sonde: new since the baseline::{}",
                gh_escape(w)
            );
        }
        for f in run.2.iter().take(10) {
            // what the project keeps on purpose is said with why, never warned of
            let level = if f.level == "warn" && f.expected.is_none() {
                "warning"
            } else {
                "notice"
            };
            let text = match &f.expected {
                Some(why) => format!("{} (expected: {why})", f.text),
                None => f.text.clone(),
            };
            match f.callers.first().and_then(|c| code_place(&c.place)) {
                Some((file, line)) => println!(
                    "::{level} file={},line={line},title=sonde::{}",
                    gh_property(&repo_path(header.cwd.as_deref(), &file)),
                    gh_escape(&text)
                ),
                None => println!("::{level} title=sonde::{}", gh_escape(&text)),
            }
        }
        println!("sonde: {closing} · {judged}");
        return Ok(if passed { 0 } else { 1 });
    }
    // a pull request's comment: the verdicts, what is new, and the run's findings
    if matches!(a.format, Format::Markdown) {
        print!(
            "{}",
            sonde_analysis::program::check_comment(
                &c,
                &sonde_analysis::program::Comment {
                    currency: &run.0.currency,
                    since: since_text.as_deref(),
                    findings: &run.2,
                    trials: &trials,
                    change: change.as_ref(),
                    no_baseline: no_baseline.as_deref(),
                    report_url: a.report_url.as_deref(),
                    failed: failed.map(|code| (code, run.0.refused.as_deref())),
                }
            )
        );
        println!("\n*{}*", sonde_analysis::program::md_text(&judged));
        return Ok(if passed { 0 } else { 1 });
    }
    let rows: Vec<Vec<String>> = c
        .verdicts
        .iter()
        .map(|v| {
            vec![
                if v.ok {
                    paint("within", "32", color)
                } else {
                    paint("over", "31", color)
                },
                v.label().to_string(),
                shown(v.what, v.value),
                format!("≤ {}", shown(v.what, v.limit)),
                v.against.to_string(),
                v.over().unwrap_or_default(),
            ]
        })
        .collect();
    // which run this is about comes first: "the newest" is not a name
    println!("{judged}");
    println!("{named}");
    if let (Some(b), Some(base)) = (&base_path, &base) {
        println!("against {}", run_line(b, Some(&base.3)));
    }
    if !rows.is_empty() {
        println!(
            "{}",
            output::table(&["", "", "run", "limit", "from", "over"], &rows)
        );
    }
    for t in &trials {
        use sonde_analysis::trial::Standing;
        println!(
            "{}  {}{}",
            paint(
                &format!("{:<8}", t.standing.label()),
                match t.standing {
                    Standing::Broke => "31",
                    Standing::Degraded => "33",
                    Standing::Untested => "2",
                    Standing::Survived => "32",
                    Standing::Refused => "33",
                },
                color
            ),
            t.asked(),
            if t.line().is_empty() {
                String::new()
            } else {
                format!(" · {}", t.line())
            }
        );
    }
    for w in &c.new_warnings {
        println!("{} {w}", paint("!", "33", color));
    }
    for e in &c.expected {
        println!(
            "{} {} (expected: {})",
            paint("·", "2", color),
            e.text,
            e.expected
        );
    }
    if let Some(t) = &since_text {
        println!("{}", paint(t, "33", color));
    }
    println!(
        "{}",
        paint(&closing, if passed { "32" } else { "31" }, color)
    );
    Ok(if passed { 0 } else { 1 })
}

/// Why a run records nothing, and where to look.
const NOT_THROUGH_SONDE: &str = "The program's S3 client did not use the endpoint sonde gave it (AWS_ENDPOINT_URL; `sonde run` says how each SDK is pointed at it), or it sent nothing.";

/// Why `baseline = "auto"` found none: which program this is.
fn no_baseline_text(program: Option<&str>) -> String {
    match program {
        Some(key) => format!(
            "no baseline yet for program `{key}`: set one with `sonde check FILE --set-baseline` or in the app"
        ),
        None => "no baseline yet: this run has no command, so it is no program's run".into(),
    }
}

/// A run by name and time, as `sonde check` says which run it judged:
/// `run 20261110-091555-etl.sonde · recorded 2026-11-10 09:15`, the time
/// as `sonde runs` lists it.
pub(super) fn run_line(file: &Path, header: Option<&sonde_analysis::run::RunHeader>) -> String {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.display().to_string());
    let read;
    let header = match header {
        Some(h) => Some(h),
        None => {
            read = RunFile::open(file).ok().map(|s| s.header);
            read.as_ref()
        }
    };
    match header {
        Some(h) => format!(
            "run {name} · recorded {}",
            h.created
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
        ),
        None => format!("run {name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonde_analysis::program::md_text;

    #[test]
    fn a_findings_text_is_shown_as_written_in_a_pr_comment() {
        assert_eq!(
            md_text("key `a|b` <b>x</b> @team *hot*"),
            "key \\`a\\|b\\` \\<b\\>x\\</b\\> @\u{2060}team \\*hot\\*"
        );
        // sonde's own sentence keeps its code spans spans: 20.0 escaped
        // their backticks, and the comment showed them
        use sonde_analysis::program::md_said;
        assert_eq!(
            md_said("no baseline yet for program `python etl_job.py --x *`"),
            "no baseline yet for program `python etl_job.py --x *`"
        );
        assert_eq!(md_said("a_b `c_d` e*"), "a\\_b `c_d` e\\*");
        // unpaired: all text
        assert_eq!(md_said("a `b"), "a \\`b");
    }

    #[test]
    fn a_code_place_is_its_file_and_line_spaces_and_drives_too() {
        assert_eq!(
            code_place("etl/load.py:212 read"),
            Some(("etl/load.py".into(), 212))
        );
        assert_eq!(
            code_place("my dir/x.py:3 f"),
            Some(("my dir/x.py".into(), 3))
        );
        assert_eq!(
            code_place(r"C:\src\x.py:9 main"),
            Some((r"C:\src\x.py".into(), 9))
        );
        assert_eq!(code_place("no line here"), None);
    }

    #[test]
    fn since_says_the_runs_and_commits_around_and_never_this_run_as_a_commit() {
        use sonde_analysis::program::Since;
        use sonde_analysis::run::GitStamp;
        let at = |c: &str| {
            Some(GitStamp {
                commit: c.into(),
                branch: None,
                dirty: false,
            })
        };
        let s = |runs_ago, within: Option<&str>| Since {
            runs_ago,
            first_past: "/s/pr2.sonde".into(),
            first_past_commit: at("a1b2c3d4e5"),
            last_within: within.map(String::from),
            last_within_commit: within.and(at("9f8e7d6c5b")),
        };
        // this run alone: the verdict says it, nothing more to say
        assert_eq!(since_line(&s(0, None)), None);
        assert_eq!(
            since_line(&s(0, Some("/s/pr1.sonde"))).as_deref(),
            Some("the run before, at 9f8e7d6, was within it")
        );
        assert_eq!(
            since_line(&s(2, Some("/s/pr1.sonde"))).as_deref(),
            Some("over the baseline since a1b2c3d, 2 runs ago; within at 9f8e7d6")
        );
    }

    #[test]
    fn a_presigned_url_is_printed_without_its_signature() {
        assert_eq!(
            url_shown("https://b.s3.amazonaws.com/base.sonde?X-Amz-Signature=abc"),
            "https://b.s3.amazonaws.com/base.sonde"
        );
    }

    #[tokio::test]
    async fn a_baseline_is_fetched_over_https_only() {
        // this machine's own names too
        for url in ["http://localhost:8080/base.sonde", "http://127.0.0.1/b"] {
            let e = fetch_baseline(url).await.unwrap_err();
            assert!(e.to_string().contains("only https"), "{e}");
        }
        let e = fetch_baseline("http://example.com/base.sonde?sig=1")
            .await
            .unwrap_err();
        assert!(e.to_string().contains("only https"), "{e}");
        assert!(!e.to_string().contains("sig=1"));
    }
}
