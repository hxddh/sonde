//! Apache Iceberg tables: `metadata/*.metadata.json`, their snapshots'
//! manifest lists and manifests (Avro), and the data files they reference.
//!
//! Decodes table format versions 1, 2 and 3; any other version is reported
//! as unknown and not decoded. Of version 3: deletion vectors (a manifest
//! entry naming a blob of a Puffin file by `content_offset` and
//! `content_size_in_bytes`, its `record_count` the rows it deletes of one
//! data file) and the `statistics` and `partition-statistics` files the
//! table metadata lists. Row lineage, variants and encryption keys are not
//! read: none changes what is in the store.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use apache_avro::types::Value as Avro;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::model::*;
use crate::ph;
use crate::source::{ObjectMeta, Source, SourceError};

pub const KNOWN_FORMAT_VERSIONS: &[u64] = &[1, 2, 3];

/// Roots of tables: prefixes holding `metadata/<...>.metadata.json`.
pub fn detect(objects: &[ObjectMeta]) -> Vec<Detection> {
    let mut roots = BTreeSet::new();
    for o in objects {
        if let Some((root, name)) = o.key.rsplit_once("metadata/")
            && name.ends_with(".metadata.json")
            && !name.contains('/')
        {
            roots.insert(root.to_string());
        }
    }
    roots
        .into_iter()
        .map(|root| Detection {
            format: Format::Iceberg,
            evidence: format!("{root}metadata/*.metadata.json"),
            root,
        })
        .collect()
}

fn field<'a>(v: &'a Avro, name: &str) -> Option<&'a Avro> {
    match v {
        Avro::Record(fields) => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
        Avro::Union(_, inner) => field(inner, name),
        _ => None,
    }
}

fn unwrap(v: &Avro) -> &Avro {
    match v {
        Avro::Union(_, inner) => unwrap(inner),
        v => v,
    }
}

fn as_i64(v: Option<&Avro>) -> Option<i64> {
    match v.map(unwrap) {
        Some(Avro::Long(x)) => Some(*x),
        Some(Avro::Int(x)) => Some(*x as i64),
        _ => None,
    }
}

fn as_str(v: Option<&Avro>) -> Option<String> {
    match v.map(unwrap) {
        Some(Avro::String(s)) => Some(s.clone()),
        Some(Avro::Bytes(b)) => Some(String::from_utf8_lossy(b).to_string()),
        Some(Avro::Enum(_, s)) => Some(s.clone()),
        _ => None,
    }
}

/// The most an Avro length may say, in items or bytes: a manifest's
/// strings, maps and blocks are far smaller. apache-avro reserves what a
/// length says before it reads the items (512 MiB by default), so a file of
/// a few bytes could ask for gigabytes (found by `fuzz/`, `iceberg_avro`).
const AVRO_MAX_LEN: usize = 4 << 20;

/// The deepest an Avro value may nest: a manifest's are a few records deep.
/// A recursive schema lets each byte of a hostile file go one level deeper,
/// which would overflow the stack (and abort sonde, past any `catch_unwind`).
const AVRO_MAX_DEPTH: usize = 32;

fn avro_records(bytes: &[u8]) -> Result<Vec<Avro>, String> {
    // (the first call in the process sets them; every call sets the same)
    apache_avro::util::max_allocation_bytes(AVRO_MAX_LEN);
    apache_avro::util::max_decode_recursion_depth(AVRO_MAX_DEPTH);
    // apache-avro panics on some malformed schemas (a name it cannot parse:
    // `fuzz/`, `iceberg_avro`): an unreadable file, not the end of sonde
    std::panic::catch_unwind(|| {
        let reader = apache_avro::Reader::new(bytes).map_err(|e| e.to_string())?;
        reader.map(|r| r.map_err(|e| e.to_string())).collect()
    })
    .unwrap_or_else(|_| Err("not Avro sonde can read (a malformed schema)".into()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataFile {
    pub path: String,
    pub content: i64,
    pub format: String,
    pub records: i64,
    pub bytes: i64,
    /// A deletion vector (v3): where its blob is in the Puffin file at
    /// `path`, which other vectors share; `records` is the rows it deletes
    /// and `bytes` the whole file's size.
    pub vector: Option<Vector>,
}

/// A deletion vector's place in its Puffin file, and the data file it is of.
#[derive(Debug, Clone, PartialEq)]
pub struct Vector {
    pub offset: i64,
    pub size: i64,
    pub of: Option<String>,
}

impl DataFile {
    /// What the file holds, as the files' table says it.
    fn kind(&self) -> &'static str {
        match (self.content, &self.vector) {
            (_, Some(_)) => "deletion vector",
            (0, _) => "data",
            (1, _) => "position deletes",
            (2, _) => "equality deletes",
            _ => "?",
        }
    }
}

/// What a snapshot's files come to. A Puffin file of several deletion
/// vectors is one file, its bytes counted once; the rows its vectors delete
/// are not live, nor those position delete files name. Equality deletes
/// name values, not rows: what they delete is not known from the files.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Totals {
    /// Files: each path once.
    files: usize,
    data_files: usize,
    delete_files: usize,
    bytes: i64,
    /// Rows of the data files, less those deletion vectors and position
    /// delete files delete.
    records: i64,
    vectors: usize,
    /// Rows deletion vectors delete.
    deleted: i64,
}

impl Totals {
    fn of(files: &[DataFile]) -> Totals {
        let mut t = Totals::default();
        let mut shared: BTreeSet<&str> = BTreeSet::new();
        // rows position delete files (v2) delete
        let mut positions = 0;
        for f in files {
            if f.vector.is_some() {
                t.vectors += 1;
                t.deleted += f.records.max(0);
                // the Puffin file itself, once
                if !shared.insert(&f.path) {
                    continue;
                }
            }
            t.files += 1;
            t.bytes += f.bytes;
            if f.content == 0 {
                t.data_files += 1;
                t.records += f.records;
            } else {
                t.delete_files += 1;
                if f.content == 1 && f.vector.is_none() {
                    positions += f.records.max(0);
                }
            }
        }
        t.records = (t.records - t.deleted - positions).max(0);
        t
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotInfo {
    pub id: i64,
    pub parent: Option<i64>,
    pub time: Option<DateTime<Utc>>,
    pub operation: String,
    pub manifest_list: Option<String>,
    pub files: Vec<DataFile>,
    pub manifests: usize,
}

/// A manifest entry's `data_file`. In a v3 manifest a deletion vector is
/// position deletes (`content` 1) kept as a blob of a Puffin file: it
/// alone has a `content_offset`.
fn data_file(df: &Avro) -> DataFile {
    let content = as_i64(field(df, "content")).unwrap_or(0);
    let format = as_str(field(df, "file_format")).unwrap_or_default();
    let offset = as_i64(field(df, "content_offset"));
    let vector = (content == 1 && (offset.is_some() || format.eq_ignore_ascii_case("puffin")))
        .then(|| Vector {
            offset: offset.unwrap_or(0),
            size: as_i64(field(df, "content_size_in_bytes")).unwrap_or(0),
            of: as_str(field(df, "referenced_data_file")),
        });
    DataFile {
        path: as_str(field(df, "file_path")).unwrap_or_default(),
        content,
        format,
        records: as_i64(field(df, "record_count")).unwrap_or(0),
        bytes: as_i64(field(df, "file_size_in_bytes")).unwrap_or(0),
        vector,
    }
}

/// Maps a path in the metadata to a key of the source.
pub struct PathMap {
    pub location: String,
    pub root: String,
}

impl PathMap {
    pub fn key(&self, path: &str) -> String {
        let loc = self.location.trim_end_matches('/');
        if let Some(rest) = path.strip_prefix(loc) {
            return format!("{}{}", self.root, rest.trim_start_matches('/'));
        }
        // s3://bucket/key → key
        if let Some(rest) = path.split_once("://").map(|x| x.1) {
            return rest
                .split_once('/')
                .map(|x| x.1.to_string())
                .unwrap_or_default();
        }
        path.to_string()
    }
}

fn ms(v: &Value) -> Option<DateTime<Utc>> {
    v.as_i64().and_then(DateTime::from_timestamp_millis)
}

/// The metadata file to read: `version-hint.text`, else the highest numbered one.
fn current_metadata(objects: &[&ObjectMeta], hint: Option<String>) -> Option<String> {
    let jsons: Vec<&&ObjectMeta> = objects
        .iter()
        .filter(|o| o.key.ends_with(".metadata.json"))
        .collect();
    if let Some(h) = hint {
        let h = h.trim();
        if let Some(o) = jsons.iter().find(|o| {
            o.key.ends_with(&format!("v{h}.metadata.json"))
                || o.key.ends_with(&format!("/{h}.metadata.json"))
        }) {
            return Some(o.key.clone());
        }
    }
    let num = |k: &str| -> i64 {
        let name = k.rsplit('/').next().unwrap_or(k);
        let digits: String = name
            .trim_start_matches('v')
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        digits.parse().unwrap_or(-1)
    };
    jsons
        .iter()
        .max_by_key(|o| (num(&o.key), o.last_modified))
        .map(|o| o.key.clone())
}

pub async fn analyze(src: &dyn Source, root: &str) -> Result<Layout, SourceError> {
    let listing = src.list(root).await?;
    let mut l = Layout::new(Format::Iceberg, root);
    l.count(&listing);
    let (objects, cut) = (listing.objects, listing.cut);
    let meta_objs: Vec<&ObjectMeta> = objects
        .iter()
        .filter(|o| {
            o.key
                .strip_prefix(root)
                .is_some_and(|r| r.starts_with("metadata/"))
        })
        .collect();
    let hint = match meta_objs
        .iter()
        .find(|o| o.key.ends_with("version-hint.text"))
    {
        Some(o) => src
            .get(&o.key, None)
            .await
            .ok()
            .map(|b| String::from_utf8_lossy(&b).to_string()),
        None => None,
    };
    let Some(meta_key) = current_metadata(&meta_objs, hint) else {
        l.say_headline(ph!("iceberg.headline.none"));
        l.decoded = false;
        return Ok(l);
    };
    let meta: Value = match serde_json::from_slice(&src.get(&meta_key, None).await?) {
        Ok(v) => v,
        Err(e) => {
            l.issues.push(issue(
                Severity::Error,
                ph!("iceberg.notjson", key = meta_key, error = e),
            ));
            l.decoded = false;
            l.say_headline(ph!("iceberg.headline.unreadable"));
            return Ok(l);
        }
    };
    let fv = meta["format-version"].as_u64().unwrap_or(0);
    l.format_version = Some(format!("format-version {fv}"));
    if !KNOWN_FORMAT_VERSIONS.contains(&fv) {
        l.issues.push(issue(
            Severity::Error,
            ph!("iceberg.version.unknown", v = fv),
        ));
        l.decoded = false;
        l.say_headline(ph!("iceberg.headline.unknown", v = fv));
        return Ok(l);
    }
    let location = meta["location"].as_str().unwrap_or_default().to_string();
    let map = PathMap {
        location: location.clone(),
        root: root.to_string(),
    };
    let current = meta["current-snapshot-id"].as_i64().filter(|id| *id >= 0);
    let props = &meta["properties"];
    let prop_i64 = |k: &str, d: i64| props[k].as_str().and_then(|s| s.parse().ok()).unwrap_or(d);
    let max_age_ms = prop_i64("history.expire.max-snapshot-age-ms", 432_000_000);
    let min_keep = prop_i64("history.expire.min-snapshots-to-keep", 1) as usize;
    let refs: BTreeMap<String, i64> = meta["refs"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v["snapshot-id"].as_i64().map(|id| (k.clone(), id)))
                .collect()
        })
        .unwrap_or_default();

    // snapshots and their files
    let mut manifest_cache: HashMap<String, Vec<(i64, DataFile)>> = HashMap::new();
    let mut snaps: Vec<SnapshotInfo> = vec![];
    for s in meta["snapshots"].as_array().cloned().unwrap_or_default() {
        let mut info = SnapshotInfo {
            id: s["snapshot-id"].as_i64().unwrap_or_default(),
            parent: s["parent-snapshot-id"].as_i64(),
            time: ms(&s["timestamp-ms"]),
            operation: s["summary"]["operation"]
                .as_str()
                .unwrap_or("?")
                .to_string(),
            manifest_list: s["manifest-list"].as_str().map(String::from),
            files: vec![],
            manifests: 0,
        };
        let Some(list) = &info.manifest_list else {
            l.issues
                .push(issue(Severity::Info, ph!("iceberg.inline", id = info.id)));
            snaps.push(info);
            continue;
        };
        let list_key = map.key(list);
        let list_bytes = match src.get(&list_key, None).await {
            Ok(b) => b,
            Err(e) => {
                l.issues.push(issue(
                    Severity::Error,
                    ph!(
                        "iceberg.list.unreadable",
                        id = info.id,
                        key = list_key,
                        error = e
                    ),
                ));
                snaps.push(info);
                continue;
            }
        };
        let entries = match avro_records(&list_bytes) {
            Ok(e) => e,
            Err(e) => {
                l.issues
                    .push(issue(Severity::Error, raw(format!("{list_key}: {e}"))));
                snaps.push(info);
                continue;
            }
        };
        info.manifests = entries.len();
        for m in &entries {
            let Some(path) = as_str(field(m, "manifest_path")) else {
                continue;
            };
            if !manifest_cache.contains_key(&path) {
                let key = map.key(&path);
                let mut files = vec![];
                match src.get(&key, None).await {
                    Ok(b) => match avro_records(&b) {
                        Ok(recs) => {
                            for r in recs {
                                let status = as_i64(field(&r, "status")).unwrap_or(1);
                                let Some(df) = field(&r, "data_file") else {
                                    continue;
                                };
                                files.push((status, data_file(df)));
                            }
                        }
                        Err(e) => l.issues.push(issue(
                            Severity::Error,
                            ph!("iceberg.manifest.bad", key = key, error = e),
                        )),
                    },
                    Err(e) => l.issues.push(issue(
                        Severity::Error,
                        ph!("iceberg.manifest.missing", key = key, error = e),
                    )),
                }
                manifest_cache.insert(path.clone(), files);
            }
            info.files.extend(
                manifest_cache[&path]
                    .iter()
                    .filter(|(st, _)| *st != 2)
                    .map(|(_, f)| f.clone()),
            );
        }
        snaps.push(info);
    }
    snaps.sort_by_key(|s| s.time);

    // the statistics files the metadata lists (Puffin; a partition's, any
    // format): the table's own, wherever its writer put them
    let statistics: BTreeSet<String> = ["statistics", "partition-statistics"]
        .iter()
        .flat_map(|k| meta[*k].as_array().into_iter().flatten())
        .filter_map(|s| s["statistics-path"].as_str())
        .map(|p| map.key(p))
        .collect();

    // references, orphans, expiry
    let referenced: BTreeSet<String> = snaps
        .iter()
        .flat_map(|s| s.files.iter().map(|f| map.key(&f.path)))
        .chain(statistics.iter().cloned())
        .collect();
    let data_objs: Vec<&ObjectMeta> = objects
        .iter()
        .filter(|o| {
            o.key
                .strip_prefix(root)
                .is_some_and(|r| r.starts_with("data/"))
        })
        .collect();
    let orphans: Vec<&&ObjectMeta> = data_objs
        .iter()
        .filter(|o| !cut && !referenced.contains(&o.key))
        .collect();
    // a set: every referenced file against every object is quadratic
    let present: std::collections::HashSet<&str> = objects.iter().map(|o| o.key.as_str()).collect();
    let missing: Vec<&String> = referenced
        .iter()
        .filter(|k| !cut && !present.contains(k.as_str()))
        .collect();
    // a deletion vector's blob lies within its Puffin file
    let size_of: HashMap<&str, u64> = objects.iter().map(|o| (o.key.as_str(), o.size)).collect();
    for f in snaps
        .iter()
        .filter(|s| Some(s.id) == current)
        .flat_map(|s| &s.files)
    {
        let Some(v) = &f.vector else { continue };
        let key = map.key(&f.path);
        let within = |size: u64| {
            v.offset >= 0 && v.size >= 0 && (v.offset as u64).saturating_add(v.size as u64) <= size
        };
        if size_of.get(key.as_str()).is_some_and(|size| !within(*size)) {
            l.issues.push(issue(
                Severity::Error,
                ph!(
                    "iceberg.vector.past",
                    key = key,
                    offset = v.offset,
                    size = v.size
                ),
            ));
        }
    }
    let now = Utc::now();
    let protected: BTreeSet<i64> = refs.values().copied().chain(current).collect();
    let mut by_age: Vec<&SnapshotInfo> = snaps.iter().collect();
    by_age.sort_by_key(|s| std::cmp::Reverse(s.time));
    let expirable: Vec<&SnapshotInfo> = by_age
        .iter()
        .enumerate()
        .filter(|(i, s)| {
            *i >= min_keep
                && !protected.contains(&s.id)
                && s.time
                    .is_some_and(|t| (now - t).num_milliseconds() > max_age_ms)
        })
        .map(|(_, s)| *s)
        .collect();
    let kept_files: BTreeSet<String> = snaps
        .iter()
        .filter(|s| !expirable.iter().any(|e| e.id == s.id))
        .flat_map(|s| s.files.iter().map(|f| f.path.clone()))
        .collect();
    let reclaimable: BTreeMap<String, i64> = expirable
        .iter()
        .flat_map(|s| s.files.iter())
        .filter(|f| !kept_files.contains(&f.path))
        .map(|f| (f.path.clone(), f.bytes))
        .collect();

    if !orphans.is_empty() {
        l.issues.push(issue(
            Severity::Warning,
            ph!(
                "iceberg.orphans",
                n = orphans.len(),
                size = human_bytes(orphans.iter().map(|o| o.size).sum())
            ),
        ));
    }
    for m in &missing {
        l.issues
            .push(issue(Severity::Error, ph!("referenced.missing", key = m)));
    }
    if !expirable.is_empty() {
        l.issues.push(issue(
            Severity::Warning,
            ph!(
                "iceberg.expirable",
                n = expirable.len(),
                days = max_age_ms / 86_400_000,
                size = human_bytes(reclaimable.values().sum::<i64>().max(0) as u64),
                files = reclaimable.len()
            ),
        ));
    }
    let cur = snaps.iter().find(|s| Some(s.id) == current);
    let live = cur.map(|s| Totals::of(&s.files)).unwrap_or_default();
    let (live_bytes, live_records) = (live.bytes, live.records);
    l.facts = vec![
        fact("Metadata", meta_key.strip_prefix(root).unwrap_or(&meta_key)),
        fact("Table UUID", meta["table-uuid"].as_str().unwrap_or("—")),
        fact("Location", &location),
        fact("Format version", fv),
        fact("Last updated", time_str(ms(&meta["last-updated-ms"]))),
        match current {
            Some(c) => fact("Current snapshot", c),
            None => fact_said("Current snapshot", ph!("none")),
        },
        fact("Snapshots", snaps.len()),
        fact(
            "Refs",
            refs.iter()
                .map(|(k, v)| format!("{k} → {v}"))
                .collect::<Vec<_>>()
                .join(", "),
        ),
        fact("Live data files", live.data_files),
        fact("Live delete files", live.delete_files),
        fact("Live records", live_records),
        fact("Live bytes", human_bytes(live_bytes.max(0) as u64)),
        fact(
            "Data objects",
            format!(
                "{} ({})",
                data_objs.len(),
                human_bytes(data_objs.iter().map(|o| o.size).sum())
            ),
        ),
        fact(
            "Orphan data files",
            format!(
                "{} ({})",
                orphans.len(),
                human_bytes(orphans.iter().map(|o| o.size).sum())
            ),
        ),
        fact_said(
            "Expirable snapshots",
            ph!(
                "iceberg.expirable.fact",
                n = expirable.len(),
                days = max_age_ms / 86_400_000,
                keep = min_keep
            ),
        ),
        fact(
            "Metadata files",
            meta_objs
                .iter()
                .filter(|o| o.key.ends_with(".metadata.json"))
                .count(),
        ),
    ];
    // what only a v3 table has, said when it has it
    if live.vectors > 0 {
        let at = l.facts.iter().position(|f| f.label == "Live records");
        l.facts.splice(
            at.unwrap_or(0)..at.unwrap_or(0),
            [
                fact("Deletion vectors", live.vectors),
                fact("Deleted rows", live.deleted),
            ],
        );
    }
    if !statistics.is_empty() {
        l.facts.push(fact("Statistics files", statistics.len()));
    }
    let mut st = Table::new(
        "Snapshots",
        &[
            "snapshot",
            "parent",
            "time",
            "operation",
            "manifests",
            "data files",
            "bytes",
            "state",
        ],
    );
    for s in &snaps {
        let state = if Some(s.id) == current {
            "current".to_string()
        } else if expirable.iter().any(|e| e.id == s.id) {
            "expirable".into()
        } else if let Some((r, _)) = refs.iter().find(|(_, id)| **id == s.id) {
            format!("ref {r}")
        } else {
            "retained".into()
        };
        let totals = Totals::of(&s.files);
        st.row(vec![
            s.id.to_string(),
            s.parent.map(|p| p.to_string()).unwrap_or_default(),
            time_str(s.time),
            s.operation.clone(),
            s.manifests.to_string(),
            totals.files.to_string(),
            human_bytes(totals.bytes.max(0) as u64),
            state,
        ]);
        let mut e = Event::new(
            s.time,
            "snapshot",
            ph!("iceberg.snapshot", id = s.id, op = s.operation),
            ph!("iceberg.snapshot.detail", n = totals.files),
        );
        e.key = s.manifest_list.as_ref().map(|m| map.key(m));
        l.timeline.push(e);
    }
    let mut ot = Table::new("Orphan data files", &["key", "size", "written"]);
    for o in &orphans {
        ot.row(vec![
            o.key.clone(),
            human_bytes(o.size),
            time_str(o.last_modified),
        ]);
    }
    l.tables = vec![st, ot];
    for s in &snaps {
        let mut files = Table::new(
            "Data files",
            &["file", "content", "format", "records", "size"],
        );
        for f in s.files.iter().take(500) {
            files.row(vec![
                map.key(&f.path)
                    .strip_prefix(root)
                    .unwrap_or(&f.path)
                    .to_string(),
                f.kind().to_string(),
                f.format.clone(),
                f.records.to_string(),
                // a deletion vector's own bytes, not its file's
                human_bytes(f.vector.as_ref().map_or(f.bytes, |v| v.size).max(0) as u64),
            ]);
        }
        let totals = Totals::of(&s.files);
        let mut facts = vec![
            fact("Snapshot", s.id),
            fact("Operation", &s.operation),
            fact("Data files", totals.data_files),
            fact("Records", totals.records),
            fact("Bytes", human_bytes(totals.bytes.max(0) as u64)),
        ];
        if totals.vectors > 0 {
            facts.push(fact("Deleted rows", totals.deleted));
        }
        l.snapshots.push(Snapshot {
            time: s.time,
            label: format!("snapshot {} ({})", s.id, s.operation),
            facts,
            tables: vec![files],
        });
    }
    l.timeline.sort_by_key(|e| e.time);
    let said_current = match current {
        Some(c) => ph!("iceberg.headline.current", id = c),
        None => ph!("iceberg.headline.nocurrent"),
    };
    l.say_headline(vec![
        ph!("iceberg.headline", v = fv, n = snaps.len()),
        said_current,
        ph!(
            "iceberg.headline.rest",
            files = live.files,
            size = human_bytes(live_bytes.max(0) as u64),
            orphans = orphans.len(),
            expirable = expirable.len()
        ),
    ]);
    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, content: i64, records: i64, vector: bool) -> DataFile {
        DataFile {
            path: path.into(),
            content,
            format: if vector { "puffin" } else { "parquet" }.into(),
            records,
            bytes: 100,
            vector: vector.then_some(Vector {
                offset: 4,
                size: 40,
                of: None,
            }),
        }
    }

    /// v2 position deletes delete rows as v3 deletion vectors do; equality
    /// deletes name values, and are no count of rows.
    #[test]
    fn position_deletes_are_not_live_rows_and_delete_files_are_no_data_files() {
        let t = Totals::of(&[
            file("d1.parquet", 0, 100, false),
            file("d2.parquet", 0, 50, false),
            file("p1.parquet", 1, 30, false),
            file("e1.parquet", 2, 7, false),
            file("v.puffin", 1, 5, true),
            file("v.puffin", 1, 5, true),
        ]);
        assert_eq!((t.files, t.data_files, t.delete_files), (5, 2, 3));
        assert_eq!(t.records, 150 - 30 - 10);
        assert_eq!((t.vectors, t.deleted), (2, 10));
        // never fewer than none
        let t = Totals::of(&[file("d", 0, 10, false), file("p", 1, 30, false)]);
        assert_eq!(t.records, 0);
    }

    /// 78 bytes whose header's map says it holds some hundred million
    /// entries (`fuzz/`, `iceberg_avro`): refused, not reserved.
    #[test]
    fn an_avro_length_past_any_manifests_is_refused() {
        let file: &[u8] = &[
            0x4f, 0x62, 0x6a, 0x01, 0xff, 0xff, 0xff, 0x64, 0x22, 0x3a, 0x31, 0x2c, 0x22, 0x6e,
            0x61, 0x6d, 0x65, 0x22, 0x3a, 0x22, 0x69, 0x64, 0x22, 0x2c, 0x22, 0x74, 0x79, 0x70,
            0x65, 0x22, 0x3a, 0x22, 0x3a, 0x31, 0x2c, 0x22, 0x6e, 0x61, 0x6d, 0x65, 0x22, 0x3a,
            0x22, 0x69, 0x64, 0x22, 0x2c, 0x22, 0x74, 0x79, 0x70, 0x65, 0x22, 0x3a, 0x22, 0x6c,
            0x6f, 0x6e, 0x4f, 0x62, 0x6a, 0x01, 0x0e, 0x0c, 0x73, 0x63, 0x6d, 0x61, 0xe6, 0x68,
            0x65, 0x7b, 0xd2, 0x22, 0x74, 0x67, 0x22, 0x79,
        ];
        assert!(avro_records(file).is_err());
    }

    /// An Avro file of one value of a recursive schema, `depth` deep, one
    /// byte a level.
    fn nested(depth: usize) -> Vec<u8> {
        fn long(out: &mut Vec<u8>, n: i64) {
            let mut z = ((n << 1) ^ (n >> 63)) as u64;
            while z >= 0x80 {
                out.push((z as u8) | 0x80);
                z >>= 7;
            }
            out.push(z as u8);
        }
        fn bytes(out: &mut Vec<u8>, b: &[u8]) {
            long(out, b.len() as i64);
            out.extend_from_slice(b);
        }
        let schema =
            r#"{"type":"record","name":"Node","fields":[{"name":"next","type":["null","Node"]}]}"#;
        let sync = [7u8; 16];
        let mut file = b"Obj\x01".to_vec();
        long(&mut file, 2);
        bytes(&mut file, b"avro.schema");
        bytes(&mut file, schema.as_bytes());
        bytes(&mut file, b"avro.codec");
        bytes(&mut file, b"null");
        long(&mut file, 0);
        file.extend_from_slice(&sync);
        // "Node" chosen at each level, then null
        let mut data = vec![0x02u8; depth];
        data.push(0x00);
        long(&mut file, 1);
        bytes(&mut file, &data);
        file.extend_from_slice(&sync);
        file
    }

    /// Nested half a million deep: an error, where decoding it whole
    /// overflows the stack and aborts.
    #[test]
    fn an_avro_value_nested_past_any_manifests_is_refused() {
        let err = avro_records(&nested(500_000)).unwrap_err();
        assert!(err.contains("ecursion"), "{err}");
        // as deep as a manifest nests, it reads
        assert_eq!(avro_records(&nested(4)).map(|r| r.len()), Ok(1));
    }

    /// A schema whose name apache-avro cannot parse, where it panics.
    #[test]
    fn an_avro_schema_that_panics_its_reader_is_an_error() {
        let file = include_bytes!("../fuzzed/avro-bad-schema-name.avro");
        assert!(avro_records(file).is_err());
    }

    #[test]
    fn path_mapping() {
        let m = PathMap {
            location: "s3://wh/db/t".into(),
            root: "warehouse/db/t/".into(),
        };
        assert_eq!(
            m.key("s3://wh/db/t/data/a.parquet"),
            "warehouse/db/t/data/a.parquet"
        );
        assert_eq!(m.key("s3://other/x/y"), "x/y");
    }

    #[tokio::test]
    async fn unknown_version() {
        let mut s = crate::source::MemSource::default();
        s.put(
            "t/metadata/v1.metadata.json",
            r#"{"format-version": 9, "location": "s3://b/t"}"#,
            None,
        );
        let l = analyze(&s, "t/").await.unwrap();
        assert!(!l.decoded);
        assert!(
            l.issues[0]
                .message
                .contains("unknown Iceberg format-version 9")
        );
    }
}
