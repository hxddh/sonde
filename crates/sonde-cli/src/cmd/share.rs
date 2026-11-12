//! `sonde share`: a session as a web page anyone opens in a browser — the
//! same views as the app (operations, findings, output lined up, requests),
//! with nothing to install — or as OpenTelemetry traces (`--otlp`). The page is a static site: sonde's
//! viewer (built with Native Shell's web target, embedded in this binary)
//! and the session's `session.json`, everything already computed.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Args;
use sonde_core::Sonde;
use sonde_proxy::share;

#[derive(Args)]
pub struct ShareArgs {
    /// The session (default: the newest). Two, with --html: the run before
    /// and the run after, compared.
    #[arg(num_args = 0..=2)]
    files: Vec<PathBuf>,
    /// With --html: the run to compare it with, as `sonde check` takes one
    /// (a session, `auto` for its program's baseline, or an https URL).
    #[arg(long, value_name = "FILE|auto|URL")]
    baseline: Option<String>,
    /// Where to write the site (default: `<session>-share/` here).
    #[arg(long, short = 'o')]
    out: Option<PathBuf>,
    /// Serve it on 127.0.0.1 instead of writing it, until Ctrl-C.
    #[arg(long)]
    serve: bool,
    /// The port for --serve (default: a free one).
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// One HTML file instead of a site: findings, time, timeline, operations (to --out, or stdout).
    #[arg(long, conflicts_with = "serve")]
    html: bool,
    /// For a tracing stack: OpenTelemetry traces (OTLP/JSON), each operation
    /// a span with its requests under it (to --out, or stdout).
    #[arg(long, conflicts_with_all = ["html", "serve", "redact", "baseline"])]
    otlp: bool,
    /// With --otlp: send them to an OTLP/HTTP collector instead, e.g.
    /// http://localhost:4318/v1/traces.
    #[arg(long, value_name = "URL", requires = "otlp", conflicts_with = "out")]
    to: Option<String>,
    /// Gone in 18.0 (`sonde show FILE -f json`): said, and refused.
    #[arg(long, hide = true)]
    json: bool,
    /// Replace bucket and key names with tokens and drop headers, bodies, URLs,
    /// the program's output and arguments: safe to send outside.
    #[arg(long)]
    redact: bool,
}

async fn serve(dir: PathBuf, port: u16) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    eprintln!(
        "The session is at http://{}/ — Ctrl-C stops.",
        listener.local_addr()?
    );
    tokio::select! {
        _ = share::serve(listener, dir) => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    Ok(())
}

/// The run a page is compared with: the first of two, or --baseline.
async fn baseline_of(
    s: &Arc<Sonde>,
    a: &ShareArgs,
    file: &std::path::Path,
) -> anyhow::Result<Option<(PathBuf, Option<tempfile::TempPath>)>> {
    let asked = match (a.files.as_slice(), &a.baseline) {
        ([before, _], _) => return Ok(Some((super::sessions::resolve(s, before), None))),
        (_, Some(b)) => b.as_str(),
        _ => return Ok(None),
    };
    if asked.starts_with("https://") || asked.starts_with("http://") {
        let t = super::check::fetch_baseline(asked).await?;
        return Ok(Some((t.to_path_buf(), Some(t))));
    }
    if asked != "auto" {
        return Ok(Some((
            super::sessions::resolve(s, &PathBuf::from(asked)),
            None,
        )));
    }
    let dir = s.paths().sessions();
    let key = sonde_proxy::session::read_header(file)
        .as_ref()
        .and_then(sonde_proxy::program::key_of);
    let found = match (key, sonde_proxy::program::read_baselines(&dir)) {
        (_, Err(e)) => anyhow::bail!("{e}"),
        (Some(k), Ok(all)) => all.get(&k).map(|n| dir.join(n)).filter(|p| p.is_file()),
        (None, Ok(_)) => None,
    };
    if found.is_none() {
        eprintln!("no baseline yet for this run's program: the page shows the run alone");
    }
    Ok(found.map(|p| (p, None)))
}

pub async fn run(s: &Arc<Sonde>, a: &ShareArgs) -> anyhow::Result<i32> {
    if a.json {
        return Err(super::usage(
            "share --json is gone in 18.0: `sonde show FILE -f json` writes the session as data",
        ));
    }
    let compared = a.files.len() == 2 || a.baseline.is_some();
    if compared && a.otlp {
        eprintln!("--otlp takes one session");
        return Ok(64);
    }
    if compared && !a.html {
        eprintln!("a run compared with another is a page: add --html");
        return Ok(64);
    }
    if a.files.len() == 2 && a.baseline.is_some() {
        eprintln!("two runs, or --baseline: not both");
        return Ok(64);
    }
    let file = match a.files.last() {
        Some(f) => super::sessions::resolve(s, f),
        None => super::record::newest_session(s)?,
    };
    if !file.exists() {
        return Err(super::usage(format!(
            "no session {}: not a file, and not a name in {} (`sonde sessions` lists them)",
            file.display(),
            s.paths().sessions().display()
        )));
    }
    if a.otlp {
        return super::sessions::otlp(s, &file, a.out.as_deref(), a.to.as_deref()).await;
    }
    // redacting changes every record, so all are read in full; otherwise
    // the session is read for what it amounts to, and each request is
    // written from the file as its turn comes
    let (session, full) = if a.redact {
        let mut session = super::sessions::read(&file, sonde_proxy::session::Keep::All)?;
        let prices = super::record::request_prices_for(s, &session.header.endpoint);
        sonde_proxy::report::redact(&mut session);
        ((session, prices), None)
    } else {
        let session = super::sessions::read(&file, sonde_proxy::session::Keep::Analysis)?;
        // priced before redaction hides the endpoint
        let prices = super::record::request_prices_for(s, &session.header.endpoint);
        ((session, prices), Some(file.as_path()))
    };
    let (mut session, prices) = session;
    if a.html {
        let base = baseline_of(s, a, &file).await?;
        let page = match &base {
            None => sonde_proxy::report::html(&session, &prices),
            Some((path, _)) => {
                // both read whole when redacted: their names become the same tokens
                let mut before = super::sessions::read(
                    path,
                    if a.redact {
                        sonde_proxy::session::Keep::All
                    } else {
                        sonde_proxy::session::Keep::Analysis
                    },
                )?;
                let name = super::check::session_line(path, Some(&before.header));
                if a.redact {
                    // (the run was redacted alone above: read it again, for one salt)
                    session = super::sessions::read(&file, sonde_proxy::session::Keep::All)?;
                    sonde_proxy::report::redact_all(&mut [&mut before, &mut session]);
                }
                let run_of = |x: &sonde_proxy::session::Session| {
                    sonde_proxy::profile::Run::analysed(&x.header, &x.records, &prices)
                };
                let change =
                    sonde_proxy::profile::compare_runs(&run_of(&before), &run_of(&session))
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
        return Ok(0);
    }
    let mut doc = Vec::new();
    share::write_session_json(&session, full, &prices, &mut doc)?;
    if a.serve {
        // a directory of its own, made fresh (no name another user can claim first)
        let tmp = tempfile::Builder::new().prefix("sonde-share-").tempdir()?;
        share::write_site(tmp.path(), &doc)?;
        serve(tmp.path().to_path_buf(), a.port).await?;
        return Ok(0);
    }
    let dir = a.out.clone().unwrap_or_else(|| {
        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "session".into());
        PathBuf::from(format!("{stem}-share"))
    });
    std::fs::create_dir_all(&dir)?;
    share::write_site(&dir, &doc)?;
    eprintln!(
        "Wrote {} — put it on any web server (S3 static website hosting works), or run `sonde share --serve`.",
        dir.display()
    );
    Ok(0)
}
