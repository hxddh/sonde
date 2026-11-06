//! Credentials as the signer needs them. Resolving them (profiles, SSO,
//! IMDS, web identity, ...) is `sonde-core`'s job; this module only holds
//! the values and keeps them out of `Debug`, `Display` and serialization.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};

/// A secret string: never printed, never serialized.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Arc<str>);

impl Secret {
    pub fn new(s: impl AsRef<str>) -> Self {
        Secret(Arc::from(s.as_ref()))
    }
    /// The value, for signing only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(reqlog::redact::MASK)
    }
}

/// Signing credentials.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub access_key_id: String,
    pub secret_access_key: Secret,
    pub session_token: Option<Secret>,
    pub expiry: Option<DateTime<Utc>>,
    /// Where they came from (`env`, `profile:dev`, `imds`, ...), for display.
    pub source: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field(
                "access_key_id",
                &reqlog::redact::mask_key_id(&self.access_key_id),
            )
            .field("secret_access_key", &self.secret_access_key)
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| reqlog::redact::MASK),
            )
            .field("expiry", &self.expiry)
            .field("source", &self.source)
            .finish()
    }
}

impl Credentials {
    pub fn new(id: impl Into<String>, secret: impl AsRef<str>, token: Option<&str>) -> Self {
        Credentials {
            access_key_id: id.into(),
            secret_access_key: Secret::new(secret),
            session_token: token.map(Secret::new),
            expiry: None,
            source: "static".into(),
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    pub fn with_expiry(mut self, expiry: Option<DateTime<Utc>>) -> Self {
        self.expiry = expiry;
        self
    }

    /// Whether they expire within `margin_secs`.
    pub fn expires_within(&self, margin_secs: i64) -> bool {
        self.expiry
            .is_some_and(|e| (e - Utc::now()).num_seconds() < margin_secs)
    }
}

/// Something that yields credentials, refreshing them as needed.
#[async_trait::async_trait]
pub trait CredentialsProvider: Send + Sync {
    /// `Ok(None)` means anonymous requests (no signature).
    async fn credentials(&self) -> Result<Option<Credentials>, String>;
    /// A short description for the UI ("profile dev (sso)").
    fn describe(&self) -> String;
}

/// Fixed credentials, or none at all.
pub struct StaticProvider(pub Option<Credentials>);

#[async_trait::async_trait]
impl CredentialsProvider for StaticProvider {
    async fn credentials(&self) -> Result<Option<Credentials>, String> {
        Ok(self.0.clone())
    }
    fn describe(&self) -> String {
        match &self.0 {
            Some(c) => c.source.clone(),
            None => "anonymous".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_secrets() {
        let c = Credentials::new("AKIAIOSFODNN7EXAMPLE", "SECRETKEY", Some("TOKEN"));
        let d = format!("{c:?}");
        assert!(
            !d.contains("SECRETKEY") && !d.contains("TOKEN") && !d.contains("AKIAIOSFODNN7EXAMPLE"),
            "{d}"
        );
    }
}
