use reqlog::Record;
use serde::{Deserialize, Serialize};

use crate::phrase::{self, Phrase};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    ConditionalWrites,
    Consistency,
    Checksums,
    Multipart,
    Range,
    Presign,
    Metadata,
    Keys,
    Listing,
    Delete,
    Versioning,
    ObjectLock,
    Encryption,
    Express,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::ConditionalWrites => "Conditional writes",
            Category::Consistency => "Consistency",
            Category::Checksums => "Checksums",
            Category::Multipart => "Multipart",
            Category::Range => "Range GET",
            Category::Presign => "Presigned URLs",
            Category::Metadata => "Metadata",
            Category::Keys => "Keys",
            Category::Listing => "Listing",
            Category::Delete => "Batch delete",
            Category::Versioning => "Versioning",
            Category::ObjectLock => "Object Lock",
            Category::Encryption => "Encryption",
            Category::Express => "S3 Express",
        }
    }
}

/// What a probe needs beyond a scratch prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Needs {
    /// A bucket of its own, created and removed by the run.
    TempBucket,
    /// HTTPS (SSE-C keys are refused over HTTP).
    Https,
    /// Waits in real time (presign expiry).
    Slow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeMeta {
    pub id: String,
    pub category: Category,
    pub title: String,
    pub description: String,
    /// What AWS S3 does, the reference the verdict compares with.
    pub aws_behavior: String,
    /// The title, description and AWS behaviour as phrases
    /// (`probe.<id>.title`, `.description`, `.aws`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub title_text: Vec<Phrase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub description_text: Vec<Phrase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aws_behavior_text: Vec<Phrase>,
    pub needs: Vec<Needs>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// Behaves as AWS does.
    Pass,
    /// Breaks the guarantee (data written that should not be, wrong data read).
    Fail,
    /// Works, but differently from AWS (another status, another format).
    Differs,
    /// The feature is not implemented.
    Unsupported,
    /// The probe could not run (setup failed, skipped).
    Error,
    Skipped,
}

impl Verdict {
    pub fn symbol(self) -> &'static str {
        match self {
            Verdict::Pass => "✓",
            Verdict::Fail => "✗",
            Verdict::Differs => "≠",
            Verdict::Unsupported => "∅",
            Verdict::Error => "!",
            Verdict::Skipped => "–",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::Differs => "differs",
            Verdict::Unsupported => "unsupported",
            Verdict::Error => "error",
            Verdict::Skipped => "skipped",
        }
    }
}

/// One request a probe made, as the judge sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub label: String,
    pub status: Option<u16>,
    pub code: Option<String>,
    pub headers: Vec<(String, String)>,
    /// The body, up to 4 KiB (error bodies, small objects).
    pub body: String,
    pub record_id: Option<u64>,
}

impl Step {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    pub fn ok(&self) -> bool {
        self.status.is_some_and(|s| (200..300).contains(&s))
    }
    pub fn is(&self, status: u16) -> bool {
        self.status == Some(status)
    }
    pub fn not_implemented(&self) -> bool {
        self.status == Some(501)
            || matches!(
                self.code.as_deref(),
                Some("NotImplemented" | "NotSupported" | "UnsupportedOperation")
            )
    }
    pub fn describe(&self) -> String {
        match (self.status, &self.code) {
            (Some(s), Some(c)) => format!("{} → {s} {c}", self.label),
            (Some(s), None) => format!("{} → {s}", self.label),
            (None, _) => format!("{} → no response ({})", self.label, self.body),
        }
    }
}

/// The steps of a probe, by label.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Steps(pub Vec<Step>);

impl Steps {
    pub fn get(&self, label: &str) -> Option<&Step> {
        self.0.iter().find(|s| s.label == label)
    }
    /// The step, or an empty one (status `None`) when it never ran.
    pub fn at(&self, label: &str) -> Step {
        self.get(label).cloned().unwrap_or_else(|| Step {
            label: label.into(),
            body: "not run".into(),
            ..Default::default()
        })
    }
    pub fn any_not_implemented(&self) -> Option<&Step> {
        self.0.iter().find(|s| s.not_implemented())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Judgement {
    pub verdict: Verdict,
    pub summary: String,
    pub observations: Vec<String>,
    /// The summary as phrases, and each observation as a text of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_text: Vec<Phrase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observations_text: Vec<Vec<Phrase>>,
}

impl Judgement {
    /// A verdict with its summary as phrases (and their English); the
    /// steps are its first observations.
    pub fn say(verdict: Verdict, summary: impl Into<Vec<Phrase>>, steps: &Steps) -> Self {
        let summary_text = summary.into();
        let mut j = Judgement {
            verdict,
            summary: phrase::en(&summary_text),
            observations: vec![],
            summary_text,
            observations_text: vec![],
        };
        for s in &steps.0 {
            // a step is statuses and codes: said as it is
            j.observe(phrase::raw(s.describe()));
        }
        j
    }

    /// Adds an observation.
    pub fn observe(&mut self, text: impl Into<Vec<Phrase>>) {
        let text = text.into();
        self.observations.push(phrase::en(&text));
        self.observations_text.push(text);
    }

    /// Says `text` before the summary (what overrides the verdict).
    pub fn preface(&mut self, text: Phrase) {
        self.summary_text.insert(0, text);
        self.summary = phrase::en(&self.summary_text);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub id: String,
    pub category: Category,
    pub verdict: Verdict,
    pub summary: String,
    pub observations: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_text: Vec<Phrase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observations_text: Vec<Vec<Phrase>>,
    pub steps: Vec<Step>,
    /// The raw requests and responses, redacted.
    pub exchanges: Vec<Record>,
    pub duration_ms: f64,
}

/// One endpoint's run of the suite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub endpoint: String,
    pub endpoint_name: String,
    pub vendor: String,
    pub server: Option<String>,
    pub started: String,
    pub finished: String,
    pub sonde_version: String,
    pub results: Vec<ProbeResult>,
}

impl Run {
    pub fn count(&self, v: Verdict) -> usize {
        self.results.iter().filter(|r| r.verdict == v).count()
    }
    pub fn result(&self, id: &str) -> Option<&ProbeResult> {
        self.results.iter().find(|r| r.id == id)
    }
}
