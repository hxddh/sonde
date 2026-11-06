//! "Copy as aws cli" and "copy as curl".
//!
//! Both renderings come from a redacted record, so they never carry a
//! secret. The curl command has no signature: it is marked as needing one,
//! with curl's own `--aws-sigv4` option shown as the way to add it.

use crate::Record;

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@,+%".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn kebab(op: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = op.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let prev_lower =
                i > 0 && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit());
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            let prev_upper = i > 0 && chars[i - 1].is_ascii_uppercase();
            if i > 0 && (prev_lower || (prev_upper && next_lower)) {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(*c);
        }
    }
    out
}

/// The endpoint part of a URL (`scheme://host[:port]`).
fn endpoint_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let mut out = format!("{}://{}", parsed.scheme(), parsed.host_str()?);
    if let Some(p) = parsed.port() {
        out.push_str(&format!(":{p}"));
    }
    Some(out)
}

fn query<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.query
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn req_header<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.request_headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

/// An `aws s3api` command doing what the record did.
pub fn as_aws_cli(r: &Record) -> String {
    let op = match r.operation.as_str() {
        "ListObjectsV2" => "list-objects-v2".to_string(),
        "ListBuckets" => "list-buckets".to_string(),
        "GetCallerIdentity" => return format!("aws sts get-caller-identity{}", region_arg(r)),
        other => kebab(other),
    };
    let mut parts = vec!["aws".to_string(), "s3api".into(), op.clone()];
    if let Some(b) = &r.bucket {
        parts.push("--bucket".into());
        parts.push(shell_quote(b));
    }
    if let Some(k) = &r.key {
        parts.push("--key".into());
        parts.push(shell_quote(k));
    }
    let map = [
        ("prefix", "--prefix"),
        ("delimiter", "--delimiter"),
        ("start-after", "--start-after"),
        ("continuation-token", "--continuation-token"),
        ("max-keys", "--max-keys"),
        ("versionId", "--version-id"),
        ("uploadId", "--upload-id"),
        ("partNumber", "--part-number"),
        ("key-marker", "--key-marker"),
        ("version-id-marker", "--version-id-marker"),
        ("upload-id-marker", "--upload-id-marker"),
        ("part-number-marker", "--part-number-marker"),
        ("max-uploads", "--max-uploads"),
        ("max-parts", "--max-parts"),
    ];
    for (q, flag) in map {
        if let Some(v) = query(r, q) {
            let decoded = percent_encoding::percent_decode_str(v)
                .decode_utf8_lossy()
                .to_string();
            parts.push(flag.into());
            parts.push(shell_quote(&decoded));
        }
    }
    let headers = [
        ("range", "--range"),
        ("if-match", "--if-match"),
        ("if-none-match", "--if-none-match"),
        ("x-amz-checksum-mode", "--checksum-mode"),
        ("x-amz-copy-source", "--copy-source"),
        ("x-amz-storage-class", "--storage-class"),
    ];
    for (h, flag) in headers {
        if let Some(v) = req_header(r, h) {
            parts.push(flag.into());
            parts.push(shell_quote(v));
        }
    }
    if op == "get-object" {
        parts.push("/dev/null".into());
    }
    if let Some(e) = endpoint_url(&r.url)
        && !e.ends_with(".amazonaws.com")
    {
        parts.push("--endpoint-url".into());
        parts.push(e);
    }
    let mut out = parts.join(" ");
    out.push_str(&region_arg(r));
    if matches!(r.method.as_str(), "PUT" | "POST") && r.request_bytes.unwrap_or(0) > 0 {
        out.push_str("  # request body not reproduced: add --body <file>");
    }
    out
}

fn region_arg(r: &Record) -> String {
    r.region
        .as_ref()
        .map(|reg| format!(" --region {reg}"))
        .unwrap_or_default()
}

/// A curl command with the record's method, URL and headers, minus every
/// signing header. It needs a signature: curl adds one with `--aws-sigv4`.
pub fn as_curl(r: &Record) -> String {
    let skip = [
        "authorization",
        "x-amz-date",
        "x-amz-security-token",
        "x-amz-content-sha256",
        "host",
        "content-length",
        "user-agent",
        "amz-sdk-invocation-id",
        "amz-sdk-request",
    ];
    let mut url = r.url.clone();
    if url.contains("X-Amz-Signature") {
        // a presigned URL: drop the presign query, curl signs anew
        if let Ok(mut u) = url::Url::parse(&url) {
            let kept: Vec<(String, String)> = u
                .query_pairs()
                .filter(|(k, _)| !k.starts_with("X-Amz-"))
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            u.set_query(None);
            if !kept.is_empty() {
                u.query_pairs_mut().extend_pairs(kept);
            }
            url = u.to_string();
        }
    }
    let mut lines = vec![
        "# UNSIGNED: sonde never exports a signature. Sign it with curl itself:".to_string(),
        format!(
            "#   --aws-sigv4 'aws:amz:{}:s3' --user \"$AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY\"",
            r.region.clone().unwrap_or_else(|| "us-east-1".into())
        ),
        format!("curl -sS -i -X {} {}", r.method, shell_quote(&url)),
    ];
    for h in &r.request_headers {
        if skip.contains(&h.name.to_ascii_lowercase().as_str()) {
            continue;
        }
        lines.push(format!(
            "  -H {}",
            shell_quote(&format!("{}: {}", h.name, h.value))
        ));
    }
    if r.method == "HEAD" {
        lines[2] = format!("curl -sS -I {}", shell_quote(&url));
    }
    lines.join(" \\\n").replacen(" \\\n", "\n", 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Header;

    fn rec() -> Record {
        Record {
            operation: "ListObjectsV2".into(),
            method: "GET".into(),
            url: "http://127.0.0.1:9000/bucket?list-type=2&prefix=logs%2F&delimiter=%2F".into(),
            bucket: Some("bucket".into()),
            query: vec![
                ("list-type".into(), "2".into()),
                ("prefix".into(), "logs%2F".into()),
                ("delimiter".into(), "%2F".into()),
            ],
            region: Some("us-east-1".into()),
            request_headers: vec![
                Header::new(
                    "Authorization",
                    "AWS4-HMAC-SHA256 Credential=AKIAX/s, SignedHeaders=host, Signature=zzz",
                ),
                Header::new("x-amz-date", "20240101T000000Z"),
                Header::new("x-amz-security-token", "TOKEN"),
                Header::new("x-amz-request-payer", "requester"),
            ],
            ..Default::default()
        }
        .redacted()
    }

    #[test]
    fn kebab_case() {
        assert_eq!(kebab("HeadObject"), "head-object");
        assert_eq!(
            kebab("GetBucketLifecycleConfiguration"),
            "get-bucket-lifecycle-configuration"
        );
        assert_eq!(kebab("ListMultipartUploads"), "list-multipart-uploads");
        assert_eq!(kebab("UploadPartCopy"), "upload-part-copy");
    }

    #[test]
    fn aws_cli() {
        let cmd = as_aws_cli(&rec());
        assert_eq!(
            cmd,
            "aws s3api list-objects-v2 --bucket bucket --prefix logs/ --delimiter / --endpoint-url http://127.0.0.1:9000 --region us-east-1"
        );
    }

    #[test]
    fn curl_has_no_secrets() {
        let cmd = as_curl(&rec());
        assert!(cmd.contains("UNSIGNED"));
        assert!(cmd.contains("--aws-sigv4"));
        assert!(!cmd.contains("zzz") && !cmd.contains("TOKEN"));
        assert!(!cmd.to_lowercase().contains("authorization:"));
        assert!(cmd.contains("x-amz-request-payer: requester"));
        assert!(cmd.contains("curl -sS -i -X GET"));
    }

    #[test]
    fn curl_strips_presign() {
        let mut r = rec();
        r.url = "https://h/b/k?versionId=1&X-Amz-Signature=abc&X-Amz-Date=x".into();
        let cmd = as_curl(&r.redacted());
        assert!(cmd.contains("https://h/b/k?versionId=1"), "{cmd}");
        assert!(!cmd.contains("X-Amz-Signature"));
    }
}
