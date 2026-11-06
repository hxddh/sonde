use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One header, as sent or received (names keep their case on the wire;
/// comparisons ignore it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl Header {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Header { name: name.into(), value: value.into() }
    }
}

/// One HTTP exchange with an object store. Every field is redacted when the
/// record is built ([`Record::redacted`]), never later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    /// Monotonic id within the log (assigned by [`crate::RequestLog::push`]).
    pub id: u64,
    pub time: DateTime<Utc>,
    /// The endpoint's configured id (never its credentials).
    pub endpoint: String,
    /// The S3 operation name, such as `ListObjectsV2`.
    pub operation: String,
    pub method: String,
    /// The full URL, with presign / signature query parameters masked.
    pub url: String,
    pub bucket: Option<String>,
    pub key: Option<String>,
    /// Query parameters, masked like the URL.
    pub query: Vec<(String, String)>,
    /// `None` when no response arrived (a transport error).
    pub status: Option<u16>,
    pub latency_ms: f64,
    /// How many times the request was retried before this outcome.
    pub retries: u32,
    pub request_id: Option<String>,
    pub host_id: Option<String>,
    pub request_headers: Vec<Header>,
    pub response_headers: Vec<Header>,
    /// Bytes of the request body, when known.
    pub request_bytes: Option<u64>,
    /// Bytes of the response body read, when known.
    pub response_bytes: Option<u64>,
    /// The raw error body (XML usually), kept verbatim up to 64 KiB.
    pub error_body: Option<String>,
    /// A transport error (DNS, TLS, timeout), when there was no response.
    pub transport_error: Option<String>,
    /// The region used for signing, for "copy as aws cli".
    pub region: Option<String>,
    /// The job this request belongs to, if any.
    pub job: Option<String>,
}

/// A compact view for list rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordSummary {
    pub id: u64,
    pub time: DateTime<Utc>,
    pub endpoint: String,
    pub operation: String,
    pub method: String,
    pub target: String,
    pub status: Option<u16>,
    pub latency_ms: f64,
    pub retries: u32,
    pub request_id: Option<String>,
}

/// Largest error body kept on a record.
pub const MAX_ERROR_BODY: usize = 64 * 1024;

impl Record {
    /// Redacts every field that can carry a secret. Idempotent.
    pub fn redacted(mut self) -> Self {
        self.url = crate::redact::redact_url(&self.url);
        for (k, v) in &mut self.query {
            *v = crate::redact::redact_query_value(k, v);
        }
        for h in self.request_headers.iter_mut().chain(self.response_headers.iter_mut()) {
            h.value = crate::redact::redact_header(&h.name, &h.value);
        }
        if let Some(body) = &mut self.error_body {
            if body.len() > MAX_ERROR_BODY {
                let mut cut = MAX_ERROR_BODY;
                while !body.is_char_boundary(cut) {
                    cut -= 1;
                }
                body.truncate(cut);
                body.push_str("\n… (truncated)");
            }
            *body = crate::redact::redact_text(body);
        }
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.response_headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    pub fn is_error(&self) -> bool {
        self.status.is_none_or(|s| s >= 400)
    }

    pub fn summary(&self) -> RecordSummary {
        let target = match (&self.bucket, &self.key) {
            (Some(b), Some(k)) => format!("{b}/{k}"),
            (Some(b), None) => b.clone(),
            _ => String::from("/"),
        };
        RecordSummary {
            id: self.id,
            time: self.time,
            endpoint: self.endpoint.clone(),
            operation: self.operation.clone(),
            method: self.method.clone(),
            target,
            status: self.status,
            latency_ms: self.latency_ms,
            retries: self.retries,
            request_id: self.request_id.clone(),
        }
    }
}
