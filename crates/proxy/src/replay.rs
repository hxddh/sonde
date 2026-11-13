//! Replaying a recorded session on another endpoint, and comparing the two.
//!
//! A session does not keep object data, and the identifiers an endpoint hands
//! out — upload ids, ETags, continuation tokens — differ from one endpoint to
//! the next. Replay therefore re-enacts each request rather than re-sending
//! bytes:
//!
//! - payloads are deterministic synthetic bytes of the recorded length;
//! - control documents (DeleteObjects, tagging, …) are the recorded ones,
//!   with their keys moved under the replay prefix;
//! - upload ids, part ETags, ETags in conditional headers and continuation
//!   tokens are mapped from the original to what the replay endpoint returned;
//! - `CompleteMultipartUpload` is rebuilt from the replayed parts.
//!
//! Every key is moved under a prefix (`replay-<time>/` by default), so the
//! replay never touches existing objects; bucket-level changes are skipped.

use std::collections::{BTreeMap, HashMap, VecDeque};

use bytes::Bytes;
use reqlog::Record;
use s3raw::{Body, S3Client, S3Request, S3Response};
use serde::Serialize;

use crate::summary::is_miss;

/// How to replay.
#[derive(Debug, Clone, Default)]
pub struct ReplayOptions {
    /// Prepended to every key and listing prefix ("" replays keys as they were).
    pub prefix: String,
    /// Bucket renames (original → target).
    pub buckets: Vec<(String, String)>,
}

impl ReplayOptions {
    fn bucket(&self, b: &str) -> String {
        self.buckets
            .iter()
            .find(|(from, _)| from == b)
            .map(|(_, to)| to.clone())
            .unwrap_or_else(|| b.to_string())
    }
}

/// What happened to one original request.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Replayed {
    pub original: u64,
    /// The replay's record id in the client's log, when it was sent.
    pub replay: Option<u64>,
    /// Why it was not sent.
    pub skipped: Option<String>,
}

/// The tag every replayed record carries in `job`: `replay:<original id>`.
pub fn replay_tag(original: u64) -> String {
    format!("replay:{original}")
}

/// The original id a replayed record stands for.
pub fn original_of(r: &Record) -> Option<u64> {
    r.job.as_deref()?.strip_prefix("replay:")?.parse().ok()
}

/// Operations a replay leaves out when it is confined to a prefix: they would
/// change the bucket itself.
pub fn changes_bucket(op: &str) -> bool {
    op == "DeleteBucket"
        || (op.starts_with("PutBucket") || op.starts_with("DeleteBucket"))
        || op == "PutPublicAccessBlock"
        || op == "DeletePublicAccessBlock"
        || op == "PutObjectLockConfiguration"
}

/// Whether a record would be replayed (for planning, before anything is sent).
pub fn plan(records: &[Record], opts: &ReplayOptions) -> Plan {
    let mut p = Plan::default();
    for r in records {
        match skip_reason(r, opts) {
            Some(why) => *p.skipped.entry(why).or_default() += 1,
            None => {
                p.requests += 1;
                match r.method.as_str() {
                    "DELETE" => p.deletes += 1,
                    "POST" if r.operation == "DeleteObjects" => p.deletes += 1,
                    "PUT" | "POST" => p.writes += 1,
                    _ => {}
                }
            }
        }
    }
    p
}

/// What a replay will send.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub requests: usize,
    pub writes: usize,
    pub deletes: usize,
    /// Left out, by reason.
    pub skipped: BTreeMap<String, usize>,
}

fn refused(r: &Record) -> bool {
    r.response_headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("x-sonde-proxy"))
}

/// The operation a record's own method, place and query name today.
fn named(r: &Record) -> String {
    let headers: Vec<(String, String)> = r
        .request_headers
        .iter()
        .map(|h| (h.name.to_string(), h.value.to_string()))
        .collect();
    crate::op::classify(
        &r.method,
        r.bucket.as_deref(),
        r.key.as_deref(),
        &r.query,
        &headers,
    )
}

fn skip_reason(r: &Record, opts: &ReplayOptions) -> Option<String> {
    if refused(r) {
        return Some("refused by sonde when recorded".into());
    }
    if crate::trial::faulted(r) {
        return Some("failed by sonde on purpose when recorded (a trial)".into());
    }
    if r.operation == "Unknown" {
        return Some("not an S3 operation sonde knows".into());
    }
    // by what the request is, not only by the name the session stored: a
    // session recorded before 13.0 calls `PUT /bucket?inventory` CreateBucket
    if !opts.prefix.is_empty() && (changes_bucket(&r.operation) || changes_bucket(&named(r))) {
        return Some("changes the bucket itself".into());
    }
    if r.query.iter().any(|(k, _)| k == "versionId") {
        return Some("names a version of the original endpoint".into());
    }
    // a copy of a version: its id is the original endpoint's
    if r.request_headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("x-amz-copy-source") && h.value.contains("versionId="))
    {
        return Some("copies a version of the original endpoint".into());
    }
    // sessions before 7.0 have no flag: a cut document no longer ends its root element
    if r.request_body_truncated
        || r.request_body.as_ref().is_some_and(|b| {
            b.len() + 1024 >= reqlog::MAX_REQUEST_BODY && !b.trim_end().ends_with('>')
        })
    {
        return Some("its body was too large to keep when recorded".into());
    }
    if reqlog::is_control_operation(&r.operation)
        && r.operation != "CompleteMultipartUpload"
        && r.operation != "CreateBucket"
        && r.request_bytes.unwrap_or(0) > 0
        && r.request_body.is_none()
    {
        return Some("its body was not recorded (an older session)".into());
    }
    None
}

/// The text inside every `<name>…</name>`.
fn tags<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        let Some(j) = after.find(&close) else { break };
        out.push(&after[..j]);
        rest = &after[j + close.len()..];
    }
    out
}

/// Deterministic bytes of length `n` (the same key gives the same bytes).
pub fn synthetic(n: u64, seed: &str) -> Bytes {
    let mut x: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes() {
        x = (x ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    let mut v = Vec::with_capacity(n as usize);
    while (v.len() as u64) < n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.extend_from_slice(&x.to_le_bytes());
    }
    v.truncate(n as usize);
    Bytes::from(v)
}

/// Request headers replay keeps: what changes the meaning of a request.
/// Signatures, lengths, checksums of the original data and SDK chatter go.
fn keep_header(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("if-")
        || n.starts_with("x-amz-meta-")
        || n.starts_with("x-amz-copy-source-if-")
        || matches!(
            n.as_str(),
            "content-type"
                | "range"
                | "cache-control"
                | "content-disposition"
                | "content-language"
                | "expires"
                | "x-amz-storage-class"
                | "x-amz-tagging"
                | "x-amz-copy-source"
                | "x-amz-copy-source-range"
                | "x-amz-metadata-directive"
                | "x-amz-tagging-directive"
                | "x-amz-server-side-encryption"
                | "x-amz-object-lock-mode"
                | "x-amz-object-lock-retain-until-date"
                | "x-amz-object-lock-legal-hold"
                | "x-amz-write-offset-bytes"
        )
}

#[derive(Default)]
struct State {
    /// original upload id → replay upload id
    uploads: HashMap<String, String>,
    /// uploads created in the replay, waiting for the original id to be seen
    pending: HashMap<(String, String), VecDeque<String>>,
    /// replay upload id → part number → ETag
    parts: HashMap<String, BTreeMap<u32, String>>,
    /// original ETag → replay ETag
    etags: HashMap<String, String>,
    /// (bucket, original prefix) → the replay's next continuation token
    tokens: HashMap<(String, String), String>,
    /// listings the replay's store answered to their end: no page follows
    ended: std::collections::HashSet<(String, String)>,
}

fn query<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.query
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn etag_of(headers: &[(String, String)]) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
        .map(|(_, v)| v.clone())
}

impl State {
    fn map_etags(&self, value: &str) -> String {
        value
            .split(',')
            .map(|v| {
                let v = v.trim();
                self.etags.get(v).cloned().unwrap_or_else(|| v.to_string())
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The request to send for `r`, or why it cannot be.
    fn build(&mut self, r: &Record, opts: &ReplayOptions) -> Result<S3Request, String> {
        let method: http::Method = r.method.parse().map_err(|_| "a bad method")?;
        let mut req = S3Request::new(&r.operation, method).no_retry();
        let bucket = r.bucket.as_deref().map(|b| opts.bucket(b));
        if let Some(b) = &bucket {
            req = req.bucket(b);
        }
        if let Some(k) = &r.key {
            req = req.key(&format!("{}{k}", opts.prefix));
        }
        let listing =
            r.operation.starts_with("List") && r.operation != "ListBuckets" && r.key.is_none();
        let mut has_prefix = false;
        for (k, v) in &r.query {
            match k.as_str() {
                "prefix" => {
                    has_prefix = true;
                    req = req.query(k, format!("{}{v}", opts.prefix));
                }
                "start-after" | "marker" | "key-marker" => {
                    req = req.query(k, format!("{}{v}", opts.prefix));
                }
                "continuation-token" => {
                    let key = (
                        bucket.clone().unwrap_or_default(),
                        query(r, "prefix").unwrap_or("").to_string(),
                    );
                    match self.tokens.get(&key) {
                        Some(t) => req = req.query(k, t.clone()),
                        // the store said this listing was over: it holds
                        // fewer keys under the prefix than the original did
                        None if self.ended.contains(&key) => {
                            return Err(
                                "asks for a further page of a listing that had ended on this store"
                                    .into(),
                            );
                        }
                        None => {
                            return Err(
                                "continues a listing whose page before it was not replayed".into(),
                            );
                        }
                    }
                }
                "uploadId" => {
                    let new = match self.uploads.get(v.as_str()) {
                        Some(n) => n.clone(),
                        None => {
                            let key = (
                                bucket.clone().unwrap_or_default(),
                                r.key.as_deref().unwrap_or_default().to_string(),
                            );
                            let n = self
                                .pending
                                .get_mut(&key)
                                .and_then(VecDeque::pop_front)
                                .ok_or("belongs to an upload that was not replayed")?;
                            self.uploads.insert(v.to_string(), n.clone());
                            n
                        }
                    };
                    req = req.query(k, new);
                }
                _ => req = req.query(k, v.clone()),
            }
        }
        if listing && !has_prefix && !opts.prefix.is_empty() {
            req = req.query("prefix", opts.prefix.clone());
        }
        for h in &r.request_headers {
            if !keep_header(&h.name) {
                continue;
            }
            let n = h.name.to_ascii_lowercase();
            let v = if n == "if-match" || n == "if-none-match" || n == "x-amz-copy-source-if-match"
            {
                self.map_etags(&h.value)
            } else if n == "x-amz-copy-source" {
                copy_source(&h.value, opts)
            } else {
                h.value.to_string()
            };
            req = req.header(&n, v);
        }
        // the body
        let seed = format!(
            "{}/{}",
            bucket.as_deref().unwrap_or(""),
            r.key.as_deref().unwrap_or("")
        );
        if r.operation == "CompleteMultipartUpload" {
            let upload = req
                .query
                .iter()
                .find(|(k, _)| k == "uploadId")
                .map(|(_, v)| v.clone())
                .ok_or("no upload id")?;
            let parts = self.parts.get(&upload).cloned().unwrap_or_default();
            let wanted: Vec<u32> = r
                .request_body
                .as_deref()
                .map(|b| {
                    tags(b, "PartNumber")
                        .iter()
                        .filter_map(|n| n.trim().parse().ok())
                        .collect()
                })
                .unwrap_or_default();
            let mut xml = String::from("<CompleteMultipartUpload>");
            for (n, etag) in &parts {
                if wanted.is_empty() || wanted.contains(n) {
                    xml.push_str(&format!(
                        "<Part><PartNumber>{n}</PartNumber><ETag>{etag}</ETag></Part>"
                    ));
                }
            }
            xml.push_str("</CompleteMultipartUpload>");
            req = req.body(Body::from(xml));
        } else if let Some(b) = &r.request_body {
            let body = if r.operation == "DeleteObjects" {
                move_keys(b, &opts.prefix)
            } else {
                b.clone()
            };
            req = req.body(Body::from(body));
        } else if let Some(n) = r.request_bytes.filter(|n| *n > 0) {
            if n > 1 << 30 {
                return Err("a body over 1 GiB".into());
            }
            req = req.body(Body::Bytes(synthetic(n, &seed)));
        }
        Ok(req)
    }

    /// Learns the identifiers a response hands out.
    fn learn(&mut self, r: &Record, req: &S3Request, resp: &S3Response) {
        if !resp.is_success() {
            return;
        }
        let bucket = req.bucket.clone().unwrap_or_default();
        match r.operation.as_str() {
            "CreateMultipartUpload" => {
                if let Some(id) = tags(&resp.text(), "UploadId").first() {
                    self.pending
                        .entry((bucket, r.key.as_deref().unwrap_or_default().to_string()))
                        .or_default()
                        .push_back(id.to_string());
                }
            }
            "UploadPart" | "UploadPartCopy" => {
                let upload = req
                    .query
                    .iter()
                    .find(|(k, _)| k == "uploadId")
                    .map(|(_, v)| v.clone());
                let n = query(r, "partNumber").and_then(|n| n.parse().ok());
                let etag = etag_of(&resp.headers)
                    .or_else(|| tags(&resp.text(), "ETag").first().map(|s| s.to_string()));
                if let (Some(u), Some(n), Some(e)) = (upload, n, etag) {
                    self.parts.entry(u).or_default().insert(n, e);
                }
            }
            "ListObjectsV2" => {
                let key = (bucket, query(r, "prefix").unwrap_or("").to_string());
                match tags(&resp.text(), "NextContinuationToken").first() {
                    Some(t) => {
                        self.ended.remove(&key);
                        self.tokens.insert(key, t.to_string());
                    }
                    None => {
                        self.tokens.remove(&key);
                        self.ended.insert(key);
                    }
                }
            }
            _ => {}
        }
        let original = r
            .response_headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("etag"))
            .map(|h| h.value.to_string());
        if let (Some(o), Some(n)) = (original, etag_of(&resp.headers)) {
            self.etags.insert(o, n);
        }
    }
}

fn copy_source(v: &str, opts: &ReplayOptions) -> String {
    let decoded = percent_encoding::percent_decode_str(v)
        .decode_utf8_lossy()
        .into_owned();
    let path = decoded.split('?').next().unwrap_or("");
    let path = path.trim_start_matches('/');
    match path.split_once('/') {
        Some((b, k)) => format!(
            "{}/{}",
            opts.bucket(b),
            s3raw::client::encode_key(&format!("{}{k}", opts.prefix))
        ),
        None => v.to_string(),
    }
}

/// Moves every `<Key>` of a DeleteObjects document under `prefix` and drops
/// version ids (they belong to the original endpoint).
fn move_keys(body: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(body.len() + 64);
    let mut rest = body;
    while let Some(i) = rest.find("<Key>") {
        out.push_str(&rest[..i + 5]);
        out.push_str(prefix);
        rest = &rest[i + 5..];
    }
    out.push_str(rest);
    // drop <VersionId>…</VersionId>
    let mut clean = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(i) = rest.find("<VersionId>") {
        clean.push_str(&rest[..i]);
        match rest[i..].find("</VersionId>") {
            Some(j) => rest = &rest[i + j + "</VersionId>".len()..],
            None => {
                rest = "";
            }
        }
    }
    clean.push_str(rest);
    clean
}

/// Replays `records` in order with `client` (each replayed record is tagged
/// [`replay_tag`]). `progress` is called after each request with (done, total).
pub async fn replay(
    client: &S3Client,
    records: &[Record],
    opts: &ReplayOptions,
    mut progress: impl FnMut(usize, usize),
) -> Vec<Replayed> {
    let mut sorted: Vec<&Record> = records.iter().collect();
    sorted.sort_by_key(|r| (r.time, r.id));
    let mut state = State::default();
    let mut out = Vec::with_capacity(sorted.len());
    let total = sorted.len();
    for (i, r) in sorted.into_iter().enumerate() {
        if let Some(why) = skip_reason(r, opts) {
            out.push(Replayed {
                original: r.id,
                replay: None,
                skipped: Some(why),
            });
            progress(i + 1, total);
            continue;
        }
        let req = match state.build(r, opts) {
            Ok(req) => req,
            Err(why) => {
                out.push(Replayed {
                    original: r.id,
                    replay: None,
                    skipped: Some(why),
                });
                progress(i + 1, total);
                continue;
            }
        };
        let tagged = client.for_job(Some(replay_tag(r.id)), None);
        let sent = tagged.send_raw(req.clone()).await;
        let id = match &sent {
            Ok(resp) => {
                state.learn(r, &req, resp);
                Some(resp.record_id)
            }
            Err(e) => e.record_id,
        };
        out.push(Replayed {
            original: r.id,
            replay: id,
            skipped: None,
        });
        progress(i + 1, total);
    }
    out
}

/// A request's outcome, for comparison: its status and S3 error code.
pub fn outcome(r: &Record) -> String {
    if r.status.is_none() {
        return "no response".into();
    }
    let status = r.status.unwrap_or(0);
    let code = r
        .error_body
        .as_deref()
        .and_then(|b| tags(b, "Code").first().map(|c| c.to_string()));
    match code {
        Some(c) if status >= 300 => format!("{status} {c}"),
        _ => status.to_string(),
    }
}

/// What a request answered well carried, to compare beside its outcome: a
/// listing's keys, a read's bytes. `None` when there is nothing to compare.
pub fn carried(r: &Record) -> Option<String> {
    if !r.status.is_some_and(|s| (200..300).contains(&s)) || r.transport_error.is_some() {
        return None;
    }
    if let Some(n) = r.header(reqlog::KEYS_HEADER) {
        return Some(format!("{n} key{}", if n == "1" { "" } else { "s" }));
    }
    (r.method == "GET" && r.key.is_some())
        .then_some(r.response_bytes)
        .flatten()
        .map(|n| format!("{n} B"))
}

/// One replayed answer, as [`compare`] reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// [`outcome`].
    pub outcome: String,
    /// A lookup that found nothing.
    pub miss: bool,
    /// [`carried`].
    pub carried: Option<String>,
}

/// Requests that behaved differently, grouped.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Difference {
    pub operation: String,
    pub original: String,
    pub replay: String,
    pub count: usize,
    /// One of the keys, for a look.
    pub example: String,
    /// What the requests asked for that may explain it (conditional headers, …).
    pub conditions: Vec<String>,
}

/// The two sessions side by side.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub compared: usize,
    pub same: usize,
    /// Different only because the target already held something (a bucket).
    pub environment: usize,
    pub differences: Vec<Difference>,
    /// Original requests with no replay, by reason.
    pub unmatched: BTreeMap<String, usize>,
}

impl Comparison {
    pub fn differ(&self) -> usize {
        self.differences.iter().map(|d| d.count).sum()
    }
}

fn conditions(r: &Record) -> Vec<String> {
    r.request_headers
        .iter()
        .filter(|h| {
            let n = h.name.to_ascii_lowercase();
            n.starts_with("if-") || n.starts_with("x-amz-copy-source-if-") || n == "range"
        })
        .map(|h| format!("{}: {}", h.name.to_ascii_lowercase(), h.value))
        .collect()
}

/// Whether these records are a replay's: any of them names the request it
/// repeats. Two ordinary runs have nothing to pair, and [`compare`] of them
/// compares nothing.
pub fn is_replay(records: &[Record]) -> bool {
    records.iter().any(|r| original_of(r).is_some())
}

/// Compares an original session with its replay (records tagged by
/// [`replay_tag`]). `replayed` gives the reasons for requests not sent.
pub fn compare(original: &[Record], replay: &[Record], replayed: &[Replayed]) -> Comparison {
    compare_answers(original, &answers(replay), replayed)
}

/// What [`compare`] reads of a replay: each [`Answer`] by the request it
/// repeats. A few bytes a request: a replay is read, kept as this, and let
/// go before its session is read.
pub fn answers(replay: &[Record]) -> HashMap<u64, Answer> {
    replay
        .iter()
        .filter_map(|r| {
            original_of(r).map(|o| {
                (
                    o,
                    Answer {
                        outcome: outcome(r),
                        miss: is_miss(r),
                        carried: carried(r),
                    },
                )
            })
        })
        .collect()
}

/// [`compare`] with the replay's [`answers`].
pub fn compare_answers(
    original: &[Record],
    by_original: &HashMap<u64, Answer>,
    replayed: &[Replayed],
) -> Comparison {
    let mut c = Comparison::default();
    let mut groups: BTreeMap<(String, String, String), Difference> = BTreeMap::new();
    for o in original {
        let Some(r) = by_original.get(&o.id) else {
            let why = replayed
                .iter()
                .find(|x| x.original == o.id)
                .and_then(|x| x.skipped.clone())
                .unwrap_or_else(|| "not replayed".into());
            *c.unmatched.entry(why).or_default() += 1;
            continue;
        };
        c.compared += 1;
        let (mut a, mut b) = (outcome(o), r.outcome.clone());
        if is_miss(o) && r.miss {
            c.same += 1;
            continue;
        }
        // the same status, and what came with it: a listing of 5 keys is
        // not one of none
        if a == b {
            match (carried(o), &r.carried) {
                (Some(x), Some(y)) if x != *y => {
                    a = format!("{a} · {x}");
                    b = format!("{b} · {y}");
                }
                _ => {
                    c.same += 1;
                    continue;
                }
            }
        }
        if o.operation == "CreateBucket"
            && (b.ends_with("BucketAlreadyOwnedByYou") || b.ends_with("BucketAlreadyExists"))
        {
            c.environment += 1;
            continue;
        }
        let conds = conditions(o);
        let key = (o.operation.to_string(), a.clone(), b.clone());
        let g = groups.entry(key).or_insert_with(|| Difference {
            operation: o.operation.to_string(),
            original: a.clone(),
            replay: b.clone(),
            count: 0,
            example: match (&o.bucket, &o.key) {
                (Some(bk), Some(k)) => format!("{bk}/{k}"),
                (Some(bk), None) => format!("{bk}/"),
                _ => "/".into(),
            },
            conditions: Vec::new(),
        });
        g.count += 1;
        for cnd in conds {
            if !g.conditions.contains(&cnd) {
                g.conditions.push(cnd);
            }
        }
    }
    c.differences = groups.into_values().collect();
    c.differences
        .sort_by(|a, b| b.count.cmp(&a.count).then(a.operation.cmp(&b.operation)));
    c
}

/// The buckets a replay writes to (after renames), each once.
pub fn buckets_of(records: &[Record], opts: &ReplayOptions) -> Vec<String> {
    let mut b: Vec<String> = records
        .iter()
        .filter_map(|r| r.bucket.as_deref().map(|x| opts.bucket(x)))
        .collect();
    b.sort();
    b.dedup();
    b
}

/// The prefix a replay starting now writes under: the time, and four hex
/// digits so two replays in the same second do not share it.
pub fn default_prefix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let salt = (nanos ^ (std::process::id().wrapping_mul(2_654_435_761))) & 0xffff;
    format!(
        "replay-{}-{salt:04x}/",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    )
}

/// A replay's prefix as it is written under: without a leading `/`, ending in
/// `/`, so it names a folder of its own and not every key that begins with
/// the same letters. An empty prefix is refused: replaying in place is asked
/// for explicitly (`ReplayOptions { prefix: "" }` from `--in-place`).
pub fn normalize_prefix(p: &str) -> Result<String, String> {
    let p = p.trim().trim_start_matches('/');
    if p.is_empty() {
        return Err(
            "a replay needs a prefix of its own to write under (or an explicit in-place replay)"
                .into(),
        );
    }
    Ok(if p.ends_with('/') {
        p.to_string()
    } else {
        format!("{p}/")
    })
}

/// The replay prefix a key or listing prefix starts with, if any:
/// `replay-<date>-<time>/` or `replay-<date>-<time>-<salt>/`.
fn replay_prefix_len(k: &str) -> Option<usize> {
    let rest = k.strip_prefix("replay-")?;
    let end = rest.find('/')?;
    let id = &rest[..end];
    let ok = id.len() >= 15
        && id.as_bytes()[..8].iter().all(u8::is_ascii_digit)
        && id.as_bytes()[8] == b'-'
        && id.as_bytes()[9..15].iter().all(u8::is_ascii_digit)
        && (id.len() == 15
            || (id.as_bytes()[15] == b'-' && id[16..].bytes().all(|c| c.is_ascii_hexdigit())));
    ok.then_some("replay-".len() + end + 1)
}

/// A replay done: its session file, what it was, and how it compares.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayRun {
    pub path: std::path::PathBuf,
    pub plan: Plan,
    pub comparison: Comparison,
    /// Objects deleted afterwards (under the prefix).
    pub removed: usize,
    /// Versions and delete markers deleted with them, where the bucket
    /// keeps versions.
    #[serde(default)]
    pub removed_versions: usize,
    /// What is still under the prefix after the cleanup, each thing counted
    /// once: not 0 when the store would not delete them. Where the store
    /// lists versions it is its versions and delete markers (an object is
    /// its newest version); elsewhere its objects.
    #[serde(default)]
    pub remaining: usize,
    /// Of what remains: objects, versions that are no object's newest, and
    /// delete markers.
    #[serde(default)]
    pub remaining_objects: usize,
    #[serde(default)]
    pub remaining_versions: usize,
    #[serde(default)]
    pub remaining_markers: usize,
    /// Multipart uploads under the prefix the store would not abort: their
    /// parts stay billed.
    #[serde(default)]
    pub unaborted_uploads: usize,
}

/// Every version and delete marker under `prefix` (key, version id,
/// whether it is a marker): none where the store keeps no versions or does
/// not list them.
async fn versions_under(
    base: &S3Client,
    bucket: &str,
    prefix: &str,
) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    let (mut key, mut version) = (None::<String>, None::<String>);
    // (a page at a time; a store that answers the same page for ever is left)
    for _ in 0..10_000 {
        let Ok(page) = base
            .list_object_versions(
                bucket,
                Some(prefix),
                key.as_deref(),
                version.as_deref(),
                None,
            )
            .await
        else {
            break;
        };
        out.extend(
            page.version
                .iter()
                .map(|v| (v.key.clone(), v.version_id.clone(), false))
                .chain(
                    page.delete_marker
                        .iter()
                        .map(|m| (m.key.clone(), m.version_id.clone(), true)),
                )
                .filter_map(|(k, v, marker)| Some((k, v?, marker))),
        );
        if !page.is_truncated || page.next_key_marker.is_none() {
            break;
        }
        (key, version) = (page.next_key_marker, page.next_version_id_marker);
    }
    out
}

/// Deletes `keys` (each with its version, if one is named) and says how
/// many the store deleted: by what its answer says of each key, not by how
/// many were asked for. In batches; a store without `DeleteObjects` (the
/// batch is answered 501 or 405) is asked key by key from then on, and a
/// batch that fails some other way is asked again key by key.
async fn delete_keys(
    base: &S3Client,
    bucket: &str,
    keys: &[(String, Option<String>)],
    batches: &mut bool,
) -> usize {
    let mut removed = 0;
    for chunk in keys.chunks(1000) {
        if *batches {
            match base.delete_objects(bucket, chunk, true).await {
                // a quiet answer names only the keys it could not delete
                Ok(answer) => {
                    removed += chunk.len().saturating_sub(answer.error.len());
                    continue;
                }
                Err(e) => *batches = !matches!(e.status, Some(501 | 405)),
            }
        }
        for (key, version) in chunk {
            if base
                .delete_object(bucket, key, version.as_deref())
                .await
                .is_ok()
            {
                removed += 1;
            }
        }
    }
    removed
}

/// Replays `original` with `base`'s endpoint into a new session file at
/// `out`, deletes what it wrote under the prefix (unless `keep`, or when it
/// replays in place), and compares the answers.
pub async fn replay_to_file(
    base: &S3Client,
    original: &crate::session::Session,
    opts: &ReplayOptions,
    out: &std::path::Path,
    keep: bool,
    progress: impl FnMut(usize, usize),
) -> Result<ReplayRun, String> {
    use std::sync::Arc;
    let plan = plan(&original.records, opts);
    let cfg = base.config();
    let name = original
        .header
        .command
        .clone()
        .unwrap_or_else(|| original.header.endpoint.clone());
    let header = crate::session::SessionHeader::new(
        &cfg.endpoint_id,
        cfg.endpoint.as_str(),
        &cfg.region,
        Some(format!("replay of {name}")),
    );
    let writer =
        Arc::new(crate::session::SessionWriter::create(out, &header).map_err(|e| e.to_string())?);
    // cleanup deletes everything under the prefix: it must hold nothing before
    if !opts.prefix.is_empty() && !keep {
        for b in buckets_of(&original.records, opts) {
            let why = match base.list_all(&b, &opts.prefix, 1).await {
                Ok(found) if found.is_empty() => continue,
                Ok(_) => format!(
                    "{b}/{} already holds objects: a replay writes under a prefix of its own and deletes what is there afterwards; choose another prefix",
                    opts.prefix
                ),
                // a bucket that is not there holds nothing (the replay may
                // create it); any other answer leaves the prefix unknown
                Err(e) if e.status == Some(404) => continue,
                Err(e) => format!(
                    "could not check that {b}/{} is empty ({e}): a replay deletes what is under its prefix afterwards, so nothing was replayed",
                    opts.prefix
                ),
            };
            let _ = std::fs::remove_file(out);
            return Err(why);
        }
    }
    let log = reqlog::RequestLog::new(4096);
    log.set_sink(Some(writer.clone()));
    let client = base.with_log(log).with_retry(s3raw::RetryPolicy::none());
    let done = replay(&client, &original.records, opts, progress).await;
    writer.flush().map_err(|e| e.to_string())?;
    let (mut removed, mut removed_versions) = (0usize, 0usize);
    let (mut left_objects, mut left_versions, mut left_markers) = (0usize, 0usize, 0usize);
    let mut unaborted_uploads = 0usize;
    // whether the store takes DeleteObjects: found out once
    let mut batches = true;
    if !opts.prefix.is_empty() && !keep {
        for b in buckets_of(&original.records, opts) {
            if let Ok(objects) = base.list_all(&b, &opts.prefix, usize::MAX).await {
                let keys: Vec<(String, Option<String>)> =
                    objects.iter().map(|o| (o.key.clone(), None)).collect();
                removed += delete_keys(base, &b, &keys, &mut batches).await;
            }
            // a bucket that keeps versions kept every one of them, and a
            // delete marker for each delete above: they go by their ids
            // (a bucket without versions lists none once its objects are gone)
            let versions: Vec<(String, Option<String>)> = versions_under(base, &b, &opts.prefix)
                .await
                .into_iter()
                .map(|(k, v, _)| (k, Some(v)))
                .collect();
            removed_versions += delete_keys(base, &b, &versions, &mut batches).await;
            // and what is there now is what was left, each thing once: an
            // object the store also lists as a version is that version
            let objects = base
                .list_all(&b, &opts.prefix, usize::MAX)
                .await
                .map_or(0, |o| o.len());
            let listed = versions_under(base, &b, &opts.prefix).await;
            let markers = listed.iter().filter(|(_, _, marker)| *marker).count();
            left_objects += objects;
            left_markers += markers;
            left_versions += (listed.len() - markers).saturating_sub(objects);
            if let Ok(up) = base
                .list_multipart_uploads(&b, Some(&opts.prefix), None, None)
                .await
            {
                for u in up.upload {
                    let _ = base.abort_multipart_upload(&b, &u.key, &u.upload_id).await;
                }
            }
            // an upload still listed would not abort: its parts are billed
            if let Ok(up) = base
                .list_multipart_uploads(&b, Some(&opts.prefix), None, None)
                .await
            {
                unaborted_uploads += up.upload.len();
            }
        }
    }
    let replayed = crate::session::Session::open(out)?;
    let comparison = compare(&original.records, &replayed.records, &done);
    Ok(ReplayRun {
        path: out.to_path_buf(),
        plan,
        comparison,
        removed,
        removed_versions,
        remaining: left_objects + left_versions + left_markers,
        remaining_objects: left_objects,
        remaining_versions: left_versions,
        remaining_markers: left_markers,
        unaborted_uploads,
    })
}

/// A session as text to compare with another: one line per request, in
/// order — operation, target (a replay's prefix taken off), the conditions
/// it carried, and its outcome.
pub fn transcript(records: &[Record]) -> String {
    let mut sorted: Vec<&Record> = records.iter().collect();
    sorted.sort_by_key(|r| (r.time, r.id));
    // a replay's own prefix taken off, a root listing's too (its prefix is the replay prefix alone)
    let strip = |k: &str| -> String {
        match replay_prefix_len(k) {
            Some(n) => k[n..].to_string(),
            None => k.to_string(),
        }
    };
    let mut out = String::new();
    for r in sorted {
        let target = match (&r.bucket, &r.key) {
            (Some(b), Some(k)) => format!("{b}/{}", strip(k)),
            (Some(b), None) => {
                let p = r
                    .query
                    .iter()
                    .find(|(k, _)| k == "prefix")
                    .map(|(_, v)| strip(v))
                    .unwrap_or_default();
                format!("{b}/{p}")
            }
            _ => "/".into(),
        };
        let conds = conditions(r);
        let outcome = if crate::summary::is_miss(r) {
            "404".to_string()
        } else {
            outcome(r)
        };
        out.push_str(&r.operation);
        out.push(' ');
        out.push_str(&target);
        if !conds.is_empty() {
            out.push_str(&format!(" [{}]", conds.join(", ")));
        }
        out.push_str(&format!(" → {outcome}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_is_a_folder_of_its_own() {
        // "test" would have made cleanup delete testing/… and tests.csv
        assert_eq!(normalize_prefix("test").unwrap(), "test/");
        assert_eq!(normalize_prefix(" /test/ ").unwrap(), "test/");
        assert!(normalize_prefix("").is_err());
        assert!(normalize_prefix("  /").is_err());
        let d = default_prefix();
        assert!(
            d.ends_with('/') && replay_prefix_len(&d) == Some(d.len()),
            "{d}"
        );
        assert_ne!(default_prefix(), "replay-");
    }

    #[test]
    fn replay_prefixes_come_off_root_listings_too() {
        // old prefixes (exactly 23 bytes for a root listing) and new ones
        assert_eq!(replay_prefix_len("replay-20261107-015048/"), Some(23));
        assert_eq!(replay_prefix_len("replay-20261107-015048/db/x"), Some(23));
        assert_eq!(
            replay_prefix_len("replay-20261107-015048-0a1f/db/x"),
            Some(28)
        );
        assert_eq!(replay_prefix_len("replay-notes/x"), None);
        assert_eq!(replay_prefix_len("db/replay-20261107-015048/x"), None);
    }

    #[test]
    fn a_listing_or_a_read_that_carried_another_thing_differs() {
        let rec = |id: u64, op: &str, method: &str, keys: Option<&str>, bytes: u64| {
            let mut r = Record {
                id,
                operation: op.into(),
                method: method.into(),
                bucket: Some("b".into()),
                key: (method == "GET" && keys.is_none()).then(|| "k".into()),
                status: Some(200),
                response_bytes: Some(bytes),
                ..Default::default()
            };
            if let Some(n) = keys {
                r.response_headers
                    .push(reqlog::Header::new(reqlog::KEYS_HEADER, n));
            }
            r
        };
        let original = [
            rec(1, "ListObjectsV2", "GET", Some("5"), 900),
            rec(2, "GetObject", "GET", None, 4096),
            rec(3, "GetObject", "GET", None, 10),
            rec(4, "ListObjectsV2", "GET", Some("1"), 300),
        ];
        let mut replay = [
            rec(11, "ListObjectsV2", "GET", Some("0"), 200),
            rec(12, "GetObject", "GET", None, 0),
            rec(13, "GetObject", "GET", None, 10),
            // a prefix makes the body longer: keys are what is compared
            rec(14, "ListObjectsV2", "GET", Some("1"), 340),
        ];
        for (r, o) in replay.iter_mut().zip(1..) {
            r.job = Some(replay_tag(o));
        }
        let c = compare(&original, &replay, &[]);
        assert_eq!((c.compared, c.same, c.differ()), (4, 2, 2), "{c:#?}");
        let said: Vec<(&str, &str)> = c
            .differences
            .iter()
            .map(|d| (d.original.as_str(), d.replay.as_str()))
            .collect();
        assert!(said.contains(&("200 · 5 keys", "200 · 0 keys")), "{said:?}");
        assert!(said.contains(&("200 · 4096 B", "200 · 0 B")), "{said:?}");
    }

    #[test]
    fn helpers() {
        assert_eq!(
            tags("<A><UploadId>u-1</UploadId></A>", "UploadId"),
            vec!["u-1"]
        );
        assert_eq!(synthetic(10, "k"), synthetic(10, "k"));
        assert_ne!(synthetic(10, "k"), synthetic(10, "j"));
        assert_eq!(synthetic(1001, "k").len(), 1001);
        assert_eq!(
            move_keys(
                "<Delete><Object><Key>a/b</Key><VersionId>v1</VersionId></Object><Object><Key>c</Key></Object></Delete>",
                "replay-1/"
            ),
            "<Delete><Object><Key>replay-1/a/b</Key></Object><Object><Key>replay-1/c</Key></Object></Delete>"
        );
        let opts = ReplayOptions {
            prefix: "p/".into(),
            buckets: vec![("a".into(), "b".into())],
        };
        assert_eq!(copy_source("/a/x%20y?versionId=3", &opts), "b/p/x%20y");
        assert!(changes_bucket("PutBucketLifecycleConfiguration"));
        assert!(changes_bucket("DeleteBucket"));
        assert!(!changes_bucket("PutObject"));
    }

    #[test]
    fn a_page_that_cannot_be_asked_for_says_which_of_the_two_reasons() {
        let page = |token: Option<&str>| {
            let mut r = Record {
                method: "GET".into(),
                operation: "ListObjectsV2".into(),
                bucket: Some("b".into()),
                query: vec![("prefix".into(), "p/".into())],
                status: Some(200),
                ..Default::default()
            };
            if let Some(t) = token {
                r.query.push(("continuation-token".into(), t.into()));
            }
            r
        };
        let opts = ReplayOptions::default();
        let mut state = State::default();
        // its first page was never replayed
        assert_eq!(
            state.build(&page(Some("t1")), &opts).unwrap_err(),
            "continues a listing whose page before it was not replayed"
        );
        // the first page was, and the store ended the listing there
        let first = page(None);
        let req = state.build(&first, &opts).unwrap();
        let answer = S3Response {
            status: 200,
            headers: Vec::new(),
            body: Bytes::from(
                "<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>",
            ),
            record_id: 1,
            request_id: None,
            host_id: None,
            latency: Default::default(),
            retries: 0,
        };
        state.learn(&first, &req, &answer);
        assert_eq!(
            state.build(&page(Some("t1")), &opts).unwrap_err(),
            "asks for a further page of a listing that had ended on this store"
        );
    }

    #[test]
    fn transcripts_take_the_replay_prefix_off() {
        let mut a = Record {
            id: 1,
            method: "PUT".into(),
            operation: "PutObject".into(),
            bucket: Some("b".into()),
            key: Some("db/x".into()),
            status: Some(200),
            ..Default::default()
        };
        a.request_headers = vec![reqlog::Header::new("If-None-Match", "*")];
        let mut b = a.clone();
        b.key = Some("replay-20261107-015048/db/x".into());
        b.status = Some(412);
        b.error_body = Some("<Error><Code>PreconditionFailed</Code></Error>".into());
        assert_eq!(
            transcript(&[a]),
            "PutObject b/db/x [if-none-match: *] → 200\n"
        );
        assert_eq!(
            transcript(&[b]),
            "PutObject b/db/x [if-none-match: *] → 412 PreconditionFailed\n"
        );
    }
}
