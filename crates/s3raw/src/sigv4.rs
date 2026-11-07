//! AWS Signature Version 4, written out so every intermediate (the
//! canonical request, the string to sign) can be shown to the user.
//!
//! Follows the S3 rules: the path is encoded once and never normalized,
//! query parameters are sorted by name then value, header values are
//! trimmed and their inner runs of spaces collapsed.

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::credentials::Credentials;

type HmacSha256 = Hmac<Sha256>;

pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";
pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
pub const STREAMING_UNSIGNED_TRAILER: &str = "STREAMING-UNSIGNED-PAYLOAD-TRAILER";

/// RFC 3986 percent-encoding as SigV4 wants it: unreserved characters stay,
/// everything else is `%XX` in upper case; `/` stays when `keep_slash`.
pub fn uri_encode(input: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// What to sign.
#[derive(Debug, Clone)]
pub struct SignInput<'a> {
    pub method: &'a str,
    /// The canonical URI: already encoded (`/bucket/a%20b`).
    pub canonical_uri: &'a str,
    /// Query parameters, not encoded.
    pub query: &'a [(String, String)],
    /// Headers to sign (name, value). `host` must be among them.
    pub headers: &'a [(String, String)],
    /// The payload's SHA-256 in hex, or `UNSIGNED-PAYLOAD` / a streaming marker.
    pub payload_hash: &'a str,
    pub region: &'a str,
    pub service: &'a str,
    pub time: DateTime<Utc>,
}

/// Every intermediate of a signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub canonical_request: String,
    pub string_to_sign: String,
    pub signed_headers: String,
    pub scope: String,
    pub signature: String,
    pub amz_date: String,
}

impl Signature {
    /// The `Authorization` header value.
    pub fn authorization(&self, access_key_id: &str) -> String {
        format!(
            "{ALGORITHM} Credential={access_key_id}/{}, SignedHeaders={}, Signature={}",
            self.scope, self.signed_headers, self.signature
        )
    }
}

pub fn canonical_query(query: &[(String, String)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (uri_encode(k, false), uri_encode(v, false)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn normalize_value(v: &str) -> String {
    v.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    hmac(&k_service, b"aws4_request")
}

/// Signs `input` with `creds`.
pub fn sign(input: &SignInput<'_>, creds: &Credentials) -> Signature {
    let amz_date = input.time.format("%Y%m%dT%H%M%SZ").to_string();
    let date = input.time.format("%Y%m%d").to_string();

    let mut headers: Vec<(String, String)> = input
        .headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), normalize_value(v)))
        .collect();
    headers.sort_by(|a, b| a.0.cmp(&b.0));
    // merge repeated names
    let mut merged: Vec<(String, String)> = Vec::new();
    for (k, v) in headers {
        match merged.last_mut() {
            Some(last) if last.0 == k => {
                last.1.push(',');
                last.1.push_str(&v);
            }
            _ => merged.push((k, v)),
        }
    }
    let canonical_headers: String = merged.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed_headers = merged
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        input.method,
        input.canonical_uri,
        canonical_query(input.query),
        canonical_headers,
        signed_headers,
        input.payload_hash
    );
    let scope = format!("{date}/{}/{}/aws4_request", input.region, input.service);
    let string_to_sign = format!(
        "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let key = signing_key(
        creds.secret_access_key.expose(),
        &date,
        input.region,
        input.service,
    );
    let signature = hex::encode(hmac(&key, string_to_sign.as_bytes()));
    Signature {
        canonical_request,
        string_to_sign,
        signed_headers,
        scope,
        signature,
        amz_date,
    }
}

/// Whether a header is signed: `host`, every `x-amz-*`, and the headers
/// whose change would change the request's meaning.
pub fn is_signed_header(name: &str) -> bool {
    let k = name.to_ascii_lowercase();
    k == "host"
        || k.starts_with("x-amz-")
        || k == "content-md5"
        || k == "content-type"
        || k == "range"
        || k.starts_with("if-")
        || k == "content-encoding"
}

/// Signs a request in place: adds `x-amz-date`, `x-amz-content-sha256`,
/// `x-amz-security-token` (with a session token) and `authorization` to
/// `headers`, which must hold `host`.
#[allow(clippy::too_many_arguments)]
pub fn sign_headers(
    headers: &mut Vec<(String, String)>,
    method: &str,
    canonical_uri: &str,
    query: &[(String, String)],
    payload_hash: &str,
    region: &str,
    service: &str,
    time: DateTime<Utc>,
    creds: &Credentials,
) -> Signature {
    headers.push((
        "x-amz-date".into(),
        time.format("%Y%m%dT%H%M%SZ").to_string(),
    ));
    headers.push(("x-amz-content-sha256".into(), payload_hash.to_string()));
    if let Some(t) = &creds.session_token {
        headers.push(("x-amz-security-token".into(), t.expose().to_string()));
    }
    let signed: Vec<(String, String)> = headers
        .iter()
        .filter(|(k, _)| is_signed_header(k))
        .cloned()
        .collect();
    let sig = sign(
        &SignInput {
            method,
            canonical_uri,
            query,
            headers: &signed,
            payload_hash,
            region,
            service,
            time,
        },
        creds,
    );
    headers.push((
        "authorization".into(),
        sig.authorization(&creds.access_key_id),
    ));
    sig
}

/// Presigns: returns the query to append (already including the signature).
/// `extra_query` holds the operation's own parameters.
#[allow(clippy::too_many_arguments)]
pub fn presign(
    method: &str,
    canonical_uri: &str,
    host: &str,
    extra_query: &[(String, String)],
    region: &str,
    service: &str,
    time: DateTime<Utc>,
    expires_secs: u64,
    creds: &Credentials,
) -> (Vec<(String, String)>, Signature) {
    let date = time.format("%Y%m%d").to_string();
    let amz_date = time.format("%Y%m%dT%H%M%SZ").to_string();
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let mut query: Vec<(String, String)> = extra_query.to_vec();
    query.push(("X-Amz-Algorithm".into(), ALGORITHM.into()));
    query.push((
        "X-Amz-Credential".into(),
        format!("{}/{scope}", creds.access_key_id),
    ));
    query.push(("X-Amz-Date".into(), amz_date));
    query.push(("X-Amz-Expires".into(), expires_secs.to_string()));
    if let Some(token) = &creds.session_token {
        query.push(("X-Amz-Security-Token".into(), token.expose().to_string()));
    }
    query.push(("X-Amz-SignedHeaders".into(), "host".into()));
    let headers = vec![("host".to_string(), host.to_string())];
    let sig = sign(
        &SignInput {
            method,
            canonical_uri,
            query: &query,
            headers: &headers,
            payload_hash: UNSIGNED_PAYLOAD,
            region,
            service,
            time,
        },
        creds,
    );
    query.push(("X-Amz-Signature".into(), sig.signature.clone()));
    (query, sig)
}

#[cfg(test)]
mod tests {
    //! Vectors from the AWS SigV4 test suite (`aws-sig-v4-test-suite`,
    //! service `service`, 2015-08-30) and from the S3 documentation's
    //! worked examples ("Signature Calculations for the Authorization
    //! Header", "Authenticating Requests: Using Query Parameters").
    use super::*;
    use chrono::TimeZone;

    fn suite_creds() -> Credentials {
        Credentials::new(
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            None,
        )
    }

    fn s3_creds() -> Credentials {
        Credentials::new(
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            None,
        )
    }

    fn suite_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap()
    }

    fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn suite(
        method: &str,
        uri: &str,
        query: &[(&str, &str)],
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Signature {
        let q = h(query);
        let hs = h(headers);
        let hash = sha256_hex(body);
        sign(
            &SignInput {
                method,
                canonical_uri: uri,
                query: &q,
                headers: &hs,
                payload_hash: &hash,
                region: "us-east-1",
                service: "service",
                time: suite_time(),
            },
            &suite_creds(),
        )
    }

    const SUITE_HEADERS: &[(&str, &str)] = &[
        ("Host", "example.amazonaws.com"),
        ("X-Amz-Date", "20150830T123600Z"),
    ];

    #[test]
    fn suite_get_vanilla() {
        let s = suite("GET", "/", &[], SUITE_HEADERS, b"");
        assert_eq!(
            s.canonical_request,
            "GET\n/\n\nhost:example.amazonaws.com\nx-amz-date:20150830T123600Z\n\nhost;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            s.string_to_sign,
            "AWS4-HMAC-SHA256\n20150830T123600Z\n20150830/us-east-1/service/aws4_request\nbb579772317eb040ac9ed261061d46c1f17a8133879d6129b6e1c25292927e63"
        );
        assert_eq!(
            s.signature,
            "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
        assert_eq!(
            s.authorization("AKIDEXAMPLE"),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    #[test]
    fn suite_post_vanilla() {
        let s = suite("POST", "/", &[], SUITE_HEADERS, b"");
        assert_eq!(
            s.signature,
            "5da7c1a2acd57cee7505fc6676e4e544621c30862966e37dddb68e92efbe5d6b"
        );
    }

    #[test]
    fn suite_get_vanilla_query_order_key_case() {
        let s = suite(
            "GET",
            "/",
            &[("Param2", "value2"), ("Param1", "value1")],
            SUITE_HEADERS,
            b"",
        );
        assert!(
            s.canonical_request
                .contains("\nParam1=value1&Param2=value2\n")
        );
        assert_eq!(
            s.signature,
            "b97d918cfa904a5beff61c982a1b6f458b799221646efd99d3219ec94cdf2500"
        );
    }

    #[test]
    fn suite_get_header_value_trim() {
        let s = suite(
            "GET",
            "/",
            &[],
            &[
                ("Host", "example.amazonaws.com"),
                ("My-Header1", " value1"),
                ("My-Header2", " \"a   b   c\""),
                ("X-Amz-Date", "20150830T123600Z"),
            ],
            b"",
        );
        assert!(s.canonical_request.contains("my-header2:\"a b c\"\n"));
        assert_eq!(
            s.signature,
            "acc3ed3afb60bb290fc8d2dd0098b9911fcaa05412b367055dee359757a9c736"
        );
    }

    #[test]
    fn suite_post_x_www_form_urlencoded() {
        let s = suite(
            "POST",
            "/",
            &[],
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Host", "example.amazonaws.com"),
                ("X-Amz-Date", "20150830T123600Z"),
            ],
            b"Param1=value1",
        );
        assert_eq!(
            s.signature,
            "ff11897932ad3f4e8b18135d722051e5ac45fc38421b1da7b9d196a0fe09473a"
        );
    }

    #[test]
    fn suite_get_unreserved() {
        let s = suite(
            "GET",
            "/-._~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
            &[],
            SUITE_HEADERS,
            b"",
        );
        assert_eq!(
            s.signature,
            "07ef7494c76fa4850883e2b006601f940f8a34d404d0cfa977f52a65bbf5f24f"
        );
    }

    #[test]
    fn suite_get_utf8() {
        let uri = uri_encode("/ሴ", true);
        assert_eq!(uri, "/%E1%88%B4");
        let s = suite("GET", &uri, &[], SUITE_HEADERS, b"");
        assert_eq!(
            s.signature,
            "8318018e0b0f223aa2bbf98705b62bb787dc9c0e678f255a891fd03141be5d85"
        );
    }

    fn s3_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap()
    }

    fn s3(
        method: &str,
        uri: &str,
        query: &[(&str, &str)],
        headers: &[(&str, &str)],
        hash: &str,
    ) -> Signature {
        let q = h(query);
        let hs = h(headers);
        sign(
            &SignInput {
                method,
                canonical_uri: uri,
                query: &q,
                headers: &hs,
                payload_hash: hash,
                region: "us-east-1",
                service: "s3",
                time: s3_time(),
            },
            &s3_creds(),
        )
    }

    #[test]
    fn s3_get_object_range() {
        let s = s3(
            "GET",
            "/test.txt",
            &[],
            &[
                ("Host", "examplebucket.s3.amazonaws.com"),
                ("Range", "bytes=0-9"),
                ("x-amz-content-sha256", EMPTY_SHA256),
                ("x-amz-date", "20130524T000000Z"),
            ],
            EMPTY_SHA256,
        );
        assert_eq!(
            s.signed_headers,
            "host;range;x-amz-content-sha256;x-amz-date"
        );
        assert_eq!(
            s.signature,
            "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn s3_put_object() {
        let body = b"Welcome to Amazon S3.";
        let hash = sha256_hex(body);
        assert_eq!(
            hash,
            "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
        );
        let s = s3(
            "PUT",
            &uri_encode("/test$file.text", true),
            &[],
            &[
                ("Date", "Fri, 24 May 2013 00:00:00 GMT"),
                ("Host", "examplebucket.s3.amazonaws.com"),
                ("x-amz-content-sha256", &hash),
                ("x-amz-date", "20130524T000000Z"),
                ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
            ],
            &hash,
        );
        assert_eq!(
            s.signature,
            "98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
        );
    }

    #[test]
    fn s3_get_lifecycle() {
        let s = s3(
            "GET",
            "/",
            &[("lifecycle", "")],
            &[
                ("Host", "examplebucket.s3.amazonaws.com"),
                ("x-amz-content-sha256", EMPTY_SHA256),
                ("x-amz-date", "20130524T000000Z"),
            ],
            EMPTY_SHA256,
        );
        assert_eq!(
            s.signature,
            "fea454ca298b7da1c68078a5d1bdbfbbe0d65c699e0f91ac7a200a0136783543"
        );
    }

    #[test]
    fn s3_list_objects() {
        let s = s3(
            "GET",
            "/",
            &[("max-keys", "2"), ("prefix", "J")],
            &[
                ("Host", "examplebucket.s3.amazonaws.com"),
                ("x-amz-content-sha256", EMPTY_SHA256),
                ("x-amz-date", "20130524T000000Z"),
            ],
            EMPTY_SHA256,
        );
        assert_eq!(
            s.signature,
            "34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7"
        );
    }

    #[test]
    fn s3_presigned_get() {
        let (query, sig) = presign(
            "GET",
            "/test.txt",
            "examplebucket.s3.amazonaws.com",
            &[],
            "us-east-1",
            "s3",
            s3_time(),
            86400,
            &s3_creds(),
        );
        assert_eq!(
            sig.signature,
            "aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"
        );
        assert_eq!(query.last().unwrap().0, "X-Amz-Signature");
    }

    #[test]
    fn encoding() {
        assert_eq!(uri_encode("a b+c/d", true), "a%20b%2Bc/d");
        assert_eq!(uri_encode("a/b", false), "a%2Fb");
        assert_eq!(
            canonical_query(&h(&[("b", "2"), ("a", "x y"), ("a", "1")])),
            "a=1&a=x%20y&b=2"
        );
    }
}
