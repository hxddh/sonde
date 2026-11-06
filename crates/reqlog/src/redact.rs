//! Masks secrets in headers, URLs and text.
//!
//! What is masked:
//! - `Authorization`: the signature is replaced, the access key id shortened
//!   to its first four and last two characters; the scope and the signed
//!   header list stay (they are what one diagnoses);
//! - `x-amz-security-token`, the SSE-C key headers, `Cookie`,
//!   `Proxy-Authorization`, `x-amz-copy-source-server-side-encryption-customer-key`;
//! - presign query parameters: `X-Amz-Signature`, `X-Amz-Security-Token`,
//!   `X-Amz-Credential` (its key id), SigV2's `Signature` and `AWSAccessKeyId`;
//! - the same names when they appear inside text (error bodies echo
//!   `<StringToSign>`, `<SignatureProvided>`, `<AWSAccessKeyId>`).

pub const MASK: &str = "«redacted»";

const SECRET_HEADERS: &[&str] = &[
    "x-amz-security-token",
    "x-amz-server-side-encryption-customer-key",
    "x-amz-copy-source-server-side-encryption-customer-key",
    "cookie",
    "set-cookie",
    "proxy-authorization",
    "x-amz-content-sha256-secret",
];

const SECRET_QUERY: &[&str] = &["x-amz-signature", "x-amz-security-token", "signature"];

/// Shortens an access key id: `AKIAIOSFODNN7EXAMPLE` → `AKIA…LE`.
pub fn mask_key_id(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() <= 6 {
        return MASK.to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 2..].iter().collect();
    format!("{head}…{tail}")
}

/// Redacts one header value given its name.
pub fn redact_header(name: &str, value: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower == "authorization" {
        return redact_authorization(value);
    }
    if SECRET_HEADERS.contains(&lower.as_str()) {
        return MASK.to_string();
    }
    value.to_string()
}

/// `AWS4-HMAC-SHA256 Credential=AKID/20150830/us-east-1/s3/aws4_request,
/// SignedHeaders=host;x-amz-date, Signature=abc` keeps everything but the
/// signature and most of the key id. Unknown schemes are masked whole.
pub fn redact_authorization(value: &str) -> String {
    if value.contains(MASK) {
        return value.to_string();
    }
    let Some((scheme, rest)) = value.split_once(' ') else {
        return MASK.to_string();
    };
    if scheme.starts_with("AWS4-") {
        let parts: Vec<String> = rest
            .split(',')
            .map(|p| {
                let p = p.trim();
                if let Some(cred) = p.strip_prefix("Credential=") {
                    let (id, scope) = cred.split_once('/').unwrap_or((cred, ""));
                    format!("Credential={}/{}", mask_key_id(id), scope)
                } else if p.starts_with("Signature=") {
                    format!("Signature={MASK}")
                } else {
                    p.to_string()
                }
            })
            .collect();
        format!("{scheme} {}", parts.join(", "))
    } else if scheme == "AWS" {
        // SigV2: `AWS AKID:signature`
        let id = rest.split(':').next().unwrap_or("");
        format!("AWS {}:{MASK}", mask_key_id(id))
    } else {
        format!("{scheme} {MASK}")
    }
}

/// Redacts one query parameter's value.
pub fn redact_query_value(name: &str, value: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if SECRET_QUERY.contains(&lower.as_str()) {
        return MASK.to_string();
    }
    if lower == "x-amz-credential" {
        let decoded = percent_encoding::percent_decode_str(value).decode_utf8_lossy();
        let (id, scope) = decoded.split_once('/').unwrap_or((&decoded, ""));
        return format!("{}/{}", mask_key_id(id), scope);
    }
    if lower == "awsaccesskeyid" {
        return mask_key_id(value);
    }
    value.to_string()
}

/// Redacts the query of a URL, keeping its shape.
pub fn redact_url(raw: &str) -> String {
    let Some((base, query)) = raw.split_once('?') else {
        return raw.to_string();
    };
    let (query, fragment) = match query.split_once('#') {
        Some((q, f)) => (q, Some(f)),
        None => (query, None),
    };
    let parts: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => {
                let decoded_k = percent_encoding::percent_decode_str(k).decode_utf8_lossy();
                let red = redact_query_value(&decoded_k, v);
                if red == v {
                    pair.to_string()
                } else {
                    let enc: String = percent_encoding::utf8_percent_encode(
                        &red,
                        percent_encoding::NON_ALPHANUMERIC,
                    )
                    .to_string();
                    // keep the mask readable: it never needs to round-trip
                    let enc = if red.contains(MASK) { red.clone() } else { enc };
                    format!("{k}={enc}")
                }
            }
            None => pair.to_string(),
        })
        .collect();
    let mut out = format!("{base}?{}", parts.join("&"));
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

/// Masks secrets that services echo in error bodies (SignatureDoesNotMatch
/// returns the string to sign, the signature provided and the key id).
pub fn redact_text(text: &str) -> String {
    let mut out = text.to_string();
    for tag in ["SignatureProvided", "StringToSignBytes", "Signature", "SecurityToken", "Token"] {
        out = replace_element(&out, tag, |_| MASK.to_string());
    }
    out = replace_element(&out, "AWSAccessKeyId", mask_key_id);
    // presigned URLs inside text
    if out.contains("X-Amz-Signature=") || out.contains("X-Amz-Security-Token=") {
        let mut result = String::with_capacity(out.len());
        for (i, word) in out.split(' ').enumerate() {
            if i > 0 {
                result.push(' ');
            }
            if word.contains('?') && (word.contains("X-Amz-")) {
                result.push_str(&redact_url(word));
            } else {
                result.push_str(word);
            }
        }
        out = result;
    }
    out
}

fn replace_element(text: &str, tag: &str, f: impl Fn(&str) -> String) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find(&close) else { break };
        out.push_str(&rest[..start + open.len()]);
        let inner = &after[..end];
        if inner == MASK || inner.contains('…') {
            out.push_str(inner);
        } else {
            out.push_str(&f(inner));
        }
        out.push_str(&close);
        rest = &after[end + close.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTH: &str = "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41";

    #[test]
    fn authorization_keeps_scope_drops_signature() {
        let r = redact_header("Authorization", AUTH);
        assert!(!r.contains("f0e8bdb8"));
        assert!(!r.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(r.contains("AKIA…LE/20130524/us-east-1/s3/aws4_request"));
        assert!(r.contains("SignedHeaders=host;range;x-amz-content-sha256;x-amz-date"));
        assert!(r.contains(&format!("Signature={MASK}")));
        // idempotent
        assert_eq!(redact_header("authorization", &r), r);
    }

    #[test]
    fn sigv2_and_unknown_schemes() {
        assert_eq!(redact_header("Authorization", "AWS AKIAIOSFODNN7EXAMPLE:frJIUN8DYpKDtOLCwo//yllqDzg="), format!("AWS AKIA…LE:{MASK}"));
        assert_eq!(redact_header("authorization", "Bearer abc.def"), format!("Bearer {MASK}"));
        assert_eq!(redact_header("authorization", "garbage"), MASK);
    }

    #[test]
    fn secret_headers() {
        for h in ["x-amz-security-token", "X-Amz-Security-Token", "x-amz-server-side-encryption-customer-key", "Cookie"] {
            assert_eq!(redact_header(h, "secretvalue"), MASK, "{h}");
        }
        assert_eq!(redact_header("x-amz-server-side-encryption-customer-key-MD5", "md5"), "md5");
        assert_eq!(redact_header("x-amz-date", "20130524T000000Z"), "20130524T000000Z");
    }

    #[test]
    fn presigned_url() {
        let url = "https://examplebucket.s3.amazonaws.com/test.txt?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Date=20130524T000000Z&X-Amz-Expires=86400&X-Amz-SignedHeaders=host&X-Amz-Security-Token=FQoGZXIvYXdzEJr&X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404";
        let r = redact_url(url);
        assert!(!r.contains("aeeed9bb"), "{r}");
        assert!(!r.contains("FQoGZXIvYXdzEJr"), "{r}");
        assert!(!r.contains("AKIAIOSFODNN7EXAMPLE"), "{r}");
        assert!(r.contains("X-Amz-Expires=86400"));
        assert!(r.contains("X-Amz-Date=20130524T000000Z"));
        assert_eq!(redact_url(&r), r, "idempotent");
        assert_eq!(redact_url("https://h/b/k"), "https://h/b/k");
        let v2 = redact_url("https://h/b/k?AWSAccessKeyId=AKIAIOSFODNN7EXAMPLE&Expires=1&Signature=abc%2B");
        assert!(!v2.contains("abc%2B") && !v2.contains("AKIAIOSFODNN7EXAMPLE"), "{v2}");
    }

    #[test]
    fn error_bodies() {
        let body = "<Error><Code>SignatureDoesNotMatch</Code><AWSAccessKeyId>AKIAIOSFODNN7EXAMPLE</AWSAccessKeyId><StringToSign>AWS4-HMAC-SHA256\n20130524T000000Z</StringToSign><SignatureProvided>deadbeef</SignatureProvided><StringToSignBytes>41 57</StringToSignBytes><RequestId>R1</RequestId></Error>";
        let r = redact_text(body);
        assert!(!r.contains("deadbeef"));
        assert!(!r.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(r.contains("<RequestId>R1</RequestId>"));
        assert!(r.contains("<Code>SignatureDoesNotMatch</Code>"));
        assert_eq!(redact_text(&r), r);
    }

    #[test]
    fn short_key_ids() {
        assert_eq!(mask_key_id("abc"), MASK);
        assert_eq!(mask_key_id("minioadmin"), "mini…in");
    }
}
