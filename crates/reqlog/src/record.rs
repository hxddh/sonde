use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A header's name or value: text that the records repeating it share. A
/// session of a million requests says `user-agent` and its value a million
/// times; kept once each, a record's headers are a third of the size. It
/// reads as a `str` everywhere, and is written to a file as one.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Text(std::sync::Arc<str>);

/// How many texts a thread remembers to share (the newest at each place).
const SHARED: usize = 2048;
/// Longer texts are not looked for: they are seldom repeated.
const SHARED_LEN: usize = 512;

thread_local! {
    static RECENT: std::cell::RefCell<Vec<Option<std::sync::Arc<str>>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl Text {
    /// `s` as a text: the one a recent equal text is kept as, when there is one.
    pub fn new(s: &str) -> Text {
        if s.len() > SHARED_LEN {
            return Text(s.into());
        }
        // FNV-1a
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in s.as_bytes() {
            h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        let at = (h ^ (h >> 32)) as usize % SHARED;
        RECENT
            .try_with(|recent| {
                let mut recent = recent.borrow_mut();
                if recent.is_empty() {
                    recent.resize(SHARED, None);
                }
                match &recent[at] {
                    Some(t) if **t == *s => Text(t.clone()),
                    _ => {
                        let t: std::sync::Arc<str> = s.into();
                        recent[at] = Some(t.clone());
                        Text(t)
                    }
                }
            })
            .unwrap_or_else(|_| Text(s.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for Text {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Text {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for Text {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl Default for Text {
    fn default() -> Self {
        Text::new("")
    }
}

impl std::fmt::Debug for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&*self.0, f)
    }
}

impl std::fmt::Display for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Text {
    fn from(s: &str) -> Self {
        Text::new(s)
    }
}

impl From<String> for Text {
    fn from(s: String) -> Self {
        Text::new(&s)
    }
}

impl From<&String> for Text {
    fn from(s: &String) -> Self {
        Text::new(s)
    }
}

impl From<Text> for String {
    fn from(t: Text) -> Self {
        t.0.as_ref().to_owned()
    }
}

impl PartialEq<str> for Text {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<&str> for Text {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == *other
    }
}

impl PartialEq<String> for Text {
    fn eq(&self, other: &String) -> bool {
        &*self.0 == other.as_str()
    }
}

impl PartialEq<Text> for str {
    fn eq(&self, other: &Text) -> bool {
        self == &*other.0
    }
}

impl PartialEq<Text> for &str {
    fn eq(&self, other: &Text) -> bool {
        *self == &*other.0
    }
}

impl PartialEq<Text> for String {
    fn eq(&self, other: &Text) -> bool {
        self.as_str() == &*other.0
    }
}

impl Serialize for Text {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = Text;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a string")
            }
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Text, E> {
                Ok(Text::new(s))
            }
        }
        d.deserialize_str(V)
    }
}

/// One header, as sent or received (names keep their case on the wire;
/// comparisons ignore it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: Text,
    pub value: Text,
}

impl Header {
    pub fn new(name: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        Header {
            name: Text::new(name.as_ref()),
            value: Text::new(value.as_ref()),
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
    pub endpoint: Text,
    /// The S3 operation name, such as `ListObjectsV2`.
    pub operation: Text,
    pub method: Text,
    /// The full URL, with presign / signature query parameters masked.
    pub url: String,
    pub bucket: Option<Text>,
    pub key: Option<Text>,
    /// Query parameters, masked like the URL.
    pub query: Vec<(Text, Text)>,
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
    pub region: Option<Text>,
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
    /// The fault scenario sonde answered this request under (a trial's
    /// injected failure or delay): the row is marked, not the store blamed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub injected: Option<String>,
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

/// The header a listing's record says how many keys it listed in: the
/// body is not kept, and a replay compares listings by it.
pub const KEYS_HEADER: &str = "x-sonde-keys";

/// Whether `op` lists keys (its record gets [`KEYS_HEADER`]).
pub fn is_listing(op: &str) -> bool {
    matches!(op, "ListObjects" | "ListObjectsV2" | "ListObjectVersions")
}

/// `<Key>`s in a listing's body, read a piece at a time: a tag across two
/// pieces is counted once.
#[derive(Debug, Default, Clone)]
pub struct KeyCounter {
    pub keys: usize,
    tail: Vec<u8>,
}

impl KeyCounter {
    pub fn feed(&mut self, piece: &[u8]) {
        const TAG: &[u8] = b"<Key>";
        let mut seen = std::mem::take(&mut self.tail);
        seen.extend_from_slice(piece);
        self.keys += seen.windows(TAG.len()).filter(|w| *w == TAG).count();
        // (shorter than the tag: nothing counted is counted again)
        self.tail = seen[seen.len().saturating_sub(TAG.len() - 1)..].to_vec();
    }
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
            *v = crate::redact::redact_query_value(k, v).into();
        }
        for h in self
            .request_headers
            .iter_mut()
            .chain(self.response_headers.iter_mut())
        {
            h.value = crate::redact::redact_header(&h.name, &h.value).into();
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
            (Some(b), None) => b.to_string(),
            _ => String::from("/"),
        };
        RecordSummary {
            id: self.id,
            time: self.time,
            endpoint: self.endpoint.to_string(),
            operation: self.operation.to_string(),
            method: self.method.to_string(),
            target,
            status: self.status,
            latency_ms: self.latency_ms,
            retries: self.retries,
            request_id: self.request_id.clone(),
            first_byte_ms: self.first_byte_ms,
            injected: self.injected.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_counted_across_pieces_once() {
        let body = b"<R><Contents><Key>a</Key></Contents><Contents><Key>b</Key></Contents></R>";
        for cut in 0..body.len() {
            let mut k = KeyCounter::default();
            k.feed(&body[..cut]);
            k.feed(&body[cut..]);
            assert_eq!(k.keys, 2, "cut at {cut}");
        }
    }

    #[test]
    fn a_summary_says_which_scenario_answered_in_the_stores_place() {
        let mut r = Record {
            id: 7,
            operation: "PutObject".into(),
            ..Default::default()
        };
        let plain = serde_json::to_value(r.summary()).unwrap();
        assert!(plain.get("injected").is_none(), "{plain}");
        r.injected = Some("slowdown".into());
        let marked = serde_json::to_value(r.summary()).unwrap();
        assert_eq!(marked["injected"], "slowdown");
        // a row without the mark reads as the store's own answer
        let old: RecordSummary = serde_json::from_value(plain).unwrap();
        assert_eq!(old.injected, None);
    }
}
