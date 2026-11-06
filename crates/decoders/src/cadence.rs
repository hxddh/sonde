//! Formats sonde does not decode — turbopuffer and WarpStream keep their
//! formats unpublished — and any other prefix: object counts by
//! sub-prefix, sizes, and the write cadence (objects per interval, gaps,
//! bursts). Nothing is read but the listing.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, DurationRound, Utc};

use crate::model::*;
use crate::source::{ObjectMeta, Source, SourceError};

pub fn detect(objects: &[ObjectMeta]) -> Vec<Detection> {
    let mut out = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for o in objects {
        let k = &o.key;
        let found = if let Some(i) = k.find("warpstream/") {
            Some((Format::Warpstream, k[..i + "warpstream/".len()].to_string()))
        } else if let Some(i) = k.find("_segments/") {
            k[..i]
                .contains("warpstream")
                .then(|| (Format::Warpstream, k[..i].to_string()))
        } else if let Some(i) = k.find("tpuf/") {
            Some((Format::Turbopuffer, k[..i + "tpuf/".len()].to_string()))
        } else {
            k.find("turbopuffer/").map(|i| {
                (
                    Format::Turbopuffer,
                    k[..i + "turbopuffer/".len()].to_string(),
                )
            })
        };
        if let Some((f, root)) = found
            && seen.insert((f, root.clone()))
        {
            out.push(Detection {
                format: f,
                evidence: format!("{}… (name pattern only)", root),
                root,
            });
        }
    }
    out
}

fn bucket_size(span: Duration) -> Duration {
    if span <= Duration::hours(2) {
        Duration::minutes(1)
    } else if span <= Duration::days(2) {
        Duration::hours(1)
    } else {
        Duration::days(1)
    }
}

pub async fn analyze(src: &dyn Source, root: &str, format: Format) -> Result<Layout, SourceError> {
    let objects = src.list(root).await?;
    let mut l = Layout::new(format, root);
    l.decoded = false;
    l.objects = objects.len() as u64;
    l.bytes = objects.iter().map(|o| o.size).sum();
    if matches!(format, Format::Turbopuffer | Format::Warpstream) {
        l.issues.push(issue(Severity::Info, format!("The {} format is not published: sonde does not decode it. Shown: prefix statistics and write cadence from the listing only.", format.label())));
    }
    // by sub-prefix (two levels)
    let mut by: BTreeMap<String, (u64, u64, Option<DateTime<Utc>>)> = BTreeMap::new();
    for o in &objects {
        let rel = o.key.strip_prefix(root).unwrap_or(&o.key);
        let parts: Vec<&str> = rel.split('/').collect();
        let p = if parts.len() > 2 {
            format!("{}/{}/", parts[0], parts[1])
        } else if parts.len() == 2 {
            format!("{}/", parts[0])
        } else {
            "(top level)".into()
        };
        let e = by.entry(p).or_insert((0, 0, None));
        e.0 += 1;
        e.1 += o.size;
        e.2 = e.2.max(o.last_modified);
    }
    let mut pt = Table::new(
        "By prefix",
        &["prefix", "objects", "bytes", "avg size", "last write"],
    );
    let mut rows: Vec<_> = by.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1.1));
    for (p, (n, b, t)) in &rows {
        pt.row(vec![
            p.clone(),
            n.to_string(),
            human_bytes(*b),
            human_bytes(b / (*n).max(1)),
            time_str(*t),
        ]);
    }
    // cadence
    let mut times: Vec<DateTime<Utc>> = objects.iter().filter_map(|o| o.last_modified).collect();
    times.sort();
    let mut ct = Table::new("Write cadence", &["interval", "objects", "bytes"]);
    if let (Some(first), Some(last)) = (times.first().copied(), times.last().copied()) {
        let step = bucket_size(last - first);
        let mut buckets: BTreeMap<DateTime<Utc>, (u64, u64)> = BTreeMap::new();
        for o in &objects {
            if let Some(t) = o.last_modified {
                let b = t.duration_trunc(step).unwrap_or(t);
                let e = buckets.entry(b).or_default();
                e.0 += 1;
                e.1 += o.size;
            }
        }
        let gaps: Vec<i64> = times
            .windows(2)
            .map(|w| (w[1] - w[0]).num_milliseconds())
            .collect();
        let mut sorted = gaps.clone();
        sorted.sort();
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0);
        let p99 = sorted
            .get(sorted.len().saturating_sub(1) * 99 / 100)
            .copied()
            .unwrap_or(0);
        let max_gap = sorted.last().copied().unwrap_or(0);
        let busiest = buckets.values().map(|b| b.0).max().unwrap_or(0);
        let mut cumulative = (0u64, 0u64);
        for (t, (n, b)) in &buckets {
            ct.row(vec![
                t.format("%Y-%m-%d %H:%M").to_string(),
                n.to_string(),
                human_bytes(*b),
            ]);
            cumulative.0 += n;
            cumulative.1 += b;
            l.snapshots.push(Snapshot {
                time: Some(*t + step),
                label: t.format("%Y-%m-%d %H:%M").to_string(),
                facts: vec![
                    fact("Objects written by then", cumulative.0),
                    fact("Bytes written by then", human_bytes(cumulative.1)),
                ],
                tables: vec![],
            });
            l.timeline.push(Event {
                time: Some(*t),
                kind: "writes".into(),
                label: format!("{n} objects"),
                detail: human_bytes(*b),
                key: None,
                bytes: Some(*b),
            });
        }
        if max_gap > median.max(1) * 20 && times.len() > 20 {
            l.issues.push(issue(
                Severity::Warning,
                format!(
                    "a write pause of {} s against a median interval of {} ms",
                    max_gap / 1000,
                    median
                ),
            ));
        }
        l.facts = vec![
            fact("Objects", objects.len()),
            fact("Bytes", human_bytes(l.bytes)),
            fact("First write", time_str(Some(first))),
            fact("Last write", time_str(Some(last))),
            fact(
                "Writes per second",
                format!(
                    "{:.2}",
                    times.len() as f64
                        / ((last - first).num_milliseconds().max(1000) as f64 / 1000.0)
                ),
            ),
            fact("Median interval", format!("{median} ms")),
            fact("p99 interval", format!("{p99} ms")),
            fact("Longest pause", format!("{} s", max_gap / 1000)),
            fact(
                "Busiest interval",
                format!("{busiest} objects per {} min", step.num_minutes()),
            ),
        ];
    } else {
        l.facts = vec![
            fact("Objects", objects.len()),
            fact("Bytes", human_bytes(l.bytes)),
        ];
    }
    l.tables = vec![pt, ct];
    l.headline = format!(
        "{}: {} objects ({}), cadence only{}",
        format.label(),
        objects.len(),
        human_bytes(l.bytes),
        if format.decoded() {
            ""
        } else {
            " — format not published, not decoded"
        }
    );
    Ok(l)
}
