//! From requests to operations.
//!
//! A recorded session is grouped into [`Episode`]s: a multipart upload, a
//! retried request, a burst of lookups, a paged listing, or a step of a
//! storage format — a SlateDB manifest or WAL write, an Iceberg or Delta
//! commit — with the reads and file writes that led to it.
//!
//! The model per format is one idea: some writes are *commit points*. Other
//! requests of the format wait in a buffer and are folded into the next commit
//! point; a buffer that sits idle is flushed on its own (reads, or files
//! written without a commit — which [`observations`] reports).

use chrono::{DateTime, Duration, Utc};
use reqlog::Record;
use serde::Serialize;

use crate::phrase::{Phrase, p, plural};
use crate::summary::is_miss;

/// A storage format sonde recognizes from keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Format {
    SlateDB,
    Iceberg,
    Delta,
    Lance,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::SlateDB => "SlateDB",
            Format::Iceberg => "Iceberg",
            Format::Delta => "Delta",
            Format::Lance => "Lance",
        }
    }
}

/// What a key is within its format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    SlateManifest(u64),
    SlateWal(u64),
    SlateSst,
    SlateCompactions(u64),
    SlateGcBoundary,
    IcebergMetadata(Option<u64>),
    IcebergVersionHint,
    IcebergManifestList,
    IcebergManifest,
    IcebergData,
    DeltaCommit(u64),
    DeltaCheckpoint,
    DeltaLastCheckpoint,
    DeltaData,
    /// A dataset's version: `_versions/<n>.manifest`, written if absent.
    LanceManifest(u64),
    /// What a commit did, written before its manifest: `_transactions/*.txn`.
    LanceTransaction,
    /// A fragment's file: `data/*.lance`.
    LanceData,
    /// An index's file: `_indices/<uuid>/…`.
    LanceIndex,
    /// Rows marked deleted in a fragment: `_deletions/*`.
    LanceDeletion,
}

impl Part {
    pub fn format(&self) -> Format {
        use Part::*;
        match self {
            SlateManifest(_) | SlateWal(_) | SlateSst | SlateCompactions(_) | SlateGcBoundary => {
                Format::SlateDB
            }
            IcebergMetadata(_) | IcebergVersionHint | IcebergManifestList | IcebergManifest
            | IcebergData => Format::Iceberg,
            DeltaCommit(_) | DeltaCheckpoint | DeltaLastCheckpoint | DeltaData => Format::Delta,
            LanceManifest(_) | LanceTransaction | LanceData | LanceIndex | LanceDeletion => {
                Format::Lance
            }
        }
    }

    /// Whether a write of this part ends a step of the format.
    fn commits(&self) -> bool {
        use Part::*;
        matches!(
            self,
            SlateManifest(_)
                | SlateWal(_)
                | SlateSst
                | SlateCompactions(_)
                | IcebergMetadata(_)
                | IcebergVersionHint
                | DeltaCommit(_)
                | DeltaCheckpoint
                | DeltaLastCheckpoint
                | LanceManifest(_)
        )
    }
}

fn digits(s: &str, n: usize) -> Option<u64> {
    (s.len() == n && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// The format part a key names, from its last two path segments. A data
/// file's name is only a guess: [`episodes`] takes it when the session also
/// touched that table's log.
pub fn part_of(key: &str) -> Option<Part> {
    let mut segs = key.rsplit('/');
    let name = segs.next()?;
    let dir = segs.next().unwrap_or("");
    // an index's files are a folder deeper: _indices/<uuid>/<file>
    if segs.next() == Some("_indices") {
        return Some(Part::LanceIndex);
    }
    let stem = |ext: &str| name.strip_suffix(ext);
    match dir {
        "manifest" => stem(".manifest")
            .and_then(|s| digits(s, 20))
            .map(Part::SlateManifest),
        "wal" => stem(".sst").and_then(|s| digits(s, 20)).map(Part::SlateWal),
        "compacted" if name.ends_with(".sst") => Some(Part::SlateSst),
        "compactions" => stem(".compactions")
            .and_then(|s| digits(s, 20))
            .map(Part::SlateCompactions),
        "gc" if name.ends_with(".boundary") => Some(Part::SlateGcBoundary),
        // newer datasets name a version by what is left to u64's end, so the
        // newest lists first; older ones by the version itself
        "_versions" => stem(".manifest").and_then(|s| {
            digits(s, 20)
                .map(|n| u64::MAX - n)
                .or_else(|| s.parse().ok())
                .map(Part::LanceManifest)
        }),
        "_transactions" if name.ends_with(".txn") => Some(Part::LanceTransaction),
        "_deletions" if name.ends_with(".arrow") || name.ends_with(".bin") => {
            Some(Part::LanceDeletion)
        }
        "data" if name.ends_with(".lance") => Some(Part::LanceData),
        "_delta_log" => {
            if let Some(n) = stem(".json").and_then(|s| digits(s, 20)) {
                Some(Part::DeltaCommit(n))
            } else if name == "_last_checkpoint" {
                Some(Part::DeltaLastCheckpoint)
            } else if name.contains(".checkpoint") {
                Some(Part::DeltaCheckpoint)
            } else {
                None
            }
        }
        "metadata" => {
            if let Some(s) = stem(".metadata.json") {
                // v3.metadata.json or 00003-<uuid>.metadata.json
                let v = s
                    .strip_prefix('v')
                    .and_then(|n| n.parse().ok())
                    .or_else(|| s.split('-').next().and_then(|n| n.parse().ok()));
                Some(Part::IcebergMetadata(v))
            } else if name == "version-hint.text" {
                Some(Part::IcebergVersionHint)
            } else if name.starts_with("snap-") && name.ends_with(".avro") {
                Some(Part::IcebergManifestList)
            } else if name.ends_with(".avro") {
                Some(Part::IcebergManifest)
            } else {
                None
            }
        }
        _ => {
            // data files: under …/data/ next to metadata/ (Iceberg) or _delta_log/ (Delta)
            if name.ends_with(".parquet") && key.contains("/data/") {
                Some(Part::IcebergData)
            } else if name.ends_with(".parquet") && name.starts_with("part-") {
                Some(Part::DeltaData)
            } else {
                None
            }
        }
    }
}

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
        _ => return None,
    };
    Some((f, dir))
}

/// The tables a session touched the log of: a `_delta_log/` key or listing
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
                .operation
                .starts_with("ListObjects")
                .then(|| query(r, "prefix").unwrap_or("").trim_end_matches('/'));
            let Some(path) = r.key.as_deref().or(listed) else {
                continue;
            };
            let part = part_of(path);
            if matches!(part, Some(Part::IcebergData | Part::DeltaData)) {
                continue;
            }
            // a listing of the directory itself has no part, a key in it does
            let format = match (&part, listed) {
                (Some(p), _) => Some(p.format()),
                (None, Some(prefix)) => listed_format(prefix).map(|(f, _)| f),
                (None, None) => None,
            };
            let dir = match format {
                Some(Format::Delta) => "_delta_log",
                Some(Format::Iceberg) => "metadata",
                _ => continue,
            };
            if let Some(root) = root_before(path, dir) {
                let entry = (r.bucket.clone(), format.expect("matched above"), root);
                if !roots.contains(&entry) {
                    roots.push(entry);
                }
            }
        }
        Tables { roots }
    }

    /// The part a request's key names, when the session gives a reason to
    /// believe it: a data file counts only beside its table's log.
    fn part(&self, r: &Record) -> Option<Part> {
        let key = r.key.as_deref()?;
        let part = part_of(key)?;
        match part {
            Part::IcebergData | Part::DeltaData => self.data_part(&r.bucket, key),
            _ => Some(part),
        }
    }

    /// Whose data file a Parquet file is: the table's whose log the session
    /// touched, the deepest when one table is under another. The path alone
    /// does not say: a Delta table under `…/data/` has its files there too.
    fn data_part(&self, bucket: &Option<String>, key: &str) -> Option<Part> {
        self.roots
            .iter()
            .filter(|(b, _, root)| b == bucket && key.starts_with(root.as_str()))
            .max_by_key(|(_, _, root)| root.len())
            .and_then(|(_, f, _)| match f {
                Format::Iceberg => Some(Part::IcebergData),
                Format::Delta => Some(Part::DeltaData),
                _ => None,
            })
    }

    /// The format of a request: from its key, or from a listing's prefix.
    fn format_of(&self, r: &Record) -> Option<Format> {
        if r.key.is_some() {
            return self.part(r).map(|p| p.format());
        }
        if !r.operation.starts_with("ListObjects") {
            return None;
        }
        let prefix = query(r, "prefix").unwrap_or("");
        let trimmed = prefix.trim_end_matches('/');
        match part_of(trimmed) {
            // a listing of a data file's path is of a table only when the table is
            Some(Part::IcebergData | Part::DeltaData) => {
                self.data_part(&r.bucket, trimmed).map(|p| p.format())
            }
            _ => listed_format(prefix).map(|(f, _)| f),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
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

/// One operation: requests that belong together.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    pub kind: Kind,
    pub format: Option<Format>,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Record ids, in time order.
    pub records: Vec<u64>,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub outcome: Outcome,
    pub notes: Vec<String>,
    /// Its lookups (GET, HEAD) answered 404: not errors, and worth telling apart.
    pub not_found: usize,
    /// The title and the notes as phrases, for saying them in other languages.
    pub title_text: Vec<Phrase>,
    pub notes_text: Vec<Phrase>,
}

impl Episode {
    /// Adds a note, in English and as a phrase.
    fn note(&mut self, phrase: Phrase) {
        self.notes.push(phrase.en());
        self.notes_text.push(phrase);
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
    r.response_headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("x-sonde-proxy"))
}

/// S3's code in an error body, as it stands there (nothing is copied:
/// [`retryable`] asks of every record).
fn s3_code(r: &Record) -> Option<&str> {
    let b = r.error_body.as_deref()?;
    let start = b.find("<Code>")? + 6;
    Some(&b[start..start + b[start..].find("</Code>")?])
}

fn failure(r: &Record) -> Option<String> {
    if r.transport_error.is_some() {
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
    r.request_headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn response_header<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.response_headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn query<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.query
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn human(b: u64) -> String {
    human_bytes(b)
}

/// A size as it is written everywhere: the one ladder
/// ([`crate::signals::bytes_text`]).
pub(crate) fn human_bytes(b: u64) -> String {
    crate::signals::bytes_text(b as f64)
}

/// Payload bytes a request uploads (not the XML of Create / Complete).
fn data_bytes(r: &Record) -> u64 {
    match r.operation.as_str() {
        "PutObject" | "UploadPart" => r.request_bytes.unwrap_or(0),
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
fn build(kind: Kind, format: Option<Format>, title: Vec<Phrase>, mut rs: Vec<&Record>) -> Episode {
    rs.sort_by_key(|r| (r.time, r.id));
    let start = rs.first().map(|r| r.time).unwrap_or_default();
    let end = rs.iter().map(|r| end_of(r)).max().unwrap_or(start);
    let last = rs.last().copied();
    let outcome = match last {
        Some(r) if refused(r) => Outcome::Refused,
        Some(r) => failure(r).map_or(Outcome::Ok, Outcome::Failed),
        None => Outcome::Ok,
    };
    Episode {
        kind,
        format,
        title: crate::phrase::en(&title),
        title_text: title,
        start,
        end,
        records: rs.iter().map(|r| r.id).collect(),
        bytes_up: rs.iter().map(|r| r.request_bytes.unwrap_or(0)).sum(),
        bytes_down: rs
            .iter()
            .filter(|r| !r.is_error())
            .map(|r| r.response_bytes.unwrap_or(0))
            .sum(),
        outcome,
        notes: Vec::new(),
        notes_text: Vec::new(),
        not_found: rs.iter().filter(|r| crate::summary::is_miss(r)).count(),
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
    let mut out = Vec::new();

    // a completed multipart upload of a format's file is one write of it
    let tables = Tables::of(&sorted);
    let mut units = multipart(&sorted, &tables, &mut used, &mut out);
    // a format's write retried is still that write: its attempts go with it to the step
    units.extend(retries(&sorted, &tables, &mut used, &mut out));

    // what is left, in order: format steps, bursts of lookups, listings, single requests
    let mut items: Vec<Vec<&Record>> = units;
    items.extend(
        sorted
            .iter()
            .enumerate()
            .filter(|(i, _)| !used[*i])
            .map(|(_, r)| vec![*r]),
    );
    items.sort_by_key(|u| (u[0].time, u[0].id));
    let mut buffers: Vec<(Format, Vec<&Record>)> = Vec::new();
    let mut probe: Vec<&Record> = Vec::new();
    let mut listing: Vec<&Record> = Vec::new();
    // requests that are nothing more: most of a large session, made into
    // operations together once the rest is placed
    let mut alone: Vec<&Record> = Vec::new();
    for unit in items {
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
            buffers[pos].1.extend(unit);
            if is_write(r) && part.as_ref().is_some_and(Part::commits) {
                let (_, rs) = buffers.remove(pos);
                out.extend(step(f, rs));
            }
            continue;
        }
        if r.operation == "ListObjectsV2" || r.operation == "ListObjects" {
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
    out.extend(singles(&alone));
    for (f, rs) in buffers {
        out.push(loose(f, rs));
    }
    if !probe.is_empty() {
        out.extend(flush_probe(probe));
    }
    if !listing.is_empty() {
        out.push(flush_listing(listing));
    }
    out.sort_by_key(|e| (e.start, e.records.first().copied()));
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

/// Each request an operation of its own ([`single`]), on the cores there
/// are: a session of a million requests is mostly these, and each is a
/// title and its phrases to write. (The operations are sorted by their
/// start afterwards: the order they are made in is not seen.)
fn singles(rs: &[&Record]) -> Vec<Episode> {
    const WORTH: usize = 20_000;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    if threads < 2 || rs.len() < WORTH {
        return rs.iter().map(|r| single(r)).collect();
    }
    let each = rs.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let parts: Vec<_> = rs
            .chunks(each)
            .map(|part| scope.spawn(move || part.iter().map(|r| single(r)).collect::<Vec<_>>()))
            .collect();
        let mut out = Vec::with_capacity(rs.len());
        for part in parts {
            match part.join() {
                Ok(made) => out.extend(made),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
        out
    })
}

fn single(r: &Record) -> Episode {
    let target = match (&r.bucket, &r.key) {
        (Some(b), Some(k)) => format!("{b}/{k}"),
        (Some(b), None) => format!("{b}/"),
        _ => "/".into(),
    };
    let mut title = vec![p(
        "request",
        &[("op", r.operation.clone()), ("target", target)],
    )];
    if let Some(x) = query(r, "prefix").filter(|x| !x.is_empty()) {
        title.push(p("request.prefix", &[("prefix", x.to_string())]));
    }
    if matches!(r.method.as_str(), "PUT" | "POST") && r.request_bytes.unwrap_or(0) > 0 {
        title.push(p("size", &[("size", human(r.request_bytes.unwrap_or(0)))]));
    }
    let mut e = build(Kind::Request, None, title, vec![r]);
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
    let bucket = rs[0].bucket.clone().unwrap_or_default();
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
        Kind::Probe,
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
    let bucket = rs[0].bucket.clone().unwrap_or_default();
    let n = rs.len();
    build(
        Kind::Listing,
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
        match r.operation.as_str() {
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
            .all(|&i| sorted[i].is_error() && sorted[i].operation == "CreateMultipartUpload")
        {
            continue;
        }
        for &i in &idx {
            used[i] = true;
        }
        let rs: Vec<&Record> = idx.iter().map(|&i| sorted[i]).collect();
        let parts = rs
            .iter()
            .filter(|r| r.operation.starts_with("UploadPart"))
            .count();
        let bytes: u64 = rs
            .iter()
            .filter(|r| r.operation == "UploadPart")
            .map(|r| r.request_bytes.unwrap_or(0))
            .sum();
        let completed = rs.iter().any(completes);
        let aborted = rs.iter().any(|r| r.operation == "AbortMultipartUpload");
        let formatted = tables.part(rs[0]).is_some();
        if completed && formatted {
            let mut unit = rs;
            unit.sort_by_key(|r| (r.operation == "CompleteMultipartUpload", r.time, r.id));
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
        } else if !completed {
            title.push(p("mpu.open", &[]));
        }
        let mut e = build(Kind::Multipart, None, title, rs);
        if !completed && !aborted {
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
    r.operation == "CompleteMultipartUpload"
        && (!r.is_error()
            || response_header(r, "x-sonde-store-status").is_some_and(|s| s.starts_with('2')))
}

/// Whether sonde failed the request in the store's place.
fn sondes_fault(r: &Record) -> bool {
    r.injected.is_some() && response_header(r, "x-sonde-fault").is_some()
}

/// The statuses a retry is owed for: the ones the AWS SDKs send again. A 501
/// or a 505 is the store's answer however often it is asked, like a 404. The
/// trial's verdict and the retries read from a session both go by this.
pub fn status_owes_retry(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

/// Whether a client is expected to send the request again: no answer (or
/// one cut short), a status that says so ([`status_owes_retry`]), or the
/// store asking for fewer.
pub(crate) fn retryable(r: &Record) -> bool {
    r.transport_error.is_some()
        || r.status.is_some_and(status_owes_retry)
        || s3_code(r).is_some_and(|c| c == "SlowDown" || c == "RequestTimeout")
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
        &'r [(String, String)],
    );
    fn identity<'r>(r: &'r Record) -> Identity<'r> {
        (
            r.method.as_str(),
            r.bucket.as_deref(),
            r.key.as_deref(),
            r.query.as_slice(),
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
            let target = rs[0].key.clone().unwrap_or_default();
            let mut e = build(
                Kind::Retry,
                None,
                vec![p(
                    "retry",
                    &[
                        ("op", rs[0].operation.clone()),
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
        _ => p(
            "step.other",
            &[
                ("op", commit.operation.clone()),
                ("key", commit.key.clone().unwrap_or_default()),
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
    let mut e = build(Kind::Step, Some(f), title, rs.clone());
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
            Kind::Step,
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
            .map(|r| r.response_bytes.unwrap_or(0))
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
    build(Kind::Step, Some(f), title, rs)
}

/// Something worth knowing about a session.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    /// `warn` (likely a problem) or `info`.
    pub level: &'static str,
    pub text: String,
    /// The text as phrases, for saying it in other languages.
    pub phrases: Vec<Phrase>,
    /// The episodes it is about (indexes, ascending), for showing them.
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
    /// The requests acting on it would not send, by id: what "acting on N
    /// findings" takes away (their own waiting, their own price).
    #[serde(skip)]
    pub saved_ids: Vec<u64>,
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
    pub callers: Vec<crate::callers::Caller>,
    /// The SDK that sent them, from their user agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk: Option<String>,
    /// The same finding across runs (`sonde.toml`'s `expect` names it so).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity: String,
}

fn is_zero(n: &usize) -> bool {
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
                    ("pct", (saves * 100 / total.max(1)).max(1).to_string()),
                ],
            );
            self.saving_text = Some(saving.en());
            self.saving = Some(saving);
        }
        self
    }
}

/// Notes that hold for (nearly) every step of a format: said once for the
/// session rather than on each line. Returns (format, note, steps with it,
/// steps of the format).
pub fn shared_notes(episodes: &[Episode]) -> Vec<(Format, String, usize, usize)> {
    let mut out = Vec::new();
    for f in [
        Format::SlateDB,
        Format::Iceberg,
        Format::Delta,
        Format::Lance,
    ] {
        let of_f: Vec<&Episode> = episodes.iter().filter(|e| e.format == Some(f)).collect();
        let mut notes: Vec<&String> = of_f.iter().flat_map(|e| e.notes.iter()).collect();
        notes.sort();
        notes.dedup();
        for n in notes {
            let with = of_f.iter().filter(|e| e.notes.contains(n)).count();
            // writes only: reads never carry a write's note
            let writes = of_f.iter().filter(|e| !e.notes.is_empty()).count();
            if with >= 3 && with * 10 >= writes * 8 && !n.starts_with("after ") {
                out.push((f, n.clone(), with, of_f.len()));
            }
        }
    }
    out
}

/// Something for each request, by its id. A session's ids are 1 to n (or
/// near it): a table by id then, with no hashing; a map when they are not.
enum ById<V> {
    Table(u64, Vec<Option<V>>),
    Map(std::collections::HashMap<u64, V>),
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

/// What stands out across a session's requests and episodes.
pub fn observations(records: &[Record], episodes: &[Episode]) -> Vec<Observation> {
    use std::collections::{BTreeSet, HashMap};
    // each record by its id: read once, for every finding that names requests
    let by_id: ById<&Record> = ById::of(records.len(), || records.iter().map(|r| (r.id, r)));
    // which episode each record belongs to
    let owner: ById<usize> = ById::of(records.len(), || {
        episodes
            .iter()
            .enumerate()
            .flat_map(|(i, e)| e.records.iter().map(move |r| (*r, i)))
    });
    let of_records = |ids: &mut dyn Iterator<Item = u64>| -> Vec<usize> {
        ids.filter_map(|id| owner.get(&id).copied())
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
    for (f, note, with, _) in shared_notes(episodes) {
        // the note as a phrase: from an episode that has it
        let note_phrase = episodes.iter().find_map(|e| {
            e.notes
                .iter()
                .position(|n| *n == note)
                .and_then(|i| e.notes_text.get(i).cloned())
        });
        let mut phrases = vec![p(
            "obs.shared",
            &[("n", with.to_string()), ("format", f.name().to_string())],
        )];
        phrases.extend(note_phrase);
        out.push(Observation {
            level: "info",
            text: format!("{with} {} writes: {note}", f.name()),
            phrases,
            episodes: of_episodes(&|e| e.format == Some(f) && e.notes.contains(&note)),
            impact: 0,
            saving: None,
            saving_text: None,
            saves_requests: 0,
            saves_ms: 0.0,
            saved_ids: Vec::new(),
            timed_ids: Vec::new(),
            fixes: Vec::new(),
            callers: Vec::new(),
            sdk: None,
            identity: String::new(),
        });
    }
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
        saved_ids: Vec::new(),
        timed_ids: Vec::new(),
        fixes: Vec::new(),
        callers: Vec::new(),
        sdk: None,
        identity: String::new(),
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
        saved_ids: Vec::new(),
        timed_ids: Vec::new(),
        fixes: Vec::new(),
        callers: Vec::new(),
        sdk: None,
        identity: String::new(),
    };
    let total = records.len().max(1);
    let requests_of = |eps: &[usize]| eps.iter().map(|&i| episodes[i].requests()).sum::<usize>();
    let unfinished =
        of_episodes(&|e| e.kind == Kind::Multipart && e.title.ends_with("(not completed)"));
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
    // a trial's faults are sonde's doing: said first among the notes, so
    // that the errors and retries below are not read as the store's
    let injected: Vec<&Record> = records.iter().filter(|r| r.injected.is_some()).collect();
    if let Some(first) = injected.first() {
        let n = injected.len();
        out.push(
            info(
                p(
                    "obs.injected",
                    &[
                        ("n", n.to_string()),
                        ("s", plural(n)),
                        ("scenario", first.injected.clone().unwrap_or_default()),
                    ],
                ),
                of_records(&mut injected.iter().map(|r| r.id)),
            )
            .weigh(n, 0, total),
        );
    }
    let orphans = of_episodes(&|e| e.title.contains("no commit followed"));
    if !orphans.is_empty() {
        let n = orphans.len();
        let impact = requests_of(&orphans);
        out.push(
            warn(
                if n == 1 {
                    p("obs.orphans.one", &[])
                } else {
                    p("obs.orphans", &[("n", n.to_string())])
                },
                orphans,
            )
            .weigh(impact, 0, total),
        );
    }
    // a retry after a fault sonde injected is the trial's doing, said by the
    // finding above: only what followed the store's own failures is the store's
    let own_failure: std::collections::HashSet<u64> = records
        .iter()
        .filter(|r| retryable(r) && !sondes_fault(r))
        .map(|r| r.id)
        .collect();
    let injected_ids: std::collections::HashSet<u64> = records
        .iter()
        .filter(|r| sondes_fault(r))
        .map(|r| r.id)
        .collect();
    let retries = of_episodes(&|e| {
        e.kind == Kind::Retry
            && (e.records.iter().any(|id| own_failure.contains(id))
                || !e.records.iter().any(|id| injected_ids.contains(id)))
    });
    let retried: usize = retries.iter().map(|&i| episodes[i].requests() - 1).sum();
    if retried > 0 {
        out.push(
            warn(
                p(
                    "obs.retried",
                    &[("n", retried.to_string()), ("s", plural(retried))],
                ),
                retries,
            )
            .weigh(retried, 0, total),
        );
    }
    // small files uploaded in parts: 3 requests where 1 PUT would do
    let mut uploads: HashMap<&str, (usize, u64, bool, Vec<u64>)> = HashMap::new();
    for r in records {
        if let Some(id) = query(r, "uploadId") {
            let u = uploads.entry(id).or_default();
            if r.operation == "UploadPart" {
                u.0 += 1;
                u.1 += r.request_bytes.unwrap_or(0);
            }
            u.2 |= r.operation == "CompleteMultipartUpload" && !r.is_error();
            u.3.push(r.id);
        }
    }
    // each upload began with a CreateMultipartUpload (`?uploads`, no upload
    // id): the creates of a key go to its uploads in order, each the earliest
    // left before the upload's first part
    let mut uploads: Vec<(usize, u64, bool, Vec<u64>)> = uploads.into_values().collect();
    {
        use std::collections::VecDeque;
        let at = &by_id;
        let mut creates: HashMap<(Option<&str>, Option<&str>), VecDeque<&Record>> = HashMap::new();
        let mut sorted: Vec<&Record> = records
            .iter()
            .filter(|r| r.operation == "CreateMultipartUpload" && !r.is_error())
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
                .copied()
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
    if small.len() >= 3 {
        let impact: usize = small.iter().map(|u| u.3.len()).sum();
        out.push(
            info(
                p("obs.small", &[("n", small.len().to_string())]),
                of_records(&mut small.iter().flat_map(|u| u.3.iter().copied())),
            )
            // one PUT for each: the rest of each upload's requests go
            .weigh(impact, impact - small.len(), total)
            .saving_of(
                small
                    .iter()
                    .flat_map(|u| u.3.iter().copied())
                    .filter(|id| by_id.get(id).is_some_and(|r| r.operation != "UploadPart"))
                    .collect(),
            ),
        );
    }
    let races: Vec<u64> = records
        .iter()
        // a 412 sonde answered itself (a trial's lost race) is not the store's
        .filter(|r| r.status == Some(412) && !sondes_fault(r))
        .map(|r| r.id)
        .collect();
    if !races.is_empty() {
        out.push(
            info(
                p(
                    "obs.races",
                    &[("n", races.len().to_string()), ("s", plural(races.len()))],
                ),
                of_records(&mut races.iter().copied()),
            )
            .weigh(races.len(), 0, total),
        );
    }
    // the same key read again and again (counted by the records' own
    // names: only a hot key's is written out)
    let mut reads: HashMap<(&str, &str), Vec<u64>> = HashMap::new();
    for r in records
        .iter()
        .filter(|r| r.method == "GET" || r.method == "HEAD")
    {
        if let (Some(b), Some(k)) = (&r.bucket, &r.key) {
            reads.entry((b, k)).or_default().push(r.id);
        }
    }
    let mut hot: Vec<(String, Vec<u64>)> = reads
        .into_iter()
        .filter(|(_, ids)| ids.len() >= 10 && ids.len() * 10 >= total)
        .map(|((b, k), ids)| (format!("{b}/{k}"), ids))
        .collect();
    hot.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    for (k, ids) in hot.into_iter().take(3) {
        let n = ids.len();
        let again = ids[1..].to_vec();
        out.push(
            info(
                p(
                    "obs.hot",
                    &[
                        ("key", k),
                        ("n", n.to_string()),
                        ("pct", (n * 100 / total).to_string()),
                    ],
                ),
                of_records(&mut ids.into_iter()),
            )
            // a cache reads it once
            .weigh(n, n - 1, total)
            .saving_of(again),
        );
    }
    // the same prefix listed again and again
    let mut lists: HashMap<String, Vec<u64>> = HashMap::new();
    for r in records.iter().filter(|r| {
        // a later page (ListObjectsV2's token, ListObjects' marker) is the
        // same listing going on
        r.operation.starts_with("ListObjects")
            && query(r, "continuation-token").is_none()
            && query(r, "marker").is_none_or(str::is_empty)
    }) {
        lists
            .entry(format!(
                "{}/{}",
                r.bucket.as_deref().unwrap_or(""),
                query(r, "prefix").unwrap_or("")
            ))
            .or_default()
            .push(r.id);
    }
    let mut repeated: Vec<(String, Vec<u64>)> = lists
        .into_iter()
        .filter(|(_, ids)| ids.len() >= 5)
        .collect();
    repeated.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    for (prefix_name, ids) in repeated.into_iter().take(3) {
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
    // HEAD, then GET of the same object: the GET's answer has the HEAD's
    let mut headed: HashMap<(&str, &str), (u64, chrono::DateTime<chrono::Utc>)> = HashMap::new();
    let mut pairs: Vec<u64> = Vec::new();
    let mut checked = 0usize;
    for r in records {
        let (Some(b), Some(k)) = (r.bucket.as_deref(), r.key.as_deref()) else {
            continue;
        };
        match r.operation.as_str() {
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
                    && (r.time - at).num_milliseconds() <= 2_000
                {
                    pairs.extend([head, r.id]);
                    checked += 1;
                }
            }
            _ => {}
        }
    }
    if checked >= 3 {
        out.push(
            info(
                p("obs.headget", &[("n", checked.to_string())]),
                of_records(&mut pairs.iter().copied()),
            )
            .weigh(pairs.len(), checked, total)
            // the HEAD of each pair
            .saving_of(pairs.iter().step_by(2).copied().collect()),
        );
    }
    // throttled: and how many requests were in flight when it happened
    // (the store's own: a 503 sonde answered in its place asks for nothing)
    let throttled: Vec<&Record> = records
        .iter()
        .filter(|r| matches!(r.status, Some(503) | Some(429)) && !sondes_fault(r))
        .collect();
    if !throttled.is_empty() {
        let n = throttled.len();
        let mut starts: Vec<i64> = records.iter().map(|r| r.time.timestamp_micros()).collect();
        let mut ends: Vec<i64> = records
            .iter()
            .map(|r| r.time.timestamp_micros() + r.latency_us())
            .collect();
        starts.sort_unstable();
        ends.sort_unstable();
        // started by then and not yet answered (itself included)
        // (a request that took no time still counts itself; odd latencies never
        // make the count negative)
        let in_flight = |t: i64| {
            (starts.partition_point(|s| *s <= t))
                .saturating_sub(ends.partition_point(|e| *e < t))
                .max(1)
        };
        let least = throttled
            .iter()
            .map(|r| in_flight(r.time.timestamp_micros()))
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
        out.push(warn(phrase, of_records(&mut throttled.iter().map(|r| r.id))).weigh(n, 0, total));
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
            if n * 2 >= limit {
                let prefix = prefix_named(prefix.0, prefix.1);
                hot.push((prefix, n, limit, read, ids, throttled));
            }
        }
        // nearest its limit first: n/limit, compared without dividing
        hot.sort_by(|a, b| (b.1 * a.2).cmp(&(a.1 * b.2)).then(a.0.cmp(&b.0)));
        for (prefix, n, limit, read, ids, throttled) in hot.into_iter().take(3) {
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
            let o = if pct >= 80 || throttled > 0 {
                warn(phrase, of_records(&mut ids.into_iter()))
            } else {
                info(phrase, of_records(&mut ids.into_iter()))
            };
            out.push(o.weigh(impact, 0, total));
        }
    }
    // requests one after another, each waiting for the one before
    {
        use crate::profile::{CHAIN_CONCURRENCY, chains};
        let found = chains(records);
        let serial: usize = found.iter().map(|c| c.requests).sum();
        let wall: f64 = found.iter().map(|c| c.end_ms - c.start_ms).sum();
        let fast: f64 = found.iter().map(|c| c.at_once_ms).sum();
        // only the waiting goes: the program's own work between requests stays
        let waiting: f64 = found.iter().map(|c| c.waiting_ms).sum();
        let saved = (waiting - fast).max(0.0);
        if !found.is_empty() && saved >= 100.0 {
            let busy = crate::profile::busy_ms(records).max(1e-9);
            let time = |v: f64| {
                if v >= 1000.0 {
                    format!("{:.1} s", v / 1000.0)
                } else {
                    format!("{v:.0} ms")
                }
            };
            let ids: Vec<u64> = found.iter().flat_map(|c| c.ids.iter().copied()).collect();
            let chained = ids.clone();
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
                of_records(&mut ids.into_iter()),
            );
            if saved * 4.0 < busy {
                o.level = "info";
            }
            o.impact = serial;
            let s = p(
                "obs.savesTime",
                &[
                    ("time", time(saved)),
                    (
                        "pct",
                        ((saved * 100.0 / busy).round() as usize)
                            .min(100)
                            .to_string(),
                    ),
                ],
            );
            o.saving_text = Some(s.en());
            o.saving = Some(s);
            o.saves_ms = saved;
            o.timed_ids = chained;
            out.push(o);
        }
    }
    // far slower than the rest: a second or more, and ten times the median
    if records.len() >= 10 {
        let mut lat: Vec<f64> = records.iter().map(|r| r.latency_ms).collect();
        lat.sort_by(f64::total_cmp);
        let median = lat[lat.len() / 2];
        let over = (median * 10.0).max(1_000.0);
        let slow: Vec<&Record> = records.iter().filter(|r| r.latency_ms >= over).collect();
        if let Some(worst) = slow
            .iter()
            .max_by(|a, b| a.latency_ms.total_cmp(&b.latency_ms))
        {
            let n = slow.len();
            let what = match &worst.key {
                Some(k) => format!("{} {k}", worst.operation),
                None => worst.operation.clone(),
            };
            out.push(
                info(
                    p(
                        "obs.slow",
                        &[
                            ("n", n.to_string()),
                            ("s", plural(n)),
                            ("ms", format!("{over:.0}")),
                            ("median", format!("{median:.0}")),
                            ("op", what),
                            ("max", format!("{:.0}", worst.latency_ms)),
                        ],
                    ),
                    of_records(&mut slow.iter().map(|r| r.id)),
                )
                .weigh(n, 0, total),
            );
        }
    }
    // the most telling first: warnings, then by the requests they bear on
    // (a trial's faults before all: what follows is read in their light)
    let injects = |o: &Observation| o.phrases.first().is_some_and(|p| p.key == "obs.injected");
    out.sort_by(|a, b| {
        injects(b)
            .cmp(&injects(a))
            .then((b.level == "warn").cmp(&(a.level == "warn")))
            .then(b.impact.cmp(&a.impact))
    });
    for o in &mut out {
        if let Some(first) = o.phrases.first() {
            o.fixes = crate::fixes::for_key(first.key);
        }
        // who in the program sent its requests
        let rs: Vec<&Record> = o
            .episodes
            .iter()
            .filter_map(|&i| episodes.get(i))
            .flat_map(|e| e.records.iter().filter_map(|id| by_id.get(id).copied()))
            .collect();
        (o.callers, o.sdk) = crate::callers::of(&rs);
        o.identity = crate::profile::finding_identity(o);
    }
    out
}

/// [`observations`] of a session, read with its header: a session whose
/// times are to the second (an imported log's:
/// [`crate::session::SessionHeader::whole_seconds`]) says nothing that
/// depends on where in its second a request began. Requests "one after
/// another" are not found in it (spread over their second they all look
/// so), and a throttling is said without how many were in flight.
pub fn observations_of(
    header: &crate::session::SessionHeader,
    records: &[Record],
    episodes: &[Episode],
) -> Vec<Observation> {
    let mut out = observations(records, episodes);
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
                    ("n", said.args.get("n").cloned().unwrap_or_default()),
                    ("s", said.args.get("s").cloned().unwrap_or_default()),
                ],
            );
            o.text = phrase.en();
            o.phrases = vec![phrase];
            o.fixes = crate::fixes::for_key("obs.throttled.rate");
            o.identity = crate::profile::finding_identity(o);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqlog::Header;

    #[test]
    fn a_failures_retry_is_the_next_request_of_its_own_name_within_a_minute() {
        let at = |id: u64, ms: i64, key: &str, status: u16| Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + ms).unwrap(),
            method: "GET".into(),
            operation: "GetObject".into(),
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
        let retried: Vec<&Episode> = eps.iter().filter(|e| e.kind == Kind::Retry).collect();
        assert_eq!(retried.len(), 1, "{eps:#?}");
        assert_eq!(retried[0].records, [1, 2, 3]);
        assert_eq!(retried[0].title, "GetObject a retried 2× after 503");
        // every other failure stands alone: 200 keys, and the late one
        assert_eq!(eps.len(), 1 + 200 + 2);
    }

    #[test]
    fn creates_go_to_their_uploads_in_order_whatever_else_shares_the_key() {
        let at = |id: u64, ms: i64, op: &str, q: &[(&str, &str)], status: u16| Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + ms).unwrap(),
            method: if op == "UploadPart" { "PUT" } else { "POST" }.into(),
            operation: op.into(),
            bucket: Some("b".into()),
            key: Some("big".into()),
            query: q
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            status: Some(status),
            request_bytes: Some(if op == "UploadPart" { 8 << 20 } else { 0 }),
            latency_ms: 5.0,
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
        let uploads: Vec<&Episode> = eps.iter().filter(|e| e.kind == Kind::Multipart).collect();
        assert_eq!(uploads.len(), 2, "{eps:#?}");
        assert_eq!(uploads[0].records, [1, 2, 4, 6]);
        assert_eq!(uploads[1].records, [3, 5, 7]);
        assert!(
            eps.iter()
                .any(|e| e.kind == Kind::Request && e.records == [8])
        );
    }

    fn rec(id: u64, at_ms: i64, method: &str, op: &str, key: Option<&str>, status: u16) -> Record {
        Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
            method: method.into(),
            operation: op.into(),
            bucket: Some("b".into()),
            key: key.map(Into::into),
            status: Some(status),
            latency_ms: 5.0,
            request_bytes: Some(if method == "PUT" { 100 } else { 0 }),
            response_bytes: Some(10),
            ..Default::default()
        }
    }

    fn with_query(mut r: Record, q: &[(&str, &str)]) -> Record {
        r.query = q
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        r
    }

    #[test]
    fn retries_chain_until_success() {
        let mut slow = rec(1, 0, "GET", "GetObject", Some("k"), 503);
        slow.error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
        let mut slow2 = slow.clone();
        slow2.id = 2;
        slow2.time += Duration::milliseconds(200);
        let ok = rec(3, 700, "GET", "GetObject", Some("k"), 200);
        let other = rec(4, 100, "GET", "GetObject", Some("other"), 200);
        let eps = episodes(&[slow, slow2, ok, other]);
        assert_eq!(eps.len(), 2);
        let retry = eps.iter().find(|e| e.kind == Kind::Retry).unwrap();
        assert_eq!(retry.records, vec![1, 2, 3]);
        assert_eq!(retry.title, "GetObject k retried 2× after 503 SlowDown");
        assert_eq!(retry.outcome, Outcome::Ok);
        let obs = observations(&[], &eps);
        assert!(
            obs.iter()
                .any(|o| o.text == "2 requests repeated after a retryable failure")
        );
    }

    #[test]
    fn a_trials_fault_is_said_first_and_its_503s_are_not_the_stores() {
        let fault = |mut r: Record| {
            r.injected = Some("slowdown".into());
            r.response_headers
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
        assert_eq!(obs[0].phrases[0].key, "obs.injected", "{obs:#?}");
        let keys: Vec<&str> = obs.iter().map(|o| o.phrases[0].key).collect();
        assert!(
            !keys
                .iter()
                .any(|k| k.starts_with("obs.throttled") || *k == "obs.retried"),
            "the store asked for nothing: {keys:?}"
        );
        // a 503 of the store's own, in the same session, is still the store's
        let mut own = rec(90, 5000, "GET", "GetObject", Some("z"), 503);
        own.error_body = Some("<Error><Code>SlowDown</Code></Error>".into());
        rs.push(own);
        rs.push(rec(91, 5200, "GET", "GetObject", Some("z"), 200));
        let obs = observations(&rs, &episodes(&rs));
        assert_eq!(obs[0].phrases[0].key, "obs.injected");
        let throttled = obs
            .iter()
            .find(|o| o.phrases[0].key.starts_with("obs.throttled"))
            .expect("the store's own 503");
        assert_eq!(throttled.phrases[0].args["n"], "1");
        assert!(
            obs.iter()
                .any(|o| o.text == "1 request repeated after a retryable failure")
        );
    }

    #[test]
    fn an_upload_the_store_completed_is_complete_though_the_program_was_told_500() {
        let mut create = rec(1, 0, "POST", "CreateMultipartUpload", Some("big"), 200);
        create.query = vec![("uploads".into(), String::new())];
        let q = [("uploadId", "u1")];
        let part = with_query(
            rec(2, 10, "PUT", "UploadPart", Some("big"), 200),
            &[("uploadId", "u1"), ("partNumber", "1")],
        );
        let mut lost = with_query(
            rec(3, 20, "POST", "CompleteMultipartUpload", Some("big"), 500),
            &q,
        );
        lost.injected = Some("lost-answer".into());
        lost.response_headers
            .push(Header::new("x-sonde-store-status", "200"));
        lost.response_headers
            .push(Header::new("x-sonde-fault", "lost-answer"));
        let mut again = with_query(
            rec(4, 130, "POST", "CompleteMultipartUpload", Some("big"), 404),
            &q,
        );
        again.error_body = Some("<Error><Code>NoSuchUpload</Code></Error>".into());
        let abort = with_query(
            rec(5, 140, "DELETE", "AbortMultipartUpload", Some("big"), 404),
            &q,
        );
        let rs = [create, part, lost, again, abort];
        let eps = episodes(&rs);
        let upload = eps.iter().find(|e| e.kind == Kind::Multipart).unwrap();
        assert!(
            !upload.title.contains("aborted") && !upload.title.contains("not completed"),
            "{}",
            upload.title
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
            eps[0].title,
            "multipart upload b/big · 2 parts · 200 B (not completed)"
        );
        assert_eq!(eps[0].records, vec![1, 2, 3]);
        assert_eq!(
            eps[1].title,
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
        assert_eq!(eps[0].title, "commit: table metadata v1 · 3 files (300 B)");
        assert_eq!(eps[1].title, "1 file written, no commit followed (100 B)");
        assert!(
            observations(&[], &eps)
                .iter()
                .any(|o| o.text.contains("no commit after them"))
        );
    }

    #[test]
    fn a_parquet_file_is_no_table_until_the_session_touches_its_log() {
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
        assert!(eps.iter().all(|e| e.kind == Kind::Request), "{eps:#?}");

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
        won.request_headers = vec![Header::new("If-None-Match", "*")];
        let mut lost = rec(
            2,
            50,
            "PUT",
            "PutObject",
            Some("t/_delta_log/00000000000000000008.json"),
            412,
        );
        lost.request_headers = vec![Header::new("If-None-Match", "*")];
        lost.error_body = Some("<Error><Code>PreconditionFailed</Code></Error>".into());
        let mut refused = rec(3, 100, "DELETE", "DeleteObject", Some("x"), 403);
        refused.response_headers = vec![Header::new("x-sonde-proxy", "refused")];
        let records = [won, lost, refused];
        let eps = episodes(&records);
        assert_eq!(eps[0].title, "commit 7");
        assert_eq!(eps[0].notes, vec!["conditional create (If-None-Match: *)"]);
        assert_eq!(
            eps[1].outcome,
            Outcome::Failed("412 PreconditionFailed".into())
        );
        assert!(eps[1].notes.iter().any(|n| n.starts_with("lost a race")));
        assert_eq!(eps[2].outcome, Outcome::Refused);
        assert!(
            observations(&records, &eps)
                .iter()
                .any(|o| o.text.starts_with("1 conditional write lost a race"))
        );
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
        assert_eq!(eps[0].title, "listed b/logs/ in 2 pages");
        assert_eq!(eps[1].title, "ListObjectsV2 b/logs/");
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
}
