//! A small in-memory S3 for tests: buckets, objects with conditional writes,
//! multipart uploads, paged ListObjectsV2, DeleteObjects. With
//! `honor_conditions: false` it ignores If-None-Match and If-Match, as some
//! stores do.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use parking_lot::Mutex;
use tokio::net::TcpListener;

type Object = (Vec<u8>, String);

#[derive(Default)]
pub struct State {
    pub buckets: BTreeMap<String, BTreeMap<String, Object>>,
    uploads: HashMap<String, (String, String, BTreeMap<u32, Object>)>,
    next: u64,
}

pub struct MiniS3 {
    pub url: String,
    pub state: Arc<Mutex<State>>,
}

fn etag(data: &[u8]) -> String {
    format!("\"{}\"", &s3raw::sigv4::sha256_hex(data)[..32])
}

fn tags<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{name}>"), format!("</{name}>"));
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

fn reply(status: u16, headers: &[(&str, String)], body: impl Into<Bytes>) -> Response<Full<Bytes>> {
    let mut b = Response::builder()
        .status(status)
        .header("x-amz-request-id", "MINI");
    for (k, v) in headers {
        b = b.header(*k, v);
    }
    b.body(Full::new(body.into())).unwrap()
}

fn err(status: u16, code: &str) -> Response<Full<Bytes>> {
    reply(
        status,
        &[("content-type", "application/xml".into())],
        format!("<Error><Code>{code}</Code><Message>{code}</Message></Error>"),
    )
}

async fn serve(
    st: Arc<Mutex<State>>,
    honor: bool,
    req: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let (parts, body) = req.into_parts();
    let body = body.collect().await.unwrap().to_bytes().to_vec();
    let host = parts
        .headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    let (bucket, key) = sonde_proxy::parse_target(&host, parts.uri.path());
    let q = sonde_proxy::parse_query(parts.uri.query());
    let has = |n: &str| q.iter().any(|(k, _)| k == n);
    let get = |n: &str| q.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    let header = |n: &str| {
        parts
            .headers
            .get(n)
            .and_then(|v| v.to_str().ok())
            .map(String::from)
    };
    let mut s = st.lock();
    let Some(bucket) = bucket else {
        return Ok(reply(200, &[], "<ListAllMyBucketsResult/>"));
    };
    let m = parts.method.as_str();
    Ok(match (m, key) {
        ("PUT", None) => match s.buckets.entry(bucket) {
            std::collections::btree_map::Entry::Occupied(_) => err(409, "BucketAlreadyOwnedByYou"),
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(BTreeMap::new());
                reply(200, &[], "")
            }
        },
        ("PUT", Some(k)) if has("uploadId") => {
            let id = get("uploadId").unwrap();
            let n: u32 = get("partNumber").and_then(|n| n.parse().ok()).unwrap_or(0);
            match s.uploads.get_mut(&id) {
                Some(u) => {
                    let e = etag(&body);
                    u.2.insert(n, (body, e.clone()));
                    let _ = k;
                    reply(200, &[("etag", e)], "")
                }
                None => err(404, "NoSuchUpload"),
            }
        }
        ("PUT", Some(k)) => {
            let objects = s.buckets.entry(bucket).or_default();
            let existing = objects.get(&k).map(|o| o.1.clone());
            if honor {
                if header("if-none-match").as_deref() == Some("*") && existing.is_some() {
                    return Ok(err(412, "PreconditionFailed"));
                }
                if let Some(want) = header("if-match")
                    && existing.as_deref() != Some(want.as_str())
                {
                    return Ok(err(412, "PreconditionFailed"));
                }
            }
            let e = etag(&body);
            objects.insert(k, (body, e.clone()));
            reply(200, &[("etag", e)], "")
        }
        ("POST", Some(k)) if has("uploads") => {
            s.next += 1;
            let id = format!("upload-{}", s.next);
            s.uploads
                .insert(id.clone(), (bucket.clone(), k.clone(), BTreeMap::new()));
            reply(
                200,
                &[],
                format!(
                    "<InitiateMultipartUploadResult><Bucket>{bucket}</Bucket><Key>{k}</Key><UploadId>{id}</UploadId></InitiateMultipartUploadResult>"
                ),
            )
        }
        ("POST", Some(_)) if has("uploadId") => {
            let id = get("uploadId").unwrap();
            let Some((b, k, stored)) = s.uploads.remove(&id) else {
                return Ok(err(404, "NoSuchUpload"));
            };
            let xml = String::from_utf8_lossy(&body).to_string();
            let numbers = tags(&xml, "PartNumber");
            let etags = tags(&xml, "ETag");
            let mut data = Vec::new();
            for (n, e) in numbers.iter().zip(etags.iter()) {
                let n: u32 = n.trim().parse().unwrap_or(0);
                match stored.get(&n) {
                    Some((d, pe)) if pe == e.trim() => data.extend_from_slice(d),
                    _ => return Ok(err(400, "InvalidPart")),
                }
            }
            let e = format!(
                "\"{}-{}\"",
                &s3raw::sigv4::sha256_hex(&data)[..32],
                numbers.len()
            );
            s.buckets.entry(b).or_default().insert(k, (data, e.clone()));
            reply(
                200,
                &[],
                format!(
                    "<CompleteMultipartUploadResult><ETag>{e}</ETag></CompleteMultipartUploadResult>"
                ),
            )
        }
        ("POST", None) if has("delete") => {
            let xml = String::from_utf8_lossy(&body).to_string();
            let objects = s.buckets.entry(bucket).or_default();
            let mut out = String::from("<DeleteResult>");
            for k in tags(&xml, "Key") {
                objects.remove(k);
                out.push_str(&format!("<Deleted><Key>{k}</Key></Deleted>"));
            }
            out.push_str("</DeleteResult>");
            reply(200, &[], out)
        }
        ("GET", None) if has("uploads") => {
            let prefix = get("prefix").unwrap_or_default();
            let mut out =
                String::from("<ListMultipartUploadsResult><IsTruncated>false</IsTruncated>");
            for (id, (b, k, _)) in &s.uploads {
                if *b == bucket && k.starts_with(&prefix) {
                    out.push_str(&format!(
                        "<Upload><Key>{k}</Key><UploadId>{id}</UploadId></Upload>"
                    ));
                }
            }
            out.push_str("</ListMultipartUploadsResult>");
            reply(200, &[], out)
        }
        ("GET", None) => {
            let prefix = get("prefix").unwrap_or_default();
            let max: usize = get("max-keys").and_then(|m| m.parse().ok()).unwrap_or(1000);
            let after = get("continuation-token").map(|t| t.trim_start_matches("ct:").to_string());
            let objects = s.buckets.entry(bucket).or_default();
            let keys: Vec<&String> = objects
                .keys()
                .filter(|k| k.starts_with(&prefix) && after.as_ref().is_none_or(|a| *k > a))
                .collect();
            let page = &keys[..keys.len().min(max)];
            let mut out = format!(
                "<ListBucketResult><Prefix>{prefix}</Prefix><KeyCount>{}</KeyCount>",
                page.len()
            );
            for k in page {
                let (d, e) = &objects[*k];
                out.push_str(&format!(
                    "<Contents><Key>{k}</Key><Size>{}</Size><ETag>{e}</ETag></Contents>",
                    d.len()
                ));
            }
            if keys.len() > page.len() {
                out.push_str(&format!("<IsTruncated>true</IsTruncated><NextContinuationToken>ct:{}</NextContinuationToken>", page.last().unwrap()));
            } else {
                out.push_str("<IsTruncated>false</IsTruncated>");
            }
            out.push_str("</ListBucketResult>");
            reply(200, &[], out)
        }
        ("GET" | "HEAD", Some(k)) => match s.buckets.get(&bucket).and_then(|o| o.get(&k)) {
            Some((d, e)) => {
                let len = d.len().to_string();
                reply(
                    200,
                    &[("etag", e.clone()), ("content-length", len)],
                    if m == "HEAD" { Vec::new() } else { d.clone() },
                )
            }
            None if m == "HEAD" => reply(404, &[], ""),
            None => err(404, "NoSuchKey"),
        },
        ("DELETE", Some(k)) if has("uploadId") => {
            s.uploads.remove(&get("uploadId").unwrap());
            let _ = k;
            reply(204, &[], "")
        }
        ("DELETE", Some(k)) => {
            if let Some(o) = s.buckets.get_mut(&bucket) {
                o.remove(&k);
            }
            reply(204, &[], "")
        }
        _ => err(501, "NotImplemented"),
    })
}

impl MiniS3 {
    pub async fn start(honor_conditions: bool) -> MiniS3 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state: Arc<Mutex<State>> = Arc::default();
        let st = state.clone();
        tokio::spawn(async move {
            loop {
                let (s, _) = listener.accept().await.unwrap();
                let st = st.clone();
                tokio::spawn(async move {
                    let svc = service_fn(move |r| serve(st.clone(), honor_conditions, r));
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(s), svc)
                        .await;
                });
            }
        });
        MiniS3 { url, state }
    }

    pub fn keys(&self, bucket: &str) -> Vec<String> {
        self.state
            .lock()
            .buckets
            .get(bucket)
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default()
    }
}
