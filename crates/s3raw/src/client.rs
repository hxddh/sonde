//! The client: builds, signs, sends, retries and logs one request.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use chrono::Utc;
use reqlog::{Header, Record, RequestLog};
use serde::{Deserialize, Serialize};

use crate::credentials::{Credentials, CredentialsProvider};
use crate::error::{ErrorKind, S3Error};
use crate::sigv4;
use crate::throttle::Throttle;

/// How buckets appear in URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Addressing {
    /// `https://host/bucket/key`
    Path,
    /// `https://bucket.host/key`
    Virtual,
    /// Virtual-hosted when the bucket name and the host allow it.
    #[default]
    Auto,
}

/// Retries of throttled and failed requests.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryPolicy {
    /// Attempts in total (1 = no retry).
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 5,
            base_delay_ms: 100,
            max_delay_ms: 20_000,
        }
    }
}

impl RetryPolicy {
    pub fn none() -> Self {
        RetryPolicy {
            max_attempts: 1,
            ..Default::default()
        }
    }

    /// Exponential backoff with full jitter: `rand(0, min(max, base·2^n))`.
    pub fn delay(&self, attempt: u32) -> Duration {
        let cap = self
            .base_delay_ms
            .saturating_mul(1u64 << attempt.min(20))
            .min(self.max_delay_ms);
        let ms = if cap == 0 {
            0
        } else {
            rand::random::<u64>() % (cap + 1)
        };
        Duration::from_millis(ms.max(self.base_delay_ms.min(cap) / 2))
    }
}

/// What a client talks to.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// The endpoint's id, written on every record.
    pub endpoint_id: String,
    /// `https://s3.us-east-1.amazonaws.com`, `http://127.0.0.1:9000`, ...
    pub endpoint: url::Url,
    pub region: String,
    pub addressing: Addressing,
    /// Extra trusted CA certificates (PEM).
    pub ca_pem: Option<Vec<u8>>,
    pub timeout: Duration,
    pub retry: RetryPolicy,
    /// Send `UNSIGNED-PAYLOAD` instead of hashing bodies.
    pub unsigned_payload: bool,
    pub user_agent: String,
}

impl ClientConfig {
    pub fn new(endpoint_id: &str, endpoint: &str, region: &str) -> Result<Self, S3Error> {
        let endpoint = url::Url::parse(endpoint)
            .map_err(|e| S3Error::config(format!("endpoint URL {endpoint:?}: {e}")))?;
        if !matches!(endpoint.scheme(), "http" | "https") {
            return Err(S3Error::config(format!(
                "endpoint scheme must be http or https, got {}",
                endpoint.scheme()
            )));
        }
        Ok(ClientConfig {
            endpoint_id: endpoint_id.into(),
            endpoint,
            region: region.into(),
            addressing: Addressing::Auto,
            ca_pem: None,
            timeout: Duration::from_secs(60),
            retry: RetryPolicy::default(),
            unsigned_payload: false,
            user_agent: format!("sonde/{}", env!("CARGO_PKG_VERSION")),
        })
    }
}

/// A request body.
#[derive(Debug, Clone, Default)]
pub enum Body {
    #[default]
    Empty,
    Bytes(Bytes),
    /// `aws-chunked` with an unsigned trailer carrying a checksum
    /// (`STREAMING-UNSIGNED-PAYLOAD-TRAILER`).
    ChunkedTrailer {
        data: Bytes,
        trailer_name: String,
        trailer_value: String,
    },
}

impl Body {
    pub fn len(&self) -> u64 {
        match self {
            Body::Empty => 0,
            Body::Bytes(b) => b.len() as u64,
            Body::ChunkedTrailer { data, .. } => data.len() as u64,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl From<Bytes> for Body {
    fn from(b: Bytes) -> Self {
        Body::Bytes(b)
    }
}
impl From<Vec<u8>> for Body {
    fn from(b: Vec<u8>) -> Self {
        Body::Bytes(b.into())
    }
}
impl From<&'static str> for Body {
    fn from(b: &'static str) -> Self {
        Body::Bytes(Bytes::from_static(b.as_bytes()))
    }
}
impl From<String> for Body {
    fn from(b: String) -> Self {
        Body::Bytes(b.into())
    }
}

/// One S3 request.
#[derive(Debug, Clone)]
pub struct S3Request {
    pub operation: String,
    pub method: http::Method,
    pub bucket: Option<String>,
    pub key: Option<String>,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Body,
    /// The service to sign for (`s3`, `sts`).
    pub service: String,
    /// Overrides the endpoint (STS).
    pub endpoint: Option<url::Url>,
    /// Skip retries for this request (probes want the first answer).
    pub no_retry: bool,
}

impl S3Request {
    pub fn new(operation: &str, method: http::Method) -> Self {
        S3Request {
            operation: operation.into(),
            method,
            bucket: None,
            key: None,
            query: Vec::new(),
            headers: Vec::new(),
            body: Body::Empty,
            service: "s3".into(),
            endpoint: None,
            no_retry: false,
        }
    }
    pub fn bucket(mut self, b: &str) -> Self {
        self.bucket = Some(b.into());
        self
    }
    pub fn key(mut self, k: &str) -> Self {
        self.key = Some(k.into());
        self
    }
    pub fn query(mut self, k: &str, v: impl Into<String>) -> Self {
        self.query.push((k.into(), v.into()));
        self
    }
    pub fn query_opt(self, k: &str, v: Option<impl Into<String>>) -> Self {
        match v {
            Some(v) => self.query(k, v),
            None => self,
        }
    }
    pub fn header(mut self, k: &str, v: impl Into<String>) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }
    pub fn header_opt(self, k: &str, v: Option<impl Into<String>>) -> Self {
        match v {
            Some(v) => self.header(k, v),
            None => self,
        }
    }
    pub fn body(mut self, b: impl Into<Body>) -> Self {
        self.body = b.into();
        self
    }
    pub fn no_retry(mut self) -> Self {
        self.no_retry = true;
        self
    }
}

/// A response, whatever its status.
#[derive(Debug, Clone)]
pub struct S3Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
    pub record_id: u64,
    pub request_id: Option<String>,
    pub host_id: Option<String>,
    pub latency: Duration,
    pub retries: u32,
}

impl S3Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
    /// The error this response stands for (`None` when it succeeded).
    pub fn error(&self) -> Option<S3Error> {
        if self.is_success() {
            None
        } else {
            Some(S3Error::from_response(
                self.status,
                &self.body,
                self.request_id.clone(),
                self.host_id.clone(),
                self.record_id,
            ))
        }
    }
    /// `Err` for a non-2xx status.
    pub fn ok(self) -> Result<S3Response, S3Error> {
        match self.error() {
            Some(e) => Err(e),
            None => Ok(self),
        }
    }
}

/// A raw S3 client. Cheap to clone; clones share the HTTP pool and log.
#[derive(Clone)]
pub struct S3Client {
    cfg: Arc<ClientConfig>,
    http: reqwest::Client,
    creds: Arc<dyn CredentialsProvider>,
    log: RequestLog,
    throttle: Option<Arc<Throttle>>,
    job: Option<String>,
    /// Seconds added to the signing clock (probes of clock skew).
    skew: Arc<AtomicI64>,
}

impl std::fmt::Debug for S3Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Client")
            .field("endpoint", &self.cfg.endpoint.as_str())
            .field("credentials", &self.creds.describe())
            .finish()
    }
}

/// Whether `bucket` can be a DNS label under the endpoint host.
pub fn dns_compatible(bucket: &str, https: bool) -> bool {
    let len_ok = (3..=63).contains(&bucket.len());
    let chars_ok = bucket
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.');
    let ends_ok = !bucket.starts_with(['-', '.']) && !bucket.ends_with(['-', '.']);
    let dots_ok = !https || !bucket.contains('.');
    len_ok && chars_ok && ends_ok && dots_ok && !bucket.contains("..")
}

fn is_ip_or_local(host: &str) -> bool {
    host == "localhost"
        || host.parse::<std::net::IpAddr>().is_ok()
        || host.starts_with('[')
        || !host.contains('.')
}

const REQUEST_ID_HEADERS: &[&str] = &[
    "x-amz-request-id",
    "x-oss-request-id",
    "x-guploader-uploadid",
    "x-request-id",
    "cf-ray",
    "x-bz-request-id",
];

impl S3Client {
    pub fn new(
        cfg: ClientConfig,
        creds: Arc<dyn CredentialsProvider>,
        log: RequestLog,
    ) -> Result<Self, S3Error> {
        let mut builder = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(cfg.user_agent.clone())
            .pool_max_idle_per_host(32);
        if let Some(pem) = &cfg.ca_pem {
            let certs = reqwest::Certificate::from_pem_bundle(pem)
                .map_err(|e| S3Error::config(format!("CA certificate: {e}")))?;
            if certs.is_empty() {
                return Err(S3Error::config(
                    "CA certificate file holds no PEM certificate",
                ));
            }
            for c in certs {
                builder = builder.add_root_certificate(c);
            }
        }
        let http = builder
            .build()
            .map_err(|e| S3Error::config(format!("HTTP client: {e}")))?;
        Ok(S3Client {
            cfg: Arc::new(cfg),
            http,
            creds,
            log,
            throttle: None,
            job: None,
            skew: Arc::new(AtomicI64::new(0)),
        })
    }

    pub fn config(&self) -> &ClientConfig {
        &self.cfg
    }

    pub fn log(&self) -> &RequestLog {
        &self.log
    }

    /// The HTTP client (its pool, CA roots and timeouts), for callers that
    /// stream bodies themselves.
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn credentials_provider(&self) -> &Arc<dyn CredentialsProvider> {
        &self.creds
    }

    /// A clone whose requests share `throttle` and are tagged with `job`.
    pub fn for_job(&self, job: Option<String>, throttle: Option<Arc<Throttle>>) -> Self {
        let mut c = self.clone();
        c.job = job;
        c.throttle = throttle;
        c
    }

    /// A clone that records into another log.
    pub fn with_log(&self, log: RequestLog) -> Self {
        let mut c = self.clone();
        c.log = log;
        c
    }

    /// A clone with another retry policy.
    pub fn with_retry(&self, retry: RetryPolicy) -> Self {
        let mut cfg = (*self.cfg).clone();
        cfg.retry = retry;
        let mut c = self.clone();
        c.cfg = Arc::new(cfg);
        c
    }

    /// A clone that addresses buckets another way.
    pub fn with_addressing(&self, addressing: Addressing) -> Self {
        let mut cfg = (*self.cfg).clone();
        cfg.addressing = addressing;
        let mut c = self.clone();
        c.cfg = Arc::new(cfg);
        c
    }

    /// A clone whose signing clock is shifted by `secs` (clock-skew probes).
    pub fn with_clock_skew(&self, secs: i64) -> Self {
        let mut c = self.clone();
        c.skew = Arc::new(AtomicI64::new(secs));
        c
    }

    /// GETs (or HEADs) a URL as is, unsigned: presigned URLs. Logged like
    /// every request.
    pub async fn send_url(
        &self,
        operation: &str,
        method: http::Method,
        url: &str,
    ) -> Result<S3Response, S3Error> {
        let parsed = url::Url::parse(url).map_err(|e| S3Error::config(format!("URL: {e}")))?;
        let req = S3Request::new(operation, method.clone());
        let start = Instant::now();
        let sent = self.http.request(method, parsed).send().await;
        let latency = start.elapsed();
        let mut record = self.record(&req, url, &[], 0, latency);
        record.query = url::Url::parse(url)
            .map(|u| {
                u.query_pairs()
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect()
            })
            .unwrap_or_default();
        match sent {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let headers = header_pairs(resp.headers());
                let body = resp.bytes().await.unwrap_or_default();
                record.status = Some(status);
                record.response_bytes = Some(body.len() as u64);
                fill_ids(&mut record, &headers);
                record.response_headers = headers.iter().map(|(k, v)| Header::new(k, v)).collect();
                if !(200..300).contains(&status) && !body.is_empty() {
                    record.error_body = Some(String::from_utf8_lossy(&body).to_string());
                }
                let (request_id, host_id) = (record.request_id.clone(), record.host_id.clone());
                let record_id = self.log.push(record);
                Ok(S3Response {
                    status,
                    headers,
                    body,
                    record_id,
                    request_id,
                    host_id,
                    latency,
                    retries: 0,
                })
            }
            Err(e) => {
                record.transport_error = Some(e.to_string());
                let id = self.log.push(record);
                let mut err = S3Error::new(ErrorKind::Transport, e.to_string());
                err.record_id = Some(id);
                Err(err)
            }
        }
    }

    pub async fn credentials(&self) -> Result<Option<Credentials>, S3Error> {
        self.creds
            .credentials()
            .await
            .map_err(|e| S3Error::new(ErrorKind::Credentials, e))
    }

    fn path_style(&self, bucket: &str) -> bool {
        match self.cfg.addressing {
            Addressing::Path => true,
            Addressing::Virtual => false,
            Addressing::Auto => {
                let host = self.cfg.endpoint.host_str().unwrap_or("");
                is_ip_or_local(host)
                    || !dns_compatible(bucket, self.cfg.endpoint.scheme() == "https")
            }
        }
    }

    /// The scheme://host[:port], the canonical (encoded) path, and the host header.
    pub fn address(
        &self,
        endpoint: &url::Url,
        bucket: Option<&str>,
        key: Option<&str>,
    ) -> (String, String, String) {
        let scheme = endpoint.scheme();
        let mut host = endpoint.host_str().unwrap_or("").to_string();
        let port = endpoint.port().map(|p| format!(":{p}")).unwrap_or_default();
        let base = endpoint.path().trim_end_matches('/').to_string();
        let mut path = base;
        if let Some(b) = bucket {
            if self.path_style(b) {
                path.push('/');
                path.push_str(&sigv4::uri_encode(b, false));
            } else {
                host = format!("{b}.{host}");
            }
        }
        path.push('/');
        if let Some(k) = key {
            path.push_str(&encode_key(k));
        }
        let host_header = format!("{host}{port}");
        (format!("{scheme}://{host_header}"), path, host_header)
    }

    /// A presigned URL for `method` on `bucket/key`.
    pub async fn presign(
        &self,
        method: &str,
        bucket: &str,
        key: &str,
        expires_secs: u64,
        extra_query: &[(String, String)],
    ) -> Result<String, S3Error> {
        let creds = self
            .credentials()
            .await?
            .ok_or_else(|| S3Error::config("presigning needs credentials"))?;
        let (origin, path, host) = self.address(&self.cfg.endpoint, Some(bucket), Some(key));
        let time = Utc::now() + chrono::Duration::seconds(self.skew.load(Ordering::Relaxed));
        let (query, _) = sigv4::presign(
            method,
            &path,
            &host,
            extra_query,
            &self.cfg.region,
            "s3",
            time,
            expires_secs,
            &creds,
        );
        Ok(format!("{origin}{path}?{}", sigv4::canonical_query(&query)))
    }

    /// Sends a request; any status is an `Ok`. Retries throttled and failed
    /// attempts by the policy; each attempt is a record of its own.
    pub async fn send_raw(&self, req: S3Request) -> Result<S3Response, S3Error> {
        let attempts = if req.no_retry {
            1
        } else {
            self.cfg.retry.max_attempts.max(1)
        };
        let mut attempt = 0;
        loop {
            let result = self.attempt(&req, attempt).await;
            let retryable = match &result {
                Ok(r) => is_retryable_status(r),
                Err(e) => e.kind == ErrorKind::Transport,
            };
            if !retryable || attempt + 1 >= attempts {
                return result;
            }
            let mut delay = self.cfg.retry.delay(attempt);
            if let Ok(r) = &result
                && let Some(secs) = r.header("retry-after").and_then(|v| v.parse::<u64>().ok())
            {
                delay = delay.max(Duration::from_secs(secs.min(30)));
            }
            tracing::debug!(op = %req.operation, attempt, ?delay, "retrying");
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }

    /// Sends a request; a non-2xx status is an `Err`.
    pub async fn send(&self, req: S3Request) -> Result<S3Response, S3Error> {
        self.send_raw(req).await?.ok()
    }

    /// Sends a GET and returns the response with its body unread, for
    /// streaming downloads. Logged when the headers arrive.
    pub async fn send_stream(&self, req: S3Request) -> Result<(reqwest::Response, u64), S3Error> {
        let _permit = match &self.throttle {
            Some(t) => Some(t.acquire().await),
            None => None,
        };
        let Prepared {
            request,
            url,
            headers: sent_headers,
        } = self.prepare(&req).await?;
        let prepared = Meta {
            url,
            headers: sent_headers,
        };
        let start = Instant::now();
        let sent = self.http.execute(request).await;
        let latency = start.elapsed();
        match sent {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let headers = header_pairs(resp.headers());
                let mut record = self.record(&req, &prepared.url, &prepared.headers, 0, latency);
                record.status = Some(status);
                record.response_bytes = resp.content_length();
                fill_ids(&mut record, &headers);
                record.response_headers = headers.iter().map(|(k, v)| Header::new(k, v)).collect();
                if !(200..300).contains(&status) {
                    let rid = record.request_id.clone();
                    let hid = record.host_id.clone();
                    let body = resp.bytes().await.unwrap_or_default();
                    record.error_body = Some(String::from_utf8_lossy(&body).to_string());
                    let id = self.log.push(record);
                    return Err(S3Error::from_response(status, &body, rid, hid, id));
                }
                let id = self.log.push(record);
                Ok((resp, id))
            }
            Err(e) => Err(self.transport_error(&req, &prepared, 0, latency, e)),
        }
    }

    async fn attempt(&self, req: &S3Request, attempt: u32) -> Result<S3Response, S3Error> {
        let _permit = match &self.throttle {
            Some(t) => Some(t.acquire().await),
            None => None,
        };
        let Prepared {
            request,
            url,
            headers: sent_headers,
        } = self.prepare(req).await?;
        let prepared = Meta {
            url,
            headers: sent_headers,
        };
        let start = Instant::now();
        let sent = self.http.execute(request).await;
        match sent {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let headers = header_pairs(resp.headers());
                let body = match resp.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        return Err(self.transport_error(
                            req,
                            &prepared,
                            attempt,
                            start.elapsed(),
                            e,
                        ));
                    }
                };
                let latency = start.elapsed();
                let mut record =
                    self.record(req, &prepared.url, &prepared.headers, attempt, latency);
                record.status = Some(status);
                record.response_bytes = Some(body.len() as u64);
                fill_ids(&mut record, &headers);
                record.response_headers = headers.iter().map(|(k, v)| Header::new(k, v)).collect();
                if !(200..300).contains(&status) && !body.is_empty() {
                    record.error_body = Some(String::from_utf8_lossy(&body).to_string());
                }
                let request_id = record.request_id.clone();
                let host_id = record.host_id.clone();
                let record_id = self.log.push(record);
                Ok(S3Response {
                    status,
                    headers,
                    body,
                    record_id,
                    request_id,
                    host_id,
                    latency,
                    retries: attempt,
                })
            }
            Err(e) => Err(self.transport_error(req, &prepared, attempt, start.elapsed(), e)),
        }
    }

    fn transport_error(
        &self,
        req: &S3Request,
        prepared: &Meta,
        attempt: u32,
        latency: Duration,
        e: reqwest::Error,
    ) -> S3Error {
        let mut msg = e.to_string();
        let mut src: Option<&dyn std::error::Error> = std::error::Error::source(&e);
        while let Some(s) = src {
            msg.push_str(": ");
            msg.push_str(&s.to_string());
            src = s.source();
        }
        let mut record = self.record(req, &prepared.url, &prepared.headers, attempt, latency);
        record.transport_error = Some(msg.clone());
        let id = self.log.push(record);
        let mut err = S3Error::new(ErrorKind::Transport, msg);
        err.record_id = Some(id);
        err
    }

    fn record(
        &self,
        req: &S3Request,
        url: &str,
        headers: &[(String, String)],
        attempt: u32,
        latency: Duration,
    ) -> Record {
        Record {
            id: 0,
            time: Utc::now(),
            endpoint: self.cfg.endpoint_id.clone(),
            operation: req.operation.clone(),
            method: req.method.to_string(),
            url: url.to_string(),
            bucket: req.bucket.clone(),
            key: req.key.clone(),
            query: req.query.clone(),
            status: None,
            latency_ms: latency.as_secs_f64() * 1000.0,
            retries: attempt,
            request_id: None,
            host_id: None,
            request_headers: headers.iter().map(|(k, v)| Header::new(k, v)).collect(),
            response_headers: Vec::new(),
            request_bytes: Some(req.body.len()),
            response_bytes: None,
            error_body: None,
            request_body: match &req.body {
                Body::Bytes(b) if reqlog::is_control_operation(&req.operation) => {
                    Some(String::from_utf8_lossy(b).into_owned())
                }
                _ => None,
            },
            transport_error: None,
            region: Some(self.cfg.region.clone()),
            job: self.job.clone(),
        }
    }

    async fn prepare(&self, req: &S3Request) -> Result<Prepared, S3Error> {
        let endpoint = req
            .endpoint
            .clone()
            .unwrap_or_else(|| self.cfg.endpoint.clone());
        let (origin, path, host) =
            self.address(&endpoint, req.bucket.as_deref(), req.key.as_deref());
        let query = sigv4::canonical_query(&req.query);
        let url = if query.is_empty() {
            format!("{origin}{path}")
        } else {
            format!("{origin}{path}?{query}")
        };

        let (payload_hash, body_bytes, mut extra): (String, Bytes, Vec<(String, String)>) =
            match &req.body {
                Body::Empty => {
                    let h = if self.cfg.unsigned_payload {
                        sigv4::UNSIGNED_PAYLOAD.to_string()
                    } else {
                        sigv4::EMPTY_SHA256.to_string()
                    };
                    (h, Bytes::new(), vec![])
                }
                Body::Bytes(b) => {
                    let h = if self.cfg.unsigned_payload {
                        sigv4::UNSIGNED_PAYLOAD.to_string()
                    } else {
                        sigv4::sha256_hex(b)
                    };
                    (h, b.clone(), vec![])
                }
                Body::ChunkedTrailer {
                    data,
                    trailer_name,
                    trailer_value,
                } => {
                    let mut out = Vec::with_capacity(data.len() + 128);
                    if !data.is_empty() {
                        out.extend_from_slice(format!("{:x}\r\n", data.len()).as_bytes());
                        out.extend_from_slice(data);
                        out.extend_from_slice(b"\r\n");
                    }
                    out.extend_from_slice(b"0\r\n");
                    out.extend_from_slice(
                        format!(
                            "{}:{}\r\n\r\n",
                            trailer_name.to_ascii_lowercase(),
                            trailer_value
                        )
                        .as_bytes(),
                    );
                    (
                        sigv4::STREAMING_UNSIGNED_TRAILER.to_string(),
                        Bytes::from(out),
                        vec![
                            ("content-encoding".into(), "aws-chunked".into()),
                            (
                                "x-amz-decoded-content-length".into(),
                                data.len().to_string(),
                            ),
                            ("x-amz-trailer".into(), trailer_name.to_ascii_lowercase()),
                        ],
                    )
                }
            };

        let creds = self.credentials().await?;
        let time = Utc::now() + chrono::Duration::seconds(self.skew.load(Ordering::Relaxed));
        let mut headers: Vec<(String, String)> = req.headers.clone();
        headers.append(&mut extra);
        headers.push(("host".into(), host.clone()));
        if let Some(c) = &creds {
            let region = if req.service == "sts" && self.cfg.region.is_empty() {
                "us-east-1"
            } else {
                &self.cfg.region
            };
            sigv4::sign_headers(
                &mut headers,
                req.method.as_str(),
                &path,
                &req.query,
                &payload_hash,
                region,
                &req.service,
                time,
                c,
            );
        }

        let mut rb = self.http.request(req.method.clone(), &url);
        for (k, v) in &headers {
            if k.eq_ignore_ascii_case("host") {
                continue;
            }
            rb = rb.header(k.as_str(), v.as_str());
        }
        if body_bytes.is_empty() && matches!(req.method, http::Method::PUT | http::Method::POST) {
            // hyper sends no Content-Length for an empty body; S3 wants one
            rb = rb.header("content-length", "0");
            headers.push(("content-length".into(), "0".into()));
        }
        if !body_bytes.is_empty() || matches!(req.method, http::Method::PUT | http::Method::POST) {
            rb = rb.body(body_bytes);
        }
        let request = rb
            .build()
            .map_err(|e| S3Error::config(format!("request: {e}")))?;
        Ok(Prepared {
            request,
            url,
            headers,
        })
    }
}

struct Prepared {
    request: reqwest::Request,
    url: String,
    headers: Vec<(String, String)>,
}

struct Meta {
    url: String,
    headers: Vec<(String, String)>,
}

fn is_retryable_status(r: &S3Response) -> bool {
    matches!(r.status, 500 | 502 | 503 | 504 | 429)
        || (r.status == 400 && r.text().contains("<Code>RequestTimeout</Code>"))
}

/// Header (name, value) pairs of a map, values read lossily.
pub fn header_pairs(map: &http::HeaderMap) -> Vec<(String, String)> {
    map.iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).to_string(),
            )
        })
        .collect()
}

/// Fills a record's request id and host id from response headers.
pub fn fill_ids(record: &mut Record, headers: &[(String, String)]) {
    for name in REQUEST_ID_HEADERS {
        if let Some((_, v)) = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
            record.request_id = Some(v.clone());
            break;
        }
    }
    record.host_id = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-amz-id-2"))
        .map(|(_, v)| v.clone());
}

/// Encodes a key for the path: each byte outside the unreserved set, and
/// the dot segments `.` and `..` (which URL parsers would collapse).
pub fn encode_key(key: &str) -> String {
    key.split('/')
        .map(|seg| match seg {
            "." => "%2E".to_string(),
            ".." => "%2E%2E".to_string(),
            s => sigv4::uri_encode(s, false),
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::StaticProvider;

    fn client(endpoint: &str, addressing: Addressing) -> S3Client {
        let mut cfg = ClientConfig::new("t", endpoint, "us-east-1").unwrap();
        cfg.addressing = addressing;
        S3Client::new(cfg, Arc::new(StaticProvider(None)), RequestLog::new(10)).unwrap()
    }

    #[test]
    fn addressing_styles() {
        let c = client("https://s3.us-east-1.amazonaws.com", Addressing::Auto);
        let ep = c.config().endpoint.clone();
        assert_eq!(
            c.address(&ep, Some("my-bucket"), Some("a b/c")),
            (
                "https://my-bucket.s3.us-east-1.amazonaws.com".into(),
                "/a%20b/c".into(),
                "my-bucket.s3.us-east-1.amazonaws.com".into()
            )
        );
        // dots over https fall back to path style
        assert_eq!(
            c.address(&ep, Some("my.bucket"), Some("k")).1,
            "/my.bucket/k"
        );
        let c = client("http://127.0.0.1:9000", Addressing::Auto);
        let ep = c.config().endpoint.clone();
        assert_eq!(
            c.address(&ep, Some("b"), Some("k")),
            (
                "http://127.0.0.1:9000".into(),
                "/b/k".into(),
                "127.0.0.1:9000".into()
            )
        );
        let c = client("http://minio.local:9000", Addressing::Virtual);
        let ep = c.config().endpoint.clone();
        assert_eq!(
            c.address(&ep, Some("b"), None),
            (
                "http://b.minio.local:9000".into(),
                "/".into(),
                "b.minio.local:9000".into()
            )
        );
        let c = client("https://gw.example.com/s3/", Addressing::Path);
        let ep = c.config().endpoint.clone();
        assert_eq!(c.address(&ep, Some("b"), Some("k")).1, "/s3/b/k");
        assert_eq!(c.address(&ep, None, None).1, "/s3/");
    }

    #[test]
    fn key_encoding() {
        assert_eq!(encode_key("a/./b/../c"), "a/%2E/b/%2E%2E/c");
        assert_eq!(encode_key("日本/x+y=z"), "%E6%97%A5%E6%9C%AC/x%2By%3Dz");
        assert_eq!(encode_key("a//b"), "a//b");
    }

    #[test]
    fn dns_names() {
        assert!(dns_compatible("abc", true));
        assert!(!dns_compatible("ABC", true));
        assert!(!dns_compatible("a.b", true));
        assert!(dns_compatible("a.b.c", false));
        assert!(!dns_compatible("ab", false));
    }

    #[test]
    fn backoff_bounds() {
        let p = RetryPolicy {
            max_attempts: 5,
            base_delay_ms: 100,
            max_delay_ms: 1000,
        };
        for a in 0..10 {
            assert!(p.delay(a) <= Duration::from_millis(1000));
        }
    }
}
