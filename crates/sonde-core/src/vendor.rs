//! The object stores sonde knows by name.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Vendor {
    Aws,
    R2,
    Minio,
    CephRgw,
    AliyunOss,
    Tigris,
    BackblazeB2,
    Wasabi,
    GcsXml,
    #[default]
    Other,
}

impl Vendor {
    /// The vendor's own list price per 1,000 writes (and listings) and per
    /// 1,000 reads, in USD, where it differs from S3 Standard's: R2's, B2's,
    /// Wasabi's and Tigris's; nothing per request on a store of one's own.
    /// `None`: S3 Standard's (AWS, GCS's XML API, and the rest).
    pub fn request_prices(self) -> Option<(f64, f64)> {
        match self {
            Vendor::R2 => Some((0.0045, 0.00036)),
            Vendor::BackblazeB2 => Some((0.0, 0.0004)),
            Vendor::Wasabi => Some((0.0, 0.0)),
            Vendor::Tigris => Some((0.005, 0.0005)),
            Vendor::Minio | Vendor::CephRgw => Some((0.0, 0.0)),
            _ => None,
        }
    }
}

pub const ALL: [Vendor; 10] = [
    Vendor::Aws,
    Vendor::R2,
    Vendor::Minio,
    Vendor::CephRgw,
    Vendor::AliyunOss,
    Vendor::Tigris,
    Vendor::BackblazeB2,
    Vendor::Wasabi,
    Vendor::GcsXml,
    Vendor::Other,
];

impl Vendor {
    pub fn label(self) -> &'static str {
        match self {
            Vendor::Aws => "AWS S3",
            Vendor::R2 => "Cloudflare R2",
            Vendor::Minio => "MinIO",
            Vendor::CephRgw => "Ceph RGW",
            Vendor::AliyunOss => "Alibaba Cloud OSS",
            Vendor::Tigris => "Tigris",
            Vendor::BackblazeB2 => "Backblaze B2",
            Vendor::Wasabi => "Wasabi",
            Vendor::GcsXml => "GCS (XML API)",
            Vendor::Other => "Other",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Vendor::Aws => "aws",
            Vendor::R2 => "r2",
            Vendor::Minio => "minio",
            Vendor::CephRgw => "ceph-rgw",
            Vendor::AliyunOss => "aliyun-oss",
            Vendor::Tigris => "tigris",
            Vendor::BackblazeB2 => "backblaze-b2",
            Vendor::Wasabi => "wasabi",
            Vendor::GcsXml => "gcs-xml",
            Vendor::Other => "other",
        }
    }

    pub fn from_id(s: &str) -> Option<Vendor> {
        ALL.into_iter().find(|v| v.id() == s)
    }

    /// Whether the vendor answers STS `GetCallerIdentity`.
    pub fn has_sts_identity(self) -> bool {
        matches!(self, Vendor::Aws)
    }

    /// The STS endpoint for a region.
    pub fn sts_endpoint(self, region: &str) -> Option<String> {
        match self {
            Vendor::Aws if region.starts_with("cn-") => {
                Some(format!("https://sts.{region}.amazonaws.com.cn"))
            }
            Vendor::Aws => Some(format!(
                "https://sts.{}.amazonaws.com",
                if region.is_empty() {
                    "us-east-1"
                } else {
                    region
                }
            )),
            _ => None,
        }
    }

    /// A URL template for the vendor (`{region}`, `{account}` placeholders).
    pub fn url_template(self) -> &'static str {
        match self {
            Vendor::Aws => "https://s3.{region}.amazonaws.com",
            Vendor::R2 => "https://{account}.r2.cloudflarestorage.com",
            Vendor::Minio => "http://127.0.0.1:9000",
            Vendor::CephRgw => "http://127.0.0.1:7480",
            Vendor::AliyunOss => "https://oss-{region}.aliyuncs.com",
            Vendor::Tigris => "https://fly.storage.tigris.dev",
            Vendor::BackblazeB2 => "https://s3.{region}.backblazeb2.com",
            Vendor::Wasabi => "https://s3.{region}.wasabisys.com",
            Vendor::GcsXml => "https://storage.googleapis.com",
            Vendor::Other => "https://",
        }
    }

    /// Guesses the vendor from an endpoint URL.
    pub fn guess(url: &str) -> Vendor {
        let u = url.to_ascii_lowercase();
        if u.contains("amazonaws.com") {
            Vendor::Aws
        } else if u.contains("r2.cloudflarestorage.com") {
            Vendor::R2
        } else if u.contains("aliyuncs.com") {
            Vendor::AliyunOss
        } else if u.contains("tigris.dev") {
            Vendor::Tigris
        } else if u.contains("backblazeb2.com") {
            Vendor::BackblazeB2
        } else if u.contains("wasabisys.com") {
            Vendor::Wasabi
        } else if u.contains("storage.googleapis.com") {
            Vendor::GcsXml
        } else if u.contains(":9000") || u.contains("minio") {
            Vendor::Minio
        } else if u.contains(":7480") || u.contains("rgw") || u.contains("ceph") {
            Vendor::CephRgw
        } else {
            Vendor::Other
        }
    }

    /// Path-style by default for vendors that expect it.
    pub fn default_path_style(self) -> bool {
        matches!(
            self,
            Vendor::Minio | Vendor::CephRgw | Vendor::Other | Vendor::R2
        )
    }

    /// The default region name sigining uses.
    pub fn default_region(self) -> &'static str {
        match self {
            Vendor::R2 | Vendor::GcsXml => "auto",
            _ => "us-east-1",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for v in ALL {
            assert_eq!(Vendor::from_id(v.id()), Some(v));
        }
        assert_eq!(
            Vendor::guess("https://s3.eu-west-1.amazonaws.com"),
            Vendor::Aws
        );
        assert_eq!(Vendor::guess("http://localhost:9000"), Vendor::Minio);
        assert_eq!(
            Vendor::guess("https://abc.r2.cloudflarestorage.com"),
            Vendor::R2
        );
        assert!(Vendor::Aws.has_sts_identity() && !Vendor::Minio.has_sts_identity());
        assert_eq!(
            Vendor::Aws.sts_endpoint("cn-north-1").unwrap(),
            "https://sts.cn-north-1.amazonaws.com.cn"
        );
    }
}
