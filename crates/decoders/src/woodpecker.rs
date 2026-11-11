//! Milvus Woodpecker logs on object storage:
//! `<root>/<logId>/<segmentId>/<blockId>.blk` (one object per block),
//! `m_<id>.blk` (compacted blocks), `footer.blk` (index records and the
//! footer of a sealed segment) and `write.lock`.
//!
//! Records are `[CRC32:4][type:1][length:4][payload]`, little-endian, with
//! the CRC (IEEE) over type, length and payload. Format versions 5 and 6
//! are decoded; any other is reported and not decoded.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::model::*;
use crate::ph;
use crate::source::{ObjectMeta, Source, SourceError};

pub const KNOWN_VERSIONS: &[u16] = &[5, 6];
const HEADER: u8 = 1;
const INDEX: u8 = 3;
const FOOTER: u8 = 4;
const BLOCK_HEADER: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub kind: u8,
    pub payload: Vec<u8>,
    pub crc_ok: bool,
}

/// Decodes the records in `b`, stopping at the first that does not fit.
pub fn records(b: &[u8]) -> Vec<Record> {
    let mut out = vec![];
    let mut p = 0;
    while p + 9 <= b.len() {
        let crc = u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
        let kind = b[p + 4];
        let len = u32::from_le_bytes(b[p + 5..p + 9].try_into().unwrap()) as usize;
        if !(1..=5).contains(&kind) || p + 9 + len > b.len() {
            break;
        }
        let crc_ok = crc32fast::hash(&b[p + 4..p + 9 + len]) == crc;
        out.push(Record {
            kind,
            payload: b[p + 9..p + 9 + len].to_vec(),
            crc_ok,
        });
        p += 9 + len;
    }
    out
}

fn le_u16(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
        .unwrap_or(0)
}
fn le_u32(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .unwrap_or(0)
}
fn le_i64(b: &[u8], at: usize) -> i64 {
    b.get(at..at + 8)
        .map(|s| i64::from_le_bytes(s.try_into().unwrap()))
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Footer {
    pub total_blocks: i32,
    pub total_records: u32,
    pub total_size: u64,
    pub version: u16,
    pub compacted: bool,
    pub lac: Option<i64>,
}

pub fn footer(payload: &[u8]) -> Footer {
    let version = le_u16(payload, 28);
    Footer {
        total_blocks: le_u32(payload, 0) as i32,
        total_records: le_u32(payload, 4),
        total_size: le_i64(payload, 8) as u64,
        version,
        compacted: le_u16(payload, 30) & 1 == 1,
        lac: (version >= 6)
            .then(|| le_i64(payload, 36))
            .filter(|x| *x >= 0),
    }
}

/// (block number, first entry, last entry)
pub fn block_header(payload: &[u8]) -> (i32, i64, i64) {
    (
        le_u32(payload, 0) as i32,
        le_i64(payload, 4),
        le_i64(payload, 12),
    )
}

/// (block number, first entry, last entry) of an index record.
pub fn index_record(payload: &[u8]) -> (i32, i64, i64) {
    (
        le_u32(payload, 0) as i32,
        le_i64(payload, 16),
        le_i64(payload, 24),
    )
}

fn parse_key(rel: &str) -> Option<(i64, i64, String)> {
    let mut it = rel.split('/');
    let log = it.next()?.parse().ok()?;
    let seg = it.next()?.parse().ok()?;
    let name = it.next()?.to_string();
    if it.next().is_some() {
        return None;
    }
    let ok = name == "footer.blk"
        || name == "write.lock"
        || name.strip_suffix(".blk").is_some_and(|n| {
            n.trim_start_matches("m_")
                .bytes()
                .all(|b| b.is_ascii_digit())
        });
    ok.then_some((log, seg, name))
}

pub fn detect(objects: &[ObjectMeta]) -> Vec<Detection> {
    let mut roots = std::collections::BTreeSet::new();
    for o in objects {
        let parts: Vec<&str> = o.key.rsplitn(4, '/').collect();
        if parts.len() == 4
            && parse_key(&format!("{}/{}/{}", parts[2], parts[1], parts[0])).is_some()
            && parts[0].ends_with(".blk")
        {
            roots.insert(format!("{}/", parts[3]));
        } else if parts.len() == 3
            && parse_key(&format!("{}/{}/{}", parts[2], parts[1], parts[0])).is_some()
            && parts[0].ends_with(".blk")
        {
            roots.insert(String::new());
        }
    }
    roots
        .into_iter()
        .map(|root| Detection {
            format: Format::Woodpecker,
            evidence: format!("{root}<log>/<segment>/<block>.blk"),
            root,
        })
        .collect()
}

#[derive(Debug, Default)]
struct Segment {
    blocks: BTreeMap<i64, ObjectMeta>,
    merged: BTreeMap<i64, ObjectMeta>,
    footer: Option<ObjectMeta>,
    lock: bool,
}

pub async fn analyze(src: &dyn Source, root: &str) -> Result<Layout, SourceError> {
    let objects = src.list(root).await?;
    let mut l = Layout::new(Format::Woodpecker, root);
    l.objects = objects.len() as u64;
    l.bytes = objects.iter().map(|o| o.size).sum();
    let mut segs: BTreeMap<(i64, i64), Segment> = BTreeMap::new();
    for o in &objects {
        let rel = o.key.strip_prefix(root).unwrap_or(&o.key);
        let Some((log, seg, name)) = parse_key(rel) else {
            l.issues
                .push(issue(Severity::Info, ph!("wp.unexpected", key = o.key)));
            continue;
        };
        let s = segs.entry((log, seg)).or_default();
        if name == "footer.blk" {
            s.footer = Some(o.clone());
        } else if name == "write.lock" {
            s.lock = true;
        } else if let Some(n) = name.strip_prefix("m_").and_then(|n| n.strip_suffix(".blk")) {
            s.merged.insert(n.parse().unwrap_or(0), o.clone());
        } else if let Some(n) = name.strip_suffix(".blk") {
            s.blocks.insert(n.parse().unwrap_or(0), o.clone());
        }
    }
    let mut table = Table::new(
        "Segments",
        &[
            "log",
            "segment",
            "state",
            "blocks",
            "entries",
            "LAC",
            "bytes",
            "first write",
            "last write",
        ],
    );
    let mut versions = std::collections::BTreeSet::new();
    let mut sealed_events: Vec<(Option<DateTime<Utc>>, crate::Phrase)> = vec![];
    let (mut sealed, mut active, mut entries_total) = (0, 0, 0i64);
    for ((log, seg), s) in &segs {
        let bytes: u64 = s
            .blocks
            .values()
            .chain(s.merged.values())
            .map(|o| o.size)
            .sum();
        let first = s
            .blocks
            .values()
            .chain(s.merged.values())
            .filter_map(|o| o.last_modified)
            .min();
        let last = s
            .blocks
            .values()
            .chain(s.merged.values())
            .chain(s.footer.iter())
            .filter_map(|o| o.last_modified)
            .max();
        let (state, nblocks, entries, lac);
        if let Some(f) = &s.footer {
            let b = src.get(&f.key, None).await?;
            let recs = records(&b);
            if recs.iter().any(|r| !r.crc_ok) {
                l.issues
                    .push(issue(Severity::Error, ph!("wp.crc", key = f.key)));
            }
            match recs.iter().rev().find(|r| r.kind == FOOTER) {
                Some(fr) => {
                    let ft = footer(&fr.payload);
                    versions.insert(ft.version);
                    if !KNOWN_VERSIONS.contains(&ft.version) {
                        l.issues.push(issue(
                            Severity::Error,
                            ph!("wp.footer.version", key = f.key, v = ft.version),
                        ));
                        state = ph!("wp.state.unknown");
                        nblocks = String::from("?");
                        entries = String::from("?");
                        lac = String::from("?");
                    } else {
                        let idx: Vec<(i32, i64, i64)> = recs
                            .iter()
                            .filter(|r| r.kind == INDEX)
                            .map(|r| index_record(&r.payload))
                            .collect();
                        let n_entries = idx.last().map(|x| x.2 + 1).unwrap_or(0)
                            - idx.first().map(|x| x.1).unwrap_or(0);
                        entries_total += n_entries.max(0);
                        if idx.len() as i32 != ft.total_blocks {
                            l.issues.push(issue(
                                Severity::Warning,
                                ph!(
                                    "wp.index.count",
                                    key = f.key,
                                    n = idx.len(),
                                    blocks = ft.total_blocks
                                ),
                            ));
                        }
                        let have = if ft.compacted {
                            s.merged.len()
                        } else {
                            s.blocks.len()
                        };
                        if (have as i32) < ft.total_blocks {
                            l.issues.push(issue(
                                Severity::Error,
                                ph!(
                                    "wp.blocks.missing",
                                    log = log,
                                    seg = seg,
                                    n = ft.total_blocks,
                                    have = have
                                ),
                            ));
                        }
                        state = if ft.compacted {
                            ph!("wp.state.compacted")
                        } else {
                            ph!("wp.state.sealed")
                        };
                        nblocks = ft.total_blocks.to_string();
                        entries = n_entries.to_string();
                        lac = ft.lac.map(|x| x.to_string()).unwrap_or_else(|| "—".into());
                    }
                    sealed += 1;
                    sealed_events.push((f.last_modified, ph!("wp.sealed", log = log, seg = seg)));
                }
                None => {
                    l.issues
                        .push(issue(Severity::Error, ph!("wp.footer.none", key = f.key)));
                    state = ph!("wp.state.corrupt");
                    nblocks = s.blocks.len().to_string();
                    entries = "?".into();
                    lac = "?".into();
                }
            }
        } else {
            // active: read each block's leading records
            active += 1;
            let mut first_e = None;
            let mut last_e = None;
            let ids: Vec<i64> = s.blocks.keys().copied().collect();
            if let (Some(a), Some(b)) = (ids.first(), ids.last())
                && (b - a + 1) as usize != ids.len()
            {
                l.issues.push(issue(
                    Severity::Error,
                    ph!("wp.blocks.gaps", log = log, seg = seg, from = a, to = b),
                ));
            }
            for (n, o) in &s.blocks {
                let head = src.get(&o.key, Some((0, 127))).await?;
                let recs = records(&head);
                if let Some(h) = recs.iter().find(|r| r.kind == HEADER) {
                    let v = le_u16(&h.payload, 0);
                    versions.insert(v);
                    if !KNOWN_VERSIONS.contains(&v) {
                        l.issues.push(issue(
                            Severity::Error,
                            ph!("wp.version", key = o.key, v = v),
                        ));
                    }
                }
                match recs.iter().find(|r| r.kind == BLOCK_HEADER) {
                    Some(bh) => {
                        let (bn, f, last) = block_header(&bh.payload);
                        if bn as i64 != *n {
                            l.issues.push(issue(
                                Severity::Warning,
                                ph!("wp.block.name", key = o.key, n = n, holds = bn),
                            ));
                        }
                        first_e = first_e.or(Some(f));
                        last_e = Some(last);
                    }
                    None => l.issues.push(issue(
                        Severity::Warning,
                        ph!("wp.block.noheader", key = o.key),
                    )),
                }
            }
            let n = match (first_e, last_e) {
                (Some(a), Some(b)) => b - a + 1,
                _ => 0,
            };
            entries_total += n;
            state = if s.lock {
                ph!("wp.state.locked")
            } else {
                ph!("wp.state.active")
            };
            nblocks = s.blocks.len().to_string();
            entries = n.to_string();
            lac = "—".into();
        }
        l.timeline.push(
            Event::new(
                first,
                "segment",
                ph!("wp.started", log = log, seg = seg),
                state.clone(),
            )
            .sized(bytes),
        );
        let state = crate::phrase::say(&state);
        table.row(vec![
            log.to_string(),
            seg.to_string(),
            state,
            nblocks,
            entries,
            lac,
            human_bytes(bytes),
            time_str(first),
            time_str(last),
        ]);
    }
    for (t, label) in &sealed_events {
        l.timeline
            .push(Event::new(*t, "seal", label.clone(), vec![]));
    }
    l.format_version = Some(
        versions
            .iter()
            .map(|v| format!("v{v}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    let logs: std::collections::BTreeSet<i64> = segs.keys().map(|k| k.0).collect();
    l.facts = vec![
        fact("Logs", logs.len()),
        fact("Segments", segs.len()),
        fact("Sealed", sealed),
        fact("Active", active),
        fact("Entries", entries_total),
    ];
    l.tables = vec![table];
    l.timeline.sort_by_key(|e| e.time);
    // replay: segments started / sealed by each event
    let times: Vec<Option<DateTime<Utc>>> = l.timeline.iter().map(|e| e.time).collect();
    for t in times {
        let started = l
            .timeline
            .iter()
            .filter(|e| e.kind == "segment" && e.time <= t)
            .count();
        let sealed_by = l
            .timeline
            .iter()
            .filter(|e| e.kind == "seal" && e.time <= t)
            .count();
        l.snapshots.push(Snapshot {
            time: t,
            label: time_str(t),
            facts: vec![
                fact("Segments started", started),
                fact("Sealed", sealed_by),
                fact("Open", started.saturating_sub(sealed_by)),
            ],
            tables: vec![],
        });
    }
    l.say_headline(ph!(
        "wp.headline",
        logs = logs.len(),
        segs = segs.len(),
        sealed = sealed,
        active = active,
        entries = entries_total
    ));
    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 9];
        b[4] = kind;
        b[5..9].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        b.extend_from_slice(payload);
        let crc = crc32fast::hash(&b[4..]);
        b[0..4].copy_from_slice(&crc.to_le_bytes());
        b
    }

    #[test]
    fn records_and_crc() {
        let mut b = rec(BLOCK_HEADER, &[0; 28]);
        b.extend(rec(2, b"hello"));
        let r = records(&b);
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|x| x.crc_ok));
        b[12] ^= 1;
        assert!(!records(&b)[0].crc_ok);
    }

    #[test]
    fn keys() {
        assert_eq!(parse_key("1/2/3.blk"), Some((1, 2, "3.blk".into())));
        assert_eq!(parse_key("1/2/m_0.blk"), Some((1, 2, "m_0.blk".into())));
        assert!(parse_key("1/x/3.blk").is_none());
    }
}
