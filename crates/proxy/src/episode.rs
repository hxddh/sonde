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
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::SlateDB => "SlateDB",
            Format::Iceberg => "Iceberg",
            Format::Delta => "Delta",
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
        )
    }
}

fn digits(s: &str, n: usize) -> Option<u64> {
    (s.len() == n && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// The format part a key names, from its last two path segments.
pub fn part_of(key: &str) -> Option<Part> {
    let mut segs = key.rsplit('/');
    let name = segs.next()?;
    let dir = segs.next().unwrap_or("");
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
        _ => return None,
    };
    Some((f, dir))
}

/// The format of a request: from its key, or from a listing's prefix.
fn format_of(r: &Record) -> Option<Format> {
    match r.key.as_deref().and_then(part_of) {
        Some(p) => Some(p.format()),
        None if r.operation.starts_with("ListObjects") => {
            listed_format(query(r, "prefix").unwrap_or("")).map(|(f, _)| f)
        }
        None => None,
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
    r.time + Duration::microseconds((r.latency_ms * 1000.0) as i64)
}

fn refused(r: &Record) -> bool {
    r.response_headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("x-sonde-proxy"))
}

fn s3_code(r: &Record) -> Option<String> {
    let b = r.error_body.as_deref()?;
    let start = b.find("<Code>")? + 6;
    Some(b[start..start + b[start..].find("</Code>")?].to_string())
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

fn query<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.query
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn human(b: u64) -> String {
    const U: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
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
        not_found: rs
            .iter()
            .filter(|r| r.status == Some(404) && (r.method == "GET" || r.method == "HEAD"))
            .count(),
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
    let mut units = multipart(&sorted, &mut used, &mut out);
    // a format's write retried is still that write: its attempts go with it to the step
    units.extend(retries(&sorted, &mut used, &mut out));

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
    for unit in items {
        // a unit's last record stands for it: the Complete of an upload
        let r = *unit.last().expect("units are not empty");
        // idle buffers flush before anything newer is placed
        flush_idle(&mut buffers, r.time, &mut out);
        if !probe.is_empty() && (r.time - probe.last().unwrap().time).num_milliseconds() > BURST {
            out.extend(flush_probe(std::mem::take(&mut probe)));
        }
        if let Some(f) = format_of(r) {
            let part = r.key.as_deref().and_then(part_of);
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
        out.push(single(r));
    }
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
    used: &mut [bool],
    out: &mut Vec<Episode>,
) -> Vec<Vec<&'a Record>> {
    let mut units = Vec::new();
    use std::collections::HashMap;
    // upload id → record indexes; creates wait for the first upload id seen on their key
    let mut groups: Vec<(Option<String>, String, Vec<usize>)> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    for (i, r) in sorted.iter().enumerate() {
        let key = format!(
            "{}/{}",
            r.bucket.as_deref().unwrap_or(""),
            r.key.as_deref().unwrap_or("")
        );
        match r.operation.as_str() {
            // a Create that failed and was tried again is part of the upload that followed
            "CreateMultipartUpload" => {
                let retried = groups.iter().rposition(|(gid, k, idx)| {
                    gid.is_none() && *k == key && idx.iter().all(|&j| sorted[j].is_error())
                });
                match retried {
                    Some(g) => groups[g].2.push(i),
                    _ => groups.push((None, key, vec![i])),
                }
            }
            "UploadPart"
            | "UploadPartCopy"
            | "CompleteMultipartUpload"
            | "AbortMultipartUpload"
            | "ListParts" => {
                let Some(id) = query(r, "uploadId").map(str::to_string) else {
                    continue;
                };
                let g = match by_id.get(&id) {
                    Some(&g) => g,
                    None => {
                        // the upload a successful Create began
                        let pending = groups.iter().position(|(gid, k, idx)| {
                            gid.is_none() && *k == key && idx.iter().any(|&j| !sorted[j].is_error())
                        });
                        let g = match pending {
                            Some(g) => g,
                            None => {
                                groups.push((None, key.clone(), Vec::new()));
                                groups.len() - 1
                            }
                        };
                        groups[g].0 = Some(id.clone());
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
        let completed = rs
            .iter()
            .any(|r| r.operation == "CompleteMultipartUpload" && !r.is_error());
        let aborted = rs.iter().any(|r| r.operation == "AbortMultipartUpload");
        let formatted = rs[0].key.as_deref().and_then(part_of).is_some();
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

fn retryable(r: &Record) -> bool {
    r.transport_error.is_some()
        || matches!(r.status, Some(500 | 502 | 503 | 504 | 429))
        || s3_code(r).is_some_and(|c| c == "SlowDown" || c == "RequestTimeout")
}

/// A request repeated after a retryable failure. A retried write of a
/// format's file (a commit, a manifest) is returned as a unit, so the step it
/// belongs to keeps it, attempts and all.
fn retries<'a>(
    sorted: &[&'a Record],
    used: &mut [bool],
    out: &mut Vec<Episode>,
) -> Vec<Vec<&'a Record>> {
    let mut units = Vec::new();
    let same = |a: &Record, b: &Record| {
        a.method == b.method && a.bucket == b.bucket && a.key == b.key && a.query == b.query
    };
    let mut i = 0;
    while i < sorted.len() {
        if used[i] || !retryable(sorted[i]) {
            i += 1;
            continue;
        }
        let mut chain = vec![i];
        let mut j = i + 1;
        while j < sorted.len() {
            let last = sorted[*chain.last().unwrap()];
            if (sorted[j].time - last.time).num_seconds() > 60 {
                break;
            }
            if !used[j] && same(sorted[j], last) {
                chain.push(j);
                if !retryable(sorted[j]) {
                    break;
                }
            }
            j += 1;
        }
        if chain.len() > 1 {
            for &c in &chain {
                used[c] = true;
            }
            let rs: Vec<&Record> = chain.iter().map(|&c| sorted[c]).collect();
            let last = *rs.last().expect("a chain has records");
            if format_of(last).is_some() && is_write(last) && !retryable(last) {
                units.push(rs);
                i += 1;
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
        i += 1;
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
    if !files.is_empty() {
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
    let title = if wal_probe.len() >= 3 {
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
}

impl Observation {
    /// Weighs a finding: the requests it bears on and those acting on it would
    /// save (out of `total`).
    fn weigh(mut self, impact: usize, saves: usize, total: usize) -> Self {
        self.impact = impact;
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
    for f in [Format::SlateDB, Format::Iceberg, Format::Delta] {
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

/// What stands out across a session's requests and episodes.
pub fn observations(records: &[Record], episodes: &[Episode]) -> Vec<Observation> {
    use std::collections::{BTreeSet, HashMap};
    // which episode each record belongs to
    let owner: HashMap<u64, usize> = episodes
        .iter()
        .enumerate()
        .flat_map(|(i, e)| e.records.iter().map(move |r| (*r, i)))
        .collect();
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
    };
    let info = |t: Phrase, episodes: Vec<usize>| Observation {
        level: "info",
        text: t.en(),
        phrases: vec![t],
        episodes,
        impact: 0,
        saving: None,
        saving_text: None,
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
    let retries = of_episodes(&|e| e.kind == Kind::Retry);
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
    let small: Vec<&(usize, u64, bool, Vec<u64>)> = uploads
        .values()
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
            .weigh(impact, impact - small.len(), total),
        );
    }
    let races: Vec<u64> = records
        .iter()
        .filter(|r| r.status == Some(412))
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
    // the same key read again and again
    let mut reads: HashMap<String, Vec<u64>> = HashMap::new();
    for r in records
        .iter()
        .filter(|r| r.method == "GET" || r.method == "HEAD")
    {
        if let (Some(b), Some(k)) = (&r.bucket, &r.key) {
            reads.entry(format!("{b}/{k}")).or_default().push(r.id);
        }
    }
    let mut hot: Vec<(String, Vec<u64>)> = reads
        .into_iter()
        .filter(|(_, ids)| ids.len() >= 10 && ids.len() * 10 >= total)
        .collect();
    hot.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    for (k, ids) in hot.into_iter().take(3) {
        let n = ids.len();
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
            .weigh(n, n - 1, total),
        );
    }
    // the same prefix listed again and again
    let mut lists: HashMap<String, Vec<u64>> = HashMap::new();
    for r in records.iter().filter(|r| {
        r.operation.starts_with("ListObjects") && query(r, "continuation-token").is_none()
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
        out.push(
            info(
                p(
                    "obs.listed",
                    &[("prefix", prefix_name), ("n", n.to_string())],
                ),
                of_records(&mut ids.into_iter()),
            )
            .weigh(n, n - 1, total),
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
            .weigh(pairs.len(), checked, total),
        );
    }
    // throttled: and how many requests were in flight when it happened
    let throttled: Vec<&Record> = records
        .iter()
        .filter(|r| matches!(r.status, Some(503) | Some(429)))
        .collect();
    if !throttled.is_empty() {
        let n = throttled.len();
        let mut starts: Vec<i64> = records.iter().map(|r| r.time.timestamp_micros()).collect();
        let mut ends: Vec<i64> = records
            .iter()
            .map(|r| r.time.timestamp_micros() + (r.latency_ms * 1000.0) as i64)
            .collect();
        starts.sort_unstable();
        ends.sort_unstable();
        // started by then and not yet answered (itself included)
        let in_flight =
            |t: i64| starts.partition_point(|s| *s <= t) - ends.partition_point(|e| *e <= t);
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
    out.sort_by(|a, b| {
        (b.level == "warn")
            .cmp(&(a.level == "warn"))
            .then(b.impact.cmp(&a.impact))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqlog::Header;

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
}
