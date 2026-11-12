//! A local S3 endpoint for recording what an application does.
//!
//! The application talks to `http://127.0.0.1:<port>` with any credentials.
//! Each request is parsed back into bucket, key and query, re-signed with the
//! credentials of the endpoint sonde is configured for (they never leave
//! Rust, and the application never sees them), forwarded, and recorded in a
//! [`RequestLog`] with the headers the application sent. Responses stream
//! back unchanged.

pub mod callers;
pub mod chunked;
pub mod episode;
pub mod fault;
pub mod fixes;
pub mod import;
pub mod logs;
pub mod op;
pub mod otlp;
pub mod phrase;
pub mod profile;
pub mod program;
pub mod project;
pub mod replay;
pub mod report;
pub mod session;
pub mod share;
pub mod signals;
pub mod summary;
pub mod trial;
pub mod view;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use chrono::Utc;
use futures::{StreamExt, TryStreamExt};
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

/// What the proxy lets through. A recording takes [`Policy::NoDelete`]
/// unless told otherwise (`--allow-writes`, `[record] policy`, the app's
/// setting); a proxy built by hand ([`ProxyConfig::default`]) forwards all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Policy {
    /// Everything.
    Allow,
    /// Nothing that removes or replaces data: deletes, aborts, lifecycle
    /// rules, bucket policies, versioning suspended, and a write over an
    /// object the recording did not write itself ([`Written`]).
    #[default]
    NoDelete,
    /// GET and HEAD only.
    ReadOnly,
}

impl Policy {
    /// By its name, as the flags, `sonde.toml` and the app say it.
    pub fn named(name: &str) -> Option<Policy> {
        match name {
            "allow" => Some(Policy::Allow),
            "no-delete" => Some(Policy::NoDelete),
            "read-only" => Some(Policy::ReadOnly),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Policy::Allow => "allow",
            Policy::NoDelete => "no-delete",
            Policy::ReadOnly => "read-only",
        }
    }

    /// The one of two that lets less through.
    pub fn stricter(self, other: Policy) -> Policy {
        self.max(other)
    }

    /// Why `op` is refused, if it is. No-delete goes by the method and the
    /// query as well as the name: a request the proxy cannot name is forwarded
    /// as it came, and the store may read it as a delete.
    pub fn refuses(
        self,
        method: &str,
        op: &str,
        query: &[(String, String)],
    ) -> Option<&'static str> {
        let q = |n: &str| query.iter().any(|(k, _)| k == n);
        // a bucket policy can deny its owner the bucket
        let removes = op::is_destructive(op)
            || method == "DELETE"
            || (method == "POST" && q("delete"))
            || (method == "PUT" && (q("lifecycle") || q("policy")))
            || (op == "Unknown" && method != "GET" && method != "HEAD");
        match self {
            Policy::Allow => None,
            Policy::NoDelete if removes => Some("no-delete"),
            Policy::ReadOnly if method != "GET" && method != "HEAD" => Some("read-only"),
            _ => None,
        }
    }

    /// Why a request is refused for what its document says, once it is
    /// read: versioning suspended (or a document not read) under no-delete.
    /// A suspended bucket's next write replaces the version it would keep.
    pub fn refuses_document(self, op: &str, body: Option<&str>) -> Option<&'static str> {
        (self == Policy::NoDelete
            && op == "PutBucketVersioning"
            && body.is_none_or(|b| b.contains("Suspended")))
        .then_some("no-delete")
    }
}

/// Whether `op` writes an object whole at its key (and so may write over one).
pub fn writes_object(op: &str) -> bool {
    matches!(op, "PutObject" | "CompleteMultipartUpload" | "CopyObject")
}

/// The objects a recording wrote (bucket, key): under no-delete it may write
/// them again, and of the objects already there only them. A program that
/// writes a key twice, and a trial's runs after its clean run, share one.
#[derive(Debug, Default)]
pub struct Written(parking_lot::Mutex<std::collections::HashSet<(String, String)>>);

impl Written {
    /// What `records` wrote: a trial's runs may write again what its clean
    /// run wrote, which that run's policy let through.
    pub fn of(records: &[Record]) -> Written {
        let w = Written::default();
        for r in records {
            if writes_object(&r.operation)
                && r.status.is_some_and(|s| (200..300).contains(&s))
                && let (Some(b), Some(k)) = (&r.bucket, &r.key)
            {
                w.add(b, k);
            }
        }
        w
    }

    pub fn has(&self, bucket: &str, key: &str) -> bool {
        self.0
            .lock()
            .contains(&(bucket.to_string(), key.to_string()))
    }

    fn add(&self, bucket: &str, key: &str) {
        self.0.lock().insert((bucket.to_string(), key.to_string()));
    }
}

/// Proxy settings.
#[derive(Clone)]
pub struct ProxyConfig {
    /// Where to listen; port 0 picks a free one.
    pub listen: SocketAddr,
    pub policy: Policy,
    /// Largest body buffered (signed `aws-chunked` bodies are decoded whole).
    pub max_buffer: usize,
    /// All the bodies buffered at once, together: a request past it waits
    /// for the ones before it, so concurrent large uploads queue rather than
    /// hold a gigabyte each.
    pub max_buffered_total: usize,
    /// Tags every record (the `job` field), e.g. a session id.
    pub tag: Option<String>,
    /// The keys the application signs with: made for each proxy, so only the
    /// program sonde gave them to can use the endpoint's credentials.
    pub client_key: String,
    pub client_secret: String,
    /// Requests may come from other machines (`--allow-remote`): any Host is
    /// taken. Otherwise only loopback names are (a web page that rebinds a
    /// name of its own to 127.0.0.1 is turned away).
    pub allow_remote: bool,
    /// The faults this proxy answers with (a trial run's scenario).
    pub faults: Option<Arc<fault::Faults>>,
    /// What the recording wrote, which no-delete lets it write again.
    pub written: Arc<Written>,
}

/// A fresh client key and secret: the key names the proxy, the secret is 32
/// random bytes in hex.
pub fn client_keys() -> (String, String) {
    use rand::RngCore;
    let mut k = [0u8; 8];
    let mut s = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut k);
    rand::thread_rng().fill_bytes(&mut s);
    (format!("SONDE{}", hex_upper(&k)), hex_lower(&s))
}

fn hex_upper(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

fn hex_lower(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Default for ProxyConfig {
    fn default() -> Self {
        let (key, secret) = client_keys();
        ProxyConfig {
            listen: SocketAddr::from(([127, 0, 0, 1], 0)),
            policy: Policy::Allow,
            max_buffer: 1 << 30,
            max_buffered_total: 2 << 30,
            tag: None,
            client_key: key,
            client_secret: secret,
            allow_remote: false,
            faults: None,
            written: Arc::default(),
        }
    }
}

struct Shared {
    client: S3Client,
    log: RequestLog,
    cfg: ProxyConfig,
    /// The buffering budget, in MiB (`max_buffered_total`).
    buffered: Arc<tokio::sync::Semaphore>,
}

const MIB: usize = 1 << 20;

/// What a body about to be buffered takes from the budget: its declared
/// size (at most `max_buffer`), or `max_buffer` when it declares none.
fn buffer_share(cfg: &ProxyConfig, declared: Option<u64>) -> u32 {
    let bytes = declared.map_or(cfg.max_buffer, |n| (n as usize).min(cfg.max_buffer));
    let total = (cfg.max_buffered_total / MIB).max(1);
    bytes.div_ceil(MIB).clamp(1, total) as u32
}

/// A running proxy. Dropping it stops accepting connections.
pub struct Proxy {
    addr: SocketAddr,
    region: String,
    keys: (String, String),
    task: tokio::task::JoinHandle<()>,
    /// Tells every connection to finish the request it is on and close.
    stop: tokio::sync::watch::Sender<bool>,
    /// Closed once the last connection has gone.
    open: tokio::sync::mpsc::Receiver<()>,
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
        let keys = (cfg.client_key.clone(), cfg.client_secret.clone());
        let buffered = Arc::new(tokio::sync::Semaphore::new(
            (cfg.max_buffered_total / MIB).max(1),
        ));
        let shared = Arc::new(Shared {
            client,
            log,
            cfg,
            buffered,
        });
        let (stop, stopping) = tokio::sync::watch::channel(false);
        let (alive, open) = tokio::sync::mpsc::channel::<()>(1);
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let shared = shared.clone();
                let mut stopping = stopping.clone();
                let alive = alive.clone();
                tokio::spawn(async move {
                    let svc = service_fn(move |req| handle(shared.clone(), req));
                    let conn = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), svc);
                    tokio::pin!(conn);
                    tokio::select! {
                        _ = conn.as_mut() => {}
                        // (a proxy dropped without a shutdown leaves them be)
                        Ok(()) = stopping.changed() => {
                            // the request it is on is answered and recorded
                            conn.as_mut().graceful_shutdown();
                            let _ = conn.await;
                        }
                    }
                    drop(alive);
                });
            }
        });
        Ok(Proxy {
            addr,
            region,
            keys,
            task,
            stop,
            open,
        })
    }

    /// Stops accepting, and waits (`grace` at most) for the requests in
    /// flight to be answered and recorded: what reads the log afterwards
    /// reads all of it. Whether every connection had gone by then.
    pub async fn shutdown(&mut self, grace: std::time::Duration) -> bool {
        self.task.abort();
        // the accepting task holds a sender of its own: gone with it
        let _ = (&mut self.task).await;
        let _ = self.stop.send(true);
        tokio::time::timeout(grace, self.open.recv()).await.is_ok()
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The key and secret the application signs with.
    pub fn client_keys(&self) -> (&str, &str) {
        (&self.keys.0, &self.keys.1)
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
            ("AWS_ACCESS_KEY_ID", self.keys.0.as_str()),
            ("AWS_SECRET_ACCESS_KEY", self.keys.1.as_str()),
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

/// Whether a request may use the proxy: a loopback Host (unless remote
/// requests are allowed), and a SigV4 signature, in its Authorization header
/// or its query (a presigned URL), made with this proxy's client secret.
/// The body's hash is the client's to declare: the signature proves the key.
pub fn admit(cfg: &ProxyConfig, parts: &http::request::Parts, host: &str) -> Result<(), String> {
    if !cfg.allow_remote {
        let name = host
            .rsplit_once(':')
            .filter(|(h, p)| !h.ends_with(']') || p.chars().all(|c| c.is_ascii_digit()))
            .map_or(host, |(h, p)| {
                if p.chars().all(|c| c.is_ascii_digit()) {
                    h
                } else {
                    host
                }
            })
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase();
        // a fully qualified name may end with a dot
        let name = name.strip_suffix('.').unwrap_or(&name);
        let loopback = name == "localhost"
            || name.ends_with(".localhost")
            || name
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if !loopback {
            return Err(format!(
                "Host {host:?} is not this machine's (start with --allow-remote to take other hosts)"
            ));
        }
    }
    let query = parse_query(parts.uri.query());
    let q = |k: &str| query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let header = |k: &str| {
        parts
            .headers
            .get(k)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    };
    // the legacy form some SDKs still presign with (boto3's default for S3)
    if q("AWSAccessKeyId").is_some() || header("authorization").starts_with("AWS ") {
        return admit_v2(cfg, parts, host, &query);
    }
    let (credential, signed, signature, amz_date, payload, presigned) = if let Some(sig) =
        q("X-Amz-Signature")
    {
        (
            q("X-Amz-Credential").unwrap_or("").to_string(),
            q("X-Amz-SignedHeaders").unwrap_or("").to_string(),
            sig.to_string(),
            q("X-Amz-Date").unwrap_or("").to_string(),
            sigv4::UNSIGNED_PAYLOAD.to_string(),
            true,
        )
    } else {
        let auth = header("authorization");
        if auth.is_empty() {
            return Err("it is not signed: give the program the keys sonde set (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)".into());
        }
        if !auth.starts_with("AWS4-HMAC-SHA256") {
            return Err("it is not signed with SigV4".into());
        }
        let field = |name: &str| {
            auth.split([',', ' '])
                .find_map(|p| p.trim().strip_prefix(name))
                .unwrap_or("")
                .to_string()
        };
        let payload = match header("x-amz-content-sha256") {
            "" => sigv4::UNSIGNED_PAYLOAD.to_string(),
            v => v.to_string(),
        };
        (
            field("Credential="),
            field("SignedHeaders="),
            field("Signature="),
            header("x-amz-date").to_string(),
            payload,
            false,
        )
    };
    let mut scope = credential.split('/');
    let key = scope.next().unwrap_or("");
    if key != cfg.client_key {
        return Err(format!(
            "it is signed with the key {key:?}, not the one sonde gave the program"
        ));
    }
    let _day = scope.next();
    let region = scope.next().unwrap_or("").to_string();
    let service = scope.next().unwrap_or("s3").to_string();
    let time = chrono::NaiveDateTime::parse_from_str(&amz_date, "%Y%m%dT%H%M%SZ")
        .map(|t| t.and_utc())
        .map_err(|_| "its signature has no date".to_string())?;
    let now = chrono::Utc::now();
    if presigned {
        // a presigned URL lasts what it says (at most a week, as S3 has it)
        let expires: i64 = q("X-Amz-Expires").and_then(|v| v.parse().ok()).unwrap_or(0);
        if !(1..=604_800).contains(&expires) {
            return Err("its X-Amz-Expires is missing or out of range".into());
        }
        if now > time + chrono::Duration::seconds(expires) || time > now + SKEW {
            return Err("its presigned URL has expired".into());
        }
    } else if (now - time).abs() > SKEW {
        return Err(
            "its signature's time is more than 15 minutes from this machine's clock".into(),
        );
    }
    let headers: Vec<(String, String)> = signed
        .split(';')
        .filter(|h| !h.is_empty())
        .map(|h| {
            let v = parts
                .headers
                .get_all(h)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .collect::<Vec<_>>()
                .join(",");
            (h.to_string(), v)
        })
        .collect();
    let mut unsigned_query = query.clone();
    if presigned {
        unsigned_query.retain(|(k, _)| k != "X-Amz-Signature");
    }
    let creds = s3raw::Credentials::new(&cfg.client_key, &cfg.client_secret, None);
    let canonical_uri = if parts.uri.path().is_empty() {
        "/"
    } else {
        parts.uri.path()
    };
    let expected = sigv4::sign(
        &sigv4::SignInput {
            method: parts.method.as_str(),
            canonical_uri,
            query: &unsigned_query,
            headers: &headers,
            payload_hash: &payload,
            region: &region,
            service: &service,
            time,
        },
        &creds,
    );
    if constant_eq(expected.signature.as_bytes(), signature.as_bytes()) {
        Ok(())
    } else {
        Err("its signature does not match the secret sonde gave the program".into())
    }
}

/// How far a signature's time may be from the clock, as S3 allows.
const SKEW: chrono::Duration = chrono::Duration::minutes(15);

/// Subresources a SigV2 signature covers, as S3 lists them.
const V2_SUBRESOURCES: &[&str] = &[
    "accelerate",
    "acl",
    "analytics",
    "cors",
    "defaultObjectAcl",
    "delete",
    "inventory",
    "lifecycle",
    "location",
    "logging",
    "metrics",
    "notification",
    "object-lock",
    "partNumber",
    "policy",
    "replication",
    "requestPayment",
    "response-cache-control",
    "response-content-disposition",
    "response-content-encoding",
    "response-content-language",
    "response-content-type",
    "response-expires",
    "restore",
    "select",
    "select-type",
    "storageClass",
    "tagging",
    "torrent",
    "uploadId",
    "uploads",
    "versionId",
    "versioning",
    "versions",
    "website",
];

/// A SigV2 request (`Authorization: AWS key:signature`, or a presigned URL
/// with `AWSAccessKeyId`, `Signature` and `Expires`) signed with this
/// recording's keys.
fn admit_v2(
    cfg: &ProxyConfig,
    parts: &http::request::Parts,
    host: &str,
    query: &[(String, String)],
) -> Result<(), String> {
    use base64::Engine;
    use hmac::{Hmac, Mac};
    let q = |k: &str| query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let header = |k: &str| {
        parts
            .headers
            .get(k)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let (key, signature, date) = match q("AWSAccessKeyId") {
        Some(k) => (
            k.to_string(),
            q("Signature").unwrap_or("").to_string(),
            q("Expires").unwrap_or("").to_string(),
        ),
        None => {
            let auth = header("authorization");
            let (k, s) = auth
                .strip_prefix("AWS ")
                .and_then(|x| x.split_once(':'))
                .ok_or("its SigV2 signature is malformed")?;
            // x-amz-date stands in for Date and is signed among the amz headers
            let date = if parts.headers.contains_key("x-amz-date") {
                String::new()
            } else {
                header("date")
            };
            (k.to_string(), s.to_string(), date)
        }
    };
    if key != cfg.client_key {
        return Err(format!(
            "it is signed with the key {key:?}, not the one sonde gave the program"
        ));
    }
    let now = chrono::Utc::now();
    match q("Expires") {
        Some(e) if q("AWSAccessKeyId").is_some() => {
            let at: i64 = e.parse().map_err(|_| "its Expires is not a time")?;
            if now.timestamp() > at {
                return Err("its presigned URL has expired".into());
            }
            // as SigV4's: a week at most, so a URL handed out does not last forever
            if at - now.timestamp() > 604_800 {
                return Err("its presigned URL lasts more than a week".into());
            }
        }
        _ => {
            let date = header("x-amz-date");
            let date = if date.is_empty() {
                header("date")
            } else {
                date
            };
            let t = chrono::DateTime::parse_from_rfc2822(&date)
                .map(|t| t.with_timezone(&chrono::Utc))
                .or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(&date, "%Y%m%dT%H%M%SZ")
                        .map(|t| t.and_utc())
                })
                .map_err(|_| "its signature has no date")?;
            if (now - t).abs() > SKEW {
                return Err(
                    "its signature's time is more than 15 minutes from this machine's clock".into(),
                );
            }
        }
    }
    let mut amz: Vec<(String, String)> = parts
        .headers
        .iter()
        .filter(|(n, _)| n.as_str().starts_with("x-amz-"))
        .map(|(n, v)| {
            (
                n.as_str().to_string(),
                v.to_str().unwrap_or("").trim().to_string(),
            )
        })
        .collect();
    amz.sort();
    let mut merged: Vec<(String, String)> = Vec::new();
    for (n, v) in amz {
        match merged.last_mut() {
            Some((ln, lv)) if *ln == n => {
                lv.push(',');
                lv.push_str(&v);
            }
            _ => merged.push((n, v)),
        }
    }
    // a virtual-hosted request names its bucket in the Host
    let name = host
        .rsplit_once(':')
        .map_or(host, |(h, _)| h)
        .to_ascii_lowercase();
    let mut resource = match name.strip_suffix(".localhost") {
        Some(bucket) if !bucket.is_empty() => format!("/{bucket}{}", parts.uri.path()),
        _ => parts.uri.path().to_string(),
    };
    let mut subs: Vec<&(String, String)> = query
        .iter()
        .filter(|(k, _)| V2_SUBRESOURCES.contains(&k.as_str()))
        .collect();
    subs.sort_by(|a, b| a.0.cmp(&b.0));
    for (i, (k, v)) in subs.iter().enumerate() {
        resource.push(if i == 0 { '?' } else { '&' });
        resource.push_str(k);
        if !v.is_empty() {
            resource.push('=');
            resource.push_str(v);
        }
    }
    let mut to_sign = format!(
        "{}\n{}\n{}\n{}\n",
        parts.method.as_str(),
        header("content-md5"),
        header("content-type"),
        date
    );
    for (n, v) in &merged {
        to_sign.push_str(&format!("{n}:{v}\n"));
    }
    to_sign.push_str(&resource);
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(cfg.client_secret.as_bytes())
        .map_err(|e| e.to_string())?;
    mac.update(to_sign.as_bytes());
    let expected = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    if constant_eq(expected.as_bytes(), signature.as_bytes()) {
        Ok(())
    } else {
        Err("its signature does not match the secret sonde gave the program".into())
    }
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Headers not forwarded: hop-by-hop ones, and the client's signature.
const DROP_REQUEST: &[&str] = &[
    // where in the program a request came from: sonde's to read, not the store's
    callers::CALLER_HEADER,
    // sonde's own name for a control body: one a program sends is not it
    fault::BODY_HASH_HEADER,
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
pub(crate) const PRESIGN_PARAMS: &[&str] = &[
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

/// Whether a path-style path starts with an empty segment (`//bucket/key`):
/// it names no bucket here, but the store may fold the slashes and find one.
pub fn empty_first_segment(host: &str, path: &str) -> bool {
    let host = host.rsplit_once(':').map_or(host, |(h, p)| {
        if p.chars().all(|c| c.is_ascii_digit()) {
            h
        } else {
            host
        }
    });
    let virtual_hosted = host
        .strip_suffix(".localhost")
        .is_some_and(|b| !b.is_empty());
    !virtual_hosted && path.starts_with("//")
}

/// Whether a request's place has a `.` or `..` segment, in the bucket's
/// position or in the key: what the proxy would record is not what the
/// store would be asked for.
pub fn dot_segment(bucket: Option<&str>, key: Option<&str>) -> bool {
    bucket.is_some_and(|b| b == "." || b == "..") || key.is_some_and(s3raw::has_dot_segment)
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

/// An injected fault's answer: S3's error document, marked as sonde's.
fn fault_error(status: u16, code: &str, message: &str, scenario: &str) -> Response<Out> {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>{code}</Code><Message>{message}</Message><RequestId>sonde-fault</RequestId></Error>"
    );
    Response::builder()
        .status(status)
        .header("content-type", "application/xml")
        .header("x-sonde-fault", scenario)
        .body(full(Bytes::from(body)))
        .expect("static response")
}

fn full(b: Bytes) -> Out {
    Full::new(b).map_err(|never| match never {}).boxed()
}

/// A request the policy refuses: recorded as refused, answered 403.
fn refuse(shared: &Shared, mut record: Record, msg: String) -> Response<Out> {
    record.status = Some(403);
    record.error_body = Some(msg.clone());
    record
        .response_headers
        .push(Header::new("x-sonde-proxy", "refused"));
    shared.log.push(record);
    xml_error(403, "AccessDenied", &msg)
}

/// Whether `bucket/key` is in the store: a HEAD of sonde's own, signed
/// with the endpoint's credentials. Not a request of the program's, so not
/// recorded.
async fn stored(shared: &Shared, bucket: &str, key: &str) -> Result<bool, String> {
    let client = &shared.client;
    let cfg = client.config();
    let (origin, path, host) = client.address(&cfg.endpoint, Some(bucket), Some(key));
    let mut headers = vec![("host".to_string(), host)];
    if let Some(creds) = client.credentials().await.map_err(|e| e.to_string())? {
        sigv4::sign_headers(
            &mut headers,
            "HEAD",
            &path,
            &[],
            sigv4::EMPTY_SHA256,
            &cfg.region,
            "s3",
            Utc::now(),
            &creds,
        );
    }
    let mut rb = client.http_streaming().head(format!("{origin}{path}"));
    for (k, v) in &headers {
        if !k.eq_ignore_ascii_case("host") {
            rb = rb.header(k.as_str(), v.as_str());
        }
    }
    let resp = tokio::time::timeout(cfg.timeout, rb.send())
        .await
        .map_err(|_| "no answer".to_string())?
        .map_err(|e| e.to_string())?;
    match resp.status().as_u16() {
        200..=299 => Ok(true),
        404 => Ok(false),
        s => Err(format!("the store answered {s}")),
    }
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

    // the request as the application saw it: from its arrival (a buffered
    // body's upload included) to the answer
    let mut received = Instant::now();
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
        request_headers: app_headers
            .iter()
            // (a control body's hash is sonde's to say: see below)
            .filter(|(k, _)| !k.eq_ignore_ascii_case(fault::BODY_HASH_HEADER))
            .map(|(k, v)| Header::new(k, v))
            .collect(),
        response_headers: Vec::new(),
        request_bytes: None,
        response_bytes: None,
        error_body: None,
        request_body: None,
        request_body_truncated: false,
        transport_error: None,
        region: Some(cfg.region.clone()),
        job: shared.cfg.tag.clone(),
        first_byte_ms: None,
        requester: None,
        injected: None,
    };
    if presigned {
        record.request_headers.push(Header::new(
            "x-sonde-note",
            "presigned URL, re-signed by sonde",
        ));
    }

    // only the program sonde gave its keys to, from this machine
    if let Err(why) = admit(&shared.cfg, &parts, &host) {
        let msg = format!("sonde refused the request: {why}");
        record.status = Some(403);
        record.error_body = Some(msg.clone());
        record
            .response_headers
            .push(Header::new("x-sonde-proxy", "refused"));
        shared.log.push(record);
        return Ok(xml_error(403, "SignatureDoesNotMatch", &msg));
    }

    // under every policy: what the proxy read as no bucket must not reach a
    // store that reads it as one
    if empty_first_segment(&host, parts.uri.path()) {
        let msg = "sonde refused the request: its path starts with an empty segment (//), so it names no bucket";
        record.status = Some(400);
        record.error_body = Some(msg.into());
        record
            .response_headers
            .push(Header::new("x-sonde-proxy", "refused"));
        shared.log.push(record);
        return Ok(xml_error(400, "InvalidURI", msg));
    }

    // a `.` or `..` segment never reaches the store as the place it names:
    // the HTTP library folds it, and `a/../b` would be signed and sent as
    // `b`, `/../demo` as a listing of `demo`. The bucket's place as the key's.
    if dot_segment(bucket.as_deref(), key.as_deref()) {
        let msg = "sonde refused the request: its path has a . or .. segment, which would reach the store as another place";
        record.status = Some(400);
        record.error_body = Some(msg.into());
        record
            .response_headers
            .push(Header::new("x-sonde-proxy", "refused"));
        shared.log.push(record);
        return Ok(xml_error(400, "InvalidURI", msg));
    }

    if let Some(mode) = shared.cfg.policy.refuses(&method, &operation, &query) {
        let msg = format!("sonde refused {operation}: the proxy runs in {mode} mode");
        return Ok(refuse(&shared, record, msg));
    }

    // under no-delete, an object already there is written over only if
    // this recording wrote it: a copy's target is looked for first (a HEAD
    // of sonde's own, not recorded and not timed), a write that says no
    // condition goes with `If-None-Match: *` and the store's 412 is the
    // refusal. A write on `If-Match` names an object already there.
    let mut guarded = false;
    let mut looked = std::time::Duration::ZERO;
    if shared.cfg.policy == Policy::NoDelete
        && writes_object(&operation)
        && let (Some(b), Some(k)) = (bucket.as_deref(), key.as_deref())
        && !shared.cfg.written.has(b, k)
    {
        let said = |n: &str| app_headers.iter().any(|(h, _)| h.eq_ignore_ascii_case(n));
        let over = |why: &str| {
            format!(
                "sonde refused {operation}: {b}/{k} {why}, and the proxy runs in no-delete mode (--allow-writes lets a program write over it)"
            )
        };
        if said("if-match") {
            return Ok(refuse(
                &shared,
                record,
                over("is an object this recording did not write"),
            ));
        }
        if operation == "CopyObject" {
            let at = Instant::now();
            let found = stored(&shared, b, k).await;
            looked = at.elapsed();
            match found {
                Ok(false) => {}
                Ok(true) => return Ok(refuse(&shared, record, over("was there before"))),
                Err(e) => {
                    let why = format!("may be there already (looking for it: {e})");
                    return Ok(refuse(&shared, record, over(&why)));
                }
            }
        } else {
            guarded = !said("if-none-match");
        }
    }
    received += looked;

    let content_sha = header_value(&parts.headers, "x-amz-content-sha256");
    let length: Option<u64> = header_value(&parts.headers, "content-length").parse().ok();
    let chunked_te = header_value(&parts.headers, "transfer-encoding")
        .to_ascii_lowercase()
        .contains("chunked");
    let has_body = length.is_some_and(|l| l > 0) || chunked_te;
    // a body read whole waits its turn in the buffering budget, held until
    // the request is answered
    let _budget = if chunked::is_signed_streaming(&content_sha) || (has_body && length.is_none()) {
        let declared = header_value(&parts.headers, "x-amz-decoded-content-length")
            .parse()
            .ok()
            .or(length);
        shared
            .buffered
            .clone()
            .acquire_many_owned(buffer_share(&shared.cfg, declared))
            .await
            .ok()
    } else {
        None
    };

    // a control document (DeleteObjects, a lifecycle rule) is read now: it
    // is small, it is kept on the record, and it tells one request without a
    // key from the next (two batches of deletes are two requests to fail)
    let streaming_sig = chunked::is_signed_streaming(&content_sha);
    let mut body = Some(body);
    let mut early: Option<Bytes> = None;
    let mut body_hash: Option<String> = None;
    if has_body
        && reqlog::is_control_operation(&operation)
        && (streaming_sig || length.is_none_or(|n| n <= 1 << 20))
    {
        let limit = if streaming_sig || length.is_none() {
            shared.cfg.max_buffer
        } else {
            1 << 20
        };
        let taken = body.take().expect("not read yet");
        match Limited::new(taken, limit).collect().await {
            Ok(c) => {
                let raw = c.to_bytes();
                // the document itself, not its transfer framing
                // (kept now: a request sonde answers itself has it too)
                let hash = match streaming_sig.then(|| chunked::decode(&raw)) {
                    Some(Ok(d)) => {
                        record.request_body =
                            Some(String::from_utf8_lossy(&d.payload).into_owned());
                        fault::body_hash(&d.payload)
                    }
                    _ => {
                        record.request_body = Some(String::from_utf8_lossy(&raw).into_owned());
                        fault::body_hash(&raw)
                    }
                };
                record
                    .request_headers
                    .push(Header::new(fault::BODY_HASH_HEADER, &hash));
                body_hash = Some(hash);
                early = Some(raw);
            }
            Err(e) => {
                let msg = format!("sonde could not read the request body: {e}");
                record.transport_error = Some(msg.clone());
                shared.log.push(record);
                return Ok(xml_error(400, "IncompleteBody", &msg));
            }
        }
    }
    if let Some(mode) = shared
        .cfg
        .policy
        .refuses_document(&operation, record.request_body.as_deref())
    {
        let msg = format!("sonde refused {operation}: the proxy runs in {mode} mode");
        return Ok(refuse(&shared, record, msg));
    }

    // a trial's faults: answered here, delayed, or sent on with the answer
    // to be lost. The record says so, so that no reader blames the store.
    let mut after = After::Forward;
    let fault = shared.cfg.faults.as_ref().and_then(|f| {
        f.decide(&fault::Asked {
            method: &method,
            op: &operation,
            bucket: bucket.as_deref(),
            key: key.as_deref(),
            query: &query,
            body_hash: body_hash.as_deref(),
            headers: &app_headers,
        })
        .map(|v| (f.name.clone(), v))
    });
    // a write that loses its race without being sent on is answered as any
    // error in the store's place is
    let fault = fault.map(|(name, v)| match v {
        fault::Verdict::LoseRace { forward: false, .. } => (
            name,
            fault::Verdict::Answer {
                status: 412,
                code: "PreconditionFailed".into(),
            },
        ),
        v => (name, v),
    });
    match fault {
        Some((name, fault::Verdict::Answer { status, code })) => {
            // the body is taken first, as a store that answers an error does
            if let Some(mut body) = body.take() {
                while let Some(Ok(_)) = body.frame().await {}
            }
            let msg = format!("sonde answered in the store's place (scenario {name})");
            record.injected = Some(name.clone());
            record.status = Some(status);
            record.error_body = Some(format!("{code}: {msg}"));
            record.latency_ms = received.elapsed().as_secs_f64() * 1000.0;
            record
                .response_headers
                .push(Header::new("x-sonde-fault", &name));
            shared.log.push(record);
            return Ok(fault_error(status, &code, &msg, &name));
        }
        Some((name, fault::Verdict::Wait(d))) => {
            record.injected = Some(name);
            tokio::time::sleep(d).await;
        }
        Some((name, fault::Verdict::LoseAnswer)) => {
            record.injected = Some(name);
            after = After::LoseAnswer;
        }
        Some((name, fault::Verdict::LoseRace { met, .. })) => {
            record.injected = Some(name);
            after = After::LoseRace(met);
        }
        Some((name, fault::Verdict::CutBody { met })) => {
            record.injected = Some(name);
            after = After::CutBody(met);
        }
        None => {}
    }

    // headers to forward, and the payload
    let mut out: Vec<(String, String)> = app_headers
        .iter()
        .filter(|(k, _)| !DROP_REQUEST.contains(&k.to_ascii_lowercase().as_str()))
        .cloned()
        .collect();
    // (signed, sent, and not on the record: the program did not ask it)
    if guarded {
        out.push(("if-none-match".into(), "*".into()));
    }
    // no time for the whole exchange (a large body takes what it takes), only
    // for one that has stopped moving
    let idle = cfg.timeout;
    let moved = Moved::default();
    let (payload_hash, upstream_body, sent_len): (String, reqwest::Body, Option<u64>) =
        if chunked::is_signed_streaming(&content_sha) {
            let raw = match whole(early.take(), body.take(), shared.cfg.max_buffer).await {
                Ok(raw) => raw,
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
            if reqlog::is_control_operation(&operation) {
                record.request_body = Some(String::from_utf8_lossy(&decoded.payload).into_owned());
            }
            (hash, moved.body(decoded.payload.into()), Some(n))
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
                // a control document (DeleteObjects, a lifecycle rule): small,
                // kept on the record so the request can be replayed
                Some(n) if n <= 1 << 20 && reqlog::is_control_operation(&operation) => {
                    match whole(early.take(), body.take(), 1 << 20).await {
                        Ok(raw) => {
                            record.request_body = Some(String::from_utf8_lossy(&raw).into_owned());
                            (hash, moved.body(raw), Some(n))
                        }
                        Err(e) => {
                            let msg = format!("sonde could not read the request body: {e}");
                            record.transport_error = Some(msg.clone());
                            shared.log.push(record);
                            return Ok(xml_error(400, "IncompleteBody", &msg));
                        }
                    }
                }
                Some(n) => {
                    let moved = moved.clone();
                    let stream = body
                        .take()
                        .expect("a body not read early is still here")
                        .into_data_stream()
                        .inspect_ok(move |_| moved.touch());
                    (hash, reqwest::Body::wrap_stream(stream), Some(n))
                }
                None => {
                    // chunked transfer without a length: S3 wants one
                    let raw = match whole(early.take(), body.take(), shared.cfg.max_buffer).await {
                        Ok(raw) => raw,
                        Err(e) => {
                            let msg = format!("sonde could not read the request body: {e}");
                            record.transport_error = Some(msg.clone());
                            shared.log.push(record);
                            return Ok(xml_error(400, "IncompleteBody", &msg));
                        }
                    };
                    let n = raw.len() as u64;
                    // a control document sent chunked is kept too, for replay
                    if reqlog::is_control_operation(&operation) {
                        record.request_body = Some(String::from_utf8_lossy(&raw).into_owned());
                    }
                    (hash, moved.body(raw), Some(n))
                }
            }
        };
    record.request_bytes = sent_len;

    out.push(("host".into(), upstream_host));
    // signing's credentials may be fetched now (STS, IMDS): not the store's time
    let fetching = Instant::now();
    let credentials = client.credentials().await;
    let fetched = fetching.elapsed();
    match credentials {
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

    let mut rb = client.http_streaming().request(parts.method.clone(), &url);
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
    moved.touch();
    let sent = tokio::select! {
        r = rb.body(upstream_body).send() => r.map_err(|e| {
            let mut msg = e.to_string();
            let mut src = std::error::Error::source(&e);
            while let Some(s) = src {
                msg.push_str(": ");
                msg.push_str(&s.to_string());
                src = s.source();
            }
            msg
        }),
        _ = moved.quiet_for(idle) => Err(stalled(idle)),
    };
    record.latency_ms = received.elapsed().saturating_sub(fetched).as_secs_f64() * 1000.0;
    let resp = match sent {
        Ok(r) => r,
        Err(msg) => {
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
    if guarded && status.as_u16() == 412 {
        drain(resp, idle).await;
        record
            .response_headers
            .push(Header::new("x-sonde-store-status", status.as_str()));
        record.latency_ms = received.elapsed().saturating_sub(fetched).as_secs_f64() * 1000.0;
        let (b, k) = (bucket.unwrap_or_default(), key.unwrap_or_default());
        let msg = format!(
            "sonde refused {operation}: {b}/{k} was there before, and the proxy runs in no-delete mode (--allow-writes lets a program write over it)"
        );
        return Ok(refuse(&shared, record, msg));
    }
    if status.is_success()
        && writes_object(&operation)
        && let (Some(b), Some(k)) = (bucket.as_deref(), key.as_deref())
    {
        shared.cfg.written.add(b, k);
    }
    // a fault that needed the store's answer first, and cannot be done to
    // this one, was no fault: the answer goes through as the store's own
    let mut cut: Option<(u64, u64)> = None;
    let faults = shared.cfg.faults.as_deref();
    match &after {
        After::LoseRace(met) if !status.is_success() => {
            // the store refused the write itself: its answer, not sonde's
            if let Some(f) = faults {
                f.unmeet(met);
            }
            record.injected = None;
            after = After::Forward;
        }
        After::CutBody(met) => {
            let whole = resp.content_length();
            match whole.and_then(fault::cut_at) {
                Some(at) if status.is_success() => cut = whole.map(|n| (at, n)),
                _ => {
                    if let Some(f) = faults {
                        f.unmeet(met);
                    }
                    record.injected = None;
                    after = After::Forward;
                }
            }
        }
        _ => {}
    }
    if matches!(after, After::LoseAnswer | After::LoseRace(_)) {
        // the store acted; the program is told it failed
        // (its body read and let go piece by piece: a lost GET of a large
        // object is not held in memory to be thrown away)
        drain(resp, idle).await;
        let name = record.injected.clone().unwrap_or_default();
        let (told, code, msg) = if after == After::LoseAnswer {
            (
                500,
                "InternalError",
                format!(
                    "sonde lost the store's answer ({}) on purpose (scenario {name})",
                    status.as_u16()
                ),
            )
        } else {
            (
                412,
                "PreconditionFailed",
                format!(
                    "sonde answered that the write lost its race, on purpose (scenario {name}); the store made it ({})",
                    status.as_u16()
                ),
            )
        };
        record
            .response_headers
            .push(Header::new("x-sonde-store-status", status.as_str()));
        record
            .response_headers
            .push(Header::new("x-sonde-fault", &name));
        record.status = Some(told);
        record.error_body = Some(format!("{code}: {msg}"));
        record.latency_ms = received.elapsed().saturating_sub(fetched).as_secs_f64() * 1000.0;
        shared.log.push(record);
        return Ok(fault_error(told, code, &msg, &name));
    }
    let mut builder = Response::builder().status(status);
    for (k, v) in resp.headers() {
        if !DROP_RESPONSE.contains(&k.as_str()) {
            builder = builder.header(k, v);
        }
    }
    // a body to be cut: the status and the whole length go out as the
    // store gave them, and the record says where the body will stop
    let cutting = cut.map(|(at, whole)| {
        let name = record.injected.clone().unwrap_or_default();
        record
            .response_headers
            .push(Header::new("x-sonde-store-status", status.as_str()));
        record
            .response_headers
            .push(Header::new("x-sonde-fault", &name));
        record
            .response_headers
            .push(Header::new(CUT_HEADER, at.to_string()));
        builder = std::mem::replace(&mut builder, Response::builder())
            .header("x-sonde-fault", name.as_str());
        Cutting { at, whole, name }
    });

    if status.as_u16() >= 400 && method != "HEAD" {
        let body = tokio::time::timeout(idle, resp.bytes())
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        record.response_bytes = Some(body.len() as u64);
        if !body.is_empty() {
            record.error_body = Some(String::from_utf8_lossy(&body).into_owned());
        }
        shared.log.push(record);
        return Ok(builder.body(full(body)).expect("valid response"));
    }
    record.response_bytes = resp.content_length();
    // the headers are the first byte; the record is logged once the body has
    // gone through (or the program stopped reading it), its time the whole
    // exchange's
    record.first_byte_ms = Some(record.latency_ms);
    let mut pending = Pending {
        record: Some(record),
        log: shared.clone(),
        received,
        fetched,
        read: 0,
        cut: None,
        cutting: cutting.clone(),
    };
    let body = match cutting {
        Some(c) => cut_short(quiet_limited(resp.bytes_stream(), idle), c.at).left_stream(),
        None => quiet_limited(resp.bytes_stream(), idle).right_stream(),
    };
    let stream = body
        // the closure owns the whole of `pending` (a method, not a field): it
        // goes, and the record is logged, when the body does
        .map(move |piece| match piece {
            Ok(chunk) => {
                pending.count(chunk.len());
                Ok(Frame::data(chunk))
            }
            // the status went out already: the record is what says the
            // program did not get the whole of it
            Err(e) => {
                pending.cut(&e);
                Err(e)
            }
        });
    Ok(builder
        .body(BodyExt::boxed(StreamBody::new(stream)))
        .expect("valid response"))
}

/// What is done with a request once it has been sent on.
#[derive(Debug, PartialEq)]
enum After {
    /// The store's answer goes back as it is.
    Forward,
    /// The answer is replaced by a 500.
    LoseAnswer,
    /// A write the store made is answered 412.
    LoseRace(fault::Met),
    /// The answer's body stops part way.
    CutBody(fault::Met),
}

/// The response header a record keeps the place of an injected cut in: the
/// bytes of the body that went out.
pub const CUT_HEADER: &str = "x-sonde-cut-at";

/// How long a cut body's last bytes are given to leave before the
/// connection is dropped.
const CUT_PAUSE: std::time::Duration = std::time::Duration::from_millis(50);

/// A body sonde stops on purpose: after `at` of `whole` bytes.
#[derive(Clone)]
struct Cutting {
    at: u64,
    whole: u64,
    name: String,
}

/// `body` up to `at` bytes, then an error: the connection drops with the
/// rest unsent, as a store's does when it dies in the middle of an answer.
fn cut_short(
    body: impl futures::Stream<Item = std::io::Result<Bytes>> + Send + 'static,
    at: u64,
) -> impl futures::Stream<Item = std::io::Result<Bytes>> + Send + 'static {
    futures::stream::unfold(
        (Box::pin(body), 0u64, false),
        move |(mut body, sent, done)| async move {
            if done {
                return None;
            }
            let stop = || std::io::Error::other("cut by sonde");
            if sent >= at {
                // what went out is on its way before the connection drops:
                // an error at once would take the bytes not yet written
                // with it
                tokio::time::sleep(CUT_PAUSE).await;
                return Some((Err(stop()), (body, sent, true)));
            }
            match body.next().await {
                Some(Ok(chunk)) => {
                    let take = ((at - sent) as usize).min(chunk.len());
                    Some((Ok(chunk.slice(..take)), (body, sent + take as u64, false)))
                }
                Some(Err(e)) => Some((Err(e), (body, sent, true))),
                None => Some((Err(stop()), (body, sent, true))),
            }
        },
    )
}

/// A request body whole: the one read early, or the one still to read
/// (`limit` bytes at most).
async fn whole(
    early: Option<Bytes>,
    body: Option<Incoming>,
    limit: usize,
) -> Result<Bytes, String> {
    match (early, body) {
        (Some(raw), _) => Ok(raw),
        (None, Some(body)) => Limited::new(body, limit)
            .collect()
            .await
            .map(|c| c.to_bytes())
            .map_err(|e| e.to_string()),
        (None, None) => Ok(Bytes::new()),
    }
}

/// The most of an answer read to be thrown away: past it the connection is
/// dropped instead (the store has acted by then).
const DRAIN_MAX: u64 = 8 << 20;

/// Reads an answer and keeps none of it: each piece is dropped as it
/// comes, and a body past [`DRAIN_MAX`], or one that stops for `idle`, is
/// left unread.
async fn drain(resp: reqwest::Response, idle: std::time::Duration) -> u64 {
    let mut read = 0u64;
    let mut body = resp.bytes_stream();
    while read < DRAIN_MAX {
        match tokio::time::timeout(idle, body.next()).await {
            Ok(Some(Ok(piece))) => read += piece.len() as u64,
            _ => break,
        }
    }
    read
}

/// When a request's body last moved towards the store.
#[derive(Clone)]
struct Moved(Arc<parking_lot::Mutex<Instant>>);

impl Default for Moved {
    fn default() -> Self {
        Moved(Arc::new(parking_lot::Mutex::new(Instant::now())))
    }
}

impl Moved {
    fn touch(&self) {
        *self.0.lock() = Instant::now();
    }

    /// A body read whole, sent in pieces so that its going is seen.
    fn body(&self, data: Bytes) -> reqwest::Body {
        if data.is_empty() {
            return reqwest::Body::from(data);
        }
        let moved = self.clone();
        let pieces = futures::stream::unfold(data, move |mut rest| {
            moved.touch();
            let piece = rest.split_to(rest.len().min(64 << 10));
            futures::future::ready((!piece.is_empty()).then(|| (Ok::<_, Infallible>(piece), rest)))
        });
        reqwest::Body::wrap_stream(pieces)
    }

    /// Returns once nothing has moved for `limit`.
    async fn quiet_for(&self, limit: std::time::Duration) {
        loop {
            let quiet = self.0.lock().elapsed();
            if quiet >= limit {
                return;
            }
            tokio::time::sleep(limit - quiet).await;
        }
    }
}

fn stalled(limit: std::time::Duration) -> String {
    format!(
        "the exchange with the store stopped moving for {} s",
        limit.as_secs_f64()
    )
}

/// A response body that fails when the store sends nothing for `limit`; the
/// time the program takes to read is not the store's.
fn quiet_limited(
    body: impl futures::Stream<Item = reqwest::Result<Bytes>> + Send + 'static,
    limit: std::time::Duration,
) -> impl futures::Stream<Item = std::io::Result<Bytes>> + Send + 'static {
    futures::stream::try_unfold(Box::pin(body), move |mut body| async move {
        match tokio::time::timeout(limit, body.next()).await {
            Ok(Some(Ok(chunk))) => Ok(Some((chunk, body))),
            Ok(Some(Err(e))) => Err(std::io::Error::other(e)),
            Ok(None) => Ok(None),
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                stalled(limit),
            )),
        }
    })
}

/// A record waiting for its response body to go through: logged when the
/// stream is done with, read to its end or dropped early.
struct Pending {
    record: Option<Record>,
    log: Arc<Shared>,
    received: Instant,
    fetched: std::time::Duration,
    read: u64,
    /// Why the body ended before its end: the idle limit, or the store's
    /// connection.
    cut: Option<String>,
    /// The cut sonde makes itself (a trial's scenario).
    cutting: Option<Cutting>,
}

impl Pending {
    fn count(&mut self, n: usize) {
        self.read += n as u64;
    }

    fn cut(&mut self, why: &std::io::Error) {
        self.cut = Some(why.to_string());
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(mut r) = self.record.take() {
            r.latency_ms = self
                .received
                .elapsed()
                .saturating_sub(self.fetched)
                .as_secs_f64()
                * 1000.0;
            if r.response_bytes.is_none() || self.read > 0 || self.cut.is_some() {
                r.response_bytes = Some(self.read);
            }
            // a 200 with fewer bytes is not a clean 200
            if let Some(c) = self.cutting.take() {
                self.cut = None;
                r.transport_error = Some(format!(
                    "sonde cut the response body after {} of {} bytes on purpose (scenario {})",
                    self.read, c.whole, c.name
                ));
            }
            if let Some(why) = self.cut.take() {
                r.transport_error = Some(format!(
                    "the response body was cut after {} byte{}: {why}",
                    self.read,
                    if self.read == 1 { "" } else { "s" }
                ));
            }
            self.log.log.push(r);
        }
    }
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
        assert_eq!(Policy::Allow.refuses("DELETE", "DeleteObject", &[]), None);
        assert_eq!(
            Policy::NoDelete.refuses("DELETE", "DeleteObject", &[]),
            Some("no-delete")
        );
        assert_eq!(Policy::NoDelete.refuses("PUT", "PutObject", &[]), None);
        assert_eq!(
            Policy::ReadOnly.refuses("PUT", "PutObject", &[]),
            Some("read-only")
        );
        assert_eq!(Policy::ReadOnly.refuses("GET", "GetObject", &[]), None);
    }

    #[test]
    fn no_delete_goes_by_the_method_whatever_the_request_is_named() {
        let q = |k: &str| vec![(k.to_string(), String::new())];
        let no = Policy::NoDelete;
        assert_eq!(no.refuses("DELETE", "Unknown", &[]), Some("no-delete"));
        assert_eq!(
            no.refuses("POST", "Unknown", &q("delete")),
            Some("no-delete")
        );
        assert_eq!(
            no.refuses("PUT", "Unknown", &q("lifecycle")),
            Some("no-delete")
        );
        // a write the proxy cannot name may be anything
        assert_eq!(no.refuses("POST", "Unknown", &[]), Some("no-delete"));
        assert_eq!(no.refuses("GET", "Unknown", &[]), None);
        assert_eq!(no.refuses("HEAD", "Unknown", &[]), None);
        assert_eq!(no.refuses("POST", "CreateMultipartUpload", &[]), None);
        assert_eq!(Policy::Allow.refuses("DELETE", "Unknown", &[]), None);
    }

    #[test]
    fn a_path_that_starts_with_an_empty_segment_names_no_bucket() {
        assert!(empty_first_segment("127.0.0.1:9000", "//bucket/key"));
        assert!(empty_first_segment("localhost", "//"));
        assert!(!empty_first_segment("127.0.0.1:9000", "/bucket//key"));
        assert!(!empty_first_segment("127.0.0.1:9000", "/"));
        // virtual-hosted: the key itself starts with a slash
        assert!(!empty_first_segment("data.localhost:9000", "//key"));
    }

    #[test]
    fn a_dot_segment_is_refused_wherever_in_the_path_it_stands() {
        let at = |path: &str| {
            let (b, k) = parse_target("127.0.0.1:9000", path);
            dot_segment(b.as_deref(), k.as_deref())
        };
        // the bucket's place: the store would be asked for a listing of `demo`
        assert!(at("/%2e%2e/demo") && at("/../demo") && at("/./demo/k"));
        assert!(at("/..") && at("/%2E"));
        assert!(at("/b/a/../k") && at("/b/./k") && at("/b/k/%2e%2e"));
        assert!(!at("/b/k") && !at("/b/.hidden") && !at("/b/a..b/k...") && !at("/.b/k"));
        // virtual-hosted: the path is all key
        let (b, k) = parse_target("data.localhost:9000", "/../k");
        assert!(dot_segment(b.as_deref(), k.as_deref()));
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn a_body_takes_its_size_from_the_budget_and_an_unsized_one_the_most() {
        let cfg = ProxyConfig::default();
        assert_eq!(buffer_share(&cfg, Some(10)), 1);
        assert_eq!(buffer_share(&cfg, Some(5 << 20)), 5);
        assert_eq!(buffer_share(&cfg, None), 1024);
        // never more than one body may take, nor than the whole budget
        assert_eq!(buffer_share(&cfg, Some(8 << 30)), 1024);
        let small = ProxyConfig {
            max_buffered_total: 100 << 20,
            ..ProxyConfig::default()
        };
        assert_eq!(buffer_share(&small, None), 100);
    }
}
