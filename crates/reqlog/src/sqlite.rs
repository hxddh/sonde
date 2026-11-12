//! Optional persistence of the request log in a local SQLite file.
//!
//! Records are written already redacted. Writes go through a channel to a
//! thread of their own, so pushing a record never waits for the disk.

use std::path::Path;
use std::sync::mpsc;
use std::thread;

use rusqlite::{Connection, params};

use crate::ring::Sink;
use crate::{Filter, Record};

pub struct SqliteSink {
    tx: mpsc::Sender<Record>,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS records (
    id INTEGER, time TEXT NOT NULL, endpoint TEXT NOT NULL, operation TEXT NOT NULL,
    status INTEGER, request_id TEXT, json TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS records_time ON records(time);";

impl SqliteSink {
    pub fn open(path: &Path) -> rusqlite::Result<SqliteSink> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        let (tx, rx) = mpsc::channel::<Record>();
        thread::Builder::new()
            .name("sonde-reqlog-sqlite".into())
            .spawn(move || {
                let conn = conn;
                while let Ok(r) = rx.recv() {
                    let json = serde_json::to_string(&r).unwrap_or_default();
                    let _ = conn.execute(
                        "INSERT INTO records (id, time, endpoint, operation, status, request_id, json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![r.id as i64, r.time.to_rfc3339(), r.endpoint.as_str(), r.operation.as_str(), r.status, r.request_id, json],
                    );
                }
            })
            .expect("spawn sqlite writer");
        Ok(SqliteSink { tx })
    }
}

impl Sink for SqliteSink {
    fn write(&self, record: &Record) {
        let _ = self.tx.send(record.clone());
    }
}

/// Reads records back from a file written by [`SqliteSink`], newest first.
pub fn read(path: &Path, filter: &Filter, limit: usize) -> rusqlite::Result<Vec<Record>> {
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA)?;
    let mut stmt = conn.prepare("SELECT json FROM records ORDER BY time DESC, id DESC")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for json in rows {
        if let Ok(r) = serde_json::from_str::<Record>(&json?)
            && filter.matches(&r)
        {
            out.push(r);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Header, RequestLog};
    use std::sync::Arc;

    #[test]
    fn persists_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.sqlite");
        let log = RequestLog::new(10);
        log.set_sink(Some(Arc::new(SqliteSink::open(&path).unwrap())));
        log.push(Record {
            operation: "GetObject".into(),
            status: Some(200),
            request_headers: vec![Header::new("x-amz-security-token", "SECRET")],
            ..Default::default()
        });
        log.set_sink(None); // drops the sender: the writer finishes
        std::thread::sleep(std::time::Duration::from_millis(200));
        let back = read(&path, &Filter::default(), 10).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].operation, "GetObject");
        let raw = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("SECRET"));
    }
}
