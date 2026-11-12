//! TLS on the local endpoint, for SDKs that will not speak plain HTTP.
//!
//! The first run makes a certificate authority of its own, issues one
//! certificate for `localhost`, `*.localhost`, `127.0.0.1` and `::1` with
//! it, and forgets the authority's key: nothing else can ever be issued in
//! its name, so trusting it trusts this endpoint alone. The certificate's
//! key is kept (0600) in the folder it was made in, and made again when the
//! certificate is near its end.
//!
//! The program is given a bundle to trust: the machine's own roots and the
//! authority, so that what it reaches besides sonde is trusted as before.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// A certificate's life, in days: under the 825 some platforms allow a
/// server's certificate, and made again once this old.
const LIFE_DAYS: i64 = 800;
const RENEW_DAYS: u64 = 760;

/// The endpoint's certificate, ready to serve.
pub struct Tls {
    pub config: Arc<rustls::ServerConfig>,
    /// The authority to trust, alone (NODE_EXTRA_CA_CERTS adds it).
    pub ca: PathBuf,
    /// The machine's roots and the authority (for variables that replace
    /// the roots: AWS_CA_BUNDLE, SSL_CERT_FILE, REQUESTS_CA_BUNDLE).
    pub bundle: PathBuf,
}

impl Tls {
    /// The certificate kept in `dir`, made there first if it is missing or
    /// near its end.
    pub fn load_or_make(dir: &Path) -> Result<Tls, String> {
        let (ca, cert, key) = (
            dir.join("ca.pem"),
            dir.join("cert.pem"),
            dir.join("key.pem"),
        );
        let fresh = std::fs::metadata(&cert)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age.as_secs() < RENEW_DAYS * 86_400);
        if !(fresh && ca.is_file() && key.is_file()) {
            make(dir)?;
        }
        let chain: Vec<CertificateDer<'static>> = [&cert, &ca]
            .iter()
            .map(|p| CertificateDer::from_pem_file(p).map_err(|e| format!("{}: {e}", p.display())))
            .collect::<Result<_, _>>()?;
        let private =
            PrivateKeyDer::from_pem_file(&key).map_err(|e| format!("{}: {e}", key.display()))?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
            .with_single_cert(chain, private)
            .map_err(|e| e.to_string())?;
        // HTTP/1.1 only: the proxy serves no other
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let bundle = dir.join("bundle.pem");
        write_bundle(&ca, &bundle)?;
        Ok(Tls {
            config: Arc::new(config),
            ca,
            bundle,
        })
    }

    /// Variables that make a program trust the endpoint.
    pub fn env(&self) -> Vec<(String, String)> {
        let bundle = self.bundle.to_string_lossy().into_owned();
        let ca = self.ca.to_string_lossy().into_owned();
        [
            ("AWS_CA_BUNDLE", &bundle),
            ("SSL_CERT_FILE", &bundle),
            ("REQUESTS_CA_BUNDLE", &bundle),
            ("CURL_CA_BUNDLE", &bundle),
            ("NODE_EXTRA_CA_CERTS", &ca),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }
}

/// A new authority, the endpoint's certificate issued by it, and its key.
/// The authority's key is never written.
fn make(dir: &Path) -> Result<(), String> {
    use rcgen::{
        BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
        KeyUsagePurpose,
    };
    let err = |e: rcgen::Error| e.to_string();
    use chrono::Datelike;
    let day = |d: chrono::DateTime<chrono::Utc>| {
        rcgen::date_time_ymd(d.year(), d.month() as u8, d.day() as u8)
    };
    let now = chrono::Utc::now();
    let (since, until) = (
        day(now - chrono::Duration::days(1)),
        day(now + chrono::Duration::days(LIFE_DAYS)),
    );

    let ca_key = KeyPair::generate().map_err(err)?;
    let mut ca = CertificateParams::new(Vec::<String>::new()).map_err(err)?;
    ca.distinguished_name
        .push(DnType::CommonName, "sonde local endpoint");
    ca.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    (ca.not_before, ca.not_after) = (since, until);
    let ca_cert = ca.self_signed(&ca_key).map_err(err)?;

    let key = KeyPair::generate().map_err(err)?;
    let mut leaf = CertificateParams::new(vec![
        "localhost".to_string(),
        "*.localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ])
    .map_err(err)?;
    leaf.distinguished_name
        .push(DnType::CommonName, "localhost");
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    (leaf.not_before, leaf.not_after) = (since, until);
    let cert = leaf.signed_by(&key, &ca_cert, &ca_key).map_err(err)?;

    create_private_dir(dir)?;
    write_file(&dir.join("key.pem"), key.serialize_pem().as_bytes(), true)?;
    write_file(&dir.join("ca.pem"), ca_cert.pem().as_bytes(), false)?;
    // the certificate last: its time is what says the three are fresh
    write_file(&dir.join("cert.pem"), cert.pem().as_bytes(), false)
}

/// The machine's roots (as far as they can be read) and the authority.
fn write_bundle(ca: &Path, bundle: &Path) -> Result<(), String> {
    use base64::Engine;
    let mut text = String::new();
    for c in rustls_native_certs::load_native_certs().certs {
        let b64 = base64::engine::general_purpose::STANDARD.encode(c.as_ref());
        text.push_str("-----BEGIN CERTIFICATE-----\n");
        for line in b64.as_bytes().chunks(64) {
            text.push_str(std::str::from_utf8(line).unwrap_or_default());
            text.push('\n');
        }
        text.push_str("-----END CERTIFICATE-----\n");
    }
    text.push_str(&std::fs::read_to_string(ca).map_err(|e| format!("{}: {e}", ca.display()))?);
    write_file(bundle, text.as_bytes(), false)
}

fn create_private_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Writes a file whole, through a new file renamed over it (two runs at
/// once never read half of one); a private one is the owner's alone.
fn write_file(path: &Path, bytes: &[u8], private: bool) -> Result<(), String> {
    let wrong = |e: std::io::Error| format!("{}: {e}", path.display());
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.mode(if private { 0o600 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut f = open.open(&tmp).map_err(wrong)?;
    f.write_all(bytes).map_err(wrong)?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(wrong)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn made_once_kept_private_and_bundled_with_the_machines_roots() {
        let dir = tempfile::tempdir().unwrap();
        let at = dir.path().join("tls");
        let t = Tls::load_or_make(&at).unwrap();
        let cert = std::fs::read(at.join("cert.pem")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&at.join("key.pem")), 0o600);
            assert_eq!(mode(&at), 0o700);
        }
        // the authority's key is nowhere
        let names: Vec<String> = std::fs::read_dir(&at)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 4, "{names:?}");
        let bundle = std::fs::read_to_string(&t.bundle).unwrap();
        assert!(bundle.ends_with(&std::fs::read_to_string(&t.ca).unwrap()));
        // made again only when it is old
        Tls::load_or_make(&at).unwrap();
        assert_eq!(std::fs::read(at.join("cert.pem")).unwrap(), cert);
        let env = t.env();
        assert!(
            env.iter()
                .any(|(k, v)| k == "NODE_EXTRA_CA_CERTS" && v.ends_with("ca.pem"))
        );
        assert!(
            env.iter()
                .any(|(k, v)| k == "AWS_CA_BUNDLE" && v.ends_with("bundle.pem"))
        );
    }
}
