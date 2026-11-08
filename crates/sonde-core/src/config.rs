//! `config.toml`: settings and endpoints.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::vendor::Vendor;

/// How an endpoint gets its credentials. Secrets are never written here
/// except for `static`, which exists for local test servers (the file is
/// written with mode 0600 then).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case",
    tag = "kind"
)]
pub enum CredentialSource {
    /// The AWS default chain: environment, web identity (IRSA), the
    /// profile in `AWS_PROFILE` (or `default`), container, IMDSv2.
    #[default]
    Default,
    /// A named profile of `~/.aws/config` / `~/.aws/credentials`
    /// (static keys, `credential_process`, SSO, `role_arn`, web identity).
    Profile { name: String },
    /// Environment variables with these names.
    Env {
        access_key_id_var: String,
        secret_access_key_var: String,
        session_token_var: Option<String>,
    },
    /// The instance's role through IMDSv2.
    Imds,
    /// Web identity: an OIDC token file and a role (IRSA uses the env vars).
    WebIdentity {
        role_arn: String,
        token_file: PathBuf,
        session_name: Option<String>,
    },
    /// A command printing credentials as `credential_process` does.
    Process { command: String },
    /// Keys in this file. For local servers only.
    Static {
        access_key_id: String,
        secret_access_key: String,
    },
    /// Unsigned requests (public buckets).
    Anonymous,
}

impl CredentialSource {
    pub fn describe(&self) -> String {
        match self {
            CredentialSource::Default => "default chain".into(),
            CredentialSource::Profile { name } => format!("profile {name}"),
            CredentialSource::Env {
                access_key_id_var, ..
            } => format!("env ${access_key_id_var}"),
            CredentialSource::Imds => "instance role (IMDSv2)".into(),
            CredentialSource::WebIdentity { role_arn, .. } => format!("web identity {role_arn}"),
            CredentialSource::Process { .. } => "credential_process".into(),
            CredentialSource::Static { .. } => "static keys (config file)".into(),
            CredentialSource::Anonymous => "anonymous".into(),
        }
    }
}

/// One object store endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct EndpointConfig {
    /// Stable id: what the UI and the request log name.
    pub id: String,
    /// Display name.
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub path_style: bool,
    /// A PEM file of extra trusted CA certificates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<PathBuf>,
    #[serde(default)]
    pub vendor: Vendor,
    #[serde(default)]
    pub credentials: CredentialSource,
    /// Buckets to show when the credentials may not list buckets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buckets: Vec<String>,
    /// Hash payloads (`false`: `UNSIGNED-PAYLOAD`).
    #[serde(default = "yes")]
    pub sign_payload: bool,
}

fn yes() -> bool {
    true
}

impl EndpointConfig {
    pub fn new(id: &str, name: &str, url: &str, region: &str, vendor: Vendor) -> Self {
        EndpointConfig {
            id: id.into(),
            name: name.into(),
            url: url.into(),
            region: region.into(),
            path_style: vendor.default_path_style(),
            ca_file: None,
            vendor,
            credentials: CredentialSource::Default,
            buckets: Vec::new(),
            sign_payload: true,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        {
            return Err(format!(
                "endpoint id {:?}: letters, digits, '-', '_' and '.' only",
                self.id
            ));
        }
        let u = url::Url::parse(&self.url)
            .map_err(|e| format!("endpoint {}: URL {:?}: {e}", self.id, self.url))?;
        if !matches!(u.scheme(), "http" | "https") {
            return Err(format!("endpoint {}: URL must be http or https", self.id));
        }
        if let Some(ca) = &self.ca_file
            && !ca.exists()
        {
            return Err(format!(
                "endpoint {}: CA file {} does not exist",
                self.id,
                ca.display()
            ));
        }
        Ok(())
    }
}

/// Storage prices, per GiB-month, by storage class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Prices {
    /// Used for classes not listed.
    pub default_per_gib_month: f64,
    #[serde(default)]
    pub by_class: BTreeMap<String, f64>,
    #[serde(default = "usd")]
    pub currency: String,
    /// Requests that write or list (PUT, COPY, POST, LIST), per thousand.
    #[serde(default = "writes_per_1000")]
    pub per_1000_writes: f64,
    /// Requests that read (GET, HEAD and the rest), per thousand.
    #[serde(default = "reads_per_1000")]
    pub per_1000_reads: f64,
}

fn writes_per_1000() -> f64 {
    0.005
}

fn reads_per_1000() -> f64 {
    0.0004
}

fn usd() -> String {
    "USD".into()
}

impl Default for Prices {
    fn default() -> Self {
        let mut by_class = BTreeMap::new();
        by_class.insert("STANDARD".into(), 0.023);
        by_class.insert("INTELLIGENT_TIERING".into(), 0.023);
        by_class.insert("STANDARD_IA".into(), 0.0125);
        by_class.insert("ONEZONE_IA".into(), 0.01);
        by_class.insert("GLACIER_IR".into(), 0.004);
        by_class.insert("GLACIER".into(), 0.0036);
        by_class.insert("DEEP_ARCHIVE".into(), 0.00099);
        by_class.insert("EXPRESS_ONEZONE".into(), 0.11);
        Prices {
            default_per_gib_month: 0.023,
            by_class,
            currency: usd(),
            per_1000_writes: writes_per_1000(),
            per_1000_reads: reads_per_1000(),
        }
    }
}

impl Prices {
    pub fn per_gib_month(&self, class: Option<&str>) -> f64 {
        class
            .and_then(|c| self.by_class.get(c))
            .copied()
            .unwrap_or(self.default_per_gib_month)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Settings {
    /// Destructive operations are refused while on (the default).
    pub read_only: bool,
    /// Requests in flight per job.
    pub concurrency: usize,
    /// Requests per second per job (`None`: unlimited).
    pub rate_limit: Option<f64>,
    pub prices: Prices,
    /// Records kept in the request log.
    pub reqlog_capacity: usize,
    /// Also keep the request log in SQLite.
    pub reqlog_persist: bool,
    /// `system`, `light` or `dark`.
    pub theme: String,
    /// Stronger borders and text: the theme's high-contrast variant.
    pub high_contrast: bool,
    /// `auto`, `en` or `zh`.
    pub language: String,
    /// Multipart uploads older than this are flagged.
    pub stale_upload_days: u32,
    pub exports_dir: Option<PathBuf>,
    pub downloads_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            read_only: true,
            concurrency: 16,
            rate_limit: None,
            prices: Prices::default(),
            reqlog_capacity: reqlog::DEFAULT_CAPACITY,
            reqlog_persist: false,
            theme: "system".into(),
            high_contrast: false,
            language: "auto".into(),
            stale_upload_days: 7,
            exports_dir: None,
            downloads_dir: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case", default)]
pub struct Config {
    pub settings: Settings,
    #[serde(rename = "endpoint")]
    pub endpoints: Vec<EndpointConfig>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let c: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        c.validate()?;
        Ok(c)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for e in &self.endpoints {
            e.validate()?;
            if !ids.insert(&e.id) {
                return Err(format!("endpoint id {:?} appears twice", e.id));
            }
        }
        if self.settings.concurrency == 0 || self.settings.concurrency > 1024 {
            return Err("settings.concurrency: 1 to 1024".into());
        }
        Ok(())
    }

    /// Writes atomically; 0600 on Unix (the file may hold static keys).
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(
            &tmp,
            format!("# sonde configuration — docs/configuration.md\n{text}"),
        )
        .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn endpoint(&self, id: &str) -> Option<&EndpointConfig> {
        self.endpoints.iter().find(|e| e.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[settings]
read-only = true
concurrency = 8
rate-limit = 50.0

[settings.prices]
default-per-gib-month = 0.02

[[endpoint]]
id = "local-minio"
name = "MinIO (docker)"
url = "http://127.0.0.1:9000"
region = "us-east-1"
path-style = true
vendor = "minio"
credentials = { kind = "static", access-key-id = "minioadmin", secret-access-key = "minioadmin" }

[[endpoint]]
id = "prod"
name = "Production"
url = "https://s3.eu-west-1.amazonaws.com"
region = "eu-west-1"
vendor = "aws"
credentials = { kind = "profile", name = "prod" }
"#;

    #[test]
    fn parse_and_round_trip() {
        let c = Config::parse(SAMPLE).unwrap();
        assert_eq!(c.endpoints.len(), 2);
        assert!(c.settings.read_only);
        assert_eq!(c.settings.concurrency, 8);
        assert_eq!(
            c.settings.prices.per_gib_month(Some("STANDARD")),
            0.02,
            "listed classes replaced by the file's empty map"
        );
        assert_eq!(
            c.endpoints[1].credentials,
            CredentialSource::Profile {
                name: "prod".into()
            }
        );
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c/config.toml");
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p).unwrap(), c);
    }

    #[test]
    fn defaults_are_safe() {
        let c = Config::default();
        assert!(c.settings.read_only);
        assert_eq!(c.settings.reqlog_capacity, 5000);
        assert_eq!(c.settings.stale_upload_days, 7);
    }

    #[test]
    fn rejects_bad() {
        assert!(Config::parse("[[endpoint]]\nid='a b'\nname='x'\nurl='http://x'").is_err());
        assert!(Config::parse("[[endpoint]]\nid='a'\nname='x'\nurl='ftp://x'").is_err());
        assert!(Config::parse("[[endpoint]]\nid='a'\nname='x'\nurl='http://x'\n[[endpoint]]\nid='a'\nname='y'\nurl='http://y'").is_err());
        assert!(
            Config::load(Path::new("/nonexistent/config.toml"))
                .unwrap()
                .endpoints
                .is_empty()
        );
    }
}
