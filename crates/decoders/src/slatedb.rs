//! SlateDB: `<root>/manifest/<id:020>.manifest` (a big-endian u16 format
//! version, then a FlatBuffers `ManifestV1` or `ManifestV2`),
//! `<root>/wal/<id:020>.sst` and `<root>/compacted/<ULID>.sst`.
//!
//! Format versions are checked strictly: sonde decodes manifest versions
//! 1 and 2 (SlateDB 0.x up to 0.18) and reports any other version as
//! unknown without guessing.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::fb::{Buf, FbError, Table as FbTable};
use crate::model::*;
use crate::source::{ObjectMeta, Source, SourceError};

pub const KNOWN_MANIFEST_VERSIONS: &[u16] = &[1, 2];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: String,
    pub manifest_id: u64,
    pub expire_time_s: u32,
    pub create_time_s: u32,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortedRun {
    pub id: u32,
    pub ssts: Vec<String>,
}

/// One decoded manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u16,
    pub manifest_id: u64,
    pub initialized: bool,
    pub writer_epoch: u64,
    pub compactor_epoch: u64,
    pub replay_after_wal_id: u64,
    pub wal_id_last_seen: u64,
    /// SST file ULIDs in L0, newest first.
    pub l0: Vec<String>,
    pub sorted_runs: Vec<SortedRun>,
    pub checkpoints: Vec<Checkpoint>,
    pub last_l0_seq: u64,
    pub external_dbs: Vec<String>,
}

impl Manifest {
    /// Every compacted SST this manifest references.
    pub fn referenced(&self) -> BTreeSet<String> {
        self.l0
            .iter()
            .cloned()
            .chain(self.sorted_runs.iter().flat_map(|r| r.ssts.iter().cloned()))
            .collect()
    }
}

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A ULID as its 26-character text.
pub fn ulid_string(high: u64, low: u64) -> String {
    let v: u128 = ((high as u128) << 64) | low as u128;
    (0..26)
        .map(|i| CROCKFORD[((v >> (125 - 5 * i)) & 0x1f) as usize] as char)
        .collect()
}

/// The millisecond timestamp in a ULID's text.
pub fn ulid_time(s: &str) -> Option<DateTime<Utc>> {
    if s.len() != 26 {
        return None;
    }
    let mut v: u128 = 0;
    for c in s.bytes() {
        let d = CROCKFORD
            .iter()
            .position(|x| *x == c.to_ascii_uppercase())? as u128;
        v = (v << 5) | d;
    }
    DateTime::from_timestamp_millis((v >> 80) as i64)
}

fn ulid(t: Option<FbTable<'_>>) -> Result<String, FbError> {
    let t = t.ok_or_else(|| FbError("missing ULID".into()))?;
    Ok(ulid_string(t.u64(0, 0)?, t.u64(1, 0)?))
}

fn uuid(t: Option<FbTable<'_>>) -> Result<String, FbError> {
    let t = t.ok_or_else(|| FbError("missing UUID".into()))?;
    let v = ((t.u64(0, 0)? as u128) << 64) | t.u64(1, 0)? as u128;
    let h = format!("{v:032x}");
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("empty manifest")]
    Empty,
    #[error("unknown manifest format version {0} (sonde decodes versions 1 and 2); not decoded")]
    UnknownVersion(u16),
    #[error("{0}")]
    Corrupt(#[from] FbError),
}

/// Decodes one `.manifest` file.
pub fn decode_manifest(bytes: &[u8]) -> Result<Manifest, ManifestError> {
    if bytes.len() < 6 {
        return Err(ManifestError::Empty);
    }
    let version = u16::from_be_bytes([bytes[0], bytes[1]]);
    if !KNOWN_MANIFEST_VERSIONS.contains(&version) {
        return Err(ManifestError::UnknownVersion(version));
    }
    let root = Buf(&bytes[2..]).root()?;
    let checkpoints_at = if version == 1 { 11 } else { 12 };
    let checkpoints = root
        .tables(checkpoints_at)?
        .iter()
        .map(|c| {
            Ok(Checkpoint {
                id: uuid(c.table(0)?)?,
                manifest_id: c.u64(1, 0)?,
                expire_time_s: c.u32(2, 0)?,
                create_time_s: c.u32(3, 0)?,
                name: c.string(6)?,
            })
        })
        .collect::<Result<Vec<_>, FbError>>()?;
    let external_dbs = root
        .tables(1)?
        .iter()
        .map(|e| e.string(0).map(|s| s.unwrap_or_default()))
        .collect::<Result<Vec<_>, _>>()?;
    let (l0, sorted_runs, last_l0_seq) = if version == 1 {
        // l0: [CompactedSsTable] (id 8), compacted: [SortedRun] (id 9); CompactedSsTable.id = 0
        let l0 = root
            .tables(8)?
            .iter()
            .map(|t| ulid(t.table(0)?))
            .collect::<Result<Vec<_>, _>>()?;
        let runs = root
            .tables(9)?
            .iter()
            .map(|r| {
                Ok(SortedRun {
                    id: r.u32(0, 0)?,
                    ssts: r
                        .tables(1)?
                        .iter()
                        .map(|t| ulid(t.table(0)?))
                        .collect::<Result<Vec<_>, FbError>>()?,
                })
            })
            .collect::<Result<Vec<_>, FbError>>()?;
        (l0, runs, root.u64(12, 0)?)
    } else {
        // ssts (8) hold the files; l0 (9) and runs (10) hold views whose sst_id (1) names them
        let l0 = root
            .tables(9)?
            .iter()
            .map(|v| ulid(v.table(1)?))
            .collect::<Result<Vec<_>, _>>()?;
        let runs = root
            .tables(10)?
            .iter()
            .map(|r| {
                Ok(SortedRun {
                    id: r.u32(0, 0)?,
                    ssts: r
                        .tables(1)?
                        .iter()
                        .map(|v| ulid(v.table(1)?))
                        .collect::<Result<Vec<_>, FbError>>()?,
                })
            })
            .collect::<Result<Vec<_>, FbError>>()?;
        (l0, runs, root.u64(13, 0)?)
    };
    Ok(Manifest {
        version,
        manifest_id: root.u64(0, 0)?,
        initialized: root.bool(2)?,
        writer_epoch: root.u64(3, 0)?,
        compactor_epoch: root.u64(4, 0)?,
        replay_after_wal_id: root.u64(5, 0)?,
        wal_id_last_seen: root.u64(6, 0)?,
        l0,
        sorted_runs,
        checkpoints,
        last_l0_seq,
        external_dbs,
    })
}

fn numbered(key: &str, dir: &str, ext: &str) -> Option<u64> {
    let name = key.rsplit_once(&format!("{dir}/"))?.1;
    let id = name.strip_suffix(ext)?;
    (id.len() == 20 && id.bytes().all(|b| b.is_ascii_digit()))
        .then(|| id.parse().ok())
        .flatten()
}

/// Roots of SlateDB databases among `objects`.
pub fn detect(objects: &[ObjectMeta]) -> Vec<Detection> {
    let mut roots = BTreeSet::new();
    for o in objects {
        if numbered(&o.key, "manifest", ".manifest").is_some()
            && let Some((root, _)) = o.key.rsplit_once("manifest/")
        {
            roots.insert(root.to_string());
        }
    }
    roots
        .into_iter()
        .map(|root| Detection {
            format: Format::Slatedb,
            evidence: format!("{root}manifest/<id>.manifest"),
            root,
        })
        .collect()
}

/// At most this many manifests are decoded (the newest).
pub const MAX_MANIFESTS: usize = 500;

pub async fn analyze(src: &dyn Source, root: &str) -> Result<Layout, SourceError> {
    let objects = src.list(root).await?;
    let mut l = Layout::new(Format::Slatedb, root);
    l.objects = objects.len() as u64;
    l.bytes = objects.iter().map(|o| o.size).sum();
    let rel = |o: &ObjectMeta| o.key.strip_prefix(root).unwrap_or(&o.key).to_string();
    let mut manifests: Vec<(u64, &ObjectMeta)> = vec![];
    let mut wals: BTreeMap<u64, &ObjectMeta> = BTreeMap::new();
    let mut compacted: BTreeMap<String, &ObjectMeta> = BTreeMap::new();
    let mut other: Vec<&ObjectMeta> = vec![];
    let mut compactions = 0usize;
    for o in &objects {
        let r = rel(o);
        if let Some(id) =
            numbered(&r, "manifest", ".manifest").filter(|_| r.starts_with("manifest/"))
        {
            manifests.push((id, o));
        } else if let Some(id) = numbered(&r, "wal", ".sst").filter(|_| r.starts_with("wal/")) {
            wals.insert(id, o);
        } else if let Some(name) = r
            .strip_prefix("compacted/")
            .and_then(|n| n.strip_suffix(".sst"))
        {
            compacted.insert(name.to_string(), o);
        } else if numbered(&r, "compactions", ".compactions").is_some()
            && r.starts_with("compactions/")
        {
            compactions += 1;
        } else {
            other.push(o);
        }
    }
    manifests.sort_by_key(|m| m.0);
    let skipped = manifests.len().saturating_sub(MAX_MANIFESTS);
    let mut decoded: Vec<(u64, Option<DateTime<Utc>>, Manifest)> = vec![];
    let mut versions = BTreeSet::new();
    for (id, o) in manifests.iter().skip(skipped) {
        let bytes = src.get(&o.key, None).await?;
        match decode_manifest(&bytes) {
            Ok(m) => {
                versions.insert(m.version);
                decoded.push((*id, o.last_modified, m));
            }
            Err(e @ ManifestError::UnknownVersion(_)) => l
                .issues
                .push(issue(Severity::Error, format!("{}: {e}", o.key))),
            Err(e) => l.issues.push(issue(
                Severity::Error,
                format!("{}: cannot decode: {e}", o.key),
            )),
        }
    }
    if skipped > 0 {
        l.issues.push(issue(
            Severity::Info,
            format!("{skipped} older manifests not decoded (only the newest {MAX_MANIFESTS})"),
        ));
    }
    l.format_version = Some(
        versions
            .iter()
            .map(|v| format!("manifest v{v}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    let Some((latest_id, latest_time, latest)) = decoded.last().cloned() else {
        l.headline = if manifests.is_empty() {
            "no manifest found".into()
        } else {
            "no manifest could be decoded".into()
        };
        l.decoded = false;
        return Ok(l);
    };

    // --- epochs and fencing ---
    let mut prev: Option<&Manifest> = None;
    let mut fences = 0;
    for (id, t, m) in &decoded {
        l.timeline.push(Event {
            time: *t,
            kind: "manifest".into(),
            label: format!("manifest {id}"),
            detail: format!(
                "writer epoch {}, compactor epoch {}, L0 {}, runs {}, WAL replay after {}",
                m.writer_epoch,
                m.compactor_epoch,
                m.l0.len(),
                m.sorted_runs.len(),
                m.replay_after_wal_id
            ),
            key: Some(format!("{root}manifest/{id:020}.manifest")),
            bytes: None,
        });
        if let Some(p) = prev {
            if m.writer_epoch != p.writer_epoch {
                fences += 1;
                l.timeline.push(Event {
                    time: *t,
                    kind: "fence".into(),
                    label: format!("writer epoch {} → {}", p.writer_epoch, m.writer_epoch),
                    detail: "a new writer fenced the previous one".into(),
                    key: None,
                    bytes: None,
                });
            }
            if m.compactor_epoch != p.compactor_epoch {
                l.timeline.push(Event {
                    time: *t,
                    kind: "fence".into(),
                    label: format!(
                        "compactor epoch {} → {}",
                        p.compactor_epoch, m.compactor_epoch
                    ),
                    detail: "a new compactor took over".into(),
                    key: None,
                    bytes: None,
                });
            }
        }
        prev = Some(m);
    }

    // --- WAL ---
    for (id, o) in &wals {
        l.timeline.push(Event {
            time: o.last_modified,
            kind: "wal".into(),
            label: format!("WAL {id}"),
            detail: if *id <= latest.replay_after_wal_id {
                "flushed to L0 (GC-able)".into()
            } else {
                "not yet in L0".into()
            },
            key: Some(o.key.clone()),
            bytes: Some(o.size),
        });
    }
    let mut gaps = vec![];
    if let (Some(first), Some(last)) = (wals.keys().next(), wals.keys().last()) {
        let mut missing_run: Option<(u64, u64)> = None;
        for id in *first..=*last {
            if !wals.contains_key(&id) {
                missing_run = Some(match missing_run {
                    Some((s, _)) => (s, id),
                    None => (id, id),
                });
            } else if let Some(r) = missing_run.take() {
                gaps.push(r);
            }
        }
    }
    for (s, e) in &gaps {
        let msg = if s == e {
            format!("WAL {s} is missing")
        } else {
            format!("WAL {s}–{e} are missing")
        };
        let sev = if *e > latest.replay_after_wal_id {
            Severity::Error
        } else {
            Severity::Info
        };
        l.issues.push(issue(
            sev,
            format!(
                "{msg}{}",
                if sev == Severity::Error {
                    ": entries not yet in L0 are lost"
                } else {
                    " (already flushed; collected out of order)"
                }
            ),
        ));
        l.timeline.push(Event {
            time: None,
            kind: "gap".into(),
            label: msg,
            detail: String::new(),
            key: None,
            bytes: None,
        });
    }

    // --- references, GC lag, orphans ---
    let mut kept: BTreeSet<String> = latest.referenced();
    let checkpointed: BTreeSet<u64> = latest.checkpoints.iter().map(|c| c.manifest_id).collect();
    for (id, _, m) in &decoded {
        if checkpointed.contains(id) {
            kept.extend(m.referenced());
        }
    }
    let gc_wal: Vec<&&ObjectMeta> = wals
        .iter()
        .filter(|(id, _)| **id <= latest.replay_after_wal_id)
        .map(|(_, o)| o)
        .collect();
    let gc_wal_bytes: u64 = gc_wal.iter().map(|o| o.size).sum();
    let unreferenced: Vec<(&String, &&ObjectMeta)> = compacted
        .iter()
        .filter(|(n, _)| !kept.contains(*n))
        .collect();
    let missing: Vec<&String> = kept
        .iter()
        .filter(|n| !compacted.contains_key(*n))
        .collect();
    let now = Utc::now();
    let oldest_gc = gc_wal.iter().filter_map(|o| o.last_modified).min();
    for n in &missing {
        l.issues.push(issue(
            Severity::Error,
            format!("compacted/{n}.sst is referenced by the manifest but missing"),
        ));
    }
    if !unreferenced.is_empty() {
        l.issues.push(issue(Severity::Warning, format!("{} compacted SSTs ({}) are referenced by no manifest or checkpoint: orphans awaiting GC", unreferenced.len(), human_bytes(unreferenced.iter().map(|(_, o)| o.size).sum()))));
    }
    if let Some(t) = oldest_gc
        && (now - t).num_hours() >= 1
    {
        l.issues.push(issue(Severity::Warning, format!("{} WAL SSTs ({}) were flushed to L0 but not collected; the oldest is {} h old (GC lag)", gc_wal.len(), human_bytes(gc_wal_bytes), (now - t).num_hours())));
    }
    for o in &other {
        l.issues.push(issue(
            Severity::Info,
            format!("unexpected object {} ({})", o.key, human_bytes(o.size)),
        ));
    }
    let wal_tail = wals
        .keys()
        .filter(|id| **id > latest.replay_after_wal_id)
        .count();
    let old_manifests = decoded
        .iter()
        .filter(|(id, _, _)| *id != latest_id && !checkpointed.contains(id))
        .count();

    l.facts = vec![
        fact(
            "Latest manifest",
            format!(
                "{latest_id} (v{}, {})",
                latest.version,
                time_str(latest_time)
            ),
        ),
        fact("Writer epoch", latest.writer_epoch),
        fact("Compactor epoch", latest.compactor_epoch),
        fact("Fencing events", fences),
        fact(
            "WAL SSTs",
            format!(
                "{} ({}), ids {}–{}",
                wals.len(),
                human_bytes(wals.values().map(|o| o.size).sum()),
                wals.keys()
                    .next()
                    .map(|x| x.to_string())
                    .unwrap_or_default(),
                wals.keys()
                    .last()
                    .map(|x| x.to_string())
                    .unwrap_or_default()
            ),
        ),
        fact("WAL replay after", latest.replay_after_wal_id),
        fact("WAL not yet in L0", wal_tail),
        fact("WAL gaps", gaps.len()),
        fact("L0 SSTs", latest.l0.len()),
        fact("Sorted runs", latest.sorted_runs.len()),
        fact(
            "Compacted SSTs",
            format!(
                "{} ({})",
                compacted.len(),
                human_bytes(compacted.values().map(|o| o.size).sum())
            ),
        ),
        fact(
            "GC lag",
            format!(
                "{} WAL SSTs ({}) flushed but kept; {} orphan SSTs; {} superseded manifests",
                gc_wal.len(),
                human_bytes(gc_wal_bytes),
                unreferenced.len(),
                old_manifests
            ),
        ),
        fact("Checkpoints", latest.checkpoints.len()),
        fact("Compactor state files", compactions),
        fact("Last L0 sequence", latest.last_l0_seq),
    ];
    let mut ssts = Table::new(
        "Compacted SSTs",
        &["SST", "level", "size", "created", "state"],
    );
    let level_of = |n: &str| -> String {
        if latest.l0.iter().any(|x| x == n) {
            return "L0".into();
        }
        latest
            .sorted_runs
            .iter()
            .find(|r| r.ssts.iter().any(|x| x == n))
            .map(|r| format!("SR {}", r.id))
            .unwrap_or_else(|| "—".into())
    };
    for (n, o) in &compacted {
        let state = if latest.referenced().contains(n) {
            "live"
        } else if kept.contains(n) {
            "checkpoint"
        } else {
            "orphan"
        };
        ssts.row(vec![
            n.clone(),
            level_of(n),
            human_bytes(o.size),
            time_str(ulid_time(n)),
            state.into(),
        ]);
    }
    for n in &missing {
        ssts.row(vec![
            (*n).clone(),
            level_of(n),
            "—".into(),
            time_str(ulid_time(n)),
            "MISSING".into(),
        ]);
    }
    let mut wal_t = Table::new("WAL SSTs", &["id", "size", "written", "state"]);
    for (id, o) in &wals {
        wal_t.row(vec![
            id.to_string(),
            human_bytes(o.size),
            time_str(o.last_modified),
            if *id <= latest.replay_after_wal_id {
                "in L0 (GC-able)".into()
            } else {
                "pending".into()
            },
        ]);
    }
    let mut cp = Table::new(
        "Checkpoints",
        &["id", "manifest", "name", "created", "expires"],
    );
    for c in &latest.checkpoints {
        cp.row(vec![
            c.id.clone(),
            c.manifest_id.to_string(),
            c.name.clone().unwrap_or_default(),
            time_str(DateTime::from_timestamp(c.create_time_s as i64, 0)),
            if c.expire_time_s == 0 {
                "never".into()
            } else {
                time_str(DateTime::from_timestamp(c.expire_time_s as i64, 0))
            },
        ]);
    }
    l.tables = vec![ssts, wal_t, cp];

    // --- replay: one state per manifest ---
    for (id, t, m) in &decoded {
        let wal_present = wals
            .iter()
            .filter(|(_, o)| o.last_modified.zip(*t).is_none_or(|(w, mt)| w <= mt))
            .count();
        let mut lsm = Table::new("LSM", &["level", "SSTs"]);
        lsm.row(vec!["L0".into(), m.l0.join(", ")]);
        for r in &m.sorted_runs {
            lsm.row(vec![format!("SR {}", r.id), r.ssts.join(", ")]);
        }
        l.snapshots.push(Snapshot {
            time: *t,
            label: format!("manifest {id}"),
            facts: vec![
                fact("Writer epoch", m.writer_epoch),
                fact("Compactor epoch", m.compactor_epoch),
                fact("WAL replay after", m.replay_after_wal_id),
                fact("WAL last seen", m.wal_id_last_seen),
                fact("WAL objects written by then", wal_present),
                fact("L0 SSTs", m.l0.len()),
                fact("Sorted runs", m.sorted_runs.len()),
                fact("Last L0 sequence", m.last_l0_seq),
            ],
            tables: vec![lsm],
        });
    }
    l.timeline.sort_by_key(|e| e.time);
    l.headline = format!(
        "SlateDB: manifest {latest_id}, writer epoch {}, {} WAL SSTs ({} pending), {} L0 + {} sorted runs{}",
        latest.writer_epoch,
        wals.len(),
        wal_tail,
        latest.l0.len(),
        latest.sorted_runs.len(),
        if l.has_errors() {
            " — errors found"
        } else {
            ""
        }
    );
    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulids() {
        let s = ulid_string(0x0191_E5C2_1D93_C326, 0x1325_C5F0_EF97_E3B6);
        assert_eq!(s.len(), 26);
        let t = ulid_time(&s).unwrap();
        assert_eq!(t.timestamp_millis(), 0x0191_E5C2_1D93);
        assert_eq!(
            ulid_time("01J79C21YKR31J2BS1EFXJZ7MR")
                .unwrap()
                .format("%Y")
                .to_string(),
            "2024"
        );
    }

    #[test]
    fn version_is_strict() {
        assert_eq!(
            decode_manifest(&[0, 3, 0, 0, 0, 0, 0, 0]),
            Err(ManifestError::UnknownVersion(3))
        );
        assert_eq!(decode_manifest(&[0, 1]), Err(ManifestError::Empty));
        assert!(matches!(
            decode_manifest(&[0, 2, 255, 255, 255, 0, 0, 0]),
            Err(ManifestError::Corrupt(_))
        ));
    }

    #[test]
    fn numbering() {
        assert_eq!(
            numbered("db/wal/00000000000000000003.sst", "wal", ".sst"),
            Some(3)
        );
        assert_eq!(numbered("db/wal/3.sst", "wal", ".sst"), None);
    }
}
