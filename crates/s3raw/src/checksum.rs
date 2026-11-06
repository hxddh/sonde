//! Checksums S3 knows (`x-amz-checksum-*`) and what a stored one means.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use sha1::Digest as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Algorithm {
    Crc32,
    Crc32c,
    Crc64nvme,
    Sha1,
    Sha256,
}

pub const ALL: [Algorithm; 5] = [
    Algorithm::Crc32,
    Algorithm::Crc32c,
    Algorithm::Crc64nvme,
    Algorithm::Sha1,
    Algorithm::Sha256,
];

impl Algorithm {
    /// `CRC32`, as in `x-amz-checksum-algorithm`.
    pub fn name(self) -> &'static str {
        match self {
            Algorithm::Crc32 => "CRC32",
            Algorithm::Crc32c => "CRC32C",
            Algorithm::Crc64nvme => "CRC64NVME",
            Algorithm::Sha1 => "SHA1",
            Algorithm::Sha256 => "SHA256",
        }
    }
    /// `x-amz-checksum-crc32`.
    pub fn header(self) -> String {
        format!("x-amz-checksum-{}", self.name().to_ascii_lowercase())
    }
    /// `ChecksumCRC32`, the XML element.
    pub fn element(self) -> String {
        format!("Checksum{}", self.name())
    }
    pub fn from_name(s: &str) -> Option<Self> {
        ALL.into_iter().find(|a| a.name().eq_ignore_ascii_case(s))
    }
    /// Whether S3 can store a full-object checksum of a multipart upload
    /// (CRCs only; SHA checksums of multipart uploads are composite).
    pub fn supports_full_object(self) -> bool {
        matches!(
            self,
            Algorithm::Crc32 | Algorithm::Crc32c | Algorithm::Crc64nvme
        )
    }

    pub fn raw(self, data: &[u8]) -> Vec<u8> {
        match self {
            Algorithm::Crc32 => crc32fast::hash(data).to_be_bytes().to_vec(),
            Algorithm::Crc32c => crc32c::crc32c(data).to_be_bytes().to_vec(),
            Algorithm::Crc64nvme => {
                let mut d = crc64fast_nvme::Digest::new();
                d.write(data);
                d.sum64().to_be_bytes().to_vec()
            }
            Algorithm::Sha1 => sha1::Sha1::digest(data).to_vec(),
            Algorithm::Sha256 => sha2::Sha256::digest(data).to_vec(),
        }
    }

    /// Base64 of the checksum, as headers carry it.
    pub fn compute(self, data: &[u8]) -> String {
        B64.encode(self.raw(data))
    }

    /// A composite checksum of parts: the checksum of the concatenated raw
    /// part checksums, `-N`.
    pub fn composite(self, parts: &[&[u8]]) -> String {
        let mut cat = Vec::new();
        for p in parts {
            cat.extend(self.raw(p));
        }
        format!("{}-{}", self.compute(&cat), parts.len())
    }
}

/// Full-object or composite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChecksumType {
    FullObject,
    Composite,
}

/// A checksum as stored on an object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredChecksum {
    pub algorithm: Algorithm,
    /// Base64, without the `-N` suffix.
    pub value: String,
    pub kind: ChecksumType,
    /// The number of parts of a composite checksum.
    pub parts: Option<u32>,
    /// Where `kind` came from: the `x-amz-checksum-type` header, or the
    /// value's `-N` suffix when the header is missing.
    pub kind_from_header: bool,
}

/// The checksums in a HEAD / GET response's headers (with
/// `x-amz-checksum-mode: ENABLED`).
pub fn from_headers(headers: &[(String, String)]) -> Vec<StoredChecksum> {
    let get = |n: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n))
            .map(|(_, v)| v.clone())
    };
    let type_header = get("x-amz-checksum-type");
    let mut out = Vec::new();
    for a in ALL {
        if let Some(v) = get(&a.header()) {
            let (value, parts) = match v.rsplit_once('-') {
                Some((val, n)) if n.bytes().all(|b| b.is_ascii_digit()) && !n.is_empty() => {
                    (val.to_string(), n.parse().ok())
                }
                _ => (v.clone(), None),
            };
            let (kind, from_header) = match type_header.as_deref() {
                Some("COMPOSITE") => (ChecksumType::Composite, true),
                Some("FULL_OBJECT") => (ChecksumType::FullObject, true),
                _ => (
                    if parts.is_some() {
                        ChecksumType::Composite
                    } else {
                        ChecksumType::FullObject
                    },
                    false,
                ),
            };
            out.push(StoredChecksum {
                algorithm: a,
                value,
                kind,
                parts,
                kind_from_header: from_header,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        // "hello world"
        let d = b"hello world";
        assert_eq!(Algorithm::Crc32.compute(d), "DUoRhQ==");
        assert_eq!(Algorithm::Crc32c.compute(d), "yZRlqg==");
        assert_eq!(Algorithm::Sha1.compute(d), "Kq5sNclPz7QV2+lfQIuc6R7oRu0=");
        assert_eq!(
            Algorithm::Sha256.compute(d),
            "uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek="
        );
        // CRC-64/NVME of "123456789" is 0xAE8B14860A799888
        assert_eq!(
            Algorithm::Crc64nvme.raw(b"123456789"),
            0xAE8B14860A799888u64.to_be_bytes().to_vec()
        );
    }

    #[test]
    fn names() {
        assert_eq!(Algorithm::Crc64nvme.header(), "x-amz-checksum-crc64nvme");
        assert_eq!(Algorithm::Crc32c.element(), "ChecksumCRC32C");
        assert_eq!(Algorithm::from_name("sha256"), Some(Algorithm::Sha256));
    }

    #[test]
    fn stored() {
        let h = vec![("x-amz-checksum-crc32".to_string(), "AAAAAA==-3".to_string())];
        let s = from_headers(&h);
        assert_eq!(s[0].kind, ChecksumType::Composite);
        assert_eq!(s[0].parts, Some(3));
        assert!(!s[0].kind_from_header);
        let h = vec![
            (
                "x-amz-checksum-crc64nvme".to_string(),
                "AAAAAAAAAAA=".to_string(),
            ),
            ("x-amz-checksum-type".to_string(), "FULL_OBJECT".to_string()),
        ];
        let s = from_headers(&h);
        assert_eq!(s[0].kind, ChecksumType::FullObject);
        assert!(s[0].kind_from_header);
        assert!(Algorithm::Sha1.composite(&[b"a", b"b"]).ends_with("-2"));
    }
}
