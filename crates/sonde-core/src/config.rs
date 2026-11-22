//! `config.toml`: settings and endpoints.

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
        crate::phrase::say(&self.describe_text())
    }

    /// [`Self::describe`] as a phrase.
    pub fn describe_text(&self) -> crate::Phrase {
        use crate::phrase::ph;
        match self {
            CredentialSource::Default => ph!("cred.default"),
            CredentialSource::Profile { name } => ph!("cred.profile", name = name),
            CredentialSource::Env {
                access_key_id_var, ..
            } => ph!("cred.env", var = access_key_id_var),
            CredentialSource::Imds => ph!("cred.imds"),
            CredentialSource::WebIdentity { role_arn, .. } => {
                ph!("cred.webIdentity", role = role_arn)
            }
            CredentialSource::Process { .. } => ph!("cred.process"),
            CredentialSource::Static { .. } => ph!("cred.static"),
            CredentialSource::Anonymous => ph!("cred.anonymous"),
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
    /// Whether what is read from it leaves its region: `true`, read over
    /// the internet or from another region (egress, charged); `false`,
    /// read in its region or VPC (free). Unset: not known, and a run's
    /// bytes out are said as what they would cost, not counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress: Option<bool>,
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
            egress: None,
        }
    }

    /// An endpoint as it is added or changed: [`Self::validate_shape`], and
    /// its CA file exists.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_shape()?;
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

    /// Its id and URL, what `config.toml` is refused on load for. A CA file
    /// that has gone is not: it is said when the endpoint is used (its
    /// client is not made), so one endpoint's never stops every command.
    pub fn validate_shape(&self) -> Result<(), String> {
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
        Ok(())
    }
}

/// Storage and request prices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Prices {
    /// Storage per GiB-month: S3 Standard's, and of a class not in
    /// [`CLASS_PRICES`].
    pub default_per_gib_month: f64,
    #[serde(default = "usd")]
    pub currency: String,
    /// Requests that write or list (PUT, COPY, POST, LIST), per thousand.
    #[serde(default = "writes_per_1000")]
    pub per_1000_writes: f64,
    /// Requests that read (GET, HEAD and the rest), per thousand.
    #[serde(default = "reads_per_1000")]
    pub per_1000_reads: f64,
    /// Bytes sent out of the store to the internet (egress), per GiB.
    #[serde(default = "out_per_gib")]
    pub per_gib_out: f64,
}

fn out_per_gib() -> f64 {
    0.09
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

/// AWS's storage prices of the classes other than Standard, per
/// GiB-month (us-east-1). Standard, and a class not listed, is
/// [`Prices::default_per_gib_month`].
pub const CLASS_PRICES: &[(&str, f64)] = &[
    ("STANDARD_IA", 0.0125),
    ("ONEZONE_IA", 0.01),
    ("GLACIER_IR", 0.004),
    ("GLACIER", 0.0036),
    ("DEEP_ARCHIVE", 0.00099),
    ("EXPRESS_ONEZONE", 0.11),
];

impl Default for Prices {
    fn default() -> Self {
        Prices {
            default_per_gib_month: 0.023,
            currency: usd(),
            per_1000_writes: writes_per_1000(),
            per_1000_reads: reads_per_1000(),
            per_gib_out: out_per_gib(),
        }
    }
}

impl Prices {
    pub fn per_gib_month(&self, class: Option<&str>) -> f64 {
        class
            .and_then(|c| CLASS_PRICES.iter().find(|(n, _)| *n == c))
            .map_or(self.default_per_gib_month, |(_, p)| *p)
    }
}

/// What a run on one endpoint is priced at ([`crate::Sonde::prices_for`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RunPrices {
    pub per_1000_writes: f64,
    pub per_1000_reads: f64,
    /// Lists, when they cost other than a write (B2's class C).
    pub per_1000_lists: Option<f64>,
    /// Bytes out, per GiB, when they leave the region.
    pub per_gib_out: f64,
    /// Whether they do: the endpoint's `egress` (`None`: not known).
    pub egress: Option<bool>,
    /// Storage of what a run leaves (S3 Standard's class), per GiB-month.
    pub per_gib_month: f64,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Settings {
    pub prices: Prices,
    /// `system`, `light` or `dark`.
    pub theme: String,
    /// `auto`, `en` or `zh`.
    pub language: String,
}

/// Requests in flight per job of sonde's own (a scan, a listing): fixed.
pub const CONCURRENCY: usize = 16;

/// How old an incomplete upload is before it is stale: a week, as the
/// lifecycle rule sonde adds aborts them.
pub const STALE_UPLOAD_DAYS: u32 = 7;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            prices: Prices::default(),
            theme: "system".into(),
            language: "auto".into(),
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

    /// What `config.toml` is refused on load (and save) for: each
    /// endpoint's shape ([`EndpointConfig::validate_shape`]), not its files.
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for e in &self.endpoints {
            e.validate_shape()?;
            if !ids.insert(&e.id) {
                return Err(format!("endpoint id {:?} appears twice", e.id));
            }
        }
        Ok(())
    }

    /// Writes durably ([`reqlog::private::replace`]): mode 0600 on Unix, as
    /// the file may hold static keys.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        reqlog::private::replace(
            path,
            format!("# sonde configuration — docs/configuration.md\n{text}"),
        )
        .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn endpoint(&self, id: &str) -> Option<&EndpointConfig> {
        self.endpoints.iter().find(|e| e.id == id)
    }
}

/// Holds `config.toml.lock` beside `path` until dropped, so two processes
/// (the app and the CLI) do not read and write the file at once. `None`
/// where the lock cannot be taken: the save goes on without it.
pub(crate) fn lock(path: &Path) -> Option<std::fs::File> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("toml.lock"))
        .ok()?;
    f.lock().ok()?;
    Some(f)
}

#[cfg(test)]
mod tests {
    #[test]
    fn how_credentials_are_obtained_is_a_phrase_that_says_its_english() {
        use super::CredentialSource as C;
        for (c, said) in [
            (C::Default, "default chain"),
            (C::Profile { name: "dev".into() }, "profile dev"),
            (C::Imds, "instance role (IMDSv2)"),
            (C::Anonymous, "anonymous"),
        ] {
            assert_eq!(c.describe(), said);
            assert_eq!(crate::phrase::say(&c.describe_text()), said);
        }
    }

    use super::*;

    const SAMPLE: &str = r#"
[settings]
theme = "dark"

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
        assert_eq!(c.settings.theme, "dark");
        // Standard, and a class sonde does not know, at the file's price;
        // the others at AWS's
        assert_eq!(c.settings.prices.per_gib_month(Some("STANDARD")), 0.02);
        assert_eq!(c.settings.prices.per_gib_month(Some("NEW_CLASS")), 0.02);
        assert_eq!(c.settings.prices.per_gib_month(Some("GLACIER")), 0.0036);
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

    /// docs/configuration.md's `config.toml` is one sonde reads as it says.
    #[test]
    fn the_documented_example_reads() {
        let doc = include_str!("../../../docs/configuration.md");
        let example = doc
            .split("```toml\n")
            .nth(1)
            .and_then(|b| b.split("```").next())
            .unwrap();
        // (as read, not checked: its CA file is an example's)
        let c: Config = toml::from_str(example).unwrap();
        assert_eq!(c.endpoints.len(), 2);
        assert!(!c.endpoints[1].buckets.is_empty());
    }

    /// One endpoint's CA file gone stops that endpoint, not the file: it
    /// loads and saves, and the endpoint is refused as it is added.
    #[test]
    fn a_missing_ca_file_is_the_endpoints_not_the_files() {
        let text = "[[endpoint]]\nid='lab'\nname='Lab'\nurl='https://minio.lab:9000'\nca-file='/nonexistent/lab-ca.pem'\n";
        let c = Config::parse(text).unwrap();
        assert_eq!(
            c.endpoints[0].ca_file.as_deref(),
            Some(Path::new("/nonexistent/lab-ca.pem"))
        );
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p).unwrap(), c);
        let err = c.endpoints[0].validate().unwrap_err();
        assert!(
            err.contains("CA file /nonexistent/lab-ca.pem does not exist"),
            "{err}"
        );
    }

    /// 29.0 took sonde's own pace, its folders and its prices by class out of
    /// `config.toml`: a file that still has them reads, and is saved
    /// without them.
    #[test]
    fn the_keys_29_took_out_are_read_past_and_not_saved() {
        let old = "[settings]\nconcurrency = 4\nrate-limit = 5.0\nexports-dir = '/x'\ndownloads-dir = '/y'\n[settings.prices]\ndefault-per-gib-month = 0.02\n[settings.prices.by-class]\nGLACIER = 1.0\n";
        let c = Config::parse(old).unwrap();
        assert_eq!(c.settings.prices.per_gib_month(Some("GLACIER")), 0.0036);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        c.save(&p).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        for gone in [
            "concurrency",
            "rate-limit",
            "exports-dir",
            "downloads-dir",
            "by-class",
        ] {
            assert!(!text.contains(gone), "{gone} in:\n{text}");
        }
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
