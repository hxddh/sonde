//! The XML documents of the S3 API, as the services send them. Fields are
//! optional wherever some vendor leaves them out.

use serde::{Deserialize, Serialize};

fn de<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, crate::S3Error> {
    quick_xml::de::from_str(text).map_err(|e| crate::S3Error::decode(format!("XML: {e}")))
}

/// Escapes text for an XML element.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
    out
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ErrorBody {
    pub code: String,
    pub message: Option<String>,
    pub request_id: Option<String>,
    pub host_id: Option<String>,
    pub resource: Option<String>,
    pub key: Option<String>,
    pub bucket_name: Option<String>,
}

/// Parses an `<Error>` document (also inside STS's `<ErrorResponse>`).
pub fn parse_error(text: &str) -> Option<ErrorBody> {
    if let Ok(e) = quick_xml::de::from_str::<ErrorBody>(text) {
        return Some(e);
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Wrapped {
        error: ErrorBody,
        request_id: Option<String>,
    }
    quick_xml::de::from_str::<Wrapped>(text)
        .ok()
        .map(|w| ErrorBody {
            request_id: w.error.request_id.or(w.request_id),
            ..w.error
        })
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Owner {
    #[serde(rename = "ID")]
    pub id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Bucket {
    pub name: String,
    pub creation_date: Option<String>,
    pub bucket_region: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Buckets {
    #[serde(default)]
    pub bucket: Vec<Bucket>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ListAllMyBucketsResult {
    pub owner: Option<Owner>,
    #[serde(default)]
    pub buckets: Buckets,
    pub continuation_token: Option<String>,
}

pub fn list_buckets(text: &str) -> Result<ListAllMyBucketsResult, crate::S3Error> {
    de(text)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Object {
    pub key: String,
    pub last_modified: Option<String>,
    #[serde(rename = "ETag")]
    pub etag: Option<String>,
    #[serde(default)]
    pub size: u64,
    pub storage_class: Option<String>,
    pub owner: Option<Owner>,
    #[serde(default)]
    pub checksum_algorithm: Vec<String>,
    pub checksum_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CommonPrefix {
    pub prefix: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ListBucketResult {
    pub name: Option<String>,
    pub prefix: Option<String>,
    pub delimiter: Option<String>,
    pub max_keys: Option<u64>,
    pub key_count: Option<u64>,
    #[serde(default)]
    pub is_truncated: bool,
    pub continuation_token: Option<String>,
    pub next_continuation_token: Option<String>,
    pub start_after: Option<String>,
    pub encoding_type: Option<String>,
    #[serde(default)]
    pub contents: Vec<Object>,
    #[serde(default)]
    pub common_prefixes: Vec<CommonPrefix>,
}

fn url_decode(s: &str) -> String {
    // `encoding-type=url` encodes with `+` for spaces in some services
    percent_encoding::percent_decode_str(&s.replace('+', " "))
        .decode_utf8_lossy()
        .to_string()
}

pub fn list_objects_v2(text: &str) -> Result<ListBucketResult, crate::S3Error> {
    let mut r: ListBucketResult = de(text)?;
    if r.encoding_type.as_deref() == Some("url") {
        for o in &mut r.contents {
            o.key = url_decode(&o.key);
        }
        for p in &mut r.common_prefixes {
            p.prefix = url_decode(&p.prefix);
        }
        r.prefix = r.prefix.map(|p| url_decode(&p));
        r.delimiter = r.delimiter.map(|p| url_decode(&p));
        r.start_after = r.start_after.map(|p| url_decode(&p));
    }
    Ok(r)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Version {
    pub key: String,
    pub version_id: Option<String>,
    #[serde(default)]
    pub is_latest: bool,
    pub last_modified: Option<String>,
    #[serde(rename = "ETag")]
    pub etag: Option<String>,
    #[serde(default)]
    pub size: u64,
    pub storage_class: Option<String>,
    pub owner: Option<Owner>,
    #[serde(default)]
    pub checksum_algorithm: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteMarkerEntry {
    pub key: String,
    pub version_id: Option<String>,
    #[serde(default)]
    pub is_latest: bool,
    pub last_modified: Option<String>,
    pub owner: Option<Owner>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ListVersionsResult {
    pub name: Option<String>,
    pub prefix: Option<String>,
    #[serde(default)]
    pub is_truncated: bool,
    pub key_marker: Option<String>,
    pub version_id_marker: Option<String>,
    pub next_key_marker: Option<String>,
    pub next_version_id_marker: Option<String>,
    pub encoding_type: Option<String>,
    #[serde(default)]
    pub version: Vec<Version>,
    #[serde(default)]
    pub delete_marker: Vec<DeleteMarkerEntry>,
    #[serde(default)]
    pub common_prefixes: Vec<CommonPrefix>,
}

pub fn list_object_versions(text: &str) -> Result<ListVersionsResult, crate::S3Error> {
    let mut r: ListVersionsResult = de(text)?;
    if r.encoding_type.as_deref() == Some("url") {
        for v in &mut r.version {
            v.key = url_decode(&v.key);
        }
        for d in &mut r.delete_marker {
            d.key = url_decode(&d.key);
        }
        for p in &mut r.common_prefixes {
            p.prefix = url_decode(&p.prefix);
        }
        r.next_key_marker = r.next_key_marker.map(|p| url_decode(&p));
    }
    Ok(r)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Initiator {
    #[serde(rename = "ID")]
    pub id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Upload {
    pub key: String,
    pub upload_id: String,
    pub initiated: Option<String>,
    pub storage_class: Option<String>,
    pub initiator: Option<Initiator>,
    pub owner: Option<Owner>,
    pub checksum_algorithm: Option<String>,
    pub checksum_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ListMultipartUploadsResult {
    pub bucket: Option<String>,
    pub key_marker: Option<String>,
    pub upload_id_marker: Option<String>,
    pub next_key_marker: Option<String>,
    pub next_upload_id_marker: Option<String>,
    pub prefix: Option<String>,
    #[serde(default)]
    pub is_truncated: bool,
    pub encoding_type: Option<String>,
    #[serde(default)]
    pub upload: Vec<Upload>,
    #[serde(default)]
    pub common_prefixes: Vec<CommonPrefix>,
}

pub fn list_multipart_uploads(text: &str) -> Result<ListMultipartUploadsResult, crate::S3Error> {
    let mut r: ListMultipartUploadsResult = de(text)?;
    if r.encoding_type.as_deref() == Some("url") {
        for u in &mut r.upload {
            u.key = url_decode(&u.key);
        }
        r.next_key_marker = r.next_key_marker.map(|p| url_decode(&p));
    }
    Ok(r)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Part {
    pub part_number: u32,
    pub last_modified: Option<String>,
    #[serde(rename = "ETag")]
    pub etag: Option<String>,
    #[serde(default)]
    pub size: u64,
    #[serde(rename = "ChecksumCRC32")]
    pub checksum_crc32: Option<String>,
    #[serde(rename = "ChecksumCRC32C")]
    pub checksum_crc32c: Option<String>,
    #[serde(rename = "ChecksumCRC64NVME")]
    pub checksum_crc64nvme: Option<String>,
    #[serde(rename = "ChecksumSHA1")]
    pub checksum_sha1: Option<String>,
    #[serde(rename = "ChecksumSHA256")]
    pub checksum_sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ListPartsResult {
    pub bucket: Option<String>,
    pub key: Option<String>,
    pub upload_id: Option<String>,
    pub part_number_marker: Option<String>,
    pub next_part_number_marker: Option<String>,
    pub max_parts: Option<u32>,
    #[serde(default)]
    pub is_truncated: bool,
    pub storage_class: Option<String>,
    pub checksum_algorithm: Option<String>,
    pub checksum_type: Option<String>,
    #[serde(default)]
    pub part: Vec<Part>,
}

pub fn list_parts(text: &str) -> Result<ListPartsResult, crate::S3Error> {
    de(text)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct InitiateMultipartUploadResult {
    pub bucket: Option<String>,
    pub key: Option<String>,
    pub upload_id: String,
}

pub fn initiate_multipart_upload(
    text: &str,
) -> Result<InitiateMultipartUploadResult, crate::S3Error> {
    de(text)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CompleteMultipartUploadResult {
    pub location: Option<String>,
    pub bucket: Option<String>,
    pub key: Option<String>,
    #[serde(rename = "ETag")]
    pub etag: Option<String>,
    #[serde(rename = "ChecksumCRC32")]
    pub checksum_crc32: Option<String>,
    #[serde(rename = "ChecksumCRC32C")]
    pub checksum_crc32c: Option<String>,
    #[serde(rename = "ChecksumCRC64NVME")]
    pub checksum_crc64nvme: Option<String>,
    #[serde(rename = "ChecksumSHA1")]
    pub checksum_sha1: Option<String>,
    #[serde(rename = "ChecksumSHA256")]
    pub checksum_sha256: Option<String>,
    pub checksum_type: Option<String>,
}

pub fn complete_multipart_upload(
    text: &str,
) -> Result<CompleteMultipartUploadResult, crate::S3Error> {
    de(text)
}

/// A part to complete: number, ETag, and an optional checksum (element, value).
pub type CompletedPart = (u32, String, Option<(String, String)>);

/// The `<CompleteMultipartUpload>` body for `parts` (number, ETag, optional checksum name/value).
pub fn complete_body(parts: &[CompletedPart]) -> String {
    let mut s =
        String::from("<CompleteMultipartUpload xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">");
    for (n, etag, checksum) in parts {
        s.push_str(&format!(
            "<Part><PartNumber>{n}</PartNumber><ETag>{}</ETag>",
            escape(etag)
        ));
        if let Some((name, value)) = checksum {
            s.push_str(&format!("<{name}>{}</{name}>", escape(value)));
        }
        s.push_str("</Part>");
    }
    s.push_str("</CompleteMultipartUpload>");
    s
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CopyResult {
    #[serde(rename = "ETag")]
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

pub fn copy_result(text: &str) -> Result<CopyResult, crate::S3Error> {
    de(text)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct VersioningConfiguration {
    pub status: Option<String>,
    #[serde(rename = "MfaDelete", alias = "MFADelete")]
    pub mfa_delete: Option<String>,
}

pub fn versioning(text: &str) -> Result<VersioningConfiguration, crate::S3Error> {
    if !text.contains('<')
        || text.contains("<VersioningConfiguration/>")
        || !text.contains("</VersioningConfiguration>")
    {
        return Ok(VersioningConfiguration::default());
    }
    de(text)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct AbortIncomplete {
    pub days_after_initiation: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct RuleFilter {
    pub prefix: Option<String>,
    pub object_size_greater_than: Option<u64>,
    pub and: Option<RuleAnd>,
    pub tag: Option<Tag>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct RuleAnd {
    pub prefix: Option<String>,
    #[serde(default)]
    pub tag: Vec<Tag>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Tag {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Expiration {
    pub days: Option<u32>,
    pub date: Option<String>,
    pub expired_object_delete_marker: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct NoncurrentExpiration {
    pub noncurrent_days: Option<u32>,
    pub newer_noncurrent_versions: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct LifecycleRule {
    #[serde(rename = "ID")]
    pub id: Option<String>,
    pub status: Option<String>,
    pub prefix: Option<String>,
    pub filter: Option<RuleFilter>,
    pub abort_incomplete_multipart_upload: Option<AbortIncomplete>,
    pub expiration: Option<Expiration>,
    pub noncurrent_version_expiration: Option<NoncurrentExpiration>,
}

impl LifecycleRule {
    pub fn enabled(&self) -> bool {
        self.status.as_deref() == Some("Enabled")
    }
    /// The prefix the rule applies to ("" for the whole bucket); `None`
    /// when it filters by tags or size too (it may not cover every upload).
    pub fn scope_prefix(&self) -> Option<String> {
        if let Some(p) = &self.prefix {
            return Some(p.clone());
        }
        match &self.filter {
            None => Some(String::new()),
            Some(f)
                if f.tag.is_none() && f.and.is_none() && f.object_size_greater_than.is_none() =>
            {
                Some(f.prefix.clone().unwrap_or_default())
            }
            Some(_) => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct LifecycleConfiguration {
    #[serde(default)]
    pub rule: Vec<LifecycleRule>,
}

pub fn lifecycle(text: &str) -> Result<LifecycleConfiguration, crate::S3Error> {
    de(text)
}

/// A rule aborting incomplete uploads after `days` under `prefix`.
pub fn abort_rule_xml(id: &str, prefix: &str, days: u32) -> String {
    format!(
        "<Rule><ID>{}</ID><Filter><Prefix>{}</Prefix></Filter><Status>Enabled</Status><AbortIncompleteMultipartUpload><DaysAfterInitiation>{days}</DaysAfterInitiation></AbortIncompleteMultipartUpload></Rule>",
        escape(id),
        escape(prefix)
    )
}

/// Adds `rule_xml` to an existing lifecycle document (kept verbatim), or
/// makes a new one.
pub fn add_rule(existing: Option<&str>, rule_xml: &str) -> String {
    match existing.and_then(|e| e.rfind("</LifecycleConfiguration>").map(|i| (e, i))) {
        Some((e, i)) => format!("{}{rule_xml}{}", &e[..i], &e[i..]),
        None => format!(
            "<LifecycleConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">{rule_xml}</LifecycleConfiguration>"
        ),
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Deleted {
    pub key: String,
    pub version_id: Option<String>,
    pub delete_marker: Option<bool>,
    pub delete_marker_version_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteError {
    pub key: Option<String>,
    pub version_id: Option<String>,
    pub code: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteResult {
    #[serde(default)]
    pub deleted: Vec<Deleted>,
    #[serde(default)]
    pub error: Vec<DeleteError>,
}

pub fn delete_result(text: &str) -> Result<DeleteResult, crate::S3Error> {
    de(text)
}

/// The `<Delete>` body: (key, version).
pub fn delete_body(objects: &[(String, Option<String>)], quiet: bool) -> String {
    let mut s = String::from("<Delete xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">");
    if quiet {
        s.push_str("<Quiet>true</Quiet>");
    }
    for (k, v) in objects {
        s.push_str(&format!("<Object><Key>{}</Key>", escape(k)));
        if let Some(v) = v {
            s.push_str(&format!("<VersionId>{}</VersionId>", escape(v)));
        }
        s.push_str("</Object>");
    }
    s.push_str("</Delete>");
    s
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CallerIdentity {
    pub arn: Option<String>,
    pub user_id: Option<String>,
    pub account: Option<String>,
}

pub fn caller_identity(text: &str) -> Result<CallerIdentity, crate::S3Error> {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct R {
        get_caller_identity_result: CallerIdentity,
    }
    de::<R>(text).map(|r| r.get_caller_identity_result)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub expiration: Option<String>,
}

/// The credentials of `AssumeRole` / `AssumeRoleWithWebIdentity`.
pub fn sts_credentials(text: &str) -> Result<StsCredentials, crate::S3Error> {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Result_ {
        credentials: StsCredentials,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct R {
        #[serde(alias = "AssumeRoleWithWebIdentityResult", alias = "AssumeRoleResult")]
        result: Result_,
    }
    de::<R>(text).map(|r| r.result.credentials)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct DefaultRetention {
    pub mode: Option<String>,
    pub days: Option<u32>,
    pub years: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct LockRule {
    pub default_retention: Option<DefaultRetention>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct ObjectLockConfiguration {
    pub object_lock_enabled: Option<String>,
    pub rule: Option<LockRule>,
}

pub fn object_lock(text: &str) -> Result<ObjectLockConfiguration, crate::S3Error> {
    de(text)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Retention {
    pub mode: Option<String>,
    pub retain_until_date: Option<String>,
}

pub fn retention(text: &str) -> Result<Retention, crate::S3Error> {
    de(text)
}

/// `<LocationConstraint>` text ("" for us-east-1).
pub fn location(text: &str) -> String {
    let start = text
        .find("<LocationConstraint")
        .and_then(|i| text[i..].find('>').map(|j| i + j + 1));
    match start {
        Some(s) => match text[s..].find("</LocationConstraint>") {
            Some(e) => text[s..s + e].to_string(),
            None => String::new(),
        },
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_v2() {
        let x = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>b</Name><Prefix>a%2F</Prefix><KeyCount>2</KeyCount><MaxKeys>1000</MaxKeys><Delimiter>%2F</Delimiter><EncodingType>url</EncodingType><IsTruncated>true</IsTruncated><NextContinuationToken>tok==</NextContinuationToken>
<Contents><Key>a%2Fhello+world.txt</Key><LastModified>2024-01-01T00:00:00.000Z</LastModified><ETag>&quot;9b2cf535f27731c974343645a3985328-3&quot;</ETag><ChecksumAlgorithm>CRC32</ChecksumAlgorithm><ChecksumType>COMPOSITE</ChecksumType><Size>15728640</Size><StorageClass>STANDARD</StorageClass></Contents>
<CommonPrefixes><Prefix>a%2Fsub%2F</Prefix></CommonPrefixes></ListBucketResult>"#;
        let r = list_objects_v2(x).unwrap();
        assert!(r.is_truncated);
        assert_eq!(r.next_continuation_token.as_deref(), Some("tok=="));
        assert_eq!(r.contents[0].key, "a/hello world.txt");
        assert_eq!(
            r.contents[0].etag.as_deref(),
            Some("\"9b2cf535f27731c974343645a3985328-3\"")
        );
        assert_eq!(r.contents[0].checksum_algorithm, vec!["CRC32"]);
        assert_eq!(r.common_prefixes[0].prefix, "a/sub/");
        assert_eq!(r.prefix.as_deref(), Some("a/"));
    }

    #[test]
    fn versions_interleaved() {
        let x = r#"<ListVersionsResult><Name>b</Name><IsTruncated>false</IsTruncated>
<Version><Key>k</Key><VersionId>v3</VersionId><IsLatest>false</IsLatest><LastModified>2024-01-03T00:00:00Z</LastModified><ETag>"e"</ETag><Size>3</Size></Version>
<DeleteMarker><Key>k</Key><VersionId>v4</VersionId><IsLatest>true</IsLatest><LastModified>2024-01-04T00:00:00Z</LastModified></DeleteMarker>
<Version><Key>k</Key><VersionId>v1</VersionId><IsLatest>false</IsLatest><LastModified>2024-01-01T00:00:00Z</LastModified><Size>1</Size></Version>
</ListVersionsResult>"#;
        let r = list_object_versions(x).unwrap();
        assert_eq!(r.version.len(), 2);
        assert_eq!(r.delete_marker.len(), 1);
        assert!(r.delete_marker[0].is_latest);
    }

    #[test]
    fn uploads_parts() {
        let u = list_multipart_uploads("<ListMultipartUploadsResult><Bucket>b</Bucket><IsTruncated>false</IsTruncated><Upload><Key>big</Key><UploadId>U1</UploadId><Initiated>2024-01-01T00:00:00.000Z</Initiated><StorageClass>STANDARD</StorageClass></Upload></ListMultipartUploadsResult>").unwrap();
        assert_eq!(u.upload[0].upload_id, "U1");
        let p = list_parts("<ListPartsResult><IsTruncated>true</IsTruncated><NextPartNumberMarker>2</NextPartNumberMarker><Part><PartNumber>1</PartNumber><ETag>\"a\"</ETag><Size>5242880</Size><ChecksumCRC32>AAAAAA==</ChecksumCRC32></Part><Part><PartNumber>2</PartNumber><Size>10</Size></Part></ListPartsResult>").unwrap();
        assert_eq!(p.part.iter().map(|p| p.size).sum::<u64>(), 5242890);
        assert_eq!(p.part[0].checksum_crc32.as_deref(), Some("AAAAAA=="));
    }

    #[test]
    fn errors() {
        let e = parse_error("<ErrorResponse><Error><Type>Sender</Type><Code>InvalidClientTokenId</Code><Message>bad</Message></Error><RequestId>abc</RequestId></ErrorResponse>").unwrap();
        assert_eq!(e.code, "InvalidClientTokenId");
        assert_eq!(e.request_id.as_deref(), Some("abc"));
        assert!(parse_error("not xml").is_none());
    }

    #[test]
    fn lifecycle_rules() {
        let x = "<LifecycleConfiguration><Rule><ID>a</ID><Filter><Prefix></Prefix></Filter><Status>Enabled</Status><AbortIncompleteMultipartUpload><DaysAfterInitiation>7</DaysAfterInitiation></AbortIncompleteMultipartUpload></Rule><Rule><ID>b</ID><Filter><And><Prefix>x</Prefix><Tag><Key>t</Key><Value>v</Value></Tag></And></Filter><Status>Disabled</Status></Rule></LifecycleConfiguration>";
        let l = lifecycle(x).unwrap();
        assert_eq!(l.rule.len(), 2);
        assert_eq!(
            l.rule[0]
                .abort_incomplete_multipart_upload
                .as_ref()
                .unwrap()
                .days_after_initiation,
            Some(7)
        );
        assert_eq!(l.rule[0].scope_prefix().as_deref(), Some(""));
        assert!(l.rule[1].scope_prefix().is_none());
        let added = add_rule(Some(x), &abort_rule_xml("sonde", "", 7));
        assert_eq!(lifecycle(&added).unwrap().rule.len(), 3);
        assert!(add_rule(None, "<Rule/>").starts_with("<LifecycleConfiguration"));
    }

    #[test]
    fn versioning_states() {
        assert_eq!(
            versioning("<VersioningConfiguration xmlns=\"x\"/>")
                .unwrap()
                .status,
            None
        );
        let v = versioning("<VersioningConfiguration><Status>Suspended</Status><MfaDelete>Disabled</MfaDelete></VersioningConfiguration>").unwrap();
        assert_eq!(v.status.as_deref(), Some("Suspended"));
        assert_eq!(v.mfa_delete.as_deref(), Some("Disabled"));
    }

    #[test]
    fn sts() {
        let c = sts_credentials("<AssumeRoleWithWebIdentityResponse><AssumeRoleWithWebIdentityResult><Credentials><AccessKeyId>ASIA</AccessKeyId><SecretAccessKey>s</SecretAccessKey><SessionToken>t</SessionToken><Expiration>2030-01-01T00:00:00Z</Expiration></Credentials></AssumeRoleWithWebIdentityResult></AssumeRoleWithWebIdentityResponse>").unwrap();
        assert_eq!(c.access_key_id, "ASIA");
        let i = caller_identity("<GetCallerIdentityResponse><GetCallerIdentityResult><Arn>arn:aws:iam::1:user/a</Arn><UserId>U</UserId><Account>1</Account></GetCallerIdentityResult></GetCallerIdentityResponse>").unwrap();
        assert_eq!(i.account.as_deref(), Some("1"));
    }

    #[test]
    fn misc() {
        assert_eq!(
            location("<LocationConstraint xmlns=\"x\">eu-west-1</LocationConstraint>"),
            "eu-west-1"
        );
        assert_eq!(location("<LocationConstraint xmlns=\"x\"/>"), "");
        assert!(
            delete_body(&[("a&b".into(), Some("v".into()))], true)
                .contains("<Key>a&amp;b</Key><VersionId>v</VersionId>")
        );
        let d = delete_result("<DeleteResult><Deleted><Key>a</Key></Deleted><Error><Key>b</Key><Code>AccessDenied</Code></Error></DeleteResult>").unwrap();
        assert_eq!((d.deleted.len(), d.error.len()), (1, 1));
    }
}
