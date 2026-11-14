//! `sonde share`: a run as one HTML file anyone opens in a browser, with
//! nothing to install: what `show` says of it, drawn.

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
    /// The file to write (default: stdout).
    #[arg(long, short = 'o')]
    out: Option<PathBuf>,
    /// With -o: write over the file if it is there (else it is refused).
    #[arg(long, requires = "out")]
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
        return Ok(Some((
            super::runs::resolve(s, &PathBuf::from(asked)),
            None,
        )));
    }
    let dir = s.paths().runs();
    let key = sonde_proxy::session::read_header(file)
        .as_ref()
        .and_then(sonde_proxy::program::key_of);
    let found = key.and_then(|k| {
        sonde_proxy::program::baselines(&dir)
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
    if let Some(out) = &a.out {
        super::record::refuse_existing(out, a.force)?;
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
    // redacting changes every record, so all are read in full; otherwise
    // the session is read for what it amounts to
    let keep = if a.redact {
        sonde_proxy::session::Keep::All
    } else {
        sonde_proxy::session::Keep::Analysis
    };
    let mut session = super::runs::read(&file, keep)?;
    // priced before redaction hides the endpoint
    let prices = super::record::request_prices_for(s, &session.header.endpoint);
    if a.redact {
        sonde_proxy::report::redact(&mut session);
    }
    let base = baseline_of(s, a, &file).await?;
    let page = match &base {
        None => sonde_proxy::report::html(&session, &prices),
        Some((path, _)) => {
            // both read whole when redacted: their names become the same tokens
            let mut before = super::runs::read(path, keep)?;
            let name = super::check::session_line(path, Some(&before.header));
            if a.redact {
                // (the run was redacted alone above: read it again, for one salt)
                session = super::runs::read(&file, keep)?;
                sonde_proxy::report::redact_all(&mut [&mut before, &mut session]);
            }
            let run_of = |x: &sonde_proxy::session::Session| {
                sonde_proxy::profile::Run::analysed(&x.header, &x.records, &prices)
            };
            let change = sonde_proxy::profile::compare_runs(&run_of(&before), &run_of(&session))
                .of_programs(
                    sonde_proxy::program::key_of(&before.header).as_deref(),
                    sonde_proxy::program::key_of(&session.header).as_deref(),
                );
            drop(before);
            sonde_proxy::report::html_against(
                &session,
                &prices,
                Some(&sonde_proxy::report::Against {
                    change: &change,
                    name,
                }),
            )
        }
    };
    match &a.out {
        Some(p) => {
            std::fs::write(p, page)?;
            eprintln!("{}", p.display());
        }
        None => print!("{page}"),
    }
    Ok(0)
}
