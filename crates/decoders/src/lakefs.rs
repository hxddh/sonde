//! lakeFS storage namespaces: `_lakefs/dummy`, the committed metadata as
//! Pebble SSTables under `_lakefs/<hash>` (metaranges list ranges; ranges
//! map paths to physical objects), and object data under `data/`.
//!
//! Commits, branches and tags live in lakeFS's key-value store (PostgreSQL,
//! DynamoDB, local), not in object storage, so the lens shows the trees
//! (one metarange per committed state) and what they map, and says so.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use chrono::{DateTime, Utc};

use crate::model::*;
use crate::ph;
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
                ph!("lakefs.notgraveler", key = o.key, why = why),
            )),
        }
    }
    // physical addresses referenced by any range
    let referenced: BTreeSet<String> = ranges
        .values()
        .flat_map(|(e, _, _)| e.iter().map(|x| x.address.clone()))
        .collect();
    let rel = |o: &ObjectMeta| o.key.strip_prefix(root).unwrap_or(&o.key).to_string();
    // an address may be absolute (`s3://bucket/root/data/x`): every tail of
    // it at a `/`, so each object is one lookup, not a pass over all addresses
    let tails: HashSet<&str> = referenced
        .iter()
        .flat_map(|a| {
            std::iter::once(a.as_str()).chain(a.match_indices('/').map(|(i, _)| &a[i + 1..]))
        })
        .collect();
    let unreferenced: Vec<&&ObjectMeta> = data
        .iter()
        .filter(|o| !tails.contains(rel(o).as_str()))
        .collect();
    let unreferenced_keys: HashSet<&str> = unreferenced.iter().map(|u| u.key.as_str()).collect();
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
            ph!("lakefs.range.missing", id = d),
        ));
    }
    if !unreferenced.is_empty() {
        l.issues.push(issue(
            Severity::Info,
            ph!(
                "lakefs.unreferenced",
                n = unreferenced.len(),
                size = human_bytes(unreferenced.iter().map(|o| o.size).sum())
            ),
        ));
    }
    l.issues.push(issue(Severity::Info, ph!("lakefs.kv")));
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
            short(id),
            time_str(*t),
            rs.len().to_string(),
            entries.len().to_string(),
            human_bytes(entries.iter().map(|e| e.size).sum()),
        ]);
        l.timeline.push(
            Event::new(
                *t,
                "commit",
                ph!("lakefs.tree", id = short(id)),
                ph!("lakefs.tree.detail", ranges = rs.len(), entries = entries.len()),
            )
            .at(format!("{root}_lakefs/{id}")),
        );
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
            label: format!("tree {}", short(id)),
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
            short(id),
            e.len().to_string(),
            e.first().map(|x| x.path.clone()).unwrap_or_default(),
            e.last().map(|x| x.path.clone()).unwrap_or_default(),
            human_bytes(*size),
        ]);
    }
    for o in &data {
        l.timeline.push(
            Event::new(
                o.last_modified,
                "object",
                raw(rel(o)),
                if unreferenced_keys.contains(o.key.as_str()) {
                    ph!("lakefs.unreferenced.detail")
                } else {
                    ph!("lakefs.committed.detail")
                },
            )
            .at(o.key.clone())
            .sized(o.size),
        );
    }
    l.tables = vec![mt, rt];
    l.timeline.sort_by_key(|e| e.time);
    l.say_headline(ph!(
        "lakefs.headline",
        trees = metaranges.len(),
        ranges = ranges.len(),
        data = data.len(),
        unreferenced = unreferenced.len()
    ));
    Ok(l)
}

/// The first 12 characters of an id (an object's name: any UTF-8).
fn short(id: &str) -> String {
    id.chars().take(12).collect()
}

#[cfg(test)]
mod tests {
    use super::short;

    #[test]
    fn a_name_with_multi_byte_characters_is_shortened_not_sliced() {
        // byte 12 falls inside the seventh `é`
        assert_eq!(short("ééééééé"), "ééééééé");
        assert_eq!(short("0123456789abcdef"), "0123456789ab");
        assert_eq!(
            short("日本語のテーブル名はとても長い"),
            "日本語のテーブル名はとて"
        );
    }
}
