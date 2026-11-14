//! Traffic sonde did not record, read as a session: S3 server access logs,
//! CloudTrail data events and HAR files. Production traffic gets the same
//! operations, findings and charts as a recording, with no proxy in front of
//! the program. What a source does not carry stays empty: CloudTrail has no
//! latency, an access log no request headers.

use chrono::{DateTime, Utc};
use reqlog::Record;
use serde_json::Value;

use crate::session::{Session, SessionHeader};

/// Where imported traffic came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    AccessLog,
    CloudTrail,
    Har,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::AccessLog => "S3 server access log",
            Source::CloudTrail => "CloudTrail",
            Source::Har => "HAR",
        }
    }

    /// One of them, as a sentence says it: "from an S3 server access log".
    pub fn a(self) -> &'static str {
        match self {
            Source::AccessLog => "an S3 server access log",
            Source::CloudTrail => "a CloudTrail file",
            Source::Har => "a HAR",
        }
    }
}

/// What a text is, by its first characters: a session needs no import.
pub fn detect(text: &str) -> Option<Source> {
    let t = text.trim_start();
    if t.starts_with('{') {
        let head: String = t.chars().take(4096).collect();
        if head.contains("\"format\":\"sonde-session\"")
            || head.contains("\"format\": \"sonde-session\"")
        {
            return None;
        }
        if head.contains("\"Records\"") || head.contains("\"eventSource\"") {
            return Some(Source::CloudTrail);
        }
        if head.contains("\"log\"") && head.contains("\"entries\"") || head.contains("\"entries\"")
        {
            return Some(Source::Har);
        }
        return None;
    }
    // owner bucket [time …
    let first = t.lines().next().unwrap_or("");
    let mut parts = first.splitn(4, ' ');
    let (_, _, time) = (parts.next(), parts.next(), parts.next());
    time.filter(|s| s.starts_with('['))
        .map(|_| Source::AccessLog)
}

/// An import: the session, and what of the source was left out of it.
#[derive(Debug, Clone)]
pub struct Imported {
    pub source: Source,
    pub session: Session,
    /// Lines that could not be read (cut off, or not the source's format).
    pub malformed: usize,
    /// Lines that are no request of a program's (a lifecycle expiry, a batch
    /// delete's objects): the store's own work.
    pub not_requests: usize,
    /// A HAR's entries for other hosts (a page, its scripts, another
    /// service): no request to S3, and left out.
    pub other_hosts: usize,
    /// CloudTrail events of other services than S3, left out.
    pub other_services: usize,
}

/// A file's text: gunzipped when it is gzip (as S3 delivers CloudTrail).
pub fn text_of(bytes: &[u8]) -> Result<String, String> {
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Ok(String::from_utf8_lossy(bytes).into_owned());
    }
    let out = gunzip(bytes, GUNZIP_LIMIT).map_err(|e| format!("gzip: {e}"))?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The most a gzip file may hold once read into memory: a few bytes of gzip
/// can say they are any size.
pub const GUNZIP_LIMIT: u64 = 2 << 30;

/// Gzip bytes, read whole; an error past `limit` bytes of content.
pub fn gunzip(bytes: &[u8], limit: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(bytes)
        .take(limit + 1)
        .read_to_end(&mut out)?;
    if out.len() as u64 > limit {
        return Err(std::io::Error::other(format!(
            "it holds more than {} MiB",
            limit >> 20
        )));
    }
    Ok(out)
}

/// Reads `text` as the source it is into a session (records numbered in time
/// order). `name` names it (the endpoint shown for it).
pub fn import(text: &str, name: &str) -> Result<(Source, Session), String> {
    import_counted(text, name).map(|i| (i.source, i.session))
}

/// [`import`], with the count of the lines left out.
pub fn import_counted(text: &str, name: &str) -> Result<Imported, String> {
    let mut parts = Parts::default();
    parts.add(text)?;
    parts.finish(name)
}

/// A session gathered from several files of one source, one text at a time:
/// S3 delivers an hour of access logs as thousands of small files, and only
/// the requests of each are kept once it is read.
#[derive(Default)]
pub struct Parts {
    source: Option<Source>,
    records: Vec<Record>,
    malformed: usize,
    not_requests: usize,
    other_hosts: usize,
    other_services: usize,
    /// The hosts the requests went to, each with how many.
    hosts: std::collections::BTreeMap<String, usize>,
    /// request id → the object a copy read, from the store's own line of
    /// it: the line may be in another file of the folder than the copy's.
    sources: std::collections::HashMap<String, String>,
}

impl Parts {
    /// Reads one file's text. An error leaves what was read before as it
    /// was: the text is no source sonde reads, or another than the first's.
    pub fn add(&mut self, text: &str) -> Result<(), String> {
        if text.trim().is_empty() {
            return Err(format!("{EMPTY}: the file is empty"));
        }
        let source = detect(text).ok_or("not an S3 server access log, CloudTrail file or HAR")?;
        match self.source {
            Some(first) if first != source => {
                return Err(format!("{} among {} files", source.a(), first.name()));
            }
            _ => {}
        }
        let (mut malformed, mut not_requests) = (0, 0);
        let records = match source {
            Source::AccessLog => access_log(
                text,
                &mut malformed,
                &mut not_requests,
                &mut self.sources,
                &mut self.hosts,
            ),
            Source::CloudTrail => cloudtrail(
                text,
                &mut malformed,
                &mut self.other_services,
                &mut self.hosts,
            )?,
            Source::Har => har(text, &mut self.other_hosts, &mut self.hosts)?,
        };
        self.source = Some(source);
        self.records.extend(records);
        self.malformed += malformed;
        self.not_requests += not_requests;
        Ok(())
    }

    /// The session of everything added, its records numbered in time order.
    pub fn finish(self, name: &str) -> Result<Imported, String> {
        let source = self
            .source
            .ok_or("not an S3 server access log, CloudTrail file or HAR")?;
        let mut records = self.records;
        let mut sources = self.sources;
        for r in &mut records {
            if let Some(source) = r.request_id.as_ref().and_then(|id| sources.remove(id)) {
                r.request_headers
                    .push(reqlog::Header::new("x-amz-copy-source", source));
            }
        }
        if records.is_empty() {
            return Err(format!("{EMPTY}: no S3 requests in this {}", source.name()));
        }
        // (stable: requests of one moment stay in the log's order)
        records.sort_by_key(|r| r.time);
        let whole_seconds = spread_seconds(&mut records);
        // where the requests went, when the source says: the store's host
        // (the file's name otherwise, which is all there is)
        let endpoint = self
            .hosts
            .iter()
            .max_by_key(|(_, n)| **n)
            .map(|(h, _)| h.clone())
            .unwrap_or_else(|| name.to_string());
        for (i, r) in records.iter_mut().enumerate() {
            r.id = i as u64 + 1;
            r.endpoint = endpoint.as_str().into();
        }
        let mut header = SessionHeader::new(
            &endpoint,
            "",
            "",
            Some(format!("imported from {}", source.name())),
        );
        header.created = records[0].time;
        header.whole_seconds = whole_seconds;
        Ok(Imported {
            source,
            session: Session {
                header,
                records,
                logs: Vec::new(),
            },
            malformed: self.malformed,
            not_requests: self.not_requests,
            other_hosts: self.other_hosts,
            other_services: self.other_services,
        })
    }
}

/// A log whose times are whole seconds says a second's requests all began
/// at once: they are spread evenly over their second, in the log's order
/// (records sorted by time, the order kept within one). Returns whether the
/// times were to the second (two requests or more, none with a fraction).
fn spread_seconds(records: &mut [Record]) -> bool {
    let whole = records.len() >= 2 && records.iter().all(|r| r.time.timestamp_subsec_nanos() == 0);
    if !whole {
        return false;
    }
    let mut i = 0;
    while i < records.len() {
        let second = records[i].time;
        let n = records[i..].iter().take_while(|r| r.time == second).count();
        for (k, r) in records[i..i + n].iter_mut().enumerate() {
            r.time = second + chrono::Duration::microseconds((k as i64 * 1_000_000) / n as i64);
        }
        i += n;
    }
    true
}

/// A store's host as an endpoint is named: without the bucket a
/// virtual-hosted request puts before it.
fn store_host(host: &str, bucket: Option<&str>) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host == "-" {
        return None;
    }
    let host = match bucket {
        Some(b) if !b.is_empty() => host
            .strip_prefix(&format!("{}.", b.to_ascii_lowercase()))
            .unwrap_or(&host)
            .to_string(),
        _ => host,
    };
    Some(host)
}

/// A folder imported as one session, and the files that were no part of it.
#[derive(Debug, Clone)]
pub struct ImportedDir {
    pub imported: Imported,
    /// Files read into the session.
    pub files: usize,
    /// Files left out, each with why (relative to the folder).
    pub left_out: Vec<(String, String)>,
}

/// Every file under `dir` (folders within too, as CloudTrail delivers), in
/// the order of their names, gzip or plain, as one session. A file that is
/// not of the first file's source is left out and named; hidden files are
/// passed over.
pub fn import_dir(dir: &std::path::Path, name: &str) -> Result<ImportedDir, String> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            // what the folder holds, not what it points at: a link to the
            // folder itself would be walked for ever, and one out of it
            // would import what was never put here
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            } else if kind.is_dir() {
                walk(&path, out)?;
            } else {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, &mut files).map_err(|e| e.to_string())?;
    files.sort();
    if files.is_empty() {
        return Err(format!("{EMPTY}: no files in this folder"));
    }
    let mut parts = Parts::default();
    let (mut read, mut left_out) = (0, Vec::new());
    for path in &files {
        // one file in memory at a time
        let added = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| text_of(&bytes))
            .and_then(|text| parts.add(&text));
        match added {
            Ok(()) => read += 1,
            Err(why) => left_out.push((
                path.strip_prefix(dir)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned(),
                why,
            )),
        }
    }
    if read == 0 {
        return Err(format!(
            "no S3 server access log, CloudTrail file or HAR among its {} file{}",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        ));
    }
    Ok(ImportedDir {
        imported: parts.finish(name)?,
        files: read,
        left_out,
    })
}

fn decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

fn query_of(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (decode(k), decode(v)),
            None => (decode(p), String::new()),
        })
        .collect()
}

/// The S3 error code an HTTP status most often means, for sources that give
/// only one of the two.
fn status_of(code: &str) -> u16 {
    match code {
        "NoSuchKey" | "NoSuchBucket" | "NoSuchUpload" | "NoSuchVersion" => 404,
        "AccessDenied"
        | "SignatureDoesNotMatch"
        | "InvalidAccessKeyId"
        | "RequestTimeTooSkewed" => 403,
        "PreconditionFailed" => 412,
        "NotModified" => 304,
        "SlowDown" | "ServiceUnavailable" => 503,
        "InternalError" => 500,
        "InvalidRange" => 416,
        "EntityTooLarge" | "EntityTooSmall" | "InvalidArgument" | "InvalidRequest"
        | "MalformedXML" => 400,
        "" => 200,
        _ => 400,
    }
}

fn record(
    time: DateTime<Utc>,
    method: &str,
    bucket: Option<String>,
    key: Option<String>,
    query: Vec<(String, String)>,
    status: u16,
    latency_ms: f64,
) -> Record {
    let operation = crate::op::classify(method, bucket.as_deref(), key.as_deref(), &query, &[]);
    Record {
        time,
        method: method.into(),
        operation: operation.into(),
        bucket: bucket.map(Into::into),
        key: key.map(Into::into),
        query: query.iter().map(|(k, v)| (k.into(), v.into())).collect(),
        status: Some(status),
        latency_ms,
        ..Default::default()
    }
}

/// Splits an access-log line into its fields: words, `"quoted strings"` and
/// `[bracketed times]`.
fn fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == ' ' {
            chars.next();
            continue;
        }
        let close = match c {
            '"' => Some('"'),
            '[' => Some(']'),
            _ => None,
        };
        let mut f = String::new();
        if let Some(end) = close {
            chars.next();
            for c in chars.by_ref() {
                if c == end {
                    break;
                }
                f.push(c);
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ' ' {
                    break;
                }
                f.push(c);
                chars.next();
            }
        }
        out.push(f);
    }
    out
}

/// S3 server access log lines (`bucket_owner bucket [time] remote_ip
/// requester request_id operation key "request_uri" status error_code
/// bytes_sent object_size total_time turnaround "referer" "user_agent" …`).
/// A line that cannot be read (a file cut off mid-line) is counted in
/// `malformed` and one that is no request (`S3.EXPIRE.OBJECT`,
/// `BATCH.DELETE.OBJECT`) in `not_requests`: neither fails the file.
fn access_log(
    text: &str,
    malformed: &mut usize,
    not_requests: &mut usize,
    sources: &mut std::collections::HashMap<String, String>,
    hosts: &mut std::collections::BTreeMap<String, usize>,
) -> Vec<Record> {
    let mut out: Vec<Record> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let f = fields(line);
        let time = f
            .get(2)
            .filter(|_| f.len() >= 14)
            .and_then(|t| DateTime::parse_from_str(t, "%d/%b/%Y:%H:%M:%S %z").ok());
        let Some(time) = time else {
            *malformed += 1;
            continue;
        };
        let time = time.with_timezone(&Utc);
        // requests only: the rest has no method, no status and no price
        if !(f[6].starts_with("REST.") || f[6].starts_with("WEBSITE.")) {
            *not_requests += 1;
            continue;
        }
        // a copy is logged twice: the request, and the store reading the
        // source for it (REST.COPY.OBJECT_GET, REST.COPY.PART_GET) under the
        // same request id. the second is no request: it names the source
        if f[6].contains(".COPY.") && f[6].ends_with("_GET") {
            *not_requests += 1;
            if f[5] != "-" && f[1] != "-" && f[7] != "-" {
                sources.insert(f[5].clone(), format!("/{}/{}", f[1], f[7]));
            }
            continue;
        }
        // "GET /bucket/key?query HTTP/1.1"
        let mut uri = f[8].split(' ');
        let method = uri.next().unwrap_or("GET").to_string();
        let target = uri.next().unwrap_or("/");
        let query = target
            .split_once('?')
            .map(|(_, q)| query_of(q))
            .unwrap_or_default();
        let bucket = (f[1] != "-").then(|| f[1].clone());
        let key = (f[7] != "-").then(|| decode(&f[7]));
        let status: u16 = f[9].parse().unwrap_or(0);
        let num = |s: &str| s.parse::<u64>().ok();
        let mut r = record(
            time,
            &method,
            bucket,
            key,
            query,
            status,
            num(&f[13]).unwrap_or(0) as f64,
        );
        r.request_id = (f[5] != "-").then(|| f[5].clone());
        // the host the request named (the log's Host Header field)
        if let Some(h) = f.get(22).and_then(|h| store_host(h, r.bucket.as_deref())) {
            *hosts.entry(h).or_default() += 1;
        }
        // who sent it, how long S3 took to its first byte, and with what
        r.requester = f.get(4).filter(|v| *v != "-").cloned();
        r.first_byte_ms = f.get(14).and_then(|v| v.parse::<f64>().ok());
        if let Some(ua) = f.get(16).filter(|v| *v != "-" && !v.is_empty()) {
            r.request_headers
                .push(reqlog::Header::new("user-agent", ua));
        }
        r.response_bytes = num(&f[11]);
        // the object's size is what was sent only where the object is the
        // body: a CompleteMultipartUpload's is its parts', sent already
        if matches!(f[6].as_str(), "REST.PUT.OBJECT" | "REST.PUT.PART") {
            r.request_bytes = num(&f[12]);
        }
        if f[10] != "-" {
            r.error_body = Some(format!("<Error><Code>{}</Code></Error>", f[10]));
        }
        // the access log has the operation itself: keep S3's word for a copy
        if f[6].contains("COPY") && !r.operation.contains("Copy") {
            r.operation = if f[6].contains("PART") {
                "UploadPartCopy"
            } else {
                "CopyObject"
            }
            .into();
        }
        out.push(r);
    }
    out
}

/// The request parameters CloudTrail keeps that are the request's query:
/// which upload and part, which listing and page, which version.
const TRAIL_QUERY: &[&str] = &[
    "uploadId",
    "partNumber",
    "list-type",
    "prefix",
    "delimiter",
    "continuation-token",
    "start-after",
    "marker",
    "key-marker",
    "upload-id-marker",
    "version-id-marker",
    "max-keys",
    "versionId",
];

/// How an import's error begins when the file is not what it says it is
/// (bad data, 65), not the wrong file (64): [`is_bad_data`].
const BROKEN: &str = "the file breaks";

/// How an import's error begins when there is nothing in it to import (an
/// empty file or folder, a log of no S3 requests): bad data too.
const EMPTY: &str = "nothing to import";

/// Whether an import's error is bad data (65): a file broken part way
/// ([`BROKEN`]) or nothing to import ([`EMPTY`]); else it is the wrong
/// file (64).
pub fn is_bad_data(error: &str) -> bool {
    error.starts_with(BROKEN) || error.starts_with(EMPTY)
}

/// CloudTrail S3 data events (`{"Records": [...]}`, or one event per line).
/// A line or event that does not read is counted in `malformed`; another
/// service's event in `other`. A `{"Records": …}` document that does not
/// read to its end is an error saying where it broke.
fn cloudtrail(
    text: &str,
    malformed: &mut usize,
    other: &mut usize,
    hosts: &mut std::collections::BTreeMap<String, usize>,
) -> Result<Vec<Record>, String> {
    let events: Vec<Value> = match serde_json::from_str::<Value>(text) {
        Ok(v) => match v.get("Records") {
            Some(Value::Array(a)) => a.clone(),
            _ => vec![v],
        },
        // one document, not one event a line: cut short or damaged
        Err(e)
            if text
                .chars()
                .take(4096)
                .collect::<String>()
                .contains("\"Records\"")
                && text
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .is_none_or(|l| serde_json::from_str::<Value>(l).is_err()) =>
        {
            return Err(format!(
                "{BROKEN} at line {}, column {}: a CloudTrail file's JSON does not read to its end ({e})",
                e.line(),
                e.column()
            ));
        }
        // JSON lines (as Athena or a log pipeline writes them)
        Err(_) => text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| {
                let v = serde_json::from_str(l).ok();
                *malformed += v.is_none() as usize;
                v
            })
            .collect(),
    };
    let mut out = Vec::new();
    for e in events {
        if e["eventSource"].as_str() != Some("s3.amazonaws.com") {
            *other += 1;
            continue;
        }
        let name = e["eventName"].as_str().unwrap_or("");
        let Some(time) = e["eventTime"]
            .as_str()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc))
        else {
            *malformed += 1;
            continue;
        };
        let p = &e["requestParameters"];
        let bucket = p["bucketName"].as_str().map(String::from);
        let key = p["key"].as_str().map(String::from);
        let code = e["errorCode"].as_str().unwrap_or("");
        let method = match name {
            n if n.starts_with("Get") || n.starts_with("List") => "GET",
            n if n.starts_with("Head") => "HEAD",
            n if n.starts_with("Delete") && n != "DeleteObjects" => "DELETE",
            "AbortMultipartUpload" => "DELETE",
            "CreateMultipartUpload"
            | "CompleteMultipartUpload"
            | "DeleteObjects"
            | "RestoreObject"
            | "SelectObjectContent" => "POST",
            _ => "PUT",
        };
        let query: Vec<(String, String)> = TRAIL_QUERY
            .iter()
            .filter_map(|k| {
                let v = match &p[*k] {
                    Value::String(v) => v.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => return None,
                };
                Some((k.to_string(), v))
            })
            .collect();
        let mut r = record(time, method, bucket, key, query, status_of(code), 0.0);
        // S3 in the event's region
        if let Some(region) = e["awsRegion"].as_str().filter(|r| !r.is_empty()) {
            *hosts
                .entry(format!("s3.{region}.amazonaws.com"))
                .or_default() += 1;
            r.region = Some(region.into());
        }
        // CloudTrail names the operation itself
        r.operation = name.into();
        r.request_id = e["requestID"].as_str().map(String::from);
        let who = &e["userIdentity"];
        r.requester = who["arn"]
            .as_str()
            .or_else(|| who["principalId"].as_str())
            .map(String::from);
        if let Some(ua) = e["userAgent"].as_str().filter(|v| !v.is_empty()) {
            r.request_headers
                .push(reqlog::Header::new("user-agent", ua));
        }
        let extra = &e["additionalEventData"];
        r.request_bytes = extra["bytesTransferredIn"].as_f64().map(|v| v as u64);
        r.response_bytes = extra["bytesTransferredOut"].as_f64().map(|v| v as u64);
        if !code.is_empty() {
            r.error_body = Some(format!("<Error><Code>{code}</Code></Error>"));
        }
        out.push(r);
    }
    Ok(out)
}

/// A HAR file's requests to S3 (a browser's, say).
fn har(
    text: &str,
    other_hosts: &mut usize,
    hosts: &mut std::collections::BTreeMap<String, usize>,
) -> Result<Vec<Record>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let entries = v["log"]["entries"]
        .as_array()
        .ok_or("a HAR without log.entries")?;
    let mut out = Vec::new();
    for e in entries {
        let req = &e["request"];
        let Some(url) = req["url"]
            .as_str()
            .and_then(|u| reqwest::Url::parse(u).ok())
        else {
            continue;
        };
        let time = e["startedDateTime"]
            .as_str()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc))
            .ok_or("an entry without startedDateTime")?;
        let method = req["method"].as_str().unwrap_or("GET");
        let host = url.host_str().unwrap_or("");
        // virtual-hosted (bucket.s3….) or path-style (/bucket/key)
        let path = decode(url.path().trim_start_matches('/'));
        let (bucket, key) = match host.split_once(".s3") {
            Some((b, _)) if !b.is_empty() => {
                (Some(b.to_string()), (!path.is_empty()).then_some(path))
            }
            _ => match path.split_once('/') {
                Some((b, k)) => (Some(b.to_string()), (!k.is_empty()).then(|| k.to_string())),
                None => ((!path.is_empty()).then_some(path), None),
            },
        };
        let mut query = url.query().map(query_of).unwrap_or_default();
        query.retain(|(k, _)| !crate::PRESIGN_PARAMS.contains(&k.as_str()));
        let headers: Vec<(String, String)> = req["headers"]
            .as_array()
            .map(|h| {
                h.iter()
                    .filter_map(|x| {
                        Some((
                            x["name"].as_str()?.to_string(),
                            x["value"].as_str()?.to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // S3's requests only: a browser's HAR holds every page, script and image
        let names = |v: &Value| -> Vec<String> {
            v.as_array()
                .map(|h| {
                    h.iter()
                        .filter_map(|x| x["name"].as_str().map(str::to_ascii_lowercase))
                        .collect()
                })
                .unwrap_or_default()
        };
        let signed = headers.iter().any(|(n, v)| {
            let n = n.to_ascii_lowercase();
            n.starts_with("x-amz-")
                || (n == "authorization"
                    && (v.starts_with("AWS4-HMAC-SHA256") || v.starts_with("AWS ")))
        }) || ["X-Amz-Signature", "X-Amz-Credential", "AWSAccessKeyId"]
            .iter()
            .any(|k| url.query_pairs().any(|(q, _)| q == *k));
        let answered_by_s3 = names(&e["response"]["headers"])
            .iter()
            .any(|n| n == "x-amz-request-id" || n == "x-amz-id-2");
        let s3_host = host.contains(".s3.")
            || host.contains(".s3-")
            || host.starts_with("s3.")
            || host.starts_with("s3-");
        if !(signed || answered_by_s3 || s3_host || e["_sonde"].is_object()) {
            // another host's: counted, so that the import says what it left out
            *other_hosts += 1;
            continue;
        }
        if let Some(h) = store_host(host, bucket.as_deref().filter(|_| host.contains(".s3"))) {
            *hosts.entry(h).or_default() += 1;
        }
        let status = e["response"]["status"].as_u64().unwrap_or(0) as u16;
        let mut r = record(
            time,
            method,
            bucket,
            key,
            query,
            status,
            e["time"].as_f64().unwrap_or(0.0),
        );
        r.operation = crate::op::classify(
            method,
            r.bucket.as_deref(),
            r.key.as_deref(),
            &r.query,
            &headers,
        )
        .into();
        // what the request carried: the SDK it names, its conditions, its
        // range (secrets masked here, as on every record)
        r.request_headers = headers
            .iter()
            .filter(|(n, _)| !n.starts_with(':'))
            .map(|(n, v)| reqlog::Header::new(n, reqlog::redact::redact_header(n, v)))
            .collect();
        // sonde's own HAR names the kind (`operation` before 20.0)
        if let Some(op) = e["_sonde"]["kind"]
            .as_str()
            .or_else(|| e["_sonde"]["operation"].as_str())
        {
            r.operation = op.into();
        }
        // a presigned URL's signature, key and token never reach the session
        let mut shown = url.clone();
        if url.query().is_some() {
            let kept: Vec<(String, String)> = url
                .query_pairs()
                .filter(|(k, _)| !crate::PRESIGN_PARAMS.contains(&k.as_ref()))
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            if kept.is_empty() {
                shown.set_query(None);
            } else {
                shown.query_pairs_mut().clear().extend_pairs(kept);
            }
        }
        r.url = shown.to_string();
        let size = |v: &Value| v.as_i64().filter(|n| *n >= 0).map(|n| n as u64);
        r.request_bytes = size(&req["bodySize"]);
        r.response_bytes = size(&e["response"]["bodySize"]);
        // the wait for the answer's first byte, as HAR times it
        r.first_byte_ms = e["timings"]["wait"].as_f64().filter(|w| *w >= 0.0);
        if let Some(t) = e["response"]["content"]["text"].as_str()
            && status >= 300
        {
            r.error_body = Some(t.to_string());
        }
        out.push(r);
    }
    Ok(out)
}

/// Writes an imported session as a `.sonde` file, compressed as a finished
/// recording is.
pub fn save(session: &Session, path: &std::path::Path) -> std::io::Result<()> {
    use reqlog::Sink;
    {
        let w = crate::session::SessionWriter::create(path, &session.header)?;
        for r in &session.records {
            w.write(r);
        }
        w.flush()?;
    }
    crate::session::compress(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gzip_file_larger_than_the_limit_is_refused_not_read() {
        use std::io::Write;
        let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(&vec![0u8; 1 << 20]).unwrap();
        let bytes = z.finish().unwrap();
        assert_eq!(gunzip(&bytes, 1 << 20).unwrap().len(), 1 << 20);
        let e = gunzip(&bytes, (1 << 20) - 1).unwrap_err();
        assert!(e.to_string().contains("more than"), "{e}");
    }

    const LOG: &str = r#"79a5 lake [06/Feb/2026:00:00:38 +0000] 192.0.2.3 arn:aws:iam::1:user/etl 3E57427F3EXAMPLE REST.GET.OBJECT t/part-00.parquet "GET /lake/t/part-00.parquet HTTP/1.1" 200 - 4096 4096 12 10 "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -
79a5 lake [06/Feb/2026:00:00:39 +0000] 192.0.2.3 arn:aws:iam::1:user/etl 3E57427F3EXAMPL2 REST.HEAD.OBJECT t/missing "HEAD /lake/t/missing HTTP/1.1" 404 NoSuchKey 0 - 3 - "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -
79a5 lake [06/Feb/2026:00:00:40 +0000] 192.0.2.3 arn:aws:iam::1:user/etl 3E57427F3EXAMPL3 REST.GET.BUCKET - "GET /lake?list-type=2&prefix=t%2F HTTP/1.1" 200 - 812 - 25 24 "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#;

    #[test]
    fn a_seconds_requests_are_spread_over_it_and_the_session_says_so() {
        // five requests logged in one second, then two in the next
        let line = |sec: u32, key: &str| {
            format!(
                "79a5 lake [06/Feb/2026:00:00:{sec:02} +0000] 192.0.2.3 arn:aws:iam::1:user/etl ID{key} REST.GET.OBJECT {key} \"GET /lake/{key} HTTP/1.1\" 503 SlowDown 0 - 40 30 \"-\" \"aws-sdk-java/2.21\" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"
            )
        };
        let log: Vec<String> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|k| line(38, k))
            .chain(["f", "g"].iter().map(|k| line(39, k)))
            .collect();
        let s = import(&log.join("\n"), "small").unwrap().1;
        assert!(s.header.whole_seconds);
        let ms: Vec<i64> = s
            .records
            .iter()
            .map(|r| (r.time - s.records[0].time).num_milliseconds())
            .collect();
        // in the log's order, evenly: not five at once
        assert_eq!(ms, [0, 200, 400, 600, 800, 1000, 1500]);
        assert_eq!(
            s.records
                .iter()
                .map(|r| r.key.as_deref().unwrap())
                .collect::<String>(),
            "abcdefg"
        );
        // named by the store, not by the file
        assert_eq!(s.header.endpoint, "s3.amazonaws.com");
        assert_eq!(s.records[0].endpoint, "s3.amazonaws.com");
        // what depends on where in its second each began is not said:
        // requests "one after another", and how many were in flight when
        // the store throttled
        let eps = crate::episode::episodes(&s.records);
        let obs = crate::episode::observations_of(&s.header, &s.records, &eps);
        let keys: Vec<&str> = obs.iter().map(|o| o.phrases[0].key).collect();
        assert!(keys.contains(&"obs.throttled.rate"), "{keys:?}");
        assert!(
            !keys.contains(&"obs.serial") && !keys.contains(&"obs.throttled"),
            "{keys:?}"
        );
        let said = serde_json::to_string(&s.header).unwrap();
        assert!(said.contains("\"wholeSeconds\":true"), "{said}");
        // a recording says nothing of the kind, and reads as before
        let recorded = SessionHeader::new("e", "", "", None);
        assert!(
            !serde_json::to_string(&recorded)
                .unwrap()
                .contains("wholeSeconds")
        );
        assert_eq!(Source::AccessLog.a(), "an S3 server access log");
    }

    #[test]
    fn a_har_says_what_it_left_out_and_times_the_first_byte() {
        let entry = |url: &str, headers: &str| {
            format!(
                r#"{{"startedDateTime":"2026-11-09T10:00:00.250Z","time":25,"timings":{{"send":1,"wait":18.5,"receive":5.5}},"request":{{"method":"GET","url":"{url}","headers":[{headers}]}},"response":{{"status":200,"headers":[],"bodySize":10}}}}"#
            )
        };
        let har = format!(
            r#"{{"log":{{"version":"1.2","entries":[{},{},{}]}}}}"#,
            entry("https://cdn.example.com/app.js", ""),
            entry("https://example.com/", ""),
            entry(
                "https://lake.s3.eu-west-1.amazonaws.com/t/k",
                r#"{"name":"x-amz-date","value":"20261109T100000Z"}"#
            ),
        );
        let i = import_counted(&har, "page").unwrap();
        assert_eq!(i.session.records.len(), 1);
        assert_eq!(
            i.other_hosts, 2,
            "the page and its script: counted, not dropped unsaid"
        );
        assert_eq!(i.session.records[0].first_byte_ms, Some(18.5));
        assert_eq!(i.session.header.endpoint, "s3.eu-west-1.amazonaws.com");
        // times with fractions are the request's own
        assert!(!i.session.header.whole_seconds);
    }

    #[test]
    fn an_access_log_reads_as_requests() {
        assert_eq!(detect(LOG), Some(Source::AccessLog));
        let (_, s) = import(LOG, "prod").unwrap();
        let ops: Vec<&str> = s.records.iter().map(|r| r.operation.as_str()).collect();
        assert_eq!(ops, ["GetObject", "HeadObject", "ListObjectsV2"]);
        assert_eq!(s.records[0].latency_ms, 12.0);
        assert_eq!(s.records[0].response_bytes, Some(4096));
        // who, with what, and S3's own time to its first byte
        assert_eq!(
            s.records[0].requester.as_deref(),
            Some("arn:aws:iam::1:user/etl")
        );
        assert_eq!(s.records[0].first_byte_ms, Some(10.0));
        assert_eq!(s.records[1].first_byte_ms, None);
        let ua = s.records[0]
            .request_headers
            .iter()
            .find(|h| h.name == "user-agent");
        assert_eq!(ua.map(|h| h.value.as_str()), Some("aws-sdk-java/2.21"));
        assert!(crate::summary::is_miss(&s.records[1]));
        assert_eq!(s.records[2].query[1], ("prefix".into(), "t/".into()));
        assert_eq!(
            s.header.command.as_deref(),
            Some("imported from S3 server access log")
        );
    }

    #[test]
    fn cloudtrail_events_and_a_har_read_as_requests() {
        let ct = r#"{"Records":[
          {"eventTime":"2026-02-06T00:00:02Z","eventSource":"s3.amazonaws.com","eventName":"PutObject","requestParameters":{"bucketName":"lake","key":"a"},"additionalEventData":{"bytesTransferredIn":10.0}},
          {"eventTime":"2026-02-06T00:00:01Z","eventSource":"s3.amazonaws.com","eventName":"GetObject","requestParameters":{"bucketName":"lake","key":"b"},"errorCode":"NoSuchKey","userIdentity":{"arn":"arn:aws:iam::1:role/etl"},"userAgent":"[aws-cli/2.15]"},
          {"eventTime":"2026-02-06T00:00:01Z","eventSource":"ec2.amazonaws.com","eventName":"RunInstances"}]}"#;
        let (src, s) = import(ct, "trail").unwrap();
        assert_eq!(src, Source::CloudTrail);
        assert_eq!(s.records.len(), 2);
        assert_eq!(s.records[0].operation, "GetObject");
        assert_eq!(s.records[0].status, Some(404));
        assert_eq!(
            s.records[0].requester.as_deref(),
            Some("arn:aws:iam::1:role/etl")
        );
        assert!(
            s.records[0]
                .request_headers
                .iter()
                .any(|h| h.name == "user-agent" && h.value == "[aws-cli/2.15]")
        );
        assert_eq!(s.records[1].request_bytes, Some(10));

        let har = serde_json::to_string(&reqlog::export::to_har(&[Record {
            time: Utc::now(),
            method: "GET".into(),
            operation: "GetObject".into(),
            url: "https://lake.s3.us-east-1.amazonaws.com/t/k%20x".into(),
            status: Some(200),
            latency_ms: 7.0,
            ..Default::default()
        }]))
        .unwrap();
        let (src, s) = import(&har, "browser").unwrap();
        assert_eq!(src, Source::Har);
        assert_eq!(s.records[0].bucket.as_deref(), Some("lake"));
        assert_eq!(s.records[0].key.as_deref(), Some("t/k x"));
        assert_eq!(s.records[0].latency_ms, 7.0);
    }

    #[test]
    fn a_browser_har_keeps_only_its_s3_requests() {
        let entry = |url: &str, req: &str, resp: &str| {
            format!(
                r#"{{"startedDateTime":"2026-11-09T10:00:00Z","time":5,"request":{{"method":"GET","url":"{url}","headers":[{req}]}},"response":{{"status":200,"headers":[{resp}],"bodySize":10}}}}"#
            )
        };
        let har = format!(
            r#"{{"log":{{"version":"1.2","entries":[{},{},{},{},{}]}}}}"#,
            entry("https://cdn.example.com/js/app.js", "", ""),
            entry("https://example.com/", "", ""),
            entry(
                "http://127.0.0.1:9000/data/k",
                r#"{"name":"Authorization","value":"AWS4-HMAC-SHA256 Credential=x"}"#,
                ""
            ),
            entry(
                "https://minio.local/b/k2",
                "",
                r#"{"name":"x-amz-request-id","value":"1"}"#
            ),
            entry(
                "https://b.s3.amazonaws.com/k3?versionId=7&X-Amz-Credential=AKIAX&X-Amz-Security-Token=TOK&X-Amz-Signature=SIG",
                "",
                ""
            ),
        );
        let (_, s) = import(&har, "browser").unwrap();
        // a presigned URL's credentials never reach the session
        let text = serde_json::to_string(&s.records).unwrap();
        for secret in ["AKIAX", "TOK", "SIG"] {
            assert!(!text.contains(secret), "{secret} kept: {text}");
        }
        assert!(
            s.records[2].url.ends_with("/k3?versionId=7"),
            "{}",
            s.records[2].url
        );
        let keys: Vec<_> = s
            .records
            .iter()
            .map(|r| r.key.clone().unwrap_or_default())
            .collect();
        assert_eq!(
            keys,
            ["k", "k2", "k3"],
            "the page and its script are not S3's"
        );
    }

    #[test]
    fn the_stores_read_for_a_copy_is_part_of_the_copy_not_a_request() {
        let log = [
            // the copy as the program sent it, and the store reading its source
            r#"79a5 lake [06/Feb/2026:00:00:41 +0000] 192.0.2.3 arn:aws:iam::1:user/etl C0FFEE REST.COPY.OBJECT t/copy "PUT /lake/t/copy HTTP/1.1" 200 - 234 4096 30 - "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#,
            r#"79a5 archive [06/Feb/2026:00:00:41 +0000] 192.0.2.3 arn:aws:iam::1:user/etl C0FFEE REST.COPY.OBJECT_GET t/src - 200 - - 4096 - - - - - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#,
            // a part copied into an upload
            r#"79a5 lake [06/Feb/2026:00:00:42 +0000] 192.0.2.3 arn:aws:iam::1:user/etl BEEF REST.COPY.PART t/big "PUT /lake/t/big?partNumber=1&uploadId=u1 HTTP/1.1" 200 - 234 4096 30 - "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#,
            r#"79a5 lake [06/Feb/2026:00:00:42 +0000] 192.0.2.3 arn:aws:iam::1:user/etl BEEF REST.COPY.PART_GET t/src - 206 - - 4096 - - - - - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#,
        ]
        .join("\n");
        let i = import_counted(&log, "prod").unwrap();
        let ops: Vec<(&str, Option<&str>)> = i
            .session
            .records
            .iter()
            .map(|r| (r.operation.as_str(), r.key.as_deref()))
            .collect();
        assert_eq!(
            ops,
            [
                ("CopyObject", Some("t/copy")),
                ("UploadPartCopy", Some("t/big"))
            ],
            "two requests were sent, not four"
        );
        assert_eq!(i.not_requests, 2);
        // what it read is kept on the copy, as the request's own header says it
        let source = |r: &Record| {
            r.request_headers
                .iter()
                .find(|h| h.name == "x-amz-copy-source")
                .map(|h| h.value.clone())
        };
        assert_eq!(
            source(&i.session.records[0]).as_deref(),
            Some("/archive/t/src")
        );
        assert_eq!(
            source(&i.session.records[1]).as_deref(),
            Some("/lake/t/src")
        );
    }

    #[test]
    fn a_folder_of_log_files_is_one_session_in_the_order_of_their_names() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let lines: Vec<&str> = LOG.lines().collect();
        // as S3 delivers them: many small files, some in folders, some gzipped
        std::fs::write(dir.path().join("2026-02-06-00-10-00-B"), lines[1]).unwrap();
        std::fs::create_dir(dir.path().join("2026-02-06-00-05")).unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), Default::default());
        gz.write_all(lines[0].as_bytes()).unwrap();
        std::fs::write(
            dir.path().join("2026-02-06-00-05/A.gz"),
            gz.finish().unwrap(),
        )
        .unwrap();
        std::fs::write(dir.path().join("2026-02-06-00-15-00-C"), lines[2]).unwrap();
        // what else a folder holds
        std::fs::write(dir.path().join("README.txt"), "the logs of lake").unwrap();
        std::fs::write(dir.path().join(".DS_Store"), [0u8, 1, 2]).unwrap();
        std::fs::write(dir.path().join("trail.json"), r#"{"Records":[]}"#).unwrap();

        let d = import_dir(dir.path(), "prod").unwrap();
        assert_eq!(d.files, 3);
        let ops: Vec<&str> = d
            .imported
            .session
            .records
            .iter()
            .map(|r| r.operation.as_str())
            .collect();
        assert_eq!(ops, ["GetObject", "HeadObject", "ListObjectsV2"]);
        assert_eq!(d.imported.source, Source::AccessLog);
        let left: Vec<&str> = d.left_out.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(left, ["README.txt", "trail.json"], "{:?}", d.left_out);
        assert!(d.left_out[1].1.contains("CloudTrail"), "{:?}", d.left_out);

        // a folder with nothing sonde reads says so
        let empty = tempfile::tempdir().unwrap();
        assert!(
            import_dir(empty.path(), "x")
                .unwrap_err()
                .contains("no files")
        );
        std::fs::write(empty.path().join("notes"), "hello").unwrap();
        assert!(
            import_dir(empty.path(), "x")
                .unwrap_err()
                .contains("among its 1 file")
        );
    }

    #[test]
    fn a_copys_source_line_in_another_file_of_the_folder_is_still_its_source() {
        let dir = tempfile::tempdir().unwrap();
        let copy = r#"79a5 lake [06/Feb/2026:00:00:41 +0000] 192.0.2.3 arn:aws:iam::1:user/etl C0FFEE REST.COPY.OBJECT t/copy "PUT /lake/t/copy HTTP/1.1" 200 - 234 4096 30 - "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#;
        let read = r#"79a5 archive [06/Feb/2026:00:00:41 +0000] 192.0.2.3 arn:aws:iam::1:user/etl C0FFEE REST.COPY.OBJECT_GET t/src - 200 - - 4096 - - - - - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#;
        // the store's read is delivered in the source bucket's file, first by name
        std::fs::write(dir.path().join("a-archive.log"), read).unwrap();
        std::fs::write(dir.path().join("b-lake.log"), copy).unwrap();
        let d = import_dir(dir.path(), "logs").unwrap();
        let r = &d.imported.session.records;
        assert_eq!(r.len(), 1);
        assert_eq!(
            r[0].request_headers
                .iter()
                .find(|h| h.name == "x-amz-copy-source")
                .map(|h| h.value.as_str()),
            Some("/archive/t/src")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_import_does_not_follow_links() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let line = r#"79a5 lake [06/Feb/2026:00:00:41 +0000] 192.0.2.3 arn:aws:iam::1:user/etl C0FFEE REST.GET.OBJECT t/k "GET /lake/t/k HTTP/1.1" 200 - 234 4096 30 - "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#;
        std::fs::write(dir.path().join("a.log"), line).unwrap();
        std::fs::write(outside.path().join("elsewhere.log"), line).unwrap();
        // a link to the folder itself, and one out of it
        std::os::unix::fs::symlink(dir.path(), dir.path().join("self")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("out")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("elsewhere.log"),
            dir.path().join("z.log"),
        )
        .unwrap();
        let d = import_dir(dir.path(), "logs").unwrap();
        assert_eq!(d.files, 1);
        assert_eq!(d.imported.session.records.len(), 1);
    }

    #[test]
    fn lines_that_are_not_requests_or_cut_off_are_counted_not_imported() {
        let log = format!(
            "{LOG}\n{}\n{}\n{}\n{}",
            // a lifecycle expiry and a batch delete's object: no method, no status
            r#"79a5 lake [06/Feb/2026:00:00:41 +0000] - AmazonS3 5E57 S3.EXPIRE.OBJECT t/old - "-" - - - 4096 - - "-" "-" - xyz= - - - - - - -"#,
            r#"79a5 lake [06/Feb/2026:00:00:42 +0000] 192.0.2.3 arn:aws:iam::1:user/etl 6E57 BATCH.DELETE.OBJECT t/gone - "-" 204 - - 4096 - - "-" "-" - xyz= SigV4 - - - - - -"#,
            // a completed upload: the object's size is not this request's body
            r#"79a5 lake [06/Feb/2026:00:00:43 +0000] 192.0.2.3 arn:aws:iam::1:user/etl 7E57 REST.POST.UPLOAD t/big "POST /lake/t/big?uploadId=u1 HTTP/1.1" 200 - 300 10485760 40 39 "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#,
            // the file ends mid-line
            r#"79a5 lake [06/Feb/2026:00:00:44 +0000] 192.0.2.3 arn:aws"#,
        );
        let i = import_counted(&log, "prod").unwrap();
        assert_eq!((i.malformed, i.not_requests), (1, 2));
        let ops: Vec<&str> = i
            .session
            .records
            .iter()
            .map(|r| r.operation.as_str())
            .collect();
        assert_eq!(
            ops,
            [
                "GetObject",
                "HeadObject",
                "ListObjectsV2",
                "CompleteMultipartUpload"
            ]
        );
        assert_eq!(i.session.records[3].request_bytes, None);
        // an upload's own body is still counted
        let put = r#"79a5 lake [06/Feb/2026:00:00:45 +0000] 192.0.2.3 - 8E57 REST.PUT.OBJECT t/new "PUT /lake/t/new HTTP/1.1" 200 - - 512 9 8 "-" "-" - xyz= SigV4 - - - - - -"#;
        let (_, s) = import(put, "prod").unwrap();
        assert_eq!(s.records[0].request_bytes, Some(512));
    }

    #[test]
    fn cloudtrail_keeps_which_upload_part_and_listing_a_request_was() {
        let ct = r#"{"Records":[
          {"eventTime":"2026-02-06T00:00:01Z","eventSource":"s3.amazonaws.com","eventName":"CreateMultipartUpload","requestParameters":{"bucketName":"lake","key":"big","uploads":""},"responseElements":{"uploadId":"u1"}},
          {"eventTime":"2026-02-06T00:00:02Z","eventSource":"s3.amazonaws.com","eventName":"UploadPart","requestParameters":{"bucketName":"lake","key":"big","uploadId":"u1","partNumber":"1","Host":"lake.s3.amazonaws.com"}},
          {"eventTime":"2026-02-06T00:00:03Z","eventSource":"s3.amazonaws.com","eventName":"CompleteMultipartUpload","requestParameters":{"bucketName":"lake","key":"big","uploadId":"u1"}},
          {"eventTime":"2026-02-06T00:00:04Z","eventSource":"s3.amazonaws.com","eventName":"ListObjects","requestParameters":{"bucketName":"lake","list-type":"2","prefix":"t/","max-keys":1000,"continuation-token":"abc"}},
          {"eventTime":"2026-02-06T00:00:05Z","eventSource":"s3.amazonaws.com","eventName":"GetObject","requestParameters":{"bucketName":"lake","key":"k","versionId":"v7"}},
          {"eventTime":"2026-02-06T00:00:06Z","eventSource":"s3.amazonaws.com","eventName":"AbortMultipartUpload","requestParameters":{"bucketName":"lake","key":"other","uploadId":"u2"}}]}"#;
        let (_, s) = import(ct, "trail").unwrap();
        let q = |i: usize, k: &str| {
            s.records[i]
                .query
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(q(1, "uploadId"), Some("u1"));
        assert_eq!(q(1, "partNumber"), Some("1"));
        assert_eq!(q(1, "Host"), None);
        assert_eq!(q(2, "uploadId"), Some("u1"));
        assert_eq!(q(3, "prefix"), Some("t/"));
        assert_eq!(q(3, "continuation-token"), Some("abc"));
        assert_eq!(q(3, "max-keys"), Some("1000"));
        assert_eq!(q(4, "versionId"), Some("v7"));
        // an abort removes: it is not a write
        assert_eq!(s.records[5].method, "DELETE");
        // the upload reads as finished, with its part
        let eps = crate::episode::episodes(&s.records);
        let obs = crate::episode::observations(&s.records, &eps);
        assert!(
            !obs.iter().any(|o| o.phrases[0].key == "obs.unfinished"),
            "{obs:?}"
        );
        let upload = eps
            .iter()
            .find(|e| e.kind == crate::episode::Kind::Multipart && e.records.len() == 3);
        assert!(upload.is_some(), "{eps:?}");
    }

    #[test]
    fn a_cut_cloudtrail_line_and_other_services_events_are_counted_not_fatal() {
        let ct = [
            r#"{"eventTime":"2026-02-06T00:00:01Z","eventSource":"s3.amazonaws.com","eventName":"GetObject","requestParameters":{"bucketName":"lake","key":"a"}}"#,
            r#"{"eventTime":"2026-02-06T00:00:02Z","eventSource":"ec2.amazonaws.com","eventName":"RunInstances"}"#,
            r#"{"eventTime":"2026-02-06T00:00:03Z","eventSource":"s3.amazonaws.com","eventName":"GetObject","requestParameters":{"bucketName":"lake","key":"b"}}"#,
            r#"{"eventSource":"s3.amazonaws.com","eventName":"GetObject"}"#,
            r#"{"eventTime":"2026-02-06T00:00:04Z","eventSource":"s3.amaz"#,
        ]
        .join("\n");
        let i = import_counted(&ct, "trail").unwrap();
        assert_eq!(i.session.records.len(), 2);
        assert_eq!((i.malformed, i.other_services), (2, 1));
    }

    #[test]
    fn a_cloudtrail_document_cut_short_says_where_it_broke() {
        let whole = r#"{"Records":[
  {"eventTime":"2026-02-06T00:00:01Z","eventSource":"s3.amazonaws.com","eventName":"GetObject","requestParameters":{"bucketName":"lake","key":"a"}},
  {"eventTime":"2026-02-06T00:00:02Z","eventSource":"s3.amazonaws.com","eventName":"GetObject","requestParameters":{"bucketName":"lake","key":"b"}}]}"#;
        assert_eq!(import(whole, "trail").unwrap().1.records.len(), 2);
        let cut = &whole[..whole.len() - 40];
        let Err(e) = import(cut, "trail") else {
            panic!("a cut document read as whole");
        };
        assert!(is_bad_data(&e), "{e}");
        assert!(e.contains("at line 3, column"), "{e}");
        // the wrong file is not broken
        assert!(!is_bad_data(&import("hello", "x").unwrap_err()));
        // nothing to import is bad data too
        assert!(is_bad_data(&import("", "x").unwrap_err()));
        assert!(is_bad_data(&import(" \n\n", "x").unwrap_err()));
    }

    #[test]
    fn a_gzipped_file_is_read_as_its_text() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(LOG.as_bytes()).unwrap();
        let bytes = gz.finish().unwrap();
        let text = text_of(&bytes).unwrap();
        assert_eq!(text, LOG);
        assert_eq!(import(&text, "prod").unwrap().1.records.len(), 3);
        assert_eq!(text_of(LOG.as_bytes()).unwrap(), LOG);
        assert!(text_of(&bytes[..bytes.len() / 2]).is_err());
    }

    #[test]
    fn a_har_keeps_what_each_request_carried() {
        let har = r#"{"log":{"version":"1.2","entries":[{"startedDateTime":"2026-11-09T10:00:00Z","time":5,
          "request":{"method":"PUT","url":"https://lake.s3.amazonaws.com/k","headers":[
            {"name":":authority","value":"lake.s3.amazonaws.com"},
            {"name":"User-Agent","value":"aws-sdk-js/3.500"},
            {"name":"If-Match","value":"\"e1\""},
            {"name":"x-amz-copy-source","value":"/lake/src"},
            {"name":"Authorization","value":"AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20261109/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=deadbeefdeadbeef"},
            {"name":"x-amz-security-token","value":"TOKENTOKENTOKEN"}]},
          "response":{"status":200,"headers":[],"bodySize":0}}]}}"#;
        let (_, s) = import(har, "browser").unwrap();
        let r = &s.records[0];
        let h = |n: &str| {
            r.request_headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(n))
                .map(|h| h.value.as_str())
        };
        assert_eq!(h("user-agent"), Some("aws-sdk-js/3.500"));
        assert_eq!(h("if-match"), Some("\"e1\""));
        assert_eq!(h("x-amz-copy-source"), Some("/lake/src"));
        assert_eq!(h(":authority"), None);
        assert_eq!(r.operation, "CopyObject");
        let text = serde_json::to_string(r).unwrap();
        for secret in [
            "deadbeefdeadbeef",
            "AKIAIOSFODNN7EXAMPLE",
            "TOKENTOKENTOKEN",
        ] {
            assert!(!text.contains(secret), "{secret} kept: {text}");
        }
    }

    #[test]
    fn a_session_is_not_imported() {
        let s = r#"{"format":"sonde-session","version":1}"#;
        assert_eq!(detect(s), None);
    }
}
