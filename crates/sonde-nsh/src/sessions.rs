//! Sessions as TSX sees them: recorded files, their operations, and a live
//! recording. Credentials stay in Rust: the recording's endpoint signs with
//! them; the application gets dummy keys and the proxy's URL.

use std::io::BufRead;
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

/// FNV-1a, 64 bits: which file a session was made from.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The files opened into sessions, by their contents' digest: `imports.json`.
fn read_imports(dir: &Path) -> std::collections::BTreeMap<String, String> {
    std::fs::read(dir.join("imports.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_imports(dir: &Path, all: &std::collections::BTreeMap<String, String>) {
    let tmp = dir.join(format!("imports.json.{}.tmp", std::process::id()));
    if serde_json::to_vec(all)
        .ok()
        .is_some_and(|b| std::fs::write(&tmp, b).is_ok())
    {
        let _ = std::fs::rename(&tmp, dir.join("imports.json"));
    }
}

/// The platform has no trash (as opposed to moving there failing).
fn no_trash(e: &str) -> bool {
    let e = e.to_ascii_lowercase();
    e.contains("not supported") || e.contains("no trash") || e.contains("or trash")
}

/// Starts a recording: a local endpoint forwarding to `endpoint`.
async fn begin(
    endpoint: String,
    policy: String,
    command: Option<String>,
    cwd: Option<std::path::PathBuf>,
) -> Result<JsonValue, String> {
    // a recording starting counts as running: two starts at once would both
    // pass the check below before either set RECORDING
    static STARTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    struct Starting;
    impl Drop for Starting {
        fn drop(&mut self) {
            STARTING.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
    if RECORDING.lock().is_some() || STARTING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err(err_msg("a recording is running: stop it first"));
    }
    let _starting = Starting;
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
        .and_then(sonde_proxy::program::program_name)
        .unwrap_or_else(|| "recording".into());
    let path = sonde_proxy::session::fresh_path(
        &dir,
        &format!("{}-{name}", chrono::Local::now().format("%Y%m%d-%H%M%S")),
    );
    let cfg = client.config();
    let mut header = SessionHeader::new(&endpoint, cfg.endpoint.as_str(), &cfg.region, command);
    if let Some(dir) = &cwd {
        header = header.ran_in(dir);
    }
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

/// A session read and analysed once for as long as its file is unchanged
/// (same length, same modification time): reopening a session, its
/// records, its output or a live recording that has not grown costs nothing.
struct Cached {
    stamp: (u64, Option<std::time::SystemTime>),
    /// For a growing (uncompressed) file: where its unread lines start.
    read_to: Option<u64>,
    used: u64,
    session: Arc<Session>,
    view: Option<Arc<JsonValue>>,
}

/// The sessions kept read: the open ones and a few recent.
const KEEP: usize = 8;
static CACHE: std::sync::LazyLock<Mutex<std::collections::HashMap<PathBuf, Cached>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
static USE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn stamp(path: &Path) -> Result<(u64, Option<std::time::SystemTime>), String> {
    let m = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((m.len(), m.modified().ok()))
}

fn open_session(path: &str) -> Result<Arc<Session>, String> {
    let p = Path::new(path);
    let now = stamp(p)?;
    let used = USE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // a finished recording is compressed in place, and a tiny one can come
    // out larger: never read a compressed file as appended lines
    let compressed = sonde_proxy::session::is_compressed(p);
    let grown = {
        let mut cache = CACHE.lock();
        match cache.get_mut(p) {
            Some(c) if c.stamp == now => {
                c.used = used;
                return Ok(c.session.clone());
            }
            // a live recording that grew: read only what it appended (taken
            // out of the cache so that, unshared, it grows without a copy)
            Some(c) if !compressed && now.0 > c.stamp.0 && c.read_to.is_some() => {
                cache.remove(p).map(|c| (c.session, c.read_to.unwrap_or(0)))
            }
            _ => None,
        }
    };
    // the same recording, grown; not another written in its place
    let grown = grown.filter(|(old, _)| header_of(p).as_ref() == Some(&old.header));
    let (session, read_to) = match grown {
        Some((old, from)) => {
            let mut s = Arc::unwrap_or_clone(old);
            let to = s.read_more(p, from).map_err(err_msg)?;
            (Arc::new(s), Some(to))
        }
        None => {
            // where complete lines end, taken before the read: the next read starts there
            let to = (!compressed)
                .then(|| sonde_proxy::session::complete_len(p).ok())
                .flatten();
            // read outside the lock: another session opens meanwhile
            (Arc::new(Session::open(p).map_err(err_msg)?), to)
        }
    };
    let mut cache = CACHE.lock();
    if cache.len() >= KEEP && !cache.contains_key(p) {
        let oldest = cache
            .iter()
            .min_by_key(|(_, c)| c.used)
            .map(|(k, _)| k.clone());
        if let Some(k) = oldest {
            cache.remove(&k);
        }
    }
    cache.insert(
        p.to_path_buf(),
        Cached {
            stamp: now,
            read_to,
            used,
            session: session.clone(),
            view: None,
        },
    );
    Ok(session)
}

/// What [`Sessions::open`] returns, built once per version of the file.
fn view_of(path: &str) -> Result<Arc<JsonValue>, String> {
    let s = open_session(path)?;
    let p = Path::new(path);
    if let Some(v) = CACHE
        .lock()
        .get(p)
        .filter(|c| Arc::ptr_eq(&c.session, &s))
        .and_then(|c| c.view.clone())
    {
        return Ok(v);
    }
    let eps = episodes(&s.records);
    let obs = observations(&s.records, &eps);
    let shared: Vec<String> = shared_notes(&eps)
        .into_iter()
        .map(|(_, n, _, _)| n)
        .collect();
    let prof = sonde_proxy::profile::profile(
        &s.records,
        &crate::state::request_prices_for(&s.header.endpoint),
    );
    let what_if = sonde_proxy::fixes::what_if(&s.records, &prof, &obs);
    let view = Arc::new(json!({
        "path": path,
        "header": s.header,
        "summary": summarize(&s.records),
        "profile": prof,
        "whatIf": what_if,
        "episodes": eps,
        "observations": obs,
        "sharedNotes": shared,
    }));
    if let Some(c) = CACHE.lock().get_mut(p)
        && Arc::ptr_eq(&c.session, &s)
    {
        c.view = Some(view.clone());
    }
    Ok(view)
}

/// path → (bytes counted up to the last complete line, records in them)
static COUNTS: std::sync::LazyLock<
    Mutex<std::collections::HashMap<PathBuf, (u64, usize, Option<String>)>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Forgets what was read of a session file (it was removed or replaced).
fn forget(path: &Path) {
    CACHE.lock().remove(path);
    COUNTS.lock().remove(path);
}

/// The titles given to sessions, by file name: `titles.json` beside them.
fn titles() -> std::collections::BTreeMap<String, String> {
    std::fs::read(sonde().paths().sessions().join("titles.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_titles(t: &std::collections::BTreeMap<String, String>) -> Result<(), String> {
    let dir = sonde().paths().sessions();
    let tmp = dir.join("titles.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(t).map_err(err_msg)?).map_err(err_msg)?;
    std::fs::rename(&tmp, dir.join("titles.json")).map_err(err_msg)
}

/// A session of the sessions directory, by path: what may be renamed or
/// removed (not a file elsewhere on the disk).
fn own_session(path: &str) -> Result<(PathBuf, String), String> {
    let p = Path::new(path);
    let dir = sonde().paths().sessions();
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let here = p.parent().and_then(|d| d.canonicalize().ok());
    let inside = here.is_some() && here == dir.canonicalize().ok();
    if !inside || !name.ends_with(".sonde") || !p.is_file() {
        return Err(format!("{path} is not one of sonde's sessions"));
    }
    // as the list names it (a dropped file's path may be spelled otherwise:
    // forward slashes on Windows, a symlinked folder)
    Ok((dir.join(&name), name))
}

/// A run's figures, kept per version of its file (a history reads many runs).
static POINTS: std::sync::LazyLock<
    Mutex<
        std::collections::HashMap<
            PathBuf,
            (
                (u64, Option<std::time::SystemTime>),
                sonde_proxy::program::RunPoint,
            ),
        >,
    >,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

fn run_point(path: &Path) -> Option<sonde_proxy::program::RunPoint> {
    let now = stamp(path).ok()?;
    if let Some((s, p)) = POINTS.lock().get(path)
        && *s == now
    {
        return Some(p.clone());
    }
    // kept on disk too, beside the sessions: a program's history reads 30
    // runs, and a finished run does not change
    let name = path.file_name()?.to_string_lossy().into_owned();
    let mtime = now
        .1
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let disk = path.parent()?.join(".runpoints.json");
    let mut stored: std::collections::HashMap<String, (u64, u64, sonde_proxy::program::RunPoint)> =
        std::fs::read(&disk)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
    if let Some((len, t, p)) = stored.get(&name)
        && *len == now.0
        && *t == mtime
    {
        let mut p = p.clone();
        p.path = path.to_string_lossy().into_owned();
        POINTS.lock().insert(path.to_path_buf(), (now, p.clone()));
        return Some(p);
    }
    let s = Session::open(path).ok()?;
    let pr = sonde_proxy::profile::profile(
        &s.records,
        &crate::state::request_prices_for(&s.header.endpoint),
    );
    let ob = observations(&s.records, &episodes(&s.records));
    let p = sonde_proxy::program::run_point(
        &path.to_string_lossy(),
        &s.header.created.to_rfc3339(),
        &s.records,
        &pr,
        &ob,
    );
    POINTS.lock().insert(path.to_path_buf(), (now, p.clone()));
    // only finished runs: a live one changes
    if sonde_proxy::session::is_compressed(path) {
        let dir = path.parent()?;
        stored.retain(|n, _| dir.join(n).is_file());
        stored.insert(name, (now.0, mtime, p.clone()));
        let tmp = dir.join(".runpoints.json.tmp");
        if serde_json::to_vec(&stored)
            .ok()
            .and_then(|b| std::fs::write(&tmp, b).ok())
            .is_some()
        {
            let _ = std::fs::rename(&tmp, &disk);
        }
    }
    Some(p)
}

/// Each program's baseline run, by file name: `baselines.json` beside the sessions.
fn baselines() -> std::collections::BTreeMap<String, String> {
    sonde_proxy::program::read_baselines(&sonde().paths().sessions())
}

/// The sites shared from the app, each served on its own local port.
static SHARED: std::sync::LazyLock<Mutex<std::collections::HashMap<PathBuf, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// The requests in a session file: its record lines (not the header, not the
/// program's output). Counted once and then only over what was appended, so
/// listing sessions does not read every file whole each time.
fn count_records(path: &Path) -> usize {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(len) = std::fs::metadata(path).map(|m| m.len()) else {
        return 0;
    };
    // the file's first line tells one session from another written in its place
    let header = header_line(path);
    let (mut from, mut n, seen) = COUNTS.lock().get(path).cloned().unwrap_or((0, 0, None));
    if len < from || (from > 0 && seen != header) {
        // rewritten: count again
        (from, n) = (0, 0);
    }
    // a finished session is compressed: counted whole, once
    if len > from && sonde_proxy::session::is_compressed(path) {
        let n = sonde_proxy::session::reader(path)
            .map(|r| {
                r.split(b'\n')
                    .skip(1)
                    .filter_map(Result::ok)
                    .filter(|l| !l.is_empty() && l.starts_with(b"{") && !l.starts_with(b"{\"log\""))
                    .count()
            })
            .unwrap_or(0);
        COUNTS.lock().insert(path.to_path_buf(), (len, n, header));
        return n;
    }
    if len > from {
        let Ok(mut f) = std::fs::File::open(path) else {
            return n;
        };
        let mut buf = Vec::with_capacity((len - from) as usize);
        if f.seek(SeekFrom::Start(from)).is_err() || f.read_to_end(&mut buf).is_err() {
            return n;
        }
        // complete lines only: the last may still be being written
        let end = buf.iter().rposition(|c| *c == b'\n').map_or(0, |i| i + 1);
        // records are the lines after the header that are not output
        let mut lines = buf[..end].split(|c| *c == b'\n');
        if from == 0 {
            lines.next();
        }
        n += lines
            .filter(|l| !l.is_empty() && l.starts_with(b"{") && !l.starts_with(b"{\"log\""))
            .count();
        from += end as u64;
        COUNTS.lock().insert(path.to_path_buf(), (from, n, header));
    }
    n
}

/// A finished session's requests over its time, in `SHAPE_BINS` slices: the
/// list's sparkline. Read once per version of the file; a live session has
/// none (it is still growing).
const SHAPE_BINS: usize = 24;
static SHAPES: std::sync::LazyLock<Mutex<std::collections::HashMap<PathBuf, (u64, Vec<u32>)>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

fn shape_of(path: &Path) -> Vec<u32> {
    let Ok(len) = std::fs::metadata(path).map(|m| m.len()) else {
        return Vec::new();
    };
    if !sonde_proxy::session::is_compressed(path) {
        return Vec::new();
    }
    if let Some((l, s)) = SHAPES.lock().get(path)
        && *l == len
    {
        return s.clone();
    }
    // the request's start, from its line, without reading the rest of it
    let time_of = |l: &[u8]| -> Option<i64> {
        let line = std::str::from_utf8(l).ok()?;
        let at = line.find("\"time\":\"")? + 8;
        let end = line[at..].find('"')? + at;
        chrono::DateTime::parse_from_rfc3339(&line[at..end])
            .ok()
            .map(|t| t.timestamp_micros())
    };
    let times: Vec<i64> = sonde_proxy::session::reader(path)
        .map(|r| {
            r.split(b'\n')
                .skip(1)
                .filter_map(Result::ok)
                .filter(|l| l.starts_with(b"{") && !l.starts_with(b"{\"log\""))
                .filter_map(|l| time_of(&l))
                .collect()
        })
        .unwrap_or_default();
    let mut bins = vec![0u32; SHAPE_BINS];
    if let (Some(a), Some(b)) = (times.iter().min(), times.iter().max()) {
        let span = (b - a).max(1) as f64;
        for t in &times {
            let i = (((t - a) as f64 / span) * SHAPE_BINS as f64) as usize;
            bins[i.min(SHAPE_BINS - 1)] += 1;
        }
    }
    SHAPES
        .lock()
        .insert(path.to_path_buf(), (len, bins.clone()));
    bins
}

fn header_line(path: &Path) -> Option<String> {
    let mut first = String::new();
    sonde_proxy::session::reader(path)
        .ok()?
        .read_line(&mut first)
        .ok()?;
    Some(first)
}

fn header_of(path: &Path) -> Option<SessionHeader> {
    serde_json::from_str(&header_line(path)?).ok()
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
        let titles = titles();
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
                        "title": p.file_name().and_then(|n| titles.get(&*n.to_string_lossy())),
                        "created": created,
                        "endpoint": h.endpoint,
                        "command": h.command,
                        "requests": count_records(&p),
                        "shape": shape_of(&p),
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
        Ok((*view_of(&path)?).clone())
    }

    /// [`Sessions::open`] off the interface's thread: what a window waits on
    /// while a large session is read, or a live one grows.
    pub async fn analyse(path: String) -> Result<JsonValue, String> {
        tokio::task::spawn_blocking(move || view_of(&path).map(|v| (*v).clone()))
            .await
            .map_err(err_msg)?
    }

    /// Short rows for the records with these ids (an operation's requests).
    pub fn records(path: String, ids: Vec<f64>) -> Result<JsonValue, String> {
        let s = open_session(&path)?;
        // one pass: an operation of thousands of requests in a session of millions
        let want: std::collections::HashMap<u64, usize> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| (*id as u64, i))
            .collect();
        let mut found: Vec<Option<&reqlog::Record>> = vec![None; ids.len()];
        for r in &s.records {
            if let Some(&i) = want.get(&r.id) {
                found[i] = Some(r);
            }
        }
        let rows: Vec<_> = found.into_iter().flatten().map(|r| r.summary()).collect();
        Ok(to_json(rows))
    }

    /// One record of a session, in full.
    pub fn record(path: String, id: f64) -> Result<JsonValue, String> {
        let s = open_session(&path)?;
        Ok(to_json(s.records.iter().find(|r| r.id == id as u64)))
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
        begin(endpoint, policy, None, None).await
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
        let dir = crate::run::folder(cwd.as_deref());
        let view = begin(endpoint, policy, Some(command.clone()), dir).await?;
        let (env_set, env_unset, writer, path) = {
            let g = RECORDING.lock();
            let r = g.as_ref().expect("begin started a recording");
            let (mut set, unset) = r.proxy.env();
            // the code that sent each request, when asked (a Python program's)
            if sonde().settings().name_callers
                && let Ok(v) = sonde_proxy::callers::python_env(&sonde().paths().python_hook())
            {
                set.push(v);
            }
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
        let out = sonde_proxy::session::fresh_path(
            &s.paths().sessions(),
            &format!("{}-replay", chrono::Local::now().format("%Y%m%d-%H%M%S")),
        );
        let prefix = replay::normalize_prefix(&prefix)?;
        let opts = ReplayOptions {
            prefix,
            buckets: Vec::new(),
        };
        let file = out.clone();
        let run = background(async move {
            replay::replay_to_file(&base, &original, &opts, &out, false, |_, _| {}).await
        })
        .await
        .map_err(err_msg)?
        .map_err(err_msg)?;
        let _ = sonde_proxy::session::compress(&file);
        Ok(to_json(run))
    }

    /// Two sessions side by side: how their answers compare (a replay's
    /// records name the original's), and each as a transcript for a diff.
    pub async fn compare(a: String, b: String) -> Result<JsonValue, String> {
        tokio::task::spawn_blocking(move || {
            let (sa, sb) = (open_session(&a)?, open_session(&b)?);
            let price = |s: &Session| crate::state::request_prices_for(&s.header.endpoint);
            let latency = sonde_proxy::profile::latency_changes(
                &sonde_proxy::profile::profile(&sa.records, &price(&sa)),
                &sonde_proxy::profile::profile(&sb.records, &price(&sb)),
            );
            Ok(json!({
                "latency": latency,
                "endpoints": [&sa.header.endpoint, &sb.header.endpoint],
                "comparison": replay::compare(&sa.records, &sb.records, &[]),
                "a": replay::transcript(&sa.records),
                "b": replay::transcript(&sb.records),
            }))
        })
        .await
        .map_err(err_msg)?
    }

    /// Two runs of a program, before and after a change: requests, time,
    /// cost and findings, by kind of request.
    pub async fn compare_runs(before: String, after: String) -> Result<JsonValue, String> {
        tokio::task::spawn_blocking(move || {
            let (a, b) = (open_session(&before)?, open_session(&after)?);
            let price = |s: &Session| crate::state::request_prices_for(&s.header.endpoint);
            let (pa, pb) = (
                sonde_proxy::profile::profile(&a.records, &price(&a)),
                sonde_proxy::profile::profile(&b.records, &price(&b)),
            );
            let (oa, ob) = (
                observations(&a.records, &episodes(&a.records)),
                observations(&b.records, &episodes(&b.records)),
            );
            Ok(to_json(
                sonde_proxy::profile::compare_runs((&a.records, &pa, &oa), (&b.records, &pb, &ob))
                    .of_programs(a.header.command.as_deref(), b.header.command.as_deref()),
            ))
        })
        .await
        .map_err(err_msg)?
    }

    /// The programs run here, newest first: each with its last runs'
    /// figures (oldest first, for a trend), its baseline, and whether its
    /// newest run is past it (more than 10% more requests or waiting).
    pub async fn programs() -> Result<JsonValue, String> {
        tokio::task::spawn_blocking(move || {
            let dir = sonde().paths().sessions();
            let mut by: std::collections::BTreeMap<String, Vec<(String, PathBuf)>> =
                Default::default();
            for p in std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sonde"))
            {
                if let Some(h) = header_of(&p)
                    && let Some(k) = sonde_proxy::program::program_key(h.command.as_deref())
                {
                    by.entry(k).or_default().push((h.created.to_rfc3339(), p));
                }
            }
            let bases = baselines();
            let mut out: Vec<(String, JsonValue)> = by
                .into_iter()
                .map(|(key, mut runs)| {
                    runs.sort();
                    let n = runs.len();
                    let points: Vec<_> = runs
                        .iter()
                        .rev()
                        .take(12)
                        .rev()
                        .filter_map(|(_, p)| run_point(p))
                        .collect();
                    let base = bases.get(&key).and_then(|name| {
                        let p = dir.join(name);
                        p.is_file().then(|| run_point(&p)).flatten()
                    });
                    let latest = points.last();
                    // the limits `sonde check` holds the run to, the default tolerance
                    let tolerance = sonde_proxy::program::DEFAULT_TOLERANCE;
                    let regressed = match (latest, &base) {
                        (Some(l), Some(b)) if l.path != b.path => {
                            sonde_proxy::program::past_baseline(l, b, tolerance)
                        }
                        _ => false,
                    };
                    let limits = base
                        .as_ref()
                        .map(|b| sonde_proxy::program::baseline_limits(b, tolerance));
                    let newest = runs.last().map(|r| r.0.clone()).unwrap_or_default();
                    (
                        newest.clone(),
                        json!({
                            "program": key,
                            "runs": n,
                            "latest": runs.last().map(|r| r.1.to_string_lossy().into_owned()),
                            "created": newest,
                            "points": points,
                            "baseline": base.as_ref().map(|b| b.path.clone()),
                            "monthly": sonde_proxy::program::monthly_cost(&points)
                                .map(|(cost, per_day)| json!({ "cost": cost, "perDay": per_day })),
                            "baselinePoint": base,
                            "limits": limits,
                            "regressed": regressed,
                        }),
                    )
                })
                .collect();
            out.sort_by(|a, b| b.0.cmp(&a.0));
            Ok(JsonValue::Array(out.into_iter().map(|(_, v)| v).collect()))
        })
        .await
        .map_err(err_msg)?
    }

    /// The program a session ran, over its runs (the newest 30, oldest first),
    /// its baseline run, and this run checked against it (10% to grow, no new
    /// warning): `{ program, runs, baseline, check }`; `null` for a replay or a
    /// recording without a command.
    pub async fn history(path: String) -> Result<JsonValue, String> {
        tokio::task::spawn_blocking(move || {
            let Some(key) = header_of(Path::new(&path))
                .and_then(|h| sonde_proxy::program::program_key(h.command.as_deref()))
            else {
                return Ok(JsonValue::Null);
            };
            let dir = sonde().paths().sessions();
            let mut runs: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sonde"))
                .filter_map(|p| {
                    let h = header_of(&p)?;
                    (sonde_proxy::program::program_key(h.command.as_deref()).as_deref()
                        == Some(key.as_str()))
                    .then(|| (h.created.to_rfc3339(), p))
                })
                .collect();
            runs.sort();
            let runs: Vec<_> = runs
                .into_iter()
                .rev()
                .take(30)
                .rev()
                .filter_map(|(_, p)| run_point(&p))
                .collect();
            let base = baselines()
                .get(&key)
                .map(|name| dir.join(name))
                .filter(|p| p.is_file());
            let check = match &base {
                Some(b) if b.to_string_lossy() != path => {
                    let read = |p: &str| -> Result<_, String> {
                        let s = open_session(p)?;
                        let pr = sonde_proxy::profile::profile(
                            &s.records,
                            &crate::state::request_prices_for(&s.header.endpoint),
                        );
                        let ob = observations(&s.records, &episodes(&s.records));
                        let pt = sonde_proxy::program::run_point(
                            p,
                            &s.header.created.to_rfc3339(),
                            &s.records,
                            &pr,
                            &ob,
                        );
                        Ok((pt, s, pr, ob))
                    };
                    let (rp, rs, rpr, rob) = read(&path)?;
                    let (bp, bs, bpr, bob) = read(&b.to_string_lossy())?;
                    let budget = sonde_proxy::program::Budget {
                        tolerance: Some(0.1),
                        no_new_warnings: true,
                        ..Default::default()
                    };
                    Some(sonde_proxy::program::check(
                        (&rp, &rs.records, &rpr, &rob),
                        Some((&bp, &bs.records, &bpr, &bob)),
                        &budget,
                    ))
                }
                _ => None,
            };
            // the baseline's own figures, though it be older than the runs shown,
            // and what a run may grow to before `sonde check` fails it
            let base_point = base.as_ref().and_then(|b| run_point(b));
            let limits = base_point.as_ref().map(|b| {
                sonde_proxy::program::baseline_limits(b, sonde_proxy::program::DEFAULT_TOLERANCE)
            });
            Ok(json!({
                "program": key,
                "runs": runs,
                "baseline": base.map(|b| b.to_string_lossy().into_owned()),
                "baselinePoint": base_point,
                "limits": limits,
                "check": check,
            }))
        })
        .await
        .map_err(err_msg)?
    }

    /// Makes a session its program's baseline (`on`), or no longer.
    pub fn set_baseline(path: String, on: bool) -> Result<bool, String> {
        let (_, name) = own_session(&path)?;
        let key = header_of(Path::new(&path))
            .and_then(|h| sonde_proxy::program::program_key(h.command.as_deref()))
            .ok_or_else(|| err_msg("only a run of a program has a baseline"))?;
        let mut all = baselines();
        if on {
            all.insert(key, name);
        } else if all.get(&key) == Some(&name) {
            all.remove(&key);
        }
        sonde_proxy::program::write_baselines(&sonde().paths().sessions(), &all)
            .map_err(err_msg)?;
        Ok(true)
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
        // the same file opened again is the session it made, not a copy
        let digest = format!("{:016x}", fnv1a(text.as_bytes()));
        let mut imported = read_imports(&dir);
        if let Some(p) = imported
            .get(&digest)
            .map(|n| dir.join(n))
            .filter(|p| p.is_file())
        {
            return Ok(p.to_string_lossy().into_owned());
        }
        let path = sonde_proxy::session::fresh_path(&dir, &stem);
        // production traffic: an access log, CloudTrail events, a HAR
        if sonde_proxy::import::detect(&text).is_some() {
            let (_, session) = sonde_proxy::import::import(&text, &stem)?;
            sonde_proxy::import::save(&session, &path).map_err(err_msg)?;
        } else {
            std::fs::write(&path, text).map_err(err_msg)?;
            if let Err(e) = Session::open(&path) {
                let _ = std::fs::remove_file(&path);
                return Err(format!("{name} is not a sonde session: {e}"));
            }
        }
        if let Some(n) = path.file_name() {
            imported.insert(digest, n.to_string_lossy().into_owned());
            write_imports(&dir, &imported);
        }
        Ok(path.to_string_lossy().into_owned())
    }

    /// sonde's own requests (the request log: browsing, scans, probes) as a
    /// session, read like any program's: its findings, its time, compared.
    /// The newest `limit` requests, oldest first; the path of the new file.
    pub fn from_request_log(limit: u32) -> Result<String, String> {
        let mut records = sonde()
            .log()
            .list(&reqlog::Filter::default(), limit as usize);
        if records.is_empty() {
            return Err(err_msg("sonde has made no requests yet"));
        }
        records.reverse();
        let dir = sonde().paths().sessions();
        std::fs::create_dir_all(&dir).map_err(err_msg)?;
        let path = sonde_proxy::session::fresh_path(&dir, "sonde-self");
        let endpoint = records[0].endpoint.clone();
        let mut header = SessionHeader::new(&endpoint, "", "", None);
        header.created = records[0].time;
        let session = Session {
            header,
            records,
            logs: Vec::new(),
        };
        sonde_proxy::import::save(&session, &path).map_err(err_msg)?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// The system's open dialog for a session or a log of traffic (`kinds`:
    /// the two file types' names, as the app says them), the file added as
    /// [`Sessions::import_path`]: read here, never through the app's script
    /// engine, however large. `None` when cancelled.
    pub async fn pick(
        sessions_kind: String,
        traffic_kind: String,
    ) -> Result<Option<String>, String> {
        use nsh_core::file_dialog::{FileFilter, OpenDialog};
        let dialog = OpenDialog::files()
            .filter(FileFilter::new(sessions_kind, ["sonde"]))
            .filter(FileFilter::new(
                traffic_kind,
                ["log", "txt", "json", "jsonl", "har"],
            ));
        let Some(paths) = nsh_core::element::pick_paths(dialog).await? else {
            return Ok(None);
        };
        let Some(path) = paths.into_iter().next() else {
            return Ok(None);
        };
        let path = path.to_string_lossy().into_owned();
        tokio::task::spawn_blocking(move || Self::import_path(path))
            .await
            .map_err(err_msg)?
            .map(Some)
    }

    /// Adds a session file at `path` (dropped on the window) to the sessions
    /// directory, as [`Sessions::import`]; one already there is opened as it is.
    pub fn import_path(path: String) -> Result<String, String> {
        if let Ok((own, _)) = own_session(&path) {
            return Ok(own.to_string_lossy().into_owned());
        }
        let p = Path::new(&path);
        // bytes, as a session is read: a torn last character is a torn line
        let text = std::fs::read(p)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .map_err(|e| format!("{path}: {e}"))?;
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::import(name, text)
    }

    /// Gives a session a title of the user's (an empty one takes it away).
    pub fn rename(path: String, title: String) -> Result<bool, String> {
        let (_, name) = own_session(&path)?;
        let mut t = titles();
        let title = title.trim();
        if title.is_empty() {
            t.remove(&name);
        } else {
            t.insert(name, title.chars().take(200).collect());
        }
        save_titles(&t)?;
        Ok(true)
    }

    /// Removes a session file (not the one being recorded): to the system's
    /// trash, so it can come back; deleted where there is none.
    pub async fn remove(path: String) -> Result<bool, String> {
        let (p, name) = own_session(&path)?;
        if RECORDING.lock().as_ref().is_some_and(|r| r.path == p) {
            return Err(err_msg("this session is being recorded: stop it first"));
        }
        let abs = std::path::absolute(&p).map_err(err_msg)?;
        // to the trash, which gives it back; deleted only where there is
        // no trash at all (a terminal, a test host), never because moving failed
        if let Err(e) = nsh_core::element::move_to_trash(abs).await {
            if !no_trash(&e) {
                return Err(err_msg(format!("could not move it to the trash: {e}")));
            }
            std::fs::remove_file(&p).map_err(err_msg)?;
        }
        forget(&p);
        let mut t = titles();
        if t.remove(&name).is_some() {
            save_titles(&t)?;
        }
        Ok(true)
    }

    /// Shows `path` (a session, or a folder sonde wrote) selected in the
    /// file manager.
    pub async fn reveal(path: String) -> Result<(), String> {
        let abs = std::path::absolute(Path::new(&path)).map_err(err_msg)?;
        nsh_core::element::reveal_path(abs).await
    }

    /// Writes a session as a web page (`sonde share`'s site) under the data
    /// directory and serves it on 127.0.0.1: `{ dir, url }`. The page is
    /// the same views as the app, read-only, for anyone it is sent to.
    pub async fn share(path: String) -> Result<JsonValue, String> {
        if !sonde_proxy::share::has_viewer() {
            return Err(err_msg(
                "this build has no web viewer: `sonde share` makes the page",
            ));
        }
        let session = open_session(&path)?;
        let stem = Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "session".into());
        // the stem and where the file is: two sessions of one name never share a page
        // FNV-1a of the path: the same folder from one version of sonde to the next
        let hash = path.bytes().fold(0x811c_9dc5u32, |h, b| {
            (h ^ b as u32).wrapping_mul(0x0100_0193)
        });
        let dir = sonde().paths().shared().join(format!("{stem}-{hash:08x}"));
        let doc = serde_json::to_vec(&sonde_proxy::share::session_json(
            &session,
            &crate::state::request_prices_for(&session.header.endpoint),
        ))
        .map_err(err_msg)?;
        let site = dir.clone();
        tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&site)?;
            sonde_proxy::share::write_site(&site, &doc)
        })
        .await
        .map_err(err_msg)?
        .map_err(err_msg)?;
        let served = SHARED.lock().get(&dir).cloned();
        let url = match served {
            Some(u) => u,
            None => {
                let root = dir.clone();
                let addr = background(async move {
                    let l = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
                    let addr = l.local_addr()?;
                    tokio::spawn(sonde_proxy::share::serve(l, root));
                    Ok::<_, std::io::Error>(addr)
                })
                .await
                .map_err(err_msg)?
                .map_err(err_msg)?;
                let u = format!("http://{addr}/");
                SHARED.lock().insert(dir.clone(), u.clone());
                u
            }
        };
        Ok(json!({ "dir": dir.to_string_lossy(), "url": url }))
    }

    /// Stops recording; the session file's path.
    pub fn stop() -> Option<String> {
        let r = RECORDING.lock().take()?;
        let _ = r.writer.flush();
        let file = r.path.clone();
        let path = file.to_string_lossy().into_owned();
        drop(r);
        // finished: a tenth of its size; still a .sonde file every reader opens
        let _ = sonde_proxy::session::compress(&file);
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn counts_requests_not_output_and_follows_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sonde");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{{\"format\":\"sonde-session\"}}").unwrap();
        writeln!(f, "{{\"id\":1}}").unwrap();
        writeln!(f, "{{\"log\":{{\"text\":\"hello\"}}}}").unwrap();
        writeln!(f, "{{\"id\":2}}").unwrap();
        write!(f, "{{\"id\":3").unwrap(); // still being written
        f.flush().unwrap();
        assert_eq!(count_records(&path), 2);
        writeln!(f, "}}").unwrap();
        writeln!(f, "{{\"log\":{{\"text\":\"bye\"}}}}").unwrap();
        f.flush().unwrap();
        assert_eq!(count_records(&path), 3);
        // a record written by another tool, its keys in another order
        writeln!(f, "{{\"time\":\"2026-01-01T00:00:00Z\",\"id\":4}}").unwrap();
        f.flush().unwrap();
        assert_eq!(count_records(&path), 4);
        // another session written in its place, as long or longer
        drop(f);
        let mut g = std::fs::File::create(&path).unwrap();
        writeln!(g, "{{\"format\":\"sonde-session\",\"other\":true}}").unwrap();
        for i in 0..6 {
            writeln!(g, "{{\"id\":{i}}}").unwrap();
        }
        g.flush().unwrap();
        assert_eq!(count_records(&path), 6);
    }

    #[test]
    fn a_live_session_read_in_steps_then_compressed_reads_whole() {
        use reqlog::Sink;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grows.sonde");
        let w =
            SessionWriter::create(&path, &SessionHeader::new("ep", "http://s3", "", None)).unwrap();
        let rec = |id| reqlog::Record {
            id,
            time: chrono::Utc::now(),
            operation: "GetObject".into(),
            ..Default::default()
        };
        w.flush().unwrap();
        let p = path.to_string_lossy().into_owned();
        // read while live, with its header only
        assert_eq!(open_session(&p).unwrap().records.len(), 0);
        let live = std::fs::metadata(&path).unwrap().len();
        w.write(&rec(1));
        drop(w);
        // finished: compressed in place, still longer than what was read
        sonde_proxy::session::compress(&path).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > live);
        assert_eq!(open_session(&p).unwrap().records.len(), 1);
    }

    #[test]
    fn an_analysis_is_kept_until_the_file_changes() {
        use reqlog::Sink;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sonde");
        let w =
            SessionWriter::create(&path, &SessionHeader::new("ep", "http://s3", "", None)).unwrap();
        let rec = |id| reqlog::Record {
            id,
            time: chrono::Utc::now(),
            method: "GET".into(),
            operation: "GetObject".into(),
            bucket: Some("b".into()),
            key: Some(format!("k{id}")),
            status: Some(200),
            ..Default::default()
        };
        w.write(&rec(1));
        let p = path.to_string_lossy().into_owned();
        let a = view_of(&p).unwrap();
        assert!(
            Arc::ptr_eq(&a, &view_of(&p).unwrap()),
            "unchanged: the same analysis"
        );
        assert_eq!(a["summary"]["requests"], 1);
        w.write(&rec(2));
        let b = view_of(&p).unwrap();
        assert_eq!(b["summary"]["requests"], 2, "grown: analysed again");
        assert!(Arc::ptr_eq(
            &open_session(&p).unwrap(),
            &open_session(&p).unwrap()
        ));
    }
}
