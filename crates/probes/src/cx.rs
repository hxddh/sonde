//! What a probe runs with: a client tagged for its requests, a scratch
//! prefix (and bucket), and the steps it records for its judge.

use bytes::Bytes;
use parking_lot::Mutex;
use s3raw::{S3Client, S3Error, S3Request, S3Response};

use crate::model::{Step, Steps};

pub struct ProbeCx {
    pub client: S3Client,
    /// The scratch bucket.
    pub bucket: String,
    /// A prefix unique to this probe, ending in `/`.
    pub prefix: String,
    pub https: bool,
    /// Creates temporary buckets: `None` when the run may not.
    pub temp_bucket_prefix: Option<String>,
    steps: Mutex<Vec<Step>>,
    temp_buckets: Mutex<Vec<String>>,
}

const BODY_LIMIT: usize = 4096;

impl ProbeCx {
    pub fn new(
        client: S3Client,
        bucket: &str,
        prefix: &str,
        temp_bucket_prefix: Option<String>,
    ) -> Self {
        let https = client.config().endpoint.scheme() == "https";
        ProbeCx {
            client,
            bucket: bucket.into(),
            prefix: prefix.into(),
            https,
            temp_bucket_prefix,
            steps: Mutex::new(vec![]),
            temp_buckets: Mutex::new(vec![]),
        }
    }

    pub fn key(&self, name: &str) -> String {
        format!("{}{name}", self.prefix)
    }

    pub fn steps(&self) -> Steps {
        Steps(self.steps.lock().clone())
    }

    pub fn temp_buckets(&self) -> Vec<String> {
        self.temp_buckets.lock().clone()
    }

    /// Records the outcome of a request under `label`; the response if any.
    pub fn record(&self, label: &str, r: Result<S3Response, S3Error>) -> Option<S3Response> {
        let step = match &r {
            Ok(resp) => {
                let err = resp.error();
                let mut body = reqlog::redact::redact_text(&resp.text());
                truncate(&mut body);
                Step {
                    label: label.into(),
                    status: Some(resp.status),
                    code: err.and_then(|e| e.code),
                    headers: resp.headers.clone(),
                    body,
                    record_id: Some(resp.record_id),
                }
            }
            Err(e) => {
                let mut body = e.body.clone().unwrap_or_else(|| e.message.clone());
                truncate(&mut body);
                Step {
                    label: label.into(),
                    status: e.status,
                    code: e.code.clone(),
                    headers: vec![],
                    body,
                    record_id: e.record_id,
                }
            }
        };
        self.steps.lock().push(step);
        r.ok()
    }

    /// Records a value the probe computed (no request), as a step's body.
    pub fn note(&self, label: &str, body: impl Into<String>) {
        self.steps.lock().push(Step {
            label: label.into(),
            status: None,
            code: None,
            headers: vec![],
            body: body.into(),
            record_id: None,
        });
    }

    pub async fn send(&self, label: &str, req: S3Request) -> Option<S3Response> {
        let r = self.client.send_raw(req.no_retry()).await;
        self.record(label, r)
    }

    pub async fn put(
        &self,
        label: &str,
        key: &str,
        body: impl Into<Bytes>,
        headers: &[(&str, &str)],
    ) -> Option<S3Response> {
        let mut req = S3Request::new("PutObject", http::Method::PUT)
            .bucket(&self.bucket)
            .key(key)
            .body(body.into());
        for (k, v) in headers {
            req = req.header(k, *v);
        }
        self.send(label, req).await
    }

    pub async fn get(
        &self,
        label: &str,
        key: &str,
        headers: &[(&str, &str)],
    ) -> Option<S3Response> {
        let mut req = S3Request::new("GetObject", http::Method::GET)
            .bucket(&self.bucket)
            .key(key);
        for (k, v) in headers {
            req = req.header(k, *v);
        }
        self.send(label, req).await
    }

    pub async fn head(
        &self,
        label: &str,
        key: &str,
        headers: &[(&str, &str)],
    ) -> Option<S3Response> {
        let mut req = S3Request::new("HeadObject", http::Method::HEAD)
            .bucket(&self.bucket)
            .key(key);
        for (k, v) in headers {
            req = req.header(k, *v);
        }
        self.send(label, req).await
    }

    pub async fn delete(&self, label: &str, key: &str) -> Option<S3Response> {
        self.send(
            label,
            S3Request::new("DeleteObject", http::Method::DELETE)
                .bucket(&self.bucket)
                .key(key),
        )
        .await
    }

    /// Creates a bucket of its own for the probe (removed by the runner).
    pub async fn temp_bucket(&self, label: &str, object_lock: bool) -> Option<String> {
        let prefix = self.temp_bucket_prefix.as_ref()?;
        let name = format!(
            "{prefix}-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let r = self.client.create_bucket(&name, object_lock).await;
        let ok = self.record(label, r).is_some_and(|r| r.is_success());
        if ok {
            self.temp_buckets.lock().push(name.clone());
            Some(name)
        } else {
            None
        }
    }

    /// Creates an upload; its id.
    pub async fn create_upload(
        &self,
        label: &str,
        key: &str,
        headers: &[(&str, &str)],
    ) -> Option<String> {
        let mut req = S3Request::new("CreateMultipartUpload", http::Method::POST)
            .bucket(&self.bucket)
            .key(key)
            .query("uploads", "");
        for (k, v) in headers {
            req = req.header(k, *v);
        }
        let r = self.send(label, req).await?;
        if !r.is_success() {
            return None;
        }
        s3raw::xml::initiate_multipart_upload(&r.text())
            .ok()
            .map(|u| u.upload_id)
    }

    pub async fn upload_part(
        &self,
        label: &str,
        key: &str,
        upload: &str,
        n: u32,
        body: Bytes,
        headers: &[(&str, &str)],
    ) -> Option<S3Response> {
        let mut req = S3Request::new("UploadPart", http::Method::PUT)
            .bucket(&self.bucket)
            .key(key)
            .query("partNumber", n.to_string())
            .query("uploadId", upload)
            .body(body);
        for (k, v) in headers {
            req = req.header(k, *v);
        }
        self.send(label, req).await
    }

    pub async fn complete(
        &self,
        label: &str,
        key: &str,
        upload: &str,
        parts: &[s3raw::xml::CompletedPart],
        headers: &[(&str, &str)],
    ) -> Option<S3Response> {
        let r = self
            .client
            .complete_multipart_upload_raw(&self.bucket, key, upload, parts, headers)
            .await;
        // CompleteMultipartUpload may answer 200 with an <Error> body
        let r = r.map(|mut resp| {
            if resp.is_success() && resp.text().contains("<Error>") {
                resp.status = 500;
            }
            resp
        });
        self.record(label, r)
    }
}

fn truncate(s: &mut String) {
    if s.len() > BODY_LIMIT {
        let mut cut = BODY_LIMIT;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
}

/// `n` deterministic pseudo-random bytes.
pub fn data(n: usize, seed: u8) -> Bytes {
    let mut v = Vec::with_capacity(n);
    let mut x: u32 = 2_166_136_261 ^ seed as u32;
    for _ in 0..n {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        v.push((x & 0xff) as u8);
    }
    Bytes::from(v)
}

pub fn etag_of(r: &S3Response) -> String {
    r.header("etag").unwrap_or_default().to_string()
}
