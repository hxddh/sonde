//! From requests to operations.
//!
//! A recorded run is grouped into [`Episode`]s: a multipart upload, a
//! retried request, a burst of lookups, a paged listing, or a step of a
//! storage format — a SlateDB manifest or WAL write, an Iceberg, Delta or
//! Hudi commit, a Paimon snapshot — with the reads and file writes that led
//! to it.
//!
//! The model per format is one idea: some writes are *commit points*. Other
//! requests of the format wait in a buffer and are folded into the next commit
//! point; a buffer that sits idle is flushed on its own (reads, or files
//! written without a commit — which [`observations`] reports).

use chrono::{DateTime, Duration, Utc};
use reqlog::Record;
use serde::Serialize;

use crate::phrase::{Phrase, p, p_shared, plural};
use crate::summary::is_miss;
use sonde_proxy::part::{Format, Part, part_of};

/// The format a listing belongs to, from its prefix (`…/manifest/`,
/// `…/_delta_log/`, …), with the directory's name.
pub fn listed_format(prefix: &str) -> Option<(Format, &str)> {
    let trimmed = prefix.trim_end_matches('/');
    if let Some(p) = part_of(trimmed) {
        // a listing of a file path: a client asking whether it is a directory
        return Some((p.format(), parent(trimmed).rsplit('/').next().unwrap_or("")));
    }
    let dir = trimmed.rsplit('/').next()?;
    let f = match dir {
        "manifest" | "wal" | "compacted" | "compactions" | "checkpoints" => Format::SlateDB,
        "_delta_log" => Format::Delta,
        "metadata" => Format::Iceberg,
        "_versions" | "_transactions" | "_indices" | "_deletions" => Format::Lance,
        ".hoodie" => Format::Hudi,
        "snapshot" => Format::Paimon,
        _ => return None,
    };
    Some((f, dir))
}

/// The tables a run touched the log of: a `_delta_log/` key or listing
/// (Delta), a `metadata/` file or listing (Iceberg). A Parquet file's name
/// alone says nothing: Spark, Hive, Hudi and DuckDB write `part-*.parquet`
/// and `…/data/*.parquet` too.
#[derive(Default)]
struct Tables {
    /// (bucket, format, the table's root with its trailing slash; empty at the bucket's root)
    roots: Vec<(Option<String>, Format, String)>,
}

/// The table root a log key or log listing names: what precedes `/<dir>/`.
fn root_before(path: &str, dir: &str) -> Option<String> {
    if path == dir || path.starts_with(&format!("{dir}/")) {
        return Some(String::new());
    }
    let at = path.find(&format!("/{dir}/")).or_else(|| {
        path.ends_with(&format!("/{dir}"))
            .then(|| path.len() - dir.len() - 1)
    })?;
    Some(path[..=at].to_string())
}

impl Tables {
    fn of(records: &[&Record]) -> Self {
        let mut roots = Vec::new();
        for r in records {
            let listed = r
                .kind
                .starts_with("ListObjects")
                .then(|| query(r, "prefix").unwrap_or("").trim_end_matches('/'));
            let Some(path) = r.key.as_deref().or(listed) else {
                continue;
            };
            let part = part_of(path);
            if part.as_ref().is_some_and(Part::is_data) {
                continue;
            }
            // a listing of the directory itself has no part, a key in it does
            let format = match (&part, listed) {
                (Some(p), _) => Some(p.format()),
                (None, Some(prefix)) => listed_format(prefix).map(|(f, _)| f),
                (None, None) => None,
            };
            let dir = match (format, &part) {
                (Some(Format::Delta), _) => "_delta_log",
                (Some(Format::Iceberg), _) => "metadata",
                (Some(Format::Hudi), _) => ".hoodie",
                (Some(Format::Paimon), Some(Part::PaimonManifest)) => "manifest",
                (Some(Format::Paimon), Some(Part::PaimonSchema(_))) => "schema",
                (Some(Format::Paimon), _) => "snapshot",
                _ => continue,
            };
            if let Some(root) = root_before(path, dir) {
                let entry = (
                    r.bucket.as_deref().map(String::from),
                    format.expect("matched above"),
                    root,
                );
                if !roots.contains(&entry) {
                    roots.push(entry);
                }
            }
        }
        Tables { roots }
    }

    /// The part a request's key names, when the run gives a reason to
    /// believe it: a data file counts only beside its table's log.
    fn part(&self, r: &Record) -> Option<Part> {
        let key = r.key.as_deref()?;
        let part = part_of(key)?;
        if part.is_data() {
            self.data_part(r.bucket.as_deref(), key)
        } else {
            Some(part)
        }
    }

    /// Whose data file a Parquet file is: the table's whose log the run
    /// touched, the deepest when one table is under another. The path alone
    /// does not say: a Delta table under `…/data/` has its files there too.
    fn data_part(&self, bucket: Option<&str>, key: &str) -> Option<Part> {
        self.roots
            .iter()
            .filter(|(b, _, root)| b.as_deref() == bucket && key.starts_with(root.as_str()))
            .max_by_key(|(_, _, root)| root.len())
            .and_then(|(_, f, _)| match f {
                Format::Iceberg => Some(Part::IcebergData),
                Format::Delta => Some(Part::DeltaData),
                Format::Hudi => Some(Part::HudiData),
                Format::Paimon => Some(Part::PaimonData),
                _ => None,
            })
    }

    /// The format of a request: from its key, or from a listing's prefix.
    fn format_of(&self, r: &Record) -> Option<Format> {
        if r.key.is_some() {
            return self.part(r).map(|p| p.format());
        }
        if !r.kind.starts_with("ListObjects") {
            return None;
        }
        let prefix = query(r, "prefix").unwrap_or("");
        let trimmed = prefix.trim_end_matches('/');
        match part_of(trimmed) {
            // a listing of a data file's path is of a table only when the table is
            Some(p) if p.is_data() => self
                .data_part(r.bucket.as_deref(), trimmed)
                .map(|p| p.format()),
            _ => listed_format(prefix).map(|(f, _)| f),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Shape {
    /// One request that is nothing more.
    Request,
    Multipart,
    Retry,
    /// Lookups that found nothing.
    Probe,
    /// A listing over several pages.
    Listing,
    /// A step of a storage format.
    Step,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "detail")]
pub enum Outcome {
    Ok,
    /// The step failed: the last request's status and S3 code.
    Failed(String),
    /// The proxy's policy refused it.
    Refused,
}

/// One operation: requests that belong together. Its title is its phrases
/// ([`Episode::title_text`], and [`Episode::title`] the English of them),
/// made when they are asked for: a run may have a million operations,
/// most of them one request, and each is held in 88 bytes ([`Ids`] and
/// [`Said::Request`] hold one request's in place).
#[derive(Debug, Clone, PartialEq)]
pub struct Episode {
    pub shape: Shape,
    pub format: Option<Format>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Record ids, in time order.
    pub records: Ids,
    pub bytes_up: u64,
    pub bytes_down: u64,
    /// Its lookups (GET, HEAD) answered 404: not errors, and worth telling apart.
    pub not_found: u32,
    said: Said,
}

/// What an operation says: its title, its notes and how it ended.
#[derive(Debug, Clone, PartialEq)]
enum Said {
    /// One request that went through and says nothing more: `{op}
    /// {bucket}/{key}`, of its record's own names, and the size it wrote
    /// (the operation's bytes up) when `sized`.
    Request {
        op: reqlog::Name,
        bucket: Option<reqlog::Name>,
        key: Option<reqlog::Text>,
        sized: bool,
    },
    /// Any other, behind a pointer.
    More(Box<More>),
}

#[derive(Debug, Clone, PartialEq)]
struct More {
    title: Box<[Phrase]>,
    notes: Vec<Phrase>,
    outcome: Outcome,
}

/// An operation's record ids: one is held in place, more in a list of their own.
#[derive(Clone, PartialEq, Eq)]
pub enum Ids {
    One(u64),
    Many(Box<[u64]>),
}

impl std::ops::Deref for Ids {
    type Target = [u64];
    fn deref(&self) -> &[u64] {
        match self {
            Ids::One(id) => std::slice::from_ref(id),
            Ids::Many(ids) => ids,
        }
    }
}

impl From<Vec<u64>> for Ids {
    fn from(ids: Vec<u64>) -> Self {
        match ids[..] {
            [id] => Ids::One(id),
            _ => Ids::Many(ids.into()),
        }
    }
}

impl<'a> IntoIterator for &'a Ids {
    type Item = &'a u64;
    type IntoIter = std::slice::Iter<'a, u64>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<const N: usize> PartialEq<[u64; N]> for Ids {
    fn eq(&self, other: &[u64; N]) -> bool {
        **self == other[..]
    }
}

impl PartialEq<Vec<u64>> for Ids {
    fn eq(&self, other: &Vec<u64>) -> bool {
        **self == other[..]
    }
}

impl std::fmt::Debug for Ids {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl Serialize for Ids {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(s)
    }
}

impl Serialize for Episode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        /// The operation as it is written: its title and notes in English too.
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Written<'a> {
            shape: &'a Shape,
            format: &'a Option<Format>,
            title: String,
            start: &'a DateTime<Utc>,
            end: &'a DateTime<Utc>,
            records: &'a [u64],
            bytes_up: u64,
            bytes_down: u64,
            outcome: &'a Outcome,
            notes: Vec<String>,
            not_found: u32,
            title_text: Vec<Phrase>,
            notes_text: &'a [Phrase],
        }
        let title_text = self.title_text();
        Written {
            shape: &self.shape,
            format: &self.format,
            title: crate::phrase::en(&title_text),
            start: &self.start,
            end: &self.end,
            records: &self.records,
            bytes_up: self.bytes_up,
            bytes_down: self.bytes_down,
            outcome: self.outcome(),
            notes: self.notes(),
            not_found: self.not_found,
            title_text,
            notes_text: self.notes_text(),
        }
        .serialize(s)
    }
}

impl Episode {
    /// Its title, as phrases.
    pub fn title_text(&self) -> Vec<Phrase> {
        match &self.said {
            Said::Request {
                op,
                bucket,
                key,
                sized,
            } => {
                let mut t = vec![request_phrase(*op, *bucket, key.as_deref())];
                if *sized {
                    t.push(p("size", &[("size", human(self.bytes_up))]));
                }
                t
            }
            Said::More(m) => m.title.to_vec(),
        }
    }

    /// Whether its title says the phrase `key` (asked without making it).
    pub fn title_says(&self, key: &str) -> bool {
        match &self.said {
            Said::Request { .. } => key == "request",
            Said::More(m) => m.title.iter().any(|p| p.key == key),
        }
    }

    /// Its title in English.
    pub fn title(&self) -> String {
        crate::phrase::en(&self.title_text())
    }

    /// How it ended.
    pub fn outcome(&self) -> &Outcome {
        match &self.said {
            Said::Request { .. } => &Outcome::Ok,
            Said::More(m) => &m.outcome,
        }
    }

    /// Its notes, as phrases.
    pub fn notes_text(&self) -> &[Phrase] {
        match &self.said {
            Said::Request { .. } => &[],
            Said::More(m) => &m.notes,
        }
    }

    /// Its notes in English.
    pub fn notes(&self) -> Vec<String> {
        self.notes_text().iter().map(Phrase::en).collect()
    }

    /// What it says, to change: behind its pointer.
    fn more(&mut self) -> &mut More {
        if let Said::Request { .. } = self.said {
            let title = self.title_text().into();
            self.said = Said::More(Box::new(More {
                title,
                notes: Vec::new(),
                outcome: Outcome::Ok,
            }));
        }
        match &mut self.said {
            Said::More(m) => m,
            Said::Request { .. } => unreachable!("made above"),
        }
    }

    /// Adds a note.
    fn note(&mut self, phrase: Phrase) {
        self.more().notes.push(phrase);
    }

    pub fn requests(&self) -> usize {
        self.records.len()
    }

    pub fn millis(&self) -> f64 {
        (self.end - self.start).num_microseconds().unwrap_or(0) as f64 / 1000.0
    }
}

fn end_of(r: &Record) -> DateTime<Utc> {
    r.time + Duration::microseconds(r.latency_us())
}

fn refused(r: &Record) -> bool {
    r.response_headers()
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("x-sonde-proxy"))
}

/// S3's code in an error body, as it stands there (nothing is copied:
/// [`retryable`] asks of every record).
fn s3_code(r: &Record) -> Option<&str> {
    let b = r.rare().error_body.as_deref()?;
    let start = b.find("<Code>")? + 6;
    Some(&b[start..start + b[start..].find("</Code>")?])
}

fn failure(r: &Record) -> Option<String> {
    if r.rare().transport_error.is_some() {
        return Some("no response".into());
    }
    if !r.is_error() || is_miss(r) {
        return None;
    }
    Some(match (r.status, s3_code(r)) {
        (Some(s), Some(c)) => format!("{s} {c}"),
        (Some(s), None) => s.to_string(),
        (None, _) => "no response".into(),
    })
}

fn is_write(r: &Record) -> bool {
    matches!(r.method.as_str(), "PUT" | "POST" | "DELETE")
}

fn header<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.request_headers()
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn response_header<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.response_headers()
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

pub(crate) fn query<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.query()
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn human(b: u64) -> String {
    human_bytes(b)
}

/// A size as it is written everywhere ([`sonde_fmt::bytes`]).
pub(crate) fn human_bytes(b: u64) -> String {
    sonde_fmt::bytes(b)
}

/// Payload bytes a request uploads (not the XML of Create / Complete).
fn data_bytes(r: &Record) -> u64 {
    match r.kind.as_str() {
        "PutObject" | "UploadPart" => r.request_bytes.get().unwrap_or(0),
        _ => 0,
    }
}

/// Files written by requests (keys, once each, in order) and their bytes.
fn written<'a>(rs: &[&'a Record], except: Option<&str>) -> (Vec<&'a str>, u64) {
    let mut keys: Vec<&str> = Vec::new();
    let mut bytes = 0;
    for r in rs
        .iter()
        .filter(|r| matches!(r.method.as_str(), "PUT" | "POST"))
    {
        let Some(k) = r.key.as_deref() else { continue };
        if Some(k) == except {
            continue;
        }
        bytes += data_bytes(r);
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    (keys, bytes)
}

fn file_name(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

fn parent(key: &str) -> &str {
    key.rsplit_once('/').map_or("", |(p, _)| p)
}

/// Builds an episode from records (any order).
fn build(shape: Shape, format: Option<Format>, title: Vec<Phrase>, rs: Vec<&Record>) -> Episode {
    built(shape, format, Said::phrases(title), rs)
}

fn built(shape: Shape, format: Option<Format>, said: Said, mut rs: Vec<&Record>) -> Episode {
    rs.sort_by_key(|r| (r.time, r.id));
    let start = rs.first().map(|r| r.time).unwrap_or_default();
    let end = rs.iter().map(|r| end_of(r)).max().unwrap_or(start);
    let last = rs.last().copied();
    let outcome = match last {
        Some(r) if refused(r) => Outcome::Refused,
        Some(r) => failure(r).map_or(Outcome::Ok, Outcome::Failed),
        None => Outcome::Ok,
    };
    let mut e = Episode {
        shape,
        format,
        said,
        start,
        end,
        records: rs.iter().map(|r| r.id).collect::<Vec<u64>>().into(),
        bytes_up: rs.iter().map(|r| r.request_bytes.get().unwrap_or(0)).sum(),
        bytes_down: rs
            .iter()
            .filter(|r| !r.is_error())
            .map(|r| r.response_bytes.get().unwrap_or(0))
            .sum(),
        not_found: rs.iter().filter(|r| crate::summary::is_miss(r)).count() as u32,
    };
    if outcome != Outcome::Ok {
        e.more().outcome = outcome;
    }
    e
}

impl Said {
    fn phrases(title: Vec<Phrase>) -> Said {
        Said::More(Box::new(More {
            title: title.into(),
            notes: Vec::new(),
            outcome: Outcome::Ok,
        }))
    }
}

/// How long a format's buffer waits for its commit point.
const IDLE: i64 = 2_000;
/// Lookups closer than this belong to one burst.
const BURST: i64 = 250;

/// Groups records into episodes, in time order.
pub fn episodes(records: &[Record]) -> Vec<Episode> {
    let mut sorted: Vec<&Record> = records.iter().collect();
    sorted.sort_by_key(|r| (r.time, r.id));
    let mut used = vec![false; sorted.len()];
    // (room for an operation a request, the most there can be: what is not
    // used is never touched, and given back at the end without a copy)
    let mut out = Vec::with_capacity(records.len());

    // a completed multipart upload of a format's file is one write of it
    let tables = Tables::of(&sorted);
    let mut units = multipart(&sorted, &tables, &mut used, &mut out);
    // a format's write retried is still that write: its attempts go with it to the step
    units.extend(retries(&sorted, &tables, &mut used, &mut out));

    // what is left, in order: format steps, bursts of lookups, listings, single requests
    // (a unit by its place: of the units, or a request on its own)
    enum Item {
        Unit(u32),
        One(u32),
    }
    let unit_of = |item: &Item| -> &[&Record] {
        match *item {
            Item::Unit(u) => &units[u as usize],
            Item::One(i) => std::slice::from_ref(&sorted[i as usize]),
        }
    };
    let mut items: Vec<Item> = (0..units.len() as u32).map(Item::Unit).collect();
    items.extend(
        (0..sorted.len())
            .filter(|i| !used[*i])
            .map(|i| Item::One(i as u32)),
    );
    drop(used);
    items.sort_unstable_by_key(|item| {
        let first = unit_of(item)[0];
        (first.time, first.id)
    });
    let mut buffers: Vec<(Format, Vec<&Record>)> = Vec::new();
    let mut probe: Vec<&Record> = Vec::new();
    let mut listing: Vec<&Record> = Vec::new();
    // requests that are nothing more: most of a large run, made into
    // operations together once the rest is placed
    let mut alone: Vec<&Record> = Vec::new();
    for item in &items {
        let unit = unit_of(item);
        // a unit's last record stands for it: the Complete of an upload
        let r = *unit.last().expect("units are not empty");
        // idle buffers flush before anything newer is placed
        flush_idle(&mut buffers, r.time, &mut out);
        if !probe.is_empty() && (r.time - probe.last().unwrap().time).num_milliseconds() > BURST {
            out.extend(flush_probe(std::mem::take(&mut probe)));
        }
        if let Some(f) = tables.format_of(r) {
            let part = tables.part(r);
            let pos = match buffers.iter().position(|(bf, _)| *bf == f) {
                Some(p) => p,
                None => {
                    buffers.push((f, Vec::new()));
                    buffers.len() - 1
                }
            };
            buffers[pos].1.extend(unit.iter());
            if is_write(r) && part.as_ref().is_some_and(Part::commits) {
                let (_, rs) = buffers.remove(pos);
                out.extend(step(f, rs));
            }
            continue;
        }
        if r.kind == "ListObjectsV2" || r.kind == "ListObjects" {
            let same = listing.last().is_some_and(|l: &&Record| {
                l.bucket == r.bucket && query(l, "prefix") == query(r, "prefix")
            });
            let continued =
                query(r, "continuation-token").is_some() || query(r, "marker").is_some();
            if same && continued {
                listing.push(r);
                continue;
            }
            if !listing.is_empty() {
                out.push(flush_listing(std::mem::take(&mut listing)));
            }
            listing.push(r);
            continue;
        }
        if is_miss(r) {
            probe.push(r);
            continue;
        }
        alone.push(r);
    }
    out.extend(alone.iter().map(|r| single(r)));
    for (f, rs) in buffers {
        out.push(loose(f, rs));
    }
    if !probe.is_empty() {
        out.extend(flush_probe(probe));
    }
    if !listing.is_empty() {
        out.push(flush_listing(listing));
    }
    // (an operation's first request is its own: no two are equal)
    out.sort_unstable_by_key(|e| (e.start, e.records.first().copied()));
    out.shrink_to_fit();
    out
}

fn flush_idle(
    buffers: &mut Vec<(Format, Vec<&Record>)>,
    now: DateTime<Utc>,
    out: &mut Vec<Episode>,
) {
    let mut i = 0;
    while i < buffers.len() {
        let last = buffers[i].1.last().map(|r| r.time).unwrap_or(now);
        if (now - last).num_milliseconds() > IDLE {
            let (f, rs) = buffers.remove(i);
            out.push(loose(f, rs));
        } else {
            i += 1;
        }
    }
}

/// `{op} {bucket}/{key}`: a request's title.
fn request_phrase(op: reqlog::Name, bucket: Option<reqlog::Name>, key: Option<&str>) -> Phrase {
    let target = match (bucket, key) {
        (Some(b), Some(k)) => format!("{b}/{k}"),
        (Some(b), None) => format!("{b}/"),
        _ => "/".into(),
    };
    p_shared(
        "request",
        &[("op", op.as_str().into()), ("target", target.into())],
    )
}

/// A request that is nothing more, an operation of its own: most of a large
/// run, so its title is held as the request's own words
/// ([`Said::Request`]) when they are all it says.
fn single(r: &Record) -> Episode {
    let sized =
        matches!(r.method.as_str(), "PUT" | "POST") && r.request_bytes.get().unwrap_or(0) > 0;
    let mut title = Said::Request {
        op: r.kind,
        bucket: r.bucket,
        key: r.key.clone(),
        sized,
    };
    if let Some(x) = query(r, "prefix").filter(|x| !x.is_empty()) {
        let mut said = vec![
            request_phrase(r.kind, r.bucket, r.key.as_deref()),
            p("request.prefix", &[("prefix", x.to_string())]),
        ];
        if sized {
            said.push(p(
                "size",
                &[("size", human(r.request_bytes.get().unwrap_or(0)))],
            ));
        }
        title = Said::phrases(said);
    }
    let mut e = built(Shape::Request, None, title, vec![r]);
    conditional_note(&mut e, &[r]);
    e
}

fn conditional_note(e: &mut Episode, rs: &[&Record]) {
    if rs.iter().any(|r| header(r, "if-none-match") == Some("*")) {
        e.note(p("cond.create", &[]));
    } else if rs
        .iter()
        .any(|r| is_write(r) && header(r, "if-match").is_some())
    {
        e.note(p("cond.write", &[]));
    }
    if rs.iter().any(|r| r.status == Some(412)) {
        e.note(p("race", &[]));
    }
}

fn flush_probe(rs: Vec<&Record>) -> Vec<Episode> {
    if rs.len() < 3 {
        return rs.into_iter().map(single).collect();
    }
    let bucket = rs[0].bucket.unwrap_or_default();
    let dirs: Vec<&str> = rs
        .iter()
        .map(|r| parent(r.key.as_deref().unwrap_or("")))
        .collect();
    let dir = if dirs.iter().all(|d| *d == dirs[0]) {
        format!("{bucket}/{}/", dirs[0])
    } else {
        format!("{bucket}/")
    };
    let n = rs.len();
    vec![build(
        Shape::Probe,
        None,
        vec![p("probe", &[("n", n.to_string()), ("dir", dir)])],
        rs,
    )]
}

fn flush_listing(rs: Vec<&Record>) -> Episode {
    if rs.len() == 1 {
        return single(rs[0]);
    }
    let prefix = query(rs[0], "prefix").unwrap_or("");
    let bucket = rs[0].bucket.as_deref().unwrap_or_default().to_string();
    let n = rs.len();
    build(
        Shape::Listing,
        None,
        vec![p(
            "listing",
            &[
                ("bucket", bucket),
                ("prefix", prefix.to_string()),
                ("n", n.to_string()),
            ],
        )],
        rs,
    )
}

/// CreateMultipartUpload, its parts and Complete or Abort, by upload id.
/// Completed uploads of a format's files are returned as units (they become
/// writes of a step); every other upload is an episode.
fn multipart<'a>(
    sorted: &[&'a Record],
    tables: &Tables,
    used: &mut [bool],
    out: &mut Vec<Episode>,
) -> Vec<Vec<&'a Record>> {
    let mut units = Vec::new();
    use std::collections::{HashMap, VecDeque};
    // upload id → record indexes; creates wait for the first upload id seen on their key
    let mut groups: Vec<(Option<String>, String, Vec<usize>)> = Vec::new();
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    // by key, the uploads no part has named yet: the one whose creates have
    // all failed so far (a create tried again joins it; there is one at
    // most), and those a create began, oldest first
    #[derive(Default)]
    struct Waiting {
        failed: Option<usize>,
        begun: VecDeque<usize>,
    }
    let mut waiting: HashMap<String, Waiting> = HashMap::new();
    let key_of = |r: &Record| {
        format!(
            "{}/{}",
            r.bucket.as_deref().unwrap_or(""),
            r.key.as_deref().unwrap_or("")
        )
    };
    for (i, r) in sorted.iter().enumerate() {
        match r.kind.as_str() {
            // a Create that failed and was tried again is part of the upload that followed
            "CreateMultipartUpload" => {
                let key = key_of(r);
                let w = waiting.entry(key.clone()).or_default();
                let g = match w.failed.take() {
                    Some(g) => {
                        groups[g].2.push(i);
                        g
                    }
                    None => {
                        groups.push((None, key, vec![i]));
                        groups.len() - 1
                    }
                };
                if r.is_error() {
                    w.failed = Some(g);
                } else {
                    w.begun.push_back(g);
                }
            }
            "UploadPart"
            | "UploadPartCopy"
            | "CompleteMultipartUpload"
            | "AbortMultipartUpload"
            | "ListParts" => {
                let Some(id) = query(r, "uploadId") else {
                    continue;
                };
                let g = match by_id.get(id) {
                    Some(&g) => g,
                    None => {
                        let key = key_of(r);
                        // the upload a successful Create began
                        let pending = waiting.get_mut(&key).and_then(|w| w.begun.pop_front());
                        let g = match pending {
                            Some(g) => g,
                            None => {
                                groups.push((None, key, Vec::new()));
                                groups.len() - 1
                            }
                        };
                        groups[g].0 = Some(id.to_string());
                        by_id.insert(id, g);
                        g
                    }
                };
                groups[g].2.push(i);
            }
            _ => {}
        }
    }
    for (_, key, idx) in groups {
        // Creates that failed and were never tried again began nothing: single requests
        if idx
            .iter()
            .all(|&i| sorted[i].is_error() && sorted[i].kind == "CreateMultipartUpload")
        {
            continue;
        }
        for &i in &idx {
            used[i] = true;
        }
        let rs: Vec<&Record> = idx.iter().map(|&i| sorted[i]).collect();
        let parts = rs
            .iter()
            .filter(|r| r.kind.starts_with("UploadPart"))
            .count();
        let bytes: u64 = rs
            .iter()
            .filter(|r| r.kind == "UploadPart")
            .map(|r| r.request_bytes.get().unwrap_or(0))
            .sum();
        let completed = rs.iter().any(completes);
        let aborted = rs.iter().any(|r| r.kind == "AbortMultipartUpload");
        // parts with no create: begun before the recording (an imported
        // log's window), so whether it completed is not known
        let begun = rs.iter().any(|r| r.kind == "CreateMultipartUpload");
        let formatted = tables.part(rs[0]).is_some();
        if completed && formatted {
            let mut unit = rs;
            unit.sort_by_key(|r| (r.kind == "CompleteMultipartUpload", r.time, r.id));
            units.push(unit);
            continue;
        }
        let mut title = vec![p(
            "mpu",
            &[
                ("key", key),
                ("n", parts.to_string()),
                ("s", plural(parts)),
                ("size", human(bytes)),
            ],
        )];
        if aborted && !completed {
            title.push(p("mpu.aborted", &[]));
        } else if !completed && begun {
            title.push(p("mpu.open", &[]));
        }
        let mut e = build(Shape::Multipart, None, title, rs);
        if !completed && !aborted && begun {
            e.note(p("mpu.billed", &[]));
        }
        out.push(e);
    }
    units
}

/// Whether a request completed its upload in the store: a Complete the
/// store answered well, the program told so or not (a trial that loses the
/// answer says what the store's was, in `x-sonde-store-status`).
fn completes(r: &&Record) -> bool {
    r.kind == "CompleteMultipartUpload"
        && (!r.is_error()
            || response_header(r, "x-sonde-store-status").is_some_and(|s| s.starts_with('2')))
}

/// Whether sonde failed the request in the store's place.
fn sondes_fault(r: &Record) -> bool {
    r.rare().injected.is_some() && response_header(r, "x-sonde-fault").is_some()
}

/// The statuses a retry is owed for: the ones the AWS SDKs send again. A 501
/// or a 505 is the store's answer however often it is asked, like a 404.
/// Asked only through [`retryable`], which reads the code as well.
fn status_owes_retry(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

/// The S3 codes a retry is owed for, whatever status they come with: S3
/// sends `RequestTimeout` as a 400, and `RequestTimeTooSkewed` as a 403 to
/// a request an SDK signs again, its clock set by the answer's `Date`.
const CODES_OWE_RETRY: &[&str] = &["SlowDown", "RequestTimeout", "RequestTimeTooSkewed"];

/// S3's code of an answer: in its error document, or, for an answer sonde
/// gave in the store's place, at the head of what the record keeps
/// (`SlowDown: sonde answered …`).
pub(crate) fn code_of(r: &Record) -> Option<&str> {
    if let Some(c) = s3_code(r) {
        return Some(c);
    }
    let (code, _) = r.rare().error_body.as_deref()?.split_once(": ")?;
    (r.rare().injected.is_some()
        && !code.is_empty()
        && code.chars().all(|c| c.is_ascii_alphanumeric()))
    .then_some(code)
}

/// Whether a client is expected to send the request again: no answer (or
/// one cut short), a status that says so (408, 429, 500, 502, 503, 504), or
/// a code that does ([`CODES_OWE_RETRY`]). The one rule: the trial's
/// verdict, the retries read from a run and the signals all ask this.
pub(crate) fn retryable(r: &Record) -> bool {
    r.rare().transport_error.is_some()
        || r.status.is_some_and(status_owes_retry)
        || code_of(r).is_some_and(|c| CODES_OWE_RETRY.contains(&c))
}

/// A request repeated after a retryable failure. A retried write of a
/// format's file (a commit, a manifest) is returned as a unit, so the step it
/// belongs to keeps it, attempts and all.
fn retries<'a>(
    sorted: &[&'a Record],
    tables: &Tables,
    used: &mut [bool],
    out: &mut Vec<Episode>,
) -> Vec<Vec<&'a Record>> {
    let mut units = Vec::new();
    // a request's identity: what a retry of it repeats
    type Identity<'r> = (
        &'r str,
        Option<&'r str>,
        Option<&'r str>,
        &'r [(reqlog::Text, reqlog::Text)],
    );
    fn identity<'r>(r: &'r Record) -> Identity<'r> {
        (
            r.method.as_str(),
            r.bucket.as_deref(),
            r.key.as_deref(),
            r.query(),
        )
    }
    // the records of each identity a retryable failure has, in time order: a
    // failure looks for its retry among them, not among every later request
    // (an outage is thousands of failures, each of a key of its own)
    let mut of: std::collections::HashMap<Identity, Vec<usize>> = Default::default();
    for (i, r) in sorted.iter().enumerate() {
        if !used[i] && retryable(r) {
            of.entry(identity(r)).or_default();
        }
    }
    if of.is_empty() {
        return units;
    }
    for (i, r) in sorted.iter().enumerate() {
        if let Some(same) = of.get_mut(&identity(r)) {
            same.push(i);
        }
    }
    for i in 0..sorted.len() {
        if used[i] || !retryable(sorted[i]) {
            continue;
        }
        let same = &of[&identity(sorted[i])];
        let mut chain = vec![i];
        for &j in &same[same.partition_point(|&j| j <= i)..] {
            let last = sorted[*chain.last().unwrap()];
            if (sorted[j].time - last.time).num_seconds() > 60 {
                break;
            }
            if !used[j] {
                chain.push(j);
                if !retryable(sorted[j]) {
                    break;
                }
            }
        }
        if chain.len() > 1 {
            for &c in &chain {
                used[c] = true;
            }
            let rs: Vec<&Record> = chain.iter().map(|&c| sorted[c]).collect();
            let last = *rs.last().expect("a chain has records");
            if tables.format_of(last).is_some() && is_write(last) && !retryable(last) {
                units.push(rs);
                continue;
            }
            let first = failure(rs[0]).unwrap_or_default();
            let n = rs.len() - 1;
            let target = rs[0].key.as_deref().unwrap_or_default().to_string();
            let mut e = build(
                Shape::Retry,
                None,
                vec![p(
                    "retry",
                    &[
                        ("op", rs[0].kind.to_string()),
                        ("target", target),
                        ("n", n.to_string()),
                        ("first", first),
                    ],
                )],
                rs,
            );
            let wait = e.millis();
            e.note(p("retry.wait", &[("ms", format!("{wait:.0}"))]));
            out.push(e);
        }
    }
    units
}

/// The directory of a request: its key's parent, or a listing's prefix.
fn dir_of(r: &Record) -> String {
    match &r.key {
        Some(k) => parent(k).to_string(),
        None => query(r, "prefix")
            .unwrap_or("")
            .trim_end_matches('/')
            .to_string(),
    }
}

/// A commit point with what led to it. Bursts of lookups in another
/// directory (SlateDB probing for the newest WAL while writing a manifest)
/// are steps of their own.
fn step(f: Format, rs: Vec<&Record>) -> Vec<Episode> {
    let commit = *rs.last().expect("a step has its commit");
    let home = dir_of(commit);
    let mut by_dir: Vec<(String, Vec<&Record>)> = Vec::new();
    for r in &rs[..rs.len() - 1] {
        let d = dir_of(r);
        match by_dir.iter_mut().find(|(x, _)| *x == d) {
            Some((_, v)) => v.push(r),
            None => by_dir.push((d, vec![r])),
        }
    }
    let mut out = Vec::new();
    let mut kept: Vec<&Record> = Vec::new();
    for (d, group) in by_dir {
        // a search: misses on 3+ different numbered files (WAL ids, commits)
        let mut ids: Vec<u64> = group
            .iter()
            .filter(|r| is_miss(r))
            .filter_map(|r| match r.key.as_deref().and_then(part_of) {
                Some(Part::SlateWal(n) | Part::SlateManifest(n) | Part::DeltaCommit(n)) => Some(n),
                _ => None,
            })
            .collect();
        ids.sort_unstable();
        ids.dedup();
        if d != home && ids.len() >= 3 && group.iter().all(|r| !is_write(r)) {
            out.push(loose(f, group));
        } else {
            kept.extend(group);
        }
    }
    kept.push(commit);
    out.push(commit_step(f, kept));
    out
}

fn commit_step(f: Format, mut rs: Vec<&Record>) -> Episode {
    rs.sort_by_key(|r| (r.time, r.id));
    let commit = *rs
        .iter()
        .rev()
        .find(|r| {
            is_write(r)
                && r.key
                    .as_deref()
                    .and_then(part_of)
                    .is_some_and(|p| p.commits())
        })
        .expect("a step has its commit");
    let part = commit
        .key
        .as_deref()
        .and_then(part_of)
        .expect("a commit has a part");
    let commit_key = commit.key.as_deref();
    // the commit file and how it was written (one PUT, or an upload's parts)
    let size: u64 = rs
        .iter()
        .filter(|r| r.key.as_deref() == commit_key)
        .map(|r| data_bytes(r))
        .sum();
    let before: Vec<&Record> = rs
        .iter()
        .copied()
        .filter(|r| r.key.as_deref() != commit_key || !is_write(r))
        .filter(|r| r.id != commit.id)
        .collect();
    let deleted = commit.method == "DELETE";
    let mut title = vec![match (&part, deleted) {
        (_, true) => p(
            "step.deleted",
            &[(
                "file",
                file_name(commit.key.as_deref().unwrap_or("")).to_string(),
            )],
        ),
        (Part::SlateManifest(n), _) => p("slate.manifest", &[("n", n.to_string())]),
        (Part::SlateWal(n), _) if size == 0 => p("slate.wal.empty", &[("n", n.to_string())]),
        (Part::SlateWal(n), _) => p("slate.wal", &[("n", n.to_string()), ("size", human(size))]),
        (Part::SlateSst, _) => p("slate.sst", &[("size", human(size))]),
        (Part::SlateCompactions(n), _) => p("slate.compactions", &[("n", n.to_string())]),
        (Part::IcebergMetadata(Some(v)), _) => p("iceberg.metadata.v", &[("v", v.to_string())]),
        (Part::IcebergMetadata(None), _) | (Part::IcebergVersionHint, _) => {
            p("iceberg.metadata", &[])
        }
        (Part::DeltaCommit(n), _) => p("delta.commit", &[("n", n.to_string())]),
        (Part::DeltaCheckpoint, _) => p("delta.checkpoint", &[]),
        (Part::DeltaLastCheckpoint, _) => p("delta.last", &[]),
        (Part::LanceManifest(v), _) => p("lance.commit", &[("v", v.to_string())]),
        (
            Part::HudiInstant {
                instant,
                action,
                metadata,
                ..
            },
            _,
        ) => p(
            if *metadata {
                "hudi.metadata"
            } else {
                "hudi.instant"
            },
            &[("action", action.clone()), ("instant", instant.clone())],
        ),
        (Part::HudiProperties, _) => p("hudi.properties", &[]),
        (Part::PaimonSnapshot(n), _) => p("paimon.snapshot", &[("n", n.to_string())]),
        (Part::PaimonHint, _) => p(
            "paimon.hint",
            &[(
                "file",
                file_name(commit.key.as_deref().unwrap_or("")).to_string(),
            )],
        ),
        (Part::PaimonSchema(n), _) => p("paimon.schema", &[("n", n.to_string())]),
        _ => p(
            "step.other",
            &[
                ("op", commit.kind.to_string()),
                ("key", commit.key.as_deref().unwrap_or_default().to_string()),
            ],
        ),
    }];
    // files written on the way to the commit (data files, manifests)
    let (files, bytes) = written(&before, commit_key);
    let listed = before
        .iter()
        .filter(|r| r.key.is_none())
        .filter_map(|r| listed_format(query(r, "prefix").unwrap_or("")).map(|(_, d)| d))
        .next();
    if f == Format::Lance {
        // what the commit brought, by what its files are (the transaction
        // file says it too, but its body is not kept)
        let of = |want: fn(&Part) -> bool| {
            let mut keys: Vec<&str> = before
                .iter()
                .filter(|r| is_write(r) && !r.is_error())
                .filter(|r| r.key.as_deref().and_then(part_of).is_some_and(|p| want(&p)))
                .filter_map(|r| r.key.as_deref())
                .collect();
            keys.sort_unstable();
            keys.dedup();
            let bytes: u64 = before
                .iter()
                .filter(|r| is_write(r) && r.key.as_deref().is_some_and(|k| keys.contains(&k)))
                .map(|r| data_bytes(r))
                .sum();
            (keys.len(), bytes)
        };
        let (data, data_bytes) = of(|p| matches!(p, Part::LanceData));
        let (index, index_bytes) = of(|p| matches!(p, Part::LanceIndex));
        let (deletions, _) = of(|p| matches!(p, Part::LanceDeletion));
        if data > 0 {
            title.push(p(
                "lance.data",
                &[
                    ("n", data.to_string()),
                    ("s", plural(data)),
                    ("size", human(data_bytes)),
                ],
            ));
        }
        if index > 0 {
            title.push(p("lance.index", &[("size", human(index_bytes))]));
        }
        if deletions > 0 {
            title.push(p("lance.deletions", &[]));
        }
    } else if matches!(f, Format::Hudi | Format::Paimon) {
        // the data files alone: an instant's own states, markers and
        // manifests are how it commits, not what it brought
        let data: Vec<&str> = files
            .iter()
            .copied()
            .filter(|k| part_of(k).is_some_and(|p| p.is_data()))
            .collect();
        let bytes: u64 = before
            .iter()
            .filter(|r| r.key.as_deref().is_some_and(|k| data.contains(&k)))
            .map(|r| data_bytes(r))
            .sum();
        if !data.is_empty() {
            title.push(p(
                "step.files",
                &[
                    ("n", data.len().to_string()),
                    ("s", plural(data.len())),
                    ("size", human(bytes)),
                ],
            ));
        }
    } else if !files.is_empty() {
        title.push(p(
            "step.files",
            &[
                ("n", files.len().to_string()),
                ("s", plural(files.len())),
                ("size", human(bytes)),
            ],
        ));
    }
    let mut e = build(Shape::Step, Some(f), title, rs.clone());
    conditional_note(&mut e, &[commit]);
    if let Some(d) = listed {
        e.note(p("after.listing", &[("dir", d.to_string())]));
    }
    let lookups = before
        .iter()
        .filter(|r| !is_write(r) && r.key.is_some())
        .count();
    if lookups > 0 {
        let misses = before.iter().filter(|r| is_miss(r)).count();
        let n = lookups.to_string();
        e.note(match misses {
            0 => p("after.reads", &[("n", n), ("s", plural(lookups))]),
            m if m == lookups => p("after.misses", &[("n", n), ("s", plural(lookups))]),
            m => p("after.mixed", &[("n", n), ("m", m.to_string())]),
        });
    }
    e
}

/// A format's requests with no commit point: reads, or files left uncommitted.
fn loose(f: Format, rs: Vec<&Record>) -> Episode {
    let (writes, bytes) = written(&rs, None);
    if !writes.is_empty() {
        return build(
            Shape::Step,
            Some(f),
            vec![p(
                "loose.uncommitted",
                &[
                    ("n", writes.len().to_string()),
                    ("s", plural(writes.len())),
                    ("size", human(bytes)),
                ],
            )],
            rs,
        );
    }
    let misses = rs.iter().filter(|r| is_miss(r)).count();
    let n = rs.len();
    // what a commit leaves to clear away afterwards (Hudi's markers)
    if rs
        .iter()
        .all(|r| r.method == "DELETE" || r.kind == "DeleteObjects")
    {
        return build(
            Shape::Step,
            Some(f),
            vec![p(
                "loose.deletes",
                &[("n", n.to_string()), ("s", plural(n))],
            )],
            rs,
        );
    }
    let wal_probe: Vec<u64> = rs
        .iter()
        .filter(|r| is_miss(r))
        .filter_map(|r| match r.key.as_deref().and_then(part_of) {
            Some(Part::SlateWal(n)) => Some(n),
            _ => None,
        })
        .collect();
    let lists: Vec<&str> = rs
        .iter()
        .filter(|r| r.key.is_none())
        .filter_map(|r| listed_format(query(r, "prefix").unwrap_or("")).map(|(_, d)| d))
        .collect();
    // a Lance read: ranges of its data and index files, after the version was found
    let lance_reads: Vec<&&Record> = rs
        .iter()
        .filter(|r| r.method == "GET" && !r.is_error())
        .filter(|r| {
            matches!(
                r.key.as_deref().and_then(part_of),
                Some(Part::LanceData | Part::LanceIndex | Part::LanceDeletion)
            )
        })
        .collect();
    let title = if f == Format::Lance && !lance_reads.is_empty() {
        let mut files: Vec<&str> = lance_reads
            .iter()
            .filter_map(|r| r.key.as_deref())
            .collect();
        files.sort_unstable();
        files.dedup();
        let bytes: u64 = lance_reads
            .iter()
            .map(|r| r.response_bytes.get().unwrap_or(0))
            .sum();
        let version = rs
            .iter()
            .rev()
            .find_map(|r| match r.key.as_deref().and_then(part_of) {
                Some(Part::LanceManifest(v)) if r.method == "GET" && !r.is_error() => Some(v),
                _ => None,
            });
        let mut t = vec![match version {
            Some(v) => p("lance.read.v", &[("v", v.to_string())]),
            None => p("lance.read", &[]),
        }];
        t.push(p(
            "lance.ranges",
            &[
                ("n", lance_reads.len().to_string()),
                ("files", files.len().to_string()),
                ("s", plural(files.len())),
                ("size", human(bytes)),
            ],
        ));
        t
    } else if wal_probe.len() >= 3 {
        let lo = wal_probe.iter().min().unwrap();
        let hi = wal_probe.iter().max().unwrap();
        vec![p(
            "slate.walprobe",
            &[("lo", lo.to_string()), ("hi", hi.to_string())],
        )]
    } else if let Some(r) = rs
        .iter()
        .rev()
        .find(|r| !r.is_error() && r.method == "GET" && r.key.is_some())
    {
        let what = match r.key.as_deref().and_then(part_of) {
            Some(Part::SlateManifest(m)) => p("read.manifest", &[("n", m.to_string())]),
            Some(Part::SlateCompactions(c)) => p("read.compactions", &[("n", c.to_string())]),
            Some(Part::IcebergMetadata(Some(v))) => p("read.metadata", &[("v", v.to_string())]),
            Some(Part::DeltaCommit(c)) => p("read.commit", &[("n", c.to_string())]),
            Some(Part::LanceManifest(v)) => p("lance.read.v", &[("v", v.to_string())]),
            _ => p(
                "read.file",
                &[(
                    "file",
                    file_name(r.key.as_deref().unwrap_or("")).to_string(),
                )],
            ),
        };
        if n > 1 {
            vec![what, p("read.count", &[("n", n.to_string())])]
        } else {
            vec![what]
        }
    } else if !lists.is_empty() && lists.len() == n {
        vec![p("loose.listed", &[("dir", lists[0].to_string())])]
    } else if misses == n {
        vec![p("loose.misses", &[("n", n.to_string()), ("s", plural(n))])]
    } else {
        vec![p("loose.reads", &[("n", n.to_string())])]
    };
    build(Shape::Step, Some(f), title, rs)
}

/// Something worth knowing about a run.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    /// `warn` (likely a problem) or `info`.
    pub level: &'static str,
    pub text: String,
    /// The text as phrases, for saying it in other languages.
    pub phrases: Vec<Phrase>,
    /// The operations it is about (indexes, ascending), for showing them.
    #[serde(rename = "operations")]
    pub episodes: Vec<usize>,
    /// The requests it bears on: findings are ranked by it, warnings first.
    pub impact: usize,
    /// What acting on it would save, when that can be counted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saving: Option<Phrase>,
    /// The saving in English.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saving_text: Option<String>,
    /// The saving, counted: requests acting on it would not send …
    #[serde(default, skip_serializing_if = "is_zero")]
    pub saves_requests: usize,
    /// … and time the program would not wait, in ms.
    #[serde(default, skip_serializing_if = "is_zero_ms")]
    pub saves_ms: f64,
    /// … and bytes it would not read out of the store.
    #[serde(default, skip_serializing_if = "is_zero_bytes")]
    pub saves_bytes: u64,
    /// The requests acting on it would not send, by id: what "acting on N
    /// findings" takes away (their own waiting, their own price).
    #[serde(skip)]
    pub saved_ids: Vec<u64>,
    /// The requests acting on it keeps in place of those it saves (a cache's
    /// first read of each stretch): another finding's saving does not take
    /// them away too ([`crate::fixes::what_if`]).
    #[serde(skip)]
    pub kept_ids: Vec<u64>,
    /// The requests it saves move their bytes still, in fewer and larger
    /// ones (small objects read from shards): acting on it saves their
    /// price as requests, not their bytes out nor its egress.
    #[serde(skip)]
    pub keeps_bytes: bool,
    /// The requests a saving of time alone is about (those sent one after
    /// another), by id.
    #[serde(skip)]
    pub timed_ids: Vec<u64>,
    /// What to change in the program, by SDK.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<crate::fixes::Fix>,
    /// Where in the program its requests were sent from, most first, when
    /// the program says (`x-sonde-caller`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub callers: Vec<sonde_proxy::callers::Caller>,
    /// The SDK that sent them, from their user agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk: Option<String>,
    /// The same finding across runs (`sonde.toml`'s `expect` names it so).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity: String,
    /// What kind of finding it is, as its identity begins (`obs.headget`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// What the requests it would save cost, in the run's currency.
    #[serde(default, skip_serializing_if = "is_zero_ms")]
    pub saves_cost: f64,
    /// Why the project keeps it on purpose (`sonde.toml`'s `[check]
    /// expect`): leave it ([`crate::program::mark_expected`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
}

/// `n` of `total` as a whole percent, rounded: one rule for every share a
/// finding says (29 of 1,500 is 2%, as the 30 reads they are of). Never 0%
/// for some, nor 100% for less than all.
pub(crate) fn share_pct(n: usize, total: usize) -> usize {
    let total = total.max(1);
    let pct = (n * 100 + total / 2) / total;
    match n {
        0 => 0,
        _ if n < total => pct.clamp(1, 99),
        _ => pct,
    }
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

fn is_zero_bytes(n: &u64) -> bool {
    *n == 0
}

fn is_zero_ms(n: &f64) -> bool {
    *n == 0.0
}

impl Observation {
    /// Names the requests acting on it would not send.
    fn saving_of(mut self, ids: Vec<u64>) -> Self {
        self.saved_ids = ids;
        self
    }

    /// Names the requests acting on it keeps in their place.
    fn keeping(mut self, ids: Vec<u64>) -> Self {
        self.kept_ids = ids;
        self
    }

    /// Says the requests it saves move their bytes still, in fewer ones.
    fn keeping_bytes(mut self) -> Self {
        self.keeps_bytes = true;
        self
    }

    /// Weighs a finding: the requests it bears on and those acting on it would
    /// save (out of `total`).
    fn weigh(mut self, impact: usize, saves: usize, total: usize) -> Self {
        self.impact = impact;
        self.saves_requests = saves;
        if saves > 0 {
            let saving = p(
                "obs.saves",
                &[
                    ("n", saves.to_string()),
                    ("s", plural(saves)),
                    ("pct", share_pct(saves, total).to_string()),
                ],
            );
            self.saving_text = Some(saving.en());
            self.saving = Some(saving);
        }
        self
    }
}

/// Notes that hold for (nearly) every step of a format: said once for the
/// run rather than on each line. Returns (format, note, steps with it,
/// steps of the format).
pub fn shared_notes(episodes: &[Episode]) -> Vec<(Format, String, usize, usize)> {
    let mut out = Vec::new();
    for f in [
        Format::SlateDB,
        Format::Iceberg,
        Format::Delta,
        Format::Lance,
        Format::Hudi,
        Format::Paimon,
    ] {
        let of_f: Vec<&Episode> = episodes.iter().filter(|e| e.format == Some(f)).collect();
        let each: Vec<Vec<String>> = of_f.iter().map(|e| e.notes()).collect();
        let mut notes: Vec<&String> = each.iter().flatten().collect();
        notes.sort();
        notes.dedup();
        // writes only: reads never carry a write's note
        let writes = each.iter().filter(|n| !n.is_empty()).count();
        for n in notes {
            let with = each.iter().filter(|e| e.contains(n)).count();
            if with >= 3 && with * 10 >= writes * 8 && !n.starts_with("after ") {
                out.push((f, n.clone(), with, of_f.len()));
            }
        }
    }
    out
}

/// Something for each request, by its id. A run's ids are 1 to n (or
/// near it): a table by id then, with no hashing; a map when they are not.
pub(crate) enum ById<V> {
    Table(u64, Vec<Option<V>>),
    Map(std::collections::HashMap<u64, V>),
}

/// Which operation each request belongs to, by its id (a request two
/// operations claim is the later's): a table when the ids are about 1 to n
/// (a recording's), sorted pairs when not (a map's buckets would cost twice
/// as much a request).
pub(crate) enum Owners {
    Table { first: u64, of: Vec<u32> },
    Pairs(Vec<(u64, u32)>),
}

impl Owners {
    pub(crate) fn of(episodes: &[Episode]) -> Owners {
        let ids = || {
            episodes
                .iter()
                .enumerate()
                .flat_map(|(i, e)| e.records.iter().map(move |id| (*id, i as u32)))
        };
        let (n, first, last) = ids().fold((0u64, u64::MAX, 0), |(n, lo, hi), (id, _)| {
            (n + 1, lo.min(id), hi.max(id))
        });
        if n > 0 && last - first <= 2 * n {
            let mut of = vec![u32::MAX; (last - first + 1) as usize];
            for (id, i) in ids() {
                of[(id - first) as usize] = i;
            }
            return Owners::Table { first, of };
        }
        let mut pairs: Vec<(u64, u32)> = ids().collect();
        pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        pairs.dedup_by_key(|(id, _)| *id);
        Owners::Pairs(pairs)
    }

    pub(crate) fn get(&self, id: u64) -> Option<u32> {
        match self {
            Owners::Table { first, of } => of
                .get(id.checked_sub(*first)? as usize)
                .copied()
                .filter(|i| *i != u32::MAX),
            Owners::Pairs(pairs) => {
                let at = pairs.binary_search_by_key(&id, |(id, _)| *id).ok()?;
                Some(pairs[at].1)
            }
        }
    }
}

/// Each record by its id: looked up in place when the records are in the
/// order of their ids (a recording's are), else through a [`ById`] of places.
pub(crate) enum RecordsById<'a> {
    InOrder(&'a [Record]),
    Table(&'a [Record], ById<u32>),
}

impl<'a> RecordsById<'a> {
    pub(crate) fn of(records: &'a [Record]) -> RecordsById<'a> {
        if records.is_sorted_by(|a, b| a.id < b.id) {
            return RecordsById::InOrder(records);
        }
        RecordsById::Table(
            records,
            ById::of(records.len(), || {
                records.iter().enumerate().map(|(i, r)| (r.id, i as u32))
            }),
        )
    }

    /// Its place in the records.
    pub(crate) fn place(&self, id: &u64) -> Option<usize> {
        match self {
            RecordsById::InOrder(records) => records.binary_search_by_key(id, |r| r.id).ok(),
            RecordsById::Table(_, t) => t.get(id).map(|&i| i as usize),
        }
    }

    pub(crate) fn get(&self, id: &u64) -> Option<&'a Record> {
        let records = match self {
            RecordsById::InOrder(r) | RecordsById::Table(r, _) => *r,
        };
        self.place(id).map(|i| &records[i])
    }
}

impl<V> ById<V> {
    /// Of `n` ids or so, given twice over (once to see how they lie).
    fn of<I: Iterator<Item = (u64, V)>>(n: usize, pairs: impl Fn() -> I) -> ById<V> {
        let (mut low, mut high) = (u64::MAX, 0);
        for (id, _) in pairs() {
            (low, high) = (low.min(id), high.max(id));
        }
        if low > high {
            return ById::Table(0, Vec::new());
        }
        let span = high - low;
        if span > 4 * n as u64 + 1024 {
            return ById::Map(pairs().collect());
        }
        let mut table: Vec<Option<V>> = Vec::new();
        table.resize_with(span as usize + 1, || None);
        for (id, v) in pairs() {
            table[(id - low) as usize] = Some(v);
        }
        ById::Table(low, table)
    }

    fn get(&self, id: &u64) -> Option<&V> {
        match self {
            ById::Table(low, table) => table.get(id.checked_sub(*low)? as usize)?.as_ref(),
            ById::Map(map) => map.get(id),
        }
    }
}

/// What stands out across a run's requests and episodes.
/// The share of a run's requests throttled from which throttling is a
/// warning even when every throttled request was retried and got through:
/// the retries alone then add as many requests as `sonde check` lets a run
/// grow past its baseline by default ([`crate::program::DEFAULT_TOLERANCE`]).
pub const THROTTLED_SHARE: f64 = crate::program::DEFAULT_TOLERANCE;

/// The size under which an object written alone is a small file: written
/// together, 64 KiB or more is what one PUT's price is worth.
pub const SMALL: u64 = 64 * 1024;

/// The size under which an object read whole is a small read: an image, a
/// sample, a record (`obs.smallreads`).
pub const SMALL_READ: u64 = 1024 * 1024;

/// The size small objects are sharded into (a tar, a WebDataset shard):
/// what reading them takes, one read a shard.
pub const SHARD: u64 = 64 * 1024 * 1024;

/// The size from which an object read whole that is also read by range
/// is a whole read (`obs.wholeread`); 16 of it saved is a finding.
pub const WHOLE_READ: u64 = 1024 * 1024;

/// The prefixes listed one by one that are a crawl (`obs.crawl`).
pub const CRAWL: usize = 20;

/// A key's folder: all of it to its last `/` (`""` at the top).
fn folder_of(key: &str) -> &str {
    &key[..key.rfind('/').map_or(0, |i| i + 1)]
}

/// The folder `place` and every place before it (`under`) are in: what a
/// finding about many objects is about.
fn common_folder(under: Option<String>, place: &str) -> String {
    match under {
        None => folder_of(place).to_string(),
        Some(u) => {
            let same = u
                .char_indices()
                .zip(place.chars())
                .take_while(|((_, x), y)| x == y)
                .last()
                .map_or(0, |((i, x), _)| i + x.len_utf8());
            folder_of(&u[..same]).to_string()
        }
    }
}

/// How many of `n` tries for a key that is not there a program backing off
/// (a second, doubling to a minute) makes over `span_s` seconds.
fn backoff_tries(span_s: f64, n: usize) -> usize {
    let (mut t, mut wait, mut tries) = (0.0, 1.0, 1);
    while tries < n && t + wait <= span_s {
        t += wait;
        tries += 1;
        wait = (wait * 2.0f64).min(60.0);
    }
    tries.min(n)
}

pub fn observations(records: &[Record], episodes: &[Episode]) -> Vec<Observation> {
    observations_at(records, episodes, false, &mut None)
}

/// The findings at any threshold: each pattern however little of it there
/// is (a key read twice, one HEAD before a GET, requests one after another
/// that would save a millisecond). What tells a finding fixed (gone) from
/// one only under its threshold (less).
pub fn traces(records: &[Record], episodes: &[Episode]) -> Vec<Observation> {
    observations_at(records, episodes, true, &mut None)
}

/// [`observations`] and [`traces`], each made once: a finding is named
/// among the traces ([`crate::profile::name_findings`]), when they are made
/// for it.
pub fn findings_and_traces(
    records: &[Record],
    episodes: &[Episode],
) -> (Vec<Observation>, Vec<Observation>) {
    let mut traces = None;
    let findings = observations_at(records, episodes, false, &mut traces);
    let traces = traces.unwrap_or_else(|| self::traces(records, episodes));
    (findings, traces)
}

/// [`observations`], or at `any` threshold ([`traces`]). The findings are
/// named among the traces where what they are about holds what changes
/// between runs (`t/00/`, `t/01/`): `traces`, made then if not made yet.
fn observations_at(
    records: &[Record],
    episodes: &[Episode],
    any: bool,
    traces: &mut Option<Vec<Observation>>,
) -> Vec<Observation> {
    use std::collections::{BTreeSet, HashMap};
    // a threshold, or the least that is still the pattern
    let at = |normal: usize, least: usize| if any { least } else { normal };
    let most = at(3, usize::MAX);
    // each record by its id: read once, for every finding that names requests
    let by_id = RecordsById::of(records);
    // which episode each record belongs to
    let owner = Owners::of(episodes);
    let of_records = |ids: &mut dyn Iterator<Item = u64>| -> Vec<usize> {
        ids.filter_map(|id| owner.get(id).map(|i| i as usize))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    let of_episodes = |f: &dyn Fn(&Episode) -> bool| -> Vec<usize> {
        episodes
            .iter()
            .enumerate()
            .filter(|(_, e)| f(e))
            .map(|(i, _)| i)
            .collect()
    };
    let mut out = Vec::new();
    let warn = |t: Phrase, episodes: Vec<usize>| Observation {
        level: "warn",
        text: t.en(),
        phrases: vec![t],
        episodes,
        impact: 0,
        saving: None,
        saving_text: None,
        saves_requests: 0,
        saves_ms: 0.0,
        saves_bytes: 0,
        saved_ids: Vec::new(),
        kept_ids: Vec::new(),
        keeps_bytes: false,
        timed_ids: Vec::new(),
        fixes: Vec::new(),
        callers: Vec::new(),
        sdk: None,
        identity: String::new(),
        kind: String::new(),
        saves_cost: 0.0,
        expected: None,
    };
    let info = |t: Phrase, episodes: Vec<usize>| Observation {
        level: "info",
        text: t.en(),
        phrases: vec![t],
        episodes,
        impact: 0,
        saving: None,
        saving_text: None,
        saves_requests: 0,
        saves_ms: 0.0,
        saves_bytes: 0,
        saved_ids: Vec::new(),
        kept_ids: Vec::new(),
        keeps_bytes: false,
        timed_ids: Vec::new(),
        fixes: Vec::new(),
        callers: Vec::new(),
        sdk: None,
        identity: String::new(),
        kind: String::new(),
        saves_cost: 0.0,
        expected: None,
    };
    let total = records.len().max(1);
    // what a finding that saves requests must save to be one: 1% of the
    // run's requests, or of its cost (at S3's prices: what is a share of
    // cost does not hang on the store)
    let prices = crate::profile::RequestPrices::default();
    let cost_of = |ids: &[u64]| -> f64 {
        ids.iter()
            .filter_map(|id| by_id.get(id))
            .map(|r| crate::profile::price(r, &prices))
            .sum()
    };
    let run_cost: f64 = records
        .iter()
        .map(|r| crate::profile::price(r, &prices))
        .sum();
    let worth = |saved: &[u64]| {
        saved.len() * 100 >= total || (run_cost > 0.0 && cost_of(saved) * 100.0 >= run_cost)
    };
    let requests_of = |eps: &[usize]| eps.iter().map(|&i| episodes[i].requests()).sum::<usize>();
    let unfinished =
        of_episodes(&|e| e.shape == Shape::Multipart && e.title().ends_with("(not completed)"));
    if !unfinished.is_empty() {
        let n = unfinished.len();
        let impact = requests_of(&unfinished);
        out.push(
            warn(
                p("obs.unfinished", &[("n", n.to_string()), ("s", plural(n))]),
                unfinished,
            )
            .weigh(impact, 0, total),
        );
    }
    let orphans = of_episodes(&|e| e.title_says("loose.uncommitted"));
    if !orphans.is_empty() {
        let n = orphans.len();
        let impact = requests_of(&orphans);
        out.push(
            warn(
                p("obs.orphans", &[("n", n.to_string()), ("s", plural(n))]),
                orphans,
            )
            .weigh(impact, 0, total),
        );
    }
    // the uploads in parts, each by its upload id: its parts stored (a part
    // retried is one part, its bytes those of its last answered upload),
    // whether it completed, its requests
    type Upload<'a> = (HashMap<&'a str, u64>, bool, Vec<u64>);
    let mut uploads: HashMap<&str, Upload> = HashMap::new();
    for r in records {
        if let Some(id) = query(r, "uploadId") {
            let u = uploads.entry(id).or_default();
            if r.kind == "UploadPart" && !r.is_error() {
                u.0.insert(
                    query(r, "partNumber").unwrap_or(""),
                    r.request_bytes.get().unwrap_or(0),
                );
            }
            u.1 |= r.kind == "CompleteMultipartUpload" && !r.is_error();
            u.2.push(r.id);
        }
    }
    // each upload began with a CreateMultipartUpload (`?uploads`, no upload
    // id): the creates of a key go to its uploads in order, each the earliest
    // left before the upload's first part
    let mut uploads: Vec<(usize, u64, bool, Vec<u64>)> = uploads
        .into_values()
        .map(|(parts, done, ids)| (parts.len(), parts.values().sum(), done, ids))
        .collect();
    {
        use std::collections::VecDeque;
        let at = &by_id;
        let mut creates: HashMap<(Option<&str>, Option<&str>), VecDeque<&Record>> = HashMap::new();
        let mut sorted: Vec<&Record> = records
            .iter()
            .filter(|r| r.kind == "CreateMultipartUpload" && !r.is_error())
            .collect();
        sorted.sort_by_key(|r| (r.time, r.id));
        for r in sorted {
            creates
                .entry((r.bucket.as_deref(), r.key.as_deref()))
                .or_default()
                .push_back(r);
        }
        let mut order: Vec<(chrono::DateTime<chrono::Utc>, usize)> = uploads
            .iter()
            .enumerate()
            .filter_map(|(i, u)| {
                let first =
                    u.3.iter()
                        .filter_map(|id| at.get(id))
                        .min_by_key(|r| (r.time, r.id))?;
                Some((first.time, i))
            })
            .collect();
        order.sort();
        for (t, i) in order {
            let Some(first) = uploads[i]
                .3
                .iter()
                .filter_map(|id| at.get(id))
                .min_by_key(|r| (r.time, r.id))
            else {
                continue;
            };
            if let Some(q) = creates.get_mut(&(first.bucket.as_deref(), first.key.as_deref()))
                && q.front().is_some_and(|c| c.time <= t)
            {
                let c = q.pop_front().expect("a create in front");
                uploads[i].3.push(c.id);
            }
        }
    }
    let small: Vec<&(usize, u64, bool, Vec<u64>)> = uploads
        .iter()
        .filter(|(parts, bytes, done, _)| *done && *parts == 1 && *bytes < 5 * 1024 * 1024)
        .collect();
    // small files uploaded in parts: 3 requests where 1 PUT would do
    let parts_found = small.len() >= at(3, 1);
    let parts_ids: Vec<u64> = small.iter().flat_map(|u| u.3.iter().copied()).collect();
    // one PUT for each: the rest of each upload's requests go
    let parts_saved: Vec<u64> = parts_ids
        .iter()
        .copied()
        .filter(|id| by_id.get(id).is_some_and(|r| r.kind != "UploadPart"))
        .collect();
    // small objects written one PUT each, many to a folder: the small-file
    // problem (each a write's price, and a request to read it back). Written
    // as objects of [`SMALL`] or more, a folder's PUTs are as few as its bytes
    // fill. One key written again and again is no small file
    // a record by its place in `records`: where it is (bucket, key)
    let place = |i: &u32| {
        let r = &records[*i as usize];
        (r.bucket.as_deref(), r.key.as_deref())
    };
    // (by key: a folder's keys side by side, no set of them made; their
    // names beside them, sorted without reaching into the records)
    let mut smalls: Vec<(&str, &str, u32)> = records
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.kind == "PutObject"
                && !r.is_error()
                && r.request_bytes.get().is_some_and(|b| b < SMALL)
        })
        .filter_map(|(i, r)| Some((r.bucket.as_deref()?, r.key.as_deref()?, i as u32)))
        // a directory marker is its own finding
        .filter(|(_, k, _)| !k.ends_with('/'))
        .collect();
    fn folder<'a>(s: &(&'a str, &'a str, u32)) -> (&'a str, &'a str) {
        (s.0, folder_of(s.1))
    }
    // by bucket and folder first: `out/000` and `out/000/meta` sort side by
    // side by key, which would split `out/`'s objects into ones
    smalls.sort_unstable_by(|a, b| folder(a).cmp(&folder(b)).then(a.cmp(b)));
    let (mut put_ids, mut put_saved, mut put_bytes) = (Vec::new(), Vec::new(), 0u64);
    let mut written_to: Option<String> = None;
    for g in smalls.chunk_by(|a, b| folder(a) == folder(b)) {
        // each key's first write: one key written again and again is no
        // small file, and the finding counts objects, not PUTs
        let firsts: Vec<u32> = g.chunk_by(|a, b| a.1 == b.1).map(|k| k[0].2).collect();
        let keys = firsts.len();
        let bytes: u64 = firsts
            .iter()
            .filter_map(|i| records[*i as usize].request_bytes.get())
            .sum();
        let keep = (bytes.div_ceil(SMALL) as usize).max(1);
        if keys < 2 || keys <= keep {
            continue;
        }
        let ids: Vec<u64> = firsts.iter().map(|i| records[*i as usize].id).collect();
        put_saved.extend_from_slice(&ids[keep..]);
        put_ids.extend(ids);
        put_bytes += bytes;
        let (b, dir) = folder(&g[0]);
        written_to = Some(common_folder(written_to, &format!("{b}/{dir}")));
    }
    drop(smalls);
    put_ids.sort_unstable();
    put_saved.sort_unstable();
    // many, a fifth of what the run's requests cost or more
    let puts_found = if any {
        !put_saved.is_empty()
    } else {
        put_ids.len() >= 50 && cost_of(&put_ids) * 5.0 >= run_cost && worth(&put_saved)
    };
    // two findings of one kind: each is fixed on its own
    if puts_found {
        out.push(
            info(
                p(
                    "obs.small",
                    &[
                        ("n", put_ids.len().to_string()),
                        ("size", human(put_bytes)),
                        (
                            "prefix",
                            written_to.filter(|d| !d.is_empty()).unwrap_or("/".into()),
                        ),
                    ],
                ),
                of_records(&mut put_ids.iter().copied()),
            )
            .weigh(put_ids.len(), put_saved.len(), total)
            .saving_of(put_saved),
        );
    }
    if parts_found {
        out.push(
            info(
                p("obs.small.parts", &[("n", small.len().to_string())]),
                of_records(&mut parts_ids.iter().copied()),
            )
            .weigh(parts_ids.len(), parts_saved.len(), total)
            .saving_of(parts_saved),
        );
    }
    // the same key read again and again, and the same each time: what a
    // cache would answer as the store did (counted by the records' own
    // names: only a hot key's is written out)
    let batches = batch_deletes(records);
    let writes = writes_by_bucket(records, &batches);
    // each bucket's writes by key, then time (by place): where a key's are
    let by_key: HashMap<&str, Vec<u32>> = writes
        .iter()
        .map(|(b, w)| {
            let mut at: Vec<u32> = (0..w.len() as u32).collect();
            at.sort_unstable_by_key(|i| {
                let (when, key) = w[*i as usize];
                (key, when)
            });
            (*b, at)
        })
        .collect();
    // each key's reads side by side (sorted by place, in the records' order
    // within a key): no list is made of each
    let mut reads: Vec<u32> = (0..records.len() as u32)
        .filter(|i| {
            let r = &records[*i as usize];
            (r.method == "GET" || r.method == "HEAD") && r.bucket.is_some() && r.key.is_some()
        })
        .collect();
    reads.sort_by(|a, b| place(a).cmp(&place(b)));
    // each stretch of a key's reads between its writes on its own: a cache
    // the program keeps answers each stretch from its first read (and is
    // told of its own writes). A key is a finding when its stretches that
    // would be answered the same hold at least 10 reads and what a cache
    // saves is at least 1% of the run's requests or of its cost
    let mut hot: Vec<(String, Vec<u64>, Vec<u64>, Vec<u64>)> = Vec::new();
    for group in reads.chunk_by(|a, b| place(a) == place(b)) {
        if group.len() < at(10, 2) {
            continue;
        }
        let stretches = unchanging(records, group, &writes, &by_key);
        let n: usize = stretches.iter().map(Vec::len).sum();
        // a cache reads each stretch once
        let again: Vec<u64> = stretches
            .iter()
            .flat_map(|s| s[1..].iter().copied())
            .collect();
        let enough = if any {
            !again.is_empty()
        } else {
            n >= 10 && worth(&again)
        };
        if enough {
            let (Some(b), Some(k)) = place(&group[0]) else {
                continue;
            };
            let firsts: Vec<u64> = stretches
                .iter()
                .filter_map(|s| s.first().copied())
                .collect();
            hot.push((format!("{b}/{k}"), stretches.concat(), again, firsts));
        }
    }
    drop(reads);
    hot.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    for (k, ids, again, firsts) in hot.into_iter().take(most) {
        let n = ids.len();
        out.push(
            info(
                p(
                    "obs.hot",
                    &[
                        ("key", k),
                        ("n", n.to_string()),
                        ("pct", share_pct(n, total).to_string()),
                    ],
                ),
                of_records(&mut ids.into_iter()),
            )
            .weigh(n, again.len(), total)
            .saving_of(again)
            .keeping(firsts),
        );
    }
    // the same prefix listed again and again, with nothing written under it
    // in between: a listing kept from the first would answer the same (one
    // written between is the program looking for what changed; one not
    // answered, retried, is no listing to keep)
    let mut lists: HashMap<(&str, &str), Vec<&Record>> = HashMap::new();
    for r in records.iter().filter(|r| {
        // a later page (ListObjectsV2's token, ListObjects' marker) is the
        // same listing going on
        r.kind.starts_with("ListObjects")
            && !r.is_error()
            && query(r, "continuation-token").is_none()
            && query(r, "marker").is_none_or(str::is_empty)
    }) {
        lists
            .entry((
                r.bucket.as_deref().unwrap_or(""),
                query(r, "prefix").unwrap_or(""),
            ))
            .or_default()
            .push(r);
    }
    let mut repeated: Vec<(String, Vec<u64>)> = lists
        .into_iter()
        .filter(|(_, rs)| rs.len() >= at(5, 2))
        .filter(|((b, prefix), rs)| {
            let first = rs.iter().map(|r| order_of(r)).min();
            let last = rs.iter().map(|r| order_of(r)).max();
            let (Some(first), Some(last)) = (first, last) else {
                return false;
            };
            !writes
                .get(b)
                .is_some_and(|w| written_between(w, first, last, |k| k.starts_with(prefix)))
        })
        .map(|((b, prefix), rs)| (format!("{b}/{prefix}"), rs.iter().map(|r| r.id).collect()))
        .collect();
    repeated.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    for (prefix_name, ids) in repeated.into_iter().take(most) {
        let n = ids.len();
        let again = ids[1..].to_vec();
        out.push(
            info(
                p(
                    "obs.listed",
                    &[("prefix", prefix_name), ("n", n.to_string())],
                ),
                of_records(&mut ids.into_iter()),
            )
            .weigh(n, n - 1, total)
            .saving_of(again),
        );
    }
    // reads of bytes: a range read again, objects read whole that the
    // program also reads by range, a storm of small objects, prefixes
    // listed one by one
    {
        let ok = |r: &Record| r.status.is_some_and(|s| (200..300).contains(&s));
        let ranged = |r: &Record| header(r, "range").is_some();
        let gets = || {
            records.iter().enumerate().filter(|(_, r)| {
                r.kind == "GetObject" && ok(r) && r.bucket.is_some() && r.key.is_some()
            })
        };
        fn place_of(r: &Record) -> (&str, &str) {
            (
                r.bucket.as_deref().unwrap_or(""),
                r.key.as_deref().unwrap_or(""),
            )
        }
        // the same range of the same object read again with nothing written
        // to it between: a Parquet footer read by each query, an index read
        // by each lookup
        let mut ranges: Vec<(&str, &str, &str, u32)> = gets()
            .filter(|(_, r)| ranged(r))
            .map(|(i, r)| {
                let (b, k) = place_of(r);
                (b, k, header(r, "range").unwrap_or(""), i as u32)
            })
            .collect();
        ranges.sort_unstable();
        let (mut again, mut firsts) = (Vec::new(), Vec::new());
        let (mut again_bytes, mut objects) = (0u64, 0usize);
        let mut under: Option<String> = None;
        for object in ranges.chunk_by(|a, b| (a.0, a.1) == (b.0, b.1)) {
            let mut read_again = false;
            for range in object.chunk_by(|a, b| a.2 == b.2) {
                let mut first: Option<&Record> = None;
                for (b, k, _, i) in range {
                    let r = &records[*i as usize];
                    let changed = first.is_some_and(|f| {
                        writes.get(b).is_some_and(|w| {
                            written_between(w, order_of(f), order_of(r), |x| x == *k)
                        })
                    });
                    match first {
                        Some(_) if !changed => {
                            again.push(r.id);
                            again_bytes += r.response_bytes.get().unwrap_or(0);
                            read_again = true;
                        }
                        _ => {
                            first = Some(r);
                            firsts.push(r.id);
                        }
                    }
                }
            }
            if read_again {
                objects += 1;
                under = Some(common_folder(
                    under,
                    &format!("{}/{}", object[0].0, object[0].1),
                ));
            }
        }
        let found = if any {
            !again.is_empty()
        } else {
            again.len() >= 10 && worth(&again)
        };
        if found {
            again.sort_unstable();
            let n = again.len();
            let mut o = info(
                p(
                    "obs.rangeagain",
                    &[
                        ("n", n.to_string()),
                        ("m", objects.to_string()),
                        ("os", plural(objects)),
                        ("size", human(again_bytes)),
                        (
                            "prefix",
                            under.filter(|u| !u.is_empty()).unwrap_or("/".into()),
                        ),
                    ],
                ),
                of_records(&mut again.iter().copied()),
            )
            .weigh(n, n, total)
            .saving_of(again)
            .keeping(firsts);
            o.saves_bytes = again_bytes;
            out.push(o);
        }
        drop(ranges);
        // objects read whole and by range both: the program reads only parts
        // of them elsewhere, so a whole read fetches what it does not use
        // (each read whole saves what it read past the ranges read of it,
        // of the same object: nothing written to it between)
        let mut by_range: HashMap<(&str, &str), Vec<(Order, &str, u64)>> = HashMap::new();
        for (_, r) in gets().filter(|(_, r)| ranged(r)) {
            by_range.entry(place_of(r)).or_default().push((
                order_of(r),
                header(r, "range").unwrap_or(""),
                r.response_bytes.get().unwrap_or(0),
            ));
        }
        // a key's writes, in time order
        let writes_of = |b: &str, k: &str| -> Vec<Order> {
            let (Some(w), Some(at)) = (writes.get(b), by_key.get(b)) else {
                return Vec::new();
            };
            let from = at.partition_point(|i| w[*i as usize].1 < k);
            let to = at.partition_point(|i| w[*i as usize].1 <= k);
            at[from..to].iter().map(|i| w[*i as usize].0).collect()
        };
        let (mut whole, mut whole_bytes, mut saved_bytes) = (Vec::new(), 0u64, 0u64);
        let mut whole_objects: BTreeSet<(&str, &str)> = BTreeSet::new();
        let mut under: Option<String> = None;
        for (_, r) in gets().filter(|(_, r)| !ranged(r) && query(r, "partNumber").is_none()) {
            let bytes = r.response_bytes.get().unwrap_or(0);
            let Some(read) = by_range.get(&place_of(r)).filter(|_| bytes >= WHOLE_READ) else {
                continue;
            };
            let (b, k) = place_of(r);
            let changed = writes_of(b, k);
            let at = order_of(r);
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            let parts: u64 = read
                .iter()
                .filter(|(when, _, _)| {
                    let (from, to) = (at.min(*when), at.max(*when));
                    let i = changed.partition_point(|w| *w <= from);
                    changed.get(i).is_none_or(|w| *w >= to)
                })
                .filter(|(_, range, _)| seen.insert(range))
                .map(|(_, _, n)| n)
                .sum();
            if seen.is_empty() || bytes <= parts {
                continue;
            }
            whole.push(r.id);
            whole_bytes += bytes;
            saved_bytes += bytes - parts;
            if whole_objects.insert((b, k)) {
                under = Some(common_folder(under, &format!("{b}/{k}")));
            }
        }
        drop(by_range);
        let bytes_out: u64 = records
            .iter()
            .map(|r| r.response_bytes.get().unwrap_or(0))
            .sum();
        let found = if any {
            saved_bytes > 0
        } else {
            saved_bytes >= 16 * WHOLE_READ && saved_bytes * 10 >= bytes_out
        };
        if found {
            let mut o = info(
                p(
                    "obs.wholeread",
                    &[
                        ("n", whole.len().to_string()),
                        ("m", whole_objects.len().to_string()),
                        ("os", plural(whole_objects.len())),
                        ("size", human(whole_bytes)),
                        (
                            "prefix",
                            under.filter(|u| !u.is_empty()).unwrap_or("/".into()),
                        ),
                    ],
                ),
                of_records(&mut whole.iter().copied()),
            );
            o.impact = whole.len();
            o.saves_bytes = saved_bytes;
            let s = p(
                "obs.savesBytes",
                &[
                    ("size", human(saved_bytes)),
                    (
                        "pct",
                        share_pct(saved_bytes as usize, bytes_out as usize).to_string(),
                    ),
                ],
            );
            o.saving_text = Some(s.en());
            o.saving = Some(s);
            out.push(o);
        }
        // many small objects each read whole: an ML loader over images, one
        // GET a sample. Sharded into files of [`SHARD`], as many reads as
        // their bytes fill (a key read again is `obs.hot`'s: each key once)
        // (by index: a run's objects each read once are as many reads)
        let place = |i: &u32| place_of(&records[*i as usize]);
        let mut small: Vec<u32> = gets()
            .filter(|(_, r)| {
                !ranged(r)
                    && query(r, "partNumber").is_none()
                    && r.response_bytes.get().is_some_and(|b| b < SMALL_READ)
            })
            .map(|(i, _)| i as u32)
            .collect();
        small.sort_unstable_by_key(|i| (place(i), *i));
        small.dedup_by(|a, b| place(a) == place(b));
        let small_bytes: u64 = small
            .iter()
            .map(|&i| records[i as usize].response_bytes.get().unwrap_or(0))
            .sum();
        let keep = (small_bytes.div_ceil(SHARD) as usize).max(1);
        let found = if any {
            small.len() > keep
        } else {
            small.len() >= 500 && small.len() * 5 >= total && small.len() > keep
        };
        if found {
            let mut ids: Vec<u64> = small.iter().map(|&i| records[i as usize].id).collect();
            ids.sort_unstable();
            let saved = ids[keep..].to_vec();
            let under = small.iter().fold(None, |u, i| {
                let (b, k) = place(i);
                Some(common_folder(u, &format!("{b}/{k}")))
            });
            out.push(
                info(
                    p(
                        "obs.smallreads",
                        &[
                            ("n", small.len().to_string()),
                            ("size", human(small_bytes)),
                            (
                                "prefix",
                                under.filter(|u| !u.is_empty()).unwrap_or("/".into()),
                            ),
                        ],
                    ),
                    of_records(&mut ids.iter().copied()),
                )
                .weigh(ids.len(), saved.len(), total)
                .saving_of(saved)
                // the shards hold every sample: as many bytes are read
                .keeping_bytes(),
            );
        }
    }
    // many prefixes listed, each once or a few times: discovering a
    // table's partitions (Hive, Spark), a tree walked folder by folder
    // (`rclone ls`). A manifest, or the table format's own metadata, says
    // what is there in one read (a prefix listed again and again is
    // `obs.listed`'s)
    {
        let mut by_prefix: HashMap<(&str, &str), Vec<u64>> = HashMap::new();
        for r in records
            .iter()
            .filter(|r| r.kind.starts_with("ListObjects") && !r.is_error())
        {
            by_prefix
                .entry((
                    r.bucket.as_deref().unwrap_or(""),
                    query(r, "prefix").unwrap_or(""),
                ))
                .or_default()
                .push(r.id);
        }
        let first_pages = |ids: &[u64]| {
            ids.iter()
                .filter_map(|id| by_id.get(id))
                .filter(|r| {
                    query(r, "continuation-token").is_none()
                        && query(r, "marker").is_none_or(str::is_empty)
                })
                .count()
        };
        let mut crawled: Vec<(&(&str, &str), &Vec<u64>)> = by_prefix
            .iter()
            .filter(|(_, ids)| first_pages(ids) < 5)
            .collect();
        crawled.sort_unstable();
        let found = if any {
            crawled.len() >= 2
        } else {
            crawled.len() >= CRAWL
        };
        let mut ids: Vec<u64> = crawled
            .iter()
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect();
        ids.sort_unstable();
        if found && (any || worth(&ids[1..])) {
            let under = crawled.iter().fold(None, |u, ((b, prefix), _)| {
                Some(common_folder(u, &format!("{b}/{prefix}")))
            });
            let saved = ids[1..].to_vec();
            out.push(
                info(
                    p(
                        "obs.crawl",
                        &[
                            ("n", crawled.len().to_string()),
                            ("m", ids.len().to_string()),
                            (
                                "prefix",
                                under.filter(|u| !u.is_empty()).unwrap_or("/".into()),
                            ),
                        ],
                    ),
                    of_records(&mut ids.iter().copied()),
                )
                .weigh(ids.len(), saved.len(), total)
                .saving_of(saved)
                .keeping(vec![ids[0]]),
            );
        }
    }
    // HEAD, then GET of the same object: the GET's answer has the HEAD's
    let mut headed: HashMap<(&str, &str), (u64, chrono::DateTime<chrono::Utc>)> = HashMap::new();
    let mut pairs: Vec<u64> = Vec::new();
    let mut checked = 0usize;
    let mut under: Option<String> = None;
    for r in records {
        let (Some(b), Some(k)) = (r.bucket.as_deref(), r.key.as_deref()) else {
            continue;
        };
        match r.kind.as_str() {
            "HeadObject" if !r.is_error() => {
                headed.insert((b, k), (r.id, r.time));
            }
            // a ranged or part read after a HEAD is the usual way to read in
            // parallel (the size first): not a request too many
            "GetObject" if header(r, "range").is_some() || query(r, "partNumber").is_some() => {
                headed.remove(&(b, k));
            }
            "GetObject" => {
                if let Some((head, at)) = headed.remove(&(b, k))
                    && (0..=2_000).contains(&(r.time - at).num_milliseconds())
                {
                    pairs.extend([head, r.id]);
                    checked += 1;
                    // the folder all of them are in: what the finding is about
                    under = Some(common_folder(under, &format!("{b}/{k}")));
                }
            }
            _ => {}
        }
    }
    if checked >= at(3, 1) {
        out.push(
            info(
                p(
                    "obs.headget",
                    &[
                        ("n", checked.to_string()),
                        (
                            "prefix",
                            under.filter(|u| !u.is_empty()).unwrap_or("/".into()),
                        ),
                    ],
                ),
                of_records(&mut pairs.iter().copied()),
            )
            .weigh(pairs.len(), checked, total)
            // the HEAD of each pair
            .saving_of(pairs.iter().step_by(2).copied().collect()),
        );
    }
    // Hadoop's s3a: a rename is a copy and a delete, and FileOutputCommitter
    // commits by renaming out of _temporary/ (twice, with algorithm 1)
    let (copies, deletes) = renamed_from_temporary(records);
    if copies.len() >= at(3, 1) {
        let n = copies.len();
        let saved: Vec<u64> = copies.iter().chain(&deletes).copied().collect();
        out.push(
            warn(
                p("obs.rename", &[("n", n.to_string()), ("s", plural(n))]),
                of_records(&mut saved.iter().copied()),
            )
            .weigh(saved.len(), saved.len(), total)
            .saving_of(saved),
        );
    }
    // s3a's directory markers: empty objects named `…/`, and their deletes
    let markers: Vec<u64> = records
        .iter()
        .filter(|r| r.kind == "PutObject" && !r.is_error())
        .filter(|r| r.key.as_deref().is_some_and(|k| k.ends_with('/')))
        .filter(|r| r.request_bytes.get().unwrap_or(0) == 0)
        .map(|r| r.id)
        .collect();
    if markers.len() >= at(3, 1) {
        let unmarked: Vec<u64> = records
            .iter()
            .filter(|r| {
                (r.kind == "DeleteObject" && r.key.as_deref().is_some_and(|k| k.ends_with('/')))
                    // a batch of markers alone: one with data in it is sent
                    // for the data all the same
                    || (r.kind == "DeleteObjects"
                        && r.rare().request_body.as_deref().is_some_and(|b| {
                            let keys = reqlog::deleted_objects(b);
                            !keys.is_empty() && keys.iter().all(|(k, _)| k.ends_with('/'))
                        }))
            })
            .map(|r| r.id)
            .collect();
        let n = markers.len();
        let ids: Vec<u64> = markers.iter().chain(&unmarked).copied().collect();
        out.push(
            info(
                p(
                    "obs.markers",
                    &[
                        ("n", n.to_string()),
                        ("s", plural(n)),
                        ("m", unmarked.len().to_string()),
                    ],
                ),
                of_records(&mut ids.iter().copied()),
            )
            .weigh(ids.len(), unmarked.len(), total)
            .saving_of(unmarked),
        );
    }
    // objects deleted one request each: DeleteObjects takes up to 1,000 in
    // one (a commit's renames and markers' deletes are their own findings)
    {
        let renamed: std::collections::HashSet<u64> = deletes.iter().copied().collect();
        let mut by_bucket: HashMap<&str, Vec<u64>> = HashMap::new();
        let mut under: Option<String> = None;
        for r in records
            .iter()
            .filter(|r| r.kind == "DeleteObject" && !crate::summary::is_failure(r))
        {
            let (Some(b), Some(k)) = (r.bucket.as_deref(), r.key.as_deref()) else {
                continue;
            };
            if k.ends_with('/') || renamed.contains(&r.id) {
                continue;
            }
            by_bucket.entry(b).or_default().push(r.id);
            under = Some(common_folder(under, &format!("{b}/{k}")));
        }
        let n: usize = by_bucket.values().map(Vec::len).sum();
        let mut ids: Vec<u64> = Vec::with_capacity(n);
        let mut saved: Vec<u64> = Vec::new();
        for of_bucket in by_bucket.values() {
            ids.extend_from_slice(of_bucket);
            saved.extend_from_slice(&of_bucket[of_bucket.len().div_ceil(1000)..]);
        }
        ids.sort_unstable();
        saved.sort_unstable();
        let found = if any {
            !saved.is_empty()
        } else {
            n >= 20 && worth(&saved)
        };
        if found {
            out.push(
                info(
                    p(
                        "obs.deletes",
                        &[
                            ("n", n.to_string()),
                            (
                                "prefix",
                                under.filter(|u| !u.is_empty()).unwrap_or("/".into()),
                            ),
                        ],
                    ),
                    of_records(&mut ids.iter().copied()),
                )
                .weigh(n, saved.len(), total)
                .saving_of(saved),
            );
        }
    }
    // a key read and then written back with no condition (If-Match): a write
    // by another between the read and this one is lost
    {
        // each key written: the read of it since its last write, and the
        // cycles found, the records taken as they were sent
        type Cycles = (Option<u64>, Vec<u64>);
        let mut of_key: HashMap<(&str, &str), Cycles> = records
            .iter()
            .filter(|r| r.kind == "PutObject" && !r.is_error())
            .filter_map(|r| {
                Some((
                    (r.bucket.as_deref()?, r.key.as_deref()?),
                    (None, Vec::new()),
                ))
            })
            .collect();
        if !of_key.is_empty() {
            for r in records.iter().filter(|r| !r.is_error()) {
                let get = r.kind == "GetObject";
                if !get && r.kind != "PutObject" {
                    continue;
                }
                let (Some(b), Some(k)) = (r.bucket.as_deref(), r.key.as_deref()) else {
                    continue;
                };
                let Some((read, ids)) = of_key.get_mut(&(b, k)) else {
                    continue;
                };
                if get {
                    if header(r, "range").is_none() {
                        *read = Some(r.id);
                    }
                } else if let Some(got) = read.take()
                    && header(r, "if-match").is_none()
                    && header(r, "if-none-match").is_none()
                {
                    ids.extend([got, r.id]);
                }
            }
        }
        let mut cycles: Vec<(String, Vec<u64>)> = of_key
            .into_iter()
            .filter(|(_, (_, ids))| !ids.is_empty())
            .map(|((b, k), (_, ids))| (format!("{b}/{k}"), ids))
            .collect();
        let n: usize = cycles.iter().map(|c| c.1.len() / 2).sum();
        if n >= at(3, 1) {
            cycles.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
            let key = cycles[0].0.clone();
            let mut ids: Vec<u64> = cycles.into_iter().flat_map(|c| c.1).collect();
            ids.sort_unstable();
            // a warning: another writer's change is lost, which is wrong, not slow
            out.push(
                warn(
                    p(
                        "obs.rmw",
                        &[("key", key), ("n", n.to_string()), ("s", plural(n))],
                    ),
                    of_records(&mut ids.iter().copied()),
                )
                .weigh(ids.len(), 0, total),
            );
        }
    }
    // a key asked for again and again while it is not there: a loop polling
    // for a marker (`_SUCCESS`, `_DONE`). Backing off (a second, doubling to a minute) asks as
    // often over the same time as [`backoff_tries`] says
    {
        let mut misses: Vec<u32> = (0..records.len() as u32)
            .filter(|i| {
                let r = &records[*i as usize];
                // a table's or a log's own file looked for is its format's way
                // (SlateDB's newest manifest, its GC boundaries): not a loop
                // the program can back off from
                is_miss(r)
                    && r.bucket.is_some()
                    && r.key.as_deref().is_some_and(|k| part_of(k).is_none())
            })
            .collect();
        let by_time = |i: &u32| order_of(&records[*i as usize]);
        misses.sort_unstable_by(|a, b| place(a).cmp(&place(b)).then(by_time(a).cmp(&by_time(b))));
        let mut polled: Vec<(String, Vec<u64>, Vec<u64>)> = Vec::new();
        for g in misses.chunk_by(|a, b| place(a) == place(b)) {
            if g.len() < at(5, 2) {
                continue;
            }
            let (first, last) = (&records[g[0] as usize], &records[g[g.len() - 1] as usize]);
            let span = (last.time - first.time).num_milliseconds() as f64 / 1000.0;
            let kept = backoff_tries(span, g.len());
            let saved: Vec<u64> = g[kept..].iter().map(|i| records[*i as usize].id).collect();
            let enough = if any {
                !saved.is_empty()
            } else {
                worth(&saved)
            };
            if let (true, (Some(b), Some(k))) = (enough, place(&g[0])) {
                let ids = g.iter().map(|i| records[*i as usize].id).collect();
                polled.push((format!("{b}/{k}"), ids, saved));
            }
        }
        drop(misses);
        polled.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
        for (key, ids, saved) in polled.into_iter().take(most) {
            let n = ids.len();
            out.push(
                info(
                    p("obs.polled", &[("key", key), ("n", n.to_string())]),
                    of_records(&mut ids.into_iter()),
                )
                .weigh(n, saved.len(), total)
                .saving_of(saved),
            );
        }
    }
    // throttled: and how many requests were in flight when it happened
    // (the store's own: a 503 sonde answered in its place asks for nothing)
    let is_throttle = |r: &Record| matches!(r.status, Some(503) | Some(429)) && !sondes_fault(r);
    let throttled: Vec<&Record> = records.iter().filter(|r| is_throttle(r)).collect();
    if !throttled.is_empty() {
        let n = throttled.len();
        // how many were in flight as each throttled request started: those
        // started by then and not yet answered (itself included), counted at
        // those moments alone (a request that took no time still counts
        // itself; odd latencies never make the count negative)
        let mut moments: Vec<i64> = throttled
            .iter()
            .map(|r| r.time.timestamp_micros())
            .collect();
        moments.sort_unstable();
        moments.dedup();
        let (mut started, mut answered) =
            (vec![0usize; moments.len()], vec![0usize; moments.len()]);
        for r in records {
            let start = r.time.timestamp_micros();
            // the first moment at or after it started, and after it was answered
            if let Some(n) = started.get_mut(moments.partition_point(|m| *m < start)) {
                *n += 1;
            }
            let end = start + r.latency_us();
            if let Some(n) = answered.get_mut(moments.partition_point(|m| *m <= end)) {
                *n += 1;
            }
        }
        let (mut by_then, mut done) = (0usize, 0usize);
        let least = started
            .iter()
            .zip(&answered)
            .map(|(s, a)| {
                (by_then, done) = (by_then + s, done + a);
                by_then.saturating_sub(done).max(1)
            })
            .min()
            .unwrap_or(1);
        let phrase = if least >= 4 {
            p(
                "obs.throttled",
                &[
                    ("n", n.to_string()),
                    ("s", plural(n)),
                    ("min", least.to_string()),
                    ("safe", least.to_string()),
                ],
            )
        } else {
            p(
                "obs.throttled.rate",
                &[("n", n.to_string()), ("s", plural(n))],
            )
        };
        // a warning when it cost the program a request (one it gave up on),
        // or when the retries alone are as many more requests as a check's
        // baseline lets a run grow by ([`THROTTLED_SHARE`]); throttles
        // retried away below that are the store pacing a burst: a note
        let gave_up = crate::signals::gave_up(records);
        let lost = records
            .iter()
            .zip(&gave_up)
            .any(|(r, &g)| g && is_throttle(r));
        let ids = of_records(&mut throttled.iter().map(|r| r.id));
        let o = if lost || n as f64 >= THROTTLED_SHARE * total as f64 {
            warn(phrase, ids)
        } else {
            info(phrase, ids)
        };
        out.push(o.weigh(n, 0, total));
    }
    // a prefix near what S3 serves one prefix a second
    {
        use crate::profile::{
            READS_PER_PREFIX, WRITES_PER_PREFIX, is_read, peak_second, prefix_named, prefix_parts,
        };
        let mut per: HashMap<(&str, &str), (Vec<i64>, Vec<i64>, Vec<u64>, usize)> = HashMap::new();
        for r in records {
            let e = per.entry(prefix_parts(r)).or_default();
            if is_read(r) {
                e.0.push(r.time.timestamp_micros());
            } else {
                e.1.push(r.time.timestamp_micros());
            }
            e.2.push(r.id);
            e.3 += (matches!(r.status, Some(503) | Some(429)) && !sondes_fault(r)) as usize;
        }
        let mut hot: Vec<(String, usize, usize, bool, Vec<u64>, usize)> = Vec::new();
        for (prefix, (mut reads, mut writes, ids, throttled)) in per {
            reads.sort_unstable();
            writes.sort_unstable();
            let (pr, pw) = (peak_second(&reads), peak_second(&writes));
            // the nearer of the two to its limit
            let (n, limit, read) = if pr * WRITES_PER_PREFIX >= pw * READS_PER_PREFIX {
                (pr, READS_PER_PREFIX, true)
            } else {
                (pw, WRITES_PER_PREFIX, false)
            };
            // near the limit (four fifths of it), or past half of it and
            // throttled already: half of it alone is no finding (18.0);
            // traced at any threshold, any rate at all
            if n * 5 >= limit * 4 || (n * 2 >= limit && throttled > 0) || (any && n > 0) {
                let prefix = prefix_named(prefix.0, prefix.1);
                hot.push((prefix, n, limit, read, ids, throttled));
            }
        }
        // nearest its limit first: n/limit, compared without dividing
        hot.sort_by(|a, b| (b.1 * a.2).cmp(&(a.1 * b.2)).then(a.0.cmp(&b.0)));
        for (prefix, n, limit, read, ids, _) in hot.into_iter().take(most) {
            let pct = n * 100 / limit;
            let phrase = p(
                if read {
                    "obs.prefixrate.reads"
                } else {
                    "obs.prefixrate.writes"
                },
                &[
                    ("prefix", prefix),
                    ("n", n.to_string()),
                    ("pct", pct.to_string()),
                    ("limit", limit.to_string()),
                ],
            );
            let impact = ids.len();
            out.push(warn(phrase, of_records(&mut ids.into_iter())).weigh(impact, 0, total));
        }
    }
    // requests one after another, each waiting for the one before: all of
    // those that could go at once, whatever another finding says of them
    // (sent at once is a change of its own, weighed on its own). Not a
    // key asked again while not there, a poll; not a conditional write, which
    // waits for what it is conditional on. One that needs one before it
    // goes after that one, at once with the others that need it
    {
        use crate::profile::{CHAIN_CONCURRENCY, CHAIN_MIN, at_once_ms, chains};
        let lat = |r: &Record| (r.latency_us().max(0) as f64) / 1000.0;
        let (mut serial, mut wall, mut fast, mut waiting) = (0usize, 0.0, 0.0, 0.0);
        let mut chained: Vec<u64> = Vec::new();
        for c in chains(records) {
            let mut asked: std::collections::HashSet<(&str, &str)> = Default::default();
            let mut dropped = 0.0;
            let mut kept: Vec<(u32, &Record)> = Vec::new();
            for (&level, r) in c.levels.iter().zip(&c.ids).filter_map(|(l, id)| {
                let r: &Record = by_id.get(id)?;
                Some((l, r))
            }) {
                let place = (
                    r.bucket.as_deref().unwrap_or(""),
                    r.key.as_deref().unwrap_or(""),
                );
                let poll = r.status == Some(404) && !asked.insert(place);
                let conditional = is_write(r)
                    && (header(r, "if-none-match").is_some() || header(r, "if-match").is_some());
                if poll || conditional {
                    dropped += lat(r);
                } else {
                    asked.insert(place);
                    kept.push((level, r));
                }
            }
            // one alone at its level waits for the one before and has none
            // to go at once with (a manifest read after it was written)
            let mut at_level: std::collections::HashMap<u32, usize> = Default::default();
            for &(l, _) in &kept {
                *at_level.entry(l).or_default() += 1;
            }
            kept.retain(|&(l, r)| {
                let alone = at_level[&l] == 1;
                if alone {
                    dropped += lat(r);
                }
                !alone
            });
            if kept.len() < CHAIN_MIN {
                continue;
            }
            // the same requests at once: each level after the one it needs,
            // in waves of the longest ones
            let ms: Vec<(u32, f64)> = kept.iter().map(|&(l, r)| (l, lat(r))).collect();
            serial += kept.len();
            wall += (c.end_ms - c.start_ms - dropped).max(0.0);
            // only the waiting goes: the program's own work between requests stays
            waiting += ms.iter().map(|(_, ms)| ms).sum::<f64>();
            fast += at_once_ms(ms);
            chained.extend(kept.iter().map(|(_, r)| r.id));
        }
        let saved = (waiting - fast).max(0.0);
        if serial > 0 && (saved >= 100.0 || (any && saved > 0.0)) {
            let busy = crate::profile::waiting_ms(records).max(1e-9);
            let time = sonde_fmt::duration;
            let mut o = warn(
                p(
                    "obs.serial",
                    &[
                        ("n", serial.to_string()),
                        ("time", time(wall)),
                        ("k", CHAIN_CONCURRENCY.to_string()),
                        ("fast", time(fast)),
                    ],
                ),
                of_records(&mut chained.iter().copied()),
            );
            if saved * 4.0 < busy {
                o.level = "info";
            }
            o.impact = serial;
            // (said as time of the run once that is known: `fixes::rank_by_saving`)
            o.saves_ms = (saved * 10.0).round() / 10.0;
            o.timed_ids = chained;
            out.push(o);
        }
    }
    // the most telling first: warnings, then by the requests they bear on
    out.sort_by(|a, b| {
        (b.level == "warn")
            .cmp(&(a.level == "warn"))
            .then(b.impact.cmp(&a.impact))
    });
    // two of a run's findings that read the same once what changes between
    // runs is taken out (`t00/`, `t01/`) stay two, by their keys as they
    // were, whether the other is over its threshold or under it (a trace)
    let among: &[Observation] = if !any && out.iter().any(crate::profile::varies) {
        traces.get_or_insert_with(|| observations_at(records, episodes, true, &mut None))
    } else {
        &[]
    };
    crate::profile::name_findings(&mut out, among);
    for o in &mut out {
        // a trace is only told apart from a finding: no fix, no callers
        if any {
            continue;
        }
        // who in the program sent its requests
        let rs: Vec<&Record> = o
            .episodes
            .iter()
            .filter_map(|&i| episodes.get(i))
            .flat_map(|e| e.records.iter().filter_map(|id| by_id.get(id)))
            .collect();
        if let Some(first) = o.phrases.first() {
            // requests at once by those one after another, not all of their operations'
            let timed: Vec<&Record> = o.timed_ids.iter().filter_map(|id| by_id.get(id)).collect();
            let of = if timed.is_empty() { &rs } else { &timed };
            o.fixes = crate::fixes::for_finding(first.key, of);
        }
        (o.callers, o.sdk) = sonde_proxy::callers::of(&rs);
        // the engine's own setting, where its requests are an engine's
        let formats: Vec<(Option<Format>, usize)> = o
            .episodes
            .iter()
            .filter_map(|&i| episodes.get(i))
            .map(|e| (e.format, e.records.len()))
            .collect();
        let engine = crate::engines::engine_of(&rs, &formats);
        crate::engines::apply(o, engine, &rs, records);
    }
    out
}

/// The writes of a run by bucket, each with when it was sent and the key
/// it changed (a PUT, a copy onto it, a part, a completion, a delete, each
/// key a DeleteObjects names), in order: what tells a read that would answer
/// the same again from one whose answer changed in between.
fn writes_by_bucket<'a>(
    records: &'a [Record],
    batches: &'a [(usize, Vec<reqlog::Deleted>)],
) -> std::collections::HashMap<&'a str, Vec<(Order, &'a str)>> {
    // (each bucket's list made at its size: its writes counted first)
    let mut counts: std::collections::HashMap<&str, usize> = Default::default();
    for r in records.iter().filter(|r| !crate::profile::is_read(r)) {
        if let Some(b) = r.bucket.as_deref() {
            *counts.entry(b).or_default() += 1;
        }
    }
    let mut out: std::collections::HashMap<&str, Vec<(Order, &str)>> = counts
        .into_iter()
        .map(|(b, n)| (b, Vec::with_capacity(n)))
        .collect();
    for r in records.iter().filter(|r| !crate::profile::is_read(r)) {
        let Some(b) = r.bucket.as_deref() else {
            continue;
        };
        let at = order_of(r);
        let w = out.entry(b).or_default();
        if let Some(k) = r.key.as_deref() {
            w.push((at, k));
        }
    }
    for (i, keys) in batches {
        let r = &records[*i];
        if let Some(w) = r.bucket.as_deref().and_then(|b| out.get_mut(b)) {
            w.extend(keys.iter().map(|(k, _)| (order_of(r), k.as_str())));
        }
    }
    for w in out.values_mut() {
        w.sort_unstable();
    }
    out
}

/// The objects each DeleteObjects names, by the request's place in `records`.
fn batch_deletes(records: &[Record]) -> Vec<(usize, Vec<reqlog::Deleted>)> {
    records
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == "DeleteObjects")
        .filter_map(|(i, r)| {
            let keys = reqlog::deleted_objects(r.rare().request_body.as_deref()?);
            (!keys.is_empty()).then_some((i, keys))
        })
        .collect()
}

/// When a request was sent, as requests are ordered: its time in
/// nanoseconds, then its id.
type Order = (i64, u64);

fn order_of(r: &Record) -> Order {
    (r.time.timestamp_nanos_opt().unwrap_or(i64::MAX), r.id)
}

/// Whether a write that `changes` sits strictly between `first` and `last`.
fn written_between(
    writes: &[(Order, &str)],
    first: Order,
    last: Order,
    changes: impl Fn(&str) -> bool,
) -> bool {
    let from = writes.partition_point(|(at, _)| *at <= first);
    writes[from..]
        .iter()
        .take_while(|(at, _)| *at < last)
        .any(|(_, k)| changes(k))
}

/// The stretches of one key's reads (`group`, by place) between its writes
/// (`writes` and, by key, `by_key`) that a copy kept from each stretch's
/// first read would answer as the store did, each the ids of its reads, of
/// two reads or more: each read found (no miss), whole (no range or part:
/// those are the parallel reading of one object), and of one ETag where the
/// store said it. A miss is not kept by a cache (an exception is not cached;
/// a key polled for appears), so a stretch with one is none. A read that
/// failed otherwise (throttled, a 500) said nothing of the key: it is
/// counted in its stretch and judges nothing (a cache would not have sent
/// it).
fn unchanging(
    records: &[Record],
    group: &[u32],
    writes: &std::collections::HashMap<&str, Vec<(Order, &str)>>,
    by_key: &std::collections::HashMap<&str, Vec<u32>>,
) -> Vec<Vec<u64>> {
    let Some(r) = group.first().map(|i| &records[*i as usize]) else {
        return Vec::new();
    };
    let (Some(b), Some(k)) = (r.bucket.as_deref(), r.key.as_deref()) else {
        return Vec::new();
    };
    // the key's writes, in time order
    let of_key: &[u32] = match (writes.get(b), by_key.get(b)) {
        (Some(w), Some(at)) => {
            let from = at.partition_point(|i| w[*i as usize].1 < k);
            let to = at.partition_point(|i| w[*i as usize].1 <= k);
            &at[from..to]
        }
        _ => &[],
    };
    let w = writes.get(b);
    // each read's stretch: how many of the key's writes were sent before it
    let mut reads: Vec<(usize, Order, &Record)> = group
        .iter()
        .map(|i| &records[*i as usize])
        .map(|r| {
            let at = order_of(r);
            let stretch = w.map_or(0, |w| of_key.partition_point(|i| w[*i as usize].0 < at));
            (stretch, at, r)
        })
        .collect();
    reads.sort_by_key(|(s, at, _)| (*s, *at));
    let mut out = Vec::new();
    for stretch in reads.chunk_by(|a, b| a.0 == b.0) {
        if stretch.len() < 2 {
            continue;
        }
        // (a read that failed judges nothing)
        let answered = || {
            stretch
                .iter()
                .filter(|(_, _, r)| !crate::summary::is_failure(r))
        };
        let kept = answered().find_map(|(_, _, r)| response_header(r, "etag"));
        let same = answered().next().is_some()
            && answered().all(|(_, _, r)| {
                let etag = response_header(r, "etag");
                header(r, "range").is_none()
                    && query(r, "partNumber").is_none()
                    && !is_miss(r)
                    && (etag.is_none() || etag == kept)
            });
        if same {
            out.push(stretch.iter().map(|(_, _, r)| r.id).collect());
        }
    }
    out
}

/// Copies out of a `_temporary/` folder and the deletes of what they copied
/// (by id): a rename on S3, as Hadoop's FileOutputCommitter commits.
fn renamed_from_temporary(records: &[Record]) -> (Vec<u64>, Vec<u64>) {
    use std::collections::HashSet;
    let mut sources: HashSet<(String, String)> = HashSet::new();
    let mut copies = Vec::new();
    for r in records
        .iter()
        .filter(|r| r.kind == "CopyObject" && !r.is_error())
    {
        let Some(src) = header(r, "x-amz-copy-source") else {
            continue;
        };
        let src = percent_encoding::percent_decode_str(src.split('?').next().unwrap_or(""))
            .decode_utf8_lossy()
            .into_owned();
        let src = src.trim_start_matches('/');
        let Some((bucket, key)) = src.split_once('/') else {
            continue;
        };
        if key.starts_with("_temporary/") || key.contains("/_temporary/") {
            copies.push(r.id);
            sources.insert((bucket.to_string(), key.to_string()));
        }
    }
    let deletes = records
        .iter()
        .filter(|r| r.kind == "DeleteObject")
        .filter(|r| match (&r.bucket, &r.key) {
            (Some(b), Some(k)) => sources.contains(&(b.to_string(), k.to_string())),
            _ => false,
        })
        .map(|r| r.id)
        .collect();
    (copies, deletes)
}

/// [`observations`] of a run, read with its header: a run whose
/// times are to the second (an imported log's:
/// [`crate::run::RunHeader::whole_seconds`]) says nothing that
/// depends on where in its second a request began. Requests "one after
/// another" are not found in it (spread over their second they all look
/// so), and a throttling is said without how many were in flight.
pub fn observations_of(
    header: &crate::run::RunHeader,
    records: &[Record],
    episodes: &[Episode],
) -> Vec<Observation> {
    of_header(header, observations(records, episodes))
}

/// [`findings_and_traces`] of a run, as [`observations_of`] and
/// [`traces_of`] say them.
pub fn findings_and_traces_of(
    header: &crate::run::RunHeader,
    records: &[Record],
    episodes: &[Episode],
) -> (Vec<Observation>, Vec<Observation>) {
    let (findings, traces) = findings_and_traces(records, episodes);
    (of_header(header, findings), of_header(header, traces))
}

/// [`traces`] of a run, as [`observations_of`] says its findings.
pub fn traces_of(
    header: &crate::run::RunHeader,
    records: &[Record],
    episodes: &[Episode],
) -> Vec<Observation> {
    of_header(header, traces(records, episodes))
}

fn of_header(header: &crate::run::RunHeader, mut out: Vec<Observation>) -> Vec<Observation> {
    // where the program's folder sets each engine setting now
    crate::engines::locate_all(&mut out, header.cwd.as_deref());
    if !header.whole_seconds {
        return out;
    }
    let key = |o: &Observation| o.phrases.first().map_or("", |p| p.key);
    out.retain(|o| key(o) != "obs.serial");
    for o in &mut out {
        if key(o) == "obs.throttled" {
            let said = &o.phrases[0];
            let phrase = p(
                "obs.throttled.rate",
                &[
                    (
                        "n",
                        said.args
                            .get("n")
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                    ),
                    (
                        "s",
                        said.args
                            .get("s")
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                    ),
                ],
            );
            o.text = phrase.en();
            o.phrases = vec![phrase];
            o.fixes = crate::fixes::rekeyed(std::mem::take(&mut o.fixes), "obs.throttled.rate");
            o.identity = crate::profile::finding_identity(o);
            o.kind = crate::profile::kind_of(&o.identity).to_string();
        }
        // a log says no request's headers: whether a write was conditional
        // is not known
        if key(o) == "obs.rmw" {
            let said = &o.phrases[0];
            let arg = |n: &str| said.args.get(n).map(|v| v.to_string()).unwrap_or_default();
            let phrase = p(
                "obs.rmw.log",
                &[("key", arg("key")), ("n", arg("n")), ("s", arg("s"))],
            );
            o.text = phrase.en();
            o.phrases = vec![phrase];
            o.fixes = crate::fixes::rekeyed(std::mem::take(&mut o.fixes), "obs.rmw.log");
            // (whether it is lost the log does not say: a note)
            o.level = "info";
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqlog::Header;

    #[test]
    fn an_operation_of_one_request_is_small_and_titled_as_ever() {
        assert_eq!(std::mem::size_of::<Episode>(), 88);
        let r = Record {
            id: 4,
            kind: "PutObject".into(),
            method: "PUT".into(),
            bucket: Some("b".into()),
            key: Some("k".into()),
            request_bytes: Some(2048).into(),
            ..Default::default()
        };
        let e = single(&r);
        assert_eq!(
            (e.records.clone(), e.title()),
            (Ids::One(4), "PutObject b/k · 2.0 KiB".into())
        );
        let w = serde_json::to_value(&e).unwrap();
        assert_eq!(w["records"], serde_json::json!([4]));
        assert_eq!(w["titleText"][0]["args"]["target"], "b/k");
    }

    #[test]
    fn a_failures_retry_is_the_next_request_of_its_own_name_within_a_minute() {
        let at = |id: u64, ms: i64, key: &str, status: u16| Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + ms).unwrap(),
            method: "GET".into(),
            kind: "GetObject".into(),
            bucket: Some("b".into()),
            key: Some(key.into()),
            status: Some(status),
            latency_ms: 5.0,
            ..Default::default()
        };
        // an outage's worth of other keys between a failure and its retry
        let mut rs = vec![at(1, 0, "a", 503)];
        rs.extend((0..200).map(|i| at(10 + i, 10 + i as i64, &format!("other/{i}"), 503)));
        rs.push(at(2, 500, "a", 503));
        rs.push(at(3, 900, "a", 200));
        // the same key again much later is another request, not a retry
        rs.push(at(4, 900 + 61_000, "a", 503));
        rs.push(at(5, 900 + 200_000, "a", 200));
        let eps = episodes(&rs);
        let retried: Vec<&Episode> = eps.iter().filter(|e| e.shape == Shape::Retry).collect();
        assert_eq!(retried.len(), 1, "{eps:#?}");
        assert_eq!(retried[0].records, [1, 2, 3]);
        assert_eq!(retried[0].title(), "GetObject a retried 2× after 503");
        // every other failure stands alone: 200 keys, and the late one
        assert_eq!(eps.len(), 1 + 200 + 2);
    }

    #[test]
    fn creates_go_to_their_uploads_in_order_whatever_else_shares_the_key() {
        let at = |id: u64, ms: i64, op: &str, q: &[(&str, &str)], status: u16| Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + ms).unwrap(),
            method: if op == "UploadPart" { "PUT" } else { "POST" }.into(),
            kind: op.into(),
            bucket: Some("b".into()),
            key: Some("big".into()),
            status: Some(status),
            request_bytes: Some(if op == "UploadPart" { 8 << 20 } else { 0 }).into(),
            latency_ms: 5.0,
            lists: reqlog::Lists {
                query: q.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        };
        let rs = vec![
            // a create that failed and was tried again, then its upload
            at(1, 0, "CreateMultipartUpload", &[("uploads", "")], 500),
            at(2, 10, "CreateMultipartUpload", &[("uploads", "")], 200),
            // a second upload of the same key begun before the first's parts
            at(3, 20, "CreateMultipartUpload", &[("uploads", "")], 200),
            at(
                4,
                30,
                "UploadPart",
                &[("partNumber", "1"), ("uploadId", "A")],
                200,
            ),
            at(
                5,
                40,
                "UploadPart",
                &[("partNumber", "1"), ("uploadId", "B")],
                200,
            ),
            at(6, 50, "CompleteMultipartUpload", &[("uploadId", "A")], 200),
            at(7, 60, "CompleteMultipartUpload", &[("uploadId", "B")], 200),
            // a create that failed and was never tried again began nothing
            at(8, 70, "CreateMultipartUpload", &[("uploads", "")], 500),
        ];
        let eps = episodes(&rs);
        let uploads: Vec<&Episode> = eps.iter().filter(|e| e.shape == Shape::Multipart).collect();
        assert_eq!(uploads.len(), 2, "{eps:#?}");
        assert_eq!(uploads[0].records, [1, 2, 4, 6]);
        assert_eq!(uploads[1].records, [3, 5, 7]);
        assert!(
            eps.iter()
                .any(|e| e.shape == Shape::Request && e.records == [8])
        );
    }

    fn rec(id: u64, at_ms: i64, method: &str, op: &str, key: Option<&str>, status: u16) -> Record {
        Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
            method: method.into(),
            kind: op.into(),
            bucket: Some("b".into()),
            key: key.map(Into::into),
            status: Some(status),
            latency_ms: 5.0,
            request_bytes: Some(if method == "PUT" { 100 } else { 0 }).into(),
            response_bytes: Some(10).into(),
            ..Default::default()
        }
    }

    fn with_query(mut r: Record, q: &[(&str, &str)]) -> Record {
        r.lists_mut().query = q.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect();
        r
    }

    #[test]
    fn retries_chain_until_success() {
        let mut slow = rec(1, 0, "GET", "GetObject", Some("k"), 503);
        slow.rare_mut().error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
        let mut slow2 = slow.clone();
        slow2.id = 2;
        slow2.time += Duration::milliseconds(200);
        let ok = rec(3, 700, "GET", "GetObject", Some("k"), 200);
        let other = rec(4, 100, "GET", "GetObject", Some("other"), 200);
        let eps = episodes(&[slow, slow2, ok, other]);
        assert_eq!(eps.len(), 2);
        let retry = eps.iter().find(|e| e.shape == Shape::Retry).unwrap();
        assert_eq!(retry.records, vec![1, 2, 3]);
        assert_eq!(retry.title(), "GetObject k retried 2× after 503 SlowDown");
        assert_eq!(*retry.outcome(), Outcome::Ok);
        // retrying is right: no finding of its own (18.0)
        assert!(observations(&[], &eps).is_empty());
    }

    /// Throttling is a warning when a request gave up after its retries, or
    /// when throttles reach a tenth of the requests; one 503 retried away
    /// is a note (real S3 throttles now and then).
    #[test]
    fn throttling_retried_away_is_a_note_and_one_given_up_on_a_warning() {
        let level = |rs: &[Record]| {
            observations(rs, &episodes(rs))
                .into_iter()
                .find(|o| o.phrases[0].key.starts_with("obs.throttled"))
                .map(|o| o.level)
        };
        let mut rs: Vec<Record> = (0..30u64)
            .map(|i| {
                rec(
                    i,
                    i as i64 * 100,
                    "GET",
                    "GetObject",
                    Some(&format!("k{i}")),
                    200,
                )
            })
            .collect();
        let mut retried = rs.clone();
        retried.push(rec(40, 5000, "GET", "GetObject", Some("z"), 503));
        retried.push(rec(41, 5100, "GET", "GetObject", Some("z"), 200));
        assert_eq!(level(&retried), Some("info"));
        // the same 503, never sent again: the program lost a request
        let mut lost = rs.clone();
        lost.push(rec(40, 5000, "GET", "GetObject", Some("z"), 503));
        assert_eq!(level(&lost), Some("warn"));
        // a tenth of the requests throttled, each retried: a warning
        for i in 0..4u64 {
            let key = format!("t{i}");
            rs.push(rec(
                50 + 2 * i,
                6000 + i as i64 * 300,
                "GET",
                "GetObject",
                Some(&key),
                503,
            ));
            rs.push(rec(
                51 + 2 * i,
                6100 + i as i64 * 300,
                "GET",
                "GetObject",
                Some(&key),
                200,
            ));
        }
        assert!(4.0 >= THROTTLED_SHARE * rs.len() as f64);
        assert_eq!(level(&rs), Some("warn"));
    }

    #[test]
    fn a_trials_503s_are_not_the_stores() {
        let fault = |mut r: Record| {
            r.rare_mut().injected = Some("slowdown".into());
            r.lists_mut()
                .response_headers
                .push(Header::new("x-sonde-fault", "slowdown"));
            r
        };
        let mut rs = Vec::new();
        for i in 0..6u64 {
            let key = format!("k{i}");
            let at = i as i64 * 300;
            rs.push(fault(rec(
                i * 2 + 1,
                at,
                "PUT",
                "PutObject",
                Some(&key),
                503,
            )));
            rs.push(rec(
                i * 2 + 2,
                at + 110,
                "PUT",
                "PutObject",
                Some(&key),
                200,
            ));
        }
        let obs = observations(&rs, &episodes(&rs));
        let keys: Vec<&str> = obs.iter().map(|o| o.phrases[0].key).collect();
        assert!(
            !keys.iter().any(|k| k.starts_with("obs.throttled")),
            "the store asked for nothing: {keys:?}"
        );
        // a 503 of the store's own, in the same run, is still the store's
        let mut own = rec(90, 5000, "GET", "GetObject", Some("z"), 503);
        own.rare_mut().error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
        rs.push(own);
        rs.push(rec(91, 5200, "GET", "GetObject", Some("z"), 200));
        let obs = observations(&rs, &episodes(&rs));
        let throttled = obs
            .iter()
            .find(|o| o.phrases[0].key.starts_with("obs.throttled"))
            .expect("the store's own 503");
        assert_eq!(throttled.phrases[0].args["n"], "1");
    }

    #[test]
    fn an_upload_the_store_completed_is_complete_though_the_program_was_told_500() {
        let mut create = rec(1, 0, "POST", "CreateMultipartUpload", Some("big"), 200);
        create.lists_mut().query = vec![("uploads".into(), "".into())];
        let q = [("uploadId", "u1")];
        let part = with_query(
            rec(2, 10, "PUT", "UploadPart", Some("big"), 200),
            &[("uploadId", "u1"), ("partNumber", "1")],
        );
        let mut lost = with_query(
            rec(3, 20, "POST", "CompleteMultipartUpload", Some("big"), 500),
            &q,
        );
        lost.rare_mut().injected = Some("lost-answer".into());
        lost.lists_mut()
            .response_headers
            .push(Header::new("x-sonde-store-status", "200"));
        lost.lists_mut()
            .response_headers
            .push(Header::new("x-sonde-fault", "lost-answer"));
        let mut again = with_query(
            rec(4, 130, "POST", "CompleteMultipartUpload", Some("big"), 404),
            &q,
        );
        again.rare_mut().error_body = Some("<Error><Code>NoSuchUpload</Code></Error>".into());
        let abort = with_query(
            rec(5, 140, "DELETE", "AbortMultipartUpload", Some("big"), 404),
            &q,
        );
        let rs = [create, part, lost, again, abort];
        let eps = episodes(&rs);
        let upload = eps.iter().find(|e| e.shape == Shape::Multipart).unwrap();
        assert!(
            !upload.title().contains("aborted") && !upload.title().contains("not completed"),
            "{}",
            upload.title()
        );
        let obs = observations(&rs, &eps);
        assert!(
            !obs.iter().any(|o| o.phrases[0].key == "obs.unfinished"),
            "{obs:#?}"
        );
    }

    #[test]
    fn a_delta_table_under_a_data_folder_keeps_its_files() {
        let rs = [
            rec(
                1,
                0,
                "PUT",
                "PutObject",
                Some("lake/data/events/part-00000-a.snappy.parquet"),
                200,
            ),
            rec(
                2,
                10,
                "PUT",
                "PutObject",
                Some("lake/data/events/day=1/f1.parquet"),
                200,
            ),
            rec(
                3,
                20,
                "PUT",
                "PutObject",
                Some("lake/data/events/_delta_log/00000000000000000001.json"),
                200,
            ),
        ];
        let eps = episodes(&rs);
        assert_eq!(eps.len(), 1, "{eps:#?}");
        assert_eq!(eps[0].format, Some(Format::Delta));
        assert_eq!(eps[0].records, vec![1, 2, 3], "the files are the commit's");
    }

    #[test]
    fn unfinished_multipart_and_aborts() {
        let create = rec(1, 0, "POST", "CreateMultipartUpload", Some("big"), 200);
        let p1 = with_query(
            rec(2, 10, "PUT", "UploadPart", Some("big"), 200),
            &[("partNumber", "1"), ("uploadId", "u1")],
        );
        let p2 = with_query(
            rec(3, 20, "PUT", "UploadPart", Some("big"), 200),
            &[("partNumber", "2"), ("uploadId", "u1")],
        );
        let create2 = rec(4, 30, "POST", "CreateMultipartUpload", Some("other"), 200);
        let abort = with_query(
            rec(5, 40, "DELETE", "AbortMultipartUpload", Some("other"), 204),
            &[("uploadId", "u2")],
        );
        let eps = episodes(&[create, p1, p2, create2, abort]);
        assert_eq!(eps.len(), 2);
        assert_eq!(
            eps[0].title(),
            "multipart upload b/big · 2 parts · 200 B (not completed)"
        );
        assert_eq!(eps[0].records, vec![1, 2, 3]);
        assert_eq!(
            eps[1].title(),
            "multipart upload b/other · 0 parts · 0 B (aborted)"
        );
        let obs = observations(&[], &eps);
        let unfinished = obs
            .iter()
            .find(|o| o.level == "warn" && o.text.starts_with("1 multipart upload never completed"))
            .expect("the unfinished upload is reported");
        assert_eq!(unfinished.episodes, vec![0], "and points at its episode");
    }

    #[test]
    fn parts_with_no_create_began_before_the_window_and_are_not_unfinished() {
        let q = [("partNumber", "1"), ("uploadId", "u1")];
        let p1 = with_query(rec(1, 0, "PUT", "UploadPart", Some("big"), 200), &q);
        let p2 = with_query(
            rec(2, 10, "PUT", "UploadPart", Some("big"), 200),
            &[("partNumber", "2"), ("uploadId", "u1")],
        );
        let eps = episodes(&[p1, p2]);
        let up = eps.iter().find(|e| e.shape == Shape::Multipart).unwrap();
        assert!(!up.title().contains("not completed"), "{}", up.title());
        let obs = observations(&[], &eps);
        assert!(
            !obs.iter().any(|o| o.text.contains("never completed")),
            "{obs:#?}"
        );
    }

    #[test]
    fn iceberg_commit_gathers_files_and_orphans_are_reported() {
        let data = rec(
            1,
            0,
            "PUT",
            "PutObject",
            Some("t/data/00000-0-a.parquet"),
            200,
        );
        let manifest = rec(2, 10, "PUT", "PutObject", Some("t/metadata/a-m0.avro"), 200);
        let list = rec(
            3,
            20,
            "PUT",
            "PutObject",
            Some("t/metadata/snap-1-0-a.avro"),
            200,
        );
        let meta = rec(
            4,
            30,
            "PUT",
            "PutObject",
            Some("t/metadata/00001-x.metadata.json"),
            200,
        );
        // later: files written and never committed
        let orphan = rec(
            5,
            10_000,
            "PUT",
            "PutObject",
            Some("t/data/00001-0-b.parquet"),
            200,
        );
        let unrelated = rec(6, 20_000, "GET", "GetObject", Some("readme"), 200);
        let eps = episodes(&[data, manifest, list, meta, orphan, unrelated]);
        assert_eq!(eps.len(), 3, "{eps:#?}");
        assert_eq!(eps[0].format, Some(Format::Iceberg));
        assert_eq!(
            eps[0].title(),
            "commit: table metadata v1 · 3 files (300 B)"
        );
        assert_eq!(eps[1].title(), "1 file written, no commit followed (100 B)");
        assert!(
            observations(&[], &eps)
                .iter()
                .any(|o| o.text.contains("no commit after them"))
        );
    }

    #[test]
    fn a_parquet_file_is_no_table_until_the_run_touches_its_log() {
        // what Spark, Hive, Hudi and DuckDB write: the names alone
        let spark = rec(
            1,
            0,
            "GET",
            "GetObject",
            Some("out/part-00000-a.parquet"),
            200,
        );
        let hive = rec(
            2,
            10,
            "GET",
            "GetObject",
            Some("wh/data/x=1/a.parquet"),
            200,
        );
        let eps = episodes(&[spark.clone(), hive.clone()]);
        assert_eq!(eps.len(), 2, "{eps:#?}");
        assert!(eps.iter().all(|e| e.format.is_none()), "{eps:#?}");
        assert!(eps.iter().all(|e| e.shape == Shape::Request), "{eps:#?}");

        // the same reads beside a table's log are that table's
        let log = with_query(
            rec(3, 20, "GET", "ListObjectsV2", None, 200),
            &[("prefix", "out/_delta_log/")],
        );
        let meta = rec(
            4,
            30,
            "GET",
            "GetObject",
            Some("wh/metadata/v2.metadata.json"),
            200,
        );
        let eps = episodes(&[spark.clone(), hive.clone(), log.clone(), meta]);
        let formats: Vec<_> = eps.iter().filter_map(|e| e.format).collect();
        assert_eq!(formats, vec![Format::Delta, Format::Iceberg], "{eps:#?}");
        assert_eq!(eps.iter().map(|e| e.records.len()).sum::<usize>(), 4);

        // another table's log, or the same root in another bucket, backs nothing
        let mut elsewhere = log;
        elsewhere.bucket = Some("other".into());
        let other = rec(
            5,
            40,
            "GET",
            "GetObject",
            Some("lake/_delta_log/00000000000000000001.json"),
            200,
        );
        let eps = episodes(&[spark, elsewhere, other]);
        let read = eps.iter().find(|e| e.records == vec![1]).expect("its own");
        assert_eq!(read.format, None, "{eps:#?}");
    }

    #[test]
    fn delta_commits_races_and_refusals() {
        let mut won = rec(
            1,
            0,
            "PUT",
            "PutObject",
            Some("t/_delta_log/00000000000000000007.json"),
            200,
        );
        won.lists_mut().request_headers = vec![Header::new("If-None-Match", "*")];
        let mut lost = rec(
            2,
            50,
            "PUT",
            "PutObject",
            Some("t/_delta_log/00000000000000000008.json"),
            412,
        );
        lost.lists_mut().request_headers = vec![Header::new("If-None-Match", "*")];
        lost.rare_mut().error_body = Some("<Error><Code>PreconditionFailed</Code></Error>".into());
        let mut refused = rec(3, 100, "DELETE", "DeleteObject", Some("x"), 403);
        refused.lists_mut().response_headers = vec![Header::new("x-sonde-proxy", "refused")];
        let records = [won, lost, refused];
        let eps = episodes(&records);
        assert_eq!(eps[0].title(), "commit 7");
        assert_eq!(
            eps[0].notes(),
            vec!["conditional create (If-None-Match: *)"]
        );
        assert_eq!(
            *eps[1].outcome(),
            Outcome::Failed("412 PreconditionFailed".into())
        );
        assert!(eps[1].notes().iter().any(|n| n.starts_with("lost a race")));
        assert_eq!(*eps[2].outcome(), Outcome::Refused);
        // the operation says the race; a writer that lost one is no finding (18.0)
        assert!(observations(&records, &eps).is_empty());
    }

    #[test]
    fn pages_of_one_listing() {
        let first = with_query(
            rec(1, 0, "GET", "ListObjectsV2", None, 200),
            &[("list-type", "2"), ("prefix", "logs/")],
        );
        let next = with_query(
            rec(2, 10, "GET", "ListObjectsV2", None, 200),
            &[
                ("list-type", "2"),
                ("prefix", "logs/"),
                ("continuation-token", "t"),
            ],
        );
        let again = with_query(
            rec(3, 20, "GET", "ListObjectsV2", None, 200),
            &[("list-type", "2"), ("prefix", "logs/")],
        );
        let eps = episodes(&[first, next, again]);
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].title(), "listed b/logs/ in 2 pages");
        assert_eq!(eps[1].title(), "ListObjectsV2 b/logs/");
    }

    #[test]
    fn parts() {
        assert_eq!(
            part_of("db/manifest/00000000000000000012.manifest"),
            Some(Part::SlateManifest(12))
        );
        assert_eq!(
            part_of("db/wal/00000000000000000001.sst"),
            Some(Part::SlateWal(1))
        );
        assert_eq!(part_of("db/compacted/01J9ZCQ.sst"), Some(Part::SlateSst));
        assert_eq!(
            part_of("db/gc/manifest.boundary"),
            Some(Part::SlateGcBoundary)
        );
        assert_eq!(
            part_of("t/metadata/00003-6c5e.metadata.json"),
            Some(Part::IcebergMetadata(Some(3)))
        );
        assert_eq!(
            part_of("t/metadata/v2.metadata.json"),
            Some(Part::IcebergMetadata(Some(2)))
        );
        assert_eq!(
            part_of("t/metadata/snap-1-0-abc.avro"),
            Some(Part::IcebergManifestList)
        );
        assert_eq!(
            part_of("t/metadata/abc-m0.avro"),
            Some(Part::IcebergManifest)
        );
        assert_eq!(
            part_of("t/data/x=1/00000-0-abc.parquet"),
            Some(Part::IcebergData)
        );
        assert_eq!(
            part_of("t/_delta_log/00000000000000000004.json"),
            Some(Part::DeltaCommit(4))
        );
        assert_eq!(
            part_of("t/part-00000-abc.snappy.parquet"),
            Some(Part::DeltaData)
        );
        assert_eq!(part_of("logs/2026/a.json"), None);
        assert_eq!(
            part_of("manifest/12.manifest"),
            None,
            "SlateDB ids are 20 digits"
        );
    }

    #[test]
    fn lance_parts() {
        // a version is stored as u64::MAX - v, so the newest lists first
        assert_eq!(
            part_of("d.lance/_versions/18446744073709551608.manifest"),
            Some(Part::LanceManifest(7))
        );
        assert_eq!(
            part_of("d.lance/_versions/3.manifest"),
            Some(Part::LanceManifest(3)),
            "the older naming"
        );
        assert_eq!(
            part_of("d.lance/_transactions/6-0a1b.txn"),
            Some(Part::LanceTransaction)
        );
        assert_eq!(part_of("d.lance/data/0a1b.lance"), Some(Part::LanceData));
        assert_eq!(
            part_of("d.lance/_indices/0a1b/page_data.lance"),
            Some(Part::LanceIndex)
        );
        assert_eq!(
            part_of("d.lance/_deletions/0-4-99.arrow"),
            Some(Part::LanceDeletion)
        );
        assert_eq!(part_of("d/data/a.parquet"), Some(Part::IcebergData));
    }

    /// A request at `ms`: PUT (with `bytes`), GET, DELETE or a copy.
    fn req(id: u64, ms: i64, method: &str, key: &str, bytes: u64) -> Record {
        let operation = match method {
            "PUT" => "PutObject",
            "DELETE" => "DeleteObject",
            "COPY" => "CopyObject",
            _ => "GetObject",
        };
        Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + ms).unwrap(),
            method: if method == "COPY" { "PUT" } else { method }.into(),
            kind: operation.into(),
            bucket: Some("lake".into()),
            key: Some(key.into()),
            status: Some(if method == "DELETE" { 204 } else { 200 }),
            latency_ms: 5.0,
            request_bytes: Some(bytes).into(),
            ..Default::default()
        }
    }

    #[test]
    fn hudi_and_paimon_keys_are_parts() {
        let instant = |s: &str| match part_of(s) {
            Some(Part::HudiInstant { action, state, .. }) => Some((action, state)),
            _ => None,
        };
        use sonde_proxy::part::HudiState::*;
        assert_eq!(
            instant("t/.hoodie/20261112093000123.commit"),
            Some(("commit".into(), Completed))
        );
        assert_eq!(
            instant("t/.hoodie/20261112093000123.deltacommit.requested"),
            Some(("deltacommit".into(), Requested))
        );
        assert_eq!(
            instant("t/.hoodie/20261112093000123.inflight"),
            Some(("commit".into(), Inflight))
        );
        // 1.x: the timeline folder, the completion time in the name
        assert_eq!(
            instant(".hoodie/timeline/20261112093000123_20261112093001456.replacecommit"),
            Some(("replacecommit".into(), Completed))
        );
        assert_eq!(instant("t/.hoodie/notes.txt"), None);
        assert_eq!(
            part_of("t/.hoodie/hoodie.properties"),
            Some(Part::HudiProperties)
        );
        assert_eq!(
            part_of("t/.hoodie/.temp/20261112093000123/dt=1/f1_0-1-2_2026.parquet.marker.CREATE"),
            Some(Part::HudiMarker)
        );
        assert_eq!(
            part_of("t/dt=1/3b2a-0_0-21-23_20261112093000123.parquet"),
            Some(Part::HudiData)
        );
        assert_eq!(
            part_of("t/dt=1/.3b2a-0_20261112093000123.log.1_0-21-23"),
            Some(Part::HudiData)
        );
        std::assert_matches!(
            part_of("t/.hoodie/metadata/.hoodie/20261112093000123.deltacommit"),
            Some(Part::HudiInstant { metadata: true, .. })
        );
        assert_eq!(
            part_of("p/snapshot/snapshot-7"),
            Some(Part::PaimonSnapshot(7))
        );
        assert_eq!(part_of("p/snapshot/LATEST"), Some(Part::PaimonHint));
        assert_eq!(part_of("p/schema/schema-0"), Some(Part::PaimonSchema(0)));
        assert_eq!(
            part_of("p/manifest/manifest-list-6f1a-0"),
            Some(Part::PaimonManifest)
        );
        assert_eq!(
            part_of("p/dt=1/bucket-0/data-6f1a-0.orc"),
            Some(Part::PaimonData)
        );
        // SlateDB's manifests stay SlateDB's
        assert_eq!(
            part_of("db/manifest/00000000000000000001.manifest"),
            Some(Part::SlateManifest(1))
        );
        assert_eq!(listed_format("t/.hoodie/").map(|x| x.0), Some(Format::Hudi));
    }

    #[test]
    fn a_hudi_write_is_one_commit_with_its_files() {
        let t = "t/.hoodie/20261112093000123";
        let rs = vec![
            req(1, 0, "GET", "t/.hoodie/hoodie.properties", 0),
            req(2, 10, "PUT", &format!("{t}.commit.requested"), 0),
            req(3, 20, "PUT", &format!("{t}.inflight"), 900),
            req(
                4,
                30,
                "PUT",
                "t/.hoodie/.temp/20261112093000123/dt=1/a.marker.CREATE",
                0,
            ),
            req(
                5,
                40,
                "PUT",
                "t/dt=1/a-0_0-1-2_20261112093000123.parquet",
                4000,
            ),
            req(
                6,
                50,
                "PUT",
                "t/dt=2/b-0_0-1-3_20261112093000123.parquet",
                6000,
            ),
            req(7, 60, "PUT", &format!("{t}.commit"), 1200),
            req(
                8,
                70,
                "DELETE",
                "t/.hoodie/.temp/20261112093000123/dt=1/a.marker.CREATE",
                0,
            ),
        ];
        let eps = episodes(&rs);
        let commit = eps
            .iter()
            .find(|e| e.format == Some(Format::Hudi) && e.records.contains(&7))
            .unwrap();
        assert_eq!(commit.records, [1, 2, 3, 4, 5, 6, 7], "{eps:#?}");
        assert_eq!(
            commit.title(),
            "commit 20261112093000123 · 2 files (9.8 KiB)"
        );
        // the markers cleared afterwards are said as deletes
        let cleared = eps.iter().find(|e| e.records == [8]).unwrap();
        assert_eq!(cleared.format, Some(Format::Hudi));
        assert_eq!(cleared.title(), "1 file deleted");
    }

    #[test]
    fn a_paimon_commit_is_its_snapshot() {
        let rs = vec![
            req(1, 0, "GET", "p/snapshot/LATEST", 0),
            req(2, 10, "PUT", "p/dt=1/bucket-0/data-6f1a-0.orc", 3000),
            req(3, 20, "PUT", "p/manifest/manifest-6f1a-0", 500),
            req(4, 30, "PUT", "p/manifest/manifest-list-6f1a-0", 200),
            req(5, 40, "PUT", "p/snapshot/snapshot-7", 300),
            req(6, 50, "PUT", "p/snapshot/LATEST", 1),
        ];
        let eps = episodes(&rs);
        let titles: Vec<(String, Option<Format>)> =
            eps.iter().map(|e| (e.title(), e.format)).collect();
        assert_eq!(
            titles,
            [
                (
                    "snapshot 7 · 1 file (2.9 KiB)".to_string(),
                    Some(Format::Paimon)
                ),
                ("LATEST hint updated".to_string(), Some(Format::Paimon)),
            ]
        );
    }

    #[test]
    fn a_rename_commit_and_directory_markers_are_findings() {
        let mut rs = Vec::new();
        let mut id = 0;
        let mut next = || {
            id += 1;
            id
        };
        for i in 0..4 {
            let (src, dst) = (
                format!("out/_temporary/0/task_{i}/part-{i}.parquet"),
                format!("out/part-{i}.parquet"),
            );
            let mut copy = req(next(), i * 100, "COPY", &dst, 0);
            copy.lists_mut().request_headers =
                vec![Header::new("x-amz-copy-source", format!("lake/{src}"))];
            rs.push(copy);
            rs.push(req(next(), i * 100 + 10, "DELETE", &src, 0));
            rs.push(req(next(), i * 100 + 20, "PUT", &format!("out/d{i}/"), 0));
            rs.push(req(
                next(),
                i * 100 + 30,
                "DELETE",
                &format!("out/d{i}/"),
                0,
            ));
        }
        let eps = episodes(&rs);
        let obs = observations(&rs, &eps);
        let find = |k: &str| {
            obs.iter()
                .find(|o| o.phrases[0].key == k)
                .unwrap_or_else(|| panic!("{k}: {obs:#?}"))
        };
        let rename = find("obs.rename");
        assert_eq!(rename.level, "warn");
        assert_eq!(rename.saves_requests, 8);
        assert!(
            rename
                .text
                .starts_with("4 objects copied out of _temporary/")
        );
        assert!(
            rename
                .fixes
                .iter()
                .any(|f| f.code.contains("committer.name=magic"))
        );
        let markers = find("obs.markers");
        assert_eq!(markers.saves_requests, 4);
        assert!(markers.text.starts_with("4 directory markers written"));
    }

    /// Whether keeping the first answer in place of the requests a finding
    /// saves would change what the program sees: each saved read answered
    /// as the first did, with no write to what it reads in between. What a
    /// hot key's cache and a listing kept from the first must hold.
    /// Whether each read a finding saves would be answered as the store
    /// did by the read of the same before it (one that failed answered
    /// nothing to keep): nothing written there between, the same status and
    /// ETag.
    fn a_kept_answer_is_the_same(records: &[Record], o: &Observation) -> bool {
        let by_id = |id: u64| records.iter().find(|r| r.id == id).unwrap();
        let etag = |r: &Record| response_header(r, "etag").map(str::to_string);
        let is_listing = |r: &Record| r.kind.starts_with("ListObjects");
        let place = |r: &Record| match query(r, "prefix") {
            Some(p) if is_listing(r) => p.to_string(),
            _ => r.key.as_deref().unwrap_or("").to_string(),
        };
        o.saved_ids.iter().map(|&id| by_id(id)).all(|saved| {
            if crate::summary::is_failure(saved) {
                return true;
            }
            let listing = is_listing(saved);
            let at = (saved.time, saved.id);
            let Some(kept) = records
                .iter()
                .filter(|r| crate::profile::is_read(r) && is_listing(r) == listing)
                .filter(|r| !crate::summary::is_failure(r))
                .filter(|r| place(r) == place(saved) && (r.time, r.id) < at)
                .max_by_key(|r| (r.time, r.id))
            else {
                return false;
            };
            let changed = records.iter().any(|w| {
                !crate::profile::is_read(w)
                    && (w.time, w.id) > (kept.time, kept.id)
                    && (w.time, w.id) < at
                    && w.key.as_deref().is_some_and(|k| {
                        if listing {
                            k.starts_with(&place(kept))
                        } else {
                            k == place(kept)
                        }
                    })
            });
            !changed && saved.status == kept.status && etag(saved) == etag(kept)
        })
    }

    #[test]
    fn a_hot_key_is_one_a_cache_answers_as_the_store_did() {
        let get = |id: u64, key: &str, status: u16| {
            rec(id, id as i64 * 10, "GET", "GetObject", Some(key), status)
        };
        let put = |id: u64, key: &str| rec(id, id as i64 * 10, "PUT", "PutObject", Some(key), 200);
        let hot = |rs: &[Record]| -> Vec<Observation> {
            let all = observations(rs, &episodes(rs));
            for o in all.iter().filter(|o| o.phrases[0].key == "obs.hot") {
                assert!(
                    a_kept_answer_is_the_same(rs, o),
                    "{} would change what the program reads",
                    o.text
                );
            }
            all.into_iter()
                .filter(|o| o.phrases[0].key == "obs.hot")
                .collect()
        };
        let traced = |rs: &[Record]| {
            traces(rs, &episodes(rs))
                .iter()
                .any(|o| o.phrases[0].key == "obs.hot")
        };
        // read 15 times, unchanged: a cache reads it once
        let same: Vec<Record> = (1..=15).map(|i| get(i, "conf.json", 200)).collect();
        assert_eq!(hot(&same).len(), 1);
        assert!(
            hot(&same)[0]
                .fixes
                .iter()
                .any(|f| f.code.contains("lru_cache"))
        );
        // written before each read: a cache would answer the first value
        let rewritten: Vec<Record> = (0..15u64)
            .flat_map(|i| {
                [
                    put(2 * i + 1, "state/counter"),
                    get(2 * i + 2, "state/counter", 200),
                ]
            })
            .collect();
        assert!(hot(&rewritten).is_empty());
        assert!(
            !traced(&rewritten),
            "nor is it a trace that tells a fix from less"
        );
        // written once before the reads and once after: the reads between are one value
        let mut around = vec![put(1, "conf.json")];
        around.extend((2..=16).map(|i| get(i, "conf.json", 200)));
        around.push(put(17, "conf.json"));
        assert_eq!(hot(&around).len(), 1);
        // deleted between them, by a DeleteObjects that names it
        let mut deleted: Vec<Record> = (1..=15).map(|i| get(i, "conf.json", 200)).collect();
        let mut batch = rec(8, 75, "POST", "DeleteObjects", None, 200);
        batch.rare_mut().request_body =
            Some("<Delete><Object><Key>conf.json</Key></Object></Delete>".into());
        deleted.push(batch);
        // each stretch between the writes on its own: 7 reads, then 8, a
        // cache reading each once
        let between = hot(&deleted);
        assert_eq!(between.len(), 1);
        assert_eq!(
            (
                between[0].phrases[0].args["n"].as_ref(),
                between[0].saves_requests
            ),
            ("15", 13)
        );
        // rewritten after its reads and read once more: the reads before
        // are a finding still (hot.py with its write)
        let mut once_more: Vec<Record> = (1..=25).map(|i| get(i, "conf.json", 200)).collect();
        once_more.push(put(26, "conf.json"));
        once_more.push(get(27, "conf.json", 200));
        let found = hot(&once_more);
        assert_eq!((found.len(), found[0].saves_requests), (1, 24));
        // changed by another writer: the store said another ETag
        let other: Vec<Record> = (1..=15)
            .map(|i| {
                let mut r = get(i, "conf.json", 200);
                r.lists_mut().response_headers = vec![reqlog::Header {
                    name: "ETag".into(),
                    value: if i < 8 {
                        "\"a\"".into()
                    } else {
                        "\"b\"".into()
                    },
                }];
                r
            })
            .collect();
        assert!(hot(&other).is_empty());
        // some reads throttled: they said nothing of the key, and a cache
        // would not have sent them either
        let throttled: Vec<Record> = (1..=15)
            .map(|i| get(i, "conf.json", if i % 4 == 0 { 503 } else { 200 }))
            .collect();
        assert_eq!(hot(&throttled).len(), 1);
        assert_eq!(hot(&throttled)[0].phrases[0].args["n"], "15");
        // missed once among them: a key polled for, not one to cache
        let mut polled = throttled.clone();
        polled.push(get(16, "conf.json", 404));
        assert!(hot(&polled).is_empty());
        // looked up and not found each time: lru_cache keeps no exception,
        // and a key polled for may appear
        let missing: Vec<Record> = (1..=12).map(|i| get(i, "nothing/here", 404)).collect();
        assert!(hot(&missing).is_empty());
        assert!(!traced(&missing));
        // read in ranges: the parallel reading of one object, not a key to keep whole
        let ranged: Vec<Record> = (1..=12)
            .map(|i| {
                let mut r = get(i, "big.parquet", 206);
                r.lists_mut().request_headers = vec![reqlog::Header {
                    name: "Range".into(),
                    value: format!("bytes={}-{}", i * 100, i * 100 + 99).into(),
                }];
                r
            })
            .collect();
        assert!(hot(&ranged).is_empty());
    }

    #[test]
    fn a_prefix_written_between_its_listings_is_not_one_to_list_once() {
        let list = |id: u64| {
            with_query(
                rec(id, id as i64 * 10, "GET", "ListObjectsV2", None, 200),
                &[("list-type", "2"), ("prefix", "inbox/")],
            )
        };
        let found = |rs: &[Record]| {
            let all = observations(rs, &episodes(rs));
            for o in all.iter().filter(|o| o.phrases[0].key == "obs.listed") {
                assert!(
                    a_kept_answer_is_the_same(rs, o),
                    "{} would change what the program sees",
                    o.text
                );
            }
            all.iter().any(|o| o.phrases[0].key == "obs.listed")
        };
        let listed: Vec<Record> = (1..=6).map(list).collect();
        assert!(found(&listed));
        // a file arrives between the listings: the program looks for it
        let mut polled = listed.clone();
        polled.push(rec(100, 35, "PUT", "PutObject", Some("inbox/new.csv"), 200));
        assert!(!found(&polled));
        // a write elsewhere changes nothing
        let mut elsewhere = listed.clone();
        elsewhere.push(rec(
            100,
            35,
            "PUT",
            "PutObject",
            Some("outbox/new.csv"),
            200,
        ));
        assert!(found(&elsewhere));
    }
}
