//! One index per sessions folder (`.index.jsonl`): what sonde keeps of each
//! session in it — its counts and figures ([`crate::summary`]), its trials'
//! verdicts ([`crate::trial`]), the exit code its program ended with —
//! instead of files beside each one.
//!
//! The index is a log: each change is a line appended (under a lock, so the
//! app and the CLI may write at once), and the last line about a session is
//! what holds. When most lines are stale it is written again, compacted.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::summary::Counted;
use crate::trial::Trial;

/// The index's name in a sessions folder: hidden, so that a listing of the
/// folder is its sessions.
pub const FILE: &str = ".index.jsonl";

/// What the index keeps of one session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counted: Option<Counted>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trials: Option<Vec<Trial>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
}

/// One line of the index: a change to one session's entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "set", content = "value")]
pub enum Change {
    Counted(Box<Counted>),
    Trials(Vec<Trial>),
    Exit(i32),
    NoCounted,
    NoTrials,
    /// The session is gone: nothing of it is kept.
    Gone,
}

#[derive(Serialize, Deserialize)]
struct Line {
    name: String,
    #[serde(flatten)]
    change: Change,
}

type Entries = BTreeMap<String, Entry>;

fn apply(entries: &mut Entries, name: &str, change: Change) {
    if matches!(change, Change::Gone) {
        entries.remove(name);
        return;
    }
    let e = entries.entry(name.to_string()).or_default();
    match change {
        Change::Counted(c) => e.counted = Some(*c),
        Change::Trials(t) => e.trials = Some(t),
        Change::Exit(x) => e.exit = Some(x),
        Change::NoCounted => e.counted = None,
        Change::NoTrials => e.trials = None,
        Change::Gone => {}
    }
    if *e == Entry::default() {
        entries.remove(name);
    }
}

/// An index read: its entries, and how many lines it took (to know when to compact).
struct Read {
    stamp: Option<(u64, u64)>,
    lines: usize,
    entries: Arc<Entries>,
}

/// The indexes read in this process, by folder: read again only when the file changed.
static READ: std::sync::LazyLock<Mutex<std::collections::HashMap<PathBuf, Read>>> =
    std::sync::LazyLock::new(Default::default);

fn parse(bytes: &[u8]) -> (Entries, usize) {
    let mut entries = Entries::new();
    let mut lines = 0;
    for l in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        lines += 1;
        // a torn or unknown line is passed over: what it said is counted again
        if let Ok(Line { name, change }) = serde_json::from_slice(l) {
            apply(&mut entries, &name, change);
        }
    }
    (entries, lines)
}

fn entries_in(dir: &Path) -> Arc<Entries> {
    let file = dir.join(FILE);
    let stamp = crate::summary::stamp(&file);
    let mut read = READ.lock();
    if let Some(r) = read.get(dir)
        && r.stamp == stamp
    {
        return r.entries.clone();
    }
    let (entries, lines) = std::fs::read(&file).map(|b| parse(&b)).unwrap_or_default();
    let entries = Arc::new(entries);
    read.insert(
        dir.to_path_buf(),
        Read {
            stamp,
            lines,
            entries: entries.clone(),
        },
    );
    entries
}

fn place(session: &Path) -> Option<(PathBuf, String)> {
    let name = session.file_name()?.to_string_lossy().into_owned();
    let dir = match session.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    Some((dir, name))
}

/// What the index of its folder keeps of `session`.
pub fn get(session: &Path) -> Entry {
    place(session)
        .and_then(|(dir, name)| entries_in(&dir).get(&name).cloned())
        .unwrap_or_default()
}

/// Holds the folder's `.index.lock` until dropped; `None` where it cannot
/// be taken (the change is written without it).
fn lock(dir: &Path) -> Option<std::fs::File> {
    hold(dir, "index")
}

/// Holds the folder's `.{what}.lock` until dropped: what a file kept in it
/// (`findings.json`, `verdicts.json`, `written.json`) is read, changed and
/// written again under, so that the app and the CLI do not lose each
/// other's changes. `None` where it cannot be taken.
pub fn hold(dir: &Path, what: &str) -> Option<std::fs::File> {
    let f = reqlog::private::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(format!(".{what}.lock")))
        .ok()?;
    f.lock().ok()?;
    Some(f)
}

/// Records a change to `session`'s entry. A failure to write is no error:
/// what was to be kept is reckoned again next time.
pub fn set(session: &Path, change: Change) {
    let Some((dir, name)) = place(session) else {
        return;
    };
    if !dir.is_dir() {
        return;
    }
    let _held = lock(&dir);
    let file = dir.join(FILE);
    // read as it is before the change: the change is then applied to it, not read again
    entries_in(&dir);
    let before = crate::summary::stamp(&file);
    let Ok(mut line) = serde_json::to_vec(&Line {
        name: name.clone(),
        change: change.clone(),
    }) else {
        return;
    };
    line.push(b'\n');
    let written = reqlog::private::options()
        .append(true)
        .create(true)
        .open(&file)
        .and_then(|mut f| f.write_all(&line));
    let mut read = READ.lock();
    if written.is_err() {
        read.remove(&dir);
        return;
    }
    // what this process read is brought up to date, not read again
    let after = crate::summary::stamp(&file);
    match read.get_mut(&dir) {
        Some(r) if r.stamp == before => {
            apply(Arc::make_mut(&mut r.entries), &name, change);
            r.stamp = after;
            r.lines += 1;
            // mostly stale lines: written again, one line a session
            if r.lines > 64 && r.lines > 4 * r.entries.len() {
                let entries = r.entries.clone();
                drop(read);
                compact(&dir, &entries);
            }
        }
        _ => {
            read.remove(&dir);
        }
    }
}

/// Writes the index again with one line for each session (under the lock, held by the caller).
fn compact(dir: &Path, entries: &Entries) {
    let file = dir.join(FILE);
    let mut out = Vec::new();
    for (name, e) in entries {
        let mut push = |change: Change| {
            if let Ok(l) = serde_json::to_vec(&Line {
                name: name.clone(),
                change,
            }) {
                out.extend_from_slice(&l);
                out.push(b'\n');
            }
        };
        if let Some(c) = &e.counted {
            push(Change::Counted(Box::new(c.clone())));
        }
        if let Some(t) = &e.trials {
            push(Change::Trials(t.clone()));
        }
        if let Some(x) = e.exit {
            push(Change::Exit(x));
        }
    }
    let tmp = dir.join(format!(".index.jsonl.{}.tmp", std::process::id()));
    if reqlog::private::write(&tmp, &out).is_ok() && std::fs::rename(&tmp, &file).is_ok() {
        READ.lock().remove(dir);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counted(requests: u64) -> Counted {
        serde_json::from_value(serde_json::json!({
            "version": crate::summary::COUNTED_VERSION, "len": 1, "modifiedNs": 1,
            "created": "2026-01-01T00:00:00Z", "endpoint": "e", "recording": true,
            "requests": requests, "errors": 0, "misses": 0, "seconds": 1.0,
            "bytesUp": 0, "bytesDown": 0, "shape": []
        }))
        .unwrap()
    }

    #[test]
    fn the_last_change_holds_and_a_gone_session_is_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.sonde");
        let b = dir.path().join("b.sonde");
        set(&a, Change::Counted(Box::new(counted(1))));
        set(&a, Change::Counted(Box::new(counted(2))));
        set(&a, Change::Exit(3));
        set(&b, Change::Trials(Vec::new()));
        assert_eq!(get(&a).counted.unwrap().requests, 2);
        assert_eq!(get(&a).exit, Some(3));
        assert_eq!(get(&b).trials, Some(Vec::new()));
        set(&b, Change::NoTrials);
        assert_eq!(get(&b), Entry::default());
        set(&a, Change::Gone);
        assert_eq!(get(&a), Entry::default());
        // another process reads the same from the file
        READ.lock().clear();
        assert_eq!(get(&a), Entry::default());
        // one file in the folder, hidden: the folder lists its sessions
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.'))
            .collect();
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn many_changes_are_compacted_to_a_line_a_session() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.sonde");
        for i in 0..200 {
            set(&a, Change::Counted(Box::new(counted(i))));
        }
        let lines = std::fs::read_to_string(dir.path().join(FILE))
            .unwrap()
            .lines()
            .count();
        assert!(lines < 100, "{lines} lines");
        READ.lock().clear();
        assert_eq!(get(&a).counted.unwrap().requests, 199);
    }
}
