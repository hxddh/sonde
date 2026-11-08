//! `sonde share`: a session as a web page anyone opens in a browser — the
//! same views as the app (operations, observations, output lined up,
//! requests), with nothing to install. The page is a static site: sonde's
//! viewer (built with Native Shell's web target, embedded in this binary)
//! and the session's `session.json`, everything already computed.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Args;
use sonde_core::Sonde;
use sonde_proxy::session::Session;
use sonde_proxy::share;

#[derive(Args)]
pub struct ShareArgs {
    /// The session (default: the newest).
    file: Option<PathBuf>,
    /// Where to write the site (default: `<session>-share/` here).
    #[arg(long, short = 'o')]
    out: Option<PathBuf>,
    /// Serve it on 127.0.0.1 instead of writing it, until Ctrl-C.
    #[arg(long)]
    serve: bool,
    /// The port for --serve (default: a free one).
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Write only session.json (to --out, or stdout).
    #[arg(long)]
    json: bool,
    /// One HTML file instead of a site: findings, time, timeline, operations (to --out, or stdout).
    #[arg(long, conflicts_with_all = ["json", "serve"])]
    html: bool,
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

pub async fn run(s: &Arc<Sonde>, a: &ShareArgs) -> anyhow::Result<i32> {
    let file = match &a.file {
        Some(f) => super::sessions::resolve(s, f),
        None => super::record::newest_session(s)?,
    };
    let mut session = Session::open(&file).map_err(|e| anyhow::anyhow!(e))?;
    if a.redact {
        sonde_proxy::report::redact(&mut session);
    }
    if a.html {
        let page = sonde_proxy::report::html(&session, &super::record::request_prices(s));
        match &a.out {
            Some(p) => {
                std::fs::write(p, page)?;
                eprintln!("{}", p.display());
            }
            None => print!("{page}"),
        }
        return Ok(0);
    }
    let doc = serde_json::to_vec(&share::session_json(
        &session,
        &super::record::request_prices(s),
    ))?;
    if a.json {
        match &a.out {
            Some(p) => std::fs::write(p, &doc)?,
            None => println!("{}", String::from_utf8_lossy(&doc)),
        }
        return Ok(0);
    }
    if a.serve {
        let dir = std::env::temp_dir().join(format!("sonde-share-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        share::write_site(&dir, &doc)?;
        let r = serve(dir.clone(), a.port).await;
        let _ = std::fs::remove_dir_all(&dir);
        r?;
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
