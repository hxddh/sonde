//! What every lens produces: facts, issues, a timeline, tables, and
//! snapshots of the logical state over time (for replay).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::phrase::{self, Phrase};
pub use crate::phrase::{plural, raw};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Slatedb,
    Lakefs,
    Woodpecker,
    Iceberg,
    Turbopuffer,
    Warpstream,
    /// Any prefix: write cadence only.
    Generic,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Slatedb => "SlateDB",
            Format::Lakefs => "lakeFS",
            Format::Woodpecker => "Milvus Woodpecker",
            Format::Iceberg => "Apache Iceberg",
            Format::Turbopuffer => "turbopuffer",
            Format::Warpstream => "WarpStream",
            Format::Generic => "Generic prefix",
        }
    }
    /// Whether sonde decodes the format (closed formats are only measured).
    pub fn decoded(self) -> bool {
        !matches!(
            self,
            Format::Turbopuffer | Format::Warpstream | Format::Generic
        )
    }
    pub fn id(self) -> &'static str {
        match self {
            Format::Slatedb => "slatedb",
            Format::Lakefs => "lakefs",
            Format::Woodpecker => "woodpecker",
            Format::Iceberg => "iceberg",
            Format::Turbopuffer => "turbopuffer",
            Format::Warpstream => "warpstream",
            Format::Generic => "generic",
        }
    }
    pub fn from_id(s: &str) -> Option<Format> {
        [
            Format::Slatedb,
            Format::Lakefs,
            Format::Woodpecker,
            Format::Iceberg,
            Format::Turbopuffer,
            Format::Warpstream,
            Format::Generic,
        ]
        .into_iter()
        .find(|f| f.id() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub format: Format,
    /// The layout's root prefix.
    pub root: String,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub label: String,
    pub value: String,
    /// The label as a phrase, and the value when it is words (a figure is
    /// said as it is).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub label_text: Vec<Phrase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_text: Vec<Phrase>,
}

pub fn fact(label: &str, value: impl ToString) -> Fact {
    Fact {
        label: label.into(),
        value: value.to_string(),
        label_text: vec![phrase::fact_label(label)],
        value_text: vec![],
    }
}

/// A fact whose value is words.
pub fn fact_said(label: &str, value: impl Into<Vec<Phrase>>) -> Fact {
    let value_text = value.into();
    Fact {
        value: phrase::en(&value_text),
        value_text,
        ..fact(label, "")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub severity: Severity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_text: Vec<Phrase>,
}

pub fn issue(severity: Severity, message: impl Into<Vec<Phrase>>) -> Issue {
    let message_text = message.into();
    Issue {
        severity,
        message: phrase::en(&message_text),
        message_text,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub time: Option<DateTime<Utc>>,
    /// `wal`, `manifest`, `fence`, `snapshot`, `segment`, `commit`, `gap`, ...
    pub kind: String,
    pub label: String,
    pub detail: String,
    pub key: Option<String>,
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub label_text: Vec<Phrase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_text: Vec<Phrase>,
}

impl Event {
    /// An entry of the timeline; `detail` may be empty.
    pub fn new(
        time: Option<DateTime<Utc>>,
        kind: &str,
        label: impl Into<Vec<Phrase>>,
        detail: impl Into<Vec<Phrase>>,
    ) -> Event {
        let (label_text, detail_text) = (label.into(), detail.into());
        Event {
            time,
            kind: kind.into(),
            label: phrase::en(&label_text),
            detail: phrase::en(&detail_text),
            key: None,
            bytes: None,
            label_text,
            detail_text,
        }
    }
    pub fn at(mut self, key: impl Into<String>) -> Event {
        self.key = Some(key.into());
        self
    }
    pub fn sized(mut self, bytes: u64) -> Event {
        self.bytes = Some(bytes);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(title: &str, columns: &[&str]) -> Self {
        Table {
            title: title.into(),
            columns: columns.iter().map(|s| s.to_string()).collect(),
            rows: vec![],
        }
    }
    pub fn row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }
}

/// The logical state at one point in time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub time: Option<DateTime<Utc>>,
    pub label: String,
    pub facts: Vec<Fact>,
    pub tables: Vec<Table>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub format: Format,
    pub root: String,
    /// The on-storage format version decoded (`manifest v2`, `format-version 2`).
    pub format_version: Option<String>,
    pub decoded: bool,
    /// One line: the headline finding.
    pub headline: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headline_text: Vec<Phrase>,
    pub facts: Vec<Fact>,
    pub issues: Vec<Issue>,
    pub timeline: Vec<Event>,
    pub tables: Vec<Table>,
    /// Logical states over time, oldest first.
    pub snapshots: Vec<Snapshot>,
    pub objects: u64,
    pub bytes: u64,
}

impl Layout {
    pub fn new(format: Format, root: &str) -> Self {
        Layout {
            format,
            root: root.into(),
            format_version: None,
            decoded: format.decoded(),
            headline: String::new(),
            headline_text: vec![],
            facts: vec![],
            issues: vec![],
            timeline: vec![],
            tables: vec![],
            snapshots: vec![],
            objects: 0,
            bytes: 0,
        }
    }

    /// The state at `t`: the last snapshot taken at or before it.
    pub fn state_at(&self, t: DateTime<Utc>) -> Option<&Snapshot> {
        self.snapshots
            .iter()
            .rev()
            .find(|s| s.time.is_some_and(|st| st <= t))
            .or_else(|| self.snapshots.iter().find(|s| s.time.is_none()))
    }

    /// Says the headline: the phrases and their English.
    pub fn say_headline(&mut self, text: impl Into<Vec<Phrase>>) {
        self.headline_text = text.into();
        self.headline = phrase::en(&self.headline_text);
    }

    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::Error)
    }
}

pub fn human_bytes(b: u64) -> String {
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

pub fn time_str(t: Option<DateTime<Utc>>) -> String {
    t.map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "—".into())
}
