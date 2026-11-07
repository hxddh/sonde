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
}

impl Episode {
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
fn build(kind: Kind, format: Option<Format>, title: String, mut rs: Vec<&Record>) -> Episode {
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
        title,
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
    let units = multipart(&sorted, &mut used, &mut out);
    retries(&sorted, &mut used, &mut out);

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
    let mut title = format!("{} {target}", r.operation);
    if let Some(p) = query(r, "prefix").filter(|p| !p.is_empty()) {
        title.push_str(p);
    }
    if matches!(r.method.as_str(), "PUT" | "POST") && r.request_bytes.unwrap_or(0) > 0 {
        title.push_str(&format!(" · {}", human(r.request_bytes.unwrap_or(0))));
    }
    let mut e = build(Kind::Request, None, title, vec![r]);
    conditional_note(&mut e, &[r]);
    e
}

fn conditional_note(e: &mut Episode, rs: &[&Record]) {
    if rs.iter().any(|r| header(r, "if-none-match") == Some("*")) {
        e.notes.push("conditional create (If-None-Match: *)".into());
    } else if rs
        .iter()
        .any(|r| is_write(r) && header(r, "if-match").is_some())
    {
        e.notes.push("conditional write (If-Match)".into());
    }
    if rs.iter().any(|r| r.status == Some(412)) {
        e.notes.push("lost a race: 412 Precondition Failed".into());
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
        format!("{n} lookups in {dir}, none found"),
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
        format!("listed {bucket}/{prefix} in {n} pages"),
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
            "CreateMultipartUpload" => groups.push((None, key, vec![i])),
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
                        let pending = groups
                            .iter()
                            .position(|(gid, k, _)| gid.is_none() && *k == key);
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
        let state = if completed {
            ""
        } else if aborted {
            " (aborted)"
        } else {
            " (not completed)"
        };
        let mut e = build(
            Kind::Multipart,
            None,
            format!(
                "multipart upload {key} · {parts} part{} · {}{state}",
                if parts == 1 { "" } else { "s" },
                human(bytes)
            ),
            rs,
        );
        if !completed && !aborted {
            e.notes
                .push("the upload was never completed or aborted: its parts are billed".into());
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

/// A request repeated after a retryable failure.
fn retries(sorted: &[&Record], used: &mut [bool], out: &mut Vec<Episode>) {
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
            let first = failure(rs[0]).unwrap_or_default();
            let n = rs.len() - 1;
            let target = rs[0].key.clone().unwrap_or_default();
            let mut e = build(
                Kind::Retry,
                None,
                format!("{} {target} retried {n}× after {first}", rs[0].operation),
                rs,
            );
            let wait = e.millis();
            e.notes
                .push(format!("{wait:.0} ms until it succeeded or gave up"));
            out.push(e);
        }
        i += 1;
    }
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
    let mut title = match (&part, deleted) {
        (_, true) => format!("deleted {}", file_name(commit.key.as_deref().unwrap_or(""))),
        (Part::SlateManifest(n), _) => format!("manifest {n} written"),
        (Part::SlateWal(n), _) if size == 0 => format!("WAL {n} written empty (fencing marker)"),
        (Part::SlateWal(n), _) => format!("WAL {n} flushed · {}", human(size)),
        (Part::SlateSst, _) => format!("SST flushed to L0 · {}", human(size)),
        (Part::SlateCompactions(n), _) => format!("compaction state {n} written"),
        (Part::IcebergMetadata(Some(v)), _) => format!("commit: table metadata v{v}"),
        (Part::IcebergMetadata(None), _) | (Part::IcebergVersionHint, _) => {
            "commit: table metadata".to_string()
        }
        (Part::DeltaCommit(n), _) => format!("commit {n}"),
        (Part::DeltaCheckpoint, _) => "checkpoint written".into(),
        (Part::DeltaLastCheckpoint, _) => "_last_checkpoint updated".into(),
        _ => format!(
            "{} {}",
            commit.operation,
            commit.key.as_deref().unwrap_or("")
        ),
    };
    // files written on the way to the commit (data files, manifests)
    let (files, bytes) = written(&before, commit_key);
    let listed = before
        .iter()
        .filter(|r| r.key.is_none())
        .filter_map(|r| listed_format(query(r, "prefix").unwrap_or("")).map(|(_, d)| d))
        .next();
    if !files.is_empty() {
        title.push_str(&format!(
            " · {} file{} ({})",
            files.len(),
            if files.len() == 1 { "" } else { "s" },
            human(bytes)
        ));
    }
    let mut e = build(Kind::Step, Some(f), title, rs.clone());
    conditional_note(&mut e, &[commit]);
    if let Some(d) = listed {
        e.notes.push(format!("after listing {d}/"));
    }
    let lookups = before
        .iter()
        .filter(|r| !is_write(r) && r.key.is_some())
        .count();
    if lookups > 0 {
        let misses = before.iter().filter(|r| is_miss(r)).count();
        let s = |n: usize| if n == 1 { "" } else { "s" };
        e.notes.push(match misses {
            0 => format!("after {lookups} read{}", s(lookups)),
            m if m == lookups => format!("after {lookups} lookup{} that found nothing", s(lookups)),
            m => format!("after {lookups} reads ({m} found nothing)"),
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
            format!(
                "{} file{} written, no commit followed ({})",
                writes.len(),
                if writes.len() == 1 { "" } else { "s" },
                human(bytes)
            ),
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
        format!("looked for the newest WAL: probed ids {lo}…{hi}")
    } else if let Some(r) = rs
        .iter()
        .rev()
        .find(|r| !r.is_error() && r.method == "GET" && r.key.is_some())
    {
        let what = match r.key.as_deref().and_then(part_of) {
            Some(Part::SlateManifest(m)) => format!("read manifest {m}"),
            Some(Part::SlateCompactions(c)) => format!("read compaction state {c}"),
            Some(Part::IcebergMetadata(Some(v))) => format!("read table metadata v{v}"),
            Some(Part::DeltaCommit(c)) => format!("read commit {c}"),
            _ => format!("read {}", file_name(r.key.as_deref().unwrap_or(""))),
        };
        if n > 1 {
            format!("{what} ({n} requests)")
        } else {
            what
        }
    } else if !lists.is_empty() && lists.len() == n {
        format!("listed {}/", lists[0])
    } else if misses == n {
        format!(
            "{n} lookup{} that found nothing",
            if n == 1 { "" } else { "s" }
        )
    } else {
        format!("{n} reads")
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
    /// The episodes it is about (indexes, ascending), for showing them.
    pub episodes: Vec<usize>,
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
        out.push(Observation {
            level: "info",
            text: format!("{with} {} writes: {note}", f.name()),
            episodes: of_episodes(&|e| e.format == Some(f) && e.notes.contains(&note)),
        });
    }
    let warn = |t: String, episodes: Vec<usize>| Observation {
        level: "warn",
        text: t,
        episodes,
    };
    let info = |t: String, episodes: Vec<usize>| Observation {
        level: "info",
        text: t,
        episodes,
    };
    let unfinished =
        of_episodes(&|e| e.kind == Kind::Multipart && e.title.ends_with("(not completed)"));
    if !unfinished.is_empty() {
        let n = unfinished.len();
        out.push(warn(
            format!(
                "{n} multipart upload{} never completed or aborted: the parts stay billed until a lifecycle rule removes them",
                if n == 1 { "" } else { "s" }
            ),
            unfinished,
        ));
    }
    let orphans = of_episodes(&|e| e.title.contains("no commit followed"));
    if !orphans.is_empty() {
        let n = orphans.len();
        out.push(warn(
            if n == 1 {
                "files were written with no commit after them: orphans unless a later commit adds them"
                    .to_string()
            } else {
                format!(
                    "{n} times files were written with no commit after them: orphans unless a later commit adds them"
                )
            },
            orphans,
        ));
    }
    let retries = of_episodes(&|e| e.kind == Kind::Retry);
    let retried: usize = retries.iter().map(|&i| episodes[i].requests() - 1).sum();
    if retried > 0 {
        out.push(warn(
            format!(
                "{retried} request{} repeated after a retryable failure",
                if retried == 1 { "" } else { "s" }
            ),
            retries,
        ));
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
        out.push(info(
            format!(
                "{} files under 5 MiB were uploaded as multipart uploads: 3 requests each where one PUT would do",
                small.len()
            ),
            of_records(&mut small.iter().flat_map(|u| u.3.iter().copied())),
        ));
    }
    let races: Vec<u64> = records
        .iter()
        .filter(|r| r.status == Some(412))
        .map(|r| r.id)
        .collect();
    if !races.is_empty() {
        out.push(info(
            format!(
                "{} conditional write{} lost a race (412): another writer got there first",
                races.len(),
                if races.len() == 1 { "" } else { "s" }
            ),
            of_records(&mut races.iter().copied()),
        ));
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
    let total = records.len().max(1);
    let mut hot: Vec<(String, Vec<u64>)> = reads
        .into_iter()
        .filter(|(_, ids)| ids.len() >= 10 && ids.len() * 10 >= total)
        .collect();
    hot.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    for (k, ids) in hot.into_iter().take(3) {
        out.push(info(
            format!(
                "{k} was read {} times ({}% of all requests)",
                ids.len(),
                ids.len() * 100 / total
            ),
            of_records(&mut ids.into_iter()),
        ));
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
    for (p, ids) in repeated.into_iter().take(3) {
        out.push(info(
            format!("{p} was listed {} times", ids.len()),
            of_records(&mut ids.into_iter()),
        ));
    }
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
