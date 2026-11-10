//! Credential resolution, entirely on the Rust side: environment
//! variables, the shared config / credentials files (static keys,
//! `credential_process`, SSO, `role_arn` + `source_profile`, web identity),
//! IRSA (web identity from the environment), container credentials and
//! IMDSv2. Values live in [`s3raw::Credentials`] and never leave this
//! process: the UI only sees [`CredentialSource::describe`].

pub mod ini;

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use s3raw::{Credentials, CredentialsProvider};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::config::CredentialSource;
use ini::AwsFiles;

/// One way to obtain credentials, without caching.
#[async_trait::async_trait]
pub trait Source: Send + Sync {
    /// `Ok(None)`: this source has nothing to offer (try the next one).
    async fn fetch(&self) -> Result<Option<Credentials>, String>;
    fn name(&self) -> String;
}

/// Caches what a source returns until five minutes before it expires.
pub struct Cached {
    source: Box<dyn Source>,
    cache: Mutex<Option<Credentials>>,
    anonymous: bool,
}

#[async_trait::async_trait]
impl CredentialsProvider for Cached {
    async fn credentials(&self) -> Result<Option<Credentials>, String> {
        if self.anonymous {
            return Ok(None);
        }
        let mut cache = self.cache.lock().await;
        if let Some(c) = cache.as_ref()
            && !c.expires_within(300)
        {
            return Ok(Some(c.clone()));
        }
        match self.source.fetch().await? {
            Some(c) => {
                *cache = Some(c.clone());
                Ok(Some(c))
            }
            None => Err(format!("no credentials from {}", self.source.name())),
        }
    }

    fn describe(&self) -> String {
        if self.anonymous {
            "anonymous".into()
        } else {
            self.source.name()
        }
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(2))
        .user_agent(concat!("sonde/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

// --- environment ------------------------------------------------------------

pub struct EnvSource {
    pub id_var: String,
    pub secret_var: String,
    pub token_var: Option<String>,
}

impl Default for EnvSource {
    fn default() -> Self {
        EnvSource {
            id_var: "AWS_ACCESS_KEY_ID".into(),
            secret_var: "AWS_SECRET_ACCESS_KEY".into(),
            token_var: Some("AWS_SESSION_TOKEN".into()),
        }
    }
}

#[async_trait::async_trait]
impl Source for EnvSource {
    async fn fetch(&self) -> Result<Option<Credentials>, String> {
        let id = std::env::var(&self.id_var).ok().filter(|s| !s.is_empty());
        let secret = std::env::var(&self.secret_var)
            .ok()
            .filter(|s| !s.is_empty());
        match (id, secret) {
            (Some(id), Some(secret)) => {
                let token = self
                    .token_var
                    .as_ref()
                    .and_then(|v| std::env::var(v).ok())
                    .filter(|s| !s.is_empty());
                Ok(Some(
                    Credentials::new(id, secret, token.as_deref())
                        .with_source(format!("env ${}", self.id_var)),
                ))
            }
            _ => Ok(None),
        }
    }
    fn name(&self) -> String {
        format!("environment (${})", self.id_var)
    }
}

// --- credential_process --------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ProcessOutput {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    expiration: Option<String>,
}

pub struct ProcessSource {
    pub command: String,
}

#[async_trait::async_trait]
impl Source for ProcessSource {
    async fn fetch(&self) -> Result<Option<Credentials>, String> {
        let shell = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };
        let out = tokio::process::Command::new(shell.0)
            .arg(shell.1)
            .arg(&self.command)
            .output()
            .await
            .map_err(|e| format!("credential_process: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "credential_process exited with {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let p: ProcessOutput = serde_json::from_slice(&out.stdout)
            .map_err(|e| format!("credential_process output: {e}"))?;
        Ok(Some(
            Credentials::new(
                p.access_key_id,
                p.secret_access_key,
                p.session_token.as_deref(),
            )
            .with_expiry(p.expiration.as_deref().and_then(parse_time))
            .with_source("credential_process"),
        ))
    }
    fn name(&self) -> String {
        "credential_process".into()
    }
}

// --- IMDSv2 ----------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MetadataCredentials {
    access_key_id: String,
    secret_access_key: String,
    token: Option<String>,
    expiration: Option<String>,
}

pub struct ImdsSource {
    pub endpoint: String,
}

impl Default for ImdsSource {
    fn default() -> Self {
        ImdsSource {
            endpoint: std::env::var("AWS_EC2_METADATA_SERVICE_ENDPOINT")
                .unwrap_or_else(|_| "http://169.254.169.254".into()),
        }
    }
}

#[async_trait::async_trait]
impl Source for ImdsSource {
    async fn fetch(&self) -> Result<Option<Credentials>, String> {
        if std::env::var("AWS_EC2_METADATA_DISABLED").is_ok_and(|v| v.eq_ignore_ascii_case("true"))
        {
            return Ok(None);
        }
        let base = self.endpoint.trim_end_matches('/');
        let c = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .connect_timeout(Duration::from_secs(1))
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?;
        let token = c
            .put(format!("{base}/latest/api/token"))
            .header("x-aws-ec2-metadata-token-ttl-seconds", "21600")
            // an empty PUT without a length is refused (411) by some metadata servers
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .send()
            .await
            .map_err(|e| format!("IMDSv2 token: {e}"))?;
        if !token.status().is_success() {
            return Err(format!("IMDSv2 token: HTTP {}", token.status()));
        }
        let token = token.text().await.map_err(|e| e.to_string())?;
        let role = c
            .get(format!("{base}/latest/meta-data/iam/security-credentials/"))
            .header("x-aws-ec2-metadata-token", &token)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if role.status().as_u16() == 404 {
            return Ok(None);
        }
        let role = role.text().await.map_err(|e| e.to_string())?;
        let role = role.lines().next().unwrap_or("").trim().to_string();
        if role.is_empty() {
            return Ok(None);
        }
        let creds: MetadataCredentials = c
            .get(format!(
                "{base}/latest/meta-data/iam/security-credentials/{role}"
            ))
            .header("x-aws-ec2-metadata-token", &token)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| format!("IMDS credentials: {e}"))?;
        Ok(Some(
            Credentials::new(
                creds.access_key_id,
                creds.secret_access_key,
                creds.token.as_deref(),
            )
            .with_expiry(creds.expiration.as_deref().and_then(parse_time))
            .with_source(format!("imds role {role}")),
        ))
    }
    fn name(&self) -> String {
        "instance role (IMDSv2)".into()
    }
}

// --- container (ECS, EKS Pod Identity) ---------------------------------------------

pub struct ContainerSource;

#[async_trait::async_trait]
impl Source for ContainerSource {
    async fn fetch(&self) -> Result<Option<Credentials>, String> {
        let url = if let Ok(rel) = std::env::var("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI") {
            format!("http://169.254.170.2{rel}")
        } else if let Ok(full) = std::env::var("AWS_CONTAINER_CREDENTIALS_FULL_URI") {
            full
        } else {
            return Ok(None);
        };
        let mut req = http().get(&url);
        let token = std::env::var("AWS_CONTAINER_AUTHORIZATION_TOKEN")
            .ok()
            .or_else(|| {
                std::env::var("AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE")
                    .ok()
                    .and_then(|f| std::fs::read_to_string(f).ok())
                    .map(|s| s.trim().to_string())
            });
        if let Some(t) = token {
            req = req.header("authorization", t);
        }
        let c: MetadataCredentials = req
            .send()
            .await
            .map_err(|e| format!("container credentials: {e}"))?
            .json()
            .await
            .map_err(|e| format!("container credentials: {e}"))?;
        Ok(Some(
            Credentials::new(c.access_key_id, c.secret_access_key, c.token.as_deref())
                .with_expiry(c.expiration.as_deref().and_then(parse_time))
                .with_source("container"),
        ))
    }
    fn name(&self) -> String {
        "container credentials".into()
    }
}

// --- STS ------------------------------------------------------------------------

fn form(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                s3raw::sigv4::uri_encode(k, false),
                s3raw::sigv4::uri_encode(v, false)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Calls STS with a form body, signed with `creds` when given.
async fn sts(
    region: &str,
    params: &[(&str, &str)],
    creds: Option<&Credentials>,
) -> Result<Credentials, String> {
    let region = if region.is_empty() {
        "us-east-1"
    } else {
        region
    };
    let host = if region.starts_with("cn-") {
        format!("sts.{region}.amazonaws.com.cn")
    } else {
        format!("sts.{region}.amazonaws.com")
    };
    let endpoint =
        std::env::var("AWS_ENDPOINT_URL_STS").unwrap_or_else(|_| format!("https://{host}"));
    let host = host_header(&endpoint).unwrap_or(host);
    let body = form(params);
    let mut req = http().post(&endpoint).header(
        "content-type",
        "application/x-www-form-urlencoded; charset=utf-8",
    );
    if let Some(c) = creds {
        let now = Utc::now();
        let hash = s3raw::sigv4::sha256_hex(body.as_bytes());
        let mut headers = vec![
            (
                "content-type".to_string(),
                "application/x-www-form-urlencoded; charset=utf-8".to_string(),
            ),
            ("host".to_string(), host.clone()),
            (
                "x-amz-date".to_string(),
                now.format("%Y%m%dT%H%M%SZ").to_string(),
            ),
        ];
        if let Some(t) = &c.session_token {
            headers.push(("x-amz-security-token".into(), t.expose().to_string()));
        }
        let sig = s3raw::sigv4::sign(
            &s3raw::sigv4::SignInput {
                method: "POST",
                canonical_uri: "/",
                query: &[],
                headers: &headers,
                payload_hash: &hash,
                region,
                service: "sts",
                time: now,
            },
            c,
        );
        for (k, v) in headers
            .iter()
            .filter(|(k, _)| k != "host" && k != "content-type")
        {
            req = req.header(k, v);
        }
        req = req.header("authorization", sig.authorization(&c.access_key_id));
    }
    let resp = req
        .body(body)
        .send()
        .await
        .map_err(|e| format!("STS: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let e = s3raw::xml::parse_error(&text);
        return Err(format!(
            "STS {}: {}",
            e.as_ref()
                .map(|e| e.code.clone())
                .unwrap_or_else(|| status.to_string()),
            e.and_then(|e| e.message).unwrap_or_default()
        ));
    }
    let c = s3raw::xml::sts_credentials(&text).map_err(|e| e.to_string())?;
    Ok(Credentials::new(
        c.access_key_id,
        c.secret_access_key,
        c.session_token.as_deref(),
    )
    .with_expiry(c.expiration.as_deref().and_then(parse_time)))
}

pub struct WebIdentitySource {
    pub role_arn: String,
    pub token_file: std::path::PathBuf,
    pub session_name: String,
    pub region: String,
}

impl WebIdentitySource {
    /// IRSA / EKS: `AWS_WEB_IDENTITY_TOKEN_FILE` and `AWS_ROLE_ARN`.
    pub fn from_env() -> Option<Self> {
        let token_file = std::env::var_os("AWS_WEB_IDENTITY_TOKEN_FILE")?;
        let role_arn = std::env::var("AWS_ROLE_ARN").ok()?;
        Some(WebIdentitySource {
            role_arn,
            token_file: token_file.into(),
            session_name: std::env::var("AWS_ROLE_SESSION_NAME")
                .unwrap_or_else(|_| format!("sonde-{}", Utc::now().timestamp())),
            region: std::env::var("AWS_REGION")
                .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
                .unwrap_or_default(),
        })
    }
}

#[async_trait::async_trait]
impl Source for WebIdentitySource {
    async fn fetch(&self) -> Result<Option<Credentials>, String> {
        let token = std::fs::read_to_string(&self.token_file)
            .map_err(|e| format!("web identity token {}: {e}", self.token_file.display()))?;
        let c = sts(
            &self.region,
            &[
                ("Action", "AssumeRoleWithWebIdentity"),
                ("Version", "2011-06-15"),
                ("RoleArn", &self.role_arn),
                ("RoleSessionName", &self.session_name),
                ("WebIdentityToken", token.trim()),
            ],
            None,
        )
        .await?;
        Ok(Some(
            c.with_source(format!("web identity {}", self.role_arn)),
        ))
    }
    fn name(&self) -> String {
        format!("web identity ({})", self.role_arn)
    }
}

pub struct AssumeRoleSource {
    pub role_arn: String,
    pub source: Box<dyn Source>,
    pub session_name: String,
    pub external_id: Option<String>,
    pub region: String,
}

#[async_trait::async_trait]
impl Source for AssumeRoleSource {
    async fn fetch(&self) -> Result<Option<Credentials>, String> {
        let base = self
            .source
            .fetch()
            .await?
            .ok_or_else(|| format!("source credentials for {}", self.role_arn))?;
        let mut params = vec![
            ("Action", "AssumeRole"),
            ("Version", "2011-06-15"),
            ("RoleArn", self.role_arn.as_str()),
            ("RoleSessionName", self.session_name.as_str()),
        ];
        if let Some(e) = &self.external_id {
            params.push(("ExternalId", e));
        }
        let c = sts(&self.region, &params, Some(&base)).await?;
        Ok(Some(
            c.with_source(format!("assume role {}", self.role_arn)),
        ))
    }
    fn name(&self) -> String {
        format!("assume role {} via {}", self.role_arn, self.source.name())
    }
}

// --- SSO ------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SsoToken {
    access_token: String,
    expires_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SsoRoleCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    expiration: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SsoResponse {
    role_credentials: SsoRoleCredentials,
}

/// IAM Identity Center: the token `aws sso login` cached, exchanged for
/// role credentials. sonde does not run the login itself.
pub struct SsoSource {
    /// `sso_session` name, or the start URL for legacy profiles.
    pub cache_key: String,
    pub region: String,
    pub account_id: String,
    pub role_name: String,
    pub cache_dir: std::path::PathBuf,
}

/// The file the AWS CLI caches an SSO token in.
pub fn sso_cache_file(dir: &std::path::Path, key: &str) -> std::path::PathBuf {
    use sha1::Digest;
    dir.join(format!(
        "{}.json",
        hex::encode(sha1::Sha1::digest(key.as_bytes()))
    ))
}

#[async_trait::async_trait]
impl Source for SsoSource {
    async fn fetch(&self) -> Result<Option<Credentials>, String> {
        let file = sso_cache_file(&self.cache_dir, &self.cache_key);
        let text = std::fs::read_to_string(&file).map_err(|_| {
            format!(
                "no cached SSO token for {}: run `aws sso login`",
                self.cache_key
            )
        })?;
        let token: SsoToken =
            serde_json::from_str(&text).map_err(|e| format!("SSO token cache: {e}"))?;
        if parse_time(&token.expires_at)
            .or_else(|| {
                DateTime::parse_from_str(&token.expires_at, "%Y-%m-%dT%H:%M:%S%#z")
                    .ok()
                    .map(|d| d.with_timezone(&Utc))
            })
            .is_some_and(|t| t < Utc::now())
        {
            return Err(format!(
                "the SSO token for {} expired: run `aws sso login`",
                self.cache_key
            ));
        }
        let url = format!(
            "https://portal.sso.{}.amazonaws.com/federation/credentials",
            self.region
        );
        let resp = http()
            .get(url)
            .query(&[
                ("role_name", &self.role_name),
                ("account_id", &self.account_id),
            ])
            .header("x-amz-sso_bearer_token", token.access_token)
            .send()
            .await
            .map_err(|e| format!("SSO: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("SSO GetRoleCredentials: HTTP {}", resp.status()));
        }
        let r: SsoResponse = resp.json().await.map_err(|e| format!("SSO: {e}"))?;
        let c = r.role_credentials;
        Ok(Some(
            Credentials::new(
                c.access_key_id,
                c.secret_access_key,
                c.session_token.as_deref(),
            )
            .with_expiry(DateTime::from_timestamp_millis(c.expiration))
            .with_source(format!("sso {}/{}", self.account_id, self.role_name)),
        ))
    }
    fn name(&self) -> String {
        format!("SSO {} {}", self.account_id, self.role_name)
    }
}

// --- static and profiles ------------------------------------------------------------

pub struct StaticSource(pub Credentials);

#[async_trait::async_trait]
impl Source for StaticSource {
    async fn fetch(&self) -> Result<Option<Credentials>, String> {
        Ok(Some(self.0.clone()))
    }
    fn name(&self) -> String {
        self.0.source.clone()
    }
}

/// The source a profile describes. `depth` stops `source_profile` loops.
pub fn profile_source(
    files: &AwsFiles,
    name: &str,
    depth: usize,
) -> Result<Box<dyn Source>, String> {
    if depth > 5 {
        return Err(format!("profile {name}: source_profile chain too long"));
    }
    let p = files.profile(name).ok_or_else(|| {
        format!("profile {name} not found in ~/.aws/config or ~/.aws/credentials")
    })?;
    let region = p.get("region").cloned().unwrap_or_default();
    if let Some(role) = p.get("role_arn") {
        let session_name = p
            .get("role_session_name")
            .cloned()
            .unwrap_or_else(|| format!("sonde-{}", Utc::now().timestamp()));
        if let Some(token_file) = p.get("web_identity_token_file") {
            return Ok(Box::new(WebIdentitySource {
                role_arn: role.clone(),
                token_file: token_file.into(),
                session_name,
                region,
            }));
        }
        let source: Box<dyn Source> = if let Some(src) = p.get("source_profile") {
            if src == name {
                // keys in the same profile
                static_from(&p, name)?
                    .ok_or_else(|| format!("profile {name}: no keys for its own source_profile"))?
            } else {
                profile_source(files, src, depth + 1)?
            }
        } else if let Some(cs) = p.get("credential_source") {
            match cs.as_str() {
                "Environment" => Box::new(EnvSource::default()),
                "Ec2InstanceMetadata" => Box::new(ImdsSource::default()),
                "EcsContainer" => Box::new(ContainerSource),
                other => {
                    return Err(format!(
                        "profile {name}: credential_source {other} is not supported"
                    ));
                }
            }
        } else {
            return Err(format!(
                "profile {name}: role_arn needs source_profile or credential_source"
            ));
        };
        return Ok(Box::new(AssumeRoleSource {
            role_arn: role.clone(),
            source,
            session_name,
            external_id: p.get("external_id").cloned(),
            region,
        }));
    }
    if let Some(cmd) = p.get("credential_process") {
        return Ok(Box::new(ProcessSource {
            command: cmd.clone(),
        }));
    }
    if let Some(s) = static_from(&p, name)? {
        return Ok(s);
    }
    let cache_dir = dirs::home_dir()
        .map(|h| h.join(".aws/sso/cache"))
        .unwrap_or_default();
    if let Some(session) = p.get("sso_session") {
        let s = files
            .sso_session(session)
            .ok_or_else(|| format!("profile {name}: sso-session {session} not found"))?;
        return Ok(Box::new(SsoSource {
            cache_key: session.clone(),
            region: s.get("sso_region").cloned().unwrap_or_default(),
            account_id: p.get("sso_account_id").cloned().unwrap_or_default(),
            role_name: p.get("sso_role_name").cloned().unwrap_or_default(),
            cache_dir,
        }));
    }
    if let Some(url) = p.get("sso_start_url") {
        return Ok(Box::new(SsoSource {
            cache_key: url.clone(),
            region: p.get("sso_region").cloned().unwrap_or_default(),
            account_id: p.get("sso_account_id").cloned().unwrap_or_default(),
            role_name: p.get("sso_role_name").cloned().unwrap_or_default(),
            cache_dir,
        }));
    }
    Err(format!(
        "profile {name} has no credentials (keys, credential_process, sso or role_arn)"
    ))
}

fn static_from(
    p: &std::collections::BTreeMap<String, String>,
    name: &str,
) -> Result<Option<Box<dyn Source>>, String> {
    match (p.get("aws_access_key_id"), p.get("aws_secret_access_key")) {
        (Some(id), Some(secret)) => Ok(Some(Box::new(StaticSource(
            Credentials::new(
                id.clone(),
                secret,
                p.get("aws_session_token").map(String::as_str),
            )
            .with_source(format!("profile {name}")),
        )))),
        (Some(_), None) => Err(format!(
            "profile {name}: aws_access_key_id without aws_secret_access_key"
        )),
        _ => Ok(None),
    }
}

/// The `host` a request to `endpoint` carries, which is what gets signed:
/// with the port when the URL names one (`localhost:4566`).
fn host_header(endpoint: &str) -> Option<String> {
    let u = url::Url::parse(endpoint).ok()?;
    let host = u.host_str()?;
    Some(match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    })
}

/// A step of the chain that could not be set up: it says why when asked.
struct FailedSource {
    name: String,
    error: String,
}

#[async_trait::async_trait]
impl Source for FailedSource {
    async fn fetch(&self) -> Result<Option<Credentials>, String> {
        Err(self.error.clone())
    }
    fn name(&self) -> String {
        self.name.clone()
    }
}

/// The AWS default chain.
pub struct ChainSource {
    pub sources: Vec<Box<dyn Source>>,
}

#[async_trait::async_trait]
impl Source for ChainSource {
    async fn fetch(&self) -> Result<Option<Credentials>, String> {
        let mut tried = Vec::new();
        for s in &self.sources {
            match s.fetch().await {
                Ok(Some(c)) => return Ok(Some(c)),
                Ok(None) => tried.push(format!("{}: none", s.name())),
                Err(e) => tried.push(format!("{}: {e}", s.name())),
            }
        }
        Err(format!("no credentials found ({})", tried.join("; ")))
    }
    fn name(&self) -> String {
        "default chain".into()
    }
}

pub fn default_chain() -> ChainSource {
    let mut sources: Vec<Box<dyn Source>> = vec![Box::new(EnvSource::default())];
    if let Some(w) = WebIdentitySource::from_env() {
        sources.push(Box::new(w));
    }
    let files = AwsFiles::load();
    let profile = std::env::var("AWS_PROFILE").unwrap_or_else(|_| "default".into());
    sources.push(profile_step(&files, &profile));
    sources.push(Box::new(ContainerSource));
    sources.push(Box::new(ImdsSource::default()));
    ChainSource { sources }
}

/// The chain's profile step. A profile that cannot be used stays in the
/// chain as a step that fails, so the chain's error says what it said.
fn profile_step(files: &AwsFiles, profile: &str) -> Box<dyn Source> {
    profile_source(files, profile, 0).unwrap_or_else(|error| {
        Box::new(FailedSource {
            name: format!("profile {profile}"),
            error,
        })
    })
}

/// The provider for an endpoint's configured source.
pub fn provider(source: &CredentialSource) -> Result<Arc<dyn CredentialsProvider>, String> {
    let (src, anonymous): (Box<dyn Source>, bool) = match source {
        CredentialSource::Default => (Box::new(default_chain()), false),
        CredentialSource::Profile { name } => (profile_source(&AwsFiles::load(), name, 0)?, false),
        CredentialSource::Env {
            access_key_id_var,
            secret_access_key_var,
            session_token_var,
        } => (
            Box::new(EnvSource {
                id_var: access_key_id_var.clone(),
                secret_var: secret_access_key_var.clone(),
                token_var: session_token_var.clone(),
            }),
            false,
        ),
        CredentialSource::Imds => (Box::new(ImdsSource::default()), false),
        CredentialSource::WebIdentity {
            role_arn,
            token_file,
            session_name,
        } => (
            Box::new(WebIdentitySource {
                role_arn: role_arn.clone(),
                token_file: token_file.clone(),
                session_name: session_name.clone().unwrap_or_else(|| "sonde".into()),
                region: std::env::var("AWS_REGION").unwrap_or_default(),
            }),
            false,
        ),
        CredentialSource::Process { command } => (
            Box::new(ProcessSource {
                command: command.clone(),
            }),
            false,
        ),
        CredentialSource::Static {
            access_key_id,
            secret_access_key,
        } => (
            Box::new(StaticSource(
                Credentials::new(access_key_id.clone(), secret_access_key, None)
                    .with_source("config file"),
            )),
            false,
        ),
        CredentialSource::Anonymous => {
            (Box::new(StaticSource(Credentials::new("", "", None))), true)
        }
    };
    Ok(Arc::new(Cached {
        source: src,
        cache: Mutex::new(None),
        anonymous,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> AwsFiles {
        AwsFiles {
            config: ini::parse(
                "[profile proc]\ncredential_process = printf '{\"Version\":1,\"AccessKeyId\":\"AKIPROC\",\"SecretAccessKey\":\"s\",\"SessionToken\":\"t\",\"Expiration\":\"2099-01-01T00:00:00Z\"}'\n[profile role]\nrole_arn = arn:aws:iam::1:role/r\nsource_profile = keys\n[profile loop]\nrole_arn = arn:aws:iam::1:role/r\nsource_profile = loop2\n[profile loop2]\nrole_arn = arn:aws:iam::1:role/r\nsource_profile = loop\n[profile sso]\nsso_session = s\nsso_account_id = 1\nsso_role_name = R\n[sso-session s]\nsso_region = us-east-1\n[profile empty]\nregion = x\n",
            ),
            credentials: ini::parse(
                "[keys]\naws_access_key_id = AKIKEYS\naws_secret_access_key = sec\n",
            ),
        }
    }

    #[tokio::test]
    async fn static_profile() {
        let s = profile_source(&files(), "keys", 0).unwrap();
        let c = s.fetch().await.unwrap().unwrap();
        assert_eq!(c.access_key_id, "AKIKEYS");
        assert_eq!(c.source, "profile keys");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn credential_process() {
        let s = profile_source(&files(), "proc", 0).unwrap();
        let c = s.fetch().await.unwrap().unwrap();
        assert_eq!(c.access_key_id, "AKIPROC");
        assert_eq!(c.session_token.unwrap().expose(), "t");
        assert!(c.expiry.is_some());
    }

    #[test]
    fn profile_kinds() {
        let f = files();
        assert!(
            profile_source(&f, "role", 0)
                .unwrap()
                .name()
                .starts_with("assume role arn:aws:iam::1:role/r via")
        );
        assert!(profile_source(&f, "loop", 0).err().is_some());
        assert!(
            profile_source(&f, "sso", 0)
                .unwrap()
                .name()
                .starts_with("SSO 1 R")
        );
        assert!(
            profile_source(&f, "empty", 0)
                .err()
                .unwrap()
                .contains("no credentials")
        );
        assert!(profile_source(&f, "missing", 0).err().is_some());
    }

    #[tokio::test]
    async fn sso_needs_login() {
        let dir = tempfile::tempdir().unwrap();
        let s = SsoSource {
            cache_key: "s".into(),
            region: "us-east-1".into(),
            account_id: "1".into(),
            role_name: "R".into(),
            cache_dir: dir.path().into(),
        };
        assert!(s.fetch().await.unwrap_err().contains("aws sso login"));
        std::fs::write(
            sso_cache_file(dir.path(), "s"),
            r#"{"accessToken":"x","expiresAt":"2000-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert!(s.fetch().await.unwrap_err().contains("expired"));
    }

    #[tokio::test]
    async fn imds_against_fake_server() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = if req.starts_with("PUT /latest/api/token") {
                    "TOKEN".to_string()
                } else if !req
                    .to_lowercase()
                    .contains("x-aws-ec2-metadata-token: token")
                {
                    String::from("unauthorized")
                } else if req.starts_with("GET /latest/meta-data/iam/security-credentials/ ") {
                    "my-role".into()
                } else {
                    r#"{"Code":"Success","AccessKeyId":"ASIAIMDS","SecretAccessKey":"s","Token":"t","Expiration":"2099-01-01T00:00:00Z"}"#.into()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                s.write_all(resp.as_bytes()).await.unwrap();
            }
        });
        let src = ImdsSource {
            endpoint: format!("http://{addr}"),
        };
        let c = src.fetch().await.unwrap().unwrap();
        assert_eq!(c.access_key_id, "ASIAIMDS");
        assert_eq!(c.source, "imds role my-role");
    }

    #[tokio::test]
    async fn anonymous_and_static() {
        let p = provider(&CredentialSource::Anonymous).unwrap();
        assert!(p.credentials().await.unwrap().is_none());
        let p = provider(&CredentialSource::Static {
            access_key_id: "a".into(),
            secret_access_key: "b".into(),
        })
        .unwrap();
        assert_eq!(p.credentials().await.unwrap().unwrap().access_key_id, "a");
        assert_eq!(p.describe(), "config file");
    }

    #[test]
    fn the_signed_sts_host_keeps_the_port() {
        assert_eq!(
            host_header("http://localhost:4566").as_deref(),
            Some("localhost:4566")
        );
        assert_eq!(
            host_header("https://sts.eu-west-1.amazonaws.com").as_deref(),
            Some("sts.eu-west-1.amazonaws.com")
        );
        // the scheme's own port is not in the Host header
        assert_eq!(
            host_header("https://sts.example.com:443/").as_deref(),
            Some("sts.example.com")
        );
    }

    #[tokio::test]
    async fn the_chain_says_why_the_profile_step_failed() {
        let chain = ChainSource {
            sources: vec![profile_step(&files(), "empty")],
        };
        let e = chain.fetch().await.unwrap_err();
        assert!(
            e.contains("profile empty has no credentials"),
            "the profile step is in the message: {e}"
        );
    }
}
