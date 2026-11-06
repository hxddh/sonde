//! What an ETag says about an object.

use serde::{Deserialize, Serialize};

/// The ETag's form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum EtagKind {
    /// 32 hex digits: the MD5 of a single-part upload's body, unless the
    /// object is encrypted with SSE-KMS / SSE-C (then it is opaque).
    Md5 { md5: String },
    /// `<32 hex>-N`: a multipart upload completed from N parts.
    Multipart { digest: String, parts: u32 },
    /// Anything else: a vendor's own format.
    Opaque,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EtagInfo {
    /// The ETag without quotes.
    pub value: String,
    /// `W/` prefixed.
    pub weak: bool,
    pub kind: EtagKind,
}

fn is_hex32(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parses an ETag header value (`"9b2c…-3"`, `W/"…"`).
pub fn parse(raw: &str) -> EtagInfo {
    let mut s = raw.trim();
    let weak = s.starts_with("W/");
    if weak {
        s = &s[2..];
    }
    let value = s.trim_matches('"').to_string();
    let kind = match value.rsplit_once('-') {
        Some((d, n)) if is_hex32(d) && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => {
            match n.parse::<u32>() {
                Ok(parts) if parts >= 1 => EtagKind::Multipart {
                    digest: d.to_ascii_lowercase(),
                    parts,
                },
                _ => EtagKind::Opaque,
            }
        }
        _ if is_hex32(&value) => EtagKind::Md5 {
            md5: value.to_ascii_lowercase(),
        },
        _ => EtagKind::Opaque,
    };
    EtagInfo { value, weak, kind }
}

impl EtagInfo {
    pub fn parts(&self) -> Option<u32> {
        match self.kind {
            EtagKind::Multipart { parts, .. } => Some(parts),
            _ => None,
        }
    }
}

/// The multipart ETag S3 computes: MD5 of the parts' binary MD5s, `-N`.
pub fn multipart_etag(part_md5s_hex: &[String]) -> Option<String> {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    for p in part_md5s_hex {
        h.update(hex::decode(p).ok()?);
    }
    Some(format!(
        "{}-{}",
        hex::encode(h.finalize()),
        part_md5s_hex.len()
    ))
}

/// Common part sizes (MiB) consistent with `size` bytes in `parts` parts,
/// the uniform-part guess tools like aws-cli (8 MiB) and others make.
pub fn guess_part_sizes(size: u64, parts: u32) -> Vec<u64> {
    const MIB: u64 = 1024 * 1024;
    let candidates = [5, 8, 10, 15, 16, 25, 32, 50, 64, 100, 128, 256, 512];
    candidates
        .iter()
        .map(|m| m * MIB)
        .filter(|ps| parts > 0 && size.div_ceil(*ps) == parts as u64)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        let e = parse("\"9b2cf535f27731c974343645a3985328\"");
        assert_eq!(
            e.kind,
            EtagKind::Md5 {
                md5: "9b2cf535f27731c974343645a3985328".into()
            }
        );
        let e = parse("\"9B2CF535F27731C974343645A3985328-12\"");
        assert_eq!(e.parts(), Some(12));
        assert_eq!(parse("W/\"abc\"").kind, EtagKind::Opaque);
        assert!(parse("W/\"abc\"").weak);
        assert_eq!(
            parse("\"9b2cf535f27731c974343645a3985328-\"").kind,
            EtagKind::Opaque
        );
        assert_eq!(
            parse("\"9b2cf535f27731c974343645a3985328-0\"").kind,
            EtagKind::Opaque
        );
        assert_eq!(parse("CNzz8ZC8gIkDEAE=").kind, EtagKind::Opaque);
    }

    #[test]
    fn multipart_digest() {
        use md5::{Digest, Md5};
        let a = hex::encode(Md5::digest(b"a"));
        let b = hex::encode(Md5::digest(b"b"));
        let e = multipart_etag(&[a.clone(), b.clone()]).unwrap();
        assert!(e.ends_with("-2"));
        let mut cat = hex::decode(&a).unwrap();
        cat.extend(hex::decode(&b).unwrap());
        assert_eq!(&e[..32], hex::encode(Md5::digest(&cat)));
    }

    #[test]
    fn part_size_guess() {
        let g = guess_part_sizes(100 * 1024 * 1024, 13);
        assert!(g.contains(&(8 * 1024 * 1024)));
        assert!(guess_part_sizes(10, 3).is_empty());
    }
}
