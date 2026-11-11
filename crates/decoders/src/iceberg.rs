//! Apache Iceberg tables: `metadata/*.metadata.json`, their snapshots'
//! manifest lists and manifests (Avro), and the data files they reference.
//!
//! Decodes table format versions 1, 2 and 3; any other version is reported
//! as unknown and not decoded.

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

fn avro_records(bytes: &[u8]) -> Result<Vec<Avro>, String> {
    let reader = apache_avro::Reader::new(bytes).map_err(|e| e.to_string())?;
    reader.map(|r| r.map_err(|e| e.to_string())).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataFile {
    pub path: String,
    pub content: i64,
    pub format: String,
    pub records: i64,
    pub bytes: i64,
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
    let objects = src.list(root).await?;
    let mut l = Layout::new(Format::Iceberg, root);
    l.objects = objects.len() as u64;
    l.bytes = objects.iter().map(|o| o.size).sum();
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
                                files.push((
                                    status,
                                    DataFile {
                                        path: as_str(field(df, "file_path")).unwrap_or_default(),
                                        content: as_i64(field(df, "content")).unwrap_or(0),
                                        format: as_str(field(df, "file_format"))
                                            .unwrap_or_default(),
                                        records: as_i64(field(df, "record_count")).unwrap_or(0),
                                        bytes: as_i64(field(df, "file_size_in_bytes")).unwrap_or(0),
                                    },
                                ));
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

    // references, orphans, expiry
    let referenced: BTreeSet<String> = snaps
        .iter()
        .flat_map(|s| s.files.iter().map(|f| map.key(&f.path)))
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
        .filter(|o| !referenced.contains(&o.key))
        .collect();
    // a set: every referenced file against every object is quadratic
    let present: std::collections::HashSet<&str> = objects.iter().map(|o| o.key.as_str()).collect();
    let missing: Vec<&String> = referenced
        .iter()
        .filter(|k| !present.contains(k.as_str()))
        .collect();
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
    let live_bytes: i64 = cur
        .map(|s| s.files.iter().map(|f| f.bytes).sum())
        .unwrap_or(0);
    let live_records: i64 = cur
        .map(|s| {
            s.files
                .iter()
                .filter(|f| f.content == 0)
                .map(|f| f.records)
                .sum()
        })
        .unwrap_or(0);
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
        fact(
            "Live data files",
            cur.map(|s| s.files.iter().filter(|f| f.content == 0).count())
                .unwrap_or(0),
        ),
        fact(
            "Live delete files",
            cur.map(|s| s.files.iter().filter(|f| f.content != 0).count())
                .unwrap_or(0),
        ),
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
        st.row(vec![
            s.id.to_string(),
            s.parent.map(|p| p.to_string()).unwrap_or_default(),
            time_str(s.time),
            s.operation.clone(),
            s.manifests.to_string(),
            s.files.len().to_string(),
            human_bytes(s.files.iter().map(|f| f.bytes).sum::<i64>().max(0) as u64),
            state,
        ]);
        let mut e = Event::new(
            s.time,
            "snapshot",
            ph!("iceberg.snapshot", id = s.id, op = s.operation),
            ph!("iceberg.snapshot.detail", n = s.files.len()),
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
                ["data", "position deletes", "equality deletes"]
                    .get(f.content as usize)
                    .unwrap_or(&"?")
                    .to_string(),
                f.format.clone(),
                f.records.to_string(),
                human_bytes(f.bytes.max(0) as u64),
            ]);
        }
        l.snapshots.push(Snapshot {
            time: s.time,
            label: format!("snapshot {} ({})", s.id, s.operation),
            facts: vec![
                fact("Snapshot", s.id),
                fact("Operation", &s.operation),
                fact("Data files", s.files.len()),
                fact(
                    "Records",
                    s.files
                        .iter()
                        .filter(|f| f.content == 0)
                        .map(|f| f.records)
                        .sum::<i64>(),
                ),
                fact(
                    "Bytes",
                    human_bytes(s.files.iter().map(|f| f.bytes).sum::<i64>().max(0) as u64),
                ),
            ],
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
            files = cur.map(|s| s.files.len()).unwrap_or(0),
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
