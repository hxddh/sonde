//! Sessions made up for tests of size: a lake job of sixteen workers (reads
//! in ranges, small writes, paged listings, multipart uploads, a hot
//! manifest, 503 storms with retries, lookups that find nothing), and the two
//! shapes that were quadratic. The same seed makes the same file.

#![allow(dead_code)]

use std::io::Write;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use reqlog::{Header, Record};
use sonde_proxy::session::SessionHeader;

/// A small generator of its own: the same numbers on every machine.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// In [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    /// Around `median`, skewed as latencies are.
    pub fn around(&mut self, median: f64) -> f64 {
        let u = self.unit() + self.unit() + self.unit() - 1.5;
        median * (u * 0.9).exp()
    }
}

const UA: &str = "Boto3/1.43.127 md/Botocore#1.43.127 ua/2.1 os/linux#6.8.0 md/arch#x86_64 lang/python#3.13.18 md/pyimpl#CPython m/b,N,Z,g,D cfg/retry-mode#standard Botocore/1.43.127";
const URL: &str = "https://s3.us-east-1.amazonaws.com";
const BUCKET: &str = "lake-prod";

fn t0() -> DateTime<Utc> {
    DateTime::from_timestamp(1_794_000_000, 0).expect("a time")
}

fn at(seconds: f64) -> DateTime<Utc> {
    t0() + Duration::microseconds((seconds * 1e6) as i64)
}

/// One request as the proxy records it, headers and all.
#[allow(clippy::too_many_arguments)]
fn record(
    rng: &mut Rng,
    t: f64,
    op: &str,
    method: &str,
    key: Option<&str>,
    query: &[(&str, String)],
    status: u16,
    ms: f64,
    up: u64,
    down: u64,
    attempt: u32,
) -> Record {
    let id = rng.next();
    let request_id = format!("{id:016X}");
    let qs: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let url = format!(
        "{URL}/{BUCKET}/{}{}{}",
        key.unwrap_or(""),
        if qs.is_empty() { "" } else { "?" },
        qs.join("&")
    );
    let when = at(t);
    let amz = when.format("%Y%m%dT%H%M%SZ").to_string();
    let mut request_headers = vec![
        Header::new("host", "127.0.0.1:42667"),
        Header::new("accept-encoding", "identity"),
        Header::new("user-agent", UA),
        Header::new("x-amz-date", amz.clone()),
        Header::new("x-amz-content-sha256", "UNSIGNED-PAYLOAD"),
        Header::new(
            "authorization",
            format!(
                "AWS4-HMAC-SHA256 Credential=SOND…6F/{}/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=«redacted»",
                &amz[..8]
            ),
        ),
        Header::new(
            "amz-sdk-invocation-id",
            format!("{:08x}-0000-4000-8000-{:012x}", id as u32, id >> 16),
        ),
        Header::new("amz-sdk-request", format!("attempt={attempt}")),
    ];
    if method == "PUT" || method == "POST" {
        request_headers.push(Header::new("content-length", up.to_string()));
    }
    let failed = status >= 400;
    let error_body = (failed && method != "HEAD").then(|| {
        let (code, message) = match status {
            503 => ("SlowDown", "Please reduce your request rate."),
            500 => ("InternalError", "We encountered an internal error. Please try again."),
            404 => ("NoSuchKey", "The specified key does not exist."),
            _ => ("Error", "error"),
        };
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>{code}</Code><Message>{message}</Message><RequestId>{request_id}</RequestId></Error>"
        )
    });
    let down = error_body.as_ref().map_or(down, |b| b.len() as u64);
    let mut response_headers = vec![
        Header::new("x-amz-request-id", request_id.clone()),
        Header::new(
            "x-amz-id-2",
            format!("h/{request_id}aGVsbG8gd29ybGQgaG9zdCBpZA=="),
        ),
        Header::new("server", "AmazonS3"),
        Header::new("content-length", down.to_string()),
    ];
    if !failed && matches!(op, "GetObject" | "HeadObject" | "PutObject" | "UploadPart") {
        response_headers.push(Header::new(
            "etag",
            format!("\"{:016x}{:016x}\"", id, id.rotate_left(17)),
        ));
    }
    if !failed && matches!(op, "GetObject" | "HeadObject") {
        response_headers.push(Header::new(
            "last-modified",
            "Tue, 10 Nov 2026 01:00:00 GMT",
        ));
        response_headers.push(Header::new("content-type", "application/octet-stream"));
    }
    Record {
        id: 0,
        time: when,
        endpoint: "prod".into(),
        operation: op.into(),
        method: method.into(),
        url,
        bucket: Some(BUCKET.into()),
        key: key.map(Into::into),
        query: query.iter().map(|(k, v)| ((*k).into(), v.into())).collect(),
        status: Some(status),
        latency_ms: (ms * 1000.0).round() / 1000.0,
        retries: 0,
        request_id: Some(request_id.clone()),
        host_id: Some(format!("h/{request_id}")),
        request_headers,
        response_headers,
        request_bytes: Some(up),
        response_bytes: Some(down),
        error_body,
        region: Some("us-east-1".into()),
        first_byte_ms: Some((ms * (0.3 + 0.6 * rng.unit()) * 1000.0).round() / 1000.0),
        ..Default::default()
    }
}

/// The lake job's state: the files each prefix holds.
struct Lake {
    rng: Rng,
    prefixes: Vec<String>,
    files: Vec<Vec<String>>,
    seq: u64,
    out: Vec<(f64, Record)>,
}

impl Lake {
    fn stormy(t: f64) -> bool {
        let m = t % 1800.0;
        (60.0..75.0).contains(&m)
    }

    /// One logical request with its retries; returns when it ended.
    #[allow(clippy::too_many_arguments)]
    fn send(
        &mut self,
        mut t: f64,
        op: &str,
        method: &str,
        key: Option<&str>,
        query: &[(&str, String)],
        ms: f64,
        up: u64,
        down: u64,
        missing: bool,
    ) -> f64 {
        let mut attempt = 1;
        loop {
            let slow = if Self::stormy(t) { 0.25 } else { 0.001 };
            let x = self.rng.unit();
            let (status, took, wait) = if missing {
                (404, self.rng.around(12.0), None)
            } else if x < slow && attempt < 5 {
                let backoff =
                    (0.1 * f64::from(1 << attempt)).min(20.0) * (0.5 + 0.5 * self.rng.unit());
                (503, self.rng.around(8.0), Some(backoff))
            } else if x < slow + 0.0005 && attempt < 5 {
                (
                    500,
                    self.rng.around(900.0),
                    Some(0.2 * f64::from(1 << attempt) * self.rng.unit()),
                )
            } else {
                (if op == "DeleteObject" { 204 } else { 200 }, ms, None)
            };
            let ok = status < 300;
            let r = record(
                &mut self.rng,
                t,
                op,
                method,
                key,
                query,
                status,
                took,
                if ok || method != "GET" { up } else { 0 },
                if ok { down } else { 0 },
                attempt,
            );
            self.out.push((t, r));
            t += took / 1000.0;
            match wait {
                Some(w) => {
                    t += w;
                    attempt += 1;
                }
                None => return t,
            }
        }
    }

    /// One piece of a worker's work, from `t`; returns when it ended.
    fn task(&mut self, mut t: f64) -> f64 {
        let x = self.rng.unit();
        // half the work is on a few hot prefixes
        let p = if self.rng.unit() < 0.5 {
            let u = self.rng.unit().max(1e-9);
            ((1.0 / u.powf(1.0 / 1.1)) as usize - 1).min(self.prefixes.len() - 1)
        } else {
            self.rng.below(self.prefixes.len())
        };
        let prefix = self.prefixes[p].clone();
        if x < 0.50 {
            // read a file: sometimes HEAD first, then ranged GETs
            let known = !self.files[p].is_empty() && self.rng.unit() < 0.97;
            let key = if known {
                let i = self.rng.below(self.files[p].len());
                self.files[p][i].clone()
            } else {
                format!("{prefix}part-{:05}-missing.parquet", self.rng.below(99_999))
            };
            if self.rng.unit() < 0.3 {
                let ms = self.rng.around(14.0);
                t = self.send(t, "HeadObject", "HEAD", Some(&key), &[], ms, 0, 0, !known);
            }
            if !known {
                t = self.send(t, "GetObject", "GET", Some(&key), &[], 0.0, 0, 0, true);
            } else {
                for _ in 0..[1, 1, 2, 3, 5][self.rng.below(5)] {
                    let size = (self.rng.around(440_000.0) as u64) + 100;
                    let ms = self.rng.around(25.0) + size as f64 / 60e6 * 1000.0;
                    t = self.send(t, "GetObject", "GET", Some(&key), &[], ms, 0, size, false);
                    t += self.rng.unit() / 200.0;
                }
            }
        } else if x < 0.68 {
            // write a small file
            self.seq += 1;
            let key = format!(
                "{prefix}part-{:05}-{:08x}.parquet",
                self.seq,
                self.rng.next() as u32
            );
            let size = (self.rng.around(1_200_000.0) as u64) + 500;
            let ms = self.rng.around(40.0) + size as f64 / 40e6 * 1000.0;
            t = self.send(t, "PutObject", "PUT", Some(&key), &[], ms, size, 0, false);
            self.files[p].push(key);
        } else if x < 0.74 {
            // list a prefix, paged
            let mut token: Option<String> = None;
            for _ in 0..[1, 1, 2, 3, 8][self.rng.below(5)] {
                let mut q = vec![
                    ("list-type", "2".to_string()),
                    ("prefix", prefix.clone()),
                    ("encoding-type", "url".to_string()),
                ];
                if let Some(tok) = &token {
                    q.push(("continuation-token", tok.clone()));
                }
                let ms = self.rng.around(90.0);
                let down = 2000 + self.rng.below(298_000) as u64;
                t = self.send(t, "ListObjectsV2", "GET", None, &q, ms, 0, down, false);
                token = Some(format!("{:016x}{:016x}", self.rng.next(), self.rng.next()));
            }
        } else if x < 0.78 {
            // a multipart upload: parts of 8 MiB, four at a time
            self.seq += 1;
            let key = format!("{prefix}compacted-{:05}.parquet", self.seq);
            let upload = format!(
                "{:016x}{:016x}{:016x}{:016x}",
                self.rng.next(),
                self.rng.next(),
                self.rng.next(),
                self.rng.next()
            );
            let ms = self.rng.around(60.0);
            t = self.send(
                t,
                "CreateMultipartUpload",
                "POST",
                Some(&key),
                &[("uploads", String::new())],
                ms,
                0,
                300,
                false,
            );
            let parts = [3, 4, 6, 8, 12, 20, 40][self.rng.below(7)];
            let mut lanes = [t; 4];
            for part in 1..=parts {
                let lane = (0..4)
                    .min_by(|a, b| lanes[*a].total_cmp(&lanes[*b]))
                    .unwrap_or(0);
                let q = [
                    ("partNumber", part.to_string()),
                    ("uploadId", upload.clone()),
                ];
                let ms = self.rng.around(700.0);
                lanes[lane] = self.send(
                    lanes[lane],
                    "UploadPart",
                    "PUT",
                    Some(&key),
                    &q,
                    ms,
                    8 << 20,
                    0,
                    false,
                );
            }
            t = lanes.iter().copied().fold(t, f64::max);
            if self.rng.unit() < 0.03 {
                // left unfinished
            } else if self.rng.unit() < 0.03 {
                let ms = self.rng.around(40.0);
                t = self.send(
                    t,
                    "AbortMultipartUpload",
                    "DELETE",
                    Some(&key),
                    &[("uploadId", upload)],
                    ms,
                    0,
                    0,
                    false,
                );
            } else {
                let ms = self.rng.around(250.0);
                t = self.send(
                    t,
                    "CompleteMultipartUpload",
                    "POST",
                    Some(&key),
                    &[("uploadId", upload)],
                    ms,
                    120 * parts as u64,
                    320,
                    false,
                );
                self.files[p].push(key);
            }
        } else if x < 0.80 {
            if !self.files[p].is_empty() {
                let i = self.rng.below(self.files[p].len());
                let key = self.files[p].swap_remove(i);
                let ms = self.rng.around(30.0);
                t = self.send(
                    t,
                    "DeleteObject",
                    "DELETE",
                    Some(&key),
                    &[],
                    ms,
                    0,
                    0,
                    false,
                );
            }
        } else if x < 0.90 {
            // a hot manifest: read, sometimes rewritten
            let key = format!(
                "{}/metadata/current.json",
                prefix.split("/data/").next().unwrap_or("")
            );
            let ms = self.rng.around(18.0);
            t = self.send(t, "GetObject", "GET", Some(&key), &[], ms, 0, 4200, false);
            if self.rng.unit() < 0.1 {
                let ms = self.rng.around(35.0);
                t = self.send(t, "PutObject", "PUT", Some(&key), &[], ms, 4200, 0, false);
            }
        } else if !self.files[p].is_empty() {
            // HEAD, then GET whole
            let i = self.rng.below(self.files[p].len());
            let key = self.files[p][i].clone();
            let ms = self.rng.around(14.0);
            t = self.send(t, "HeadObject", "HEAD", Some(&key), &[], ms, 0, 0, false);
            let size = self.rng.around(160_000.0) as u64;
            let ms = self.rng.around(25.0) + size as f64 / 60e6 * 1000.0;
            t = self.send(t, "GetObject", "GET", Some(&key), &[], ms, 0, size, false);
        }
        t
    }
}

/// Writes records as a finished (compressed) session.
struct Out {
    gz: flate2::write::GzEncoder<std::io::BufWriter<std::fs::File>>,
    n: u64,
}

impl Out {
    fn create(path: &Path, command: &str) -> Out {
        let file = std::fs::File::create(path).expect("the session file");
        let mut gz = flate2::write::GzEncoder::new(
            std::io::BufWriter::new(file),
            flate2::Compression::new(1),
        );
        let mut header = SessionHeader::new(
            "prod",
            "https://s3.us-east-1.amazonaws.com/",
            "us-east-1",
            Some(command.into()),
        );
        header.created = t0();
        serde_json::to_writer(&mut gz, &header).expect("the header");
        gz.write_all(b"\n").expect("written");
        Out { gz, n: 0 }
    }

    fn write(&mut self, mut r: Record) {
        self.n += 1;
        r.id = self.n;
        serde_json::to_writer(&mut self.gz, &r).expect("a record");
        self.gz.write_all(b"\n").expect("written");
    }

    fn finish(self) -> u64 {
        self.gz
            .finish()
            .expect("finished")
            .flush()
            .expect("flushed");
        self.n
    }
}

/// A lake job of at least `n` requests, sixteen workers, written to `path`.
/// Returns how many requests it has.
pub fn lake(n: usize, path: &Path, seed: u64) -> u64 {
    let tables: Vec<String> = (0..4)
        .flat_map(|d| (0..15).map(move |t| format!("warehouse/db{d}/t{t:02}")))
        .collect();
    let prefixes: Vec<String> = tables
        .iter()
        .flat_map(|t| (1..6).map(move |d| format!("{t}/data/dt=2026-11-{d:02}/")))
        .collect();
    let mut lake = Lake {
        rng: Rng::new(seed),
        files: vec![Vec::new(); prefixes.len()],
        prefixes,
        seq: 0,
        out: Vec::new(),
    };
    // think time sets the span: an hour or so at a million, minutes below
    let think = if n >= 500_000 {
        0.03
    } else if n >= 100_000 {
        0.12
    } else {
        0.3
    };
    let mut clocks: Vec<f64> = (0..16).map(|_| lake.rng.unit()).collect();
    let mut out = Out::create(path, "python lake_job.py --tables 60");
    let mut made = 0;
    // written in the order they were sent: what is older than every worker's
    // clock can no longer be overtaken
    let mut held: Vec<(f64, Record)> = Vec::new();
    while made < n {
        let w = (0..clocks.len())
            .min_by(|a, b| clocks[*a].total_cmp(&clocks[*b]))
            .unwrap_or(0);
        let end = lake.task(clocks[w]);
        made += lake.out.len();
        held.append(&mut lake.out);
        clocks[w] = end - think * (1.0 - lake.rng.unit()).max(1e-9).ln();
        if held.len() > 4096 {
            let floor = clocks.iter().copied().fold(f64::INFINITY, f64::min);
            held.sort_by(|a, b| a.0.total_cmp(&b.0));
            let ready = held.partition_point(|(t, _)| *t < floor);
            for (_, r) in held.drain(..ready) {
                out.write(r);
            }
        }
    }
    held.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, r) in held {
        out.write(r);
    }
    out.finish()
}

fn plain(
    t: f64,
    n: u64,
    op: &str,
    method: &str,
    key: &str,
    query: &[(&str, String)],
    status: u16,
    ms: f64,
    up: u64,
    down: u64,
) -> Record {
    let qs: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    Record {
        id: n,
        time: at(t),
        endpoint: "prod".into(),
        operation: op.into(),
        method: method.into(),
        url: format!(
            "https://s3.amazonaws.com/b/{key}{}{}",
            if qs.is_empty() { "" } else { "?" },
            qs.join("&")
        ),
        bucket: Some("b".into()),
        key: Some(key.into()),
        query: query.iter().map(|(k, v)| ((*k).into(), v.into())).collect(),
        status: Some(status),
        latency_ms: ms,
        request_id: Some(format!("{n:016X}")),
        request_headers: vec![Header::new("user-agent", "aws-sdk-java/2.25")],
        response_headers: vec![Header::new("x-amz-request-id", format!("{n:016X}"))],
        request_bytes: Some(up),
        response_bytes: Some(down),
        error_body: (status == 503).then(|| {
            "<Error><Code>SlowDown</Code><Message>Please reduce your request rate.</Message></Error>".into()
        }),
        region: Some("us-east-1".into()),
        first_byte_ms: Some(ms * 0.8),
        ..Default::default()
    }
}

/// An outage: `n` requests at a thousand a second to keys of their own, half
/// answered 503 and never sent again.
pub fn outage(n: usize, path: &Path) -> u64 {
    let mut out = Out::create(path, "synthetic outage");
    for i in 0..n {
        let bad = i % 2 == 0;
        let key = format!("data/{:03}/obj-{i:08}", i % 300);
        out.write(plain(
            i as f64 * 0.001,
            i as u64 + 1,
            "GetObject",
            "GET",
            &key,
            &[],
            if bad { 503 } else { 200 },
            if bad { 5.0 } else { 30.0 },
            0,
            if bad { 0 } else { 50_000 },
        ));
    }
    out.finish()
}

/// Small files uploaded in parts: `n / 3` uploads of one part each (Create,
/// one UploadPart, Complete), eight at once.
pub fn small_uploads(n: usize, path: &Path) -> u64 {
    let mut out = Out::create(path, "synthetic mpu");
    let mut id = 0;
    let mut t = 0.0;
    for u in 0..n / 3 {
        let key = format!("out/d{:03}/part-{u:07}.parquet", u % 200);
        let upload = format!("{u:032x}");
        let mut put =
            |t: f64, op: &str, method: &str, q: &[(&str, String)], ms: f64, up: u64, down: u64| {
                id += 1;
                out.write(plain(t, id, op, method, &key, q, 200, ms, up, down));
            };
        put(
            t,
            "CreateMultipartUpload",
            "POST",
            &[("uploads", String::new())],
            20.0,
            0,
            300,
        );
        put(
            t + 0.021,
            "UploadPart",
            "PUT",
            &[("partNumber", "1".into()), ("uploadId", upload.clone())],
            60.0,
            200_000,
            0,
        );
        put(
            t + 0.082,
            "CompleteMultipartUpload",
            "POST",
            &[("uploadId", upload)],
            30.0,
            150,
            300,
        );
        t += 0.115 / 8.0;
    }
    out.finish()
}
