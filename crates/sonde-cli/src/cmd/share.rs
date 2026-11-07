//! `sonde share`: a session as a web page anyone opens in a browser — the
//! same views as the app (operations, observations, output lined up,
//! requests), with nothing to install. The page is a static site: sonde's
//! viewer (built with Native Shell's web target, embedded in this binary)
//! and the session's `session.json`, everything already computed.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use clap::Args;
use http_body_util::Full;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use sonde_core::Sonde;
use sonde_proxy::episode::{episodes, observations, shared_notes};
use sonde_proxy::session::Session;
use sonde_proxy::summary::summarize;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/viewer.rs"));
}

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
}

/// Everything the viewer shows, computed: header, summary, operations,
/// observations, records (with their short target) and output lines.
pub fn session_json(s: &Session) -> Value {
    let eps = episodes(&s.records);
    let records: Vec<Value> = s
        .records
        .iter()
        .map(|r| {
            let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
            if let Value::Object(m) = &mut v {
                m.insert("target".into(), Value::String(r.summary().target));
            }
            v
        })
        .collect();
    json!({
        "format": "sonde-share",
        "version": 1,
        "header": s.header,
        "summary": summarize(&s.records),
        "episodes": eps,
        "observations": observations(&s.records, &eps),
        "sharedNotes": shared_notes(&eps).into_iter().map(|(_, n, _, _)| n).collect::<Vec<_>>(),
        "records": records,
        "lines": sonde_proxy::logs::lines(&s.logs),
    })
}

fn write_site(dir: &Path, doc: &[u8]) -> anyhow::Result<()> {
    if embedded::VIEWER.is_empty() {
        anyhow::bail!(
            "this build of sonde has no web viewer: build it first (`cd viewer && nsh package App.tsx --target web`), or use --json"
        );
    }
    for (rel, bytes) in embedded::VIEWER {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, bytes)?;
    }
    std::fs::write(dir.join("session.json"), doc)?;
    Ok(())
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

async fn serve(dir: PathBuf, port: u16) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    let addr = listener.local_addr()?;
    eprintln!("The session is at http://{addr}/ — Ctrl-C stops.");
    let dir = Arc::new(dir);
    let server = async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let dir = dir.clone();
            tokio::spawn(async move {
                let svc = service_fn(move |req: Request<hyper::body::Incoming>| {
                    let dir = dir.clone();
                    async move {
                        let path = req.uri().path().trim_start_matches('/');
                        let path = if path.is_empty() { "index.html" } else { path };
                        // the site's own files only
                        let resp = if path.split('/').any(|s| s == "..") {
                            Response::builder()
                                .status(404)
                                .body(Full::new(Bytes::new()))
                        } else {
                            match tokio::fs::read(dir.join(path)).await {
                                Ok(b) => Response::builder()
                                    .header("content-type", content_type(path))
                                    .body(Full::new(Bytes::from(b))),
                                Err(_) => Response::builder()
                                    .status(404)
                                    .body(Full::new(Bytes::new())),
                            }
                        };
                        Ok::<_, Infallible>(resp.expect("a valid response"))
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .await;
            });
        }
    };
    tokio::select! {
        _ = server => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    Ok(())
}

pub async fn run(s: &Arc<Sonde>, a: &ShareArgs) -> anyhow::Result<i32> {
    let file = match &a.file {
        Some(f) => f.clone(),
        None => super::record::newest_session(s)?,
    };
    let session = Session::open(&file).map_err(|e| anyhow::anyhow!(e))?;
    let doc = serde_json::to_vec(&session_json(&session))?;
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
        write_site(&dir, &doc)?;
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
    write_site(&dir, &doc)?;
    eprintln!(
        "Wrote {} — put it on any web server (S3 static website hosting works), or run `sonde share --serve`.",
        dir.display()
    );
    Ok(0)
}
