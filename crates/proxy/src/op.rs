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

/// Object subresources that name an operation sonde knows.
const OBJECT_SUBRESOURCES: &[&str] = &[
    "uploadId",
    "uploads",
    "acl",
    "tagging",
    "attributes",
    "retention",
    "legal-hold",
    "restore",
    "select",
    "renameObject",
    "encryption",
];

/// Whether an object request's query holds only what an operation sonde
/// knows carries: its subresource and parameters, a presigned URL's
/// signature, the SDKs' `x-id` marker.
fn plain_object<K: AsRef<str>, V>(query: &[(K, V)]) -> bool {
    query.iter().all(|(k, _)| {
        let k = k.as_ref();
        OBJECT_SUBRESOURCES.contains(&k)
            || matches!(
                k,
                "versionId"
                    | "partNumber"
                    | "max-parts"
                    | "part-number-marker"
                    | "select-type"
                    | "x-id"
                    | "AWSAccessKeyId"
                    | "Signature"
                    | "Expires"
            )
            || k.starts_with("response-")
            || k.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("x-amz-"))
    })
}

fn has<K: AsRef<str>, V>(query: &[(K, V)], name: &str) -> bool {
    query.iter().any(|(k, _)| k.as_ref() == name)
}

/// The operation name, as the S3 API reference spells it.
pub fn classify<K: AsRef<str>, V: AsRef<str>>(
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
                // S3 Express One Zone's session credentials, for a directory bucket
                ("GET", None) if q("session") => "CreateSession",
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
        // a subresource sonde knows names the request, whatever else is in
        // the query (`?renameObject&zzz&x-id=PutObject` is a rename)
        (Some(_), Some(_))
            if !plain_object(query)
                && !query
                    .iter()
                    .any(|(k, _)| OBJECT_SUBRESOURCES.contains(&k.as_ref())) =>
        {
            // a subresource sonde does not know: the SDKs' own marker, when
            // it names an operation sonde does, or none (never the plain
            // read, write or delete it would otherwise look like)
            let marked = query
                .iter()
                .find(|(k, _)| k.as_ref() == "x-id")
                .map(|(_, v)| v.as_ref().to_string())
                .filter(|n| operation_names().contains(n));
            return marked.unwrap_or_else(|| "Unknown".to_string());
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
            "PUT" if q("renameObject") => "RenameObject",
            "PUT" if q("encryption") => "UpdateObjectEncryption",
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
    "CreateSession",
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
    "RenameObject",
    "UpdateObjectEncryption",
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
/// A rename removes its source's key.
pub fn is_destructive(op: &str) -> bool {
    op.starts_with("Delete")
        || op == "AbortMultipartUpload"
        || op == "RenameObject"
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
            "renameObject",
            "encryption",
            "torrent",
            "x-id=GetObjectTorrent",
            "x-id=PutObject",
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
            let delete = classify("DELETE", Some("b"), None, &query, none);
            assert_eq!(delete, format!("Delete{name}"));
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

    #[test]
    fn an_object_subresource_sonde_does_not_know_is_never_a_read_write_or_delete() {
        let none: &[(String, String)] = &[];
        let k = Some("k");
        for method in ["GET", "PUT", "DELETE", "POST"] {
            for sub in ["torrent", "annotation", "unheard-of"] {
                assert_eq!(
                    classify(method, Some("b"), k, &q(&[sub]), none),
                    "Unknown",
                    "{method} ?{sub}"
                );
            }
        }
        // the SDKs' marker names it when sonde knows the name
        assert_eq!(
            classify("PUT", Some("b"), k, &q(&["fresh", "x-id=PutObject"]), none),
            "PutObject"
        );
        assert_eq!(
            classify("PUT", Some("b"), k, &q(&["fresh", "x-id=Made-up"]), none),
            "Unknown"
        );
        // what plain requests carry changes nothing
        for plain in [
            vec!["versionId=1", "response-content-type=a"],
            vec!["partNumber=2"],
            vec!["x-id=GetObject"],
            vec!["X-Amz-Algorithm=AWS4-HMAC-SHA256", "X-Amz-Signature=s"],
            vec!["AWSAccessKeyId=a", "Signature=s", "Expires=1"],
        ] {
            assert_eq!(classify("GET", Some("b"), k, &q(&plain), none), "GetObject");
        }
        assert_eq!(
            classify("DELETE", Some("b"), k, &q(&["versionId=1"]), none),
            "DeleteObject"
        );
    }

    #[test]
    fn rename_and_encryption_update_are_named() {
        let none: &[(String, String)] = &[];
        let src = [("x-amz-rename-source".to_string(), "/b/a".to_string())];
        assert_eq!(
            classify("PUT", Some("b"), Some("k"), &q(&["renameObject"]), &src),
            "RenameObject"
        );
        assert_eq!(
            classify("PUT", Some("b"), Some("k"), &q(&["encryption"]), none),
            "UpdateObjectEncryption"
        );
        // a bucket's encryption stays the bucket's
        assert_eq!(
            classify("PUT", Some("b"), None, &q(&["encryption"]), none),
            "PutBucketEncryption"
        );
        // whatever else the query holds, and whatever the SDKs' marker says
        assert_eq!(
            classify(
                "PUT",
                Some("b"),
                Some("k"),
                &q(&["renameObject", "zzz", "x-id=PutObject"]),
                &src
            ),
            "RenameObject"
        );
        // a rename takes its source's key away; re-encrypting keeps the data
        assert!(is_destructive("RenameObject"));
        assert!(!is_destructive("UpdateObjectEncryption"));
    }
}
