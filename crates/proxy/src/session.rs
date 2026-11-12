//! Session files (`.sonde`): one JSON header line, then one redacted record
//! per line. Written as requests happen, so a crash keeps what came before.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use reqlog::{Record, Sink};
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "sonde-session";
pub const VERSION: u32 = 1;

/// The first line of a session file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    pub format: String,
    pub version: u32,
    pub created: DateTime<Utc>,
    /// The endpoint the requests went to (its id and URL, never credentials).
    pub endpoint: String,
    pub url: String,
    pub region: String,
    /// The recorded command line, when sonde started it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// The program's name, when it was given one (`sonde.toml`'s
    /// `[program] name`, or `--name`): its runs and its baseline go by it,
    /// whatever its arguments were this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The folder the command ran in: running it again runs it there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// The commit the program's code was at, when that folder is a git
    /// repository (or CI says): a run is tied to the change that made it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitStamp>,
    /// The fault scenario the run was put through (a trial), and the file
    /// name of the clean run it is a trial of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trial_of: Option<String>,
    /// Its requests' times are to the second (an imported log's): where in
    /// its second each began is not known. They are spread over it in the
    /// log's order, and what depends on it (how many were in flight at
    /// once, which waited for which) is not said of such a session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub whole_seconds: bool,
}

/// The commit a run's code was at.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitStamp {
    pub commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Changes not committed yet: the run is of more than the commit.
    #[serde(default)]
    pub dirty: bool,
}

/// The commit `dir` is at: git's own answer, or CI's (`GITHUB_SHA`) where
/// git is not there to ask.
pub fn git_stamp(dir: &Path) -> Option<GitStamp> {
    let git = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    if let Some(commit) = git(&["rev-parse", "HEAD"]).filter(|c| c.len() >= 7) {
        let branch =
            git(&["rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| !b.is_empty() && b != "HEAD");
        let dirty =
            git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
        return Some(GitStamp {
            commit,
            branch,
            dirty,
        });
    }
    let commit = std::env::var("GITHUB_SHA").ok().filter(|c| c.len() >= 7)?;
    let branch = ["GITHUB_HEAD_REF", "GITHUB_REF_NAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|b| !b.is_empty()));
    Some(GitStamp {
        commit,
        branch,
        dirty: false,
    })
}

/// A session's header alone, from its first line (compressed or not).
pub fn read_header(path: &Path) -> Option<SessionHeader> {
    let mut lines = Lines::open(path).ok()?;
    serde_json::from_slice(lines.line().ok()??).ok()
}

/// A session's first line as it is written (what tells one recording from
/// another written in its place).
pub fn header_line(path: &Path) -> Option<String> {
    let mut lines = Lines::open(path).ok()?;
    Some(String::from_utf8_lossy(lines.line().ok()??).into_owned())
}

impl SessionHeader {
    pub fn new(endpoint: &str, url: &str, region: &str, command: Option<String>) -> Self {
        SessionHeader {
            format: FORMAT.into(),
            version: VERSION,
            created: Utc::now(),
            endpoint: endpoint.into(),
            url: url.into(),
            region: region.into(),
            command,
            name: None,
            cwd: None,
            git: None,
            scenario: None,
            trial_of: None,
            whole_seconds: false,
        }
    }

    /// Whether the session was recorded from a program's own requests
    /// (`sonde run`, `proxy`, `watch`): not a replay, an import or a trial.
    /// "The newest session" is the newest of these.
    pub fn is_recording(&self) -> bool {
        self.scenario.is_none()
            && self.trial_of.is_none()
            && self
                .command
                .as_deref()
                .is_none_or(|c| !c.starts_with("replay of ") && !c.starts_with("imported from "))
    }

    /// The folder the program runs in, and the commit its code is at there.
    pub fn ran_in(mut self, dir: &Path) -> Self {
        self.cwd = Some(dir.to_string_lossy().into_owned());
        self.git = git_stamp(dir);
        self
    }
}

/// A chunk of the recorded program's output (terminal text, escape codes
/// included), when it arrived.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogChunk {
    pub time: DateTime<Utc>,
    pub text: String,
}

/// A log line in a session file: `{"log": {"time", "text"}}`, beside the
/// records.
#[derive(Serialize, Deserialize)]
struct LogLine {
    log: LogChunk,
}

/// Appends records to a session file.
pub struct SessionWriter {
    out: Mutex<BufWriter<File>>,
}

/// A path for a new session file in `dir` named `<stem>.sonde`, or
/// `<stem>-2.sonde`, `<stem>-3.sonde`… when that one exists: two sessions
/// started in the same second do not share a file.
pub fn fresh_path(dir: &Path, stem: &str) -> std::path::PathBuf {
    let mut path = dir.join(format!("{stem}.sonde"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.sonde"));
        n += 1;
    }
    path
}

impl SessionWriter {
    pub fn create(path: &Path, header: &SessionHeader) -> std::io::Result<SessionWriter> {
        let mut out = BufWriter::new(File::create(path)?);
        serde_json::to_writer(&mut out, header)?;
        out.write_all(b"\n")?;
        out.flush()?;
        Ok(SessionWriter {
            out: Mutex::new(out),
        })
    }

    /// Appends a chunk of the program's output.
    pub fn write_log(&self, text: &str) {
        let mut out = self.out.lock();
        let line = LogLine {
            log: LogChunk {
                time: Utc::now(),
                text: text.to_string(),
            },
        };
        if serde_json::to_writer(&mut *out, &line).is_ok() {
            let _ = out.write_all(b"\n");
            let _ = out.flush();
        }
    }

    pub fn flush(&self) -> std::io::Result<()> {
        self.out.lock().flush()
    }
}

impl Sink for SessionWriter {
    fn write(&self, record: &Record) {
        let mut out = self.out.lock();
        if serde_json::to_writer(&mut *out, record).is_ok() {
            let _ = out.write_all(b"\n");
            let _ = out.flush();
        }
    }
}

/// The longest line a session file may hold: a record with the largest
/// bodies kept is a few MiB of JSON; past this it is not a session's line.
pub const MAX_LINE: usize = 32 << 20;

/// The most text a session file may hold once decompressed: a few bytes of
/// gzip can say they are any size. Sessions are read as a stream, so this is
/// far above any recorded run (about forty million requests).
pub const MAX_TEXT: u64 = 64 << 30;

/// How much is read at a time.
const CHUNK: usize = 1 << 20;

/// A session file's lines, plain or gzip (a finished session is compressed:
/// [`compress`]), read as a stream: a line at a time or a block of whole
/// lines at a time, never the file whole. A line past [`MAX_LINE`] and a file
/// past [`MAX_TEXT`] are errors, not memory.
pub struct Lines {
    from: Box<dyn Read + Send>,
    buf: Vec<u8>,
    start: usize,
    end: usize,
    done: bool,
    read: u64,
    /// Compressed: a stream that stops short is read up to where it stops.
    gz: bool,
    cut: Option<String>,
}

impl Lines {
    pub fn open(path: &Path) -> std::io::Result<Lines> {
        let mut f = File::open(path)?;
        let mut magic = [0u8; 2];
        let n = f.read(&mut magic)?;
        f.seek(SeekFrom::Start(0))?;
        let gz = n == 2 && magic == [0x1f, 0x8b];
        let from: Box<dyn Read + Send> = if gz {
            Box::new(flate2::read::MultiGzDecoder::new(BufReader::with_capacity(
                1 << 16,
                f,
            )))
        } else {
            Box::new(f)
        };
        Ok(Lines {
            gz,
            ..Lines::of(from)
        })
    }

    /// The lines of any reader (already decompressed).
    pub fn of(from: Box<dyn Read + Send>) -> Lines {
        Lines {
            from,
            buf: vec![0; CHUNK],
            start: 0,
            end: 0,
            done: false,
            read: 0,
            gz: false,
            cut: None,
        }
    }

    /// Why a compressed file's text ended before its end (a partial copy, a
    /// disk that filled while it was written), when it did: what came
    /// before was read, its last line torn as a killed recorder's is.
    pub fn cut(&self) -> Option<&str> {
        self.cut.as_deref()
    }

    /// Reads more into the buffer, making room first. False at the end.
    fn fill(&mut self) -> std::io::Result<bool> {
        if self.done {
            return Ok(false);
        }
        if self.start > 0 {
            self.buf.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
        if self.end == self.buf.len() {
            // one line fills the buffer: it may grow, to a point
            if self.buf.len() >= MAX_LINE {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("a line longer than {} MiB: not a session's", MAX_LINE >> 20),
                ));
            }
            self.buf.resize(self.buf.len() * 2, 0);
        }
        let n = loop {
            match self.from.read(&mut self.buf[self.end..]) {
                Ok(n) => break n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) if self.gz => {
                    self.cut = Some(e.to_string());
                    break 0;
                }
                Err(e) => return Err(e),
            }
        };
        self.end += n;
        self.read += n as u64;
        if self.read > MAX_TEXT {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("it holds more than {} GiB", MAX_TEXT >> 30),
            ));
        }
        self.done = n == 0;
        Ok(n > 0)
    }

    /// The next line without its newline; `None` at the end. The last line
    /// need not end with one.
    pub fn line(&mut self) -> std::io::Result<Option<&[u8]>> {
        let mut searched = self.start;
        loop {
            if let Some(i) = self.buf[searched..self.end]
                .iter()
                .position(|&b| b == b'\n')
            {
                let (a, b) = (self.start, searched + i);
                self.start = b + 1;
                return Ok(Some(&self.buf[a..b]));
            }
            let pending = self.end - self.start;
            if !self.fill()? {
                if pending == 0 {
                    return Ok(None);
                }
                let (a, b) = (self.start, self.end);
                self.start = self.end;
                return Ok(Some(&self.buf[a..b]));
            }
            searched = self.start + pending;
        }
    }

    /// The next block of whole lines (about a MiB of them), its last newline
    /// left out; `None` at the end.
    pub fn block(&mut self) -> std::io::Result<Option<&[u8]>> {
        loop {
            // as much as the buffer takes, then cut at the last newline
            while self.end < self.buf.len() || self.start > 0 {
                if !self.fill()? {
                    break;
                }
            }
            let have = &self.buf[self.start..self.end];
            if have.is_empty() {
                return Ok(None);
            }
            if let Some(i) = have.iter().rposition(|&b| b == b'\n') {
                let (a, b) = (self.start, self.start + i);
                self.start = b + 1;
                return Ok(Some(&self.buf[a..b]));
            }
            if self.done {
                let (a, b) = (self.start, self.end);
                self.start = self.end;
                return Ok(Some(&self.buf[a..b]));
            }
            // one line longer than the buffer: grow it (or refuse it)
            self.fill()?;
        }
    }
}

/// Whether a session file is compressed.
pub fn is_compressed(path: &Path) -> bool {
    let mut magic = [0u8; 2];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok_and(|_| magic == [0x1f, 0x8b])
}

/// Compresses a finished session in place (gzip; a tenth of its size or
/// less): it stays a `.sonde` file every reader opens. A live session is
/// not compressed: it is still being appended to. A session of the sessions
/// folder is counted as it goes by, and its counts kept beside it
/// ([`crate::summary::Counted`]): listing it never reads it again.
pub fn compress(path: &Path) -> std::io::Result<()> {
    if is_compressed(path) {
        return Ok(());
    }
    let tmp = path.with_extension("sonde.gz.tmp");
    // (a session written elsewhere, with -o, gets no file beside it)
    let listed = path
        .parent()
        .and_then(|d| d.file_name())
        .is_some_and(|n| n == "sessions");
    let mut tally = listed.then(Tally::default);
    {
        let mut src = File::open(path)?;
        let out = File::create(&tmp)?;
        let mut gz =
            flate2::write::GzEncoder::new(BufWriter::new(out), flate2::Compression::new(6));
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = match src.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            gz.write_all(&buf[..n])?;
            if let Some(t) = &mut tally {
                t.feed(&buf[..n]);
            }
        }
        gz.finish()?.flush()?;
    }
    std::fs::rename(&tmp, path)?;
    if let Some((header, counter)) = tally.and_then(Tally::finish) {
        // (the file is ours, just written: its stamp is of what was counted)
        let at = crate::summary::stamp(path);
        crate::summary::keep(path, &counter.finish(at, &header));
    }
    Ok(())
}

/// A session's bytes going by, cut into lines and counted.
#[derive(Default)]
struct Tally {
    line: Vec<u8>,
    header: Option<SessionHeader>,
    counter: crate::summary::Counter,
    /// Not a session (no header, or a line past any record's length): not counted.
    not: bool,
    lines: usize,
}

impl Tally {
    fn feed(&mut self, mut bytes: &[u8]) {
        while !self.not && !bytes.is_empty() {
            match bytes.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    if self.line.is_empty() {
                        self.take(&bytes[..i]);
                    } else {
                        self.line.extend_from_slice(&bytes[..i]);
                        let line = std::mem::take(&mut self.line);
                        self.take(&line);
                    }
                    bytes = &bytes[i + 1..];
                }
                None => {
                    self.line.extend_from_slice(bytes);
                    self.not = self.line.len() > MAX_LINE;
                    return;
                }
            }
        }
    }

    fn take(&mut self, line: &[u8]) {
        self.lines += 1;
        if self.lines == 1 {
            self.header = serde_json::from_slice(line).ok();
            self.not = self.header.is_none();
        } else {
            self.counter.line(line);
        }
    }

    fn finish(mut self) -> Option<(SessionHeader, crate::summary::Counter)> {
        if !self.line.is_empty() && !self.not {
            let line = std::mem::take(&mut self.line);
            self.take(&line);
        }
        (!self.not).then_some(())?;
        Some((self.header?, self.counter))
    }
}

/// A session read back.
#[derive(Debug, Clone)]
pub struct Session {
    pub header: SessionHeader,
    pub records: Vec<Record>,
    /// The recorded program's output, when sonde ran it in a terminal.
    pub logs: Vec<LogChunk>,
}

/// Where a growing (uncompressed) session file's complete lines end: what
/// [`Session::read_more`] starts from next time.
pub fn complete_len(path: &Path) -> std::io::Result<u64> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let back = len.min(1 << 20);
    f.seek(SeekFrom::Start(len - back))?;
    let mut tail = Vec::with_capacity(back as usize);
    f.read_to_end(&mut tail)?;
    Ok(match tail.iter().rposition(|&b| b == b'\n') {
        Some(i) => len - back + i as u64 + 1,
        None => len - back,
    })
}

impl Session {
    /// Appends what a live recording wrote after `from` (a byte offset at a
    /// line's start, from [`complete_len`] or an earlier call): its complete
    /// lines only, and of those only what is newer than what the session
    /// holds, so `from` may lag behind it. Returns where the next call
    /// starts. Not for compressed files.
    pub fn read_more(&mut self, path: &Path, from: u64) -> std::io::Result<u64> {
        let mut f = std::fs::File::open(path)?;
        f.seek(SeekFrom::Start(from))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        let Some(end) = buf.iter().rposition(|&b| b == b'\n') else {
            return Ok(from);
        };
        // concurrent requests are written as they finish, not in id order: a
        // record a little older than the newest may still arrive
        const WINDOW: u64 = 4096;
        let last_id = self.records.iter().map(|r| r.id).max().unwrap_or(0);
        let floor = last_id.saturating_sub(WINDOW);
        let recent: std::collections::HashSet<u64> = self
            .records
            .iter()
            .rev()
            .take(WINDOW as usize * 2)
            .map(|r| r.id)
            .filter(|id| *id > floor)
            .collect();
        let last_log = self.logs.last().map(|l| l.time);
        for line in buf[..end].split(|&b| b == b'\n') {
            let l = String::from_utf8_lossy(line);
            let l = l.trim_end_matches('\r');
            if l.trim().is_empty() {
                continue;
            }
            if l.starts_with("{\"log\":") {
                if let Ok(x) = serde_json::from_str::<LogLine>(l)
                    && last_log.is_none_or(|t| x.log.time > t)
                {
                    self.logs.push(x.log);
                }
            } else if let Ok(r) = serde_json::from_str::<Record>(l)
                && r.id > floor
                && !recent.contains(&r.id)
            {
                self.records.push(r);
            }
        }
        Ok(from + end as u64 + 1)
    }

    /// Reads a session file. A torn last line (the recorder was killed) is
    /// skipped; any other bad line is an error.
    pub fn open(path: &Path) -> Result<Session, String> {
        Session::read(path, Keep::All)
    }

    /// [`Session::open`] for reading what the session amounts to (its
    /// operations, findings, time, cost): what no analysis reads of a record
    /// is not kept ([`Keep::Analysis`]), which is most of its size. Not for
    /// showing a request whole, replaying it or writing it out.
    pub fn open_lean(path: &Path) -> Result<Session, String> {
        Session::read(path, Keep::Analysis)
    }

    /// Reads a session file as a stream: the file is never held as text, its
    /// lines are parsed as they are decompressed, on the cores there are.
    pub fn read(path: &Path, keep: Keep) -> Result<Session, String> {
        Session::read_said(path, keep)
            .map(|r| r.session)
            .map_err(|e| e.to_string())
    }

    /// [`Session::read`], saying what it could not read: a file that is no
    /// session apart from one that could not be read at all, and a
    /// compressed file cut short (read up to its cut) apart from a whole one.
    pub fn read_said(path: &Path, keep: Keep) -> Result<Opened, ReadError> {
        let io = |e: std::io::Error| match e.kind() {
            std::io::ErrorKind::InvalidData => ReadError::Bad(e.to_string()),
            _ => ReadError::Io(format!("{}: {e}", path.display())),
        };
        let mut lines = Lines::open(path).map_err(io)?;
        let first = lines
            .line()
            .map_err(io)?
            .ok_or(ReadError::Bad("empty session file".into()))?;
        let first = String::from_utf8_lossy(first);
        let header: SessionHeader = serde_json::from_str(first.trim_end_matches('\r'))
            .map_err(|e| ReadError::Bad(format!("not a sonde session: {e}")))?;
        if header.format != FORMAT {
            return Err(ReadError::Bad(format!(
                "not a sonde session (format {:?})",
                header.format
            )));
        }
        if header.version > VERSION {
            return Err(ReadError::Bad(format!(
                "session version {} is newer than this sonde reads ({VERSION})",
                header.version
            )));
        }
        let small = std::fs::metadata(path).is_ok_and(|m| m.len() < 256 * 1024);
        let workers = if small {
            1
        } else {
            std::thread::available_parallelism().map_or(1, |n| n.get().min(8))
        };
        let mut all = Gathered::default();
        let cut = if workers == 1 {
            while let Some(b) = lines.block().map_err(io)? {
                all.add(parse_block(b, keep)).map_err(ReadError::Bad)?;
            }
            lines.cut().map(String::from)
        } else {
            read_blocks(lines, keep, workers, &mut all)?
        };
        let Gathered {
            mut records, logs, ..
        } = all;
        // ids name records everywhere (operations, findings, the inspector): a
        // file joined from two recordings repeats them, so number them afresh
        let mut seen = std::collections::HashSet::with_capacity(records.len());
        if !records.iter().all(|r| seen.insert(r.id)) {
            for (i, r) in records.iter_mut().enumerate() {
                r.id = i as u64 + 1;
            }
        }
        let cut = cut.map(|why| {
            format!(
                "its compressed text stops short ({why}): read up to there, {} requests",
                records.len()
            )
        });
        Ok(Opened {
            session: Session {
                header,
                records,
                logs,
            },
            cut,
        })
    }
}

/// A session read, and what of its file was not.
pub struct Opened {
    pub session: Session,
    /// The file is compressed and its text stops short of its end: what is
    /// read is what came before (said, not refused, as a torn last line is).
    pub cut: Option<String>,
}

/// Why a session file was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// It could not be read (not there, not allowed, a disk's error).
    Io(String),
    /// It was read, and it is not a session: no header, a line no record
    /// could be in the middle of it, a newer format.
    Bad(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Io(e) | ReadError::Bad(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for ReadError {}

/// What of each record a reading keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Everything the file has.
    All,
    /// What the analyses read: the URL (the bucket, key and query are kept
    /// apart), the store's ids and the headers no finding looks at are
    /// dropped. A session of a million requests is then a third of the size.
    Analysis,
}

/// Headers no analysis reads, and most of a record's size: signing, the
/// connection, ids the record has as fields of its own.
const UNREAD_HEADERS: &[&str] = &[
    "accept",
    "accept-encoding",
    "accept-ranges",
    "amz-sdk-invocation-id",
    "amz-sdk-request",
    "authorization",
    "cache-control",
    "connection",
    "content-encoding",
    "content-md5",
    "content-type",
    "date",
    "expect",
    "host",
    "keep-alive",
    "last-modified",
    "server",
    "transfer-encoding",
    "vary",
    "x-amz-content-sha256",
    "x-amz-date",
    "x-amz-decoded-content-length",
    "x-amz-id-2",
    "x-amz-request-id",
    "x-amz-sdk-checksum-algorithm",
    "x-amz-security-token",
    "x-amz-server-side-encryption",
    "x-amz-trailer",
    "x-amz-user-agent",
];

/// A record as [`Keep::Analysis`] keeps it.
pub fn lean(r: &mut Record) {
    let unread = |h: &reqlog::Header| {
        UNREAD_HEADERS
            .iter()
            .any(|n| h.name.eq_ignore_ascii_case(n))
    };
    r.request_headers.retain(|h| !unread(h));
    r.response_headers.retain(|h| !unread(h));
    r.request_headers.shrink_to_fit();
    r.response_headers.shrink_to_fit();
    r.query.shrink_to_fit();
    r.url = String::new();
    r.host_id = None;
}

/// A block of a session's lines, read.
struct Block {
    records: Vec<Record>,
    logs: Vec<LogChunk>,
    /// How many lines it had, and the first that could not be read (its
    /// place in the block, and why): the lines after it are not read.
    lines: usize,
    bad: Option<(usize, String)>,
}

fn parse_block(bytes: &[u8], keep: Keep) -> Block {
    let mut out = Block {
        records: Vec::new(),
        logs: Vec::new(),
        lines: 0,
        bad: None,
    };
    for (i, raw) in bytes.split(|&b| b == b'\n').enumerate() {
        out.lines = i + 1;
        if out.bad.is_some() {
            continue;
        }
        // bytes, not text: a line torn mid-character is still a torn line
        let text;
        let l = match std::str::from_utf8(raw) {
            Ok(l) => l,
            Err(_) => {
                text = String::from_utf8_lossy(raw);
                &*text
            }
        };
        let l = l.trim_end_matches('\r');
        if l.trim().is_empty() {
            continue;
        }
        if l.starts_with("{\"log\":") {
            match serde_json::from_str::<LogLine>(l) {
                Ok(x) => out.logs.push(x.log),
                Err(e) => out.bad = Some((i, e.to_string())),
            }
            continue;
        }
        match serde_json::from_str::<Record>(l) {
            Ok(mut r) => {
                if keep == Keep::Analysis {
                    lean(&mut r);
                }
                out.records.push(r)
            }
            Err(e) => out.bad = Some((i, e.to_string())),
        }
    }
    out.records.shrink_to_fit();
    out
}

/// A session's blocks put together, in the file's order.
#[derive(Default)]
struct Gathered {
    records: Vec<Record>,
    logs: Vec<LogChunk>,
    /// Lines read so far, and a bad last line of the block before: a torn
    /// last line of the file is the recorder killed mid-write, a bad line
    /// anywhere else is not a session.
    lines: usize,
    torn: Option<String>,
}

impl Gathered {
    fn add(&mut self, b: Block) -> Result<(), String> {
        if let Some(e) = self.torn.take() {
            return Err(e);
        }
        if let Some((at, e)) = b.bad {
            // (the header is line 1)
            let e = format!("line {}: {e}", self.lines + at + 2);
            if at + 1 < b.lines {
                return Err(e);
            }
            self.torn = Some(e);
        }
        self.lines += b.lines;
        self.records.extend(b.records);
        self.logs.extend(b.logs);
        Ok(())
    }
}

/// Reads a session's blocks on `workers` threads while another
/// decompresses, and gathers them here in the file's order.
fn read_blocks(
    mut lines: Lines,
    keep: Keep,
    workers: usize,
    all: &mut Gathered,
) -> Result<Option<String>, ReadError> {
    use std::sync::mpsc;
    let (work, jobs) = mpsc::sync_channel::<(usize, Vec<u8>)>(workers * 2);
    // the workers hold the jobs' end between them: when they are gone the
    // reader's next block has nowhere to go, and it stops
    let jobs = std::sync::Arc::new(std::sync::Mutex::new(jobs));
    let (done, results) = mpsc::sync_channel::<(usize, Block)>(workers * 2);
    std::thread::scope(|s| {
        for _ in 0..workers {
            let done = done.clone();
            let jobs = jobs.clone();
            s.spawn(move || {
                loop {
                    let job = jobs.lock().unwrap_or_else(|e| e.into_inner()).recv();
                    let Ok((seq, bytes)) = job else { break };
                    if done.send((seq, parse_block(&bytes, keep))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(done);
        drop(jobs);
        let reader = s.spawn(move || {
            let mut seq = 0;
            loop {
                match lines.block() {
                    Ok(Some(b)) => {
                        if work.send((seq, b.to_vec())).is_err() {
                            return Ok(None);
                        }
                        seq += 1;
                    }
                    Ok(None) => return Ok(lines.cut().map(String::from)),
                    Err(e) => return Err(e),
                }
            }
        });
        // blocks come back in any order: each waits for those before it
        let mut waiting: std::collections::BTreeMap<usize, Block> = Default::default();
        let mut next = 0;
        let mut gathered = Ok(());
        for (seq, block) in results {
            waiting.insert(seq, block);
            while let Some(b) = waiting.remove(&next) {
                next += 1;
                if gathered.is_ok() {
                    gathered = all.add(b);
                }
            }
            if gathered.is_err() {
                // stop reading: dropping the results ends the workers, and
                // they the reader
                break;
            }
        }
        let read = reader
            .join()
            .unwrap_or_else(|_| Err(std::io::Error::other("reading failed")));
        // a line no record could be names its place; failing that, the read
        gathered.map_err(ReadError::Bad)?;
        read.map_err(|e| match e.kind() {
            std::io::ErrorKind::InvalidData => ReadError::Bad(e.to_string()),
            _ => ReadError::Io(e.to_string()),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_torn_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sonde");
        let w = SessionWriter::create(
            &path,
            &SessionHeader::new("r2", "https://x.r2.dev", "auto", Some("app".into())),
        )
        .unwrap();
        for i in 0..3 {
            w.write(&Record {
                id: i,
                operation: "GetObject".into(),
                ..Default::default()
            });
        }
        w.write_log("hello\r\n");
        w.flush().unwrap();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(b"{\"id\": 9, \"oper").unwrap();
        let s = Session::open(&path).unwrap();
        assert_eq!(s.header.endpoint, "r2");
        assert_eq!(s.records.len(), 3);
        assert_eq!(s.records[2].id, 2);
        assert_eq!(s.logs.len(), 1);
        assert_eq!(s.logs[0].text, "hello\r\n");
    }

    /// A session of `n` requests with headers, written as a recording is.
    fn recorded(path: &Path, n: u64) {
        let w = SessionWriter::create(
            path,
            &SessionHeader::new("r2", "https://x.r2.dev", "auto", Some("app".into())),
        )
        .unwrap();
        for i in 0..n {
            w.write(&Record {
                id: i + 1,
                operation: "GetObject".into(),
                method: "GET".into(),
                url: format!("https://x.r2.dev/b/k{i}"),
                key: Some(format!("k{i}")),
                host_id: Some("host".into()),
                status: Some(200),
                request_headers: vec![
                    reqlog::Header::new("Authorization", "AWS4-HMAC-SHA256 Credential=…"),
                    reqlog::Header::new("user-agent", "aws-sdk-java/2.25"),
                    reqlog::Header::new("Range", "bytes=0-9"),
                ],
                response_headers: vec![
                    reqlog::Header::new("x-amz-request-id", format!("{i:016X}")),
                    reqlog::Header::new("ETag", "\"abc\""),
                ],
                ..Default::default()
            });
            if i % 500 == 0 {
                w.write_log(&format!("at {i}\n"));
            }
        }
        w.flush().unwrap();
    }

    #[test]
    fn a_large_session_read_in_blocks_on_several_threads_is_in_the_files_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.sonde");
        // past a block and past the size read on one thread
        recorded(&path, 6000);
        assert!(std::fs::metadata(&path).unwrap().len() > 2 * CHUNK as u64);
        for compressed in [false, true] {
            if compressed {
                compress(&path).unwrap();
            }
            let s = Session::open(&path).unwrap();
            assert_eq!(s.records.len(), 6000);
            assert!(
                s.records
                    .iter()
                    .enumerate()
                    .all(|(i, r)| r.id == i as u64 + 1)
            );
            assert_eq!(s.records[5999].key.as_deref(), Some("k5999"));
            assert_eq!(s.logs.len(), 12);
            assert_eq!(s.logs[11].text, "at 5500\n");
        }
    }

    #[test]
    fn a_torn_last_line_is_skipped_and_a_bad_line_elsewhere_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("torn.sonde");
        recorded(&path, 3000);
        let whole = std::fs::read(&path).unwrap();
        // the recorder killed mid-write: the last line has no end
        std::fs::write(&path, &whole[..whole.len() - 40]).unwrap();
        assert_eq!(Session::open(&path).unwrap().records.len(), 2999);
        // a line that is no record in the middle: not a session, and which line
        let mut bad = whole.clone();
        let at = bad
            .iter()
            .enumerate()
            .filter(|(_, b)| **b == b'\n')
            .nth(1500)
            .unwrap()
            .0;
        bad.splice(at + 1..at + 1, b"{\"id\": \"x\"}\n".iter().copied());
        std::fs::write(&path, &bad).unwrap();
        let e = Session::open(&path).unwrap_err();
        assert!(e.starts_with("line 1502: "), "{e}");
    }

    #[test]
    fn a_compressed_session_cut_short_is_read_up_to_its_cut_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cut.sonde");
        recorded(&path, 6000);
        compress(&path).unwrap();
        let whole = Session::read_said(&path, Keep::All).unwrap();
        assert_eq!((whole.session.records.len(), whole.cut), (6000, None));
        // a partial copy: the gzip stream stops in its middle
        let bytes = std::fs::read(&path).unwrap();
        for keep in [bytes.len() / 3, bytes.len() * 2 / 3, bytes.len() - 4] {
            std::fs::write(&path, &bytes[..keep]).unwrap();
            let r = Session::read_said(&path, Keep::Analysis).unwrap();
            let n = r.session.records.len();
            assert!(n > 0 && n <= 6000, "{n}");
            assert!(
                r.session
                    .records
                    .iter()
                    .enumerate()
                    .all(|(i, r)| r.id == i as u64 + 1)
            );
            let said = r.cut.expect("said");
            assert!(
                said.contains(&format!("read up to there, {n} requests")),
                "{said}"
            );
            // and every reader of it agrees: opened, counted
            assert_eq!(Session::open(&path).unwrap().records.len(), n);
            assert_eq!(crate::summary::counted(&path).unwrap().requests, n as u64);
        }
    }

    #[test]
    fn a_file_that_is_no_session_is_told_from_one_that_could_not_be_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.sonde");
        let bad = |e: Result<Opened, ReadError>| match e {
            Err(ReadError::Bad(why)) => why,
            Err(other) => panic!("{other:?}"),
            Ok(_) => panic!("read"),
        };
        assert!(matches!(
            Session::read_said(&path, Keep::All),
            Err(ReadError::Io(_))
        ));
        std::fs::write(&path, b"").unwrap();
        assert_eq!(
            bad(Session::read_said(&path, Keep::All)),
            "empty session file"
        );
        std::fs::write(&path, b"hello\n").unwrap();
        assert!(bad(Session::read_said(&path, Keep::All)).starts_with("not a sonde session"));
        recorded(&path, 10);
        let mut text = std::fs::read(&path).unwrap();
        let at = text.iter().position(|b| *b == b'\n').unwrap() + 1;
        text.splice(at..at, b"garbage\n".iter().copied());
        std::fs::write(&path, &text).unwrap();
        assert!(bad(Session::read_said(&path, Keep::All)).starts_with("line 2: "));
    }

    #[test]
    fn a_line_no_record_could_be_is_refused_with_a_message_not_read_into_memory() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absurd.sonde");
        // a small file that says it holds one line of 40 MiB
        let mut gz = flate2::write::GzEncoder::new(
            File::create(&path).unwrap(),
            flate2::Compression::fast(),
        );
        let header = SessionHeader::new("e", "https://e", "r", None);
        serde_json::to_writer(&mut gz, &header).unwrap();
        gz.write_all(b"\n{\"id\":1,\"url\":\"").unwrap();
        let filler = vec![b'a'; 1 << 20];
        for _ in 0..40 {
            gz.write_all(&filler).unwrap();
        }
        gz.finish().unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() < 1 << 20);
        let e = Session::open(&path).unwrap_err();
        assert!(e.contains("a line longer than 32 MiB"), "{e}");
        // the header alone still reads, and so does counting what is there
        assert_eq!(read_header(&path).unwrap().endpoint, "e");
        assert!(crate::summary::counted(&path).is_some_and(|c| c.requests == 0));
    }

    #[test]
    fn a_lean_reading_keeps_what_findings_read_and_not_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lean.sonde");
        recorded(&path, 3);
        let full = Session::open(&path).unwrap();
        let lean = Session::open_lean(&path).unwrap();
        let names = |r: &Record| -> Vec<String> {
            r.request_headers
                .iter()
                .chain(&r.response_headers)
                .map(|h| h.name.to_string())
                .collect()
        };
        assert_eq!(names(&full.records[0]).len(), 5);
        assert_eq!(names(&lean.records[0]), ["user-agent", "Range", "ETag"]);
        assert!(lean.records[0].url.is_empty() && lean.records[0].host_id.is_none());
        // and nothing an operation or a finding is made of
        let mut same = full.records[0].clone();
        super::lean(&mut same);
        assert_eq!(same, lean.records[0]);
        assert_eq!(
            crate::episode::episodes(&full.records),
            crate::episode::episodes(&lean.records)
        );
    }

    #[test]
    fn a_session_of_the_sessions_folder_is_counted_as_it_is_compressed() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("sessions");
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("run.sonde");
        recorded(&path, 40);
        compress(&path).unwrap();
        let kept = crate::summary::kept(&path).expect("counted while compressing");
        assert_eq!((kept.requests, kept.errors), (40, 0));
        assert_eq!(kept.shape.iter().sum::<u32>(), 40);
        assert_eq!(kept.program.as_deref(), Some("app"));
        // one written elsewhere (`-o`) gets no file beside it
        let other = data.path().join("out.sonde");
        recorded(&other, 2);
        compress(&other).unwrap();
        assert!(!crate::summary::counted_file(&other).exists());
    }
}
