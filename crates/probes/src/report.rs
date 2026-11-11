//! The vendor × probe matrix, and its JSON and Markdown reports.

use serde::{Deserialize, Serialize};

use crate::model::{ProbeMeta, Run, Verdict};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    pub verdict: Verdict,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_text: Vec<crate::phrase::Phrase>,
    pub run: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub endpoint: String,
    pub name: String,
    pub vendor: String,
    pub server: Option<String>,
    pub run: String,
    pub started: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub probe: ProbeMeta,
    /// One per column; `None` where the probe did not run.
    pub cells: Vec<Option<Cell>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Matrix {
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
}

pub fn matrix(runs: &[Run]) -> Matrix {
    let columns = runs
        .iter()
        .map(|r| Column {
            endpoint: r.endpoint.clone(),
            name: r.endpoint_name.clone(),
            vendor: r.vendor.clone(),
            server: r.server.clone(),
            run: r.id.clone(),
            started: r.started.clone(),
        })
        .collect();
    let rows = crate::catalog()
        .into_iter()
        .map(|m| Row {
            cells: runs
                .iter()
                .map(|r| {
                    r.result(&m.id).map(|x| Cell {
                        verdict: x.verdict,
                        summary: x.summary.clone(),
                        summary_text: x.summary_text.clone(),
                        run: r.id.clone(),
                    })
                })
                .collect(),
            probe: m,
        })
        .filter(|row| row.cells.iter().any(Option::is_some))
        .collect();
    Matrix { columns, rows }
}

fn md_escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// A Markdown report: the matrix, then each probe's AWS reference and
/// what every target did.
pub fn markdown(runs: &[Run]) -> String {
    let m = matrix(runs);
    let mut out = String::from("# sonde compatibility report\n\n");
    out.push_str(
        "Legend: ✓ pass · ✗ fail · ≠ differs from AWS · ∅ unsupported · ! error · – skipped\n\n",
    );
    for c in &m.columns {
        out.push_str(&format!(
            "- **{}** ({}, {}) — run `{}` at {}{}\n",
            c.name,
            c.endpoint,
            c.vendor,
            c.run,
            c.started,
            c.server
                .as_ref()
                .map(|s| format!(", server `{s}`"))
                .unwrap_or_default()
        ));
    }
    out.push_str("\n| Probe | ");
    out.push_str(
        &m.columns
            .iter()
            .map(|c| md_escape(&c.name))
            .collect::<Vec<_>>()
            .join(" | "),
    );
    out.push_str(" |\n|---|");
    out.push_str(&"---|".repeat(m.columns.len()));
    out.push('\n');
    for r in &m.rows {
        out.push_str(&format!("| `{}` | ", r.probe.id));
        out.push_str(
            &r.cells
                .iter()
                .map(|c| {
                    c.as_ref()
                        .map(|c| c.verdict.symbol().to_string())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" | "),
        );
        out.push_str(" |\n");
    }
    out.push_str("\n## Details\n");
    for r in &m.rows {
        out.push_str(&format!(
            "\n### `{}` — {}\n\n{}\n\n**AWS:** {}\n\n",
            r.probe.id, r.probe.title, r.probe.description, r.probe.aws_behavior
        ));
        for (c, cell) in m.columns.iter().zip(&r.cells) {
            if let Some(cell) = cell {
                out.push_str(&format!(
                    "- {} {}: {} — {}\n",
                    cell.verdict.symbol(),
                    c.name,
                    cell.verdict.label(),
                    md_escape(&cell.summary)
                ));
            }
        }
    }
    out
}

/// Exit status for scripts: 0 all pass (or unsupported/skipped), 1 any
/// failure, 2 any difference but no failure, 3 errors only.
pub fn exit_code(runs: &[Run]) -> i32 {
    let all: Vec<Verdict> = runs
        .iter()
        .flat_map(|r| r.results.iter().map(|x| x.verdict))
        .collect();
    if all.contains(&Verdict::Fail) {
        1
    } else if all.contains(&Verdict::Differs) {
        2
    } else if all.contains(&Verdict::Error) {
        3
    } else {
        0
    }
}
