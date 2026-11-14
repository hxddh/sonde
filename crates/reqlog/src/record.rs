use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Lists, Name};

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
///
/// What every request has is held here, in 120 bytes. Its query and
/// headers are behind one pointer ([`Record::lists`]) that the records
/// holding the same share. What few have (an error's body, a transport
/// error, a fault's mark) and what no analysis reads (the URL, the store's
/// ids, the endpoint and region) is behind another ([`Record::rare`]),
/// which a record read for its analysis has only when it failed or was
/// marked. Written to a file and sent to the app as one flat object.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Record {
    /// Monotonic id within the log (assigned by [`crate::RequestLog::push`]).
    pub id: u64,
    pub time: DateTime<Utc>,
    /// `None` when no response arrived (a transport error).
    pub status: Option<u16>,
    /// The request's kind: the S3 API call, such as `ListObjectsV2`.
    pub kind: Name,
    pub method: Name,
    pub bucket: Option<Name>,
    /// How many times the request was retried before this outcome.
    pub retries: u16,
    pub latency_ms: f64,
    /// How long until the response's first byte: the proxy's measure, or the
    /// turnaround an access log gives (S3's time to its first byte).
    pub first_byte_ms: Option<f64>,
    /// Bytes of the request body, when known.
    pub request_bytes: Option<u64>,
    /// Bytes of the response body read, when known.
    pub response_bytes: Option<u64>,
    pub key: Option<Text>,
    /// Its query and headers ([`Lists`]): `None` when they would be empty.
    pub lists: Option<std::sync::Arc<Lists>>,
    /// The rest ([`Rare`]): `None` when it would be empty.
    pub rare: Option<Box<Rare>>,
}

/// What of a record few requests have, or no analysis reads.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rare {
    /// The endpoint's configured id (never its credentials).
    pub endpoint: String,
    /// The full URL, with presign / signature query parameters masked.
    pub url: String,
    /// The region used for signing, for "copy as aws cli".
    pub region: Option<String>,
    pub request_id: Option<String>,
    pub host_id: Option<String>,
    /// The raw error body (XML usually), kept verbatim up to 64 KiB.
    pub error_body: Option<String>,
    /// A control request's body (the XML of DeleteObjects, a lifecycle rule,
    /// …), kept so the request can be replayed; never object data.
    pub request_body: Option<String>,
    /// The body was longer than what is kept: replaying it would send another document.
    pub request_body_truncated: bool,
    /// A transport error (DNS, TLS, timeout), when there was no response.
    pub transport_error: Option<String>,
    /// The job this request belongs to, if any.
    pub job: Option<String>,
    /// Who sent it, as the store's own log names them (an IAM ARN or a
    /// canonical user id); only an imported log knows.
    pub requester: Option<String>,
    /// The fault scenario that answered this request in the store's place
    /// (or delayed it, or lost its answer): sonde's doing, not the store's.
    pub injected: Option<String>,
}

/// A record's rare part with nothing in it: what [`Record::rare`] reads
/// when the record has none.
static NO_RARE: Rare = Rare {
    endpoint: String::new(),
    url: String::new(),
    region: None,
    request_id: None,
    host_id: None,
    error_body: None,
    request_body: None,
    request_body_truncated: false,
    transport_error: None,
    job: None,
    requester: None,
    injected: None,
};

impl Rare {
    pub fn is_empty(&self) -> bool {
        *self == NO_RARE
    }
}

/// A record's rare part as a field: `rare: Rare { … }.into()`.
impl From<Rare> for Option<Box<Rare>> {
    fn from(r: Rare) -> Self {
        (!r.is_empty()).then(|| Box::new(r))
    }
}

/// A record as it is written: one flat object, its fields in this order.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Written<'a> {
    id: u64,
    time: &'a DateTime<Utc>,
    endpoint: &'a str,
    kind: Name,
    method: Name,
    url: &'a str,
    bucket: Option<Name>,
    key: &'a Option<Text>,
    query: &'a [(Text, Text)],
    status: Option<u16>,
    latency_ms: f64,
    retries: u16,
    request_id: &'a Option<String>,
    host_id: &'a Option<String>,
    request_headers: &'a [Header],
    response_headers: &'a [Header],
    request_bytes: Option<u64>,
    response_bytes: Option<u64>,
    error_body: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_body: &'a Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    request_body_truncated: bool,
    transport_error: &'a Option<String>,
    region: &'a Option<String>,
    job: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_byte_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requester: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    injected: &'a Option<String>,
}

/// A record as it is read.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Read {
    id: u64,
    time: DateTime<Utc>,
    #[serde(default)]
    endpoint: String,
    kind: Name,
    method: Name,
    #[serde(default)]
    url: String,
    bucket: Option<Name>,
    key: Option<Text>,
    #[serde(default)]
    query: Vec<(Text, Text)>,
    status: Option<u16>,
    latency_ms: f64,
    #[serde(default)]
    retries: u16,
    request_id: Option<String>,
    host_id: Option<String>,
    #[serde(default)]
    request_headers: Vec<Header>,
    #[serde(default)]
    response_headers: Vec<Header>,
    request_bytes: Option<u64>,
    response_bytes: Option<u64>,
    error_body: Option<String>,
    request_body: Option<String>,
    #[serde(default)]
    request_body_truncated: bool,
    transport_error: Option<String>,
    region: Option<String>,
    job: Option<String>,
    first_byte_ms: Option<f64>,
    requester: Option<String>,
    injected: Option<String>,
}

impl Serialize for Record {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let x = self.rare();
        Written {
            id: self.id,
            time: &self.time,
            endpoint: &x.endpoint,
            kind: self.kind,
            method: self.method,
            url: &x.url,
            bucket: self.bucket,
            key: &self.key,
            query: self.query(),
            status: self.status,
            latency_ms: self.latency_ms,
            retries: self.retries,
            request_id: &x.request_id,
            host_id: &x.host_id,
            request_headers: self.request_headers(),
            response_headers: self.response_headers(),
            request_bytes: self.request_bytes,
            response_bytes: self.response_bytes,
            error_body: &x.error_body,
            request_body: &x.request_body,
            request_body_truncated: x.request_body_truncated,
            transport_error: &x.transport_error,
            region: &x.region,
            job: &x.job,
            first_byte_ms: self.first_byte_ms,
            requester: &x.requester,
            injected: &x.injected,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Record {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = Read::deserialize(d)?;
        Ok(Record {
            id: r.id,
            time: r.time,
            status: r.status,
            kind: r.kind,
            method: r.method,
            bucket: r.bucket,
            retries: r.retries,
            latency_ms: r.latency_ms,
            first_byte_ms: r.first_byte_ms,
            request_bytes: r.request_bytes,
            response_bytes: r.response_bytes,
            key: r.key,
            lists: Lists {
                query: r.query,
                request_headers: r.request_headers,
                response_headers: r.response_headers,
            }
            .into(),
            rare: Rare {
                endpoint: r.endpoint,
                url: r.url,
                region: r.region,
                request_id: r.request_id,
                host_id: r.host_id,
                error_body: r.error_body,
                request_body: r.request_body,
                request_body_truncated: r.request_body_truncated,
                transport_error: r.transport_error,
                job: r.job,
                requester: r.requester,
                injected: r.injected,
            }
            .into(),
        })
    }
}

/// A compact view for list rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordSummary {
    pub id: u64,
    pub time: DateTime<Utc>,
    pub endpoint: String,
    pub kind: String,
    pub method: String,
    pub target: String,
    pub status: Option<u16>,
    pub latency_ms: f64,
    pub retries: u16,
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
    /// Its rare part ([`Rare`]): an empty one when it has none.
    pub fn rare(&self) -> &Rare {
        self.rare.as_deref().unwrap_or(&NO_RARE)
    }

    /// Its rare part, to change: made when it has none.
    pub fn rare_mut(&mut self) -> &mut Rare {
        self.rare.get_or_insert_with(Box::default)
    }

    /// Query parameters, masked like the URL.
    pub fn query(&self) -> &[(Text, Text)] {
        self.lists.as_deref().map_or(&[], |l| &l.query)
    }

    pub fn request_headers(&self) -> &[Header] {
        self.lists.as_deref().map_or(&[], |l| &l.request_headers)
    }

    pub fn response_headers(&self) -> &[Header] {
        self.lists.as_deref().map_or(&[], |l| &l.response_headers)
    }

    /// Its query and headers, to change: its own (copied when shared),
    /// made when it has none.
    pub fn lists_mut(&mut self) -> &mut Lists {
        std::sync::Arc::make_mut(self.lists.get_or_insert_with(Default::default))
    }

    /// Redacts every field that can carry a secret. Idempotent.
    pub fn redacted(mut self) -> Self {
        if self.lists.is_some() {
            let l = self.lists_mut();
            for (k, v) in &mut l.query {
                *v = crate::redact::redact_query_value(k, v).into();
            }
            for h in l.request_headers.iter_mut().chain(&mut l.response_headers) {
                h.value = crate::redact::redact_header(&h.name, &h.value).into();
            }
        }
        let Some(x) = self.rare.as_deref_mut() else {
            return self;
        };
        x.url = crate::redact::redact_url(&x.url);
        if let Some(body) = &mut x.error_body {
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
        if let Some(body) = &mut x.request_body {
            if body.len() > MAX_REQUEST_BODY {
                let mut cut = MAX_REQUEST_BODY;
                while !body.is_char_boundary(cut) {
                    cut -= 1;
                }
                body.truncate(cut);
                x.request_body_truncated = true;
            }
            *body = crate::redact::redact_text(body);
        }
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.response_headers()
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    pub fn is_error(&self) -> bool {
        // an answer that began and was cut is an error too: it has a status
        // and a transport error
        self.status.is_none_or(|s| s >= 400) || self.rare().transport_error.is_some()
    }

    pub fn summary(&self) -> RecordSummary {
        let target = match (&self.bucket, &self.key) {
            (Some(b), Some(k)) => format!("{b}/{k}"),
            (Some(b), None) => b.to_string(),
            _ => String::from("/"),
        };
        let x = self.rare();
        RecordSummary {
            id: self.id,
            time: self.time,
            endpoint: x.endpoint.clone(),
            kind: self.kind.to_string(),
            method: self.method.to_string(),
            target,
            status: self.status,
            latency_ms: self.latency_ms,
            retries: self.retries,
            request_id: x.request_id.clone(),
            first_byte_ms: self.first_byte_ms,
            injected: x.injected.clone(),
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
    fn a_record_is_small_and_written_flat() {
        assert_eq!(std::mem::size_of::<Record>(), 120);
        let r = Record {
            id: 3,
            kind: "GetObject".into(),
            method: "GET".into(),
            bucket: Some("b".into()),
            key: Some("k".into()),
            status: Some(404),
            lists: Lists {
                request_headers: vec![Header::new("user-agent", "x")],
                ..Default::default()
            }
            .into(),
            rare: Rare {
                endpoint: "prod".into(),
                url: "https://s3/b/k".into(),
                error_body: Some("<Error/>".into()),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        };
        let text = serde_json::to_string(&r).unwrap();
        assert!(text.starts_with(r#"{"id":3,"time":"#), "{text}");
        assert!(text.contains(r#""endpoint":"prod","kind":"GetObject","method":"GET","url":"https://s3/b/k","bucket":"b""#), "{text}");
        assert_eq!(serde_json::from_str::<Record>(&text).unwrap(), r);
        // nothing rare: nothing behind the pointer
        let plain = Record {
            rare: Rare::default().into(),
            ..r.clone()
        };
        assert!(plain.rare.is_none() && plain.rare().error_body.is_none());
        let back: Record = serde_json::from_str(&serde_json::to_string(&plain).unwrap()).unwrap();
        assert!(back.rare.is_none());
    }

    #[test]
    fn a_summary_says_which_scenario_answered_in_the_stores_place() {
        let mut r = Record {
            id: 7,
            kind: "PutObject".into(),
            ..Default::default()
        };
        let plain = serde_json::to_value(r.summary()).unwrap();
        assert!(plain.get("injected").is_none(), "{plain}");
        r.rare_mut().injected = Some("slowdown".into());
        let marked = serde_json::to_value(r.summary()).unwrap();
        assert_eq!(marked["injected"], "slowdown");
        // a row without the mark reads as the store's own answer
        let old: RecordSummary = serde_json::from_value(plain).unwrap();
        assert_eq!(old.injected, None);
    }
}
