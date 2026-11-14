//! The fix loop: a finding followed across a program's runs. A finding is
//! the same finding while its identity is ([`crate::profile::finding_identity`]:
//! its kind and what it is about, never a run's own ids or counts). Between
//! two runs it is *fixed* (gone at any threshold), *less* or *more* (still
//! there, its count fallen or grown), *open* (the same), *new* or *back*
//! (fixed or under its threshold before, over it again); a fixed one says
//! what it saved, measured, against what was predicted. A run's failed
//! requests are compared by what failed them, as findings of their own.
//! Identities are made by rules of a version ([`crate::profile::IDENTITY_RULES`]), kept
//! with the findings: ones kept under older rules are made again.
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
    /// Still there (at any threshold), less of it than in the run before.
    Less,
    /// Fixed in an earlier run, or under its threshold in the run before,
    /// and here again.
    Back,
    /// Not in the run before.
    New,
    /// Still there (at any threshold), more of it than in the run before.
    More,
    /// In this run and the one before, as much of it.
    Open,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Fixed => "fixed",
            State::Less => "less",
            State::More => "more",
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
                State::More => "fix.more",
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
    /// As it read in the run before, when it reads otherwise now (less,
    /// more, back).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<FindingText>,
    /// How much of it each run had (its count: times listed, requests
    /// throttled or failed), the run before first: what less and more are
    /// judged by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<[usize; 2]>,
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
        own.min(a.profile.waiting_ms - b.profile.waiting_ms)
            .max(0.0)
    } else {
        0.0
    };
    Some(Saving { requests, ms })
}

/// How much of a finding a run had: its count (times a prefix was listed, a
/// key read, requests throttled), else the requests it bears on.
fn count_of(o: &Observation) -> usize {
    o.phrases
        .first()
        .and_then(|p| p.args.get("n"))
        .and_then(|n| n.as_ref().replace(',', "").parse().ok())
        .unwrap_or(o.impact)
}

/// Where a finding in both runs stands by its counts: more, less or as much.
fn by_counts(was: usize, now: usize) -> State {
    match now.cmp(&was) {
        std::cmp::Ordering::Greater => State::More,
        std::cmp::Ordering::Less => State::Less,
        std::cmp::Ordering::Equal => State::Open,
    }
}

/// Each finding of either run, where it stands in `b` against `a`, by its
/// counts, over its threshold or under it: fixed (gone at any threshold),
/// less, back (under its threshold before, over it now), new, more and
/// open (as much), in that order; a finding fixed by an earlier run is back
/// by a program's kept findings ([`Program::mark_back`]). One of time alone
/// is judged by its requests: with about as many requests one after
/// another (a fifth either way), it is open, whatever its timing. Then each
/// kind of failure ([`crate::profile::error_class`]) whose count moved.
pub fn changes(a: &Run, b: &Run) -> Vec<FindingChange> {
    let named = |os: &[Observation]| -> BTreeMap<String, usize> {
        os.iter()
            .enumerate()
            .filter(|(_, o)| !o.identity.is_empty())
            .map(|(i, o)| (o.identity.clone(), i))
            .collect()
    };
    let (ia, ib) = (named(&a.observations), named(&b.observations));
    let change = |id: &str, state: State, now: &Observation, was: &Observation| FindingChange {
        identity: id.to_string(),
        state,
        level: now.level,
        text: now.text.clone(),
        phrases: now.phrases.clone(),
        was: (now.text != was.text).then(|| text_of(was)),
        counts: Some([count_of(was), count_of(now)]),
        predicted: Saving::predicted(now),
        measured: None,
        saved: None,
        saved_text: None,
    };
    let mut out = Vec::new();
    for (id, &i) in &ia {
        let o = &a.observations[i];
        let left = ib.get(id).map(|&j| &b.observations[j]).or(b.traces.get(id));
        // requests one after another: their count moves with their timing,
        // so within a fifth either way it is as much
        if of_time(o)
            && let Some(t) = left
        {
            let (was, now) = (o.timed_ids.len(), t.timed_ids.len());
            if now * 5 >= was * 4 && now * 4 <= was * 5 {
                let mut c = change(id, State::Open, t, o);
                c.was = None;
                out.push(c);
                continue;
            }
        }
        // by its counts, over its threshold or under it now: as much is open
        let state = match left {
            None => State::Fixed,
            Some(t) => by_counts(count_of(o), count_of(t)),
        };
        if state != State::Fixed && state != State::Less {
            let t = left.unwrap_or(o);
            out.push(change(id, state, t, o));
            continue;
        }
        let predicted = Saving::predicted(o);
        let m = measured(o, left, a, b);
        let saved = m.and_then(|m| saved_phrase(m, predicted));
        let (text, phrases, was) = match left {
            Some(t) => (t.text.clone(), t.phrases.clone(), Some(text_of(o))),
            None => (o.text.clone(), o.phrases.clone(), None),
        };
        out.push(FindingChange {
            identity: id.clone(),
            state,
            level: o.level,
            text,
            phrases,
            was,
            counts: Some([count_of(o), left.map_or(0, count_of)]),
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
        let now = &b.observations[j];
        // under its threshold before and over it now is back
        let c = match a.traces.get(id) {
            Some(t) => change(id, State::Back, now, t),
            None => {
                let mut c = change(id, State::New, now, now);
                c.counts = Some([0, count_of(now)]);
                c
            }
        };
        out.push(c);
    }
    out.extend(error_changes(a, b));
    // fixed first, then less, back, new, more, open; a run's own order
    // within each, failures last
    let rank = |c: &FindingChange| {
        let at = ib
            .get(&c.identity)
            .or_else(|| ia.get(&c.identity))
            .copied()
            .unwrap_or(usize::MAX);
        (c.state, at)
    };
    out.sort_by_key(rank);
    out
}

/// The kind a run's failures are said by, as a finding's identity begins.
pub const ERRORS: &str = "obs.errors";

/// Each kind of failure whose count moved between `a` and `b`: as a
/// finding (`obs.errors|503 InternalError`), new, fixed, more or less.
fn error_changes(a: &Run, b: &Run) -> Vec<FindingChange> {
    let classes: std::collections::BTreeSet<&String> = a
        .error_classes
        .keys()
        .chain(b.error_classes.keys())
        .collect();
    let mut out = Vec::new();
    for class in classes {
        let was = a.error_classes.get(class).copied().unwrap_or(0);
        let now = b.error_classes.get(class).copied().unwrap_or(0);
        let state = match (was, now) {
            _ if was == now => continue,
            (0, _) => State::New,
            (_, 0) => State::Fixed,
            _ => by_counts(was, now),
        };
        let said = |n: usize| {
            p(
                "obs.errors",
                &[
                    ("n", n.to_string()),
                    ("s", plural(n)),
                    ("class", class.clone()),
                ],
            )
        };
        // a fixed one reads as it was
        let phrase = said(if now == 0 { was } else { now });
        out.push(FindingChange {
            identity: format!("{ERRORS}|{class}"),
            state,
            level: "warn",
            text: phrase.en(),
            phrases: vec![phrase],
            was: (was > 0 && now > 0).then(|| {
                let w = said(was);
                FindingText {
                    text: w.en(),
                    phrases: vec![w],
                }
            }),
            counts: Some([was, now]),
            predicted: Saving::default(),
            measured: None,
            saved: None,
            saved_text: None,
        });
    }
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Program {
    /// The newest run taken in: when it was recorded (RFC 3339) and its file
    /// name. An older run shown again changes nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<(String, String)>,
    #[serde(default)]
    pub findings: BTreeMap<String, Tracked>,
    /// The rules of identity its findings were kept by
    /// ([`crate::profile::IDENTITY_RULES`]; none written: 19.0's, 1).
    #[serde(default = "first_rules")]
    pub rules: u32,
}

fn first_rules() -> u32 {
    1
}

impl Default for Program {
    fn default() -> Self {
        Program {
            last: None,
            findings: BTreeMap::new(),
            rules: crate::profile::IDENTITY_RULES,
        }
    }
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
        // RFC 3339 of one offset (UTC) orders as text. A run older than the
        // last taken in (runs at once, ending out of order) adds what is
        // not kept yet, and changes nothing a later run said
        let late = self
            .last
            .as_ref()
            .is_some_and(|(at, _)| at.as_str() >= created);
        let tracked = |kind: String, text: &str, predicted: Saving| Tracked {
            kind,
            text: text.to_string(),
            state: State::Open,
            since: run.to_string(),
            predicted,
            measured: None,
            fixed_in: Vec::new(),
        };
        let said: HashMap<&str, State> = changes
            .iter()
            .map(|c| (c.identity.as_str(), c.state))
            .collect();
        let mut took = false;
        for o in found.iter().filter(|o| !o.identity.is_empty()) {
            if late && self.findings.contains_key(&o.identity) {
                continue;
            }
            took = true;
            let kind = o.phrases.first().map_or("", |p| p.key).to_string();
            let t = self
                .findings
                .entry(o.identity.clone())
                .or_insert_with(|| tracked(kind, &o.text, Saving::predicted(o)));
            // fixed before, or under its threshold the run before: back
            let now = match (t.state, said.get(o.identity.as_str())) {
                (State::Fixed, _) | (_, Some(State::Back)) => State::Back,
                (_, Some(State::Less)) => State::Less,
                (State::Back, _) => State::Back,
                _ => State::Open,
            };
            if now != t.state {
                t.state = now;
                t.since = run.to_string();
                t.measured = None;
            }
            t.text = o.text.clone();
            t.predicted = Saving::predicted(o);
        }
        for c in changes.iter().filter(|c| {
            matches!(c.state, State::Fixed | State::Less) && !c.identity.starts_with(ERRORS)
        }) {
            if late && self.findings.contains_key(&c.identity) {
                continue;
            }
            took = true;
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
        if late {
            return took;
        }
        self.last = Some((created.to_string(), run.to_string()));
        true
    }

    /// The findings kept under older rules of identity, made again by
    /// these ([`crate::profile::IDENTITY_RULES`]): what each is about made
    /// [`crate::profile::stable`] again. Findings that become one, or were
    /// told apart by rank (`#2`), are let go: what told them apart is not
    /// kept. How many were let go.
    pub fn rekey(&mut self) -> usize {
        use crate::profile::{IDENTITY_RULES, stable};
        if self.rules >= IDENTITY_RULES {
            return 0;
        }
        self.rules = IDENTITY_RULES;
        let mut by: BTreeMap<String, Vec<(String, Tracked)>> = BTreeMap::new();
        for (id, t) in std::mem::take(&mut self.findings) {
            // `#2` of an older run's rank
            let base = match id.rsplit_once('#') {
                Some((b, n)) if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => b,
                _ => id.as_str(),
            };
            let mut parts = base.split('|');
            let kind = parts.next().unwrap_or("");
            let about: Vec<String> = parts.map(stable).collect();
            let new = format!("{kind}|{}", about.join("|"));
            by.entry(new).or_default().push((id, t));
        }
        let mut dropped = 0;
        for (id, mut same) in by {
            if same.len() == 1 {
                let (_, t) = same.pop().expect("one");
                self.findings.insert(id, t);
            } else {
                dropped += same.len();
            }
        }
        dropped
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

/// Every program's findings: `findings.json` beside its runs, those kept
/// under older rules of identity made again ([`Program::rekey`]). None
/// kept when there is no file; a damaged one is an error.
pub fn read(dir: &Path) -> Result<BTreeMap<String, Program>, crate::program::Unreadable> {
    let path = dir.join("findings.json");
    match std::fs::read(&path) {
        Ok(b) => {
            let mut all: BTreeMap<String, Program> =
                serde_json::from_slice(&b).map_err(|e| crate::program::Unreadable {
                    path,
                    why: e.to_string(),
                })?;
            // kept under older rules of identity: made again by these
            for program in all.values_mut() {
                program.rekey();
            }
            Ok(all)
        }
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
        Some((
            s.header.created,
            (s.header.command.clone(), s.header.whole_seconds),
            run,
        ))
    });
    let (change, mut findings) = match &before {
        Some((_, (command, whole_seconds), a)) => {
            run.measure(records, a);
            let mut c = crate::profile::compare_runs(a, &run)
                .of_commands(command.as_deref(), header.command.as_deref());
            c.waiting_known = !whole_seconds && !header.whole_seconds;
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
                    counts: None,
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
    let _held = dir.is_dir().then(|| crate::index::hold(dir, "findings"));
    // back is judged by what was fixed by the earlier run compared (by the
    // run itself, when it is the program's first): never by a later one
    let upto = before
        .as_ref()
        .map_or(header.created, |(at, _, _)| *at)
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

/// Findings between two runs whose command changed
/// ([`crate::profile::RunChange::command_changed`]): what is fixed, less or
/// more is the workload's doing as much as a fix's, so none counts a saving.
pub fn by_command(findings: &mut [FindingChange]) {
    for f in findings
        .iter_mut()
        .filter(|f| matches!(f.state, State::Fixed | State::Less | State::More))
    {
        f.measured = None;
        f.saved = None;
        f.saved_text = None;
    }
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
