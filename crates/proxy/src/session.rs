//! Session files (`.sonde`): one JSON header line, then one redacted record
//! per line. Written as requests happen, so a crash keeps what came before.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
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

/// A session read back.
#[derive(Debug, Clone)]
pub struct Session {
    pub header: SessionHeader,
    pub records: Vec<Record>,
    /// The recorded program's output, when sonde ran it in a terminal.
    pub logs: Vec<LogChunk>,
}

impl Session {
    /// Reads a session file. A torn last line (the recorder was killed) is
    /// skipped; any other bad line is an error.
    pub fn open(path: &Path) -> Result<Session, String> {
        let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        // bytes, not text: a line torn mid-character is still a torn line
        let mut lines = BufReader::new(file).split(b'\n').map(|l| {
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
