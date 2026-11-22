//! The fix loop: a finding followed across a program's runs. A finding is
//! the same finding while its identity is ([`crate::profile::finding_identity`]:
//! its kind and what it is about, never a run's own ids or counts). Between
//! two runs it is *fixed* (gone at any threshold), *less* or *more* (still
//! there at any threshold, its count fallen or grown), *open* (as much) or
//! *new* (none of it before): judged by its count alone, never by a
//! threshold relative to the run's size. A fixed or less one says what the
//! run's wall time and cost moved by against what was predicted, or that
//! the move is within the program's own noise ([`noise_ms`]). A run's failed
//! requests are compared by what failed them, as findings of their own.
//! Identities are made by rules of a version ([`crate::profile::IDENTITY_RULES`]), kept
//! with the findings: ones kept under older rules are made again.
//! Each program's findings are kept in its runs folder's index ([`crate::index`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use reqlog::Record;
use serde::{Deserialize, Serialize};

use crate::episode::Observation;
use crate::phrase::{Phrase, p, plural};
use crate::profile::{FindingText, RequestPrices, Run, RunChange};
use crate::run::RunHeader;

/// Where a finding stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Gone at any threshold.
    Fixed,
    /// Still there (at any threshold), less of it than in the run before.
    Less,
    /// Not in the run before, at any threshold.
    New,
    /// Still there (at any threshold), more of it than in the run before.
    More,
    /// In this run and the one before, as much of it. (`back`, of 28.0 and
    /// before, reads as open.)
    #[serde(alias = "back")]
    Open,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Fixed => "fixed",
            State::Less => "less",
            State::More => "more",
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
                State::New => "fix.new",
                State::Open => "fix.open",
            },
            &[],
        )
    }
}

/// Requests not sent, time not taken and money not spent. Predicted, of
/// a finding; measured, between two runs: the requests its own asks fell
/// by, and the run's wall time and cost (each may be negative: it grew).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saving {
    pub requests: usize,
    pub ms: f64,
    #[serde(default)]
    pub cost: f64,
}

impl Saving {
    /// What a finding predicts acting on it saves.
    pub fn predicted(o: &Observation) -> Saving {
        Saving {
            requests: o.saves_requests,
            ms: o.saves_ms,
            cost: o.saves_cost,
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
    /// What it saved, measured between the runs (fixed and less): the
    /// requests of its own asks, and the run's wall time and cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<Saving>,
    /// The run's wall time moved by less than the program's runs vary
    /// ([`noise_ms`]): what it saved is not told from noise.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub within_noise: bool,
    /// The measured saving against the predicted, as it is said: `1.2 s
    /// less (predicted 1.5 s)`, or `within noise`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved: Option<Phrase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_text: Option<String>,
}

/// What a request asked, as another run of the program asks it again: its
/// kind and what it named, made [`stable`](crate::profile::stable) as a
/// finding's identity is (`HeadObject b/in/part-{n}.json`, a listing with
/// its prefix): a key that holds the run's own id is the same ask in the
/// next run.
pub fn asked(r: &Record) -> String {
    let prefix = r
        .query()
        .iter()
        .find(|(k, _)| k == "prefix")
        .map(|(_, v)| crate::profile::stable(v))
        .unwrap_or_default();
    format!(
        "{} {}/{}?{prefix}",
        r.kind,
        r.bucket.as_deref().unwrap_or(""),
        crate::profile::stable(r.key.as_deref().unwrap_or(""))
    )
}

/// What each finding would save, by what its requests asked: for each
/// identity, `(asked, of them saved, of them in the run)`.
pub type Asks = BTreeMap<String, Vec<(String, usize, usize)>>;

/// The asks of the findings that name the requests they save. Each ask is
/// made once, its text kept once and its requests by index (a finding of
/// small reads names a request of each of a run's objects).
pub fn asks_of(records: &[Record], findings: &[Observation]) -> Asks {
    let mut saved: Vec<u64> = findings
        .iter()
        .flat_map(|o| o.saved_ids.iter().copied())
        .collect();
    if saved.is_empty() {
        return Asks::new();
    }
    saved.sort_unstable();
    saved.dedup();
    // what each saved request asked: an index into `names`
    let mut names: HashMap<String, u32> = HashMap::new();
    let mut of_id: Vec<(u64, u32)> = Vec::new();
    for r in records
        .iter()
        .filter(|r| saved.binary_search(&r.id).is_ok())
    {
        let next = names.len() as u32;
        of_id.push((r.id, *names.entry(asked(r)).or_insert(next)));
    }
    drop(saved);
    // (a request's id twice: its last record's ask, as a map would keep)
    of_id.reverse();
    of_id.sort_by_key(|&(id, _)| id);
    of_id.dedup_by_key(|&mut (id, _)| id);
    // how many of the run's requests asked each
    let mut all = vec![0usize; names.len()];
    {
        let ops: HashSet<&str> = names
            .keys()
            .filter_map(|a| a.split_once(' ').map(|(op, _)| op))
            .collect();
        for r in records.iter().filter(|r| ops.contains(r.kind.as_str())) {
            if let Some(&i) = names.get(asked(r).as_str()) {
                all[i as usize] += 1;
            }
        }
    }
    let mut texts = vec![String::new(); names.len()];
    for (a, i) in names {
        texts[i as usize] = a;
    }
    // each finding's asks, in their order, each with how many it saves
    let mut uses = vec![0u32; texts.len()];
    let mut each: Vec<(&Observation, Vec<(u32, usize)>)> = Vec::new();
    for o in findings.iter().filter(|o| !o.saved_ids.is_empty()) {
        let mut of: Vec<u32> = o
            .saved_ids
            .iter()
            .filter_map(|id| {
                let at = of_id.binary_search_by_key(id, |&(id, _)| id).ok()?;
                Some(of_id[at].1)
            })
            .collect();
        of.sort_unstable();
        let mut counted: Vec<(u32, usize)> = of
            .chunk_by(|a, b| a == b)
            .map(|c| (c[0], c.len()))
            .collect();
        drop(of);
        counted.sort_unstable_by(|a, b| texts[a.0 as usize].cmp(&texts[b.0 as usize]));
        for &(i, _) in &counted {
            uses[i as usize] += 1;
        }
        each.push((o, counted));
    }
    drop(of_id);
    let mut out = Asks::new();
    for (o, counted) in each {
        let asks = counted
            .into_iter()
            .map(|(i, n)| {
                let i = i as usize;
                uses[i] -= 1;
                let a = if uses[i] == 0 {
                    std::mem::take(&mut texts[i])
                } else {
                    texts[i].clone()
                };
                (a, n, if all[i] == 0 { n } else { all[i] })
            })
            .collect();
        out.insert(o.identity.clone(), asks);
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
    for r in records.iter().filter(|r| ops.contains(r.kind.as_str())) {
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

/// What a finding saved, as it is said: the run's wall time (and its cost,
/// from a cent, in `currency`) against what was predicted, `1.2 s less
/// (predicted 1.5 s)`; `within noise` when the wall time moved by less than
/// the program's runs vary (`band`, ms: none known, none said). Nothing for
/// one not measured.
pub fn say_saved(f: &mut FindingChange, band: Option<f64>, currency: &str) {
    let Some(m) = f.measured else {
        return;
    };
    f.within_noise = band.is_some_and(|b| m.ms.abs() < b);
    let phrase = if f.within_noise {
        p("fix.noise", &[])
    } else {
        let mut time = sonde_fmt::duration(m.ms.abs());
        if m.cost.abs() >= crate::program::CENT {
            time = format!(
                "{time}, {}",
                crate::program::money_text(m.cost.abs(), currency)
            );
        }
        let predicted = (f.predicted.ms > 0.0).then(|| sonde_fmt::duration(f.predicted.ms));
        match (m.ms >= 0.0, predicted) {
            (true, Some(pr)) => p("fix.saved", &[("time", time), ("p", pr)]),
            (true, None) => p("fix.savedUnpredicted", &[("time", time)]),
            (false, Some(pr)) => p("fix.lost", &[("time", time), ("p", pr)]),
            (false, None) => p("fix.lostUnpredicted", &[("time", time)]),
        }
    };
    f.saved_text = Some(phrase.en());
    f.saved = Some(phrase);
}

/// What acting on `o` (of run `a`) saved by run `b`: of each request it
/// would save, as many as `b` asks less often, at most those it named; and
/// what the run's wall time and cost fell by. None when `b` was not counted
/// against `a` ([`Run::measure`]).
fn measured(o: &Observation, a: &Run, b: &Run) -> Option<Saving> {
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
    Some(Saving {
        requests,
        ms: a.profile.wall_ms - b.profile.wall_ms,
        cost: a.profile.cost.amount - b.profile.cost.amount,
    })
}

/// How much of a finding a run had: its count (times a prefix was listed, a
/// key read, requests throttled), else the requests it bears on.
pub fn count_of(o: &Observation) -> usize {
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
/// less, new (none of it in `a`, at any threshold), more and open (as
/// much), in that order: a finding under its threshold in `a` and over it
/// in `b` is judged by its counts as any other. One of time alone
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
        within_noise: false,
        saved: None,
        saved_text: None,
    };
    let currency = &b.profile.cost.currency;
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
        let (text, phrases, was) = match left {
            Some(t) => (t.text.clone(), t.phrases.clone(), Some(text_of(o))),
            None => (o.text.clone(), o.phrases.clone(), None),
        };
        let mut c = FindingChange {
            identity: id.clone(),
            state,
            level: o.level,
            text,
            phrases,
            was,
            counts: Some([count_of(o), left.map_or(0, count_of)]),
            predicted: Saving::predicted(o),
            measured: measured(o, a, b),
            within_noise: false,
            saved: None,
            saved_text: None,
        };
        say_saved(&mut c, None, currency);
        out.push(c);
    }
    for (id, &j) in &ib {
        if ia.contains_key(id) {
            continue;
        }
        let now = &b.observations[j];
        // under its threshold before and over it now: by its counts, as
        // any other (6 then 6 is as much, not back)
        let c = match a.traces.get(id) {
            Some(t) => change(id, by_counts(count_of(t), count_of(now)), now, t),
            None => {
                let mut c = change(id, State::New, now, now);
                c.counts = Some([0, count_of(now)]);
                c
            }
        };
        out.push(c);
    }
    out.extend(error_changes(a, b));
    // fixed first, then less, new, more, open; a run's own order
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
            within_noise: false,
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
    /// `open`, `less` or `fixed`.
    pub state: State,
    /// The run it came to stand so in, by file name.
    pub since: String,
    pub predicted: Saving,
    /// What fixing it saved, measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<Saving>,
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
    /// A finding the program keeps as still there (not fixed) that the run
    /// before (`a`) had only under its threshold and `b` has at none is
    /// fixed: gone at any threshold. ([`changes`] goes through `a`'s
    /// findings over their threshold; 24.0 said nothing of this one and
    /// kept it less.)
    pub fn mark_gone(&self, changes: &mut Vec<FindingChange>, a: &Run, b: &Run) {
        let said: HashSet<&str> = changes.iter().map(|c| c.identity.as_str()).collect();
        let in_b =
            |id: &str| b.traces.contains_key(id) || b.observations.iter().any(|o| o.identity == id);
        let gone: Vec<FindingChange> = self
            .findings
            .iter()
            .filter(|(id, t)| t.state != State::Fixed && !said.contains(id.as_str()))
            .filter(|(id, _)| !in_b(id) && !a.observations.iter().any(|o| &o.identity == *id))
            .filter_map(|(id, _)| {
                let was = a.traces.get(id)?;
                Some(FindingChange {
                    identity: id.clone(),
                    state: State::Fixed,
                    level: was.level,
                    text: was.text.clone(),
                    phrases: was.phrases.clone(),
                    was: None,
                    counts: Some([count_of(was), 0]),
                    predicted: Saving::predicted(was),
                    measured: None,
                    within_noise: false,
                    saved: None,
                    saved_text: None,
                })
            })
            .collect();
        if gone.is_empty() {
            return;
        }
        changes.extend(gone);
        // fixed first, as [`changes`] orders them (a stable sort: each
        // state's own order kept)
        changes.sort_by_key(|c| c.state);
    }

    /// Takes a run in (`run` its file name, `created` when it was
    /// recorded): its findings open, and those the change says are fixed
    /// or less, so. False, and nothing changed, for a
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
            let now = match said.get(o.identity.as_str()) {
                Some(State::Less) => State::Less,
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
        }
        if late {
            return took;
        }
        self.last = Some((created.to_string(), run.to_string()));
        true
    }

    /// The findings kept under older rules of identity, made again by
    /// these ([`crate::profile::IDENTITY_RULES`], [`crate::profile::by_rules`]):
    /// what each is about made [`crate::profile::stable`] again, a bucket's
    /// kinds' about their bucket. Findings that become one, or were
    /// told apart by rank (`#2`), are let go: what told them apart is not
    /// kept. How many were let go.
    pub fn rekey(&mut self) -> usize {
        use crate::profile::{IDENTITY_RULES, by_rules};
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
            by.entry(by_rules(base)).or_default().push((id, t));
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
/// verdicts and its baseline are kept (its index).
pub fn folder_of(run: &Path) -> PathBuf {
    run.parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

/// Every program's findings, as the folder's index keeps them, those kept
/// under older rules of identity made again ([`Program::rekey`]). One that
/// does not read is no program's.
pub fn read(dir: &Path) -> BTreeMap<String, Program> {
    programs_of(&crate::index::of(dir).folder.findings)
}

fn programs_of(kept: &BTreeMap<String, serde_json::Value>) -> BTreeMap<String, Program> {
    kept.iter()
        .filter_map(|(k, v)| {
            let mut p: Program = serde_json::from_value(v.clone()).ok()?;
            p.rekey();
            Some((k.clone(), p))
        })
        .collect()
}

/// The exit code a run failed with, as its folder's index keeps it (`sonde
/// run` and the app keep it when the program ends): none when it exited 0
/// or it is not known.
pub fn failed(run: &Path) -> Option<i32> {
    crate::index::get(run).exit.filter(|x| *x != 0)
}

/// Whether a run recorded no request: nothing reached sonde (the program
/// sent its requests elsewhere, or none). It is compared with nothing, as
/// a run that failed is not: what did not reach sonde is not fixed.
pub fn empty(run: &Path) -> bool {
    crate::summary::counted(run).is_some_and(|c| c.requests == 0)
}

/// Whether a run is a trial's clean run (`tried`, or kept with trials)
/// of another command than `command`'s: no run of the program as it runs
/// (`sonde run --trial -- etl.py 40` of a program run as `etl.py`), never
/// the run another is compared with, and itself compared with none.
pub fn tried_otherwise(path: &Path, tried: bool, own: Option<&str>, command: Option<&str>) -> bool {
    (tried || !crate::trial::load(path).is_empty())
        && crate::program::command_change(own, command).is_some()
}

/// Whether a run being tried (`sonde run --trial`: `path`, `header`) is of
/// another command than its program's run before it ([`tried_otherwise`]):
/// it is compared with none, and is not the program's newest.
pub fn tried_apart(path: &Path, header: &RunHeader) -> bool {
    let Some(key) = crate::program::key_of(header) else {
        return false;
    };
    let command = header.command.as_deref();
    previous_run(&folder_of(path), &key, header.created, path, command)
        .and_then(|p| crate::run::read_header(&p))
        .is_some_and(|h| tried_otherwise(path, true, command, h.command.as_deref()))
}

/// The runs of program `key` recorded before `created`, in `dir` (not
/// `not`, the run itself), the newest first, that did not fail, recorded
/// requests ([`empty`]) and are no trial of another command than `command`
/// ([`tried_otherwise`]): what a run is compared with, and what its noise
/// is read from.
fn earlier_runs<'a>(
    dir: &Path,
    key: &'a str,
    created: chrono::DateTime<chrono::Utc>,
    not: &Path,
    command: Option<&'a str>,
) -> impl Iterator<Item = (PathBuf, RunHeader)> + 'a {
    let not = not.canonicalize().ok();
    let mut runs = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sonde"))
        .filter_map(|p| {
            let h = crate::run::read_header(&p)?;
            (h.is_recording()
                && h.created < created
                && crate::program::key_of(&h).as_deref() == Some(key))
            .then_some((p, h))
        })
        .filter(|(p, _)| not.is_none() || p.canonicalize().ok() != not)
        .collect::<Vec<_>>();
    // the newest first: one that failed or is empty is passed over
    runs.sort_by_key(|(_, h)| std::cmp::Reverse(h.created));
    runs.into_iter().filter(move |(p, h)| {
        failed(p).is_none()
            && !empty(p)
            && !tried_otherwise(p, false, h.command.as_deref(), command)
    })
}

/// The run of program `key` recorded last before `created`, in `dir`, of
/// those [`earlier_runs`] gives (`command`: the run's own): what a run is
/// compared with.
pub fn previous_run(
    dir: &Path,
    key: &str,
    created: chrono::DateTime<chrono::Utc>,
    not: &Path,
    command: Option<&str>,
) -> Option<PathBuf> {
    earlier_runs(dir, key, created, not, command)
        .next()
        .map(|(p, _)| p)
}

/// How many earlier runs a program's noise is read from, at most.
pub const NOISE_RUNS: usize = 5;

/// How much a program's wall time varies run to run, in ms: the spread
/// (longest less shortest) of its last [`NOISE_RUNS`] earlier runs of the
/// same command ([`earlier_runs`], read from the folder's index). None
/// with fewer than two: no band is known.
pub fn noise_ms(
    dir: &Path,
    key: &str,
    created: chrono::DateTime<chrono::Utc>,
    not: &Path,
    command: Option<&str>,
) -> Option<f64> {
    let walls: Vec<f64> = earlier_runs(dir, key, created, not, command)
        .filter(|(_, h)| crate::program::command_change(h.command.as_deref(), command).is_none())
        .take(NOISE_RUNS)
        .filter_map(|(p, _)| Some(crate::summary::counted(&p)?.seconds * 1000.0))
        .collect();
    if walls.len() < 2 {
        return None;
    }
    let (lo, hi) = walls
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), &w| (lo.min(w), hi.max(w)));
    Some(hi - lo)
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
    /// The run recorded no request (nothing reached sonde): it is not
    /// compared, and the program's kept findings are left as they were.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub empty: bool,
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
/// run. A run that failed, or recorded no request, is not compared. None for a run that is no
/// run of a program (a replay, an import, a trial, a trial's clean run of
/// another command: [`tried_otherwise`]).
pub fn since_last(
    dir: &Path,
    path: &Path,
    header: &RunHeader,
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
            empty: false,
            previous: None,
            change: None,
            findings: Vec::new(),
            unsaved: None,
        });
    }
    // nor is a run nothing reached: what it did not see is not gone
    if records.is_empty() {
        return Some(SinceLast {
            program: key,
            failed: None,
            empty: true,
            previous: None,
            change: None,
            findings: Vec::new(),
            unsaved: None,
        });
    }
    let command = header.command.as_deref();
    let previous = previous_run(dir, &key, header.created, path, command);
    if let Some(p) = &previous
        && tried_otherwise(
            path,
            false,
            command,
            crate::run::read_header(p)
                .and_then(|h| h.command)
                .as_deref(),
        )
    {
        return None;
    }
    let before = previous.as_ref().and_then(|p| {
        let s = crate::run::RunFile::open_lean(p).ok()?;
        let run = Run::analysed(&s.header, &s.records, &prices(&s.header.endpoint));
        Some(((s.header.command.clone(), s.header.whole_seconds), run))
    });
    let (change, mut findings) = match &before {
        Some(((command, whole_seconds), a)) => {
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
                    within_noise: false,
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
    let created = header.created.to_rfc3339();
    // read, changed and kept under the index's lock: the app and the CLI
    // may record at once (nothing recorded here yet: a run written
    // elsewhere with -o)
    let unsaved = if dir.is_dir() {
        crate::index::update(dir, |ix| {
            let mut kept = ix
                .folder
                .findings
                .get(&key)
                .and_then(|v| serde_json::from_value::<Program>(v.clone()).ok())
                .map(|mut p| {
                    p.rekey();
                    p
                })
                .unwrap_or_default();
            if let Some((_, a)) = &before {
                kept.mark_gone(&mut findings, a, &run);
            }
            let changed = kept.take(&name, &created, &run.observations, &findings);
            let changes = match serde_json::to_value(&kept) {
                Ok(v) if changed => vec![(key.clone(), crate::index::Change::Findings(Some(v)))],
                _ => Vec::new(),
            };
            (changes, ())
        })
        .err()
        .map(|e| e.to_string())
    } else {
        None
    };
    // what the run's wall time moved by is said against the program's own
    // noise, read from its earlier runs of this command
    let noise = change
        .as_ref()
        .and_then(|_| noise_ms(dir, &key, header.created, path, command));
    let currency = run.profile.cost.currency.clone();
    for f in &mut findings {
        say_saved(f, noise, &currency);
    }
    let change = change.map(|mut c| {
        c.noise_ms = noise;
        c.findings = findings.clone();
        c.appeared = said(&findings, &[State::New]);
        c.resolved = said(&findings, &[State::Fixed]);
        c.same_program = true;
        c
    });
    Some(SinceLast {
        program: key,
        failed: None,
        empty: false,
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
        f.within_noise = false;
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
