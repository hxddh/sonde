//! Masks secrets in headers, URLs and text.
//!
//! What is masked:
//! - `Authorization`: the signature is replaced, the access key id shortened
//!   to its first four and last two characters; the scope and the signed
//!   header list stay (they are what one diagnoses);
//! - `x-amz-security-token`, `x-amz-s3session-token` (S3 Express), the
//!   SSE-C key headers, `Cookie`, `Proxy-Authorization`,
//!   `x-amz-copy-source-server-side-encryption-customer-key`;
//! - presign query parameters: `X-Amz-Signature`, `X-Amz-Security-Token`,
//!   `X-Amz-Credential` (its key id), SigV2's `Signature` and `AWSAccessKeyId`;
//! - the same names when they appear inside text, in any case, and what an
//!   error body echoes of a signature: `<SignatureProvided>`,
//!   `<StringToSign>` and `<CanonicalRequest>` (it holds signed headers'
//!   values) with their `…Bytes` twins whole, `<AWSAccessKeyId>` shortened;
//!   a session's credentials (`<SessionToken>`, `<SecretAccessKey>`,
//!   `<AccessKeyId>`).

pub const MASK: &str = "«redacted»";

const SECRET_HEADERS: &[&str] = &[
    "x-amz-security-token",
    "x-amz-s3session-token",
    "x-amz-server-side-encryption-customer-key",
    "x-amz-copy-source-server-side-encryption-customer-key",
    "cookie",
    "set-cookie",
    "proxy-authorization",
    "x-amz-content-sha256-secret",
];

const SECRET_QUERY: &[&str] = &[
    "x-amz-signature",
    "x-amz-security-token",
    "x-amz-s3session-token",
    "signature",
];

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

/// Whether [`redact_header`] would leave the value as it is, without
/// making it: what a record already redacted is asked on its way to a file.
pub fn header_is_redacted(name: &str, value: &str) -> bool {
    if name.eq_ignore_ascii_case("authorization") {
        return value.contains(MASK);
    }
    !SECRET_HEADERS.iter().any(|h| name.eq_ignore_ascii_case(h)) || value == MASK
}

/// Whether [`redact_query_value`] would leave the value as it is.
pub fn query_value_is_redacted(name: &str, value: &str) -> bool {
    if SECRET_QUERY.iter().any(|q| name.eq_ignore_ascii_case(q)) {
        return value == MASK;
    }
    if name.eq_ignore_ascii_case("x-amz-credential") || name.eq_ignore_ascii_case("awsaccesskeyid")
    {
        return redact_query_value(name, value) == value;
    }
    true
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
/// returns the string to sign, the signature provided, the key id and the
/// canonical request, which holds every signed header's value: a session
/// token, an SSE-C key), and presign parameters anywhere in the text. An
/// element cut open (a body cut at its limit) is masked to the end.
pub fn redact_text(text: &str) -> String {
    let mut out = text.to_string();
    for tag in [
        "SignatureProvided",
        "StringToSign",
        "StringToSignBytes",
        "CanonicalRequest",
        "CanonicalRequestBytes",
        "Signature",
        "SecurityToken",
        "SessionToken",
        "SecretAccessKey",
        "Token",
    ] {
        out = replace_element(&out, tag, |_| MASK.to_string());
    }
    for tag in ["AWSAccessKeyId", "AccessKeyId"] {
        out = replace_element(&out, tag, mask_key_id);
    }
    redact_parameters(&out)
}

/// Presign parameters in text (a URL, a canonical query line, either in
/// XML with `&amp;`), any case: each value redacted as a query's is.
fn redact_parameters(text: &str) -> String {
    let b = text.as_bytes();
    let starts = |c: u8| c.is_ascii_whitespace() || b"?&;\"'>(".contains(&c);
    let ends = |c: u8| c.is_ascii_whitespace() || b"&#<>\"'".contains(&c);
    let mut out = String::with_capacity(text.len());
    let (mut i, mut copied) = (0, 0);
    while i < b.len() {
        if !(b[i].is_ascii_alphabetic() && (i == 0 || starts(b[i - 1]))) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-' || b[j] == b'_') {
            j += 1;
        }
        if j < b.len() && b[j] == b'=' {
            let mut k = j + 1;
            while k < b.len() && !ends(b[k]) {
                k += 1;
            }
            let (name, value) = (&text[i..j], &text[j + 1..k]);
            let red = redact_query_value(name, value);
            if red != value {
                out.push_str(&text[copied..j + 1]);
                out.push_str(&red);
                copied = k;
            }
            i = k;
        } else {
            i = j;
        }
    }
    out.push_str(&text[copied..]);
    out
}

fn replace_element(text: &str, tag: &str, f: impl Fn(&str) -> String) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        out.push_str(&rest[..start + open.len()]);
        let Some(end) = after.find(&close) else {
            // cut open: nothing after it is known not to be the secret
            // (what is there already is a mask, perhaps itself cut)
            out.push_str(if after.starts_with('«') { after } else { MASK });
            return out;
        };
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
        assert_eq!(
            redact_header(
                "Authorization",
                "AWS AKIAIOSFODNN7EXAMPLE:frJIUN8DYpKDtOLCwo//yllqDzg="
            ),
            format!("AWS AKIA…LE:{MASK}")
        );
        assert_eq!(
            redact_header("authorization", "Bearer abc.def"),
            format!("Bearer {MASK}")
        );
        assert_eq!(redact_header("authorization", "garbage"), MASK);
    }

    #[test]
    fn secret_headers() {
        for h in [
            "x-amz-security-token",
            "X-Amz-Security-Token",
            "x-amz-server-side-encryption-customer-key",
            "Cookie",
        ] {
            assert_eq!(redact_header(h, "secretvalue"), MASK, "{h}");
        }
        assert_eq!(
            redact_header("x-amz-server-side-encryption-customer-key-MD5", "md5"),
            "md5"
        );
        assert_eq!(
            redact_header("x-amz-date", "20130524T000000Z"),
            "20130524T000000Z"
        );
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
        let v2 = redact_url(
            "https://h/b/k?AWSAccessKeyId=AKIAIOSFODNN7EXAMPLE&Expires=1&Signature=abc%2B",
        );
        assert!(
            !v2.contains("abc%2B") && !v2.contains("AKIAIOSFODNN7EXAMPLE"),
            "{v2}"
        );
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

    /// S3's answer to a request signed wrong, as it comes: the canonical
    /// request echoes every signed header's value.
    const MISMATCH: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>SignatureDoesNotMatch</Code><Message>The request signature we calculated does not match the signature you provided. Check your key and signing method.</Message><AWSAccessKeyId>ASIAIOSFODNN7EXAMPLE</AWSAccessKeyId><StringToSign>AWS4-HMAC-SHA256\n20130524T000000Z\n20130524/us-east-1/s3/aws4_request\n7344ae5b7ee6c3e7e6b0fe0640412a37625d1fbfff95c48bbb2dc43964946972</StringToSign><SignatureProvided>f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41</SignatureProvided><StringToSignBytes>41 57 53 34</StringToSignBytes><CanonicalRequest>PUT\n/b/k\nX-Amz-Credential=ASIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request&amp;X-Amz-Security-Token=FQoGZXIvYXdzQUERY\nhost:b.s3.amazonaws.com\nx-amz-date:20130524T000000Z\nx-amz-security-token:FQoGZXIvYXdzHEADER\nx-amz-server-side-encryption-customer-key:c3NlLWMta2V5LXNlY3JldA==\n\nhost;x-amz-date;x-amz-security-token;x-amz-server-side-encryption-customer-key\nUNSIGNED-PAYLOAD</CanonicalRequest><CanonicalRequestBytes>50 55 54 0a 2f 62 2f 6b 0a 46 51 6f 47</CanonicalRequestBytes><RequestId>R1</RequestId><HostId>H1</HostId></Error>";

    #[test]
    fn a_signature_mismatch_keeps_nothing_it_echoes() {
        let r = redact_text(MISMATCH);
        for secret in [
            "FQoGZXIvYXdz",
            "c3NlLWMta2V5LXNlY3JldA",
            "f0e8bdb8",
            "ASIAIOSFODNN7EXAMPLE",
            "50 55 54 0a",
            // the string to sign, as its bytes beside it
            "7344ae5b7ee6c3e7",
            "41 57 53 34",
        ] {
            assert!(!r.contains(secret), "{secret} in {r}");
        }
        assert!(r.contains("<Code>SignatureDoesNotMatch</Code>"), "{r}");
        assert!(r.contains("<RequestId>R1</RequestId>"), "{r}");
        assert_eq!(redact_text(&r), r, "idempotent");
    }

    #[test]
    fn presign_parameters_in_text_in_any_case_and_any_separator() {
        for text in [
            "url\thttps://h/k?x-amz-signature=deadbeef&X-Amz-Date=1",
            "<Url>https://h/k?X-Amz-Date=1&amp;X-Amz-Signature=deadbeef&amp;X-Amz-Expires=9</Url>",
            "GET\n/k\nX-Amz-Date=1&X-Amz-Security-Token=deadbeef&X-Amz-SignedHeaders=host",
            "see\nhttps://h/k?AWSAccessKeyId=AKIAIOSFODNN7EXAMPLE&Signature=deadbeef",
        ] {
            let r = redact_text(text);
            assert!(!r.contains("deadbeef"), "{r}");
            assert!(!r.contains("AKIAIOSFODNN7EXAMPLE"), "{r}");
            assert_eq!(redact_text(&r), r, "idempotent");
        }
        let r = redact_text("q: X-Amz-Date=1&amp;X-Amz-Expires=9");
        assert_eq!(r, "q: X-Amz-Date=1&amp;X-Amz-Expires=9");
    }

    #[test]
    fn an_element_cut_open_is_masked_to_the_end() {
        let at = MISMATCH.find("x-amz-security-token:").unwrap() + 30;
        let r = redact_text(&MISMATCH[..at]);
        assert!(!r.contains("FQoG"), "{r}");
        assert_eq!(redact_text(&r), r, "idempotent");
        // a record's error body, cut at its limit inside the echo
        let pad =
            crate::record::MAX_ERROR_BODY - MISMATCH.find("x-amz-security-token:").unwrap() - 25;
        let body = format!("{}{}", " ".repeat(pad), MISMATCH);
        let mut rec = crate::Record::default();
        rec.rare_mut().error_body = Some(body);
        let rec = rec.redacted();
        let got = rec.rare().error_body.clone().unwrap();
        assert!(!got.contains("FQoG"), "{}", &got[got.len() - 200..]);
        assert!(
            got.ends_with("… (truncated)"),
            "{}",
            &got[got.len() - 200..]
        );
        assert_eq!(rec.clone().redacted(), rec, "idempotent");
    }

    /// A transport error names the URL it failed on: a presigned one's
    /// signature is masked there as anywhere.
    #[test]
    fn a_transport_errors_url_is_redacted() {
        let mut rec = crate::Record::default();
        rec.rare_mut().transport_error = Some(
            "error sending request for url (https://h/b/k?X-Amz-Credential=AKIAXYZ%2F1&X-Amz-Signature=deadbeef)".into(),
        );
        let rec = rec.redacted();
        let got = rec.rare().transport_error.clone().unwrap();
        assert!(
            !got.contains("deadbeef") && !got.contains("AKIAXYZ"),
            "{got}"
        );
        assert!(got.starts_with("error sending request for url (https://h/b/k?"));
    }

    /// 33.0's review: S3 Express signs with a session's token in a header
    /// of its own, and `CreateSession` answers with the session's keys.
    #[test]
    fn an_express_sessions_token_and_keys() {
        assert_eq!(redact_header("x-amz-s3session-token", "AgAAAsecret"), MASK);
        assert_eq!(redact_header("X-Amz-S3session-Token", "AgAAAsecret"), MASK);
        let url = redact_url(
            "https://b--usw2-az1--x-s3.s3express-usw2-az1.us-west-2.amazonaws.com/k?X-Amz-S3session-Token=AgAAAsecret&X-Amz-Expires=9",
        );
        assert!(
            !url.contains("AgAAAsecret") && url.contains("X-Amz-Expires=9"),
            "{url}"
        );
        let body = "<CreateSessionResult><Credentials><SessionToken>AgAAAsecret</SessionToken><SecretAccessKey>wJalrXUtnFEMI</SecretAccessKey><AccessKeyId>ASIAIOSFODNN7EXAMPLE</AccessKeyId><Expiration>2026-01-01T00:00:00Z</Expiration></Credentials></CreateSessionResult>";
        let r = redact_text(body);
        for secret in ["AgAAAsecret", "wJalrXUtnFEMI", "ASIAIOSFODNN7EXAMPLE"] {
            assert!(!r.contains(secret), "{secret} in {r}");
        }
        assert!(
            r.contains("<Expiration>2026-01-01T00:00:00Z</Expiration>"),
            "{r}"
        );
        assert_eq!(redact_text(&r), r, "idempotent");
        let r = redact_text(
            "x-amz-s3session-token:AgAAAsecret\nhost:h\n\nGET /?x-amz-s3session-token=AgAAAsecret&a=1",
        );
        assert!(!r.contains("=AgAAAsecret"), "{r}");
    }

    /// What a record's way into a file asks ([`crate::Record::is_redacted`])
    /// is what redacting it again would find.
    #[test]
    fn a_record_says_it_is_redacted_when_redacting_it_changes_nothing() {
        use crate::{Header, Record};
        let mut records = vec![Record::default()];
        for (name, value) in [
            ("Authorization", AUTH),
            ("authorization", "Bearer abc"),
            ("x-amz-security-token", "T"),
            ("X-Amz-S3session-Token", "T"),
            ("Cookie", "a=b"),
            ("range", "bytes=0-1"),
            ("x-amz-server-side-encryption-customer-key-MD5", "md5"),
        ] {
            let mut r = Record::default();
            r.lists_mut().request_headers =
                vec![Header::new("host", "h"), Header::new(name, value)];
            records.push(r.clone());
            let mut r = Record::default();
            r.lists_mut().response_headers = vec![Header::new(name, value)];
            records.push(r);
        }
        for (name, value) in [
            ("X-Amz-Signature", "abc"),
            ("x-amz-security-token", "abc"),
            ("Signature", "abc"),
            (
                "X-Amz-Credential",
                "AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request",
            ),
            ("AWSAccessKeyId", "AKIAIOSFODNN7EXAMPLE"),
            ("prefix", "a/"),
        ] {
            let mut r = Record::default();
            r.lists_mut().query = vec![(name.into(), value.into())];
            records.push(r);
        }
        for url in [
            "https://h/b/k",
            "https://h/b/k?versionId=1",
            "https://h/b/k?X-Amz-Signature=abc&X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2Fs",
        ] {
            let mut r = Record::default();
            r.rare_mut().url = url.into();
            records.push(r);
        }
        for text in ["<Error><Code>NoSuchKey</Code></Error>", MISMATCH, "plain"] {
            let mut r = Record::default();
            r.rare_mut().error_body = Some(text.into());
            records.push(r.clone());
            r.rare_mut().request_body = Some(text.into());
            r.rare_mut().transport_error = Some(format!("url (https://h/k?Signature={text})"));
            records.push(r);
        }
        let mut secret = 0;
        for r in records {
            let redacted = r.clone().redacted();
            assert_eq!(r.is_redacted(), redacted == r, "{r:?}");
            assert!(redacted.is_redacted(), "{redacted:?}");
            secret += usize::from(!r.is_redacted());
        }
        assert!(secret >= 15, "{secret}");
    }

    #[test]
    fn short_key_ids() {
        assert_eq!(mask_key_id("abc"), MASK);
        assert_eq!(mask_key_id("minioadmin"), "mini…in");
    }
}
