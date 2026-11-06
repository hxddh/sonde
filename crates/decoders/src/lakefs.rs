//! lakeFS storage namespaces: `_lakefs/dummy`, the committed metadata as
//! Pebble SSTables under `_lakefs/<hash>` (metaranges list ranges; ranges
//! map paths to physical objects), and object data under `data/`.
//!
//! Commits, branches and tags live in lakeFS's key-value store (PostgreSQL,
//! DynamoDB, local), not in object storage, so the lens shows the trees
//! (one metarange per committed state) and what they map, and says so.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::model::*;
use crate::source::{ObjectMeta, Source, SourceError};
use crate::sstable::{self, pb_bytes, pb_int, protobuf, svarint};

pub fn detect(objects: &[ObjectMeta]) -> Vec<Detection> {
    objects
        .iter()
        .filter_map(|o| {
            o.key.strip_suffix("_lakefs/dummy").map(|root| Detection {
                format: Format::Lakefs,
                root: root.to_string(),
                evidence: format!("{root}_lakefs/dummy"),
            })
        })
        .collect()
}

/// A graveler value: identity, then data, each a zigzag length and bytes.
fn graveler_value(v: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut p = 0;
    let il = svarint(v, &mut p).ok()? as usize;
    let identity = v.get(p..p + il)?.to_vec();
    p += il;
    let dl = svarint(v, &mut p).ok()? as usize;
    let data = v.get(p..p + dl)?.to_vec();
    Some((identity, data))
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntryInfo {
    pub path: String,
    pub address: String,
    pub size: u64,
    pub etag: String,
    pub modified: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sst {
    /// Range ids with their key bounds and entry counts.
    Metarange(Vec<(String, String, String, u64)>),
    Range(Vec<EntryInfo>),
    Unknown(String),
}

/// Classifies and decodes one `_lakefs/<hash>` table.
pub fn decode(bytes: &[u8]) -> Sst {
    let entries = match sstable::read_all(bytes) {
        Ok(e) => e,
        Err(e) => return Sst::Unknown(e.to_string()),
    };
    let mut ranges = vec![];
    let mut objects = vec![];
    for (k, v) in &entries {
        let Some((identity, data)) = graveler_value(v) else {
            continue;
        };
        let Ok(fields) = protobuf(&data) else {
            continue;
        };
        let ident = String::from_utf8_lossy(&identity).to_string();
        // RangeData {1 min_key, 2 max_key, 3 estimated_size, 4 count}
        if ident.len() == 64
            && ident.bytes().all(|b| b.is_ascii_hexdigit())
            && pb_int(&fields, 4).is_some()
            && pb_bytes(&fields, 2).is_some()
        {
            ranges.push((
                ident,
                String::from_utf8_lossy(pb_bytes(&fields, 1).unwrap_or_default()).to_string(),
                String::from_utf8_lossy(pb_bytes(&fields, 2).unwrap_or_default()).to_string(),
                pb_int(&fields, 4).unwrap_or(0),
            ));
        } else if let Some(addr) = pb_bytes(&fields, 1) {
            // Entry {1 address, 2 last_modified, 3 size, 4 e_tag, 7 content_type}
            let modified = pb_bytes(&fields, 2)
                .and_then(|ts| protobuf(ts).ok())
                .and_then(|t| pb_int(&t, 1))
                .and_then(|s| DateTime::from_timestamp(s as i64, 0));
            objects.push(EntryInfo {
                path: String::from_utf8_lossy(k).to_string(),
                address: String::from_utf8_lossy(addr).to_string(),
                size: pb_int(&fields, 3).unwrap_or(0),
                etag: String::from_utf8_lossy(pb_bytes(&fields, 4).unwrap_or_default()).to_string(),
                modified,
            });
        }
    }
    if !ranges.is_empty() && objects.is_empty() {
        Sst::Metarange(ranges)
    } else if !objects.is_empty() {
        Sst::Range(objects)
    } else {
        Sst::Unknown(format!("{} entries, none recognized", entries.len()))
    }
}

pub async fn analyze(src: &dyn Source, root: &str) -> Result<Layout, SourceError> {
    let objects = src.list(root).await?;
    let mut l = Layout::new(Format::Lakefs, root);
    l.format_version = Some("graveler (Pebble SSTables)".into());
    l.objects = objects.len() as u64;
    l.bytes = objects.iter().map(|o| o.size).sum();
    let meta: Vec<&ObjectMeta> = objects
        .iter()
        .filter(|o| {
            o.key
                .strip_prefix(root)
                .is_some_and(|r| r.starts_with("_lakefs/") && !r.ends_with("dummy"))
        })
        .collect();
    let data: Vec<&ObjectMeta> = objects
        .iter()
        .filter(|o| {
            o.key
                .strip_prefix(root)
                .is_some_and(|r| r.starts_with("data/"))
        })
        .collect();
    let mut metaranges: BTreeMap<
        String,
        (Vec<(String, String, String, u64)>, Option<DateTime<Utc>>),
    > = BTreeMap::new();
    let mut ranges: BTreeMap<String, (Vec<EntryInfo>, Option<DateTime<Utc>>, u64)> =
        BTreeMap::new();
    for o in &meta {
        let name = o.key.rsplit('/').next().unwrap_or_default().to_string();
        let bytes = src.get(&o.key, None).await?;
        match decode(&bytes) {
            Sst::Metarange(r) => {
                metaranges.insert(name, (r, o.last_modified));
            }
            Sst::Range(e) => {
                ranges.insert(name, (e, o.last_modified, o.size));
            }
            Sst::Unknown(why) => l.issues.push(issue(
                Severity::Warning,
                format!("{}: not a graveler table ({why})", o.key),
            )),
        }
    }
    // physical addresses referenced by any range
    let referenced: BTreeSet<String> = ranges
        .values()
        .flat_map(|(e, _, _)| e.iter().map(|x| x.address.clone()))
        .collect();
    let rel = |o: &ObjectMeta| o.key.strip_prefix(root).unwrap_or(&o.key).to_string();
    let unreferenced: Vec<&&ObjectMeta> = data
        .iter()
        .filter(|o| {
            !referenced.contains(&rel(o)) && !referenced.iter().any(|a| a.ends_with(&rel(o)))
        })
        .collect();
    let used_ranges: BTreeSet<String> = metaranges
        .values()
        .flat_map(|(r, _)| r.iter().map(|x| x.0.clone()))
        .collect();
    let dangling: Vec<&String> = used_ranges
        .iter()
        .filter(|r| !ranges.contains_key(*r))
        .collect();
    for d in &dangling {
        l.issues.push(issue(
            Severity::Error,
            format!("range {d} is listed by a metarange but missing"),
        ));
    }
    if !unreferenced.is_empty() {
        l.issues.push(issue(Severity::Info, format!("{} data objects ({}) are referenced by no committed range: uncommitted (staged) writes, or garbage for lakeFS's GC", unreferenced.len(), human_bytes(unreferenced.iter().map(|o| o.size).sum()))));
    }
    l.issues.push(issue(Severity::Info, "Commit history, branches and tags are kept in lakeFS's KV store, not in object storage: each metarange below is one committed tree".to_string()));
    l.facts = vec![
        fact("Metaranges (committed trees)", metaranges.len()),
        fact("Ranges", ranges.len()),
        fact(
            "Distinct paths",
            ranges
                .values()
                .flat_map(|(e, _, _)| e.iter().map(|x| x.path.clone()))
                .collect::<BTreeSet<_>>()
                .len(),
        ),
        fact(
            "Data objects",
            format!(
                "{} ({})",
                data.len(),
                human_bytes(data.iter().map(|o| o.size).sum())
            ),
        ),
        fact(
            "Unreferenced data",
            format!(
                "{} ({})",
                unreferenced.len(),
                human_bytes(unreferenced.iter().map(|o| o.size).sum())
            ),
        ),
    ];
    let mut mt = Table::new(
        "Metaranges",
        &["metarange", "written", "ranges", "entries", "bytes"],
    );
    let mut mr: Vec<(
        &String,
        &(Vec<(String, String, String, u64)>, Option<DateTime<Utc>>),
    )> = metaranges.iter().collect();
    mr.sort_by_key(|(_, (_, t))| *t);
    for (id, (rs, t)) in &mr {
        let entries: Vec<&EntryInfo> = rs
            .iter()
            .filter_map(|r| ranges.get(&r.0))
            .flat_map(|(e, _, _)| e.iter())
            .collect();
        mt.row(vec![
            id[..12.min(id.len())].to_string(),
            time_str(*t),
            rs.len().to_string(),
            entries.len().to_string(),
            human_bytes(entries.iter().map(|e| e.size).sum()),
        ]);
        l.timeline.push(Event {
            time: *t,
            kind: "commit".into(),
            label: format!("tree {}", &id[..12.min(id.len())]),
            detail: format!("{} ranges, {} entries", rs.len(), entries.len()),
            key: Some(format!("{root}_lakefs/{id}")),
            bytes: None,
        });
        let mut files = Table::new(
            "Paths → objects",
            &["path", "physical address", "size", "modified"],
        );
        for e in entries.iter().take(500) {
            files.row(vec![
                e.path.clone(),
                e.address.clone(),
                human_bytes(e.size),
                time_str(e.modified),
            ]);
        }
        l.snapshots.push(Snapshot {
            time: *t,
            label: format!("tree {}", &id[..12.min(id.len())]),
            facts: vec![fact("Ranges", rs.len()), fact("Entries", entries.len())],
            tables: vec![files],
        });
    }
    let mut rt = Table::new(
        "Ranges",
        &["range", "entries", "first path", "last path", "size"],
    );
    for (id, (e, _, size)) in &ranges {
        rt.row(vec![
            id[..12.min(id.len())].to_string(),
            e.len().to_string(),
            e.first().map(|x| x.path.clone()).unwrap_or_default(),
            e.last().map(|x| x.path.clone()).unwrap_or_default(),
            human_bytes(*size),
        ]);
    }
    for o in &data {
        l.timeline.push(Event {
            time: o.last_modified,
            kind: "object".into(),
            label: rel(o),
            detail: if unreferenced.iter().any(|u| u.key == o.key) {
                "unreferenced".into()
            } else {
                "committed".into()
            },
            key: Some(o.key.clone()),
            bytes: Some(o.size),
        });
    }
    l.tables = vec![mt, rt];
    l.timeline.sort_by_key(|e| e.time);
    l.headline = format!(
        "lakeFS: {} committed trees, {} ranges, {} data objects ({} unreferenced)",
        metaranges.len(),
        ranges.len(),
        data.len(),
        unreferenced.len()
    );
    Ok(l)
}
