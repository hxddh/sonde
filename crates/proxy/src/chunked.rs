//! `aws-chunked` bodies whose chunks are signed with the client's key. The
//! proxy cannot re-sign chunks it did not produce the seed signature for, so
//! it decodes them: the payload is forwarded whole, the trailing checksums
//! become headers.

/// The `x-amz-content-sha256` values whose body carries chunk signatures.
pub fn is_signed_streaming(content_sha256: &str) -> bool {
    content_sha256.starts_with("STREAMING-AWS4-HMAC-SHA256-PAYLOAD")
        || content_sha256.starts_with("STREAMING-AWS4-ECDSA-P256-SHA256-PAYLOAD")
}

/// A decoded body: the payload and the trailing headers (without the
/// trailer signature).
#[derive(Debug, PartialEq, Eq)]
pub struct Decoded {
    pub payload: Vec<u8>,
    pub trailers: Vec<(String, String)>,
}

fn line(buf: &[u8], at: usize) -> Result<(&[u8], usize), String> {
    let rel = buf[at..]
        .windows(2)
        .position(|w| w == b"\r\n")
        .ok_or("aws-chunked: a line without CRLF")?;
    Ok((&buf[at..at + rel], at + rel + 2))
}

/// Decodes an `aws-chunked` body.
pub fn decode(body: &[u8]) -> Result<Decoded, String> {
    let mut at = 0;
    let mut payload = Vec::with_capacity(body.len());
    loop {
        let (head, next) = line(body, at)?;
        let head = std::str::from_utf8(head).map_err(|_| "aws-chunked: chunk header not UTF-8")?;
        let size_hex = head.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| format!("aws-chunked: chunk size {size_hex:?}"))?;
        at = next;
        if size == 0 {
            break;
        }
        // a size near usize::MAX must not wrap round to a small one
        if size
            .checked_add(2)
            .and_then(|n| n.checked_add(at))
            .is_none_or(|end| end > body.len())
        {
            return Err("aws-chunked: a chunk runs past the body".into());
        }
        payload.extend_from_slice(&body[at..at + size]);
        at += size;
        if &body[at..at + 2] != b"\r\n" {
            return Err("aws-chunked: a chunk without its CRLF".into());
        }
        at += 2;
    }
    let mut trailers = Vec::new();
    while at < body.len() {
        let (l, next) = line(body, at)?;
        at = next;
        if l.is_empty() {
            break;
        }
        let l = String::from_utf8_lossy(l);
        if let Some((k, v)) = l.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            if k != "x-amz-trailer-signature" {
                trailers.push((k, v.trim().to_string()));
            }
        }
    }
    Ok(Decoded { payload, trailers })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_chunks_and_trailer() {
        let sig = "a".repeat(64);
        let body = format!(
            "5;chunk-signature={sig}\r\nhello\r\n6;chunk-signature={sig}\r\n world\r\n0;chunk-signature={sig}\r\nx-amz-checksum-crc32:AAAAAA==\r\nx-amz-trailer-signature:{sig}\r\n\r\n"
        );
        let d = decode(body.as_bytes()).unwrap();
        assert_eq!(d.payload, b"hello world");
        assert_eq!(
            d.trailers,
            vec![("x-amz-checksum-crc32".into(), "AAAAAA==".into())]
        );
    }

    #[test]
    fn without_trailer() {
        let d = decode(b"3;chunk-signature=x\r\nabc\r\n0;chunk-signature=y\r\n\r\n").unwrap();
        assert_eq!(d.payload, b"abc");
        assert!(d.trailers.is_empty());
    }

    #[test]
    fn a_chunk_size_near_the_largest_number_is_an_error_not_a_panic() {
        for size in ["ffffffffffffffff", "fffffffffffffffe", "7fffffffffffffff"] {
            let body = format!("{size};chunk-signature=x\r\nabc\r\n0\r\n\r\n");
            assert!(decode(body.as_bytes()).is_err(), "{size}");
        }
    }

    #[test]
    fn truncated() {
        assert!(decode(b"10;chunk-signature=x\r\nabc").is_err());
        assert!(is_signed_streaming(
            "STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER"
        ));
        assert!(!is_signed_streaming("STREAMING-UNSIGNED-PAYLOAD-TRAILER"));
    }
}
