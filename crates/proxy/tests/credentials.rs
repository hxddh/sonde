//! What a recorded program learns when sonde has no credentials for the
//! store: that, never the words of whatever failed to give them.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use reqlog::RequestLog;
use s3raw::{ClientConfig, Credentials, CredentialsProvider, S3Client, S3Request, StaticProvider};
use sonde_proxy::{Proxy, ProxyConfig};

/// A source of credentials that fails as a `credential_process` printing a
/// secret on its way out did before 33.0.
struct Failing(&'static str);

impl CredentialsProvider for Failing {
    fn credentials<'a, 'async_trait>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Credentials>, String>> + Send + 'async_trait>>
    where
        'a: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { Err(self.0.to_string()) })
    }

    fn describe(&self) -> String {
        "failing".into()
    }
}

async fn refused(error: &'static str) -> (String, String) {
    let mut cfg = ClientConfig::new("prod", "http://127.0.0.1:9", "us-east-1").unwrap();
    cfg.retry = s3raw::RetryPolicy::none();
    let target = S3Client::new(cfg, Arc::new(Failing(error)), RequestLog::default()).unwrap();
    let log = RequestLog::default();
    let proxy = Proxy::start(target, log.clone(), ProxyConfig::default())
        .await
        .unwrap();
    let (key, secret) = proxy.client_keys();
    let mut cfg = ClientConfig::new("app", &proxy.url(), "us-east-1").unwrap();
    cfg.retry = s3raw::RetryPolicy::none();
    let app = S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        RequestLog::default(),
    )
    .unwrap();
    let r = app
        .send_raw(
            S3Request::new("GetObject", http::Method::GET)
                .bucket("data")
                .key("k"),
        )
        .await
        .unwrap();
    assert_eq!(r.status, 500, "{}", r.text());
    let record = &log.list(&reqlog::Filter::default(), 1)[0];
    (
        r.text(),
        record.rare().transport_error.clone().unwrap_or_default(),
    )
}

#[tokio::test]
async fn a_program_is_told_sonde_has_no_credentials_and_nothing_the_source_said() {
    let (answer, kept) = refused(
        "no credentials found (profile prod: credential_process exited with exit status: 3: aws_secret_access_key=hunter2 in /home/me/.aws/vault; IMDSv2 token: timed out)",
    )
    .await;
    for text in [&answer, &kept] {
        for secret in [
            "hunter2",
            "/home/me",
            "vault",
            "IMDS",
            "aws_secret_access_key",
        ] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
    }
    assert!(answer.contains("<Code>SondeCredentials</Code>"), "{answer}");
    assert_eq!(
        kept,
        "sonde has no credentials for prod (credential_process: exit status: 3): `sonde endpoints test prod` says why"
    );
    assert!(
        answer.contains("(credential_process: exit status: 3)"),
        "{answer}"
    );

    // an exit status is a number: words where one should be are not said
    let (answer, kept) =
        refused("credential_process exited with exit status: hunter2 and more").await;
    assert!(!answer.contains("hunter2") && !kept.contains("hunter2"));
    assert!(!kept.contains("credential_process"), "{kept}");

    // any other source: that sonde has none, and where to ask why
    let (answer, kept) = refused("web identity token /var/run/secrets/token: denied").await;
    assert!(!answer.contains("/var/run") && !kept.contains("/var/run"));
    assert_eq!(
        kept,
        "sonde has no credentials for prod: `sonde endpoints test prod` says why"
    );
}
