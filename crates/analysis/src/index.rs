//! One index per runs folder (`.index.jsonl`): all that sonde keeps beside
//! the runs in it, in one file under one lock. Of each run: its counts and
//! figures ([`crate::summary`]), its trials' verdicts ([`crate::trial`]),
//! the exit code its program ended with, the title it was given. Of the
//! folder: each program's baseline, findings ([`crate::fixloop`]) and trial
//! history, the files imported into it, and where each import of logs got to.
//!
//! The index is a log: each change is a line appended (under a lock, so the
//! app and the CLI may write at once), and the last line about a thing is
//! what holds. When most lines are stale it is written again, compacted.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::summary::Counted;
use crate::trial::Trial;

/// The index's name in a runs folder: hidden, so that a listing of the
/// folder is its runs.
pub const FILE: &str = ".index.jsonl";

/// What the index keeps of one run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counted: Option<Counted>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trials: Option<Vec<Trial>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    /// The title it was given (in the app).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// What the index keeps of the folder, each by its key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Folder {
    /// Program → the name of its baseline run.
    pub baselines: BTreeMap<String, String>,
    /// Program → its findings across its runs ([`crate::fixloop::Program`]).
    pub findings: BTreeMap<String, Value>,
    /// Program → scenario → its verdicts ([`crate::trial::Verdicts`]).
    pub verdicts: BTreeMap<String, Value>,
    /// A file's digest → the run it was imported as (the app's).
    pub imports: BTreeMap<String, String>,
    /// A log source → where its last import got to.
    pub marks: BTreeMap<String, Value>,
}

/// One line of the index: a change to one run's entry (by its file name),
/// or to one of the folder's keeps (by its key).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "set", content = "value")]
pub enum Change {
    Counted(Box<Counted>),
    Trials(Vec<Trial>),
    Exit(i32),
    NoCounted,
    NoTrials,
    /// A run's title; none: no title.
    Title(Option<String>),
    /// The run is gone: nothing of it is kept.
    Gone,
    /// A program's baseline run; none: no baseline.
    Baseline(Option<String>),
    /// A program's findings; none: forgotten.
    Findings(Option<Value>),
    /// A program's verdicts; none: forgotten.
    Verdicts(Option<Value>),
    /// The run a file was imported as; none: forgotten.
    Import(Option<String>),
    /// Where a log source's imports got to.
    Mark(Option<Value>),
}

#[derive(Serialize, Deserialize)]
struct Line {
    name: String,
    #[serde(flatten)]
    change: Change,
}

/// Everything an index holds: its runs' entries and the folder's keeps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Index {
    pub runs: BTreeMap<String, Entry>,
    pub folder: Folder,
}

fn keep<T>(map: &mut BTreeMap<String, T>, name: &str, v: Option<T>) {
    match v {
        Some(v) => {
            map.insert(name.to_string(), v);
        }
        None => {
            map.remove(name);
        }
    }
}

fn apply(index: &mut Index, name: &str, change: Change) {
    let f = &mut index.folder;
    match change {
        Change::Baseline(v) => return keep(&mut f.baselines, name, v),
        Change::Findings(v) => return keep(&mut f.findings, name, v),
        Change::Verdicts(v) => return keep(&mut f.verdicts, name, v),
        Change::Import(v) => return keep(&mut f.imports, name, v),
        Change::Mark(v) => return keep(&mut f.marks, name, v),
        Change::Gone => {
            index.runs.remove(name);
            return;
        }
        _ => {}
    }
    let e = index.runs.entry(name.to_string()).or_default();
    match change {
        Change::Counted(c) => e.counted = Some(*c),
        Change::Trials(t) => e.trials = Some(t),
        Change::Exit(x) => e.exit = Some(x),
        Change::NoCounted => e.counted = None,
        Change::NoTrials => e.trials = None,
        Change::Title(t) => e.title = t.filter(|t| !t.trim().is_empty()),
        _ => {}
    }
    if *e == Entry::default() {
        index.runs.remove(name);
    }
}

/// An index read: what it holds, and how many lines it took (to know when to compact).
struct Read {
    stamp: Option<(u64, u64)>,
    lines: usize,
    index: Arc<Index>,
}

/// The indexes read in this process, by folder: read again only when the file changed.
static READ: std::sync::LazyLock<Mutex<std::collections::HashMap<PathBuf, Read>>> =
    std::sync::LazyLock::new(Default::default);

fn parse(bytes: &[u8]) -> (Index, usize) {
    let mut index = Index::default();
    let mut lines = 0;
    for l in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        lines += 1;
        // a torn or unknown line is passed over: what it said is counted again
        if let Ok(Line { name, change }) = serde_json::from_slice(l) {
            apply(&mut index, &name, change);
        }
    }
    (index, lines)
}

/// What the index of `dir` holds, read again only when it changed.
pub fn of(dir: &Path) -> Arc<Index> {
    let file = dir.join(FILE);
    let stamp = crate::summary::stamp(&file);
    let mut read = READ.lock();
    if let Some(r) = read.get(dir)
        && r.stamp == stamp
    {
        return r.index.clone();
    }
    let (index, lines) = std::fs::read(&file).map(|b| parse(&b)).unwrap_or_default();
    let index = Arc::new(index);
    read.insert(
        dir.to_path_buf(),
        Read {
            stamp,
            lines,
            index: index.clone(),
        },
    );
    index
}

/// The folder a run is in, and its name there.
fn place(run: &Path) -> Option<(PathBuf, String)> {
    let name = run.file_name()?.to_string_lossy().into_owned();
    let dir = match run.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    Some((dir, name))
}

/// What the index of its folder keeps of `run`.
pub fn get(run: &Path) -> Entry {
    place(run)
        .and_then(|(dir, name)| of(&dir).runs.get(&name).cloned())
        .unwrap_or_default()
}

/// Holds the folder's `.index.lock` until dropped; `None` where it cannot
/// be taken (the change is written without it).
fn lock(dir: &Path) -> Option<std::fs::File> {
    let f = reqlog::private::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(".index.lock"))
        .ok()?;
    f.lock().ok()?;
    Some(f)
}

/// Records a change to `run`'s entry. A failure to write is no error:
/// what was to be kept is reckoned again next time.
pub fn set(run: &Path, change: Change) {
    let Some((dir, name)) = place(run) else {
        return;
    };
    let _ = update(&dir, |_| (vec![(name, change)], ()));
}

/// Changes what the index of `dir` holds, from what it holds now: `f` is
/// given the index as it is under the folder's lock (the app and the CLI
/// may write at once), and says the changes to append. Nothing is written
/// to a folder that is not there.
pub fn update<T>(
    dir: &Path,
    f: impl FnOnce(&Index) -> (Vec<(String, Change)>, T),
) -> std::io::Result<T> {
    if !dir.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} is no folder", dir.display()),
        ));
    }
    let _held = lock(dir);
    let file = dir.join(FILE);
    // read as it is before the change: the change is then applied to it, not read again
    let now = of(dir);
    let before = crate::summary::stamp(&file);
    let (changes, out) = f(&now);
    if changes.is_empty() {
        return Ok(out);
    }
    let mut lines = Vec::new();
    for (name, change) in &changes {
        let l = serde_json::to_vec(&Line {
            name: name.clone(),
            change: change.clone(),
        })?;
        lines.extend_from_slice(&l);
        lines.push(b'\n');
    }
    let written = reqlog::private::options()
        .append(true)
        .create(true)
        .open(&file)
        .and_then(|mut f| f.write_all(&lines));
    let mut read = READ.lock();
    if let Err(e) = written {
        read.remove(dir);
        return Err(e);
    }
    // what this process read is brought up to date, not read again
    let after = crate::summary::stamp(&file);
    match read.get_mut(dir) {
        Some(r) if r.stamp == before => {
            let index = Arc::make_mut(&mut r.index);
            for (name, change) in changes {
                apply(index, &name, change);
            }
            r.stamp = after;
            r.lines += lines.iter().filter(|b| **b == b'\n').count();
            // mostly stale lines: written again, one line a thing kept
            let f = &r.index.folder;
            let things = r.index.runs.len()
                + f.baselines.len()
                + f.findings.len()
                + f.verdicts.len()
                + f.imports.len()
                + f.marks.len();
            if r.lines > 64 && r.lines > 4 * things {
                let index = r.index.clone();
                drop(read);
                compact(dir, &index);
            }
        }
        _ => {
            read.remove(dir);
        }
    }
    Ok(out)
}

/// Writes the index again with one line for each thing kept (under the
/// lock, held by the caller).
fn compact(dir: &Path, index: &Index) {
    let file = dir.join(FILE);
    let mut out = Vec::new();
    let mut push = |name: &str, change: Change| {
        if let Ok(l) = serde_json::to_vec(&Line {
            name: name.to_string(),
            change,
        }) {
            out.extend_from_slice(&l);
            out.push(b'\n');
        }
    };
    for (name, e) in &index.runs {
        if let Some(c) = &e.counted {
            push(name, Change::Counted(Box::new(c.clone())));
        }
        if let Some(t) = &e.trials {
            push(name, Change::Trials(t.clone()));
        }
        if let Some(x) = e.exit {
            push(name, Change::Exit(x));
        }
        if let Some(t) = &e.title {
            push(name, Change::Title(Some(t.clone())));
        }
    }
    let f = &index.folder;
    for (k, v) in &f.baselines {
        push(k, Change::Baseline(Some(v.clone())));
    }
    for (k, v) in &f.findings {
        push(k, Change::Findings(Some(v.clone())));
    }
    for (k, v) in &f.verdicts {
        push(k, Change::Verdicts(Some(v.clone())));
    }
    for (k, v) in &f.imports {
        push(k, Change::Import(Some(v.clone())));
    }
    for (k, v) in &f.marks {
        push(k, Change::Mark(Some(v.clone())));
    }
    if reqlog::private::replace(&file, &out).is_ok() {
        READ.lock().remove(dir);
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
    fn the_last_change_holds_and_a_gone_run_is_forgotten() {
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
        // one file in the folder, hidden: the folder lists its runs
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.'))
            .collect();
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn many_changes_are_compacted_to_a_line_a_run() {
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

    /// 21.0 kept baselines, findings, verdicts, titles and imports in a file
    /// each, each with a lock of its own: now all in the one index.
    #[test]
    fn the_folders_keeps_are_in_the_one_index_and_survive_compaction() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let a = d.join("a.sonde");
        update(d, |_| {
            (
                vec![
                    ("etl".into(), Change::Baseline(Some("a.sonde".into()))),
                    (
                        "etl".into(),
                        Change::Findings(Some(serde_json::json!({"x": 1}))),
                    ),
                    ("etl".into(), Change::Verdicts(Some(serde_json::json!({})))),
                    ("digest".into(), Change::Import(Some("a.sonde".into()))),
                    ("a.sonde".into(), Change::Title(Some("nightly".into()))),
                ],
                (),
            )
        })
        .unwrap();
        for i in 0..200 {
            set(&a, Change::Exit(i));
        }
        READ.lock().clear();
        let ix = of(d);
        assert_eq!(ix.folder.baselines["etl"], "a.sonde");
        assert_eq!(ix.folder.findings["etl"]["x"], 1);
        assert_eq!(ix.folder.imports["digest"], "a.sonde");
        assert_eq!(get(&a).title.as_deref(), Some("nightly"));
        assert_eq!(get(&a).exit, Some(199));
        // a read-change-write sees what is there under the lock
        let n = update(d, |ix| {
            (
                vec![("etl".into(), Change::Baseline(None))],
                ix.folder.baselines.len(),
            )
        })
        .unwrap();
        assert_eq!(n, 1);
        assert!(of(d).folder.baselines.is_empty());
        // one file and its lock: nothing else beside the runs
        let mut names: Vec<String> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, [".index.jsonl", ".index.lock"]);
    }
}
