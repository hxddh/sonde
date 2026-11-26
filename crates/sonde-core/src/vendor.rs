//! The object stores sonde knows by name.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Vendor {
    Aws,
    R2,
    Minio,
    Rustfs,
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
            Vendor::Minio | Vendor::Rustfs | Vendor::CephRgw => Some((0.0, 0.0, None)),
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
            | Vendor::Rustfs
            | Vendor::CephRgw => Some(0.0),
            _ => None,
        }
    }
}

pub const ALL: [Vendor; 12] = [
    Vendor::Aws,
    Vendor::R2,
    Vendor::Minio,
    Vendor::Rustfs,
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
            Vendor::Rustfs => "RustFS",
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
            Vendor::Rustfs => "rustfs",
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
            Vendor::Minio | Vendor::Rustfs => "http://127.0.0.1:9000",
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

    /// Guesses the vendor from an endpoint URL: by a name in it. A port is
    /// no evidence (MinIO, RustFS, SeaweedFS and many a proxy listen on
    /// 9000): `http://localhost:9000` is `Other` until an answer says more
    /// ([`Vendor::from_answer`]).
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
        } else if u.contains("minio") {
            Vendor::Minio
        } else if u.contains("rustfs") {
            Vendor::Rustfs
        } else if u.contains("rgw") || u.contains("ceph") {
            Vendor::CephRgw
        } else {
            Vendor::Other
        }
    }

    /// The vendor a store's own answers name: its `Server` header, else the
    /// shape of an error body (`bodies`: any XML error documents it sent).
    /// `None` when nothing in them is a store's own mark. RustFS is known
    /// by its `Server` header alone: its error body
    /// (`<Error><Code/><Message/></Error>`) is the plain one any store may
    /// send.
    pub fn from_answer<'a>(
        server: Option<&str>,
        bodies: impl IntoIterator<Item = &'a str>,
    ) -> Option<Vendor> {
        let named = server.map(str::to_ascii_lowercase).and_then(|s| {
            // `MinIO`, `RustFS/1.0.0`, `AmazonS3`, `AliyunOSS`, `tencent-cos`,
            // `WasabiS3/7.10 (head2)`, `Tigris OS`
            [
                ("rustfs", Vendor::Rustfs),
                ("minio", Vendor::Minio),
                ("amazons3", Vendor::Aws),
                ("aliyunoss", Vendor::AliyunOss),
                ("tencent-cos", Vendor::TencentCos),
                ("wasabis3", Vendor::Wasabi),
                ("tigris", Vendor::Tigris),
            ]
            .into_iter()
            .find_map(|(name, v)| s.starts_with(name).then_some(v))
        });
        named.or_else(|| {
            bodies.into_iter().find_map(|b| {
                if !b.contains("<Error>") {
                    None
                } else if b.contains(MINIO_HOST_ID) {
                    Some(Vendor::Minio)
                } else if b.contains("<RequestId>tx0000") && b.contains("<HostId>") {
                    // RGW's transaction ids: tx000000000000000000500-006aedee7c-100e-default
                    Some(Vendor::CephRgw)
                } else {
                    None
                }
            })
        })
    }

    /// Path-style by default for vendors that expect it.
    pub fn default_path_style(self) -> bool {
        matches!(
            self,
            Vendor::Minio | Vendor::Rustfs | Vendor::CephRgw | Vendor::Other | Vendor::R2
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

/// The `HostId` in MinIO's error documents: the same on every server.
const MINIO_HOST_ID: &str =
    "<HostId>dd9025bab4ad464b049177c95eb6ebf374d3b3fd1af9251148b658df7ac2e3e8</HostId>";

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
        assert_eq!(Vendor::guess("http://minio.lab:9000"), Vendor::Minio);
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

    #[test]
    fn a_port_is_no_evidence_of_a_vendor() {
        // RustFS, SeaweedFS and many a proxy listen there as MinIO does
        for url in [
            "http://localhost:9000",
            "http://127.0.0.1:9000/",
            "https://s3.internal.example:9000",
            "http://10.0.0.7:7480",
        ] {
            assert_eq!(Vendor::guess(url), Vendor::Other, "{url}");
        }
        assert_eq!(Vendor::guess("http://rustfs:9000"), Vendor::Rustfs);
        assert_eq!(Vendor::guess("http://rgw.lab:7480"), Vendor::CephRgw);
        // a store of one's own all the same: path-style, nothing per request
        assert!(Vendor::Rustfs.default_path_style());
        assert_eq!(Vendor::Rustfs.request_prices(), Some((0.0, 0.0, None)));
        assert_eq!(Vendor::from_id("rustfs"), Some(Vendor::Rustfs));
    }

    #[test]
    fn a_store_is_known_by_what_it_answers() {
        let none: [&str; 0] = [];
        assert_eq!(
            Vendor::from_answer(Some("RustFS/1.0.0-alpha.90"), none),
            Some(Vendor::Rustfs)
        );
        assert_eq!(
            Vendor::from_answer(Some("rustfs"), none),
            Some(Vendor::Rustfs)
        );
        assert_eq!(
            Vendor::from_answer(Some("MinIO"), none),
            Some(Vendor::Minio)
        );
        assert_eq!(
            Vendor::from_answer(Some("AmazonS3"), none),
            Some(Vendor::Aws)
        );
        // a proxy in front says nothing of the store behind it
        assert_eq!(Vendor::from_answer(Some("nginx/1.27.0"), none), None);
        assert_eq!(Vendor::from_answer(None, none), None);
        // an error body: MinIO's HostId, RGW's transaction id
        let minio = "<Error><Code>NoSuchKey</Code><Message>The specified key does not exist.</Message><Key>k</Key><BucketName>b</BucketName><Resource>/b/k</Resource><RequestId>18A1</RequestId><HostId>dd9025bab4ad464b049177c95eb6ebf374d3b3fd1af9251148b658df7ac2e3e8</HostId></Error>";
        assert_eq!(Vendor::from_answer(None, [minio]), Some(Vendor::Minio));
        let rgw = "<Error><Code>AccessDenied</Code><RequestId>tx0000000000000000005d0-006aedee86-100e-default</RequestId><HostId>100e-default-default</HostId></Error>";
        assert_eq!(Vendor::from_answer(None, ["", rgw]), Some(Vendor::CephRgw));
        // the plain error document (RustFS's too) is nobody's mark
        let plain = "<Error><Code>BadDigest</Code><Message>The Content-Md5 you specified did not match what we received.</Message></Error>";
        assert_eq!(Vendor::from_answer(None, [plain]), None);
        // and the header, where there is one, is the store's own word
        assert_eq!(
            Vendor::from_answer(Some("RustFS"), [minio]),
            Some(Vendor::Rustfs)
        );
    }
}
