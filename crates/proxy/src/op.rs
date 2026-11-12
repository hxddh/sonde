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
    ("analytics", "BucketAnalyticsConfiguration"),
    ("metrics", "BucketMetricsConfiguration"),
    ("inventory", "BucketInventoryConfiguration"),
    (
        "intelligent-tiering",
        "BucketIntelligentTieringConfiguration",
    ),
];

fn has<K: AsRef<str>, V>(query: &[(K, V)], name: &str) -> bool {
    query.iter().any(|(k, _)| k.as_ref() == name)
}

/// The operation name, as the S3 API reference spells it.
pub fn classify<K: AsRef<str>, V>(
    method: &str,
    bucket: Option<&str>,
    key: Option<&str>,
    query: &[(K, V)],
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
                // a subresource not listed above is a bucket setting sonde
                // does not know, not the bucket's creation or removal
                ("PUT" | "DELETE", None) if query.iter().any(|(k, _)| k.as_ref() != "x-id") => {
                    "Unknown"
                }
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

/// Operations named without a subresource table.
const NAMED: &[&str] = &[
    "ListBuckets",
    "ListObjectsV2",
    "ListObjectVersions",
    "ListMultipartUploads",
    "GetBucketLocation",
    "ListObjects",
    "HeadBucket",
    "CreateBucket",
    "DeleteBucket",
    "DeleteObjects",
    "ListParts",
    "GetObjectAcl",
    "GetObjectTagging",
    "GetObjectAttributes",
    "GetObjectRetention",
    "GetObjectLegalHold",
    "GetObject",
    "HeadObject",
    "UploadPartCopy",
    "UploadPart",
    "PutObjectTagging",
    "PutObjectAcl",
    "PutObjectRetention",
    "PutObjectLegalHold",
    "CopyObject",
    "PutObject",
    "CreateMultipartUpload",
    "CompleteMultipartUpload",
    "RestoreObject",
    "SelectObjectContent",
    "AbortMultipartUpload",
    "DeleteObjectTagging",
    "DeleteObject",
];

/// Every name [`classify`] gives a request it can name: what a fault
/// scenario's `on` is checked against.
pub fn operation_names() -> Vec<String> {
    let mut all: Vec<String> = NAMED.iter().map(|n| n.to_string()).collect();
    for (_, n) in BUCKET_SUBRESOURCES {
        for verb in ["Get", "Put", "Delete"] {
            all.push(format!("{verb}{n}"));
        }
    }
    all
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
    fn every_name_a_request_is_given_is_a_name_a_scenario_may_say() {
        let all = operation_names();
        let none: &[(String, String)] = &[];
        let copy = [("x-amz-copy-source".to_string(), "/b/k".to_string())];
        let queries = [
            "",
            "list-type=2",
            "versions",
            "uploads",
            "location",
            "delete",
            "lifecycle",
            "versioning",
            "uploadId=u",
            "partNumber=1&uploadId=u",
            "acl",
            "tagging",
            "attributes",
            "retention",
            "legal-hold",
            "restore",
            "select",
        ];
        for method in ["GET", "HEAD", "PUT", "POST", "DELETE"] {
            for (bucket, key) in [(None, None), (Some("b"), None), (Some("b"), Some("k"))] {
                for query in queries {
                    let query: Vec<&str> = query.split('&').filter(|p| !p.is_empty()).collect();
                    for headers in [none, &copy[..]] {
                        let name = classify(method, bucket, key, &q(&query), headers);
                        assert!(
                            name == "Unknown" || all.contains(&name),
                            "{name} is given and not listed"
                        );
                    }
                }
            }
        }
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

    #[test]
    fn a_bucket_setting_is_never_named_the_buckets_creation() {
        let none: &[(String, String)] = &[];
        for (sub, name) in [
            ("inventory", "BucketInventoryConfiguration"),
            ("analytics", "BucketAnalyticsConfiguration"),
            ("metrics", "BucketMetricsConfiguration"),
            (
                "intelligent-tiering",
                "BucketIntelligentTieringConfiguration",
            ),
        ] {
            let query = q(&[sub, "id=x"]);
            let put = classify("PUT", Some("b"), None, &query, none);
            assert_eq!(put, format!("Put{name}"));
            assert!(crate::replay::changes_bucket(&put), "{put}");
            let delete = classify("DELETE", Some("b"), None, &query, none);
            assert!(crate::replay::changes_bucket(&delete), "{delete}");
        }
        // one sonde has never heard of
        let odd = q(&["unheard-of"]);
        assert_eq!(classify("PUT", Some("b"), None, &odd, none), "Unknown");
        assert_eq!(classify("DELETE", Some("b"), None, &odd, none), "Unknown");
        // the SDKs' own marker changes nothing
        let marked = q(&["x-id=CreateBucket"]);
        assert_eq!(
            classify("PUT", Some("b"), None, &marked, none),
            "CreateBucket"
        );
        assert_eq!(
            classify("DELETE", Some("b"), None, none, none),
            "DeleteBucket"
        );
    }
}
