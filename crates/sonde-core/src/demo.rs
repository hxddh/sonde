//! A small S3 server inside the process, for demos, UI tests and the
//! performance runs: three buckets — `demo` (folders, versions, delete
//! markers, incomplete uploads), `demo-large` (100 000 synthetic keys,
//! generated on the fly) and `demo-layouts` (real SlateDB, Iceberg, lakeFS
//! and Woodpecker layouts). Requests are not authenticated; it listens on
//! 127.0.0.1 only.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

mod samples {
    include!(concat!(env!("OUT_DIR"), "/samples.rs"));
}

pub const LARGE_KEYS: usize = 100_000;

#[derive(Clone)]
struct Version {
    id: String,
    data: Option<Vec<u8>>, // None: delete marker
    time: DateTime<Utc>,
    class: &'static str,
}

#[derive(Clone)]
struct Upload {
    key: String,
    id: String,
    initiated: DateTime<Utc>,
    parts: Vec<(u32, u64)>,
}

#[derive(Default)]
struct Bucket {
    versioning: Option<&'static str>,
    objects: BTreeMap<String, Vec<Version>>, // newest last
    uploads: Vec<Upload>,
    lifecycle: Option<String>,
    synthetic: usize,
    created: Option<DateTime<Utc>>,
}

#[derive(Default)]
struct State {
    buckets: BTreeMap<String, Bucket>,
    seq: u64,
}

fn synthetic_key(i: usize) -> String {
    format!(
        "logs/2026/{:02}/{:02}/events-{i:06}.json.gz",
        // keys sort as their index does: continuation tokens are positions
        1 + i / 10_000,
        1 + (i % 10_000) / 400
    )
}

fn esc(s: &str) -> String {
    s3raw::xml::escape(s)
}

fn enc(s: &str, url: bool) -> String {
    if url {
        s3raw::sigv4::uri_encode(s, true)
    } else {
        esc(s)
    }
}

fn ts(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn etag(data: &[u8]) -> String {
    use sha2::Digest;
    // not MD5 (no md-5 here): a stable 32-hex digest
    let h = sha2::Sha256::digest(data);
    format!("\"{}\"", hex::encode(&h[..16]))
}

impl State {
    fn seeded() -> State {
        let now = Utc::now();
        let mut s = State::default();
        let mut demo = Bucket {
            versioning: Some("Enabled"),
            created: Some(now - Duration::days(400)),
            ..Default::default()
        };
        let put = |b: &mut Bucket,
                   key: &str,
                   size: usize,
                   days_ago: i64,
                   class: &'static str,
                   id: &str| {
            b.objects.entry(key.into()).or_default().push(Version {
                id: id.into(),
                data: Some(vec![b'x'; size]),
                time: now - Duration::days(days_ago),
                class,
            });
        };
        for (i, k) in [
            "reports/2026-q1.csv",
            "reports/2026-q2.csv",
            "reports/2026-q3.csv",
        ]
        .iter()
        .enumerate()
        {
            put(
                &mut demo,
                k,
                2048 * (i + 1),
                30 - i as i64 * 10,
                "STANDARD",
                &format!("v{i}a"),
            );
        }
        put(
            &mut demo,
            "images/logo.png",
            48_000,
            200,
            "STANDARD_IA",
            "img1",
        );
        put(
            &mut demo,
            "images/banner.jpg",
            512_000,
            100,
            "STANDARD",
            "img2",
        );
        put(
            &mut demo,
            "backups/db-2026-10-01.tar.zst",
            3 * 1024 * 1024,
            36,
            "GLACIER",
            "bk1",
        );
        put(&mut demo, "README.md", 900, 300, "STANDARD", "rd1");
        // a key with history: three versions, then deleted
        for (i, d) in [90, 60, 20].iter().enumerate() {
            put(
                &mut demo,
                "config/app.yaml",
                300 + i * 40,
                *d,
                "STANDARD",
                &format!("cfg{i}"),
            );
        }
        demo.objects
            .get_mut("config/app.yaml")
            .unwrap()
            .push(Version {
                id: "cfg-dm".into(),
                data: None,
                time: now - Duration::days(5),
                class: "STANDARD",
            });
        demo.uploads.push(Upload {
            key: "backups/db-2026-08-12.tar.zst".into(),
            id: "upload-old-1".into(),
            initiated: now - Duration::days(86),
            parts: vec![
                (1, 64 * 1024 * 1024),
                (2, 64 * 1024 * 1024),
                (3, 12 * 1024 * 1024),
            ],
        });
        demo.uploads.push(Upload {
            key: "videos/raw-0042.mov".into(),
            id: "upload-old-2".into(),
            initiated: now - Duration::days(21),
            parts: vec![(1, 16 * 1024 * 1024)],
        });
        demo.uploads.push(Upload {
            key: "videos/raw-0043.mov".into(),
            id: "upload-new".into(),
            initiated: now - Duration::hours(3),
            parts: vec![(1, 8 * 1024 * 1024), (2, 8 * 1024 * 1024)],
        });
        s.buckets.insert("demo".into(), demo);
        s.buckets.insert(
            "demo-large".into(),
            Bucket {
                synthetic: LARGE_KEYS,
                created: Some(now - Duration::days(10)),
                ..Default::default()
            },
        );
        let mut layouts = Bucket {
            created: Some(now - Duration::days(1)),
            ..Default::default()
        };
        for (k, data) in samples::SAMPLES {
            layouts.objects.insert(
                (*k).to_string(),
                vec![Version {
                    id: "null".into(),
                    data: Some(data.to_vec()),
                    time: now - Duration::hours(2),
                    class: "STANDARD",
                }],
            );
        }
        s.buckets.insert("demo-layouts".into(), layouts);
        s
    }

    fn next(&mut self) -> String {
        self.seq += 1;
        format!("{:032x}", self.seq.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }
}

/// A running demo server.
pub struct DemoServer {
    pub addr: SocketAddr,
    _task: tokio::task::JoinHandle<()>,
}

impl DemoServer {
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

/// Starts the server on an ephemeral port of 127.0.0.1.
pub async fn start() -> std::io::Result<DemoServer> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(Mutex::new(State::seeded()));
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                continue;
            };
            let state = state.clone();
            tokio::spawn(async move {
                let mut buf = Vec::with_capacity(8192);
                loop {
                    // read one request (headers, then Content-Length bytes)
                    let head_end = loop {
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break Some(i);
                        }
                        let mut chunk = [0u8; 8192];
                        match sock.read(&mut chunk).await {
                            Ok(0) | Err(_) => break None,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    };
                    let Some(head_end) = head_end else { return };
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let mut lines = head.split("\r\n");
                    let request_line = lines.next().unwrap_or_default().to_string();
                    let headers: Vec<(String, String)> = lines
                        .filter_map(|l| {
                            l.split_once(':')
                                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                        })
                        .collect();
                    let len: usize = headers
                        .iter()
                        .find(|(k, _)| k == "content-length")
                        .and_then(|(_, v)| v.parse().ok())
                        .unwrap_or(0);
                    while buf.len() < head_end + 4 + len {
                        let mut chunk = [0u8; 65536];
                        match sock.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let body = buf[head_end + 4..head_end + 4 + len].to_vec();
                    buf.drain(..head_end + 4 + len);
                    let (status, extra, payload) = handle(&state, &request_line, &headers, body);
                    let rid = format!("DEMO{:012X}", rand_id());
                    let mut resp = format!(
                        "HTTP/1.1 {status} {}\r\nx-amz-request-id: {rid}\r\nx-amz-id-2: demo/{rid}\r\nserver: sonde-demo\r\ncontent-length: {}\r\n",
                        reason(status),
                        payload.len()
                    );
                    for (k, v) in extra {
                        resp.push_str(&format!("{k}: {v}\r\n"));
                    }
                    resp.push_str("\r\n");
                    let is_head = request_line.starts_with("HEAD ");
                    if sock.write_all(resp.as_bytes()).await.is_err() {
                        return;
                    }
                    if !is_head && sock.write_all(&payload).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    Ok(DemoServer { addr, _task: task })
}

fn rand_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

fn reason(s: u16) -> &'static str {
    match s {
        200 => "OK",
        204 => "No Content",
        206 => "Partial Content",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        416 => "Range Not Satisfiable",
        501 => "Not Implemented",
        _ => "",
    }
}

type Resp = (u16, Vec<(String, String)>, Vec<u8>);

fn xml(body: String) -> Resp {
    (
        200,
        vec![("content-type".into(), "application/xml".into())],
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{body}").into_bytes(),
    )
}

fn err(status: u16, code: &str, msg: &str) -> Resp {
    (status, vec![("content-type".into(), "application/xml".into())], format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>{code}</Code><Message>{msg}</Message></Error>").into_bytes())
}

fn decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .to_string()
}

fn handle(state: &Mutex<State>, line: &str, headers: &[(String, String)], body: Vec<u8>) -> Resp {
    let mut parts = line.split(' ');
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let q: BTreeMap<String, String> = query
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (decode(k), decode(v)),
            None => (decode(p), String::new()),
        })
        .collect();
    let segs: Vec<&str> = path.trim_start_matches('/').splitn(2, '/').collect();
    let bucket = decode(segs.first().copied().unwrap_or(""));
    let key = segs.get(1).map(|k| decode(k)).unwrap_or_default();
    let header = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    let mut st = state.lock();
    if bucket.is_empty() {
        let mut out = String::from(
            "<ListAllMyBucketsResult><Owner><ID>demo</ID><DisplayName>sonde demo</DisplayName></Owner><Buckets>",
        );
        for (name, b) in &st.buckets {
            out.push_str(&format!(
                "<Bucket><Name>{name}</Name><CreationDate>{}</CreationDate></Bucket>",
                ts(b.created.unwrap_or_else(Utc::now))
            ));
        }
        out.push_str("</Buckets></ListAllMyBucketsResult>");
        return xml(out);
    }
    if !st.buckets.contains_key(&bucket) {
        return err(404, "NoSuchBucket", "The specified bucket does not exist");
    }
    let seq_id = st.next();
    let b = st.buckets.get_mut(&bucket).unwrap();
    let now = Utc::now();
    if key.is_empty() {
        return match (method, ()) {
            ("GET", _) if q.contains_key("versioning") => xml(format!(
                "<VersioningConfiguration>{}</VersioningConfiguration>",
                b.versioning
                    .map(|v| format!("<Status>{v}</Status><MfaDelete>Disabled</MfaDelete>"))
                    .unwrap_or_default()
            )),
            ("GET", _) if q.contains_key("location") => {
                xml("<LocationConstraint></LocationConstraint>".into())
            }
            ("GET", _) if q.contains_key("object-lock") => err(
                404,
                "ObjectLockConfigurationNotFoundError",
                "Object Lock configuration does not exist for this bucket",
            ),
            ("GET", _) if q.contains_key("lifecycle") => match &b.lifecycle {
                Some(l) => xml(l.clone()),
                None => err(
                    404,
                    "NoSuchLifecycleConfiguration",
                    "The lifecycle configuration does not exist",
                ),
            },
            ("PUT", _) if q.contains_key("lifecycle") => {
                b.lifecycle = Some(String::from_utf8_lossy(&body).to_string());
                (200, vec![], vec![])
            }
            ("GET", _) if q.contains_key("uploads") => {
                let prefix = q.get("prefix").cloned().unwrap_or_default();
                let mut out = format!(
                    "<ListMultipartUploadsResult><Bucket>{bucket}</Bucket><IsTruncated>false</IsTruncated>"
                );
                for u in b.uploads.iter().filter(|u| u.key.starts_with(&prefix)) {
                    out.push_str(&format!("<Upload><Key>{}</Key><UploadId>{}</UploadId><Initiated>{}</Initiated><StorageClass>STANDARD</StorageClass></Upload>", esc(&u.key), u.id, ts(u.initiated)));
                }
                out.push_str("</ListMultipartUploadsResult>");
                xml(out)
            }
            ("GET", _) if q.contains_key("versions") => {
                let prefix = q.get("prefix").cloned().unwrap_or_default();
                let mut out = format!(
                    "<ListVersionsResult><Name>{bucket}</Name><Prefix>{}</Prefix><IsTruncated>false</IsTruncated>",
                    esc(&prefix)
                );
                for (k, vs) in b
                    .objects
                    .range(prefix.clone()..)
                    .take_while(|(k, _)| k.starts_with(&prefix))
                {
                    for (i, v) in vs.iter().enumerate().rev() {
                        let latest = i == vs.len() - 1;
                        match &v.data {
                            Some(d) => out.push_str(&format!("<Version><Key>{}</Key><VersionId>{}</VersionId><IsLatest>{latest}</IsLatest><LastModified>{}</LastModified><ETag>{}</ETag><Size>{}</Size><StorageClass>{}</StorageClass></Version>", esc(k), v.id, ts(v.time), esc(&etag(d)), d.len(), v.class)),
                            None => out.push_str(&format!("<DeleteMarker><Key>{}</Key><VersionId>{}</VersionId><IsLatest>{latest}</IsLatest><LastModified>{}</LastModified></DeleteMarker>", esc(k), v.id, ts(v.time))),
                        }
                    }
                }
                if b.synthetic > 0 {
                    return err(
                        501,
                        "NotImplemented",
                        "demo-large lists current versions only",
                    );
                }
                out.push_str("</ListVersionsResult>");
                xml(out)
            }
            ("GET", _) => list_v2(&bucket, b, &q),
            ("HEAD", _) => (200, vec![], vec![]),
            _ => err(501, "NotImplemented", "not in the demo server"),
        };
    }
    // object operations
    if let Some(up) = q.get("uploadId") {
        let Some(pos) = b.uploads.iter().position(|u| &u.id == up && u.key == key) else {
            return err(404, "NoSuchUpload", "The specified upload does not exist");
        };
        return match method {
            "GET" => {
                let u = &b.uploads[pos];
                let mut out = format!(
                    "<ListPartsResult><Bucket>{bucket}</Bucket><Key>{}</Key><UploadId>{up}</UploadId><IsTruncated>false</IsTruncated>",
                    esc(&key)
                );
                for (n, size) in &u.parts {
                    out.push_str(&format!("<Part><PartNumber>{n}</PartNumber><LastModified>{}</LastModified><ETag>\"{:032x}\"</ETag><Size>{size}</Size></Part>", ts(u.initiated), n));
                }
                out.push_str("</ListPartsResult>");
                xml(out)
            }
            "DELETE" => {
                b.uploads.remove(pos);
                (204, vec![], vec![])
            }
            _ => err(501, "NotImplemented", "uploads are read-only in the demo"),
        };
    }
    match method {
        "GET" | "HEAD" => {
            let (data, v) = match find(b, &key, q.get("versionId")) {
                Ok(x) => x,
                Err(e) => return e,
            };
            let mut h = vec![
                ("etag".to_string(), etag(&data)),
                (
                    "last-modified".into(),
                    v.time.to_rfc2822().replace("+0000", "GMT"),
                ),
                ("x-amz-storage-class".into(), v.class.into()),
                ("content-type".into(), "application/octet-stream".into()),
                ("x-amz-meta-source".into(), "sonde-demo".into()),
            ];
            if v.id != "null" {
                h.push(("x-amz-version-id".into(), v.id.clone()));
            }
            if let Some(r) =
                header("range").and_then(|r| r.strip_prefix("bytes=").map(String::from))
            {
                let len = data.len() as u64;
                let (s, e) = match r.split_once('-') {
                    Some(("", n)) => (
                        len.saturating_sub(n.parse().unwrap_or(0)),
                        len.saturating_sub(1),
                    ),
                    Some((a, "")) => (a.parse().unwrap_or(0), len.saturating_sub(1)),
                    Some((a, z)) => (
                        a.parse().unwrap_or(0),
                        z.parse::<u64>().unwrap_or(0).min(len.saturating_sub(1)),
                    ),
                    None => (0, len.saturating_sub(1)),
                };
                if s >= len {
                    return err(
                        416,
                        "InvalidRange",
                        "The requested range is not satisfiable",
                    );
                }
                h.push(("content-range".into(), format!("bytes {s}-{e}/{len}")));
                return (206, h, data[s as usize..=e as usize].to_vec());
            }
            (200, h, data)
        }
        "PUT" => {
            let id = if b.versioning == Some("Enabled") {
                seq_id
            } else {
                "null".into()
            };
            let e = etag(&body);
            let vs = b.objects.entry(key).or_default();
            if id == "null" {
                vs.retain(|v| v.id != "null");
            }
            vs.push(Version {
                id: id.clone(),
                data: Some(body),
                time: now,
                class: "STANDARD",
            });
            (
                200,
                vec![("etag".into(), e), ("x-amz-version-id".into(), id)],
                vec![],
            )
        }
        "DELETE" => {
            if let Some(vid) = q.get("versionId") {
                if let Some(vs) = b.objects.get_mut(&key) {
                    vs.retain(|v| &v.id != vid);
                    if vs.is_empty() {
                        b.objects.remove(&key);
                    }
                }
                return (204, vec![("x-amz-version-id".into(), vid.clone())], vec![]);
            }
            if b.versioning == Some("Enabled") {
                b.objects.entry(key).or_default().push(Version {
                    id: seq_id.clone(),
                    data: None,
                    time: now,
                    class: "STANDARD",
                });
                return (
                    204,
                    vec![
                        ("x-amz-delete-marker".into(), "true".into()),
                        ("x-amz-version-id".into(), seq_id),
                    ],
                    vec![],
                );
            }
            b.objects.remove(&key);
            (204, vec![], vec![])
        }
        _ => err(501, "NotImplemented", "not in the demo server"),
    }
}

fn find(b: &Bucket, key: &str, version: Option<&String>) -> Result<(Vec<u8>, Version), Resp> {
    if b.synthetic > 0 {
        if let Some(i) = key
            .rsplit_once("events-")
            .and_then(|(_, n)| n.strip_suffix(".json.gz"))
            .and_then(|n| n.parse::<usize>().ok())
            && i < b.synthetic
            && synthetic_key(i) == key
        {
            let size = 1000 + (i * 7919) % 250_000;
            return Ok((
                vec![0u8; size],
                Version {
                    id: "null".into(),
                    data: None,
                    time: Utc::now() - Duration::minutes((b.synthetic - i) as i64),
                    class: "STANDARD",
                },
            ));
        }
        return Err(err(404, "NoSuchKey", "The specified key does not exist."));
    }
    let vs = b
        .objects
        .get(key)
        .ok_or_else(|| err(404, "NoSuchKey", "The specified key does not exist."))?;
    let v = match version {
        Some(id) => vs.iter().find(|v| &v.id == id).ok_or_else(|| {
            err(
                404,
                "NoSuchVersion",
                "The specified version does not exist.",
            )
        })?,
        None => vs.last().unwrap(),
    };
    match &v.data {
        Some(d) => Ok((d.clone(), v.clone())),
        None => Err(err(404, "NoSuchKey", "The specified key does not exist.")),
    }
}

fn list_v2(bucket: &str, b: &Bucket, q: &BTreeMap<String, String>) -> Resp {
    let prefix = q.get("prefix").cloned().unwrap_or_default();
    let delim = q.get("delimiter").cloned().filter(|d| !d.is_empty());
    let max: usize = q
        .get("max-keys")
        .and_then(|m| m.parse().ok())
        .unwrap_or(1000)
        .min(1000);
    let start = q
        .get("continuation-token")
        .cloned()
        .or_else(|| q.get("start-after").cloned())
        .unwrap_or_default();
    let url = q.get("encoding-type").is_some_and(|e| e == "url");
    let now = Utc::now();
    let mut contents = vec![];
    let mut prefixes: Vec<String> = vec![];
    let mut last_key = String::new();
    let mut truncated = false;
    let push = |k: &str,
                size: usize,
                t: DateTime<Utc>,
                class: &str,
                et: String,
                contents: &mut Vec<String>,
                prefixes: &mut Vec<String>|
     -> bool {
        if contents.len() + prefixes.len() >= max {
            return false;
        }
        if let Some(d) = &delim
            && let Some(i) = k[prefix.len()..].find(d.as_str())
        {
            let p = k[..prefix.len() + i + d.len()].to_string();
            if prefixes.last() != Some(&p) {
                prefixes.push(p);
            }
            return true;
        }
        contents.push(format!("<Contents><Key>{}</Key><LastModified>{}</LastModified><ETag>{}</ETag><Size>{size}</Size><StorageClass>{class}</StorageClass></Contents>", enc(k, url), ts(t), esc(&et)));
        true
    };
    if b.synthetic > 0 {
        // synthetic keys are generated in order of index; their names sort by index too
        let mut i = if start.is_empty() {
            0
        } else {
            start
                .rsplit_once("events-")
                .and_then(|(_, n)| n.split('.').next())
                .and_then(|n| n.parse::<usize>().ok())
                .map(|n| n + 1)
                .unwrap_or(0)
        };
        while i < b.synthetic {
            let k = synthetic_key(i);
            if k.starts_with(&prefix) && k > start {
                let size = 1000 + (i * 7919) % 250_000;
                let fake = format!("\"{:032x}\"", i);
                if !push(
                    &k,
                    size,
                    now - Duration::minutes((b.synthetic - i) as i64),
                    "STANDARD",
                    fake,
                    &mut contents,
                    &mut prefixes,
                ) {
                    truncated = true;
                    break;
                }
                last_key = k;
            }
            i += 1;
        }
    } else {
        for (k, vs) in b.objects.range(prefix.clone()..) {
            if !k.starts_with(&prefix) {
                break;
            }
            if k.as_str() <= start.as_str() {
                continue;
            }
            let Some(v) = vs.last() else { continue };
            let Some(d) = &v.data else { continue };
            if let Some(dl) = &delim
                && let Some(p) = prefixes.last()
                && k.starts_with(p.as_str())
                && k[prefix.len()..].contains(dl.as_str())
            {
                last_key = k.clone();
                continue;
            }
            if !push(
                k,
                d.len(),
                v.time,
                v.class,
                etag(d),
                &mut contents,
                &mut prefixes,
            ) {
                truncated = true;
                break;
            }
            last_key = k.clone();
        }
    }
    let mut out = format!(
        "<ListBucketResult><Name>{bucket}</Name><Prefix>{}</Prefix><KeyCount>{}</KeyCount><MaxKeys>{max}</MaxKeys><IsTruncated>{truncated}</IsTruncated>",
        enc(&prefix, url),
        contents.len() + prefixes.len()
    );
    if let Some(d) = &delim {
        out.push_str(&format!("<Delimiter>{}</Delimiter>", enc(d, url)));
    }
    if url {
        out.push_str("<EncodingType>url</EncodingType>");
    }
    if truncated {
        out.push_str(&format!(
            "<NextContinuationToken>{}</NextContinuationToken>",
            esc(&last_key)
        ));
    }
    for c in contents {
        out.push_str(&c);
    }
    for p in prefixes {
        out.push_str(&format!(
            "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
            enc(&p, url)
        ));
    }
    out.push_str("</ListBucketResult>");
    xml(out)
}

/// Adds the demo endpoint (`demo`) to `sonde`, starting the server.
pub async fn install(s: &crate::Sonde) -> Result<crate::EndpointView, String> {
    // The server lives on a thread (and a runtime) of its own: it outlasts
    // whichever runtime asked for it first.
    static SERVER: std::sync::OnceLock<Result<DemoServer, String>> = std::sync::OnceLock::new();
    let server = SERVER
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::Builder::new()
                .name("sonde-demo".into())
                .spawn(move || {
                    let rt = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(rt) => rt,
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string()));
                            return;
                        }
                    };
                    rt.block_on(async move {
                        match start().await {
                            Ok(server) => {
                                let _ = tx.send(Ok(server));
                                std::future::pending::<()>().await
                            }
                            Err(e) => {
                                let _ = tx.send(Err(e.to_string()));
                            }
                        }
                    })
                })
                .map_err(|e| e.to_string())?;
            rx.recv().map_err(|e| e.to_string())?
        })
        .as_ref()
        .map_err(|e| e.clone())?;
    let mut e = crate::EndpointConfig::new(
        "demo",
        "Demo (built-in)",
        &server.url(),
        "us-east-1",
        crate::Vendor::Other,
    );
    e.path_style = true;
    e.credentials = crate::CredentialSource::Static {
        access_key_id: "AKIADEMODEMODEMO0000".into(),
        secret_access_key: "demo".into(),
    };
    s.add_transient_endpoint(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn serves_the_demo_buckets() {
        let dir = tempfile::tempdir().unwrap();
        let s = crate::Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        install(&s).await.unwrap();
        let buckets = crate::services::browse::list_buckets(&s, "demo")
            .await
            .unwrap();
        assert_eq!(buckets.len(), 3);
        let root =
            crate::services::browse::list_page(&s, "demo", "demo", "", Some("/"), None, None)
                .await
                .unwrap();
        let names: Vec<&str> = root.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["backups/", "images/", "reports/", "README.md"],
            "config/ is deleted (its latest is a marker)"
        );
        let large =
            crate::services::browse::list_page(&s, "demo", "demo-large", "", None, None, None)
                .await
                .unwrap();
        assert_eq!(large.entries.len(), 1000);
        assert!(large.is_truncated);
        let next = crate::services::browse::list_page(
            &s,
            "demo",
            "demo-large",
            "",
            None,
            large.next_token.clone(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(next.entries[0].key, synthetic_key(1000));
        let t = crate::services::versions::timeline(&s, "demo", "demo", "config/app.yaml")
            .await
            .unwrap();
        assert!(t.deleted);
        assert_eq!(t.versions.len(), 4);
        let d = crate::services::browse::object_detail(&s, "demo", "demo", "images/logo.png", None)
            .await
            .unwrap();
        assert_eq!(d.storage_class, "STANDARD_IA");
        let found = decoders::detect(
            &crate::services::layout::S3Source {
                client: s.client("demo").unwrap(),
                bucket: "demo-layouts".into(),
                max_objects: 10_000,
                ctx: None,
            },
            "",
        )
        .await
        .unwrap();
        assert!(found.len() >= 4, "{found:?}");
    }
}

#[cfg(test)]
mod paging {
    #[tokio::test]
    async fn lists_every_synthetic_key() {
        let dir = tempfile::tempdir().unwrap();
        let s = crate::Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        super::install(&s).await.unwrap();
        let all = s
            .client("demo")
            .unwrap()
            .list_all("demo-large", "", 1_000_000)
            .await
            .unwrap();
        assert_eq!(all.len(), super::LARGE_KEYS);
    }
}
