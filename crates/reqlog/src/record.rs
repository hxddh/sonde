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
        Header {
            name: name.into(),
            value: value.into(),
        }
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
    /// A control request's body (the XML of DeleteObjects, a lifecycle rule,
    /// …), kept so the request can be replayed; never object data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_body: Option<String>,
    /// The body was longer than what is kept: replaying it would send another document.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub request_body_truncated: bool,
    /// A transport error (DNS, TLS, timeout), when there was no response.
    pub transport_error: Option<String>,
    /// The region used for signing, for "copy as aws cli".
    pub region: Option<String>,
    /// The job this request belongs to, if any.
    pub job: Option<String>,
    /// How long until the response's first byte: the proxy's measure, or the
    /// turnaround an access log gives (S3's time to its first byte).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_byte_ms: Option<f64>,
    /// Who sent it, as the store's own log names them (an IAM ARN or a
    /// canonical user id); only an imported log knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester: Option<String>,
    /// The fault scenario that answered this request in the store's place
    /// (or delayed it, or lost its answer): sonde's doing, not the store's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub injected: Option<String>,
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
    /// Time to the response's first byte, when measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_byte_ms: Option<f64>,
}

/// Operations whose body is a small control document (XML or JSON) rather
/// than object data: their bodies are kept on records, for replay.
pub fn is_control_operation(op: &str) -> bool {
    matches!(
        op,
        "DeleteObjects"
            | "CompleteMultipartUpload"
            | "CreateBucket"
            | "PutObjectTagging"
            | "PutObjectAcl"
            | "PutObjectRetention"
            | "PutObjectLegalHold"
            | "RestoreObject"
    ) || (op.starts_with("PutBucket")
        || op.starts_with("PutObjectLock")
        || op == "PutPublicAccessBlock")
}

/// Largest error body kept on a record.
pub const MAX_ERROR_BODY: usize = 64 * 1024;
/// Largest control body kept on a record.
pub const MAX_REQUEST_BODY: usize = 256 * 1024;

/// The longest a request is taken to have lasted (a day): a latency past it is
/// a bad record, and kept from overflowing the times built from it.
pub const MAX_LATENCY_MS: f64 = 86_400_000.0;

impl Record {
    /// The request's latency in microseconds, never negative and never past a day.
    pub fn latency_us(&self) -> i64 {
        (self.latency_ms.clamp(0.0, MAX_LATENCY_MS) * 1000.0) as i64
    }
}

impl Record {
    /// Redacts every field that can carry a secret. Idempotent.
    pub fn redacted(mut self) -> Self {
        self.url = crate::redact::redact_url(&self.url);
        for (k, v) in &mut self.query {
            *v = crate::redact::redact_query_value(k, v);
        }
        for h in self
            .request_headers
            .iter_mut()
            .chain(self.response_headers.iter_mut())
        {
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
        if let Some(body) = &mut self.request_body {
            if body.len() > MAX_REQUEST_BODY {
                let mut cut = MAX_REQUEST_BODY;
                while !body.is_char_boundary(cut) {
                    cut -= 1;
                }
                body.truncate(cut);
                self.request_body_truncated = true;
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
        // an answer that began and was cut is an error too: it has a status
        // and a transport error
        self.status.is_none_or(|s| s >= 400) || self.transport_error.is_some()
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
            first_byte_ms: self.first_byte_ms,
        }
    }
}
