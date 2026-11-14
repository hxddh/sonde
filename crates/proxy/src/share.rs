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
use serde_json::Value;

use crate::episode::{episodes, shared_notes};
use crate::session::Session;
use crate::summary::summarize;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/viewer.rs"));
}

/// Whether this build carries the viewer (a development build may not).
pub fn has_viewer() -> bool {
    !embedded::VIEWER.is_empty()
}
/// A JSON document written as it is made: a session of a million requests
/// is never one tree in memory and then one string. Objects and arrays are
/// opened and closed; a value is written whole, its keys in order (as
/// `serde_json::Value` orders them, so a document reads the same whichever
/// way it was made). Pretty or on one line.
pub struct JsonOut<W: std::io::Write> {
    out: W,
    pretty: bool,
    /// For each open object or array: whether it has anything yet.
    open: Vec<bool>,
    /// A key was written: what follows is its value, on the same line.
    keyed: bool,
}

impl JsonOut<Vec<u8>> {
    /// How much has been written so far.
    pub fn written(&self) -> usize {
        self.out.len()
    }
}

impl<W: std::io::Write> JsonOut<W> {
    pub fn new(out: W, pretty: bool) -> Self {
        JsonOut {
            out,
            pretty,
            open: Vec::new(),
            keyed: false,
        }
    }

    fn indent(&mut self, depth: usize) -> std::io::Result<()> {
        if self.pretty {
            self.out.write_all(b"\n")?;
            for _ in 0..depth {
                self.out.write_all(b"  ")?;
            }
        }
        Ok(())
    }

    /// Before a key, or a value of an array: the comma and the line.
    fn place(&mut self) -> std::io::Result<()> {
        if self.keyed {
            self.keyed = false;
            return Ok(());
        }
        let depth = self.open.len();
        if let Some(any) = self.open.last_mut() {
            if *any {
                self.out.write_all(b",")?;
            }
            *any = true;
            self.indent(depth)?;
        }
        Ok(())
    }

    pub fn key(&mut self, name: &str) -> std::io::Result<()> {
        self.place()?;
        serde_json::to_writer(&mut self.out, name)?;
        self.out.write_all(if self.pretty { b": " } else { b":" })?;
        self.keyed = true;
        Ok(())
    }

    pub fn begin_object(&mut self) -> std::io::Result<()> {
        self.place()?;
        self.open.push(false);
        self.out.write_all(b"{")
    }

    pub fn begin_array(&mut self) -> std::io::Result<()> {
        self.place()?;
        self.open.push(false);
        self.out.write_all(b"[")
    }

    fn end(&mut self, close: &[u8]) -> std::io::Result<()> {
        if self.open.pop() == Some(true) {
            let depth = self.open.len();
            self.indent(depth)?;
        }
        self.out.write_all(close)
    }

    pub fn end_object(&mut self) -> std::io::Result<()> {
        self.end(b"}")
    }

    pub fn end_array(&mut self) -> std::io::Result<()> {
        self.end(b"]")
    }

    /// A value, whole.
    pub fn value(&mut self, v: &impl serde::Serialize) -> std::io::Result<()> {
        let v = serde_json::to_value(v)?;
        self.json(&v)
    }

    /// A value already a tree.
    pub fn json(&mut self, v: &Value) -> std::io::Result<()> {
        self.place()?;
        if !self.pretty {
            return Ok(serde_json::to_writer(&mut self.out, v)?);
        }
        // its lines, moved in to where it stands (a string's own line
        // breaks are escaped: every line break here is the printer's)
        let text = serde_json::to_string_pretty(v)?;
        let depth = self.open.len();
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.indent(depth)?;
            }
            self.out.write_all(line.as_bytes())?;
        }
        Ok(())
    }

    /// A key and its value.
    pub fn field(&mut self, name: &str, v: &impl serde::Serialize) -> std::io::Result<()> {
        self.key(name)?;
        self.value(v)
    }

    /// A key and an array of the values `items` gives, one at a time.
    pub fn array<T: serde::Serialize>(
        &mut self,
        name: &str,
        items: impl IntoIterator<Item = T>,
    ) -> std::io::Result<()> {
        self.key(name)?;
        self.begin_array()?;
        for item in items {
            self.value(&item)?;
        }
        self.end_array()
    }

    /// The writer back, flushed, once everything opened is closed.
    pub fn finish(mut self) -> std::io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

/// A session's records as they are in its file, each handed to `each` with
/// the id the session read gave it (`ids`, in the file's order: a file joined
/// from two recordings is numbered afresh). For writing every record out in
/// full beside an analysis made of [`Session::open_lean`].
pub fn each_record(
    path: &Path,
    ids: &mut dyn Iterator<Item = u64>,
    each: &mut dyn FnMut(reqlog::Record) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut lines = crate::session::Lines::open(path)?;
    // the header
    lines.line()?;
    while let Some(raw) = lines.line()? {
        if !raw.starts_with(b"{") || raw.starts_with(b"{\"log\":") {
            continue;
        }
        let text = String::from_utf8_lossy(raw);
        // a torn last line: the session read skipped it too
        let Ok(mut r) = serde_json::from_str::<reqlog::Record>(text.trim_end_matches('\r')) else {
            continue;
        };
        let Some(id) = ids.next() else { break };
        r.id = id;
        each(r)?;
    }
    Ok(())
}

/// A record as the viewer has it: with its short target.
fn shared_record(r: &reqlog::Record) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.insert("target".into(), Value::String(r.summary().target));
    }
    v
}

/// Everything the viewer shows, computed: header, summary, operations,
/// observations, records (with their short target) and output lines.
pub fn session_json(s: &Session, prices: &crate::profile::RequestPrices) -> Value {
    let mut doc = Vec::new();
    // (into memory: nothing to fail)
    let _ = write_session_json(s, None, prices, &mut doc);
    serde_json::from_slice(&doc).unwrap_or(Value::Null)
}

/// [`session_json`], written as it is made. `s` may be a session read with
/// [`Session::open_lean`] when `full` is its file: the records are then
/// written from the file, one at a time, and never all held.
pub fn write_session_json(
    s: &Session,
    full: Option<&Path>,
    prices: &crate::profile::RequestPrices,
    out: impl std::io::Write,
) -> std::io::Result<()> {
    let eps = episodes(&s.records);
    // a page meant for others: a variable set before the program (a key,
    // say) never reaches it
    let mut header = s.header.clone();
    header.command = header.command.as_deref().map(crate::program::scrub_command);
    // the folder names this machine's user
    header.cwd = None;
    let mut w = JsonOut::new(out, false);
    w.begin_object()?;
    w.array("operations", &eps)?;
    w.field("format", &"sonde-share")?;
    w.field("header", &header)?;
    w.field("lines", &crate::logs::lines(&s.logs))?;
    w.field(
        "observations",
        &crate::episode::observations_of(&s.header, &s.records, &eps),
    )?;
    w.field("profile", &crate::profile::profile(&s.records, prices))?;
    w.key("records")?;
    w.begin_array()?;
    match full {
        Some(path) => each_record(path, &mut s.records.iter().map(|r| r.id), &mut |r| {
            w.json(&shared_record(&r))
        })?,
        None => {
            for r in &s.records {
                w.json(&shared_record(r))?;
            }
        }
    }
    w.end_array()?;
    w.field(
        "sharedNotes",
        &shared_notes(&eps)
            .into_iter()
            .map(|(_, n, _, _)| n)
            .collect::<Vec<_>>(),
    )?;
    w.field("signals", &crate::signals::signals(&s.records, prices))?;
    w.field("summary", &summarize(&s.records))?;
    w.field("version", &2)?;
    w.end_object()?;
    w.finish()?;
    Ok(())
}

/// Writes the site to `dir`: the viewer's files and `doc` as `session.json`.
/// Refused when this build has no viewer.
pub fn write_site(dir: &Path, doc: &[u8]) -> std::io::Result<()> {
    if !has_viewer() {
        return Err(std::io::Error::other(
            "this build of sonde has no web viewer: build it first (`cd viewer && nsh package App.tsx --target web`), or write the session as data with `sonde show FILE -f json`",
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

/// Whether a request's path names a file inside the site: plain names only
/// (no `..`, no root, and no drive or UNC prefix, which on Windows would
/// replace the site's folder when joined).
fn inside(path: &str) -> bool {
    !path.contains('\\')
        && !path.contains(':')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
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
                    let resp = if !inside(&path) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_written_as_it_is_made_reads_as_one_built_whole() {
        let v = serde_json::json!({
            "b": [1, {"z": "two\nlines", "a": []}, [], {}],
            "a": {"k": null, "deep": {"x": [true]}},
            "empty": [],
        });
        for pretty in [true, false] {
            let mut w = JsonOut::new(Vec::new(), pretty);
            w.begin_object().unwrap();
            w.field("a", &v["a"]).unwrap();
            w.key("b").unwrap();
            w.begin_array().unwrap();
            w.value(&1).unwrap();
            w.begin_object().unwrap();
            w.array("a", Vec::<u8>::new()).unwrap();
            w.field("z", &"two\nlines").unwrap();
            w.end_object().unwrap();
            w.begin_array().unwrap();
            w.end_array().unwrap();
            w.begin_object().unwrap();
            w.end_object().unwrap();
            w.end_array().unwrap();
            w.array("empty", Vec::<u8>::new()).unwrap();
            w.end_object().unwrap();
            let text = String::from_utf8(w.finish().unwrap()).unwrap();
            let whole = if pretty {
                serde_json::to_string_pretty(&v).unwrap()
            } else {
                serde_json::to_string(&v).unwrap()
            };
            assert_eq!(text, whole);
        }
    }

    #[test]
    fn a_session_shared_from_a_lean_reading_and_its_file_is_the_same_document() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sdk-java.sonde");
        let prices = crate::profile::RequestPrices::default();
        let full = Session::open(&path).unwrap();
        let mut whole = Vec::new();
        write_session_json(&full, None, &prices, &mut whole).unwrap();
        let lean = Session::open_lean(&path).unwrap();
        let mut streamed = Vec::new();
        write_session_json(&lean, Some(&path), &prices, &mut streamed).unwrap();
        assert!(whole == streamed, "the two documents differ");
        let doc: Value = serde_json::from_slice(&streamed).unwrap();
        assert_eq!(doc["records"].as_array().unwrap().len(), full.records.len());
        // every header is there, though the analysis never held them
        assert!(
            doc["records"][0]["requestHeaders"]
                .as_array()
                .unwrap()
                .iter()
                .any(|h| h["name"]
                    .as_str()
                    .unwrap()
                    .eq_ignore_ascii_case("authorization"))
        );
        assert_eq!(session_json(&full, &prices), doc);
    }

    #[test]
    fn a_shared_page_never_carries_a_secret_set_before_the_program() {
        let s = crate::session::Session {
            header: crate::session::SessionHeader {
                format: "sonde-session".into(),
                version: 1,
                sonde: None,
                created: chrono::Utc::now(),
                endpoint: "e".into(),
                url: String::new(),
                region: String::new(),
                command: Some("AWS_SECRET_ACCESS_KEY=hunter2 python etl.py".into()),
                name: None,
                cwd: None,
                git: None,
                scenario: None,
                trial_of: None,
                whole_seconds: false,
            },
            records: vec![],
            logs: vec![],
        };
        let doc = super::session_json(&s, &Default::default()).to_string();
        assert!(!doc.contains("hunter2"), "{doc}");
        assert!(doc.contains("AWS_SECRET_ACCESS_KEY=… python etl.py"));
        let page = crate::report::html(&s, &Default::default());
        assert!(!page.contains("hunter2"));
    }

    #[test]
    fn only_the_sites_own_files_are_served() {
        assert!(inside("index.html"));
        assert!(inside("assets/app.js"));
        for bad in [
            "../x",
            "a/../../x",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "C:x",
            "a\\b",
            "//server/share",
        ] {
            assert!(!inside(bad), "{bad}");
        }
    }
}
