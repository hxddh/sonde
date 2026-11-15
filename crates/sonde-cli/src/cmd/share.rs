//! `sonde share`: a run as one HTML file anyone opens in a browser — the
//! app's own views (findings, operations, output lined up, requests, the
//! Time pane, and a comparison with another run first), with nothing to
//! install and nothing to serve.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Args;
use sonde_core::Sonde;

#[derive(Args)]
pub struct ShareArgs {
    /// The run (default: the newest). Two: the run before and the run
    /// after, compared.
    #[arg(num_args = 0..=2)]
    files: Vec<PathBuf>,
    /// The run to compare it with, as `sonde check` takes one (a run,
    /// `auto` for its program's baseline, or an https URL).
    #[arg(long, value_name = "FILE|auto|URL")]
    baseline: Option<String>,
    /// The file to write (default: `<run>.html` here).
    #[arg(long, short = 'o')]
    out: Option<PathBuf>,
    /// Write over the file if it is there (else it is refused).
    #[arg(long)]
    force: bool,
    /// Replace bucket and key names with tokens and drop headers, bodies, URLs,
    /// the program's output and arguments: safe to send outside.
    #[arg(long)]
    redact: bool,
}

/// The run a page is compared with: the first of two, or --baseline.
async fn baseline_of(
    s: &Arc<Sonde>,
    a: &ShareArgs,
    file: &std::path::Path,
) -> anyhow::Result<Option<(PathBuf, Option<tempfile::TempPath>)>> {
    let asked = match (a.files.as_slice(), &a.baseline) {
        ([before, _], _) => return Ok(Some((super::runs::resolve(s, before), None))),
        (_, Some(b)) => b.as_str(),
        _ => return Ok(None),
    };
    if asked.starts_with("https://") || asked.starts_with("http://") {
        let t = super::check::fetch_baseline(asked).await?;
        return Ok(Some((t.to_path_buf(), Some(t))));
    }
    if asked != "auto" {
        return Ok(Some((super::runs::resolve(s, &PathBuf::from(asked)), None)));
    }
    let dir = s.paths().runs();
    let key = sonde_analysis::run::read_header(file)
        .as_ref()
        .and_then(sonde_analysis::program::key_of);
    let found = key.and_then(|k| {
        sonde_analysis::program::baselines(&dir)
            .get(&k)
            .map(|n| dir.join(n))
            .filter(|p| p.is_file())
    });
    if found.is_none() {
        eprintln!("no baseline yet for this run's program: the page shows the run alone");
    }
    Ok(found.map(|p| (p, None)))
}

pub async fn run(s: &Arc<Sonde>, a: &ShareArgs) -> anyhow::Result<i32> {
    if a.files.len() == 2 && a.baseline.is_some() {
        return Err(super::usage("two runs, or --baseline: not both"));
    }
    let file = match a.files.last() {
        Some(f) => super::runs::resolve(s, f),
        None => super::record::which_run(s, super::record::project_name().as_deref())?.0,
    };
    if !file.exists() {
        return Err(super::usage(format!(
            "no run {}: not a file, and not a name in {} (`sonde runs` lists them)",
            file.display(),
            s.paths().runs().display()
        )));
    }
    // the page: -o, else the run's name here; a file that is there is not written over unasked
    let out = a.out.clone().unwrap_or_else(|| {
        let stem = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "run".into());
        let stem = stem
            .strip_suffix(".gz")
            .unwrap_or(&stem)
            .strip_suffix(".sonde")
            .unwrap_or(&stem)
            .to_string();
        PathBuf::from(format!("{stem}.html"))
    });
    super::record::refuse_existing(&out, a.force)?;
    // redacting changes every record, so all are read in full; otherwise
    // the run is read for what it amounts to, and each request is
    // written from the file as its turn comes
    let keep = if a.redact {
        sonde_analysis::run::Keep::All
    } else {
        sonde_analysis::run::Keep::Analysis
    };
    let mut run = super::runs::read(&file, keep)?;
    // priced before redaction hides the endpoint
    let prices = super::record::request_prices_for(s, &run.header.endpoint);
    let full = (!a.redact).then_some(file.as_path());
    let base = baseline_of(s, a, &file).await?;
    let compare = match &base {
        None => {
            if a.redact {
                sonde_analysis::report::redact(&mut run);
            }
            None
        }
        Some((path, _)) => {
            let mut before = super::runs::read(path, keep)?;
            let name = super::check::run_line(path, Some(&before.header));
            if a.redact {
                // both together: their names become the same tokens
                sonde_analysis::report::redact_all(&mut [&mut before, &mut run]);
            }
            let run_of = |x: &sonde_analysis::run::RunFile| {
                sonde_analysis::profile::Run::analysed(&x.header, &x.records, &prices)
            };
            let change = sonde_analysis::profile::compare_runs(&run_of(&before), &run_of(&run))
                .of_programs(
                    sonde_analysis::program::key_of(&before.header).as_deref(),
                    sonde_analysis::program::key_of(&run.header).as_deref(),
                );
            let signals = |x: &sonde_analysis::run::RunFile| {
                sonde_analysis::signals::signals(&x.records, &prices)
            };
            let profile = |x: &sonde_analysis::run::RunFile| {
                sonde_analysis::profile::profile(&x.records, &prices)
            };
            let compared = sonde_analysis::share::Compared {
                name,
                change,
                signals: [signals(&before), signals(&run)],
                profiles: [profile(&before), profile(&run)],
            };
            drop(before);
            Some(compared)
        }
    };
    // written whole beside it, then put in its place: a page that could not be made leaves nothing
    let part = PathBuf::from(format!("{}.part", out.display()));
    let written = std::fs::File::create(&part).and_then(|page| {
        sonde_analysis::share::write_page(&run, full, &prices, compare.as_ref(), page)
    });
    if let Err(e) = written.and_then(|()| std::fs::rename(&part, &out)) {
        let _ = std::fs::remove_file(&part);
        return Err(e.into());
    }
    eprintln!(
        "Wrote {} — one file: open it in a browser, or send it as you would the run",
        out.display()
    );
    Ok(0)
}
