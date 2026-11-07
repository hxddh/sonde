//! Sessions as TSX sees them: recorded files, their operations, and a live
//! recording. Credentials stay in Rust: the recording's endpoint signs with
//! them; the application gets dummy keys and the proxy's URL.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nsh_core::JsonValue;
use nsh_core::prelude::*;
use parking_lot::Mutex;
use reqlog::RequestLog;
use serde_json::json;
use sonde_proxy::episode::{episodes, observations, shared_notes};
use sonde_proxy::session::{Session, SessionHeader, SessionWriter};
use sonde_proxy::summary::summarize;
use sonde_proxy::{Policy, Proxy, ProxyConfig};

use crate::state::{err_msg, sonde, to_json};

struct Recording {
    proxy: Proxy,
    path: PathBuf,
    endpoint: String,
    log: RequestLog,
    writer: Arc<SessionWriter>,
}

static RECORDING: Mutex<Option<Recording>> = Mutex::new(None);

fn open_session(path: &str) -> Result<Session, String> {
    Session::open(Path::new(path)).map_err(err_msg)
}

/// Lines in a file: the header plus one per record.
fn count_records(path: &Path) -> usize {
    std::fs::read(path)
        .map(|b| b.iter().filter(|c| **c == b'\n').count().saturating_sub(1))
        .unwrap_or(0)
}

fn header_of(path: &Path) -> Option<SessionHeader> {
    use std::io::BufRead;
    let f = std::fs::File::open(path).ok()?;
    let mut first = String::new();
    std::io::BufReader::new(f).read_line(&mut first).ok()?;
    serde_json::from_str(&first).ok()
}

fn shell_exports(set: &[(String, String)], unset: &[String]) -> String {
    let mut s = String::new();
    for k in unset {
        s.push_str(&format!("unset {k}\n"));
    }
    for (k, v) in set {
        s.push_str(&format!("export {k}={v}\n"));
    }
    s
}

/// Recorded sessions and the live recording, as TSX calls them.
#[module]
pub struct Sessions;

#[module]
impl Sessions {
    /// The sessions in the data directory, newest first.
    pub fn list() -> JsonValue {
        let dir = sonde().paths().sessions();
        let live = RECORDING.lock().as_ref().map(|r| r.path.clone());
        let mut out: Vec<(String, JsonValue)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("sonde") {
                    continue;
                }
                let Some(h) = header_of(&p) else { continue };
                let created = h.created.to_rfc3339();
                out.push((
                    created.clone(),
                    json!({
                        "path": p.to_string_lossy(),
                        "name": p.file_name().map(|n| n.to_string_lossy().into_owned()),
                        "created": created,
                        "endpoint": h.endpoint,
                        "command": h.command,
                        "requests": count_records(&p),
                        "live": live.as_deref() == Some(p.as_path()),
                    }),
                ));
            }
        }
        out.sort_by(|a, b| b.0.cmp(&a.0));
        JsonValue::Array(out.into_iter().map(|(_, v)| v).collect())
    }

    /// A session: its header, summary, operations and observations, and the
    /// notes said once instead of on each operation.
    pub fn open(path: String) -> Result<JsonValue, String> {
        let s = open_session(&path)?;
        let eps = episodes(&s.records);
        let obs = observations(&s.records, &eps);
        let shared: Vec<String> = shared_notes(&eps)
            .into_iter()
            .map(|(_, n, _, _)| n)
            .collect();
        Ok(json!({
            "path": path,
            "header": s.header,
            "summary": summarize(&s.records),
            "episodes": eps,
            "observations": obs,
            "sharedNotes": shared,
        }))
    }

    /// Short rows for the records with these ids (an operation's requests).
    pub fn records(path: String, ids: Vec<f64>) -> Result<JsonValue, String> {
        let s = open_session(&path)?;
        let rows: Vec<_> = ids
            .iter()
            .filter_map(|id| s.records.iter().find(|r| r.id == *id as u64))
            .map(|r| r.summary())
            .collect();
        Ok(to_json(rows))
    }

    /// One record of a session, in full.
    pub fn record(path: String, id: f64) -> Result<JsonValue, String> {
        let s = open_session(&path)?;
        Ok(to_json(s.records.into_iter().find(|r| r.id == id as u64)))
    }

    /// A session's record as `aws s3api` (`cli`) or unsigned curl (`curl`).
    pub fn copy(path: String, id: f64, kind: String) -> Result<Option<String>, String> {
        let s = open_session(&path)?;
        Ok(s.records.iter().find(|r| r.id == id as u64).map(|r| {
            if kind == "curl" {
                reqlog::copy::as_curl(r)
            } else {
                reqlog::copy::as_aws_cli(r)
            }
        }))
    }

    /// Starts recording: a local endpoint forwarding to `endpoint`, refusing
    /// what `policy` (`allow`, `no-delete`, `read-only`) forbids. What to
    /// point an application at, and the session file.
    pub async fn start(endpoint: String, policy: String) -> Result<JsonValue, String> {
        if RECORDING.lock().is_some() {
            return Err(err_msg("a recording is running: stop it first"));
        }
        let s = sonde();
        let client = s.client(&endpoint).map_err(crate::state::err)?;
        let policy = match policy.as_str() {
            "no-delete" => Policy::NoDelete,
            "read-only" => Policy::ReadOnly,
            _ => Policy::Allow,
        };
        let dir = s.paths().sessions();
        std::fs::create_dir_all(&dir).map_err(err_msg)?;
        let path = dir.join(format!(
            "{}-recording.sonde",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        let cfg = client.config();
        let header = SessionHeader::new(&endpoint, cfg.endpoint.as_str(), &cfg.region, None);
        let writer = Arc::new(SessionWriter::create(&path, &header).map_err(err_msg)?);
        let log = RequestLog::new(4096);
        log.set_sink(Some(writer.clone()));
        let started = background({
            let log = log.clone();
            async move {
                Proxy::start(
                    client,
                    log,
                    ProxyConfig {
                        policy,
                        ..Default::default()
                    },
                )
                .await
            }
        })
        .await
        .map_err(err_msg)?
        .map_err(err_msg)?;
        let (set, unset) = started.env();
        let view = json!({
            "url": started.url(),
            "path": path.to_string_lossy(),
            "endpoint": endpoint,
            "requests": 0,
            "exports": shell_exports(&set, &unset),
        });
        *RECORDING.lock() = Some(Recording {
            proxy: started,
            path,
            endpoint,
            log,
            writer,
        });
        Ok(view)
    }

    /// The running recording, or null.
    pub fn current() -> JsonValue {
        let guard = RECORDING.lock();
        let Some(r) = guard.as_ref() else {
            return JsonValue::Null;
        };
        let (set, unset) = r.proxy.env();
        json!({
            "url": r.proxy.url(),
            "path": r.path.to_string_lossy(),
            "endpoint": r.endpoint,
            "requests": r.log.list(&reqlog::Filter::default(), 1).first().map(|x| x.id).unwrap_or(0),
            "exports": shell_exports(&set, &unset),
        })
    }

    /// Writes the example session (a real SlateDB run, recorded with
    /// `sonde run`) to the sessions directory; its path.
    pub fn example() -> Result<String, String> {
        const EXAMPLE: &[u8] = include_bytes!("../../proxy/tests/fixtures/slatedb.sonde");
        let dir = sonde().paths().sessions();
        std::fs::create_dir_all(&dir).map_err(err_msg)?;
        let path = dir.join("example-slatedb.sonde");
        std::fs::write(&path, EXAMPLE).map_err(err_msg)?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Stops recording; the session file's path.
    pub fn stop() -> Option<String> {
        let r = RECORDING.lock().take()?;
        let _ = r.writer.flush();
        let path = r.path.to_string_lossy().into_owned();
        drop(r.proxy);
        Some(path)
    }
}
