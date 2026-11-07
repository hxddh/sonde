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
use sonde_proxy::replay::{self, ReplayOptions};
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
static RUNNING: Mutex<Option<crate::run::Running>> = Mutex::new(None);

/// Starts a recording: a local endpoint forwarding to `endpoint`.
async fn begin(
    endpoint: String,
    policy: String,
    command: Option<String>,
) -> Result<JsonValue, String> {
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
    let name = command
        .as_deref()
        .and_then(|c| c.split_whitespace().next())
        .and_then(|c| {
            Path::new(c)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "recording".into());
    let path = dir.join(format!(
        "{}-{name}.sonde",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    let cfg = client.config();
    let header = SessionHeader::new(&endpoint, cfg.endpoint.as_str(), &cfg.region, command);
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
        begin(endpoint, policy, None).await
    }

    /// Runs `command` in a pseudo-terminal with its S3 requests recorded:
    /// the program's output arrives as `{ data }` events (and goes into the
    /// session, timed), keys go back with [`Sessions::input`]. Resolves with
    /// `{ exit, path }` when the program ends; the recording ends with it.
    pub async fn run(
        command: String,
        cwd: Option<String>,
        endpoint: String,
        policy: String,
        cols: f64,
        rows: f64,
        #[args(event)] output: Callback<JsonValue>,
    ) -> Result<JsonValue, String> {
        if RUNNING.lock().is_some() {
            return Err(err_msg("a program is running: stop it first"));
        }
        let view = begin(endpoint, policy, Some(command.clone())).await?;
        let (env_set, env_unset, writer, path) = {
            let g = RECORDING.lock();
            let r = g.as_ref().expect("begin started a recording");
            let (set, unset) = r.proxy.env();
            (
                set,
                unset,
                r.writer.clone(),
                r.path.to_string_lossy().into_owned(),
            )
        };
        let started = crate::run::spawn(
            &command,
            cwd.as_deref(),
            &env_set,
            &env_unset,
            cols as u16,
            rows as u16,
        );
        let (mut out_rx, exit_rx, handle) = match started {
            Ok(x) => x,
            Err(e) => {
                Sessions::stop();
                return Err(err_msg(format!("cannot start {command}: {e}")));
            }
        };
        *RUNNING.lock() = Some(handle);
        output.post(json!({ "started": view }));
        let mut decoder = crate::run::Utf8Stream::default();
        let mut exit_rx = exit_rx;
        let mut code = None;
        let mut exited = false;
        // after the program exits on Windows: what is left to read, briefly
        let mut drain: Option<tokio::time::Instant> = None;
        loop {
            let deadline = drain.unwrap_or_else(|| {
                tokio::time::Instant::now() + std::time::Duration::from_secs(86_400)
            });
            tokio::select! {
                bytes = out_rx.recv() => match bytes {
                    Some(bytes) => {
                        let text = decoder.push(&bytes);
                        if !text.is_empty() {
                            writer.write_log(&text);
                            output.post(json!({ "data": text }));
                        }
                    }
                    None => break,
                },
                c = &mut exit_rx, if !exited => {
                    exited = true;
                    code = c.unwrap_or(None);
                    // ConPTY keeps the program's output open until the
                    // pseudo-console closes, so a Windows run would never end:
                    // close it, then read what is left. Elsewhere the output
                    // closes when every process holding the terminal is gone.
                    if cfg!(windows) {
                        RUNNING.lock().take();
                        drain = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(2));
                    }
                }
                _ = tokio::time::sleep_until(deadline), if drain.is_some() => break,
            }
        }
        if !exited {
            code = exit_rx.await.unwrap_or(None);
        }
        RUNNING.lock().take();
        Sessions::stop();
        Ok(json!({ "exit": code, "path": path }))
    }

    /// Text for the running program (what the user typed into its terminal).
    pub fn input(data: String) -> bool {
        RUNNING
            .lock()
            .as_mut()
            .is_some_and(|r| r.write(data.as_bytes()))
    }

    /// The running program's terminal size, in cells.
    pub fn resize(cols: f64, rows: f64) -> bool {
        RUNNING
            .lock()
            .as_ref()
            .is_some_and(|r| r.resize(cols as u16, rows as u16))
    }

    /// Ends the running program.
    pub fn kill() -> bool {
        RUNNING.lock().as_mut().is_some_and(|r| r.kill())
    }

    /// What replaying a session on `endpoint` would send: the plan, the
    /// prefix it would write under, the buckets, whether sonde is read-only.
    pub fn replay_plan(path: String, endpoint: String) -> Result<JsonValue, String> {
        let original = open_session(&path)?;
        let s = sonde();
        let base = s.client(&endpoint).map_err(crate::state::err)?;
        let opts = ReplayOptions {
            prefix: replay::default_prefix(),
            buckets: Vec::new(),
        };
        Ok(json!({
            "plan": replay::plan(&original.records, &opts),
            "prefix": opts.prefix,
            "buckets": replay::buckets_of(&original.records, &opts),
            "endpoint": endpoint,
            "url": base.config().endpoint.as_str(),
            "readOnly": s.settings().read_only,
        }))
    }

    /// Replays a session on `endpoint` under `prefix`, cleans up, and
    /// compares: `{ path, plan, comparison, removed }`. Refused while sonde is
    /// read-only, as every write it makes.
    pub async fn replay(
        path: String,
        endpoint: String,
        prefix: String,
    ) -> Result<JsonValue, String> {
        let s = sonde();
        if s.settings().read_only {
            return Err(crate::state::err(sonde_core::S3Error::new(
                s3raw::ErrorKind::Refused,
                "read-only mode is on: turn it off in Settings to replay (a replay writes under its prefix)",
            )));
        }
        let original = open_session(&path)?;
        let base = s.client(&endpoint).map_err(crate::state::err)?;
        let out = s.paths().sessions().join(format!(
            "{}-replay.sonde",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        let opts = ReplayOptions {
            prefix,
            buckets: Vec::new(),
        };
        let run = background(async move {
            replay::replay_to_file(&base, &original, &opts, &out, false, |_, _| {}).await
        })
        .await
        .map_err(err_msg)?
        .map_err(err_msg)?;
        Ok(to_json(run))
    }

    /// Two sessions side by side: how their answers compare (a replay's
    /// records name the original's), and each as a transcript for a diff.
    pub fn compare(a: String, b: String) -> Result<JsonValue, String> {
        let (sa, sb) = (open_session(&a)?, open_session(&b)?);
        Ok(json!({
            "comparison": replay::compare(&sa.records, &sb.records, &[]),
            "a": replay::transcript(&sa.records),
            "b": replay::transcript(&sb.records),
        }))
    }

    /// A session's output as timed lines, for lining up with its operations.
    pub fn log_lines(path: String) -> Result<JsonValue, String> {
        let s = open_session(&path)?;
        Ok(to_json(sonde_proxy::logs::lines(&s.logs)))
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
        // a real recording (SlateDB on RustFS), named as a user would have
        // run it rather than from the build tree that made it
        let text = String::from_utf8_lossy(EXAMPLE)
            .replace(
                "\"command\":\"tools/gen-slatedb/target/release/gen-slatedb s3://slate\"",
                "\"command\":\"gen-slatedb s3://slate\"",
            )
            .replace("\"endpoint\":\"adhoc\"", "\"endpoint\":\"rustfs\"");
        std::fs::write(&path, text).map_err(err_msg)?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Adds a session file the user picked (its name and text) to the
    /// sessions directory, if it reads as one; its new path.
    pub fn import(name: String, text: String) -> Result<String, String> {
        let dir = sonde().paths().sessions();
        std::fs::create_dir_all(&dir).map_err(err_msg)?;
        let stem: String = Path::new(&name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || "-_.".contains(c) {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let stem = if stem.is_empty() {
            "imported".to_string()
        } else {
            stem
        };
        let mut path = dir.join(format!("{stem}.sonde"));
        let mut n = 2;
        while path.exists() {
            path = dir.join(format!("{stem}-{n}.sonde"));
            n += 1;
        }
        std::fs::write(&path, text).map_err(err_msg)?;
        if let Err(e) = Session::open(&path) {
            let _ = std::fs::remove_file(&path);
            return Err(format!("{name} is not a sonde session: {e}"));
        }
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
