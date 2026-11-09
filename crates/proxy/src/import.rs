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

/// Reads `text` as the source it is into a session (records numbered in time
/// order). `name` names it (the endpoint shown for it).
pub fn import(text: &str, name: &str) -> Result<(Source, Session), String> {
    let source = detect(text).ok_or("not an S3 server access log, CloudTrail file or HAR")?;
    let mut records = match source {
        Source::AccessLog => access_log(text)?,
        Source::CloudTrail => cloudtrail(text)?,
        Source::Har => har(text)?,
    };
    if records.is_empty() {
        return Err(format!("no S3 requests in this {}", source.name()));
    }
    records.sort_by_key(|r| r.time);
    for (i, r) in records.iter_mut().enumerate() {
        r.id = i as u64 + 1;
        r.endpoint = name.to_string();
    }
    let mut header = SessionHeader::new(
        name,
        "",
        "",
        Some(format!("imported from {}", source.name())),
    );
    header.created = records[0].time;
    Ok((
        source,
        Session {
            header,
            records,
            logs: Vec::new(),
        },
    ))
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
        method: method.to_string(),
        operation,
        bucket,
        key,
        query,
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
fn access_log(text: &str) -> Result<Vec<Record>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let f = fields(line);
        if f.len() < 14 {
            return Err(format!("line {}: not an access log line", n + 1));
        }
        let time = DateTime::parse_from_str(&f[2], "%d/%b/%Y:%H:%M:%S %z")
            .map_err(|e| format!("line {}: time {:?}: {e}", n + 1, f[2]))?
            .with_timezone(&Utc);
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
        r.response_bytes = num(&f[11]);
        if method == "PUT" || method == "POST" {
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
    Ok(out)
}

/// CloudTrail S3 data events (`{"Records": [...]}`, or one event per line).
fn cloudtrail(text: &str) -> Result<Vec<Record>, String> {
    let events: Vec<Value> = match serde_json::from_str::<Value>(text) {
        Ok(v) => match v.get("Records") {
            Some(Value::Array(a)) => a.clone(),
            _ => vec![v],
        },
        // JSON lines (as Athena or a log pipeline writes them)
        Err(_) => text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?,
    };
    let mut out = Vec::new();
    for e in events {
        if e["eventSource"].as_str() != Some("s3.amazonaws.com") {
            continue;
        }
        let name = e["eventName"].as_str().unwrap_or("");
        let time = e["eventTime"]
            .as_str()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc))
            .ok_or("an event without eventTime")?;
        let p = &e["requestParameters"];
        let bucket = p["bucketName"].as_str().map(String::from);
        let key = p["key"].as_str().map(String::from);
        let code = e["errorCode"].as_str().unwrap_or("");
        let method = match name {
            n if n.starts_with("Get") || n.starts_with("List") => "GET",
            n if n.starts_with("Head") => "HEAD",
            n if n.starts_with("Delete") && n != "DeleteObjects" => "DELETE",
            "CreateMultipartUpload"
            | "CompleteMultipartUpload"
            | "DeleteObjects"
            | "RestoreObject"
            | "SelectObjectContent" => "POST",
            _ => "PUT",
        };
        let mut r = record(time, method, bucket, key, Vec::new(), status_of(code), 0.0);
        // CloudTrail names the operation itself
        r.operation = name.to_string();
        r.request_id = e["requestID"].as_str().map(String::from);
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

/// A HAR file's requests to S3 (a browser's, or sonde's own `--log-out`).
fn har(text: &str) -> Result<Vec<Record>, String> {
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
            continue;
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
        );
        // sonde's own HAR names the operation
        if let Some(op) = e["_sonde"]["operation"].as_str() {
            r.operation = op.to_string();
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

    const LOG: &str = r#"79a5 lake [06/Feb/2026:00:00:38 +0000] 192.0.2.3 arn:aws:iam::1:user/etl 3E57427F3EXAMPLE REST.GET.OBJECT t/part-00.parquet "GET /lake/t/part-00.parquet HTTP/1.1" 200 - 4096 4096 12 10 "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -
79a5 lake [06/Feb/2026:00:00:39 +0000] 192.0.2.3 arn:aws:iam::1:user/etl 3E57427F3EXAMPL2 REST.HEAD.OBJECT t/missing "HEAD /lake/t/missing HTTP/1.1" 404 NoSuchKey 0 - 3 - "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -
79a5 lake [06/Feb/2026:00:00:40 +0000] 192.0.2.3 arn:aws:iam::1:user/etl 3E57427F3EXAMPL3 REST.GET.BUCKET - "GET /lake?list-type=2&prefix=t%2F HTTP/1.1" 200 - 812 - 25 24 "-" "aws-sdk-java/2.21" - xyz= SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -"#;

    #[test]
    fn an_access_log_reads_as_requests() {
        assert_eq!(detect(LOG), Some(Source::AccessLog));
        let (_, s) = import(LOG, "prod").unwrap();
        let ops: Vec<&str> = s.records.iter().map(|r| r.operation.as_str()).collect();
        assert_eq!(ops, ["GetObject", "HeadObject", "ListObjectsV2"]);
        assert_eq!(s.records[0].latency_ms, 12.0);
        assert_eq!(s.records[0].response_bytes, Some(4096));
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
          {"eventTime":"2026-02-06T00:00:01Z","eventSource":"s3.amazonaws.com","eventName":"GetObject","requestParameters":{"bucketName":"lake","key":"b"},"errorCode":"NoSuchKey"},
          {"eventTime":"2026-02-06T00:00:01Z","eventSource":"ec2.amazonaws.com","eventName":"RunInstances"}]}"#;
        let (src, s) = import(ct, "trail").unwrap();
        assert_eq!(src, Source::CloudTrail);
        assert_eq!(s.records.len(), 2);
        assert_eq!(s.records[0].operation, "GetObject");
        assert_eq!(s.records[0].status, Some(404));
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
    fn a_session_is_not_imported() {
        let s = r#"{"format":"sonde-session","version":1}"#;
        assert_eq!(detect(s), None);
    }
}
