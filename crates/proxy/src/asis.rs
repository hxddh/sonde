//! A request whose path the HTTP client would not send as written, sent as
//! it is.
//!
//! The URL library under the HTTP client folds `.` and `..` segments
//! (`a/../b` is sent as `b`, `%2E%2E` too) and writes some bytes its own way
//! (`\` as `/`, `|` as `%7C`), so the store would read another key than the
//! one the program named. S3 keeps `a/../b` as a key of its own. These
//! requests go on a connection of their own, HTTP/1.1, with the path as the
//! program wrote it: rare enough that no pool is kept for them.

use std::sync::Arc;

use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

/// Sends `method url` with `headers` (the `host` among them) and `body`,
/// the path as written. `ca_pem`: the endpoint's own authorities, beside the
/// machine's.
pub(crate) async fn send(
    method: &http::Method,
    url: &str,
    headers: &[(String, String)],
    body: reqwest::Body,
    ca_pem: Option<&[u8]>,
) -> Result<reqwest::Response, String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| format!("no scheme in {url}"))?;
    let at = rest.find('/').unwrap_or(rest.len());
    let (authority, target) = rest.split_at(at);
    let target = if target.is_empty() { "/" } else { target };
    let tls = match scheme {
        "https" => true,
        "http" => false,
        s => return Err(format!("{s}: not http or https")),
    };
    let (host, port) = host_port(authority, if tls { 443 } else { 80 })?;
    let mut req = http::Request::builder().method(method.clone()).uri(target);
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let req = req.body(body).map_err(|e| e.to_string())?;
    let tcp = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        TcpStream::connect((host.as_str(), port)),
    )
    .await
    .map_err(|_| format!("connecting to {authority}: no answer in 10 s"))?
    .map_err(|e| format!("connecting to {authority}: {e}"))?;
    let _ = tcp.set_nodelay(true);
    if !tls {
        return exchange(tcp, req).await;
    }
    let connector = tokio_rustls::TlsConnector::from(client_config(ca_pem)?);
    let name = rustls::pki_types::ServerName::try_from(host.clone())
        .map_err(|e| format!("{host}: {e}"))?;
    let stream = connector
        .connect(name, tcp)
        .await
        .map_err(|e| format!("TLS with {authority}: {e}"))?;
    exchange(stream, req).await
}

async fn exchange<S>(io: S, req: http::Request<reqwest::Body>) -> Result<reqwest::Response, String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(io))
        .await
        .map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let resp = sender.send_request(req).await.map_err(|e| e.to_string())?;
    let (parts, body) = resp.into_parts();
    let body = reqwest::Body::wrap_stream(body.into_data_stream());
    Ok(reqwest::Response::from(http::Response::from_parts(
        parts, body,
    )))
}

/// `host[:port]` or `[v6][:port]`.
fn host_port(authority: &str, default: u16) -> Result<(String, u16), String> {
    let bad = || format!("{authority}: not a host and port");
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']').ok_or_else(bad)?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().map_err(|_| bad())?,
            None => default,
        };
        return Ok((host.to_string(), port));
    }
    match authority.rsplit_once(':') {
        Some((h, p)) => Ok((h.to_string(), p.parse().map_err(|_| bad())?)),
        None => Ok((authority.to_string(), default)),
    }
}

/// The operating system verifies, as for sonde's other connections (D-227),
/// with the endpoint's own authorities (`--ca-file`) beside its roots.
fn client_config(ca_pem: Option<&[u8]>) -> Result<Arc<rustls::ClientConfig>, String> {
    use rustls::pki_types::CertificateDer;
    use rustls::pki_types::pem::PemObject;
    let extra: Vec<CertificateDer<'static>> = ca_pem
        .map(|pem| CertificateDer::pem_slice_iter(pem).flatten().collect())
        .unwrap_or_default();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier =
        rustls_platform_verifier::Verifier::new_with_extra_roots(extra, provider.clone())
            .map_err(|e| format!("the system's certificate verifier: {e}"))?;
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_and_its_port_are_read_as_written() {
        assert_eq!(host_port("s3:9000", 80).unwrap(), ("s3".into(), 9000));
        assert_eq!(host_port("s3", 443).unwrap(), ("s3".into(), 443));
        assert_eq!(host_port("[::1]:9", 80).unwrap(), ("::1".into(), 9));
        assert_eq!(host_port("[::1]", 80).unwrap(), ("::1".into(), 80));
        assert!(host_port("s3:x", 80).is_err());
    }

    /// An endpoint with its own authority (`--ca-file`), here a proxy of
    /// sonde's serving TLS: trusted with the file, beside the system's
    /// roots, and refused without it.
    #[tokio::test]
    async fn a_key_sent_as_is_trusts_the_ca_file_beside_the_systems() {
        let dir = tempfile::tempdir().unwrap();
        let tls = crate::tls::Tls::load_or_make(&dir.path().join("tls")).unwrap();
        let ca = std::fs::read(&tls.ca).unwrap();
        let cfg = s3raw::ClientConfig::new("t", "http://127.0.0.1:9", "eu-west-1").unwrap();
        let creds = s3raw::Credentials::new("k", "s", None);
        let client = s3raw::S3Client::new(
            cfg,
            Arc::new(s3raw::StaticProvider(Some(creds))),
            reqlog::RequestLog::default(),
        )
        .unwrap();
        let proxy = crate::Proxy::start(
            client,
            reqlog::RequestLog::default(),
            crate::ProxyConfig {
                tls: Some(Arc::new(tls)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let url = format!("{}/b/a/../k", proxy.url());
        let host = proxy.url().trim_start_matches("https://").to_string();
        let get = |ca: Option<Vec<u8>>| {
            let (url, host) = (url.clone(), host.clone());
            async move {
                let headers = [("host".to_string(), host)];
                send(
                    &http::Method::GET,
                    &url,
                    &headers,
                    reqwest::Body::from(""),
                    ca.as_deref(),
                )
                .await
            }
        };
        // any answer: the handshake passed
        assert!(get(Some(ca)).await.is_ok());
        let e = get(None).await.unwrap_err();
        assert!(e.starts_with("TLS with"), "{e}");
    }
}
