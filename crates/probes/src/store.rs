//! Runs on disk, by time, and the difference between two runs.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sonde_core::Paths;

use crate::model::{Run, Step, Verdict};

/// A judge fixture: steps recorded from a real service (or written from
/// AWS's documented behavior) and the verdict they must give.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fixture {
    pub probe: String,
    pub vendor: String,
    pub expect: Verdict,
    #[serde(default)]
    pub note: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: String,
    pub endpoint: String,
    pub endpoint_name: String,
    pub vendor: String,
    pub started: String,
    pub pass: usize,
    pub fail: usize,
    pub differs: usize,
    pub unsupported: usize,
    pub other: usize,
}

fn file(paths: &Paths, run: &Run) -> PathBuf {
    paths
        .probe_runs()
        .join(format!("{}--{}.json", run.id, run.endpoint))
}

pub fn save(paths: &Paths, run: &Run) -> Result<PathBuf, String> {
    let f = file(paths, run);
    std::fs::create_dir_all(paths.probe_runs()).map_err(|e| e.to_string())?;
    std::fs::write(&f, serde_json::to_vec(run).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(f)
}

pub fn summary(r: &Run) -> RunSummary {
    RunSummary {
        id: r.id.clone(),
        endpoint: r.endpoint.clone(),
        endpoint_name: r.endpoint_name.clone(),
        vendor: r.vendor.clone(),
        started: r.started.clone(),
        pass: r.count(Verdict::Pass),
        fail: r.count(Verdict::Fail),
        differs: r.count(Verdict::Differs),
        unsupported: r.count(Verdict::Unsupported),
        other: r.count(Verdict::Error) + r.count(Verdict::Skipped),
    }
}

/// Every saved run, newest first.
pub fn list(paths: &Paths) -> Vec<RunSummary> {
    let mut v: Vec<RunSummary> = std::fs::read_dir(paths.probe_runs())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice::<Run>(&b).ok())
        .map(|r| summary(&r))
        .collect();
    v.sort_by(|a, b| b.started.cmp(&a.started));
    v
}

pub fn load(paths: &Paths, id: &str, endpoint: &str) -> Option<Run> {
    serde_json::from_slice(
        &std::fs::read(paths.probe_runs().join(format!("{id}--{endpoint}.json"))).ok()?,
    )
    .ok()
}

/// The newest run of each endpoint.
pub fn latest(paths: &Paths) -> Vec<Run> {
    let mut seen = std::collections::HashSet::new();
    list(paths)
        .into_iter()
        .filter(|s| seen.insert(s.endpoint.clone()))
        .filter_map(|s| load(paths, &s.id, &s.endpoint))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub probe: String,
    pub before: Option<Verdict>,
    pub after: Option<Verdict>,
    pub before_summary: Option<String>,
    pub after_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_summary_text: Vec<crate::phrase::Phrase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_summary_text: Vec<crate::phrase::Phrase>,
    /// Same verdict, but other statuses or codes in the steps.
    pub steps_changed: bool,
}

/// What changed from `a` to `b`: verdicts, and the steps' statuses and codes.
pub fn diff(a: &Run, b: &Run) -> Vec<Change> {
    let mut ids: Vec<String> = a
        .results
        .iter()
        .chain(b.results.iter())
        .map(|r| r.id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    let sig = |r: &crate::ProbeResult| {
        r.steps
            .iter()
            .map(|s| format!("{}:{:?}:{:?}", s.label, s.status, s.code))
            .collect::<Vec<_>>()
    };
    ids.into_iter()
        .filter_map(|id| {
            let (x, y) = (a.result(&id), b.result(&id));
            let steps_changed = matches!((x, y), (Some(x), Some(y)) if sig(x) != sig(y));
            let verdict_changed = x.map(|r| r.verdict) != y.map(|r| r.verdict);
            (verdict_changed || steps_changed).then(|| Change {
                probe: id,
                before: x.map(|r| r.verdict),
                after: y.map(|r| r.verdict),
                before_summary: x.map(|r| r.summary.clone()),
                after_summary: y.map(|r| r.summary.clone()),
                before_summary_text: x.map(|r| r.summary_text.clone()).unwrap_or_default(),
                after_summary_text: y.map(|r| r.summary_text.clone()).unwrap_or_default(),
                steps_changed,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, ProbeResult};

    fn run(id: &str, v: Verdict, status: u16) -> Run {
        Run {
            id: id.into(),
            endpoint: "e".into(),
            endpoint_name: "E".into(),
            vendor: "minio".into(),
            server: None,
            started: format!("2024-01-0{}T00:00:00Z", if id == "a" { 1 } else { 2 }),
            finished: String::new(),
            sonde_version: "0".into(),
            results: vec![ProbeResult {
                id: "p".into(),
                category: Category::Range,
                verdict: v,
                summary: "s".into(),
                observations: vec![],
                summary_text: vec![],
                observations_text: vec![],
                steps: vec![Step {
                    label: "x".into(),
                    status: Some(status),
                    ..Default::default()
                }],
                exchanges: vec![],
                duration_ms: 0.0,
            }],
        }
    }

    #[test]
    fn diffs() {
        assert!(diff(&run("a", Verdict::Pass, 200), &run("b", Verdict::Pass, 200)).is_empty());
        let d = diff(
            &run("a", Verdict::Pass, 200),
            &run("b", Verdict::Differs, 409),
        );
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].after, Some(Verdict::Differs));
        let d = diff(
            &run("a", Verdict::Differs, 400),
            &run("b", Verdict::Differs, 409),
        );
        assert!(d[0].steps_changed);
    }

    #[test]
    fn store_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let p = Paths::rooted(dir.path());
        save(&p, &run("a", Verdict::Pass, 200)).unwrap();
        save(&p, &run("b", Verdict::Fail, 500)).unwrap();
        let l = list(&p);
        assert_eq!(l[0].id, "b");
        assert_eq!(l[0].fail, 1);
        assert_eq!(latest(&p).len(), 1);
        assert_eq!(
            load(&p, "a", "e").unwrap().results[0].verdict,
            Verdict::Pass
        );
    }
}
