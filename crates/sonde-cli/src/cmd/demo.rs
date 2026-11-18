//! `sonde run --demo` without a command: the tour. A small export job,
//! written out as the Python it stands for, is played by sonde request by
//! request to the built-in demo store and recorded; its finding is shown with
//! the line of code that sent it and the fix; then the job with that fix is
//! played and recorded again, and the fix loop says what the fix saved.

use std::path::PathBuf;
use std::sync::Arc;

use sonde_analysis::run::RunFile;
use sonde_core::Sonde;
use sonde_core::tour::{JOB, PROGRAM, fixed_job};

use super::record::*;

/// The command a tour's runs say they ran.
pub const TOUR: &str = "sonde run --demo";

/// A tour in one command: the job and the job fixed, each recorded as any
/// program's run is, the first shown with its findings, the second against
/// the first.
pub async fn tour(s: &Arc<Sonde>, out: Option<PathBuf>) -> anyhow::Result<i32> {
    let ep = sonde_core::demo::install(s)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let dir = s.paths().data_dir.join("tour");
    std::fs::create_dir_all(&dir)?;
    let (first_py, fixed_py) = (dir.join("export.py"), dir.join("export-fixed.py"));
    std::fs::write(&first_py, JOB)?;
    std::fs::write(&fixed_py, fixed_job())?;
    println!(
        "The tour: a small export job ({}), played by sonde request by request as boto3 sends them, to sonde's built-in demo store.",
        first_py.display()
    );
    let fixed_out = out.as_ref().map(|o| {
        let stem = o
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "tour".into());
        o.with_file_name(format!("{stem}-fixed.sonde"))
    });
    let first = play(s, &ep.id, out, JOB, "export.py").await?;
    let color = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let run = RunFile::open(&first).map_err(|e| anyhow::anyhow!(e))?;
    let analysis =
        sonde_analysis::view::Analysis::of(&run, &request_prices_for(s, &run.header.endpoint));
    print_summary_of(&analysis, &run.header, &first, color, true, false, None);
    drop(run);
    println!(
        "\nThe same job with that fix ({}), played and recorded again:",
        fixed_py.display()
    );
    let fixed = play(s, &ep.id, fixed_out, &fixed_job(), "export-fixed.py").await?;
    let run = RunFile::open(&fixed).map_err(|e| anyhow::anyhow!(e))?;
    let analysis =
        sonde_analysis::view::Analysis::of(&run, &request_prices_for(s, &run.header.endpoint));
    let since = super::fixloop::since_last(s, &fixed, &run.header, &run.records, &analysis);
    println!("run: {}", fixed.display());
    if let Some(since) = &since {
        for line in super::fixloop::since_lines(since, color) {
            println!("{line}");
        }
    }
    Ok(0)
}

/// One run of `job` (from file `file`), to endpoint `ep`, recorded: the
/// run's path.
async fn play(
    s: &Arc<Sonde>,
    ep: &str,
    out: Option<PathBuf>,
    job: &str,
    file: &str,
) -> anyhow::Result<PathBuf> {
    let rec = Recording {
        endpoint: Some(ep.to_string()),
        // the demo store is sonde's own, made for this tour
        policy: Some("allow".into()),
        out,
        tls: false,
    };
    let started = start_under(
        s,
        &rec,
        "127.0.0.1:0".parse()?,
        Some(TOUR.into()),
        false,
        &Of {
            name: Some(PROGRAM.into()),
            ..Default::default()
        },
    )
    .await?;
    let app = client_of(&started.proxy.url(), started.proxy.client_keys())?;
    sonde_core::tour::play(
        &app,
        job,
        file,
        sonde_proxy::callers::CALLER_HEADER,
        || started.proxy.first_request(),
        |_| {},
    )
    .await;
    let file = started.path.clone();
    drop(app);
    drop(started);
    let _ = sonde_analysis::run::compress(&file);
    Ok(file)
}

/// A client to `url` with `keys`, as the tour's job's boto3 is.
fn client_of(url: &str, (key, secret): (&str, &str)) -> anyhow::Result<s3raw::S3Client> {
    Ok(sonde_core::tour::client(url, key, secret)?)
}

/// `sonde demo-job FILE` (hidden): the tour's job in FILE played against
/// AWS_ENDPOINT_URL with the environment's keys, each request said as it
/// is answered. What the app's demo runs (in-process there), so that its
/// command runs as written under `sonde run --demo` too.
pub async fn job(file: &std::path::Path) -> anyhow::Result<i32> {
    let job =
        std::fs::read_to_string(file).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let url = std::env::var("AWS_ENDPOINT_URL").map_err(|_| {
        anyhow::anyhow!("AWS_ENDPOINT_URL is not set: run it under sonde run --demo")
    })?;
    let app = client_of(
        &url,
        (&var("AWS_ACCESS_KEY_ID"), &var("AWS_SECRET_ACCESS_KEY")),
    )?;
    let name = file.file_name().map_or_else(
        || sonde_core::tour::FILE.into(),
        |n| n.to_string_lossy().into_owned(),
    );
    sonde_core::tour::play(
        &app,
        &job,
        &name,
        sonde_proxy::callers::CALLER_HEADER,
        || None,
        |l| println!("{l}"),
    )
    .await;
    Ok(0)
}
