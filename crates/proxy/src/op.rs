//! Names the S3 operation of a request from its method, path and query.

/// Bucket subresources that name an operation of their own.
const BUCKET_SUBRESOURCES: &[(&str, &str)] = &[
    ("versioning", "BucketVersioning"),
    ("lifecycle", "BucketLifecycleConfiguration"),
    ("policy", "BucketPolicy"),
    ("acl", "BucketAcl"),
    ("cors", "BucketCors"),
    ("tagging", "BucketTagging"),
    ("encryption", "BucketEncryption"),
    ("object-lock", "ObjectLockConfiguration"),
    ("notification", "BucketNotificationConfiguration"),
    ("replication", "BucketReplication"),
    ("website", "BucketWebsite"),
    ("logging", "BucketLogging"),
    ("ownershipControls", "BucketOwnershipControls"),
    ("publicAccessBlock", "PublicAccessBlock"),
    ("accelerate", "BucketAccelerateConfiguration"),
    ("requestPayment", "BucketRequestPayment"),
];

fn has(query: &[(String, String)], name: &str) -> bool {
    query.iter().any(|(k, _)| k == name)
}

/// The operation name, as the S3 API reference spells it.
pub fn classify(
    method: &str,
    bucket: Option<&str>,
    key: Option<&str>,
    query: &[(String, String)],
    headers: &[(String, String)],
) -> String {
    let q = |n: &str| has(query, n);
    let copy = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("x-amz-copy-source"));
    let name = match (bucket, key) {
        (None, _) => match method {
            "GET" => "ListBuckets",
            _ => "Unknown",
        },
        (Some(_), None) => {
            let sub = BUCKET_SUBRESOURCES.iter().find(|(k, _)| q(k));
            match (method, sub) {
                ("GET", Some((_, n))) => return format!("Get{n}"),
                ("PUT", Some((_, n))) => return format!("Put{n}"),
                ("DELETE", Some((_, n))) => return format!("Delete{n}"),
                ("GET", None) if q("list-type") => "ListObjectsV2",
                ("GET", None) if q("versions") => "ListObjectVersions",
                ("GET", None) if q("uploads") => "ListMultipartUploads",
                ("GET", None) if q("location") => "GetBucketLocation",
                ("GET", None) => "ListObjects",
                ("HEAD", _) => "HeadBucket",
                ("PUT", None) => "CreateBucket",
                ("DELETE", None) => "DeleteBucket",
                ("POST", _) if q("delete") => "DeleteObjects",
                _ => "Unknown",
            }
        }
        (Some(_), Some(_)) => match method {
            "GET" if q("uploadId") => "ListParts",
            "GET" if q("acl") => "GetObjectAcl",
            "GET" if q("tagging") => "GetObjectTagging",
            "GET" if q("attributes") => "GetObjectAttributes",
            "GET" if q("retention") => "GetObjectRetention",
            "GET" if q("legal-hold") => "GetObjectLegalHold",
            "GET" => "GetObject",
            "HEAD" => "HeadObject",
            "PUT" if q("partNumber") && q("uploadId") && copy => "UploadPartCopy",
            "PUT" if q("partNumber") && q("uploadId") => "UploadPart",
            "PUT" if q("tagging") => "PutObjectTagging",
            "PUT" if q("acl") => "PutObjectAcl",
            "PUT" if q("retention") => "PutObjectRetention",
            "PUT" if q("legal-hold") => "PutObjectLegalHold",
            "PUT" if copy => "CopyObject",
            "PUT" => "PutObject",
            "POST" if q("uploads") => "CreateMultipartUpload",
            "POST" if q("uploadId") => "CompleteMultipartUpload",
            "POST" if q("restore") => "RestoreObject",
            "POST" if q("select") => "SelectObjectContent",
            "DELETE" if q("uploadId") => "AbortMultipartUpload",
            "DELETE" if q("tagging") => "DeleteObjectTagging",
            "DELETE" => "DeleteObject",
            _ => "Unknown",
        },
    };
    name.to_string()
}

/// Whether an operation removes data (directly, or by a rule that will).
pub fn is_destructive(op: &str) -> bool {
    op.starts_with("Delete")
        || op == "AbortMultipartUpload"
        || op == "PutBucketLifecycleConfiguration"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pairs: &[&str]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|p| match p.split_once('=') {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => (p.to_string(), String::new()),
            })
            .collect()
    }

    #[test]
    fn names() {
        let none: &[(String, String)] = &[];
        assert_eq!(classify("GET", None, None, none, none), "ListBuckets");
        assert_eq!(
            classify(
                "GET",
                Some("b"),
                None,
                &q(&["list-type=2", "prefix=a"]),
                none
            ),
            "ListObjectsV2"
        );
        assert_eq!(
            classify("PUT", Some("b"), None, &q(&["lifecycle"]), none),
            "PutBucketLifecycleConfiguration"
        );
        assert_eq!(
            classify(
                "PUT",
                Some("b"),
                Some("k"),
                &q(&["partNumber=1", "uploadId=u"]),
                none
            ),
            "UploadPart"
        );
        let copy = vec![("X-Amz-Copy-Source".to_string(), "/b/a".to_string())];
        assert_eq!(
            classify("PUT", Some("b"), Some("k"), none, &copy),
            "CopyObject"
        );
        assert_eq!(
            classify("POST", Some("b"), Some("k"), &q(&["uploads"]), none),
            "CreateMultipartUpload"
        );
        assert_eq!(
            classify("DELETE", Some("b"), Some("k"), &q(&["uploadId=u"]), none),
            "AbortMultipartUpload"
        );
        assert_eq!(
            classify("POST", Some("b"), None, &q(&["delete"]), none),
            "DeleteObjects"
        );
        assert!(is_destructive("DeleteObjects"));
        assert!(is_destructive("PutBucketLifecycleConfiguration"));
        assert!(!is_destructive("PutObject"));
    }
}
