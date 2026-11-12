//! Live targets for integration tests, from `SONDE_IT_TARGETS`:
//! `name=url,access,secret[,vendor];name2=...`. Unset: tests skip.
#![allow(dead_code)]

use std::sync::Arc;

use sonde_core::{Config, CredentialSource, EndpointConfig, Paths, Sonde, Vendor};

pub struct Target {
    pub name: String,
    pub sonde: Arc<Sonde>,
    pub vendor: Vendor,
    _dir: tempfile::TempDir,
}

pub fn targets() -> Vec<Target> {
    let Ok(spec) = std::env::var("SONDE_IT_TARGETS") else {
        eprintln!("SONDE_IT_TARGETS unset: skipping live tests");
        return vec![];
    };
    spec.split(';')
        .filter(|s| !s.trim().is_empty())
        .map(|t| {
            let (name, rest) = t.split_once('=').expect("name=url,access,secret");
            let parts: Vec<&str> = rest.split(',').collect();
            let vendor = parts
                .get(3)
                .and_then(|v| Vendor::from_id(v))
                .unwrap_or(Vendor::Other);
            let mut e = EndpointConfig::new(name, name, parts[0], "us-east-1", vendor);
            e.path_style = true;
            e.credentials = CredentialSource::Static {
                access_key_id: parts[1].into(),
                secret_access_key: parts[2].into(),
            };
            let mut config = Config::default();
            config.endpoints.push(e);
            let dir = tempfile::tempdir().unwrap();
            Target {
                name: name.into(),
                sonde: Sonde::with_config(Paths::rooted(dir.path()), config),
                vendor,
                _dir: dir,
            }
        })
        .collect()
}

pub fn bucket_name(tag: &str) -> String {
    format!(
        "sonde-it-{tag}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    )
}
