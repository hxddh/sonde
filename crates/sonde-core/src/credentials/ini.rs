//! The AWS shared config and credentials files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Sections of an INI file: `name → (key → value)`. Sub-sections (the
/// indented `s3 =` blocks of `services`) are flattened as `s3.endpoint_url`.
pub type Sections = BTreeMap<String, BTreeMap<String, String>>;

pub fn parse(text: &str) -> Sections {
    let mut out: Sections = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut sub: Option<String> = None;
    for raw in text.lines() {
        let indented = raw.starts_with(' ') || raw.starts_with('\t');
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current = Some(line[1..line.len() - 1].trim().to_string());
            sub = None;
            out.entry(current.clone().unwrap()).or_default();
            continue;
        }
        let Some(section) = &current else { continue };
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim().to_string();
        let v = v.trim().to_string();
        if !indented {
            sub = if v.is_empty() { Some(k.clone()) } else { None };
            if v.is_empty() {
                continue;
            }
            out.get_mut(section).unwrap().insert(k, v);
        } else if let Some(s) = &sub {
            out.get_mut(section).unwrap().insert(format!("{s}.{k}"), v);
        } else {
            out.get_mut(section).unwrap().insert(k, v);
        }
    }
    out
}

pub fn config_path() -> Option<PathBuf> {
    std::env::var_os("AWS_CONFIG_FILE")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".aws/config")))
}

pub fn credentials_path() -> Option<PathBuf> {
    std::env::var_os("AWS_SHARED_CREDENTIALS_FILE")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".aws/credentials")))
}

fn read(path: Option<PathBuf>) -> Sections {
    path.and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse(&t))
        .unwrap_or_default()
}

/// The merged view of both files.
#[derive(Debug, Clone, Default)]
pub struct AwsFiles {
    pub config: Sections,
    pub credentials: Sections,
}

impl AwsFiles {
    pub fn load() -> AwsFiles {
        AwsFiles {
            config: read(config_path()),
            credentials: read(credentials_path()),
        }
    }

    pub fn from_paths(config: &Path, credentials: &Path) -> AwsFiles {
        AwsFiles {
            config: read(Some(config.to_path_buf())),
            credentials: read(Some(credentials.to_path_buf())),
        }
    }

    /// A profile's settings: credentials file entries over config entries
    /// (config's section is `[profile name]`, or `[default]`).
    pub fn profile(&self, name: &str) -> Option<BTreeMap<String, String>> {
        let mut merged = BTreeMap::new();
        let section = if name == "default" {
            "default".to_string()
        } else {
            format!("profile {name}")
        };
        let mut found = false;
        if let Some(c) = self.config.get(&section).or_else(|| {
            if name == "default" {
                self.config.get("profile default")
            } else {
                None
            }
        }) {
            merged.extend(c.clone());
            found = true;
        }
        if let Some(c) = self.credentials.get(name) {
            merged.extend(c.clone());
            found = true;
        }
        found.then_some(merged)
    }

    pub fn sso_session(&self, name: &str) -> Option<&BTreeMap<String, String>> {
        self.config.get(&format!("sso-session {name}"))
    }

    pub fn services(&self, name: &str) -> Option<&BTreeMap<String, String>> {
        self.config.get(&format!("services {name}"))
    }

    /// Every profile name in either file.
    pub fn profile_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .config
            .keys()
            .filter_map(|k| {
                if k == "default" {
                    Some(k.clone())
                } else {
                    k.strip_prefix("profile ").map(str::to_string)
                }
            })
            .chain(self.credentials.keys().cloned())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// The S3 endpoint URL a profile names (`endpoint_url`, or its
    /// `services` section's `s3.endpoint_url`).
    pub fn s3_endpoint(&self, name: &str) -> Option<String> {
        let p = self.profile(name)?;
        if let Some(svc) = p.get("services").and_then(|s| self.services(s))
            && let Some(u) = svc.get("s3.endpoint_url")
        {
            return Some(u.clone());
        }
        p.get("endpoint_url").cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "[default]\nregion = us-east-1\n\n[profile dev]\nregion = eu-west-1\nsso_session = corp\nsso_account_id = 111\nsso_role_name = Dev\nservices = local\n\n[sso-session corp]\nsso_start_url = https://corp.awsapps.com/start\nsso_region = us-east-1\n\n[services local]\ns3 =\n  endpoint_url = http://127.0.0.1:9000\n\n[profile minio]\nendpoint_url = http://minio:9000\n";
    const CREDS: &str = "[default]\naws_access_key_id = AKIDEFAULT\naws_secret_access_key = s\n# comment\n[ci]\naws_access_key_id=AKICI\naws_secret_access_key=t\n";

    #[test]
    fn merged_profiles() {
        let f = AwsFiles {
            config: parse(CONFIG),
            credentials: parse(CREDS),
        };
        assert_eq!(f.profile_names(), vec!["ci", "default", "dev", "minio"]);
        let d = f.profile("default").unwrap();
        assert_eq!(d["region"], "us-east-1");
        assert_eq!(d["aws_access_key_id"], "AKIDEFAULT");
        assert_eq!(f.profile("ci").unwrap()["aws_access_key_id"], "AKICI");
        assert_eq!(f.sso_session("corp").unwrap()["sso_region"], "us-east-1");
        assert_eq!(
            f.s3_endpoint("dev").as_deref(),
            Some("http://127.0.0.1:9000")
        );
        assert_eq!(f.s3_endpoint("minio").as_deref(), Some("http://minio:9000"));
        assert!(f.profile("nope").is_none());
    }
}
