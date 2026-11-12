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
    /// The CA bundle's path (a path, never a secret): an edit keeps it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<String>,
    /// How credentials are obtained (`profile dev`, `instance role`), not what they are.
    pub credentials: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials_text: Vec<crate::Phrase>,
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
            ca_file: e.ca_file.as_ref().map(|p| p.to_string_lossy().into_owned()),
            credentials: e.credentials.describe(),
            credentials_text: vec![e.credentials.describe_text()],
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

/// Values of one run that stand in for the saved settings (`--rate`,
/// `--concurrency`): never written to `config.toml`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Overrides {
    pub rate_limit: Option<f64>,
    pub concurrency: Option<usize>,
}

struct State {
    /// What `config.toml` holds (or will, at the next save).
    base: Config,
    /// The file as last read or written: a save that finds it changed
    /// starts from the file. `None` when it was never read.
    disk: Option<Config>,
    /// Endpoints of this process only. One with a configured id hides the
    /// configured endpoint, which stays in `base` and in every save.
    transient: Vec<EndpointConfig>,
    overrides: Overrides,
    /// Why the file did not load. Nothing is saved while it stands.
    error: Option<String>,
    /// `base` with the transient endpoints and the overrides: what runs.
    effective: Config,
}

impl State {
    fn refresh(&mut self) {
        let mut c = self.base.clone();
        for t in &self.transient {
            match c.endpoints.iter_mut().find(|e| e.id == t.id) {
                Some(e) => *e = t.clone(),
                None => c.endpoints.push(t.clone()),
            }
        }
        if let Some(r) = self.overrides.rate_limit {
            c.settings.rate_limit = Some(r);
        }
        if let Some(n) = self.overrides.concurrency {
            c.settings.concurrency = n;
        }
        self.effective = c;
    }
}

pub struct Sonde {
    paths: Paths,
    state: RwLock<State>,
    /// One save at a time in this process (the lock file covers the others).
    saving: parking_lot::Mutex<()>,
    log: RequestLog,
    jobs: JobManager,
    guard: Guard,
    clients: RwLock<HashMap<String, S3Client>>,
}

impl Sonde {
    /// Loads the configuration from `paths` (an empty one if there is none).
    pub fn open(paths: Paths) -> Result<Arc<Sonde>, String> {
        let config = Config::load(&paths.config_file())?;
        Ok(Self::build(paths, config.clone(), Some(config), None))
    }

    /// As [`Self::open`], but a file that does not load is kept: the
    /// sonde starts empty, [`Self::config_error`] says why, and nothing is
    /// saved until [`Self::reload_config`] reads the file.
    pub fn open_lenient(paths: Paths) -> Arc<Sonde> {
        match Config::load(&paths.config_file()) {
            Ok(c) => Self::build(paths, c.clone(), Some(c), None),
            Err(e) => Self::build(paths, Config::default(), None, Some(e)),
        }
    }

    pub fn with_config(paths: Paths, config: Config) -> Arc<Sonde> {
        Self::build(paths, config, None, None)
    }

    fn build(
        paths: Paths,
        config: Config,
        disk: Option<Config>,
        error: Option<String>,
    ) -> Arc<Sonde> {
        let log = RequestLog::new(reqlog::DEFAULT_CAPACITY);
        let guard = Guard::new(paths.audit_log());
        Arc::new(Sonde {
            paths,
            state: RwLock::new(State {
                effective: config.clone(),
                base: config,
                disk,
                transient: Vec::new(),
                overrides: Overrides::default(),
                error,
            }),
            saving: Default::default(),
            log,
            jobs: JobManager::new(),
            guard,
            clients: RwLock::new(HashMap::new()),
        })
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
        self.state.read().effective.settings.clone()
    }
    pub fn config(&self) -> Config {
        self.state.read().effective.clone()
    }

    /// Why `config.toml` did not load, if it did not.
    pub fn config_error(&self) -> Option<String> {
        self.state.read().error.clone()
    }

    /// Reads `config.toml` again: after the file was fixed, or to pick up
    /// edits made outside. An error is kept (and returned).
    pub fn reload_config(&self) -> Result<(), String> {
        let _one = self.saving.lock();
        let loaded = Config::load(&self.paths.config_file());
        {
            let mut st = self.state.write();
            match loaded {
                Ok(c) => {
                    st.base = c.clone();
                    st.disk = Some(c);
                    st.error = None;
                    st.refresh();
                }
                Err(e) => {
                    // a file that loaded before stays in use, unsaved
                    st.error = Some(e.clone());
                    return Err(e);
                }
            }
        }
        self.clients.write().clear();
        Ok(())
    }

    /// Values for this run only (`--rate`, `--concurrency`): not saved.
    pub fn set_overrides(&self, o: Overrides) -> Result<(), String> {
        if o.concurrency.is_some_and(|n| n == 0 || n > 1024) {
            return Err("concurrency: 1 to 1024".into());
        }
        let mut st = self.state.write();
        st.overrides = o;
        st.refresh();
        Ok(())
    }

    /// Applies `change` to the configuration and writes it. The file is
    /// read first: one that changed since it was last read is what the
    /// change applies to (edits made outside are kept), and one that does
    /// not load is left as it is.
    fn save_with(
        &self,
        change: impl FnOnce(&mut Config) -> Result<(), String>,
    ) -> Result<(), String> {
        let _one = self.saving.lock();
        if let Some(e) = self.config_error() {
            return Err(not_loaded(&e));
        }
        let file = self.paths.config_file();
        let _lock = crate::config::lock(&file);
        let current = Config::load(&file).map_err(|e| {
            format!("{e}: the file no longer loads, so nothing was saved; fix it and reload")
        })?;
        let (known, mut next) = {
            let st = self.state.read();
            (st.disk.clone(), st.base.clone())
        };
        let outside = known.is_some_and(|k| k != current);
        if outside {
            next = current;
        }
        change(&mut next)?;
        next.validate()?;
        next.save(&file)?;
        {
            let mut st = self.state.write();
            st.base = next.clone();
            st.disk = Some(next);
            st.refresh();
        }
        // an edit made outside may have changed any endpoint: no client
        // made from the old file is kept
        if outside {
            self.clients.write().clear();
        }
        Ok(())
    }

    /// Saves the settings that differ from the current ones (and only
    /// those: the rest keep what the file says).
    pub fn set_settings(&self, settings: Settings) -> Result<(), String> {
        let (old, overrides) = {
            let st = self.state.read();
            (st.effective.settings.clone(), st.overrides)
        };
        let obj = |s: &Settings| match serde_json::to_value(s) {
            Ok(serde_json::Value::Object(m)) => Ok(m),
            _ => Err("settings: not an object".to_string()),
        };
        let (was, now) = (obj(&old)?, obj(&settings)?);
        let changed: Vec<(String, serde_json::Value)> = now
            .into_iter()
            .filter(|(k, v)| was.get(k) != Some(v))
            .collect();
        // an override the user changed is theirs now, and saved
        let mut o = overrides;
        for (k, _) in &changed {
            match k.as_str() {
                "rate-limit" => o.rate_limit = None,
                "concurrency" => o.concurrency = None,
                _ => {}
            }
        }
        self.save_with(|c| {
            let mut m = obj(&c.settings)?;
            m.extend(changed);
            c.settings = serde_json::from_value(serde_json::Value::Object(m))
                .map_err(|e| format!("settings: {e}"))?;
            Ok(())
        })?;
        {
            let mut st = self.state.write();
            st.overrides = o;
            st.refresh();
        }
        Ok(())
    }

    pub fn endpoints(&self) -> Vec<EndpointView> {
        self.state
            .read()
            .effective
            .endpoints
            .iter()
            .map(EndpointView::from)
            .collect()
    }

    /// Request prices for a session on `endpoint`: writes, reads, lists
    /// (when other than a write) and the currency. The settings', unless they
    /// are still S3 Standard's and the endpoint's vendor charges otherwise
    /// (R2, B2, Wasabi, a store of one's own). Every view prices this way.
    pub fn request_prices_for(&self, endpoint: &str) -> (f64, f64, Option<f64>, String) {
        let p = self.settings().prices;
        let standard = crate::config::Prices::default();
        let untouched = p.per_1000_writes == standard.per_1000_writes
            && p.per_1000_reads == standard.per_1000_reads
            && p.currency == standard.currency;
        match self
            .endpoint_config(endpoint)
            .and_then(|e| e.vendor.request_prices())
            .filter(|_| untouched)
        {
            Some((w, r, l)) => (w, r, l, p.currency),
            None => (p.per_1000_writes, p.per_1000_reads, None, p.currency),
        }
    }

    pub fn endpoint_config(&self, id: &str) -> Option<EndpointConfig> {
        self.state.read().effective.endpoint(id).cloned()
    }

    fn is_transient(&self, id: &str) -> bool {
        self.state.read().transient.iter().any(|t| t.id == id)
    }

    /// Adds or replaces an endpoint (by id) and saves. One that exists for
    /// this process only (the demo, `--url`) changes for this process only.
    pub fn upsert_endpoint(&self, e: EndpointConfig) -> Result<EndpointView, String> {
        e.validate()?;
        if self.is_transient(&e.id) {
            return self.add_transient_endpoint(e);
        }
        self.save_with(|c| {
            match c.endpoints.iter_mut().find(|x| x.id == e.id) {
                Some(x) => *x = e.clone(),
                None => c.endpoints.push(e.clone()),
            }
            Ok(())
        })?;
        self.clients.write().remove(&e.id);
        Ok(EndpointView::from(&e))
    }

    /// Adds an endpoint for this process only (not saved): the demo server,
    /// `--url`. A configured endpoint with the same id is hidden meanwhile,
    /// not replaced: it stays in the file and comes back when this one goes.
    pub fn add_transient_endpoint(&self, e: EndpointConfig) -> Result<EndpointView, String> {
        e.validate()?;
        {
            let mut st = self.state.write();
            st.transient.retain(|x| x.id != e.id);
            st.transient.push(e.clone());
            st.refresh();
        }
        self.clients.write().remove(&e.id);
        Ok(EndpointView::from(&e))
    }

    /// Removes an endpoint. One of this process only just goes (the
    /// configured endpoint it hid is back); a configured one leaves the file.
    pub fn remove_endpoint(&self, id: &str) -> Result<(), String> {
        if self.is_transient(id) {
            let mut st = self.state.write();
            st.transient.retain(|x| x.id != id);
            st.refresh();
        } else {
            self.save_with(|c| {
                c.endpoints.retain(|e| e.id != id);
                Ok(())
            })?;
        }
        self.clients.write().remove(id);
        Ok(())
    }

    /// Endpoints for the profiles of `~/.aws/config` that are not
    /// configured yet (not saved: the caller picks which to add).
    pub fn importable_profiles(&self) -> Vec<EndpointConfig> {
        importable(&AwsFiles::load(), &self.state.read().effective.endpoints)
    }

    /// Adds the endpoints of the named AWS profiles; how many were added.
    /// Never replaces an endpoint: [`importable`] gives each a free id.
    pub fn import_profiles(&self, names: &[String]) -> Result<u32, String> {
        let mut n = 0;
        for e in self.importable_profiles() {
            if let CredentialSource::Profile { name } = &e.credentials
                && names.contains(name)
                && self.endpoint_config(&e.id).is_none()
            {
                self.upsert_endpoint(e)?;
                n += 1;
            }
        }
        Ok(n)
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

fn not_loaded(e: &str) -> String {
    format!("{e}: the configuration was not loaded, so nothing is saved; fix the file and reload")
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
    // ids in use: the configured ones, then each one given out here
    let mut taken: std::collections::HashSet<String> =
        existing.iter().map(|e| e.id.clone()).collect();
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
            let stem = format!(
                "profile-{}",
                name.replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "-")
            );
            // `a.b` and `a_b` give one stem, and an endpoint may hold it already
            let mut id = stem.clone();
            let mut n = 2;
            while !taken.insert(id.clone()) {
                id = format!("{stem}-{n}");
                n += 1;
            }
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

    fn ep(id: &str) -> EndpointConfig {
        EndpointConfig::new(id, id, "http://127.0.0.1:1", "us-east-1", Vendor::Minio)
    }

    #[test]
    fn a_config_that_did_not_load_is_never_written_over() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::rooted(dir.path());
        let file = paths.config_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let text = "[[endpoint]]\nid = \"prod\"\nname = \"Prod\"\nurl = \"https://s3.example.com\"\nca-file = \"/nonexistent/ca.pem\"\n";
        std::fs::write(&file, text).unwrap();
        let s = Sonde::open_lenient(paths);
        assert!(s.config_error().unwrap().contains("CA file"));
        assert!(s.endpoints().is_empty());
        let mut st = s.settings();
        st.theme = "dark".into();
        let e = s.set_settings(st).unwrap_err();
        assert!(e.contains("not loaded"), "{e}");
        assert_eq!(
            s.settings().theme,
            "system",
            "a refused save changes nothing"
        );
        assert!(s.upsert_endpoint(ep("a")).is_err());
        assert!(s.remove_endpoint("prod").is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        // fixed by hand, then read again
        assert!(s.reload_config().is_err());
        std::fs::write(
            &file,
            text.replace("ca-file = \"/nonexistent/ca.pem\"\n", ""),
        )
        .unwrap();
        s.reload_config().unwrap();
        assert_eq!(s.config_error(), None);
        assert_eq!(s.endpoints().len(), 1);
        s.upsert_endpoint(ep("a")).unwrap();
        assert_eq!(Config::load(&file).unwrap().endpoints.len(), 2);
    }

    #[test]
    fn a_save_that_takes_in_an_outside_edit_sends_to_the_new_address() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::rooted(dir.path());
        let file = paths.config_file();
        let s = Sonde::open(paths).unwrap();
        s.upsert_endpoint(ep("a")).unwrap();
        let url = |s: &Sonde| s.client("a").unwrap().config().endpoint.to_string();
        assert!(url(&s).contains("127.0.0.1:1"));
        // the endpoint's address is edited in the file; then the app saves something else
        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, text.replace("127.0.0.1:1", "127.0.0.1:2")).unwrap();
        s.upsert_endpoint(ep("b")).unwrap();
        assert!(url(&s).contains("127.0.0.1:2"), "{}", url(&s));
    }

    #[test]
    fn a_save_keeps_what_was_edited_in_the_file_meanwhile() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::rooted(dir.path());
        let file = paths.config_file();
        let s = Sonde::open(paths.clone()).unwrap();
        s.upsert_endpoint(ep("a")).unwrap();
        // another process (the CLI, an editor) adds an endpoint and a setting
        let other = Sonde::open(paths).unwrap();
        other.upsert_endpoint(ep("by-hand")).unwrap();
        let mut st = other.settings();
        st.language = "zh".into();
        other.set_settings(st).unwrap();
        // this one changes one setting: only that is applied to the file
        let mut st = s.settings();
        st.theme = "dark".into();
        s.set_settings(st).unwrap();
        let on_disk = Config::load(&file).unwrap();
        assert!(on_disk.endpoint("by-hand").is_some());
        assert_eq!(on_disk.settings.language, "zh");
        assert_eq!(on_disk.settings.theme, "dark");
        assert!(s.endpoint_config("by-hand").is_some(), "and it is in use");
        // a file that no longer loads is left alone
        std::fs::write(&file, "not toml [").unwrap();
        assert!(s.upsert_endpoint(ep("b")).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "not toml [");
    }

    #[cfg(unix)]
    #[test]
    fn the_config_is_written_with_mode_0600_through_a_file_of_its_own() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::rooted(dir.path());
        let s = Sonde::open(paths.clone()).unwrap();
        s.upsert_endpoint(ep("a")).unwrap();
        let file = paths.config_file();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let left: Vec<_> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn an_endpoint_of_this_run_hides_a_configured_one_and_never_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::rooted(dir.path());
        let s = Sonde::open(paths.clone()).unwrap();
        let mut mine = ep("demo");
        mine.name = "Mine".into();
        s.upsert_endpoint(mine).unwrap();
        let mut demo = ep("demo");
        demo.name = "Demo (built-in)".into();
        s.add_transient_endpoint(demo).unwrap();
        assert_eq!(s.endpoint_config("demo").unwrap().name, "Demo (built-in)");
        assert_eq!(s.endpoints().len(), 1);
        // any save keeps the configured one in the file
        let mut st = s.settings();
        st.theme = "dark".into();
        s.set_settings(st).unwrap();
        s.upsert_endpoint(ep("other")).unwrap();
        let on_disk = Config::load(&paths.config_file()).unwrap();
        assert_eq!(on_disk.endpoint("demo").unwrap().name, "Mine");
        // removing the demo brings it back
        s.remove_endpoint("demo").unwrap();
        assert_eq!(s.endpoint_config("demo").unwrap().name, "Mine");
        let on_disk = Config::load(&paths.config_file()).unwrap();
        assert_eq!(on_disk.endpoint("demo").unwrap().name, "Mine");
    }

    #[test]
    fn values_of_one_run_are_never_saved() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::rooted(dir.path());
        let s = Sonde::open(paths.clone()).unwrap();
        s.add_transient_endpoint(ep("adhoc")).unwrap();
        s.set_overrides(Overrides {
            rate_limit: Some(5.0),
            concurrency: Some(2),
        })
        .unwrap();
        assert_eq!(s.settings().concurrency, 2);
        let mut st = s.settings();
        st.theme = "dark".into();
        s.set_settings(st).unwrap();
        let on_disk = Config::load(&paths.config_file()).unwrap();
        assert!(on_disk.endpoints.is_empty());
        assert_eq!(on_disk.settings.concurrency, 16);
        assert_eq!(on_disk.settings.rate_limit, None);
        assert_eq!(on_disk.settings.theme, "dark");
        assert_eq!(s.settings().concurrency, 2, "still in force here");
        // a value the user sets is theirs: saved, and the override ends
        let mut st = s.settings();
        st.concurrency = 8;
        s.set_settings(st).unwrap();
        assert_eq!(s.settings().concurrency, 8);
        let on_disk = Config::load(&paths.config_file()).unwrap();
        assert_eq!(on_disk.settings.concurrency, 8);
        assert_eq!(on_disk.settings.rate_limit, None);
    }

    #[test]
    fn importing_profiles_never_takes_an_id_in_use() {
        let files = AwsFiles {
            config: ini::parse("[profile dev]\nregion = us-east-1\n[profile a.b]\n[profile a_b]\n"),
            credentials: Default::default(),
        };
        let existing = vec![ep("profile-dev")];
        let got = importable(&files, &existing);
        let mut ids: Vec<&str> = got.iter().map(|e| e.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["profile-a-b", "profile-a-b-2", "profile-dev-2"]);
    }
}
