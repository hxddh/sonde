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
    TencentCos,
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
    /// Writes, reads and (when other than a write) lists, per thousand.
    pub fn request_prices(self) -> Option<(f64, f64, Option<f64>)> {
        match self {
            Vendor::R2 => Some((0.0045, 0.00036, None)),
            // B2: uploads free (class A), reads class B, lists class C
            Vendor::BackblazeB2 => Some((0.0, 0.0004, Some(0.004))),
            Vendor::Wasabi => Some((0.0, 0.0, None)),
            Vendor::Tigris => Some((0.005, 0.0005, None)),
            Vendor::Minio | Vendor::CephRgw => Some((0.0, 0.0, None)),
            _ => None,
        }
    }

    /// The vendor's list price per GiB sent out of the store to the
    /// internet (egress), in USD, where it differs from S3's: the first
    /// tier, past any monthly allowance. Read from the vendors' pricing
    /// pages on 2026-11-18:
    /// - AWS: $0.09/GB, first 10 TB a month, past 100 GB free a month
    ///   across AWS (the AWS price list, us-east-1);
    /// - GCS: $0.12/GiB to worldwide destinations but Asia and Australia,
    ///   0 to 10 TiB (Premium tier);
    /// - Alibaba Cloud OSS: $0.076 to $0.080/GB past 100 GB free in its
    ///   regions outside the Chinese mainland ($0.118 in Hong Kong): $0.08;
    /// - R2, Tigris, Wasabi: none; B2: none up to three times what is
    ///   stored, $0.01/GB past it (none here); a store of one's own: none.
    ///
    /// Tencent COS publishes no figure sonde could read (its pages answer
    /// a browser's script): S3's, until set.
    /// `None`: S3's (AWS, COS and the rest).
    pub fn transfer_price(self) -> Option<f64> {
        match self {
            Vendor::GcsXml => Some(0.12),
            Vendor::AliyunOss => Some(0.08),
            Vendor::R2
            | Vendor::Tigris
            | Vendor::Wasabi
            | Vendor::BackblazeB2
            | Vendor::Minio
            | Vendor::CephRgw => Some(0.0),
            _ => None,
        }
    }
}

pub const ALL: [Vendor; 11] = [
    Vendor::Aws,
    Vendor::R2,
    Vendor::Minio,
    Vendor::CephRgw,
    Vendor::AliyunOss,
    Vendor::TencentCos,
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
            Vendor::TencentCos => "Tencent COS",
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
            Vendor::TencentCos => "tencent-cos",
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
            Vendor::TencentCos => "https://cos.{region}.myqcloud.com",
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
        } else if u.contains("myqcloud.com") {
            // its buckets are reached by name in the host
            Vendor::TencentCos
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
        // COS buckets are reached by name in the host
        let cos = Vendor::guess("https://cos.ap-guangzhou.myqcloud.com");
        assert_eq!(cos, Vendor::TencentCos);
        assert!(!cos.default_path_style());
        assert!(Vendor::Aws.has_sts_identity() && !Vendor::Minio.has_sts_identity());
        assert_eq!(
            Vendor::Aws.sts_endpoint("cn-north-1").unwrap(),
            "https://sts.cn-north-1.amazonaws.com.cn"
        );
    }
}
