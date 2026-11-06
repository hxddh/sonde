//! Typed S3 operations on top of [`S3Client::send`].

use base64::Engine as _;
use bytes::Bytes;
use http::Method;
use md5::Digest as _;
use serde::{Deserialize, Serialize};

use crate::client::{S3Client, S3Request, S3Response};
use crate::error::S3Error;
use crate::xml::{self, CompletedPart};

/// One page of `ListObjectsV2`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListV2 {
    pub prefix: Option<String>,
    pub delimiter: Option<String>,
    pub continuation_token: Option<String>,
    pub start_after: Option<String>,
    pub max_keys: Option<u32>,
    /// Ask for `encoding-type=url` (keys with control characters survive).
    pub url_encoding: bool,
}

/// HEAD's answer: every header, and the user metadata apart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadResult {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub metadata: Vec<(String, String)>,
    pub size: Option<u64>,
    pub etag: Option<String>,
    pub record_id: u64,
}

fn md5_b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(md5::Md5::digest(data))
}

impl S3Client {
    pub async fn list_buckets(&self) -> Result<xml::ListAllMyBucketsResult, S3Error> {
        let r = self
            .send(S3Request::new("ListBuckets", Method::GET))
            .await?;
        xml::list_buckets(&r.text())
    }

    pub async fn head_bucket(&self, bucket: &str) -> Result<S3Response, S3Error> {
        self.send(S3Request::new("HeadBucket", Method::HEAD).bucket(bucket))
            .await
    }

    pub async fn bucket_location(&self, bucket: &str) -> Result<String, S3Error> {
        let r = self
            .send(
                S3Request::new("GetBucketLocation", Method::GET)
                    .bucket(bucket)
                    .query("location", ""),
            )
            .await?;
        Ok(xml::location(&r.text()))
    }

    pub async fn list_objects_v2(
        &self,
        bucket: &str,
        p: &ListV2,
    ) -> Result<(xml::ListBucketResult, S3Response), S3Error> {
        let mut req = S3Request::new("ListObjectsV2", Method::GET)
            .bucket(bucket)
            .query("list-type", "2");
        req = req
            .query_opt("prefix", p.prefix.clone().filter(|s| !s.is_empty()))
            .query_opt("delimiter", p.delimiter.clone().filter(|s| !s.is_empty()))
            .query_opt("continuation-token", p.continuation_token.clone())
            .query_opt("start-after", p.start_after.clone())
            .query_opt("max-keys", p.max_keys.map(|m| m.to_string()));
        if p.url_encoding {
            req = req.query("encoding-type", "url");
        }
        let r = self.send(req).await?;
        Ok((xml::list_objects_v2(&r.text())?, r))
    }

    pub async fn head_object(
        &self,
        bucket: &str,
        key: &str,
        version: Option<&str>,
    ) -> Result<HeadResult, S3Error> {
        let req = S3Request::new("HeadObject", Method::HEAD)
            .bucket(bucket)
            .key(key)
            .query_opt("versionId", version.map(String::from))
            .header("x-amz-checksum-mode", "ENABLED");
        let r = self.send(req).await?;
        let metadata = r
            .headers
            .iter()
            .filter_map(|(k, v)| {
                k.to_ascii_lowercase()
                    .strip_prefix("x-amz-meta-")
                    .map(|m| (m.to_string(), v.clone()))
            })
            .collect();
        Ok(HeadResult {
            status: r.status,
            size: r.header("content-length").and_then(|v| v.parse().ok()),
            etag: r.header("etag").map(String::from),
            metadata,
            headers: r.headers.clone(),
            record_id: r.record_id,
        })
    }

    pub async fn get_object(
        &self,
        bucket: &str,
        key: &str,
        range: Option<&str>,
    ) -> Result<S3Response, S3Error> {
        self.send(
            S3Request::new("GetObject", Method::GET)
                .bucket(bucket)
                .key(key)
                .header_opt("range", range.map(String::from)),
        )
        .await
    }

    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        body: Bytes,
        headers: &[(&str, &str)],
    ) -> Result<S3Response, S3Error> {
        let mut req = S3Request::new("PutObject", Method::PUT)
            .bucket(bucket)
            .key(key)
            .body(body);
        for (k, v) in headers {
            req = req.header(k, *v);
        }
        self.send(req).await
    }

    pub async fn delete_object(
        &self,
        bucket: &str,
        key: &str,
        version: Option<&str>,
    ) -> Result<S3Response, S3Error> {
        self.send(
            S3Request::new("DeleteObject", Method::DELETE)
                .bucket(bucket)
                .key(key)
                .query_opt("versionId", version.map(String::from)),
        )
        .await
    }

    pub async fn delete_objects(
        &self,
        bucket: &str,
        objects: &[(String, Option<String>)],
        quiet: bool,
    ) -> Result<xml::DeleteResult, S3Error> {
        let body = xml::delete_body(objects, quiet);
        let req = S3Request::new("DeleteObjects", Method::POST)
            .bucket(bucket)
            .query("delete", "")
            .header("content-md5", md5_b64(body.as_bytes()))
            .header("content-type", "application/xml")
            .body(body);
        let r = self.send(req).await?;
        xml::delete_result(&r.text())
    }

    pub async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        key_marker: Option<&str>,
        version_marker: Option<&str>,
        delimiter: Option<&str>,
    ) -> Result<xml::ListVersionsResult, S3Error> {
        let req = S3Request::new("ListObjectVersions", Method::GET)
            .bucket(bucket)
            .query("versions", "")
            .query("encoding-type", "url")
            .query_opt("prefix", prefix.filter(|s| !s.is_empty()).map(String::from))
            .query_opt("key-marker", key_marker.map(String::from))
            .query_opt("version-id-marker", version_marker.map(String::from))
            .query_opt("delimiter", delimiter.map(String::from));
        let r = self.send(req).await?;
        xml::list_object_versions(&r.text())
    }

    pub async fn bucket_versioning(
        &self,
        bucket: &str,
    ) -> Result<xml::VersioningConfiguration, S3Error> {
        let r = self
            .send(
                S3Request::new("GetBucketVersioning", Method::GET)
                    .bucket(bucket)
                    .query("versioning", ""),
            )
            .await?;
        xml::versioning(&r.text())
    }

    pub async fn put_bucket_versioning(
        &self,
        bucket: &str,
        status: &str,
    ) -> Result<S3Response, S3Error> {
        let body = format!(
            "<VersioningConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Status>{status}</Status></VersioningConfiguration>"
        );
        self.send(
            S3Request::new("PutBucketVersioning", Method::PUT)
                .bucket(bucket)
                .query("versioning", "")
                .header("content-md5", md5_b64(body.as_bytes()))
                .body(body),
        )
        .await
    }

    /// The lifecycle document, `None` when the bucket has none.
    pub async fn lifecycle_raw(&self, bucket: &str) -> Result<Option<String>, S3Error> {
        match self
            .send(
                S3Request::new("GetBucketLifecycleConfiguration", Method::GET)
                    .bucket(bucket)
                    .query("lifecycle", ""),
            )
            .await
        {
            Ok(r) => Ok(Some(r.text())),
            Err(e)
                if e.code.as_deref() == Some("NoSuchLifecycleConfiguration")
                    || e.status == Some(404) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    pub async fn put_lifecycle(&self, bucket: &str, document: &str) -> Result<S3Response, S3Error> {
        let req = S3Request::new("PutBucketLifecycleConfiguration", Method::PUT)
            .bucket(bucket)
            .query("lifecycle", "")
            .header("content-md5", md5_b64(document.as_bytes()))
            .header("content-type", "application/xml")
            .body(document.to_string());
        self.send(req).await
    }

    pub async fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        key_marker: Option<&str>,
        upload_marker: Option<&str>,
    ) -> Result<xml::ListMultipartUploadsResult, S3Error> {
        let req = S3Request::new("ListMultipartUploads", Method::GET)
            .bucket(bucket)
            .query("uploads", "")
            .query("encoding-type", "url")
            .query_opt("prefix", prefix.filter(|s| !s.is_empty()).map(String::from))
            .query_opt("key-marker", key_marker.map(String::from))
            .query_opt("upload-id-marker", upload_marker.map(String::from));
        let r = self.send(req).await?;
        xml::list_multipart_uploads(&r.text())
    }

    pub async fn list_parts(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        marker: Option<&str>,
    ) -> Result<xml::ListPartsResult, S3Error> {
        let req = S3Request::new("ListParts", Method::GET)
            .bucket(bucket)
            .key(key)
            .query("uploadId", upload_id)
            .query_opt("part-number-marker", marker.map(String::from));
        let r = self.send(req).await?;
        xml::list_parts(&r.text())
    }

    pub async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        headers: &[(&str, &str)],
    ) -> Result<String, S3Error> {
        let mut req = S3Request::new("CreateMultipartUpload", Method::POST)
            .bucket(bucket)
            .key(key)
            .query("uploads", "");
        for (k, v) in headers {
            req = req.header(k, *v);
        }
        let r = self.send(req).await?;
        Ok(xml::initiate_multipart_upload(&r.text())?.upload_id)
    }

    pub async fn upload_part(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        n: u32,
        body: Bytes,
        headers: &[(&str, &str)],
    ) -> Result<S3Response, S3Error> {
        let mut req = S3Request::new("UploadPart", Method::PUT)
            .bucket(bucket)
            .key(key)
            .query("partNumber", n.to_string())
            .query("uploadId", upload_id)
            .body(body);
        for (k, v) in headers {
            req = req.header(k, *v);
        }
        self.send(req).await
    }

    pub async fn upload_part_copy(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        n: u32,
        source: &str,
        range: Option<&str>,
    ) -> Result<S3Response, S3Error> {
        let req = S3Request::new("UploadPartCopy", Method::PUT)
            .bucket(bucket)
            .key(key)
            .query("partNumber", n.to_string())
            .query("uploadId", upload_id)
            .header("x-amz-copy-source", source)
            .header_opt("x-amz-copy-source-range", range.map(String::from));
        self.send(req).await
    }

    /// Completes with `parts` (number, ETag, optional checksum element/value); raw response.
    pub async fn complete_multipart_upload_raw(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[CompletedPart],
        headers: &[(&str, &str)],
    ) -> Result<S3Response, S3Error> {
        let mut req = S3Request::new("CompleteMultipartUpload", Method::POST)
            .bucket(bucket)
            .key(key)
            .query("uploadId", upload_id)
            .body(xml::complete_body(parts))
            .no_retry();
        for (k, v) in headers {
            req = req.header(k, *v);
        }
        self.send_raw(req).await
    }

    pub async fn abort_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> Result<S3Response, S3Error> {
        self.send(
            S3Request::new("AbortMultipartUpload", Method::DELETE)
                .bucket(bucket)
                .key(key)
                .query("uploadId", upload_id),
        )
        .await
    }

    pub async fn copy_object(
        &self,
        bucket: &str,
        key: &str,
        source: &str,
        headers: &[(&str, &str)],
    ) -> Result<S3Response, S3Error> {
        let mut req = S3Request::new("CopyObject", Method::PUT)
            .bucket(bucket)
            .key(key)
            .header("x-amz-copy-source", source);
        for (k, v) in headers {
            req = req.header(k, *v);
        }
        self.send(req).await
    }

    pub async fn create_bucket(
        &self,
        bucket: &str,
        object_lock: bool,
    ) -> Result<S3Response, S3Error> {
        let region = self.config().region.clone();
        let mut req = S3Request::new("CreateBucket", Method::PUT).bucket(bucket);
        if !region.is_empty() && region != "us-east-1" {
            req = req.body(format!("<CreateBucketConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><LocationConstraint>{region}</LocationConstraint></CreateBucketConfiguration>"));
        }
        if object_lock {
            req = req.header("x-amz-bucket-object-lock-enabled", "true");
        }
        self.send(req).await
    }

    pub async fn delete_bucket(&self, bucket: &str) -> Result<S3Response, S3Error> {
        self.send(S3Request::new("DeleteBucket", Method::DELETE).bucket(bucket))
            .await
    }

    pub async fn object_lock_configuration(
        &self,
        bucket: &str,
    ) -> Result<Option<xml::ObjectLockConfiguration>, S3Error> {
        match self
            .send(
                S3Request::new("GetObjectLockConfiguration", Method::GET)
                    .bucket(bucket)
                    .query("object-lock", ""),
            )
            .await
        {
            Ok(r) => Ok(Some(xml::object_lock(&r.text())?)),
            Err(e) if e.status == Some(404) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// STS GetCallerIdentity against `sts_endpoint`.
    pub async fn caller_identity(
        &self,
        sts_endpoint: &str,
    ) -> Result<xml::CallerIdentity, S3Error> {
        let url = url::Url::parse(sts_endpoint)
            .map_err(|e| S3Error::config(format!("STS endpoint: {e}")))?;
        let body = "Action=GetCallerIdentity&Version=2011-06-15";
        let mut req = S3Request::new("GetCallerIdentity", Method::POST)
            .header(
                "content-type",
                "application/x-www-form-urlencoded; charset=utf-8",
            )
            .body(body);
        req.service = "sts".into();
        req.endpoint = Some(url);
        let r = self.send(req).await?;
        xml::caller_identity(&r.text())
    }

    /// Every key under `prefix`, following continuation tokens.
    pub async fn list_all(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<xml::Object>, S3Error> {
        let mut out = Vec::new();
        let mut token = None;
        loop {
            let (page, _) = self
                .list_objects_v2(
                    bucket,
                    &ListV2 {
                        prefix: Some(prefix.into()),
                        continuation_token: token.clone(),
                        url_encoding: true,
                        ..Default::default()
                    },
                )
                .await?;
            out.extend(page.contents);
            if out.len() >= limit || !page.is_truncated || page.next_continuation_token.is_none() {
                break;
            }
            token = page.next_continuation_token;
        }
        out.truncate(limit);
        Ok(out)
    }
}
