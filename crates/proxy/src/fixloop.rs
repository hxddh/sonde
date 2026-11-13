//! The fix loop: a finding followed across a program's runs. A finding is
//! the same finding while its identity is ([`crate::profile::finding_identity`]:
//! its kind and what it is about, never a run's own ids or counts). Between
//! two runs it is *fixed* (gone at any threshold), *less* (still there,
//! under its threshold), *open*, *new* or *back* (fixed before, here again);
//! a fixed one says what it saved, measured, against what was predicted.
//! Each program's findings are kept beside its runs (`findings.json`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use reqlog::Record;
use serde::{Deserialize, Serialize};

use crate::episode::Observation;
use crate::phrase::{Phrase, p, plural};
use crate::profile::{FindingText, RequestPrices, Run, RunChange};
use crate::session::SessionHeader;

/// Where a finding stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Gone at any threshold.
    Fixed,
    /// Still there, under its threshold.
    Less,
    /// Fixed in an earlier run, and here again.
    Back,
    /// Not in the run before.
    New,
    /// In this run and the one before.
    Open,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Fixed => "fixed",
            State::Less => "less",
            State::Back => "back",
            State::New => "new",
            State::Open => "open",
        }
    }

    /// The word, as a phrase (`fix.fixed`).
    pub fn phrase(self) -> Phrase {
        p(
            match self {
                State::Fixed => "fix.fixed",
                State::Less => "fix.less",
                State::Back => "fix.back",
                State::New => "fix.new",
                State::Open => "fix.open",
            },
            &[],
        )
    }
}

/// Requests not sent and time not waited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saving {
    pub requests: usize,
    pub ms: f64,
}

impl Saving {
    /// What a finding predicts acting on it saves.
    pub fn predicted(o: &Observation) -> Saving {
        Saving {
            requests: o.saves_requests,
            ms: o.saves_ms,
        }
    }

    pub fn is_none(&self) -> bool {
        self.requests == 0 && self.ms <= 0.0
    }
}

/// A finding between two runs of a program.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingChange {
    pub identity: String,
    pub state: State,
    /// `warn` or `info`.
    pub level: &'static str,
    /// As it reads in the later run (as it read in the earlier, when fixed).
    pub text: String,
    pub phrases: Vec<Phrase>,
    /// As it read in the run before, when it reads otherwise now (less).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<FindingText>,
    /// What acting on it was predicted to save (in the run it was found in).
    pub predicted: Saving,
    /// What it saved, measured between the runs (fixed and less).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<Saving>,
    /// The measured saving against the predicted, as it is said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved: Option<Phrase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_text: Option<String>,
}

/// What a request asked, as another run of the program asks it again: its
/// kind and what it named (`HeadObject b/in/part-001.json`, a listing with
/// its prefix).
pub fn asked(r: &Record) -> String {
    let prefix = r
        .query
        .iter()
        .find(|(k, _)| k == "prefix")
        .map(|(_, v)| v.to_string())
        .unwrap_or_default();
    format!(
        "{} {}/{}?{prefix}",
        r.operation,
        r.bucket.as_deref().unwrap_or(""),
        r.key.as_deref().unwrap_or("")
    )
}

/// What each finding would save, by what its requests asked: for each
/// identity, `(asked, of them saved, of them in the run)`.
pub type Asks = BTreeMap<String, Vec<(String, usize, usize)>>;

/// The asks of the findings that name the requests they save.
pub fn asks_of(records: &[Record], findings: &[Observation]) -> Asks {
    let saved: HashSet<u64> = findings
        .iter()
        .flat_map(|o| o.saved_ids.iter().copied())
        .collect();
    if saved.is_empty() {
        return Asks::new();
    }
    let by_id: HashMap<u64, String> = records
        .iter()
        .filter(|r| saved.contains(&r.id))
        .map(|r| (r.id, asked(r)))
        .collect();
    let wanted: HashSet<&str> = by_id.values().map(String::as_str).collect();
    let all = count(records, &wanted);
    let mut out = Asks::new();
    for o in findings.iter().filter(|o| !o.saved_ids.is_empty()) {
        let mut of: BTreeMap<&str, usize> = BTreeMap::new();
        for id in &o.saved_ids {
            if let Some(a) = by_id.get(id) {
                *of.entry(a.as_str()).or_default() += 1;
            }
        }
        out.insert(
            o.identity.clone(),
            of.into_iter()
                .map(|(a, n)| (a.to_string(), n, all.get(a).copied().unwrap_or(n)))
                .collect(),
        );
    }
    out
}

/// How many of `records` asked each of `wanted`.
pub fn count(records: &[Record], wanted: &HashSet<&str>) -> HashMap<String, usize> {
    let ops: HashSet<&str> = wanted
        .iter()
        .filter_map(|a| a.split_once(' ').map(|(op, _)| op))
        .collect();
    let mut out: HashMap<String, usize> = HashMap::new();
    for r in records
        .iter()
        .filter(|r| ops.contains(r.operation.as_str()))
    {
        let a = asked(r);
        if wanted.contains(a.as_str()) {
            *out.entry(a).or_default() += 1;
        }
    }
    out
}

fn text_of(o: &Observation) -> FindingText {
    FindingText {
        text: o.text.clone(),
        phrases: o.phrases.clone(),
    }
}

/// A finding of time alone (requests one after another): what it would
/// save is waiting, not requests.
fn of_time(o: &Observation) -> bool {
    o.saves_requests == 0 && o.saves_ms > 0.0
}

/// The measured saving said against the predicted.
fn saved_phrase(measured: Saving, predicted: Saving) -> Option<Phrase> {
    if predicted.requests > 0 {
        let n = measured.requests;
        Some(p(
            "fix.saved",
            &[
                ("n", n.to_string()),
                ("s", plural(n)),
                ("p", predicted.requests.to_string()),
            ],
        ))
    } else if predicted.ms > 0.0 {
        Some(p(
            "fix.savedTime",
            &[
                ("time", sonde_fmt::duration(measured.ms)),
                ("p", sonde_fmt::duration(predicted.ms)),
            ],
        ))
    } else {
        None
    }
}

/// What acting on `o` (of run `a`) saved by run `b`: of each request it
/// would save, as many as `b` asks less often, at most those it named. A
/// finding of time alone saved what its own waiting fell by (`left` is what
/// is left of it in `b`), never more than the run's waiting fell. None when
/// `b` was not counted against `a` ([`Run::measure`]).
fn measured(o: &Observation, left: Option<&Observation>, a: &Run, b: &Run) -> Option<Saving> {
    if !b.measured {
        return None;
    }
    let requests = a.asks.get(&o.identity).map_or(0, |asks| {
        asks.iter()
            .map(|(asked, saved, all)| {
                let now = b.counts.get(asked).copied().unwrap_or(0);
                all.saturating_sub(now).min(*saved)
            })
            .sum()
    });
    let ms = if of_time(o) {
        let own = o.saves_ms - left.map_or(0.0, |t| t.saves_ms);
        own.min(a.profile.waiting_ms - b.profile.waiting_ms).max(0.0)
    } else {
        0.0
    };
    Some(Saving { requests, ms })
}

/// Each finding of either run, where it stands in `b` against `a`: fixed,
/// less, new and open, in that order (back is said by a program's kept
/// findings, [`Program::mark_back`]). A finding under its threshold in
/// either run is still there: less in `b`, or open when `a` had it under
/// its threshold. One of time alone is judged by its requests: under its
/// threshold with as many requests one after another, it is open.
pub fn changes(a: &Run, b: &Run) -> Vec<FindingChange> {
    let named = |os: &[Observation]| -> BTreeMap<String, usize> {
        os.iter()
            .enumerate()
            .filter(|(_, o)| !o.identity.is_empty())
            .map(|(i, o)| (o.identity.clone(), i))
            .collect()
    };
    let (ia, ib) = (named(&a.observations), named(&b.observations));
    let open = |id: &str, now: &Observation| FindingChange {
        identity: id.to_string(),
        state: State::Open,
        level: now.level,
        text: now.text.clone(),
        phrases: now.phrases.clone(),
        was: None,
        predicted: Saving::predicted(now),
        measured: None,
        saved: None,
        saved_text: None,
    };
    let mut out = Vec::new();
    for (id, &i) in &ia {
        let o = &a.observations[i];
        if let Some(&j) = ib.get(id) {
            out.push(open(id, &b.observations[j]));
            continue;
        }
        let left = b.traces.get(id);
        // as many of its requests one after another: only their timing moved
        if of_time(o)
            && let Some(t) = left
            && t.timed_ids.len() * 10 >= o.timed_ids.len() * 9
        {
            out.push(open(id, t));
            continue;
        }
        let predicted = Saving::predicted(o);
        let m = measured(o, left, a, b);
        let saved = m.and_then(|m| saved_phrase(m, predicted));
        let (state, text, phrases, was) = match left {
            Some(t) => (
                State::Less,
                t.text.clone(),
                t.phrases.clone(),
                Some(text_of(o)),
            ),
            None => (State::Fixed, o.text.clone(), o.phrases.clone(), None),
        };
        out.push(FindingChange {
            identity: id.clone(),
            state,
            level: o.level,
            text,
            phrases,
            was,
            predicted,
            measured: m,
            saved_text: saved.as_ref().map(Phrase::en),
            saved,
        });
    }
    for (id, &j) in &ib {
        if ia.contains_key(id) {
            continue;
        }
        let mut c = open(id, &b.observations[j]);
        // over its threshold again is open; new is new to both runs
        if !a.traces.contains_key(id) {
            c.state = State::New;
        }
        out.push(c);
    }
    // fixed first, then less, new, open; a run's own order within each
    let rank = |c: &FindingChange| {
        let at = ib
            .get(&c.identity)
            .or_else(|| ia.get(&c.identity))
            .copied()
            .unwrap_or(0);
        (c.state, at)
    };
    out.sort_by_key(rank);
    out
}

/// A finding as a program keeps it, over its runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tracked {
    /// Its kind (`obs.headget`).
    pub kind: String,
    /// As it last read, in English.
    pub text: String,
    /// `open`, `less`, `fixed` or `back`.
    pub state: State,
    /// The run it came to stand so in, by file name.
    pub since: String,
    pub predicted: Saving,
    /// What fixing it saved, measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<Saving>,
    /// When each run that fixed it was recorded (RFC 3339): back is judged
    /// by the runs compared, never by a later one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixed_in: Vec<String>,
}

/// A program's findings over its runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Program {
    /// The newest run taken in: when it was recorded (RFC 3339) and its file
    /// name. An older run shown again changes nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<(String, String)>,
    #[serde(default)]
    pub findings: BTreeMap<String, Tracked>,
}

impl Program {
    /// A new finding is back when a run recorded no later than `upto` (the
    /// earlier of the two compared, RFC 3339) had fixed it: what a later run
    /// fixed says nothing of these two.
    pub fn mark_back(&self, changes: &mut [FindingChange], upto: &str) {
        for c in changes.iter_mut().filter(|c| c.state == State::New) {
            if self
                .findings
                .get(&c.identity)
                .is_some_and(|t| t.fixed_in.iter().any(|at| at.as_str() <= upto))
            {
                c.state = State::Back;
            }
        }
    }

    /// Takes a run in (`run` its file name, `created` when it was
    /// recorded): its findings open (back, if fixed before), and those the
    /// change says are fixed or less, so. False, and nothing changed, for a
    /// run no newer than the last taken in.
    pub fn take(
        &mut self,
        run: &str,
        created: &str,
        found: &[Observation],
        changes: &[FindingChange],
    ) -> bool {
        // RFC 3339 of one offset (UTC) orders as text
        if self
            .last
            .as_ref()
            .is_some_and(|(at, _)| at.as_str() >= created)
        {
            return false;
        }
        let tracked = |kind: String, text: &str, predicted: Saving| Tracked {
            kind,
            text: text.to_string(),
            state: State::Open,
            since: run.to_string(),
            predicted,
            measured: None,
            fixed_in: Vec::new(),
        };
        for o in found.iter().filter(|o| !o.identity.is_empty()) {
            let kind = o.phrases.first().map_or("", |p| p.key).to_string();
            let t = self
                .findings
                .entry(o.identity.clone())
                .or_insert_with(|| tracked(kind, &o.text, Saving::predicted(o)));
            // fixed before: back; under its threshold before: open again
            let now = match t.state {
                State::Fixed => State::Back,
                State::Less => State::Open,
                s => s,
            };
            if now != t.state {
                t.state = now;
                t.since = run.to_string();
                t.measured = None;
            }
            t.text = o.text.clone();
            t.predicted = Saving::predicted(o);
        }
        for c in changes
            .iter()
            .filter(|c| matches!(c.state, State::Fixed | State::Less))
        {
            let kind = c.phrases.first().map_or("", |p| p.key).to_string();
            let t = self
                .findings
                .entry(c.identity.clone())
                .or_insert_with(|| tracked(kind, &c.text, c.predicted));
            if t.state != c.state {
                t.since = run.to_string();
            }
            t.state = c.state;
            t.text = c.text.clone();
            t.predicted = c.predicted;
            t.measured = c.measured;
            if c.state == State::Fixed {
                t.fixed_in.push(created.to_string());
            }
        }
        self.last = Some((created.to_string(), run.to_string()));
        true
    }
}

/// The folder a run's program keeps its state in: the run's own, as its
/// verdicts (`verdicts.json`), what it wrote (`written.json`) and its
/// baseline are kept.
pub fn folder_of(run: &Path) -> PathBuf {
    run.parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

/// Every program's findings: `findings.json` beside its runs. None
/// kept when there is no file; a damaged one is an error.
pub fn read(dir: &Path) -> Result<BTreeMap<String, Program>, crate::program::Unreadable> {
    let path = dir.join("findings.json");
    match std::fs::read(&path) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| crate::program::Unreadable {
            path,
            why: e.to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(crate::program::Unreadable {
            path,
            why: e.to_string(),
        }),
    }
}

pub fn write(dir: &Path, all: &BTreeMap<String, Program>) -> std::io::Result<()> {
    // a name of its own: the app and the CLI may write at the same time
    let tmp = dir.join(format!(
        "findings.json.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    reqlog::private::write(&tmp, serde_json::to_vec_pretty(all)?)?;
    std::fs::rename(&tmp, dir.join("findings.json"))
}

/// The exit code a run failed with, as its folder's index keeps it (`sonde
/// run` and the app keep it when the program ends): none when it exited 0
/// or it is not known.
pub fn failed(run: &Path) -> Option<i32> {
    crate::index::get(run).exit.filter(|x| *x != 0)
}

/// The run of program `key` recorded last before `created`, in `dir`
/// (not `not`, the run itself), that did not fail: what a run is compared
/// with.
pub fn previous_run(
    dir: &Path,
    key: &str,
    created: chrono::DateTime<chrono::Utc>,
    not: &Path,
) -> Option<PathBuf> {
    let not = not.canonicalize().ok();
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sonde"))
        .filter_map(|p| {
            let h = crate::session::read_header(&p)?;
            (h.is_recording()
                && h.created < created
                && crate::program::key_of(&h).as_deref() == Some(key))
            .then_some((h.created, p))
        })
        .filter(|(_, p)| not.is_none() || p.canonicalize().ok() != not)
        .filter(|(_, p)| failed(p).is_none())
        .max_by_key(|(at, _)| *at)
        .map(|(_, p)| p)
}

/// A run against the program's run before it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SinceLast {
    /// The program, as its runs are grouped.
    pub program: String,
    /// The exit code the run failed with: it is not compared, and the
    /// program's kept findings are left as they were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<i32>,
    /// The run before, by path: none for a program's first run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    /// The two runs compared: none for a program's first run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<RunChange>,
    /// The run's findings and those of the run before it no longer has,
    /// each where it stands.
    pub findings: Vec<FindingChange>,
    /// Why the program's kept findings were not read or written, when so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsaved: Option<String>,
}

/// A run (`path`, its `header`, its `records` and `run`, them analysed)
/// against the program's run before it that did not fail, found in `dir`;
/// and the program's kept findings brought up to it when it is its newest
/// run. A run that failed is not compared. None for a session that is no
/// run of a program (a replay, an import, a trial).
pub fn since_last(
    dir: &Path,
    path: &Path,
    header: &SessionHeader,
    records: &[Record],
    mut run: Run,
    prices: &dyn Fn(&str) -> RequestPrices,
) -> Option<SinceLast> {
    let key = crate::program::key_of(header)?;
    if !header.is_recording() {
        return None;
    }
    // a crash is no fix: what it did not get to is not gone
    if let Some(exit) = failed(path) {
        return Some(SinceLast {
            program: key,
            failed: Some(exit),
            previous: None,
            change: None,
            findings: Vec::new(),
            unsaved: None,
        });
    }
    let previous = previous_run(dir, &key, header.created, path);
    let before = previous.as_ref().and_then(|p| {
        let s = crate::session::Session::open_lean(p).ok()?;
        let run = Run::analysed(&s.header, &s.records, &prices(&s.header.endpoint));
        Some((s.header.created, run))
    });
    let (change, mut findings) = match &before {
        Some((_, a)) => {
            run.measure(records, a);
            let c = crate::profile::compare_runs(a, &run);
            let f = c.findings.clone();
            (Some(c), f)
        }
        None => (
            None,
            run.observations
                .iter()
                .filter(|o| !o.identity.is_empty())
                .map(|o| FindingChange {
                    identity: o.identity.clone(),
                    state: State::New,
                    level: o.level,
                    text: o.text.clone(),
                    phrases: o.phrases.clone(),
                    was: None,
                    predicted: Saving::predicted(o),
                    measured: None,
                    saved: None,
                    saved_text: None,
                })
                .collect(),
        ),
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // back is judged by what was fixed by the earlier run compared (by the
    // run itself, when it is the program's first): never by a later one
    let upto = before
        .as_ref()
        .map_or(header.created, |(at, _)| *at)
        .to_rfc3339();
    let unsaved = match read(dir) {
        // nothing recorded here yet (a run written elsewhere with -o)
        _ if !dir.is_dir() => None,
        Err(e) => Some(e.to_string()),
        Ok(mut all) => {
            let kept = all.entry(key.clone()).or_default();
            kept.mark_back(&mut findings, &upto);
            let created = header.created.to_rfc3339();
            if kept.take(&name, &created, &run.observations, &findings) {
                write(dir, &all).err().map(|e| e.to_string())
            } else {
                None
            }
        }
    };
    // a first run's findings are open unless back
    if before.is_none() {
        for f in findings.iter_mut().filter(|f| f.state == State::New) {
            f.state = State::Open;
        }
    }
    let change = change.map(|mut c| {
        c.findings = findings.clone();
        c.appeared = said(&findings, &[State::New, State::Back]);
        c.resolved = said(&findings, &[State::Fixed]);
        c.same_program = true;
        c
    });
    Some(SinceLast {
        program: key,
        failed: None,
        previous: previous.map(|p| p.to_string_lossy().into_owned()),
        change,
        findings,
        unsaved,
    })
}

/// The findings in `states`, as they read.
pub fn said(findings: &[FindingChange], states: &[State]) -> Vec<FindingText> {
    findings
        .iter()
        .filter(|f| states.contains(&f.state))
        .map(|f| FindingText {
            text: f.text.clone(),
            phrases: f.phrases.clone(),
        })
        .collect()
}
