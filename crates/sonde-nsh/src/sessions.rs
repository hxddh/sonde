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
use sonde_proxy::episode::episodes;
use sonde_proxy::replay::{self, ReplayOptions};
use sonde_proxy::session::{Session, SessionHeader, SessionWriter};
use sonde_proxy::view::Analysis;
use sonde_proxy::{Policy, Proxy, ProxyConfig};

use crate::state::{err_msg, sonde, to_json};

struct Recording {
    proxy: Proxy,
    path: PathBuf,
    endpoint: String,
    log: RequestLog,
    writer: Arc<SessionWriter>,
    /// The runtime it began on: where it is finished, from a handler that is in none.
    runtime: tokio::runtime::Handle,
    /// What it wrote, kept for its program's next run when it has one.
    written: Arc<sonde_proxy::Written>,
}

/// How long a stopping recording waits for the requests in flight (the
/// CLI's).
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

impl Recording {
    /// Ends it: the requests in flight are answered and recorded (those
    /// still going after [`SHUTDOWN_GRACE`] are left out), then the file is
    /// compressed. The session file's path.
    async fn finish(self) -> String {
        self.finish_within(SHUTDOWN_GRACE).await
    }

    /// [`Recording::finish`], waiting `grace` for the requests in flight.
    async fn finish_within(mut self, grace: std::time::Duration) -> String {
        // compressing renames a new file over this one: a connection still
        // writing would write to a file nobody reads
        self.proxy.shutdown(grace).await;
        let _ = self.writer.flush();
        let _ = self.written.keep();
        let file = self.path.clone();
        drop(self);
        // finished: a tenth of its size; still a .sonde file every reader opens
        let _ = sonde_proxy::session::compress(&file);
        file.to_string_lossy().into_owned()
    }
}

static RECORDING: Mutex<Option<Recording>> = Mutex::new(None);
static RUNNING: Mutex<Option<crate::run::Running>> = Mutex::new(None);

/// What a press on a code place does with the file it names.
#[derive(Debug, PartialEq)]
enum CodeTarget {
    Open(PathBuf),
    Reveal(PathBuf),
}

/// What a press may open: source and text, which the system shows in an
/// editor. Anything else (programs, scripts, shortcuts, installers, a file
/// with no extension) is shown in the file manager, never opened.
const OPENS: &[&str] = &[
    "py", "js", "ts", "tsx", "jsx", "mjs", "cjs", "java", "kt", "go", "rs", "rb", "c", "h", "cpp",
    "cs", "scala", "sql", "toml", "yaml", "yml", "json", "md", "txt", "xml",
];

/// Of [`OPENS`], what Windows runs when opened (Windows Script Host, an
/// installed Python or Ruby): shown there, not opened.
const RUNS_ON_WINDOWS: &[&str] = &["js", "py", "rb"];

fn opens(ext: &str, windows: bool) -> bool {
    let e = ext.to_ascii_lowercase();
    OPENS.contains(&e.as_str()) && !(windows && RUNS_ON_WINDOWS.contains(&e.as_str()))
}

/// A path on another machine (`\\host\share`, `//host/share`,
/// `\\?\UNC\…`): even looking at it reaches out to that host.
fn remote(path: &str) -> bool {
    let p = path.replace('/', "\\");
    match p.strip_prefix("\\\\") {
        // `\\?\C:\…` and `\\.\…` are this machine's, unless they spell UNC
        Some(rest) => match rest
            .strip_prefix("?\\")
            .or_else(|| rest.strip_prefix(".\\"))
        {
            Some(local) => local.to_ascii_uppercase().starts_with("UNC\\"),
            None => true,
        },
        None => false,
    }
}

/// The file `place` names, inside `dir`: opened when it is a regular file
/// there whose type is source or text ([`OPENS`]); shown in the file
/// manager otherwise. `dir` and `place` both come from the session file,
/// which may be someone else's: a folder on another machine is refused,
/// and a folder that is a whole disk is no folder to be "inside" of.
fn code_target(dir: &Path, place: &str) -> Result<CodeTarget, String> {
    // `file:line function`: a path may hold spaces and a drive's colon
    let named = sonde_proxy::callers::code_place(place);
    let file = match &named {
        Some((f, _)) => f.as_str(),
        None => place.trim(),
    };
    if file.is_empty() {
        return Err(err_msg("the place names no file"));
    }
    // before anything touches the path: canonicalize would contact the host
    if remote(&dir.to_string_lossy()) || remote(file) {
        return Err(err_msg(
            "the session names a folder on another machine: not opened",
        ));
    }
    let root = dir
        .canonicalize()
        .map_err(|e| err_msg(format!("{}: {e}", dir.display())))?;
    // a canonical root on Windows is verbatim (`\\?\C:\…`), where `/` is no
    // separator: a place written `etl/load.py` is joined as `etl\load.py`
    let file = if cfg!(windows) {
        std::borrow::Cow::Owned(file.replace('/', "\\"))
    } else {
        std::borrow::Cow::Borrowed(file)
    };
    let candidate = Path::new(file.as_ref());
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let full = joined
        .canonicalize()
        .map_err(|_| err_msg(format!("{} is not there", joined.display())))?;
    let source = full
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| opens(e, cfg!(windows)));
    let inside = root.parent().is_some() && full.starts_with(&root);
    let regular = full.metadata().map(|m| m.is_file()).unwrap_or(false);
    Ok(if inside && regular && source {
        CodeTarget::Open(full)
    } else {
        CodeTarget::Reveal(full)
    })
}

/// A recording policy by its name, none the default (no-delete); an
/// unknown one is refused, never "allow".
fn policy_of(name: &str) -> Result<Policy, String> {
    if name.is_empty() {
        return Ok(Policy::default());
    }
    Policy::named(name).ok_or_else(|| {
        err_msg(format!(
            "unknown recording policy \"{name}\": allow, no-delete or read-only"
        ))
    })
}

/// The policy a run in `dir` is recorded under: the one asked (the app's
/// setting, or Run's choice), narrowed by its project's `[record] policy`.
/// A project's file may narrow what the person chose, never widen it.
fn run_policy(asked: Policy, dir: Option<&Path>) -> Result<Policy, String> {
    let project = match dir {
        Some(d) => sonde_proxy::project::of(d).map_err(err_msg)?,
        None => None,
    };
    Ok(match project.and_then(|p| p.policy) {
        Some(p) => asked.stricter(p),
        None => asked,
    })
}

/// A JSON map beside the sessions (`titles.json`, `baselines.json`,
/// `imports.json`), to change and write back: empty when there is none, an
/// error when it is there and does not read. Writing an empty map over a
/// file that is only damaged would drop every entry in it.
fn map_to_change(path: &Path) -> Result<std::collections::BTreeMap<String, String>, String> {
    sonde_proxy::program::read_map(path).map_err(|e| {
        err_msg(format!(
            "{} cannot be read ({}): nothing was changed; fix or remove the file",
            e.path.display(),
            e.why
        ))
    })
}

/// A file's text: a finished session (or a log) is gzip, read for what it
/// holds rather than its compressed bytes; bytes as a session is read (a
/// torn last character is a torn line).
fn file_text(bytes: &[u8]) -> std::io::Result<String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let out = sonde_proxy::import::gunzip(bytes, sonde_proxy::import::GUNZIP_LIMIT)?;
        return Ok(String::from_utf8_lossy(&out).into_owned());
    }
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// FNV-1a, 64 bits: which file a session was made from.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Writes the files opened into sessions, by their contents' digest: `imports.json`.
fn write_imports(dir: &Path, all: &std::collections::BTreeMap<String, String>) {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = dir.join(format!(
        "imports.json.{}.{}.tmp",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    if serde_json::to_vec(all)
        .ok()
        .is_some_and(|b| reqlog::private::write(&tmp, b).is_ok())
    {
        let _ = std::fs::rename(&tmp, dir.join("imports.json"));
    }
}

/// The platform has no trash (as opposed to moving there failing).
fn no_trash(e: &str) -> bool {
    let e = e.to_ascii_lowercase();
    e.contains("not supported") || e.contains("no trash") || e.contains("or trash")
}

/// A trial's run: the scenario its proxy answers with, the clean run's file
/// name, and the file it is recorded to.
struct Under {
    scenario: sonde_proxy::fault::Scenario,
    of: String,
    path: PathBuf,
    /// What the clean run wrote: no-delete lets the trial write it again.
    written: Arc<sonde_proxy::Written>,
}

/// Starts a recording: a local endpoint forwarding to `endpoint`; `under`
/// a scenario, one that fails requests as it says.
async fn begin(
    endpoint: String,
    policy: String,
    command: Option<String>,
    cwd: Option<std::path::PathBuf>,
    under: Option<Under>,
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
    // none asked: the setting's
    let asked = match policy.as_str() {
        "" => policy_of(&s.settings().record_policy)?,
        p => policy_of(p)?,
    };
    let policy = run_policy(asked, cwd.as_deref())?;
    let dir = s.paths().sessions();
    std::fs::create_dir_all(&dir).map_err(err_msg)?;
    let name = command
        .as_deref()
        .and_then(sonde_proxy::program::program_name)
        .unwrap_or_else(|| "recording".into());
    let path = match &under {
        Some(u) => u.path.clone(),
        None => sonde_proxy::session::fresh_path(
            &dir,
            &format!("{}-{name}", chrono::Local::now().format("%Y%m%d-%H%M%S")),
        ),
    };
    let cfg = client.config();
    let mut header = SessionHeader::new(&endpoint, cfg.endpoint.as_str(), &cfg.region, command);
    if let Some(dir) = &cwd {
        header = header.ran_in(dir);
    }
    if let Some(u) = &under {
        header.scenario = Some(u.scenario.name.clone());
        header.trial_of = Some(u.of.clone());
    }
    let faults = under
        .as_ref()
        .map(|u| Arc::new(sonde_proxy::fault::Faults::new(&u.scenario)));
    let written = under
        .as_ref()
        .map(|u| u.written.clone())
        .unwrap_or_default();
    // under no-delete a program may write over what its earlier runs wrote
    if under.is_none()
        && let Some(program) = sonde_proxy::program::key_of(&header)
    {
        written.remember(&dir, &program, cfg.endpoint.as_str());
    }
    let writer = Arc::new(SessionWriter::create(&path, &header).map_err(err_msg)?);
    let log = RequestLog::new(4096);
    log.set_sink(Some(writer.clone()));
    let started = background({
        let log = log.clone();
        let written = written.clone();
        async move {
            Proxy::start(
                client,
                log,
                ProxyConfig {
                    policy,
                    faults,
                    written,
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
        runtime: tokio::runtime::Handle::current(),
        written,
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
    /// A large session is kept lean ([`Session::open_lean`]): a request in
    /// full is then read from the file when it is asked for.
    lean: bool,
    analysis: Option<Arc<Analysis>>,
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

/// A session past this size on disk is kept lean: about two hundred
/// thousand requests (compressed, a file is a tenth of its text).
const LEAN_FROM: u64 = 24 << 20;

fn kept_lean(len: u64, compressed: bool) -> bool {
    len > if compressed {
        LEAN_FROM
    } else {
        LEAN_FROM * 10
    }
}

fn open_session(path: &str) -> Result<Arc<Session>, String> {
    cached_session(path).map(|(s, _)| s)
}

/// A session with every record in full: the one kept, or (a large session
/// is kept lean) read again for the occasion.
fn full_session(path: &str) -> Result<Arc<Session>, String> {
    match cached_session(path)? {
        (s, false) => Ok(s),
        _ => Ok(Arc::new(Session::open(Path::new(path)).map_err(err_msg)?)),
    }
}

/// One record of a session in full: from memory, or (a session kept lean)
/// from its file.
fn full_record(path: &str, id: u64) -> Result<Option<reqlog::Record>, String> {
    let (s, lean) = cached_session(path)?;
    if !lean {
        return Ok(s.records.iter().find(|r| r.id == id).cloned());
    }
    record_from_file(Path::new(path), &s, id)
}

/// One record in full from a session's file, for a session read lean (`s`:
/// it has the ids the reading gave the file's records).
fn record_from_file(path: &Path, s: &Session, id: u64) -> Result<Option<reqlog::Record>, String> {
    if !s.records.iter().any(|r| r.id == id) {
        return Ok(None);
    }
    let mut found = None;
    // read to where it is, and no further
    let read =
        sonde_proxy::share::each_record(path, &mut s.records.iter().map(|r| r.id), &mut |r| {
            if r.id == id {
                found = Some(r);
                return Err(std::io::Error::other("found"));
            }
            Ok(())
        });
    match (found, read) {
        (Some(r), _) => Ok(Some(r)),
        (None, Err(e)) => Err(err_msg(e)),
        (None, Ok(())) => Ok(None),
    }
}

fn cached_session(path: &str) -> Result<(Arc<Session>, bool), String> {
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
                return Ok((c.session.clone(), c.lean));
            }
            // a live recording that grew: read only what it appended (taken
            // out of the cache so that, unshared, it grows without a copy)
            Some(c) if !compressed && now.0 > c.stamp.0 && c.read_to.is_some() => cache
                .remove(p)
                .map(|c| (c.session, c.read_to.unwrap_or(0), c.lean)),
            _ => None,
        }
    };
    // the same recording, grown; not another written in its place
    let grown = grown.filter(|(old, ..)| header_of(p).as_ref() == Some(&old.header));
    let (session, read_to, lean) = match grown {
        Some((old, from, was_lean)) => {
            let mut s = Arc::unwrap_or_clone(old);
            let had = s.records.len();
            let to = s.read_more(p, from).map_err(err_msg)?;
            // once lean, lean: what it appends is kept as the rest is
            let lean = was_lean || kept_lean(now.0, false);
            if lean {
                let from = if was_lean { had } else { 0 };
                s.records[from..]
                    .iter_mut()
                    .for_each(sonde_proxy::session::lean);
            }
            (Arc::new(s), Some(to), lean)
        }
        None => {
            // where complete lines end, taken before the read: the next read starts there
            let to = (!compressed)
                .then(|| sonde_proxy::session::complete_len(p).ok())
                .flatten();
            let lean = kept_lean(now.0, compressed);
            // read outside the lock: another session opens meanwhile
            let s = if lean {
                Session::open_lean(p)
            } else {
                Session::open(p)
            };
            (Arc::new(s.map_err(err_msg)?), to, lean)
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
            lean,
            analysis: None,
        },
    );
    Ok((session, lean))
}

/// A session and what it amounts to (its operations, findings, signals),
/// worked out once per version of the file: every view of it, and every
/// page of its operations, is a look at this.
fn analysis_of(path: &str) -> Result<(Arc<Session>, Arc<Analysis>), String> {
    let s = open_session(path)?;
    let p = Path::new(path);
    if let Some(a) = CACHE
        .lock()
        .get(p)
        .filter(|c| Arc::ptr_eq(&c.session, &s))
        .and_then(|c| c.analysis.clone())
    {
        return Ok((s, a));
    }
    let a = Arc::new(Analysis::of(
        &s,
        &crate::state::request_prices_for(&s.header.endpoint),
    ));
    if let Some(c) = CACHE.lock().get_mut(p)
        && Arc::ptr_eq(&c.session, &s)
    {
        c.analysis = Some(a.clone());
    }
    Ok((s, a))
}

/// What [`Sessions::view`] returns: the totals, the findings and the first
/// page of operations, built from the analysis kept.
fn view_of(path: &str) -> Result<JsonValue, String> {
    let (s, a) = analysis_of(path)?;
    let mut view = a.view(path, &s.header);
    // the SDK that sent most of its requests ("boto3 1.34"), for the header
    view.insert(
        "sdk".into(),
        json!(sonde_proxy::callers::sdk_of(&s.records)),
    );
    Ok(with_trials(JsonValue::Object(view), path))
}

/// [`Sessions::stretch`] over `records`: those that started from `from_ms`
/// up to `to_ms` after the first one's start.
fn stretch_of(
    records: &[reqlog::Record],
    prices: &sonde_proxy::profile::RequestPrices,
    from_ms: f64,
    to_ms: f64,
) -> JsonValue {
    let t0 = records
        .iter()
        .map(|r| r.time.timestamp_micros())
        .min()
        .unwrap_or(0);
    let (from_us, to_us) = ((from_ms * 1000.0) as i64, (to_ms * 1000.0) as i64);
    let held: Vec<reqlog::Record> = records
        .iter()
        .filter(|r| {
            let at = r.time.timestamp_micros() - t0;
            at >= from_us && at < to_us
        })
        .cloned()
        .collect();
    let (summary, signals, profile) = std::thread::scope(|scope| {
        let summary = scope.spawn(|| sonde_proxy::summary::summarize(&held));
        let signals = scope.spawn(|| sonde_proxy::signals::signals(&held, prices));
        let profile = sonde_proxy::profile::profile(&held, prices);
        (
            summary
                .join()
                .unwrap_or_else(|p| std::panic::resume_unwind(p)),
            signals
                .join()
                .unwrap_or_else(|p| std::panic::resume_unwind(p)),
            profile,
        )
    });
    // where the stretch's own first request started, from the session's: its times are from there
    let offset_ms = held
        .iter()
        .map(|r| r.time.timestamp_micros())
        .min()
        .map_or(0.0, |at| (at - t0) as f64 / 1000.0);
    json!({ "summary": to_json(summary), "profile": to_json(profile), "signals": to_json(signals), "offsetMs": offset_ms })
}

/// A page's query as the interface sends it.
fn query_of(query: JsonValue) -> Result<sonde_proxy::view::Query, String> {
    if query.is_null() {
        return Ok(Default::default());
    }
    serde_json::from_value(query).map_err(err_msg)
}

/// path → (bytes counted up to the last complete line, records in them)
static COUNTS: std::sync::LazyLock<
    Mutex<std::collections::HashMap<PathBuf, (u64, usize, Option<String>)>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Forgets what was read of a session file (it was removed or replaced).
fn forget(path: &Path) {
    CACHE.lock().remove(path);
    COUNTS.lock().remove(path);
    POINTS.lock().remove(path);
    sonde_proxy::summary::forget(path);
}

/// The titles given to sessions, by file name: `titles.json` beside them.
fn titles() -> std::collections::BTreeMap<String, String> {
    std::fs::read(sonde().paths().sessions().join("titles.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_titles(t: &std::collections::BTreeMap<String, String>) -> Result<(), String> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = sonde().paths().sessions();
    // a name of its own: the app and the CLI may write at the same time
    let tmp = dir.join(format!(
        "titles.json.{}.{}.tmp",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    reqlog::private::write(&tmp, serde_json::to_vec_pretty(t).map_err(err_msg)?)
        .map_err(err_msg)?;
    std::fs::rename(&tmp, dir.join("titles.json")).map_err(err_msg)
}

/// A session of the sessions directory, by path: what may be renamed or
/// removed (not a file elsewhere on the disk).
fn own_session(path: &str) -> Result<(PathBuf, String), String> {
    let dir = sonde().paths().sessions();
    own_session_in(&dir, path).map(|name| (dir.join(&name), name))
}

/// [`own_session`] for the sessions directory `dir`: the file's name.
fn own_session_in(dir: &Path, path: &str) -> Result<String, String> {
    let p = Path::new(path);
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let here = p.parent().and_then(|d| d.canonicalize().ok());
    // a trial's session opens like any other, and is no run of its own:
    // it has no title, no baseline, and is removed with its run
    if here.is_some() && here == dir.join("trials").canonicalize().ok() {
        return Err(err_msg(
            "a trial's session goes with its run: rename, remove or set as baseline the run it is a trial of",
        ));
    }
    let inside = here.is_some() && here == dir.canonicalize().ok();
    if !inside || !name.ends_with(".sonde") || !p.is_file() {
        return Err(format!("{path} is not one of sonde's sessions"));
    }
    // as the list names it (a dropped file's path may be spelled otherwise:
    // forward slashes on Windows, a symlinked folder)
    Ok(name)
}

/// Keeps the exit code a run ended with, in its folder's index: a trial
/// judges the program's exit under faults against it.
fn write_exit(session: &Path, code: Option<i32>) {
    if let Some(c) = code {
        sonde_proxy::index::set(session, sonde_proxy::index::Change::Exit(c));
    }
}

/// The exit code the run ended with; `None` when it was not kept (a run of
/// the CLI's).
fn read_exit(session: &Path) -> Option<i32> {
    sonde_proxy::index::get(session).exit
}

/// A run's verdicts for the app: each with the path of its trial's session
/// (the file keeps a name, which only this run's folder resolves).
fn trials_of(session: &Path) -> Vec<sonde_proxy::trial::Trial> {
    let program = header_of(session).and_then(|h| sonde_proxy::program::key_of(&h));
    let mut trials = sonde_proxy::trial::load_with_history(session, program.as_deref());
    for t in &mut trials {
        t.session = sonde_proxy::trial::session_path(session, t)
            .filter(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned());
    }
    trials
}

/// A run's trials as a list row marks them: the scenario, how the program
/// stood, and the trial's own session where it is still there.
fn trial_marks(session: &Path) -> Vec<JsonValue> {
    sonde_proxy::trial::load(session)
        .iter()
        .map(|t| {
            json!({
                "scenario": t.scenario,
                "standing": t.standing,
                "session": sonde_proxy::trial::session_path(session, t)
                    .filter(|p| p.is_file())
                    .map(|p| p.to_string_lossy().into_owned()),
            })
        })
        .collect()
}

/// A session's view (taken, not copied) with what is known of its trials,
/// read fresh (the verdicts are written after the session's file, and beside it): a trial
/// says `trial: { scenario, of, clean }`, a clean run with verdicts
/// `trials: [...]`.
fn with_trials(mut v: JsonValue, path: &str) -> JsonValue {
    let p = Path::new(path);
    let scenario = v["header"]["scenario"].as_str().map(String::from);
    let Some(o) = v.as_object_mut() else {
        return v;
    };
    match scenario {
        Some(scenario) => {
            let of = o["header"]["trialOf"]
                .as_str()
                .or_else(|| o["header"]["trial_of"].as_str())
                .map(String::from);
            // the clean run is beside the folder the trial is in
            let clean = of
                .as_deref()
                .filter(|n| !n.contains(['/', '\\']))
                .and_then(|n| Some(p.parent()?.parent()?.join(n)))
                .filter(|c| c.is_file())
                .map(|c| c.to_string_lossy().into_owned());
            o.insert(
                "trial".into(),
                json!({ "scenario": scenario, "of": of, "clean": clean }),
            );
        }
        None => {
            let trials = trials_of(p);
            if !trials.is_empty() {
                o.insert("trials".into(), to_json(trials));
            }
        }
    }
    v
}

/// The scenarios a project names: the `[[scenario]]` tables of the
/// `sonde.toml` found from `dir` upwards (to the repository's root), read
/// as `sonde run --trial` reads them ([`sonde_proxy::project`]). None when
/// there is no such file.
fn project_scenarios(dir: &Path) -> Result<Vec<sonde_proxy::fault::Scenario>, String> {
    Ok(sonde_proxy::project::of(dir)?
        .map(|p| p.scenarios)
        .unwrap_or_default())
}

/// The budget a project gives its program in the `[check]` of the
/// `sonde.toml` found from `dir` upwards, read as `sonde check` reads it
/// ([`sonde_proxy::project`]): what the app draws on a chart as a line —
/// `max-p95` and `max-waiting` in ms, `max-errors` a count. None without a
/// file (or one that does not read), or without any of the three.
pub(crate) fn project_budget(dir: &Path) -> Option<(Option<f64>, Option<u64>, Option<f64>)> {
    let check = sonde_proxy::project::of(dir).ok()??.file.check;
    let p95 = check.max_p95_ms().ok()?;
    let waiting = check.max_waiting_ms().ok()?;
    let errors = check.max_errors.map(|n| n as u64);
    (p95.is_some() || waiting.is_some() || errors.is_some()).then_some((p95, errors, waiting))
}

/// What a trial of a session would run: the project's scenarios when its
/// `sonde.toml` names some, else the ones chosen from what the clean run
/// did; and whether every run would write to the same place.
#[derive(Debug)]
struct TrialPlan {
    command: String,
    cwd: Option<String>,
    scenarios: Vec<sonde_proxy::fault::Scenario>,
    /// The scenarios are the project's own.
    project: bool,
    /// The command names no place of its own for each run (`{trial}` or
    /// `SONDE_TRIAL`): a trial writes where the clean run wrote.
    shares_place: bool,
}

fn trial_plan_of(s: &Session) -> Result<TrialPlan, String> {
    if s.header.scenario.is_some() {
        return Err(err_msg(
            "this session is a trial itself: try the run it is a trial of",
        ));
    }
    let command = s
        .header
        .command
        .clone()
        .filter(|c| sonde_proxy::program::program_key(Some(c)).is_some())
        .ok_or_else(|| err_msg("only a run of a command can be tried under faults"))?;
    if s.records.is_empty() {
        return Err(err_msg("no trial: the run sent no requests to fail"));
    }
    let cwd = s.header.cwd.clone();
    let project = match &cwd {
        Some(d) => project_scenarios(Path::new(d)).map_err(err_msg)?,
        None => Vec::new(),
    };
    let own = !project.is_empty();
    Ok(TrialPlan {
        shares_place: !command.contains("{trial}") && !command.contains("SONDE_TRIAL"),
        command,
        cwd,
        scenarios: if own {
            project
        } else {
            sonde_proxy::trial::choose(&s.records)
        },
        project: own,
    })
}

/// The scenarios asked for by name, from the plan's and the built-in ones
/// (`slowdown:3`: pressed harder, see [`sonde_proxy::fault::scenario_asked`]);
/// all of the plan's when none is named.
fn scenarios_named(
    plan: &TrialPlan,
    names: &[String],
) -> Result<Vec<sonde_proxy::fault::Scenario>, String> {
    if names.is_empty() {
        return Ok(plan.scenarios.clone());
    }
    names
        .iter()
        .map(|n| {
            if let Some(s) = plan.scenarios.iter().find(|s| &s.name == n) {
                return Ok(s.clone());
            }
            // a project's own scenario of the name says its times itself
            let name = n.split_once(':').map_or(n.as_str(), |(name, _)| name);
            if plan.project && plan.scenarios.iter().any(|s| s.name == name) {
                return Err(err_msg(format!(
                    "{n}: {name} is the project's own scenario (sonde.toml says its times)"
                )));
            }
            sonde_proxy::fault::scenario_asked(n)
                .map_err(err_msg)?
                .ok_or_else(|| err_msg(format!("{n}: no such scenario")))
        })
        .collect()
}

/// A trial's progress as the app hears it: `{ scenario, state }`, with the
/// verdict when the scenario is done.
fn trial_event(scenario: &str, trial: Option<&sonde_proxy::trial::Trial>) -> JsonValue {
    match trial {
        None => json!({ "scenario": scenario, "state": "running" }),
        Some(t) => json!({ "scenario": scenario, "state": "done", "trial": t }),
    }
}

/// Each program's command, as the newest of its runs wrote it.
#[derive(Default)]
struct Newest(std::collections::BTreeMap<String, (String, Option<String>)>);

impl Newest {
    fn insert(&mut self, key: String, run: (String, Option<String>)) {
        match self.0.get(&key) {
            Some(kept) if kept.0 >= run.0 => {}
            _ => {
                self.0.insert(key, run);
            }
        }
    }
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).and_then(|r| r.1.clone())
    }
}

/// A terminal's question where the cursor is (a Device Status Report),
/// which its reader is to answer.
const ASKS_CURSOR: &str = "\x1b[6n";

/// Finds the cursor's question in a terminal's output as it comes, a piece
/// at a time: a question split between two reads is found in the second,
/// from the start of it the first left over.
#[derive(Default)]
struct CursorAsks {
    /// The end of the last piece that could begin the question.
    carried: String,
}

impl CursorAsks {
    /// How many times the output asks, this piece and what was carried.
    fn asked(&mut self, text: &str) -> usize {
        let joined = std::mem::take(&mut self.carried) + text;
        let n = joined.matches(ASKS_CURSOR).count();
        // the longest end of it that begins the question (all of it was
        // counted, and is not carried)
        let keep = (1..ASKS_CURSOR.len())
            .rev()
            .find(|&k| {
                k <= joined.len()
                    && joined.is_char_boundary(joined.len() - k)
                    && ASKS_CURSOR.starts_with(&joined[joined.len() - k..])
            })
            .unwrap_or(0);
        self.carried = joined[joined.len() - keep..].to_string();
        n
    }
}

/// The command a run's shell is given: as the session keeps it, every
/// separator, pipe and quote where it was written, with `{trial}` alone
/// replaced (`clean`, or the scenario's name under one).
fn command_under(command: &str, trial: Option<&str>) -> String {
    command.replace("{trial}", trial.unwrap_or("clean"))
}

/// How long the app's quit waits for the requests in flight: the system
/// gives a quitting app 200 ms in all (Native Shell's `quit` event).
const QUIT_GRACE: std::time::Duration = std::time::Duration::from_millis(60);

/// The app is quitting: the program being recorded is ended (with what it
/// started), a running trial is stopped, and the recording is written as a
/// finished session. A trial keeps nothing, as when it is stopped by hand:
/// its half-recorded session and those of the scenarios before it go.
/// What it ended: `{ program, trial, session }`.
async fn end_for_quit() -> JsonValue {
    use std::sync::atomic::Ordering::SeqCst;
    let trial = TRYING.load(SeqCst);
    if trial {
        TRIAL_STOPPED.store(true, SeqCst);
    }
    let program = RUNNING.lock().take().is_some_and(|mut r| r.kill());
    let recording = RECORDING.lock().take();
    let mut session = None;
    if let Some(r) = recording {
        let path = r.path.clone();
        // (on the runtime's own threads, as every recording is finished)
        let _ = background(r.finish_within(QUIT_GRACE)).await;
        if trial {
            // (the run it is a trial of is beside the folder it is in)
            let _ = std::fs::remove_file(&path);
            let of = TRIAL_OF.lock().take();
            if let Some(clean) = of {
                sonde_proxy::trial::remove(&clean);
            }
        } else {
            session = Some(path.to_string_lossy().into_owned());
        }
    }
    json!({ "program": program, "trial": trial, "session": session })
}

/// A trial is running, and was asked to stop ([`Sessions::kill`]).
static TRYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The run the trial under way is of (its session's file).
static TRIAL_OF: Mutex<Option<PathBuf>> = Mutex::new(None);
static TRIAL_STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One run of `command` in a pseudo-terminal, recorded: the exit code and
/// the session's file. `{trial}` in the command (and `SONDE_TRIAL`) is
/// `clean`, or the scenario's name under one, so each run can have a place
/// of its own; the session keeps the command as it was written.
#[allow(clippy::too_many_arguments)]
async fn run_recorded(
    command: &str,
    cwd: Option<&str>,
    endpoint: String,
    policy: String,
    cols: f64,
    rows: f64,
    output: &Callback<JsonValue>,
    under: Option<Under>,
    // a terminal shows the output and answers what the program asks of it
    // (Run); none does for a trial's runs or a clean run made for one
    terminal: bool,
) -> Result<(Option<i32>, String), String> {
    if RUNNING.lock().is_some() {
        return Err(err_msg("a program is running: stop it first"));
    }
    let trial = under.as_ref().map(|u| u.scenario.name.clone());
    let dir = crate::run::folder(cwd);
    let view = begin(endpoint, policy, Some(command.to_string()), dir, under).await?;
    let (env_set, env_unset, writer, path) = {
        let g = RECORDING.lock();
        let r = g.as_ref().expect("begin started a recording");
        let (mut set, unset) = r.proxy.env();
        // the code that sent each request, when asked (a Python, Node.js
        // or Java program's)
        if sonde().settings().name_callers
            && let Ok(v) = sonde_proxy::callers::caller_env(&sonde().paths().python_hook())
        {
            set.extend(v);
        }
        set.push((
            "SONDE_TRIAL".into(),
            trial.clone().unwrap_or_else(|| "clean".into()),
        ));
        // a Python program that starts Spark reads s3a's endpoint from here
        let region = set
            .iter()
            .find(|(k, _)| k == "AWS_REGION")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        set.push((
            "PYSPARK_SUBMIT_ARGS".into(),
            sonde_proxy::jvm::pyspark_submit_args(
                std::env::var("PYSPARK_SUBMIT_ARGS").ok().as_deref(),
                &r.proxy.plain_url(),
                &region,
            ),
        ));
        (
            set,
            unset,
            r.writer.clone(),
            r.path.to_string_lossy().into_owned(),
        )
    };
    let started = crate::run::spawn(
        &command_under(command, trial.as_deref()),
        cwd,
        &env_set,
        &env_unset,
        cols as u16,
        rows as u16,
    );
    let (mut out_rx, exit_rx, handle) = match started {
        Ok(x) => x,
        Err(e) => {
            let r = RECORDING.lock().take();
            if let Some(r) = r {
                let _ = background(r.finish()).await;
            }
            return Err(err_msg(format!("cannot start {command}: {e}")));
        }
    };
    *RUNNING.lock() = Some(handle);
    match &trial {
        None => output.post(json!({ "started": view })),
        Some(t) => output.post(json!({ "started": view, "scenario": t })),
    };
    let mut decoder = crate::run::Utf8Stream::default();
    let mut asks = CursorAsks::default();
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
                    // Windows' pseudo-console asks where the cursor is as it
                    // starts and holds the program until it is told. A run's
                    // terminal answers; a run with none (a trial's, a clean
                    // run made to try again) is answered here, or the program
                    // never starts. (each answered; it may come in two reads)
                    let asked = asks.asked(&text);
                    if !terminal
                        && asked > 0
                        && let Some(r) = RUNNING.lock().as_mut()
                    {
                        for _ in 0..asked {
                            r.write(b"\x1b[1;1R");
                        }
                    }
                    if !text.is_empty() {
                        writer.write_log(&text);
                        match &trial {
                            None => output.post(json!({ "data": text })),
                            Some(t) => output.post(json!({ "data": text, "scenario": t })),
                        };
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
    // its own recording only: the user may have stopped it and begun another
    let own = {
        let mut g = RECORDING.lock();
        g.take_if(|r| r.path == Path::new(&path))
    };
    // resolves once the file is whole: what reads it next reads all of it
    if let Some(r) = own {
        let _ = background(r.finish()).await;
    }
    Ok((code, path))
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
    // kept on disk too, in the folder's index: a
    // program's history reads 30 runs, and a finished run does not change
    let h = header_of(path)?;
    let (_, figures) =
        sonde_proxy::summary::figures(path, &crate::state::request_prices_for(&h.endpoint))?;
    POINTS
        .lock()
        .insert(path.to_path_buf(), (now, figures.point.clone()));
    // (what 15.0 kept them in, all in one file)
    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_file(dir.join(".runpoints.json"));
    }
    Some(figures.point)
}

/// Each program's baseline run, by file name: `baselines.json` beside the sessions.
/// A damaged one is an error, not "none set": said, and never written over.
fn baselines() -> Result<std::collections::BTreeMap<String, String>, String> {
    sonde_proxy::program::read_baselines(&sonde().paths().sessions()).map_err(err_msg)
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
    // a finished session is compressed: counted whole, once, and the count
    // kept beside it
    if len > from && sonde_proxy::session::is_compressed(path) {
        let n = sonde_proxy::summary::counted(path).map_or(0, |c| c.requests as usize);
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
/// list's sparkline, from the counts kept beside it. A live session has
/// none (it is still growing).
#[cfg(test)]
const SHAPE_BINS: usize = sonde_proxy::summary::SHAPE_BINS;

fn shape_of(path: &Path) -> Vec<u32> {
    if !sonde_proxy::session::is_compressed(path) {
        return Vec::new();
    }
    sonde_proxy::summary::counted(path)
        .map(|c| c.shape)
        .unwrap_or_default()
}

fn header_line(path: &Path) -> Option<String> {
    sonde_proxy::session::header_line(path)
}

/// [`Sessions::code_change`]'s work: two runs' commits, the files between
/// them, the later run's findings on their lines.
fn code_change_of(before: &str, after: &str) -> Result<JsonValue, String> {
    let a = header_of(Path::new(&before)).ok_or_else(|| err_msg("not a session"))?;
    let b = header_of(Path::new(&after)).ok_or_else(|| err_msg("not a session"))?;
    let (Some(ga), Some(gb)) = (a.git, b.git) else {
        return Err(err_msg(
            "a run without its commit: run it again in a git repository",
        ));
    };
    let hex = |c: &str| (7..=64).contains(&c.len()) && c.chars().all(|x| x.is_ascii_hexdigit());
    if !hex(&ga.commit) || !hex(&gb.commit) {
        return Err(err_msg("not a commit"));
    }
    let cwd = b
        .cwd
        .ok_or_else(|| err_msg("the run's folder is not known"))?;
    let dir = PathBuf::from(&cwd);
    if !dir.is_dir() {
        return Err(err_msg(format!("{cwd} is not there")));
    }
    let git = |args: &[&str]| -> Result<String, String> {
        let o = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            // nothing of the repository's own config runs (a session
            // names the folder; it may be a file someone shared)
            .args(["-c", "core.quotepath=off", "-c", "core.fsmonitor=false"])
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| err_msg(format!("git: {e}")))?;
        if !o.status.success() {
            return Err(err_msg(
                String::from_utf8_lossy(&o.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let root = git(&["rev-parse", "--show-toplevel"])?.trim().to_string();
    let names = git(&[
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--name-only",
        &ga.commit,
        &gb.commit,
        "--",
    ])?;
    // the later run's findings, by the file and line their requests came from
    let s = open_session(after)?;
    let obs = sonde_proxy::episode::observations_of(&s.header, &s.records, &episodes(&s.records));
    let rel = |file: &str| -> String {
        let full = dir.join(file);
        let full = full.canonicalize().unwrap_or(full);
        let r = PathBuf::from(&root);
        let r = r.canonicalize().unwrap_or(r);
        full.strip_prefix(&r)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| file.replace('\\', "/"))
    };
    let mut notes: std::collections::BTreeMap<String, Vec<(u32, String)>> = Default::default();
    for o in &obs {
        for c in &o.callers {
            if let Some((file, line)) = sonde_proxy::callers::code_place(&c.place) {
                notes
                    .entry(rel(&file))
                    .or_default()
                    .push((line, o.text.clone()));
            }
        }
    }
    let mut changed: Vec<&str> = names.lines().filter(|l| !l.is_empty()).collect();
    changed.sort_by_key(|f| !notes.contains_key(*f));
    let show = |rev: &str, f: &str| -> String {
        git(&["show", "--no-textconv", &format!("{rev}:{f}")])
            .map(|t| t.chars().take(200_000).collect())
            .unwrap_or_default()
    };
    let files: Vec<JsonValue> = changed
        .iter()
        .take(6)
        .map(|f| {
            json!({
                "path": f,
                "old": show(&ga.commit, f),
                "new": show(&gb.commit, f),
                "notes": notes.get(*f).map(|n| n.iter().map(|(l, t)| json!({ "line": l, "text": t })).collect::<Vec<_>>()).unwrap_or_default(),
            })
        })
        .collect();
    Ok(json!({
        "from": ga.commit,
        "to": gb.commit,
        "files": files,
        "more": changed.len().saturating_sub(6),
    }))
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

/// The requests of `bucket`/`key` in a session, in time order, each with
/// the operation it belongs to.
fn key_history_of(s: &Session, a: &Analysis, bucket: &str, key: &str) -> Vec<JsonValue> {
    let mut rows: Vec<&reqlog::Record> = s
        .records
        .iter()
        .filter(|r| r.key.as_deref() == Some(key) && r.bucket.as_deref().unwrap_or("") == bucket)
        .collect();
    rows.sort_by_key(|r| (r.time, r.id));
    rows.into_iter()
        .map(|r| {
            let mut v = to_json(r.summary());
            v["operationIndex"] = json!(a.episode_of(r.id));
            v
        })
        .collect()
}

/// Runs a trial of `clean` (the run in `file`, named `of`) under each of
/// `chosen`, as [`Sessions::trial`] says; its verdicts replace the run's
/// last, and are added to its program's.
#[allow(clippy::too_many_arguments)]
async fn try_under(
    file: &Path,
    of: &str,
    clean: &Session,
    clean_exit: i32,
    plan: &TrialPlan,
    chosen: Vec<sonde_proxy::fault::Scenario>,
    policy: &str,
    cols: f64,
    rows: f64,
    output: &Callback<JsonValue>,
) -> Result<JsonValue, String> {
    use std::sync::atomic::Ordering::SeqCst;
    if RUNNING.lock().is_some() || RECORDING.lock().is_some() {
        return Err(err_msg("a program is running: stop it first"));
    }
    if TRYING.swap(true, SeqCst) {
        return Err(err_msg("a trial is running: stop it first"));
    }
    struct Trying;
    impl Drop for Trying {
        fn drop(&mut self) {
            TRYING.store(false, std::sync::atomic::Ordering::SeqCst);
            TRIAL_OF.lock().take();
        }
    }
    let _trying = Trying;
    TRIAL_STOPPED.store(false, SeqCst);
    *TRIAL_OF.lock() = Some(file.to_path_buf());
    let path = file.to_string_lossy().into_owned();
    let file = file.to_path_buf();
    let of = of.to_string();
    let policy = policy.to_string();
    // each run may write again what the clean run and the program's earlier
    // runs wrote, and what is under its own `{trial}` place
    let written = Arc::new(sonde_proxy::Written::of(&clean.records));
    let runs = file.parent().unwrap_or(Path::new(".")).to_path_buf();
    let program = sonde_proxy::program::key_of(&clean.header);
    if let Some(p) = &program {
        written.remember(&runs, p, &clean.header.url);
    }
    let words: Vec<String> = plan
        .command
        .split_whitespace()
        .map(|w| w.trim_matches(['"', '\'']).to_string())
        .collect();
    for name in std::iter::once("clean").chain(chosen.iter().map(|s| s.name.as_str())) {
        for place in sonde_proxy::trial::places(&words, name) {
            written.own_place(&place);
        }
    }
    // a new trial replaces the last one's verdicts and sessions
    sonde_proxy::trial::remove(&file);
    let dir = sonde_proxy::trial::folder_for(&file);
    std::fs::create_dir_all(&dir).map_err(err_msg)?;
    let mut trials = Vec::new();
    for scenario in chosen {
        output.post(trial_event(&scenario.name, None));
        let under = Under {
            path: sonde_proxy::session::fresh_path(
                &dir,
                &sonde_proxy::trial::session_stem(&file, &scenario.name),
            ),
            of: of.clone(),
            scenario: scenario.clone(),
            written: written.clone(),
        };
        let ran = run_recorded(
            &plan.command,
            plan.cwd.as_deref(),
            clean.header.endpoint.clone(),
            policy.clone(),
            cols,
            rows,
            output,
            Some(under),
            false,
        )
        .await;
        let stopped = TRIAL_STOPPED.load(SeqCst);
        let (exit, run_path) = match ran {
            Ok(x) if !stopped => x,
            other => {
                // cut short: a verdict on half a trial would be read as one
                sonde_proxy::trial::remove(&file);
                return match other {
                    Err(e) if !stopped => Err(e),
                    _ => Ok(json!({ "path": path, "trials": [], "cancelled": true })),
                };
            }
        };
        let run = open_session(&run_path)?;
        let mut t = sonde_proxy::trial::judge(
            &scenario,
            &clean.records,
            clean_exit,
            &run.records,
            exit.unwrap_or(1),
        );
        t.session = Some(run_path);
        output.post(trial_event(&scenario.name, Some(&t)));
        trials.push(t);
    }
    sonde_proxy::trial::save(&file, &trials).map_err(err_msg)?;
    if let Some(p) = &program {
        let _ = sonde_proxy::trial::record(&runs, p, &of, chrono::Utc::now(), &trials);
    }
    // each with what it was before ("broke on 12 Nov, survived since")
    let trials = trials_of(&file);
    Ok(json!({ "path": path, "trials": trials, "cancelled": false }))
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
                // a finished session is listed from the counts kept beside
                // it: never read whole to be listed
                let counted = sonde_proxy::session::is_compressed(&p)
                    .then(|| sonde_proxy::summary::counted(&p))
                    .flatten();
                let mut row = match counted {
                    Some(c) => json!({
                        "created": c.created.to_rfc3339(),
                        "endpoint": c.endpoint,
                        "command": c.command,
                        "requests": c.requests,
                        "shape": c.shape,
                        "seconds": c.seconds,
                        "errors": c.errors,
                        "program": c.name.or(c.program),
                    }),
                    None => {
                        let Some(h) = header_of(&p) else { continue };
                        json!({
                            "created": h.created.to_rfc3339(),
                            "endpoint": h.endpoint,
                            "command": h.command,
                            "requests": count_records(&p),
                            "shape": shape_of(&p),
                        })
                    }
                };
                let created = row["created"].as_str().unwrap_or_default().to_string();
                row["path"] = json!(p.to_string_lossy());
                row["name"] = json!(p.file_name().map(|n| n.to_string_lossy().into_owned()));
                row["title"] = json!(
                    p.file_name()
                        .and_then(|n| titles.get(&*n.to_string_lossy()))
                );
                row["live"] = json!(live.as_deref() == Some(p.as_path()));
                // a run that was tried says how it stood (the small
                // verdicts file, not the trials' sessions)
                let marks = trial_marks(&p);
                if !marks.is_empty() {
                    row["trials"] = JsonValue::Array(marks);
                }
                out.push((created, row));
            }
        }
        out.sort_by(|a, b| b.0.cmp(&a.0));
        JsonValue::Array(out.into_iter().map(|(_, v)| v).collect())
    }

    /// A session as a window opens it: its header, summary, signals,
    /// profile and findings, the notes said once, and the first page of its
    /// operations with how many there are (`operationsTotal`). Read and
    /// analysed off the interface's thread, once per version of the file.
    pub async fn view(path: String) -> Result<JsonValue, String> {
        tokio::task::spawn_blocking(move || view_of(&path))
            .await
            .map_err(err_msg)?
    }

    /// The requests that started in a stretch of a session (`from_ms` to
    /// `to_ms` after its first request's start), reckoned as a session of
    /// their own: `{ summary, profile, signals, offsetMs }` (its times are
    /// from its own first request, `offsetMs` after the session's) — what the Time pane's
    /// charts that are not drawn over time read while a stretch is chosen,
    /// so every chart follows the selection. Off the interface's thread.
    pub async fn stretch(path: String, from_ms: f64, to_ms: f64) -> Result<JsonValue, String> {
        tokio::task::spawn_blocking(move || {
            let s = open_session(&path)?;
            Ok(stretch_of(
                &s.records,
                &crate::state::request_prices_for(&s.header.endpoint),
                from_ms,
                to_ms,
            ))
        })
        .await
        .map_err(err_msg)?
    }

    /// A range of a session's operations, sorted and narrowed as the table
    /// asks: `{ total, offset, rows }`.
    pub async fn episodes(path: String, query: JsonValue) -> Result<JsonValue, String> {
        let q = query_of(query)?;
        tokio::task::spawn_blocking(move || Ok(analysis_of(&path)?.1.page(&q)))
            .await
            .map_err(err_msg)?
    }

    /// One operation, with every one of its requests' ids.
    pub fn episode(path: String, index: f64) -> Result<JsonValue, String> {
        Ok(to_json(analysis_of(&path)?.1.episode(index as usize)))
    }

    /// The operation a request belongs to (its index).
    pub fn episode_of(path: String, record_id: f64) -> Result<Option<f64>, String> {
        Ok(analysis_of(&path)?
            .1
            .episode_of(record_id as u64)
            .map(|i| i as f64))
    }

    /// The row the table shows an operation at under a query; -1 when the
    /// query leaves it out.
    pub fn episode_row(path: String, query: JsonValue, index: f64) -> Result<f64, String> {
        Ok(analysis_of(&path)?
            .1
            .row_of(&query_of(query)?, index as usize)
            .map_or(-1.0, |r| r as f64))
    }

    /// The operations running at a moment (an output line's), else the nearest.
    pub fn episodes_at(path: String, time: String) -> Result<JsonValue, String> {
        let at = chrono::DateTime::parse_from_rfc3339(&time)
            .map_err(err_msg)?
            .with_timezone(&chrono::Utc);
        Ok(to_json(analysis_of(&path)?.1.episodes_at(at)))
    }

    /// A range of the operations a finding is about (the view lists the first).
    pub fn observation_episodes(
        path: String,
        observation: f64,
        offset: f64,
        limit: f64,
    ) -> Result<JsonValue, String> {
        Ok(to_json(analysis_of(&path)?.1.finding_episodes(
            observation as usize,
            offset as usize,
            limit as usize,
        )))
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
        Ok(to_json(full_record(&path, id as u64)?))
    }

    /// The operations table split by `by` (`operation`, `prefix`, `size`,
    /// `caller`, `sdk`, `status`): each group's requests, errors, p50, p95,
    /// p99, waiting and cost, and the sentence that says where the slow and
    /// the costly requests are (`phrases`, for the reader's language).
    pub async fn split(path: String, by: String) -> Result<JsonValue, String> {
        let by = sonde_proxy::split::Dimension::parse(&by)
            .ok_or_else(|| err_msg(format!("no dimension {by:?}")))?;
        tokio::task::spawn_blocking(move || {
            let s = open_session(&path)?;
            let prices = crate::state::request_prices_for(&s.header.endpoint);
            Ok(to_json(sonde_proxy::split::split(&s.records, by, &prices)))
        })
        .await
        .map_err(err_msg)?
    }

    /// Every request of one key in a session, whatever its operation, in
    /// time order: a request's history for the inspector. Each is the short
    /// row with `operationIndex` (the operation it belongs to, or null).
    pub async fn key_history(
        path: String,
        bucket: String,
        key: String,
    ) -> Result<JsonValue, String> {
        tokio::task::spawn_blocking(move || {
            let (s, a) = analysis_of(&path)?;
            Ok(JsonValue::Array(key_history_of(&s, &a, &bucket, &key)))
        })
        .await
        .map_err(err_msg)?
    }

    /// A session's record as `aws s3api` (`cli`) or unsigned curl (`curl`).
    pub fn copy(path: String, id: f64, kind: String) -> Result<Option<String>, String> {
        Ok(full_record(&path, id as u64)?.map(|r| {
            if kind == "curl" {
                reqlog::copy::as_curl(&r)
            } else {
                reqlog::copy::as_aws_cli(&r)
            }
        }))
    }

    /// Starts recording: a local endpoint forwarding to `endpoint`, refusing
    /// what `policy` (`allow`, `no-delete`, `read-only`; empty: the
    /// `record-policy` setting) forbids, and a run's project's `[record]
    /// policy` too. What to point an application at, and the session file.
    pub async fn start(endpoint: String, policy: String) -> Result<JsonValue, String> {
        begin(endpoint, policy, None, None, None).await
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
        let (code, path) = run_recorded(
            &command,
            cwd.as_deref(),
            endpoint,
            policy,
            cols,
            rows,
            &output,
            None,
            true,
        )
        .await?;
        // kept beside the session: a later trial judges against it
        write_exit(Path::new(&path), code);
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

    /// Ends the running program; a trial ends with it (no further scenario
    /// runs, and nothing of the trial is kept).
    pub fn kill() -> bool {
        if TRYING.load(std::sync::atomic::Ordering::SeqCst) {
            TRIAL_STOPPED.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        RUNNING.lock().as_mut().is_some_and(|r| r.kill())
    }

    /// The budget the session's program has in its project's `sonde.toml`
    /// (found from the folder it ran in), for the charts to draw:
    /// `{ maxP95Ms, maxErrors, maxWaitingMs }`, each null where it gives
    /// none; null for a session with no folder or a project with no budget.
    pub fn budget(path: String) -> JsonValue {
        let Some(dir) = header_of(Path::new(&path)).and_then(|h| h.cwd) else {
            return JsonValue::Null;
        };
        match project_budget(Path::new(&dir)) {
            Some((p95, errors, waiting)) => {
                json!({ "maxP95Ms": p95, "maxErrors": errors, "maxWaitingMs": waiting })
            }
            None => JsonValue::Null,
        }
    }

    /// What trying a session's program under faults would run, said before
    /// it runs: `{ command, cwd, scenarios: [{ name, about, maxTimes }], project,
    /// sharesPlace, cleanExit }`. `project`: the scenarios are the
    /// `sonde.toml`'s own; `sharesPlace`: the command has no `{trial}` (or
    /// `SONDE_TRIAL`), so every run writes where the clean run wrote;
    /// `cleanExit` null when the run's exit code was not kept (0 is assumed).
    pub fn trial_plan(path: String) -> Result<JsonValue, String> {
        let s = open_session(&path)?;
        let plan = trial_plan_of(&s)?;
        Ok(json!({
            "command": plan.command,
            "cwd": plan.cwd,
            "scenarios": plan
                .scenarios
                .iter()
                .map(|s| json!({
                    "name": s.name,
                    "about": s.about,
                    // the attempts in a row it may be asked to fail (`name:N`)
                    "maxTimes": (!plan.project
                        && sonde_proxy::fault::PRESSED.contains(&s.name.as_str()))
                    .then_some(sonde_proxy::fault::MAX_TIMES),
                }))
                .collect::<Vec<_>>(),
            "project": plan.project,
            "sharesPlace": plan.shares_place,
            "cleanExit": read_exit(Path::new(&path)),
        }))
    }

    /// Tries a session's program under faults: its recorded command runs
    /// again in its recorded folder, once per scenario (`scenarios` by
    /// name, `slowdown:3` to fail each request's first three attempts;
    /// none: the plan's), each run recorded to
    /// `trials/<run>.<scenario>.sonde` and judged against the clean run.
    /// Events: `{ scenario, state: "running" }`, the program's output as
    /// `{ data, scenario }`, then `{ scenario, state: "done", trial }`.
    /// Resolves with `{ path, trials, cancelled }`; [`Sessions::kill`]
    /// cancels, and a cancelled trial keeps nothing. The run's earlier
    /// verdicts are replaced. What the program may do is `policy`'s.
    pub async fn trial(
        path: String,
        scenarios: Vec<String>,
        policy: String,
        cols: f64,
        rows: f64,
        #[args(event)] output: Callback<JsonValue>,
    ) -> Result<JsonValue, String> {
        // a run of the sessions directory: its trials are kept beside it
        let (file, of) = own_session(&path)?;
        let clean = open_session(&path)?;
        let plan = trial_plan_of(&clean)?;
        let chosen = scenarios_named(&plan, &scenarios)?;
        policy_of(&policy)?;
        let clean_exit = read_exit(&file).unwrap_or(0);
        try_under(
            &file, &of, &clean, clean_exit, &plan, chosen, &policy, cols, rows, &output,
        )
        .await
    }

    /// Tries a run's program again under what did not survive: its command
    /// runs anew (a fresh clean run, recorded as a run of its own and judged
    /// against), then only the scenarios whose last verdict for the program
    /// was broke, degraded or refused by the policy. Events as
    /// [`Sessions::trial`]'s, the clean run's output first (`{ data }`).
    /// Resolves with `{ path, trials, cancelled, cleanExit }`: `path` the
    /// fresh run; no trial when it exited non-zero.
    pub async fn trial_again(
        path: String,
        policy: String,
        cols: f64,
        rows: f64,
        #[args(event)] output: Callback<JsonValue>,
    ) -> Result<JsonValue, String> {
        let (file, _) = own_session(&path)?;
        let last = open_session(&path)?;
        let plan = trial_plan_of(&last)?;
        let program = sonde_proxy::program::key_of(&last.header)
            .ok_or_else(|| err_msg("only a run of a command can be tried again"))?;
        let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        let names = sonde_proxy::trial::to_try_again(&dir, &program);
        if names.is_empty() {
            return Err(err_msg(
                "nothing to try again: every scenario survived the program's last trial",
            ));
        }
        let chosen = scenarios_named(&plan, &names)?;
        policy_of(&policy)?;
        // the clean run, afresh
        let (code, fresh) = run_recorded(
            &plan.command,
            plan.cwd.as_deref(),
            last.header.endpoint.clone(),
            policy.clone(),
            cols,
            rows,
            &output,
            None,
            false,
        )
        .await?;
        write_exit(Path::new(&fresh), code);
        if code != Some(0) {
            return Ok(
                json!({ "path": fresh, "trials": [], "cancelled": false, "cleanExit": code }),
            );
        }
        let (file, of) = own_session(&fresh)?;
        let clean = open_session(&fresh)?;
        let mut done = try_under(
            &file, &of, &clean, 0, &plan, chosen, &policy, cols, rows, &output,
        )
        .await?;
        done["cleanExit"] = json!(code);
        Ok(done)
    }

    /// What replaying a session on `endpoint` would send: the plan, the
    /// prefix it would write under, the buckets.
    pub fn replay_plan(path: String, endpoint: String) -> Result<JsonValue, String> {
        let original = full_session(&path)?;
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
        }))
    }

    /// Replays a session on `endpoint` under `prefix`, cleans up, and
    /// compares: `{ path, plan, comparison, removed }`. Everything it wrote
    /// under `prefix` is removed after.
    pub async fn replay(
        path: String,
        endpoint: String,
        prefix: String,
    ) -> Result<JsonValue, String> {
        let s = sonde();
        let original = full_session(&path)?;
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
            replay::replay_to_file(&base, &original, &opts, &out, |_, _| {}).await
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
            // one reckoned before the other is opened: a run of either
            // size is held with what the other amounts to, not with it (the
            // later counted against what the earlier's findings would save)
            let run = |path: &str, against: Option<&sonde_proxy::profile::Run>| {
                let s = open_session(path)?;
                let prices = crate::state::request_prices_for(&s.header.endpoint);
                let mut run = sonde_proxy::profile::Run::analysed(&s.header, &s.records, &prices);
                if let Some(a) = against {
                    run.measure(&s.records, a);
                }
                Ok::<_, String>((s.header.command.clone(), run))
            };
            let (ca, a) = run(&before, None)?;
            let (cb, b) = run(&after, Some(&a))?;
            Ok(to_json(
                sonde_proxy::profile::compare_runs(&a, &b)
                    .of_programs(ca.as_deref(), cb.as_deref())
                    .of_exits((
                        sonde_proxy::fixloop::failed(Path::new(&before)),
                        sonde_proxy::fixloop::failed(Path::new(&after)),
                    )),
            ))
        })
        .await
        .map_err(err_msg)?
    }

    /// A run against its program's run before it (the fix loop): `{ program,
    /// previous, change, findings, unsaved }`. `findings` are the run's and
    /// those of the run before it no longer has, each with its `state`
    /// (fixed, less, back, new, open), `predicted` and, when fixed or less,
    /// `measured` (`{ requests, ms }`) and `saved` (a phrase). `change` is
    /// `compareRuns`'s, none for a program's first run. Null for a session
    /// that is no run of a program. Its findings are kept with the
    /// program's when it is the program's newest run.
    pub async fn since_last(path: String) -> Result<JsonValue, String> {
        tokio::task::spawn_blocking(move || {
            let s = open_session(&path)?;
            let prices = crate::state::request_prices_for(&s.header.endpoint);
            let run = sonde_proxy::profile::Run::analysed(&s.header, &s.records, &prices);
            Ok(to_json(sonde_proxy::fixloop::since_last(
                &sonde_proxy::fixloop::folder_of(Path::new(&path)),
                Path::new(&path),
                &s.header,
                &s.records,
                run,
                &crate::state::request_prices_for,
            )))
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
            // each program's command as its newest run wrote it
            let mut commands = Newest::default();
            for p in std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sonde"))
            {
                if let Some(h) = header_of(&p)
                    && let Some(k) = sonde_proxy::program::key_of(&h)
                {
                    commands.insert(k.clone(), (h.created.to_rfc3339(), h.command.clone()));
                    by.entry(k).or_default().push((h.created.to_rfc3339(), p));
                }
            }
            let bases = baselines()?;
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
                            "command": commands.get(&key),
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
            let header = header_of(Path::new(&path));
            let command = header.as_ref().and_then(|h| h.command.clone());
            let Some(key) = header.as_ref().and_then(sonde_proxy::program::key_of) else {
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
                    (sonde_proxy::program::key_of(&h).as_deref() == Some(key.as_str()))
                        .then(|| (h.created.to_rfc3339(), p))
                })
                .collect();
            runs.sort();
            // each run with the commit its code was at
            let dated: Vec<(
                sonde_proxy::program::RunPoint,
                Option<sonde_proxy::session::GitStamp>,
            )> = runs
                .into_iter()
                .rev()
                .take(30)
                .rev()
                .filter_map(|(_, p)| Some((run_point(&p)?, header_of(&p).and_then(|h| h.git))))
                .collect();
            let commits: Vec<_> = dated.iter().map(|(_, g)| g.clone()).collect();
            let runs: Vec<_> = dated.iter().map(|(r, _)| r.clone()).collect();
            let base = baselines()?
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
                        let ob = sonde_proxy::episode::observations_of(
                            &s.header,
                            &s.records,
                            &episodes(&s.records),
                        );
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
                    let c = sonde_proxy::program::check(
                        (&rp, &rs.records, &rpr, &rob),
                        Some((&bp, &bs.records, &bpr, &bob)),
                        &budget,
                    );
                    // the same check as a pull request's comment
                    let md = sonde_proxy::program::check_markdown(&c, &rp.currency, None, &rob);
                    Some((c, md))
                }
                _ => None,
            };
            let (check, markdown) = match check {
                Some((c, md)) => (Some(c), Some(md)),
                None => (None, None),
            };
            // the baseline's own figures, though it be older than the runs shown,
            // and what a run may grow to before `sonde check` fails it
            let base_point = base.as_ref().and_then(|b| run_point(b));
            let limits = base_point.as_ref().map(|b| {
                sonde_proxy::program::baseline_limits(b, sonde_proxy::program::DEFAULT_TOLERANCE)
            });
            // where it went past the baseline, and the commits on either side
            let since = base_point.as_ref().and_then(|b| {
                sonde_proxy::program::since(&dated, b, sonde_proxy::program::DEFAULT_TOLERANCE)
            });
            Ok(json!({
                "program": key,
                // the key the runs are grouped by, and this run's command whole
                "command": command,
                "runs": runs,
                "commits": commits,
                "since": since,
                "baseline": base.map(|b| b.to_string_lossy().into_owned()),
                "baselinePoint": base_point,
                "limits": limits,
                "check": check,
                "markdown": markdown,
                // what the run's last trial found (`sonde run --trial`)
                "trials": trials_of(Path::new(&path)),
            }))
        })
        .await
        .map_err(err_msg)?
    }

    /// What changed in the program's code between two of its runs: the files
    /// `git diff` names between their commits (at most 6, the ones its
    /// findings' requests came from first), each before and after, with the
    /// lines of the later run's findings. `{ from, to, files: [{ path, old,
    /// new, notes: [{ line, text }] }], more }`.
    pub async fn code_change(before: String, after: String) -> Result<JsonValue, String> {
        tokio::task::spawn_blocking(move || code_change_of(&before, &after))
            .await
            .map_err(err_msg)?
    }

    /// Makes a session its program's baseline (`on`), or no longer.
    pub fn set_baseline(path: String, on: bool) -> Result<bool, String> {
        let (_, name) = own_session(&path)?;
        let key = header_of(Path::new(&path))
            .and_then(|h| sonde_proxy::program::key_of(&h))
            .ok_or_else(|| err_msg("only a run of a program has a baseline"))?;
        let mut all = map_to_change(&sonde().paths().sessions().join("baselines.json"))?;
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
        reqlog::private::write(&path, text).map_err(err_msg)?;
        // a finished session, kept as every other is: compressed, so the
        // list reads its shape (an open file being written has none)
        sonde_proxy::session::compress(&path).map_err(err_msg)?;
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
        // an index that does not read is left as it is (the session is
        // still made; opening the file again makes another)
        let known = map_to_change(&dir.join("imports.json"));
        let mut imported = known.clone().unwrap_or_default();
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
            reqlog::private::write(&path, text).map_err(err_msg)?;
            if let Err(e) = Session::open(&path) {
                let _ = std::fs::remove_file(&path);
                return Err(format!("{name}: {e}"));
            }
        }
        if let Some(n) = path.file_name()
            && known.is_ok()
        {
            imported.insert(digest, n.to_string_lossy().into_owned());
            write_imports(&dir, &imported);
        }
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
        let bytes = std::fs::read(p).map_err(|e| format!("{path}: {e}"))?;
        let text = file_text(&bytes).map_err(|e| format!("{path}: {e}"))?;
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::import(name, text)
    }

    /// Gives a session a title of the user's (an empty one takes it away).
    pub fn rename(path: String, title: String) -> Result<bool, String> {
        let (_, name) = own_session(&path)?;
        let mut t = map_to_change(&sonde().paths().sessions().join("titles.json"))?;
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
        // its trials go with it, and all its index kept of it
        sonde_proxy::trial::remove(&p);
        forget(&p);
        sonde_proxy::index::set(&p, sonde_proxy::index::Change::Gone);
        // the session is gone either way; a titles file that does not read stays
        if let Ok(mut t) = map_to_change(&sonde().paths().sessions().join("titles.json"))
            && t.remove(&name).is_some()
        {
            save_titles(&t)?;
        }
        Ok(true)
    }

    /// Opens the file a finding's code place names (`etl/load.py:212 read`):
    /// the place is text the recorded program (or whoever shared the session)
    /// wrote, so only a regular file inside the folder the session ran in,
    /// and not one that runs, is opened; anything else is shown in the file
    /// manager, never run.
    pub async fn open_code(session: String, place: String) -> Result<(), String> {
        let dir = header_of(Path::new(&session))
            .and_then(|h| h.cwd)
            .ok_or_else(|| err_msg("the session does not say which folder it ran in"))?;
        match code_target(Path::new(&dir), &place)? {
            CodeTarget::Open(p) => nsh_core::element::open_path(p).await,
            CodeTarget::Reveal(p) => nsh_core::element::reveal_path(p).await,
        }
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
        let session = full_session(&path)?;
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

    /// The app is quitting: ends the program being recorded and a running
    /// trial, and writes the recording as a finished session, within the
    /// 200 ms a quitting app has. `{ program, trial, session }`: whether a
    /// program was ended, whether a trial was, and the session written (none
    /// for a trial, which keeps nothing when it is cut short).
    pub async fn quit() -> JsonValue {
        end_for_quit().await
    }

    /// Stops recording; the session file's path. No longer the recording
    /// at once; the file is whole and compressed once the requests in flight
    /// have been answered (five seconds at most), off the calling thread.
    pub fn stop() -> Option<String> {
        let r = RECORDING.lock().take()?;
        let path = r.path.to_string_lossy().into_owned();
        // a thread of its own on the recording's runtime: a handler is in no
        // runtime, and off the interface's thread has no reactive one either
        let runtime = r.runtime.clone();
        std::thread::spawn(move || runtime.block_on(r.finish()));
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn the_cursors_question_is_found_when_two_reads_split_it() {
        let mut asks = CursorAsks::default();
        assert_eq!(asks.asked("hello\x1b[6n"), 1);
        assert_eq!(asks.asked("more"), 0, "a question is answered once");
        // split at every place it can be
        for at in 1..ASKS_CURSOR.len() {
            let mut asks = CursorAsks::default();
            let (a, b) = ASKS_CURSOR.split_at(at);
            assert_eq!(asks.asked(&format!("starting{a}")), 0, "{at}");
            assert_eq!(asks.asked(&format!("{b}then")), 1, "{at}");
        }
        // over three reads, and a piece that begins it but does not go on
        let mut asks = CursorAsks::default();
        assert_eq!(asks.asked("\x1b"), 0);
        assert_eq!(asks.asked("["), 0);
        assert_eq!(asks.asked("6n\x1b[6n"), 2);
        assert_eq!(asks.asked("\x1b["), 0);
        assert_eq!(asks.asked("1;1H"), 0);
        assert_eq!(asks.asked("6n"), 0, "what was carried was not the question");
        // the text around it is no part of what is carried
        let mut asks = CursorAsks::default();
        assert_eq!(asks.asked("日本\x1b[6"), 0);
        assert_eq!(asks.asked("n語"), 1);
    }

    #[test]
    fn the_example_session_has_a_shape_for_the_list_once_it_is_kept_as_finished() {
        const EXAMPLE: &[u8] = include_bytes!("../../proxy/tests/fixtures/slatedb.sonde");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("example-slatedb.sonde");
        std::fs::write(&path, EXAMPLE).unwrap();
        // a file still open (not compressed) is still growing: no shape yet
        assert!(shape_of(&path).is_empty());
        // as `example()` keeps it: every request in one of the slices
        sonde_proxy::session::compress(&path).unwrap();
        let shape = shape_of(&path);
        assert_eq!(shape.len(), SHAPE_BINS);
        assert_eq!(shape.iter().sum::<u32>(), 172);
    }

    #[test]
    fn a_code_place_opens_only_a_file_of_the_run_and_never_one_that_runs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("etl")).unwrap();
        std::fs::write(root.join("etl/load.py"), "x").unwrap();
        std::fs::write(root.join("run.sh"), "x").unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        // a .py is opened, except on Windows, where opening runs it
        let load = root.join("etl").join("load.py");
        assert_eq!(
            code_target(&root, "etl/load.py:212 read").unwrap(),
            if cfg!(windows) {
                CodeTarget::Reveal(load)
            } else {
                CodeTarget::Open(load)
            }
        );
        assert_eq!(
            code_target(&root, "run.sh:1 main").unwrap(),
            CodeTarget::Reveal(root.join("run.sh"))
        );
        let o = outside.path().canonicalize().unwrap();
        assert_eq!(
            code_target(&root, &format!("{}:3 f", o.display())).unwrap(),
            CodeTarget::Reveal(o)
        );
        assert!(code_target(&root, "../../nothing-here.py:1 f").is_err());
    }

    #[test]
    fn a_code_place_opens_source_and_text_and_only_shows_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for name in [
            "a.rs",
            "notes.MD",
            "tool",
            "x.hta",
            "x.pyw",
            "x.terminal",
            "x.desktop",
        ] {
            std::fs::write(root.join(name), "x").unwrap();
        }
        let target = |n: &str| code_target(&root, &format!("{n}:1 f")).unwrap();
        assert_eq!(target("a.rs"), CodeTarget::Open(root.join("a.rs")));
        assert_eq!(target("notes.MD"), CodeTarget::Open(root.join("notes.MD")));
        // not on the list, whatever a deny-list would have missed
        for n in ["tool", "x.hta", "x.pyw", "x.terminal", "x.desktop"] {
            assert_eq!(target(n), CodeTarget::Reveal(root.join(n)), "{n}");
        }
        // what Windows runs when opened is shown there
        assert!(opens("py", false) && !opens("py", true));
        assert!(opens("JS", false) && !opens("js", true));
        assert!(opens("rs", true) && !opens("exe", false));
    }

    #[test]
    fn a_session_that_names_a_folder_elsewhere_opens_nothing() {
        assert!(remote(r"\\host\share\etl"));
        assert!(remote("//host/share"));
        assert!(remote(r"\\?\UNC\host\share"));
        assert!(!remote(r"\\?\C:\work"));
        assert!(!remote("/home/me/work") && !remote(r"C:\work"));
        // refused before the path is looked at
        let e = code_target(Path::new("//host/share"), "load.py:1 f").unwrap_err();
        assert!(e.contains("another machine"), "{e}");
        let dir = tempfile::tempdir().unwrap();
        assert!(code_target(dir.path(), "//host/share/load.py:1 f").is_err());
        // a session that says it ran in the whole disk: nothing is "inside" it
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("load.py"), "x").unwrap();
        let top = root.ancestors().last().unwrap();
        assert_eq!(
            code_target(top, &format!("{}:1 f", root.join("load.py").display())).unwrap(),
            CodeTarget::Reveal(root.join("load.py"))
        );
    }

    #[test]
    fn an_unknown_recording_policy_is_refused_not_allowed() {
        assert_eq!(policy_of("allow").unwrap(), Policy::Allow);
        assert_eq!(policy_of("").unwrap(), Policy::NoDelete, "safe by default");
        assert_eq!(policy_of("no-delete").unwrap(), Policy::NoDelete);
        assert_eq!(policy_of("read-only").unwrap(), Policy::ReadOnly);
        assert!(policy_of("readonly").unwrap_err().contains("readonly"));
        assert!(policy_of("no_delete").is_err());
    }

    #[test]
    fn a_projects_policy_narrows_the_one_asked_and_never_widens_it() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join(".git")).unwrap();
        let at = |asked| run_policy(asked, Some(t.path()));
        assert_eq!(at(Policy::Allow).unwrap(), Policy::Allow);
        let file = t.path().join("sonde.toml");
        std::fs::write(&file, "[record]\npolicy = \"read-only\"\n").unwrap();
        assert_eq!(at(Policy::Allow).unwrap(), Policy::ReadOnly);
        std::fs::write(&file, "[record]\npolicy = \"allow\"\n").unwrap();
        assert_eq!(at(Policy::NoDelete).unwrap(), Policy::NoDelete);
        // what the command line refuses, the app refuses
        std::fs::write(&file, "[record]\npolicy = \"all\"\n").unwrap();
        assert!(at(Policy::Allow).unwrap_err().contains("one of allow"));
        assert_eq!(run_policy(Policy::Allow, None).unwrap(), Policy::Allow);
    }

    #[test]
    fn a_damaged_map_is_refused_not_read_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("titles.json");
        assert!(map_to_change(&f).unwrap().is_empty(), "none yet: empty");
        std::fs::write(&f, "{\"a.sonde\": \"A\"}").unwrap();
        assert_eq!(map_to_change(&f).unwrap()["a.sonde"], "A");
        // cut short: every entry would go with the next write
        std::fs::write(&f, "{\"a.sonde\": \"A\", \"b.so").unwrap();
        let e = map_to_change(&f).unwrap_err();
        assert!(e.contains("nothing was changed"), "{e}");
    }

    #[test]
    fn the_code_between_two_runs_with_the_later_findings_on_their_lines() {
        use reqlog::Sink;
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join("etl")).unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                ok.status.success(),
                "{}",
                String::from_utf8_lossy(&ok.stderr)
            );
            String::from_utf8_lossy(&ok.stdout).trim().to_string()
        };
        git(&["init", "-q"]);
        std::fs::write(repo.join("etl/load.py"), "a = 1\nread()\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "one"]);
        let one = git(&["rev-parse", "HEAD"]);
        std::fs::write(
            repo.join("etl/load.py"),
            "a = 1\nfor k in keys:\n    read(k)\n",
        )
        .unwrap();
        git(&["commit", "-qam", "two"]);
        let two = git(&["rev-parse", "HEAD"]);
        let run = |name: &str, commit: &str, n: u64| {
            let path = dir.path().join(name);
            let mut h =
                SessionHeader::new("ep", "http://s3", "", Some("python etl/load.py".into()));
            h.cwd = Some(repo.to_string_lossy().into_owned());
            h.git = Some(sonde_proxy::session::GitStamp {
                commit: commit.into(),
                branch: None,
                dirty: false,
            });
            let w = SessionWriter::create(&path, &h).unwrap();
            for i in 0..n {
                w.write(&reqlog::Record {
                    id: i + 1,
                    time: chrono::Utc::now(),
                    operation: "GetObject".into(),
                    method: "GET".into(),
                    bucket: Some("b".into()),
                    key: Some("hot".into()),
                    status: Some(200),
                    latency_ms: 2.0,
                    request_headers: vec![reqlog::Header::new(
                        "x-sonde-caller",
                        "etl/load.py:3 main",
                    )],
                    ..Default::default()
                });
            }
            drop(w);
            path.to_string_lossy().into_owned()
        };
        let a = run("a.sonde", &one, 1);
        let b = run("b.sonde", &two, 30);
        let c = code_change_of(&a, &b).unwrap();
        assert_eq!(c["files"][0]["path"], "etl/load.py");
        assert!(
            c["files"][0]["new"]
                .as_str()
                .unwrap()
                .contains("for k in keys")
        );
        assert_eq!(c["files"][0]["notes"][0]["line"], 3, "{c:#}");
        // a commit that is not one is never handed to git
        let mut bad = SessionHeader::new("ep", "http://s3", "", None);
        bad.git = Some(sonde_proxy::session::GitStamp {
            commit: "--output=/tmp/x".into(),
            branch: None,
            dirty: false,
        });
        bad.cwd = Some(repo.to_string_lossy().into_owned());
        let p = dir.path().join("bad.sonde");
        drop(SessionWriter::create(&p, &bad).unwrap());
        assert!(code_change_of(&a, &p.to_string_lossy()).is_err());
    }

    #[test]
    fn a_finished_session_from_elsewhere_reads_as_its_text() {
        use reqlog::Sink;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ci.sonde");
        let w =
            SessionWriter::create(&path, &SessionHeader::new("ep", "http://s3", "", None)).unwrap();
        w.write(&reqlog::Record {
            id: 1,
            operation: "GetObject".into(),
            ..Default::default()
        });
        drop(w);
        sonde_proxy::session::compress(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(&[0x1f, 0x8b]));
        let text = file_text(&bytes).unwrap();
        assert!(text.starts_with("{\"format\":\"sonde-session\""), "{text}");
        assert_eq!(file_text(b"plain").unwrap(), "plain");
    }

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

    #[tokio::test]
    async fn a_request_in_flight_when_a_recording_stops_is_in_the_compressed_file() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // a store that answers 300 ms after it is asked
        let store = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream = format!("http://{}", store.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let (mut s, _) = store.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut seen = Vec::new();
                    let mut buf = [0u8; 1024];
                    while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                        match s.read(&mut buf).await {
                            Ok(n) if n > 0 => seen.extend_from_slice(&buf[..n]),
                            _ => return,
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    let _ = s
                        .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
                        .await;
                });
            }
        });
        let client = s3raw::S3Client::new(
            s3raw::ClientConfig::new("target", &upstream, "eu-west-1").unwrap(),
            Arc::new(s3raw::StaticProvider(Some(s3raw::Credentials::new(
                "AKID", "secret", None,
            )))),
            RequestLog::default(),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sonde");
        let header = SessionHeader::new("target", &upstream, "eu-west-1", None);
        let writer = Arc::new(SessionWriter::create(&path, &header).unwrap());
        let log = RequestLog::new(16);
        log.set_sink(Some(writer.clone()));
        let cfg = ProxyConfig {
            policy: Policy::Allow,
            ..Default::default()
        };
        let proxy = Proxy::start(client, log.clone(), cfg).await.unwrap();
        let mut app = tokio::net::TcpStream::connect(proxy.addr()).await.unwrap();
        // signed as an application would, with the proxy's own keys
        let (key, secret) = proxy.client_keys();
        let now = chrono::Utc::now();
        let date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let headers = vec![
            ("host".to_string(), proxy.addr().to_string()),
            ("x-amz-date".to_string(), date.clone()),
            (
                "x-amz-content-sha256".to_string(),
                "UNSIGNED-PAYLOAD".to_string(),
            ),
        ];
        let auth = s3raw::sigv4::sign(
            &s3raw::sigv4::SignInput {
                method: "GET",
                canonical_uri: "/data/k",
                query: &[],
                headers: &headers,
                payload_hash: "UNSIGNED-PAYLOAD",
                region: "us-east-1",
                service: "s3",
                time: now,
            },
            &s3raw::Credentials::new(key, secret, None),
        )
        .authorization(key);
        let ask = format!(
            "GET /data/k HTTP/1.1\r\nhost: {}\r\nx-amz-date: {date}\r\n\
             x-amz-content-sha256: UNSIGNED-PAYLOAD\r\nauthorization: {auth}\r\n\r\n",
            proxy.addr()
        );
        app.write_all(ask.as_bytes()).await.unwrap();
        // the proxy has the request, the store has not answered
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(log.list(&reqlog::Filter::default(), 10).is_empty());
        let recording = Recording {
            proxy,
            path: path.clone(),
            endpoint: "target".into(),
            log,
            writer,
            runtime: tokio::runtime::Handle::current(),
            written: Arc::default(),
        };
        let file = recording.finish().await;
        assert_eq!(Path::new(&file), path);
        assert!(sonde_proxy::session::is_compressed(&path));
        let s = Session::open(&path).unwrap();
        assert_eq!(
            s.records.len(),
            1,
            "recorded before the file was compressed"
        );
        assert_eq!(s.records[0].status, Some(200));
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

    /// A session of `n` requests, each an operation of its own, the later
    /// the quicker; every seventh a write, one in fifty failed.
    fn many(path: &Path, n: u64) {
        use reqlog::Sink;
        let w =
            SessionWriter::create(path, &SessionHeader::new("ep", "http://s3", "", None)).unwrap();
        for i in 0..n {
            w.write(&reqlog::Record {
                id: i + 1,
                time: chrono::DateTime::from_timestamp_millis(1_700_000_000_000 + i as i64 * 100)
                    .unwrap(),
                method: if i % 7 == 0 { "PUT" } else { "GET" }.into(),
                operation: if i % 7 == 0 { "PutObject" } else { "GetObject" }.into(),
                url: format!("http://s3/b/data/k{i:05}"),
                bucket: Some("b".into()),
                key: Some(format!("data/k{i:05}").into()),
                status: Some(if i % 50 == 3 { 500 } else { 200 }),
                latency_ms: 1.0 + (n - i) as f64 / 10.0,
                request_headers: vec![
                    reqlog::Header::new("authorization", "AWS4-HMAC-SHA256 Credential=…"),
                    reqlog::Header::new("user-agent", "aws-sdk-java/2.25"),
                ],
                ..Default::default()
            });
        }
        w.flush().unwrap();
    }

    /// Group by's table, and a key's history for the inspector.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_session_splits_by_a_dimension_and_a_key_has_its_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("many.sonde");
        many(&path, 300);
        let p = path.to_string_lossy().into_owned();
        let v = Sessions::split(p.clone(), "operation".into())
            .await
            .unwrap();
        assert_eq!(v["by"], "operation");
        assert_eq!(v["sdk"], "aws-sdk-java 2.25");
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        let get = rows.iter().find(|r| r["key"] == "GetObject").unwrap();
        assert!(get["errors"].as_u64().unwrap() > 0 && get["p99Ms"].is_number());
        assert!(Sessions::split(p.clone(), "bucket".into()).await.is_err());
        let h = Sessions::key_history(p.clone(), "b".into(), "data/k00007".into())
            .await
            .unwrap();
        let h = h.as_array().unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0]["operationIndex"].as_u64(), Some(7));
        assert_eq!(h[0]["operation"], "PutObject");
        assert_eq!(h[0]["target"], "b/data/k00007");
        let none = Sessions::key_history(p, "b".into(), "nope".into())
            .await
            .unwrap();
        assert_eq!(none, json!([]));
    }

    #[test]
    fn a_stretch_is_the_requests_that_started_in_it_reckoned_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("many.sonde");
        // one request every 100 ms
        many(&path, 100);
        let s = open_session(&path.to_string_lossy()).unwrap();
        let prices = sonde_proxy::profile::RequestPrices::default();
        // from 1 s up to (not including) 2 s: ten requests
        let v = stretch_of(&s.records, &prices, 1000.0, 2000.0);
        assert_eq!(v["summary"]["requests"], 10);
        assert_eq!(
            v["offsetMs"], 1000.0,
            "its times are from its own first request"
        );
        assert!(
            v["profile"]["groups"]
                .as_array()
                .is_some_and(|g| !g.is_empty())
        );
        assert!(v["signals"]["statuses"].is_array());
        // a stretch nothing started in: a session of nothing, said as such
        let none = stretch_of(&s.records, &prices, 50_000.0, 60_000.0);
        assert_eq!(none["summary"]["requests"], 0);
    }

    #[test]
    fn a_view_is_the_first_page_of_operations_and_the_rest_come_in_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("many.sonde");
        many(&path, 1200);
        let p = path.to_string_lossy().into_owned();
        let v = view_of(&p).unwrap();
        assert_eq!(v["summary"]["requests"], 1200);
        assert_eq!(v["operationsTotal"], 1200);
        assert_eq!(
            v["operations"].as_array().unwrap().len(),
            sonde_proxy::view::PAGE
        );
        assert_eq!(v["operations"][2]["index"], 2);
        assert_eq!(v["operations"][2]["requests"], 1);
        assert!(
            v["operations"][2].get("title").is_none(),
            "said by its phrases"
        );
        assert!(v["summary"]["operations"][0]["p99Ms"].is_number());
        // the calls a table makes: a range in its order, one operation, where
        // a request and a moment are, the row an operation stands at
        let (_, a) = analysis_of(&p).unwrap();
        let page = a.page(
            &query_of(json!({ "offset": 1000, "sort": "duration", "descending": true })).unwrap(),
        );
        assert_eq!(
            (&page["total"], &page["offset"]),
            (&json!(1200), &json!(1000))
        );
        assert_eq!(page["rows"].as_array().unwrap().len(), 200);
        assert_eq!(page["rows"][0]["index"], 1000, "the earliest took longest");
        let failed = a.page(&query_of(json!({ "failed": true, "text": "K0000" })).unwrap());
        assert_eq!(failed["total"], 1, "{failed}");
        assert_eq!(failed["rows"][0]["index"], 3);
        assert_eq!(query_of(JsonValue::Null).unwrap(), Default::default());
        assert!(query_of(json!({ "sort": "colour" })).is_err());
        assert_eq!(
            Sessions::episode(p.clone(), 700.0).unwrap()["records"],
            json!([701])
        );
        assert_eq!(
            Sessions::episode(p.clone(), 7000.0).unwrap(),
            JsonValue::Null
        );
        assert_eq!(Sessions::episode_of(p.clone(), 701.0).unwrap(), Some(700.0));
        assert_eq!(Sessions::episode_of(p.clone(), 7001.0).unwrap(), None);
        let backwards = json!({ "descending": true });
        assert_eq!(
            Sessions::episode_row(p.clone(), backwards.clone(), 0.0).unwrap(),
            1199.0
        );
        assert_eq!(
            Sessions::episode_row(p.clone(), json!({ "failed": true }), 0.0).unwrap(),
            -1.0
        );
        let at = Sessions::episodes_at(p.clone(), "2023-11-14T22:13:20.050Z".into()).unwrap();
        assert_eq!(at, json!([0]), "the first request was under way then");
        assert!(Sessions::episodes_at(p.clone(), "then".into()).is_err());
    }

    #[test]
    fn a_request_of_a_session_kept_lean_is_read_from_its_file_in_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lean.sonde");
        many(&path, 300);
        sonde_proxy::session::compress(&path).unwrap();
        // (sessions past `LEAN_FROM` on disk are kept so)
        assert!(kept_lean(LEAN_FROM + 1, true) && !kept_lean(LEAN_FROM, true));
        assert!(
            !kept_lean(LEAN_FROM * 5, false),
            "a live file is ten times its compressed size"
        );
        let lean = Session::open_lean(&path).unwrap();
        assert!(lean.records[41].url.is_empty());
        let full = record_from_file(&path, &lean, 42).unwrap().unwrap();
        assert_eq!(full.url, "http://s3/b/data/k00041");
        assert!(
            full.request_headers
                .iter()
                .any(|h| h.name == "authorization")
        );
        assert_eq!(record_from_file(&path, &lean, 4200).unwrap(), None);
    }

    #[test]
    fn a_finished_session_is_listed_from_the_counts_kept_in_its_folders_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("listed.sonde");
        many(&path, 120);
        // live: counted as it grows, nothing kept
        assert_eq!(count_records(&path), 120);
        assert!(sonde_proxy::index::get(&path).counted.is_none());
        sonde_proxy::session::compress(&path).unwrap();
        assert_eq!(count_records(&path), 120);
        assert_eq!(shape_of(&path).iter().sum::<u32>(), 120);
        let kept = sonde_proxy::summary::kept(&path).expect("kept in the index");
        assert_eq!((kept.requests, kept.errors), (120, 3));
        // gone with the session
        forget(&path);
        assert!(sonde_proxy::index::get(&path).counted.is_none());
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
            key: Some(format!("k{id}").into()),
            status: Some(200),
            ..Default::default()
        };
        w.write(&rec(1));
        let p = path.to_string_lossy().into_owned();
        let a = analysis_of(&p).unwrap().1;
        assert!(
            Arc::ptr_eq(&a, &analysis_of(&p).unwrap().1),
            "unchanged: the same analysis"
        );
        assert_eq!(a.summary.requests, 1);
        assert_eq!(view_of(&p).unwrap()["summary"]["requests"], 1);
        w.write(&rec(2));
        let b = view_of(&p).unwrap();
        assert_eq!(b["summary"]["requests"], 2, "grown: analysed again");
        assert_eq!(b["operationsTotal"], 2);
        assert!(Arc::ptr_eq(
            &open_session(&p).unwrap(),
            &open_session(&p).unwrap()
        ));
    }
    /// A run of `command` in `dir` (a sessions directory) with these
    /// records, and the trial of it under `scenario` in `dir/trials`.
    fn run_and_trial(
        dir: &Path,
        command: &str,
        cwd: Option<&Path>,
    ) -> (String, String, sonde_proxy::trial::Trial) {
        use reqlog::Sink;
        std::fs::create_dir_all(dir.join("trials")).unwrap();
        let rec = |id: u64, method: &str, op: &str, injected: Option<&str>| reqlog::Record {
            id,
            time: chrono::Utc::now(),
            method: method.into(),
            operation: op.into(),
            bucket: Some("b".into()),
            key: Some(format!("k{id}").into()),
            status: Some(if injected.is_some() { 503 } else { 200 }),
            injected: injected.map(String::from),
            ..Default::default()
        };
        let clean = dir.join("20260101-000000-job.sonde");
        let mut header = SessionHeader::new("ep", "http://s3", "", Some(command.into()));
        if let Some(d) = cwd {
            header = header.ran_in(d);
        }
        let w = SessionWriter::create(&clean, &header).unwrap();
        w.write(&rec(1, "PUT", "PutObject", None));
        w.write(&rec(2, "GET", "GetObject", None));
        w.write_log("clean output\n");
        w.flush().unwrap();
        let trial = dir
            .join("trials")
            .join("20260101-000000-job.slowdown.sonde");
        header.scenario = Some("slowdown".into());
        header.trial_of = Some("20260101-000000-job.sonde".into());
        let w = SessionWriter::create(&trial, &header).unwrap();
        w.write(&rec(1, "PUT", "PutObject", Some("slowdown")));
        w.write(&rec(2, "PUT", "PutObject", None));
        w.write_log("trial output\n");
        w.flush().unwrap();
        let scenario = sonde_proxy::fault::scenario("slowdown").unwrap();
        let (c, t) = (
            Session::open(&clean).unwrap(),
            Session::open(&trial).unwrap(),
        );
        let mut verdict = sonde_proxy::trial::judge(&scenario, &c.records, 0, &t.records, 0);
        verdict.session = Some(trial.to_string_lossy().into_owned());
        (
            clean.to_string_lossy().into_owned(),
            trial.to_string_lossy().into_owned(),
            verdict,
        )
    }

    #[test]
    fn a_trials_session_opens_like_any_other_and_says_what_it_is_a_trial_of() {
        let dir = tempfile::tempdir().unwrap();
        let (clean, trial, _) = run_and_trial(dir.path(), "python job.py", None);
        // open, analyse, records, record, copy and log_lines read the same session
        let view = view_of(&trial).unwrap();
        assert_eq!(view["summary"]["requests"], 2);
        assert_eq!(view["trial"]["scenario"], "slowdown");
        assert_eq!(view["trial"]["of"], "20260101-000000-job.sonde");
        assert_eq!(view["trial"]["clean"], clean.as_str());
        assert!(view.get("trials").is_none());
        let rows = Sessions::records(trial.clone(), vec![1.0, 2.0]).unwrap();
        assert_eq!(rows[0]["injected"], "slowdown", "{rows}");
        assert!(rows[1].get("injected").is_none(), "{rows}");
        let full = Sessions::record(trial.clone(), 1.0).unwrap();
        assert_eq!(full["injected"], "slowdown");
        assert!(
            Sessions::copy(trial.clone(), 1.0, "curl".into())
                .unwrap()
                .is_some()
        );
        let lines = Sessions::log_lines(trial.clone()).unwrap();
        assert!(lines.to_string().contains("trial output"), "{lines}");
        // a clean run without verdicts says nothing of trials
        let view = view_of(&clean).unwrap();
        assert!(view.get("trial").is_none() && view.get("trials").is_none());
    }

    #[test]
    fn a_clean_run_with_verdicts_carries_them_and_its_list_row_marks_them() {
        let dir = tempfile::tempdir().unwrap();
        let (clean, trial, verdict) = run_and_trial(dir.path(), "python job.py", None);
        assert!(trial_marks(Path::new(&clean)).is_empty());
        // read before the verdicts were written: the analysis is kept, the
        // verdicts are read fresh
        let before = analysis_of(&clean).unwrap().1;
        sonde_proxy::trial::save(Path::new(&clean), std::slice::from_ref(&verdict)).unwrap();
        assert!(Arc::ptr_eq(&before, &analysis_of(&clean).unwrap().1));
        let view = view_of(&clean).unwrap();
        assert_eq!(view["trials"][0]["scenario"], "slowdown");
        assert_eq!(view["trials"][0]["standing"], to_json(verdict.standing));
        assert_eq!(view["trials"][0]["session"], trial.as_str());
        let marks = trial_marks(Path::new(&clean));
        assert_eq!(
            marks,
            vec![json!({
                "scenario": "slowdown",
                "standing": verdict.standing,
                "session": trial,
            })]
        );
        // a trial's session that is gone is not offered for opening
        std::fs::remove_file(&trial).unwrap();
        assert_eq!(
            trial_marks(Path::new(&clean))[0]["session"],
            JsonValue::Null
        );
    }

    #[test]
    fn a_runs_verdicts_carry_their_programs_history() {
        let dir = tempfile::tempdir().unwrap();
        let (clean, _, verdict) = run_and_trial(dir.path(), "python job.py", None);
        sonde_proxy::trial::save(Path::new(&clean), std::slice::from_ref(&verdict)).unwrap();
        let mut before = verdict.clone();
        before.standing = sonde_proxy::trial::Standing::Broke;
        let day = chrono::Utc::now() - chrono::Duration::days(1);
        for (run, t, at) in [
            ("earlier.sonde", &before, day),
            ("20260101-000000-job.sonde", &verdict, chrono::Utc::now()),
        ] {
            sonde_proxy::trial::record(
                dir.path(),
                "python job.py",
                run,
                at,
                std::slice::from_ref(t),
            )
            .unwrap();
        }
        let view = view_of(&clean).unwrap();
        let history = view["trials"][0]["history"].as_array().unwrap();
        assert_eq!(history.len(), 2, "{history:?}");
        assert_eq!(history[0]["standing"], "broke");
        assert_eq!(history[1]["run"], "20260101-000000-job.sonde");
    }

    #[test]
    fn a_trials_file_is_not_renamed_removed_or_made_a_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let (clean, trial, _) = run_and_trial(dir.path(), "python job.py", None);
        assert_eq!(
            own_session_in(dir.path(), &clean).unwrap(),
            "20260101-000000-job.sonde"
        );
        let e = own_session_in(dir.path(), &trial).unwrap_err();
        assert!(e.contains("a trial's session goes with its run"), "{e}");
        // and a file elsewhere is still only "not one of sonde's"
        let other = tempfile::tempdir().unwrap();
        let (elsewhere, _, _) = run_and_trial(other.path(), "python job.py", None);
        let e = own_session_in(dir.path(), &elsewhere).unwrap_err();
        assert!(e.contains("is not one of sonde's sessions"), "{e}");
    }

    #[test]
    fn a_trial_plan_is_the_scenarios_chosen_from_what_the_run_did() {
        let dir = tempfile::tempdir().unwrap();
        let (clean, trial, _) = run_and_trial(dir.path(), "python job.py --out s3://b/x", None);
        let s = Session::open(Path::new(&clean)).unwrap();
        let plan = trial_plan_of(&s).unwrap();
        let chosen: Vec<String> = sonde_proxy::trial::choose(&s.records)
            .into_iter()
            .map(|x| x.name)
            .collect();
        let names: Vec<String> = plan.scenarios.iter().map(|x| x.name.clone()).collect();
        assert_eq!(names, chosen);
        assert!(names.contains(&"lost-answer".to_string()), "{names:?}");
        assert!(!plan.project);
        assert_eq!(plan.command, "python job.py --out s3://b/x");
        assert!(
            plan.shares_place,
            "no {{trial}}: every run writes one place"
        );
        // by name: of the plan's or built in; an unknown one is refused
        let one = scenarios_named(&plan, &["error".into()]).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(scenarios_named(&plan, &[]).unwrap().len(), names.len());
        assert!(scenarios_named(&plan, &["nope".into()]).is_err());
        // pressed harder: the two that fail a request's first attempt, by a count
        let hard = scenarios_named(&plan, &["slowdown:3".into(), "error".into()]).unwrap();
        assert_eq!(
            (hard[0].name.as_str(), hard[0].rules[0].times),
            ("slowdown", 3)
        );
        assert_eq!(
            hard[0].about,
            "503 SlowDown on each request's first 3 attempts"
        );
        assert_eq!(hard[1].rules[0].times, 1);
        for (spec, says) in [
            ("slowdown:0", "from 1 to 5"),
            ("slowdown:9", "from 1 to 5"),
            ("slowdown:x", "from 1 to 5"),
            ("cut-body:2", "only slowdown and error"),
            ("nope:2", "no such scenario"),
        ] {
            let e = scenarios_named(&plan, &[spec.into()]).unwrap_err();
            assert!(e.contains(says), "{spec}: {e}");
        }
        // a trial is not tried again
        let t = Session::open(Path::new(&trial)).unwrap();
        assert!(trial_plan_of(&t).unwrap_err().contains("a trial itself"));
    }

    #[test]
    fn a_command_with_a_place_of_its_own_per_run_does_not_share_it() {
        for command in [
            "python job.py --out s3://b/{trial}/",
            "sh -c 'python job.py --out s3://b/$SONDE_TRIAL/'",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (clean, _, _) = run_and_trial(dir.path(), command, None);
            let plan = trial_plan_of(&Session::open(Path::new(&clean)).unwrap()).unwrap();
            assert!(!plan.shares_place, "{command}");
            // the session keeps the command as written, to run it again
            assert_eq!(plan.command, command);
        }
    }

    /// The bridge's own path for a large session, timed: what a window
    /// asks for when it opens one (`view`), again (kept), a later page of
    /// its operations, and the size of what crosses to the interface.
    /// `SONDE_BIG_SESSION=<file> cargo test -p sonde-nsh -- --ignored
    /// a_large_session --nocapture`.
    #[test]
    #[ignore = "needs a large session: SONDE_BIG_SESSION names it"]
    fn a_large_session_through_the_bridge_is_timed() {
        let Ok(path) = std::env::var("SONDE_BIG_SESSION") else {
            return;
        };
        let t = std::time::Instant::now();
        let view = view_of(&path).unwrap();
        let first = t.elapsed();
        let bytes = view.to_string().len();
        let t = std::time::Instant::now();
        let again = view_of(&path).unwrap();
        let second = t.elapsed();
        let total = again["operationsTotal"].as_u64().unwrap();
        let q = query_of(json!({ "offset": total / 2, "limit": 200 })).unwrap();
        let t = std::time::Instant::now();
        let page = analysis_of(&path).unwrap().1.page(&q);
        let paged = t.elapsed();
        println!(
            "{} requests, {total} operations: first view {first:?} ({} KiB of JSON), again {second:?}, a page from the middle {paged:?} ({} KiB)",
            view["summary"]["requests"],
            bytes / 1024,
            page.to_string().len() / 1024
        );
    }

    #[test]
    fn a_trial_runs_the_command_as_written_with_only_its_place_changed() {
        // several steps, a pipe, both quotes, an escape, a line break
        let command = "cd etl; A='x y' python load.py \"s3://b/{trial}/in put\" | tee log-{trial}.txt && curl -s $AWS_ENDPOINT_URL/demo/missing.txt\ncurl -s x\\ y || true";
        let dir = tempfile::tempdir().unwrap();
        let (clean, _, _) = run_and_trial(dir.path(), command, None);
        let plan = trial_plan_of(&Session::open(Path::new(&clean)).unwrap()).unwrap();
        assert_eq!(plan.command, command, "kept as written");
        assert_eq!(
            command_under(&plan.command, Some("slowdown")),
            command.replace("{trial}", "slowdown")
        );
        assert_eq!(
            command_under(&plan.command, None),
            command.replace("{trial}", "clean")
        );
        // a shell given it runs each step: no separator is lost between two
        #[cfg(unix)]
        {
            let out = std::process::Command::new("sh")
                .args([
                    "-c",
                    &command_under("echo a-{trial}; echo 'b c' && echo \"d\" | cat", Some("x")),
                ])
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), "a-x\nb c\nd\n");
        }
    }

    #[test]
    fn a_projects_budget_is_read_for_its_charts_from_the_nearest_sonde_toml() {
        let project = tempfile::tempdir().unwrap();
        let inner = project.path().join("jobs");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        // none without a file, or without the three
        assert_eq!(super::project_budget(&inner), None);
        std::fs::write(
            project.path().join("sonde.toml"),
            "[check]\nmax-requests = 9\n",
        )
        .unwrap();
        assert_eq!(super::project_budget(&inner), None);
        std::fs::write(
            project.path().join("sonde.toml"),
            "[check]\nmax-p95 = \"50ms\"\nmax-errors = 3\nmax-waiting = \"2s\"\n",
        )
        .unwrap();
        assert_eq!(
            super::project_budget(&inner),
            Some((Some(50.0), Some(3), Some(2000.0)))
        );
        std::fs::write(
            project.path().join("sonde.toml"),
            "[check]\nmax-p95 = 120\n",
        )
        .unwrap();
        assert_eq!(
            super::project_budget(&inner),
            Some((Some(120.0), None, None))
        );
    }

    #[test]
    fn a_projects_own_scenarios_are_the_plan_when_its_sonde_toml_names_some() {
        let project = tempfile::tempdir().unwrap();
        let sub = project.path().join("etl");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        std::fs::write(
            project.path().join("sonde.toml"),
            "[check]\nmax-requests = 10\n\n[[scenario]]\nname = \"flaky-puts\"\nfault = \"503 SlowDown\"\non = [\"PutObject\"]\nprefix = \"out/\"\ntimes = 2\n\n[[scenario]]\nname = \"lag\"\nfault = \"delay 200ms\"\n",
        )
        .unwrap();
        // found from the folder the command ran in, upwards
        let found = project_scenarios(&sub).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "flaky-puts");
        assert_eq!(found[0].about, "503 SlowDown on PutObject under out/");
        assert_eq!(found[0].rules[0].times, 2);
        assert_eq!(found[1].rules[0].times, 0, "a delay meets every attempt");
        let dir = tempfile::tempdir().unwrap();
        let (clean, _, _) = run_and_trial(dir.path(), "python job.py", Some(&sub));
        let plan = trial_plan_of(&Session::open(Path::new(&clean)).unwrap()).unwrap();
        assert!(plan.project);
        let names: Vec<_> = plan.scenarios.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["flaky-puts", "lag"]);
        // (its own times are sonde.toml's: not asked for here)
        assert!(
            scenarios_named(&plan, &["flaky-puts:3".into()])
                .unwrap_err()
                .contains("the project's own")
        );
        // a built-in one can still be asked for by name
        assert_eq!(
            scenarios_named(&plan, &["flaky-puts".into(), "error".into()])
                .unwrap()
                .len(),
            2
        );
        // what the command line refuses is refused here: one reading
        for (toml, says) in [
            (
                "[[scenario]]\nname = \"clean\"\nfault = \"500\"\n",
                "run without faults",
            ),
            (
                "[[scenario]]\nname = \"slow\"\nfault = \"500\"\n",
                "one of sonde's own",
            ),
            ("[program]\nname = \"\"\n", "a program's name"),
        ] {
            std::fs::write(project.path().join("sonde.toml"), toml).unwrap();
            let e = project_scenarios(&sub).unwrap_err();
            assert!(e.contains(says), "{e}");
        }
        // a scenario that does not read is said, not skipped
        std::fs::write(
            project.path().join("sonde.toml"),
            "[[scenario]]\nname = \"bad name\"\nfault = \"500\"\n",
        )
        .unwrap();
        let e = project_scenarios(&sub).unwrap_err();
        assert!(e.contains("a name is letters, digits"), "{e}");
        std::fs::write(
            project.path().join("sonde.toml"),
            "[[scenario]]\nname = \"x\"\nfault = \"500\"\nwhen = 1\n",
        )
        .unwrap();
        assert!(
            project_scenarios(&sub)
                .unwrap_err()
                .contains("unknown field `when`")
        );
        // and no file above the repository's root is read
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("repo/.git")).unwrap();
        std::fs::write(outside.path().join("sonde.toml"), "[[scenario]]\n").unwrap();
        assert!(
            project_scenarios(&outside.path().join("repo"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_runs_exit_code_is_kept_in_the_index_for_a_later_trial() {
        let dir = tempfile::tempdir().unwrap();
        let (clean, _, _) = run_and_trial(dir.path(), "python job.py", None);
        let p = Path::new(&clean);
        assert_eq!(read_exit(p), None, "not kept: a run of the CLI's");
        write_exit(p, None);
        assert_eq!(read_exit(p), None, "a killed program has no code to keep");
        write_exit(p, Some(3));
        assert_eq!(read_exit(p), Some(3));
    }

    #[test]
    fn a_trials_progress_names_its_scenario_and_ends_with_the_verdict() {
        let dir = tempfile::tempdir().unwrap();
        let (_, _, verdict) = run_and_trial(dir.path(), "python job.py", None);
        assert_eq!(
            trial_event("slowdown", None),
            json!({ "scenario": "slowdown", "state": "running" })
        );
        let done = trial_event("slowdown", Some(&verdict));
        assert_eq!(done["state"], "done");
        assert_eq!(done["trial"]["scenario"], "slowdown");
        assert_eq!(done["trial"]["standing"], to_json(verdict.standing));
        assert!(done["trial"]["session"].is_string());
    }

    #[test]
    fn a_programs_command_is_its_newest_runs() {
        let mut n = Newest::default();
        n.insert(
            "k".into(),
            ("2026-01-02".into(), Some("job --day 2".into())),
        );
        n.insert(
            "k".into(),
            ("2026-01-01".into(), Some("job --day 1".into())),
        );
        n.insert(
            "k".into(),
            ("2026-01-03".into(), Some("job --day 3".into())),
        );
        assert_eq!(n.get("k").as_deref(), Some("job --day 3"));
        assert_eq!(n.get("other"), None);
    }
}
