use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use reqlog::RequestLog;
use s3raw::{Addressing, ClientConfig, S3Client, S3Error};
use serde::{Deserialize, Serialize};

use crate::config::{Config, CredentialSource, EndpointConfig, Settings};
use crate::credentials::ini::AwsFiles;
use crate::guard::Guard;
use crate::jobs::JobManager;
use crate::paths::Paths;
use crate::vendor::Vendor;

/// What the UI may know about an endpoint: never its credentials.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointView {
    pub id: String,
    pub name: String,
    pub url: String,
    pub region: String,
    pub path_style: bool,
    pub vendor: String,
    pub vendor_label: String,
    pub has_ca: bool,
    /// How credentials are obtained (`profile dev`, `instance role`), not what they are.
    pub credentials: String,
    pub buckets: Vec<String>,
}

impl From<&EndpointConfig> for EndpointView {
    fn from(e: &EndpointConfig) -> Self {
        EndpointView {
            id: e.id.clone(),
            name: e.name.clone(),
            url: e.url.clone(),
            region: e.region.clone(),
            path_style: e.path_style,
            vendor: e.vendor.id().into(),
            vendor_label: e.vendor.label().into(),
            has_ca: e.ca_file.is_some(),
            credentials: e.credentials.describe(),
            buckets: e.buckets.clone(),
        }
    }
}

/// The result of a connectivity test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityReport {
    pub endpoint: String,
    pub ok: bool,
    pub latency_ms: f64,
    pub credentials_source: Option<String>,
    pub bucket_count: Option<usize>,
    pub owner: Option<String>,
    pub identity: Option<s3raw::xml::CallerIdentity>,
    /// Why there is no identity (not supported by the vendor, or an error).
    pub identity_note: Option<String>,
    pub error: Option<S3Error>,
    pub server: Option<String>,
}

pub struct Sonde {
    paths: Paths,
    config: RwLock<Config>,
    log: RequestLog,
    jobs: JobManager,
    guard: Guard,
    clients: RwLock<HashMap<String, S3Client>>,
    transient: RwLock<std::collections::HashSet<String>>,
}

impl Sonde {
    /// Loads the configuration from `paths` (an empty one if there is none).
    pub fn open(paths: Paths) -> Result<Arc<Sonde>, String> {
        let config = Config::load(&paths.config_file())?;
        Ok(Self::with_config(paths, config))
    }

    pub fn with_config(paths: Paths, config: Config) -> Arc<Sonde> {
        let log = RequestLog::new(config.settings.reqlog_capacity);
        let guard = Guard::new(paths.audit_log());
        let s = Arc::new(Sonde {
            paths,
            config: RwLock::new(config),
            log,
            jobs: JobManager::new(),
            guard,
            clients: RwLock::new(HashMap::new()),
            transient: RwLock::new(Default::default()),
        });
        s.apply_log_settings();
        s
    }

    fn apply_log_settings(&self) {
        let settings = self.config.read().settings.clone();
        self.log.set_capacity(settings.reqlog_capacity);
        if settings.reqlog_persist {
            let _ = std::fs::create_dir_all(&self.paths.data_dir);
            match reqlog::sqlite::SqliteSink::open(&self.paths.reqlog_db()) {
                Ok(s) => self.log.set_sink(Some(Arc::new(s))),
                Err(e) => tracing::warn!("request log database: {e}"),
            }
        } else {
            self.log.set_sink(None);
        }
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }
    pub fn log(&self) -> &RequestLog {
        &self.log
    }
    pub fn jobs(&self) -> &JobManager {
        &self.jobs
    }
    pub fn guard(&self) -> &Guard {
        &self.guard
    }
    pub fn settings(&self) -> Settings {
        self.config.read().settings.clone()
    }
    pub fn read_only(&self) -> bool {
        self.config.read().settings.read_only
    }

    pub fn config(&self) -> Config {
        self.config.read().clone()
    }

    fn save(&self) -> Result<(), String> {
        let mut c = self.config.read().clone();
        let transient = self.transient.read();
        c.endpoints.retain(|e| !transient.contains(&e.id));
        c.save(&self.paths.config_file())
    }

    pub fn set_settings(&self, settings: Settings) -> Result<(), String> {
        {
            let mut c = self.config.write();
            let mut next = c.clone();
            next.settings = settings;
            next.validate()?;
            *c = next;
        }
        self.apply_log_settings();
        self.save()
    }

    pub fn endpoints(&self) -> Vec<EndpointView> {
        self.config
            .read()
            .endpoints
            .iter()
            .map(EndpointView::from)
            .collect()
    }

    pub fn endpoint_config(&self, id: &str) -> Option<EndpointConfig> {
        self.config.read().endpoint(id).cloned()
    }

    /// Adds or replaces an endpoint (by id) and saves.
    pub fn upsert_endpoint(&self, e: EndpointConfig) -> Result<EndpointView, String> {
        e.validate()?;
        {
            let mut c = self.config.write();
            match c.endpoints.iter_mut().find(|x| x.id == e.id) {
                Some(x) => *x = e.clone(),
                None => c.endpoints.push(e.clone()),
            }
        }
        self.clients.write().remove(&e.id);
        self.save()?;
        Ok(EndpointView::from(&e))
    }

    /// Adds an endpoint for this process only (not saved): the demo server.
    pub fn add_transient_endpoint(&self, e: EndpointConfig) -> Result<EndpointView, String> {
        e.validate()?;
        {
            let mut c = self.config.write();
            c.endpoints.retain(|x| x.id != e.id);
            c.endpoints.push(e.clone());
        }
        self.transient.write().insert(e.id.clone());
        self.clients.write().remove(&e.id);
        Ok(EndpointView::from(&e))
    }

    pub fn remove_endpoint(&self, id: &str) -> Result<(), String> {
        self.config.write().endpoints.retain(|e| e.id != id);
        self.clients.write().remove(id);
        self.save()
    }

    /// Endpoints for the profiles of `~/.aws/config` that are not
    /// configured yet (not saved: the caller picks which to add).
    pub fn importable_profiles(&self) -> Vec<EndpointConfig> {
        importable(&AwsFiles::load(), &self.config.read().endpoints)
    }

    /// The client of an endpoint, made on first use.
    pub fn client(&self, id: &str) -> Result<S3Client, S3Error> {
        if let Some(c) = self.clients.read().get(id) {
            return Ok(c.clone());
        }
        let e = self
            .endpoint_config(id)
            .ok_or_else(|| S3Error::config(format!("no endpoint {id:?}")))?;
        let c = build_client(&e, self.log.clone())?;
        self.clients.write().insert(id.into(), c.clone());
        Ok(c)
    }

    /// A client for a job: shares the job's throttle, tags its records.
    pub fn job_client(&self, id: &str, ctx: &crate::jobs::JobCtx) -> Result<S3Client, S3Error> {
        Ok(self
            .client(id)?
            .for_job(Some(ctx.id.clone()), Some(ctx.throttle.clone())))
    }

    /// Lists buckets and, where the vendor has it, asks STS who we are.
    pub async fn test_endpoint(&self, id: &str) -> ConnectivityReport {
        let e = match self.endpoint_config(id) {
            Some(e) => e,
            None => {
                return ConnectivityReport {
                    endpoint: id.into(),
                    ok: false,
                    latency_ms: 0.0,
                    credentials_source: None,
                    bucket_count: None,
                    owner: None,
                    identity: None,
                    identity_note: None,
                    error: Some(S3Error::config(format!("no endpoint {id:?}"))),
                    server: None,
                };
            }
        };
        let start = std::time::Instant::now();
        let mut report = ConnectivityReport {
            endpoint: id.into(),
            ok: false,
            latency_ms: 0.0,
            credentials_source: None,
            bucket_count: None,
            owner: None,
            identity: None,
            identity_note: None,
            error: None,
            server: None,
        };
        let client = match self.client(id) {
            Ok(c) => c,
            Err(err) => {
                report.error = Some(err);
                return report;
            }
        };
        match client.credentials().await {
            Ok(c) => {
                report.credentials_source =
                    Some(c.map(|c| c.source).unwrap_or_else(|| "anonymous".into()))
            }
            Err(err) => {
                report.error = Some(err);
                return report;
            }
        }
        match client.list_buckets().await {
            Ok(r) => {
                report.ok = true;
                report.bucket_count = Some(r.buckets.bucket.len());
                report.owner = r.owner.and_then(|o| o.display_name.or(o.id));
            }
            Err(err) if !e.buckets.is_empty() => {
                // credentials may be scoped to some buckets
                match client.head_bucket(&e.buckets[0]).await {
                    Ok(_) => {
                        report.ok = true;
                        report.bucket_count = Some(e.buckets.len());
                    }
                    Err(_) => report.error = Some(err),
                }
            }
            Err(err) => report.error = Some(err),
        }
        if let Some(rec) = self
            .log
            .list(
                &reqlog::Filter {
                    endpoint: Some(id.into()),
                    ..Default::default()
                },
                1,
            )
            .first()
        {
            report.server = rec.header("server").map(String::from);
        }
        report.latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        if e.vendor.has_sts_identity() {
            match e.vendor.sts_endpoint(&e.region) {
                Some(sts) => match client.caller_identity(&sts).await {
                    Ok(i) => report.identity = Some(i),
                    Err(err) => {
                        report.identity_note = Some(format!("STS GetCallerIdentity failed: {err}"))
                    }
                },
                None => report.identity_note = Some("no STS endpoint for this region".into()),
            }
        } else {
            report.identity_note = Some(format!(
                "{} has no STS GetCallerIdentity; skipped",
                e.vendor.label()
            ));
        }
        report
    }
}

/// Builds the client of an endpoint configuration.
pub fn build_client(e: &EndpointConfig, log: RequestLog) -> Result<S3Client, S3Error> {
    let region = if e.region.is_empty() {
        e.vendor.default_region().to_string()
    } else {
        e.region.clone()
    };
    let mut cfg = ClientConfig::new(&e.id, &e.url, &region)?;
    cfg.addressing = if e.path_style {
        Addressing::Path
    } else {
        Addressing::Auto
    };
    cfg.unsigned_payload = !e.sign_payload;
    if let Some(ca) = &e.ca_file {
        cfg.ca_pem = Some(
            std::fs::read(ca)
                .map_err(|err| S3Error::config(format!("CA file {}: {err}", ca.display())))?,
        );
    }
    let creds = crate::credentials::provider(&e.credentials)
        .map_err(|m| S3Error::new(s3raw::ErrorKind::Credentials, m))?;
    S3Client::new(cfg, creds, log)
}

fn importable(files: &AwsFiles, existing: &[EndpointConfig]) -> Vec<EndpointConfig> {
    files
        .profile_names()
        .into_iter()
        .filter(|p| {
            !existing
                .iter()
                .any(|e| matches!(&e.credentials, CredentialSource::Profile { name } if name == p))
        })
        .map(|name| {
            let prof = files.profile(&name).unwrap_or_default();
            let region = prof
                .get("region")
                .cloned()
                .unwrap_or_else(|| "us-east-1".into());
            let url = files.s3_endpoint(&name).unwrap_or_else(|| {
                if region.starts_with("cn-") {
                    format!("https://s3.{region}.amazonaws.com.cn")
                } else {
                    format!("https://s3.{region}.amazonaws.com")
                }
            });
            let vendor = Vendor::guess(&url);
            let id = format!(
                "profile-{}",
                name.replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "-")
            );
            let mut e = EndpointConfig::new(
                &id,
                &format!("{name} ({})", vendor.label()),
                &url,
                &region,
                vendor,
            );
            e.credentials = CredentialSource::Profile { name };
            e
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::ini;

    #[test]
    fn import_profiles() {
        let files = AwsFiles {
            config: ini::parse(
                "[default]\nregion = eu-central-1\n[profile local]\nendpoint_url = http://localhost:9000\n",
            ),
            credentials: ini::parse("[ci]\naws_access_key_id=a\naws_secret_access_key=b\n"),
        };
        let mut existing = vec![EndpointConfig::new("x", "x", "http://x", "", Vendor::Other)];
        existing[0].credentials = CredentialSource::Profile { name: "ci".into() };
        let got = importable(&files, &existing);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].url, "https://s3.eu-central-1.amazonaws.com");
        assert_eq!(got[0].vendor, Vendor::Aws);
        assert_eq!(got[1].id, "profile-local");
        assert_eq!(got[1].vendor, Vendor::Minio);
    }

    #[test]
    fn views_hide_credentials() {
        let mut e = EndpointConfig::new(
            "m",
            "MinIO",
            "http://127.0.0.1:9000",
            "us-east-1",
            Vendor::Minio,
        );
        e.credentials = CredentialSource::Static {
            access_key_id: "AKIASECRET".into(),
            secret_access_key: "SUPERSECRET".into(),
        };
        let v = serde_json::to_string(&EndpointView::from(&e)).unwrap();
        assert!(
            !v.contains("SUPERSECRET") && !v.contains("AKIASECRET"),
            "{v}"
        );
    }

    #[test]
    fn endpoints_persist() {
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(Paths::rooted(dir.path())).unwrap();
        s.upsert_endpoint(EndpointConfig::new(
            "a",
            "A",
            "http://127.0.0.1:1",
            "us-east-1",
            Vendor::Minio,
        ))
        .unwrap();
        let s2 = Sonde::open(Paths::rooted(dir.path())).unwrap();
        assert_eq!(s2.endpoints().len(), 1);
        s2.remove_endpoint("a").unwrap();
        assert!(
            Sonde::open(Paths::rooted(dir.path()))
                .unwrap()
                .endpoints()
                .is_empty()
        );
        assert!(s.client("missing").is_err());
    }
}
