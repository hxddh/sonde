//! A session as a web page anyone opens in a browser: sonde's viewer (built
//! with Native Shell's web target, embedded at build time) and the session's
//! `session.json`, everything already computed. `sonde share` and the app's
//! Share both write it.

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};

use crate::episode::{episodes, observations, shared_notes};
use crate::session::Session;
use crate::summary::summarize;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/viewer.rs"));
}

/// Whether this build carries the viewer (a development build may not).
pub fn has_viewer() -> bool {
    !embedded::VIEWER.is_empty()
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
        "lines": crate::logs::lines(&s.logs),
    })
}

/// Writes the site to `dir`: the viewer's files and `doc` as `session.json`.
/// Refused when this build has no viewer.
pub fn write_site(dir: &Path, doc: &[u8]) -> std::io::Result<()> {
    if !has_viewer() {
        return Err(std::io::Error::other(
            "this build of sonde has no web viewer: build it first (`cd viewer && nsh package App.tsx --target web`), or use --json",
        ));
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

/// Serves the files under `dir` (and nothing outside it) on `listener`,
/// for as long as the future runs.
pub async fn serve(listener: tokio::net::TcpListener, dir: PathBuf) {
    let dir = Arc::new(dir);
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
                    let path = if path.is_empty() || path.ends_with('/') {
                        format!("{path}index.html")
                    } else {
                        path.to_string()
                    };
                    // the site's own files only
                    let resp = if path.split('/').any(|s| s == ".." || s.contains('\\')) {
                        Response::builder()
                            .status(404)
                            .body(Full::new(Bytes::new()))
                    } else {
                        match tokio::fs::read(dir.join(&path)).await {
                            Ok(b) => Response::builder()
                                .header("content-type", content_type(&path))
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
}
