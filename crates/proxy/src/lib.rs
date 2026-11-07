//! A local S3 endpoint for recording what an application does.
//!
//! The application talks to `http://127.0.0.1:<port>` with any credentials.
//! Each request is parsed back into bucket, key and query, re-signed with the
//! credentials of the endpoint sonde is configured for (they never leave
//! Rust, and the application never sees them), forwarded, and recorded in a
//! [`RequestLog`] with the headers the application sent. Responses stream
//! back unchanged.

pub mod chunked;
pub mod episode;
pub mod op;
pub mod session;
pub mod summary;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use chrono::Utc;
use futures::TryStreamExt;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use reqlog::{Header, Record, RequestLog};
use s3raw::{S3Client, sigv4};
use tokio::net::TcpListener;

type Out = BoxBody<Bytes, std::io::Error>;

/// What the proxy lets through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Policy {
    /// Everything.
    #[default]
    Allow,
    /// Everything but operations that remove data ([`op::is_destructive`]).
    NoDelete,
    /// GET and HEAD only.
    ReadOnly,
}

impl Policy {
    /// Why `op` is refused, if it is.
    pub fn refuses(self, method: &str, op: &str) -> Option<&'static str> {
        match self {
            Policy::Allow => None,
            Policy::NoDelete if op::is_destructive(op) => Some("no-delete"),
            Policy::ReadOnly if method != "GET" && method != "HEAD" => Some("read-only"),
            _ => None,
        }
    }
}

/// Proxy settings.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Where to listen; port 0 picks a free one.
    pub listen: SocketAddr,
    pub policy: Policy,
    /// Largest body buffered (signed `aws-chunked` bodies are decoded whole).
    pub max_buffer: usize,
    /// Tags every record (the `job` field), e.g. a session id.
    pub tag: Option<String>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        ProxyConfig {
            listen: SocketAddr::from(([127, 0, 0, 1], 0)),
            policy: Policy::Allow,
            max_buffer: 1 << 30,
            tag: None,
        }
    }
}

/// The access key the application is given; any value works.
pub const CLIENT_ACCESS_KEY: &str = "sonde";
pub const CLIENT_SECRET_KEY: &str = "sonde-proxy-local-secret";

struct Shared {
    client: S3Client,
    log: RequestLog,
    cfg: ProxyConfig,
}

/// A running proxy. Dropping it stops accepting connections.
pub struct Proxy {
    addr: SocketAddr,
    region: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Proxy {
    /// Listens and forwards to `client`'s endpoint, recording into `log`.
    pub async fn start(
        client: S3Client,
        log: RequestLog,
        cfg: ProxyConfig,
    ) -> std::io::Result<Proxy> {
        let listener = TcpListener::bind(cfg.listen).await?;
        let addr = listener.local_addr()?;
        let region = client.config().region.clone();
        let shared = Arc::new(Shared { client, log, cfg });
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let shared = shared.clone();
                tokio::spawn(async move {
                    let svc = service_fn(move |req| handle(shared.clone(), req));
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), svc)
                        .await;
                });
            }
        });
        Ok(Proxy { addr, region, task })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Environment for an application to use the proxy: variables to set and
    /// variables to remove (they would point the SDK elsewhere).
    pub fn env(&self) -> (Vec<(String, String)>, Vec<String>) {
        let url = self.url();
        let region = if self.region.is_empty() || self.region == "auto" {
            "us-east-1".to_string()
        } else {
            self.region.clone()
        };
        let set = [
            ("AWS_ENDPOINT_URL", url.as_str()),
            ("AWS_ENDPOINT_URL_S3", url.as_str()),
            ("AWS_ACCESS_KEY_ID", CLIENT_ACCESS_KEY),
            ("AWS_SECRET_ACCESS_KEY", CLIENT_SECRET_KEY),
            ("AWS_REGION", region.as_str()),
            ("AWS_DEFAULT_REGION", region.as_str()),
            // object_store and others refuse plain HTTP without these
            ("AWS_ALLOW_HTTP", "true"),
            ("AWS_S3_FORCE_PATH_STYLE", "true"),
            ("AWS_VIRTUAL_HOSTED_STYLE_REQUEST", "false"),
            ("AWS_EC2_METADATA_DISABLED", "true"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let unset = [
            "AWS_SESSION_TOKEN",
            "AWS_SECURITY_TOKEN",
            "AWS_PROFILE",
            "AWS_DEFAULT_PROFILE",
            "AWS_ROLE_ARN",
            "AWS_WEB_IDENTITY_TOKEN_FILE",
            "AWS_CONTAINER_CREDENTIALS_FULL_URI",
            "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
            "AWS_CONTAINER_AUTHORIZATION_TOKEN",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        (set, unset)
    }
}

/// Headers not forwarded: hop-by-hop ones, and the client's signature.
const DROP_REQUEST: &[&str] = &[
    "host",
    "authorization",
    "x-amz-date",
    "x-amz-security-token",
    "x-amz-content-sha256",
    "content-length",
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "expect",
];

const DROP_RESPONSE: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Query parameters of a presigned URL's own signature (SigV4 and SigV2).
const PRESIGN_PARAMS: &[&str] = &[
    "X-Amz-Algorithm",
    "X-Amz-Credential",
    "X-Amz-Date",
    "X-Amz-Expires",
    "X-Amz-SignedHeaders",
    "X-Amz-Signature",
    "X-Amz-Security-Token",
    "AWSAccessKeyId",
    "Signature",
    "Expires",
];

fn decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// Bucket and key from the Host header and the path. `<bucket>.localhost`
/// hosts are virtual-hosted style; anything else is path style.
pub fn parse_target(host: &str, path: &str) -> (Option<String>, Option<String>) {
    let host = host.rsplit_once(':').map_or(host, |(h, p)| {
        if p.chars().all(|c| c.is_ascii_digit()) {
            h
        } else {
            host
        }
    });
    let rest = path.strip_prefix('/').unwrap_or(path);
    let non_empty = |s: &str| (!s.is_empty()).then(|| decode(s));
    if let Some(bucket) = host.strip_suffix(".localhost").filter(|b| !b.is_empty()) {
        return (Some(bucket.to_string()), non_empty(rest));
    }
    match rest.split_once('/') {
        Some((b, k)) => (non_empty(b), non_empty(k)),
        None => (non_empty(rest), None),
    }
}

/// Query pairs, decoded (`+` stays a plus: S3 clients encode spaces as %20).
pub fn parse_query(raw: Option<&str>) -> Vec<(String, String)> {
    raw.unwrap_or("")
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (decode(k), decode(v)),
            None => (decode(p), String::new()),
        })
        .collect()
}

fn xml_error(status: u16, code: &str, message: &str) -> Response<Out> {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>{code}</Code><Message>{}</Message><RequestId>sonde-proxy</RequestId></Error>",
        message
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    );
    Response::builder()
        .status(status)
        .header("content-type", "application/xml")
        .header("x-sonde-proxy", "refused")
        .body(full(Bytes::from(body)))
        .expect("static response")
}

fn full(b: Bytes) -> Out {
    Full::new(b).map_err(|never| match never {}).boxed()
}

fn header_value(headers: &http::HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
        .unwrap_or_default()
}

async fn handle(shared: Arc<Shared>, req: Request<Incoming>) -> Result<Response<Out>, Infallible> {
    let (parts, body) = req.into_parts();
    let method = parts.method.as_str().to_string();
    let host = parts
        .headers
        .get("host")
        .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
        .or_else(|| parts.uri.authority().map(|a| a.to_string()))
        .unwrap_or_default();
    let (bucket, key) = parse_target(&host, parts.uri.path());
    let mut query = parse_query(parts.uri.query());
    let presigned = query
        .iter()
        .any(|(k, _)| k == "X-Amz-Signature" || k == "Signature");
    if presigned {
        query.retain(|(k, _)| !PRESIGN_PARAMS.contains(&k.as_str()));
    }
    let app_headers = s3raw::header_pairs(&parts.headers);
    let operation = op::classify(
        &method,
        bucket.as_deref(),
        key.as_deref(),
        &query,
        &app_headers,
    );

    let client = &shared.client;
    let cfg = client.config();
    let (origin, path, upstream_host) =
        client.address(&cfg.endpoint, bucket.as_deref(), key.as_deref());
    let qs = sigv4::canonical_query(&query);
    let url = if qs.is_empty() {
        format!("{origin}{path}")
    } else {
        format!("{origin}{path}?{qs}")
    };

    let mut record = Record {
        id: 0,
        time: Utc::now(),
        endpoint: cfg.endpoint_id.clone(),
        operation: operation.clone(),
        method: method.clone(),
        url: url.clone(),
        bucket: bucket.clone(),
        key: key.clone(),
        query: query.clone(),
        status: None,
        latency_ms: 0.0,
        retries: 0,
        request_id: None,
        host_id: None,
        request_headers: app_headers.iter().map(|(k, v)| Header::new(k, v)).collect(),
        response_headers: Vec::new(),
        request_bytes: None,
        response_bytes: None,
        error_body: None,
        transport_error: None,
        region: Some(cfg.region.clone()),
        job: shared.cfg.tag.clone(),
    };
    if presigned {
        record.request_headers.push(Header::new(
            "x-sonde-note",
            "presigned URL, re-signed by sonde",
        ));
    }

    if let Some(mode) = shared.cfg.policy.refuses(&method, &operation) {
        let msg = format!("sonde refused {operation}: the proxy runs in {mode} mode");
        record.status = Some(403);
        record.error_body = Some(msg.clone());
        record
            .response_headers
            .push(Header::new("x-sonde-proxy", "refused"));
        shared.log.push(record);
        return Ok(xml_error(403, "AccessDenied", &msg));
    }

    // headers to forward, and the payload
    let mut out: Vec<(String, String)> = app_headers
        .iter()
        .filter(|(k, _)| !DROP_REQUEST.contains(&k.to_ascii_lowercase().as_str()))
        .cloned()
        .collect();
    let content_sha = header_value(&parts.headers, "x-amz-content-sha256");
    let length: Option<u64> = header_value(&parts.headers, "content-length").parse().ok();
    let chunked_te = header_value(&parts.headers, "transfer-encoding")
        .to_ascii_lowercase()
        .contains("chunked");
    let has_body = length.is_some_and(|l| l > 0) || chunked_te;

    let (payload_hash, upstream_body, sent_len): (String, reqwest::Body, Option<u64>) =
        if chunked::is_signed_streaming(&content_sha) {
            let raw = match Limited::new(body, shared.cfg.max_buffer).collect().await {
                Ok(c) => c.to_bytes(),
                Err(e) => {
                    let msg = format!("sonde could not read the request body: {e}");
                    record.transport_error = Some(msg.clone());
                    shared.log.push(record);
                    return Ok(xml_error(400, "IncompleteBody", &msg));
                }
            };
            let decoded = match chunked::decode(&raw) {
                Ok(d) => d,
                Err(e) => {
                    record.transport_error = Some(e.clone());
                    shared.log.push(record);
                    return Ok(xml_error(400, "InvalidRequest", &e));
                }
            };
            out.retain(|(k, _)| {
                let k = k.to_ascii_lowercase();
                k != "x-amz-decoded-content-length" && k != "x-amz-trailer"
            });
            for (k, v) in out.iter_mut() {
                if k.eq_ignore_ascii_case("content-encoding") {
                    *v = v
                        .split(',')
                        .map(str::trim)
                        .filter(|e| !e.eq_ignore_ascii_case("aws-chunked"))
                        .collect::<Vec<_>>()
                        .join(",");
                }
            }
            out.retain(|(k, v)| !(k.eq_ignore_ascii_case("content-encoding") && v.is_empty()));
            for (k, v) in decoded.trailers {
                if !out.iter().any(|(n, _)| n.eq_ignore_ascii_case(&k)) {
                    out.push((k, v));
                }
            }
            let hash = sigv4::sha256_hex(&decoded.payload);
            let n = decoded.payload.len() as u64;
            (hash, reqwest::Body::from(decoded.payload), Some(n))
        } else if !has_body {
            let hash = if content_sha.is_empty() {
                sigv4::EMPTY_SHA256.to_string()
            } else {
                content_sha.clone()
            };
            (hash, reqwest::Body::from(Bytes::new()), Some(0))
        } else {
            let hash = if content_sha.is_empty() {
                sigv4::UNSIGNED_PAYLOAD.to_string()
            } else {
                content_sha.clone()
            };
            match length {
                Some(n) => {
                    let stream = body.into_data_stream();
                    (hash, reqwest::Body::wrap_stream(stream), Some(n))
                }
                None => {
                    // chunked transfer without a length: S3 wants one
                    let raw = match Limited::new(body, shared.cfg.max_buffer).collect().await {
                        Ok(c) => c.to_bytes(),
                        Err(e) => {
                            let msg = format!("sonde could not read the request body: {e}");
                            record.transport_error = Some(msg.clone());
                            shared.log.push(record);
                            return Ok(xml_error(400, "IncompleteBody", &msg));
                        }
                    };
                    let n = raw.len() as u64;
                    (hash, reqwest::Body::from(raw), Some(n))
                }
            }
        };
    record.request_bytes = sent_len;

    out.push(("host".into(), upstream_host));
    match client.credentials().await {
        Ok(Some(creds)) => {
            sigv4::sign_headers(
                &mut out,
                &method,
                &path,
                &query,
                &payload_hash,
                &cfg.region,
                "s3",
                Utc::now(),
                &creds,
            );
        }
        Ok(None) => {}
        Err(e) => {
            let msg = format!("sonde has no credentials for {}: {e}", cfg.endpoint_id);
            record.transport_error = Some(msg.clone());
            shared.log.push(record);
            return Ok(xml_error(500, "SondeCredentials", &msg));
        }
    }

    let mut rb = client.http().request(parts.method.clone(), &url);
    for (k, v) in &out {
        if !k.eq_ignore_ascii_case("host") {
            rb = rb.header(k.as_str(), v.as_str());
        }
    }
    if let Some(n) = sent_len
        && (n > 0 || method == "PUT" || method == "POST")
    {
        rb = rb.header("content-length", n.to_string());
    }
    let start = Instant::now();
    let sent = rb.body(upstream_body).send().await;
    record.latency_ms = start.elapsed().as_secs_f64() * 1000.0;
    let resp = match sent {
        Ok(r) => r,
        Err(e) => {
            let mut msg = e.to_string();
            let mut src = std::error::Error::source(&e);
            while let Some(s) = src {
                msg.push_str(": ");
                msg.push_str(&s.to_string());
                src = s.source();
            }
            record.transport_error = Some(msg.clone());
            shared.log.push(record);
            return Ok(xml_error(502, "SondeUpstreamError", &msg));
        }
    };

    let status = resp.status();
    let headers = s3raw::header_pairs(resp.headers());
    record.status = Some(status.as_u16());
    s3raw::fill_ids(&mut record, &headers);
    record.response_headers = headers.iter().map(|(k, v)| Header::new(k, v)).collect();
    let mut builder = Response::builder().status(status);
    for (k, v) in resp.headers() {
        if !DROP_RESPONSE.contains(&k.as_str()) {
            builder = builder.header(k, v);
        }
    }

    if status.as_u16() >= 400 && method != "HEAD" {
        let body = resp.bytes().await.unwrap_or_default();
        record.response_bytes = Some(body.len() as u64);
        if !body.is_empty() {
            record.error_body = Some(String::from_utf8_lossy(&body).into_owned());
        }
        shared.log.push(record);
        return Ok(builder.body(full(body)).expect("valid response"));
    }
    record.response_bytes = resp.content_length();
    shared.log.push(record);
    let stream = resp
        .bytes_stream()
        .map_ok(Frame::data)
        .map_err(std::io::Error::other);
    Ok(builder
        .body(BodyExt::boxed(StreamBody::new(stream)))
        .expect("valid response"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        assert_eq!(parse_target("127.0.0.1:9000", "/"), (None, None));
        assert_eq!(
            parse_target("127.0.0.1:9000", "/b/"),
            (Some("b".into()), None)
        );
        assert_eq!(
            parse_target("127.0.0.1:9000", "/b/a%20b/c+d"),
            (Some("b".into()), Some("a b/c+d".into()))
        );
        assert_eq!(
            parse_target("data.localhost:9000", "/x/y"),
            (Some("data".into()), Some("x/y".into()))
        );
        assert_eq!(
            parse_target("data.localhost:9000", "/"),
            (Some("data".into()), None)
        );
    }

    #[test]
    fn queries() {
        assert_eq!(
            parse_query(Some("list-type=2&prefix=a%2Fb+c&uploads")),
            vec![
                ("list-type".into(), "2".into()),
                ("prefix".into(), "a/b+c".into()),
                ("uploads".into(), String::new()),
            ]
        );
        assert!(parse_query(None).is_empty());
    }

    #[test]
    fn policies() {
        assert_eq!(Policy::Allow.refuses("DELETE", "DeleteObject"), None);
        assert_eq!(
            Policy::NoDelete.refuses("DELETE", "DeleteObject"),
            Some("no-delete")
        );
        assert_eq!(Policy::NoDelete.refuses("PUT", "PutObject"), None);
        assert_eq!(
            Policy::ReadOnly.refuses("PUT", "PutObject"),
            Some("read-only")
        );
        assert_eq!(Policy::ReadOnly.refuses("GET", "GetObject"), None);
    }
}
