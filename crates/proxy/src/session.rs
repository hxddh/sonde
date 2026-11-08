//! Session files (`.sonde`): one JSON header line, then one redacted record
//! per line. Written as requests happen, so a crash keeps what came before.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
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
}

/// A session's header alone, from its first line (compressed or not).
pub fn read_header(path: &Path) -> Option<SessionHeader> {
    let mut first = String::new();
    reader(path).ok()?.read_line(&mut first).ok()?;
    serde_json::from_str(&first).ok()
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
        }
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

/// A session file's bytes as lines: plain, or gzip (a finished session is
/// compressed: [`compress`]).
pub fn reader(path: &Path) -> std::io::Result<Box<dyn BufRead>> {
    let mut f = File::open(path)?;
    let mut magic = [0u8; 2];
    let n = f.read(&mut magic)?;
    f.seek(SeekFrom::Start(0))?;
    Ok(if n == 2 && magic == [0x1f, 0x8b] {
        Box::new(BufReader::new(flate2::read::MultiGzDecoder::new(f)))
    } else {
        Box::new(BufReader::new(f))
    })
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
/// not compressed: it is still being appended to.
pub fn compress(path: &Path) -> std::io::Result<()> {
    if is_compressed(path) {
        return Ok(());
    }
    let tmp = path.with_extension("sonde.gz.tmp");
    {
        let mut src = BufReader::new(File::open(path)?);
        let out = File::create(&tmp)?;
        let mut gz =
            flate2::write::GzEncoder::new(BufWriter::new(out), flate2::Compression::new(6));
        std::io::copy(&mut src, &mut gz)?;
        gz.finish()?.flush()?;
    }
    std::fs::rename(&tmp, path)
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
        let reader = reader(path).map_err(|e| format!("{}: {e}", path.display()))?;
        // bytes, not text: a line torn mid-character is still a torn line
        let mut lines = reader.split(b'\n').map(|l| {
            l.map(|b| {
                String::from_utf8_lossy(&b)
                    .trim_end_matches('\r')
                    .to_string()
            })
        });
        let first = lines
            .next()
            .ok_or("empty session file")?
            .map_err(|e| e.to_string())?;
        let header: SessionHeader =
            serde_json::from_str(&first).map_err(|e| format!("not a sonde session: {e}"))?;
        if header.format != FORMAT {
            return Err(format!("not a sonde session (format {:?})", header.format));
        }
        if header.version > VERSION {
            return Err(format!(
                "session version {} is newer than this sonde reads ({VERSION})",
                header.version
            ));
        }
        let lines: Vec<String> = lines.collect::<Result<_, _>>().map_err(|e| e.to_string())?;
        let mut records = Vec::with_capacity(lines.len());
        let mut logs = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if l.trim().is_empty() {
                continue;
            }
            if l.starts_with("{\"log\":") {
                match serde_json::from_str::<LogLine>(l) {
                    Ok(x) => logs.push(x.log),
                    Err(_) if i + 1 == lines.len() => break,
                    Err(e) => return Err(format!("line {}: {e}", i + 2)),
                }
                continue;
            }
            match serde_json::from_str::<Record>(l) {
                Ok(r) => records.push(r),
                Err(_) if i + 1 == lines.len() => break,
                Err(e) => return Err(format!("line {}: {e}", i + 2)),
            }
        }
        // ids name records everywhere (operations, findings, the inspector): a
        // file joined from two recordings repeats them, so number them afresh
        let mut seen = std::collections::HashSet::with_capacity(records.len());
        if !records.iter().all(|r| seen.insert(r.id)) {
            for (i, r) in records.iter_mut().enumerate() {
                r.id = i as u64 + 1;
            }
        }
        Ok(Session {
            header,
            records,
            logs,
        })
    }
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
}
