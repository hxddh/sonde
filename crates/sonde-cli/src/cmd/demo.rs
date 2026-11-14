//! `sonde demo`: a small program's requests to the demo store, recorded and shown.

use std::sync::Arc;

use clap::Args;
use reqlog::RequestLog;
use sonde_core::Sonde;

use crate::output::Format;

use super::record::*;
use super::show::{ShowArgs, show};

#[derive(Args)]
pub struct DemoArgs {
    /// Open the recorded session in the sonde app afterwards.
    #[arg(long)]
    app: bool,
}

/// A tour in one command: a small program's requests to the built-in demo
/// store, recorded as any program's would be, then shown with its findings.
pub async fn demo(s: &Arc<Sonde>, a: &DemoArgs) -> anyhow::Result<i32> {
    use s3raw::{Body, ClientConfig, Credentials, S3Client, S3Request, StaticProvider};
    let ep = sonde_core::demo::install(s)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let rec = Recording {
        endpoint: Some(ep.id.clone()),
        // the demo store is sonde's own, made for this tour
        policy: Some("allow".into()),
        out: None,
        tls: false,
    };
    let started = start(
        s,
        &rec,
        "127.0.0.1:0".parse()?,
        Some("sonde demo".into()),
        false,
    )
    .await?;
    let (key, secret) = started.proxy.client_keys();
    let app = S3Client::new(
        ClientConfig::new("demo-app", &started.proxy.url(), "us-east-1")?,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        RequestLog::default(),
    )?;
    let send = |r: S3Request| {
        let app = &app;
        async move { app.send_raw(r).await.ok() }
    };
    let on =
        |op: &str, method: http::Method, k: &str| S3Request::new(op, method).bucket("demo").key(k);
    let put = |k: &str, body: Vec<u8>| on("PutObject", http::Method::PUT, k).body(Body::from(body));
    // what a small export job does. it looks for its settings where they might be
    for name in ["sonde-tour.toml", "tour.toml", "tour.yaml", "tour.json"] {
        send(on(
            "HeadObject",
            http::Method::HEAD,
            &format!("tour/{name}"),
        ))
        .await;
    }
    // reads the year's reports, asking first whether each is there
    for q in 1..=3 {
        let k = format!("reports/2026-q{q}.csv");
        send(on("HeadObject", http::Method::HEAD, &k)).await;
        send(on("GetObject", http::Method::GET, &k)).await;
    }
    // uploads the export in parts
    let export = "tour/export.csv";
    let created =
        send(on("CreateMultipartUpload", http::Method::POST, export).query("uploads", "")).await;
    if let Some(id) = created.and_then(|r| s3raw::xml::initiate_multipart_upload(&r.text()).ok()) {
        let mut parts = Vec::new();
        for n in 1..=5u32 {
            let part = send(
                on("UploadPart", http::Method::PUT, export)
                    .query("partNumber", n.to_string())
                    .query("uploadId", &id.upload_id)
                    .body(Body::from(vec![b'0' + n as u8; 64 * 1024])),
            )
            .await;
            let etag = part.and_then(|r| r.header("etag").map(String::from));
            parts.push(format!(
                "<Part><PartNumber>{n}</PartNumber><ETag>{}</ETag></Part>",
                s3raw::xml::escape(&etag.unwrap_or_default())
            ));
        }
        send(
            on("CompleteMultipartUpload", http::Method::POST, export)
                .query("uploadId", &id.upload_id)
                .body(Body::from(format!(
                    "<CompleteMultipartUpload>{}</CompleteMultipartUpload>",
                    parts.concat()
                ))),
        )
        .await;
    }
    // writes the same rows as a Delta table: the log read, three files, one commit
    let table = "tour/table";
    send(
        S3Request::new("ListObjectsV2", http::Method::GET)
            .bucket("demo")
            .query("list-type", "2")
            .query("prefix", format!("{table}/_delta_log/")),
    )
    .await;
    send(on(
        "GetObject",
        http::Method::GET,
        &format!("{table}/_delta_log/_last_checkpoint"),
    ))
    .await;
    let mut adds = String::from("{\"protocol\":{\"minReaderVersion\":1,\"minWriterVersion\":2}}\n");
    for i in 0..3 {
        let name = format!("part-{i:05}-5f0c2a1e-tour-c000.snappy.parquet");
        send(put(&format!("{table}/{name}"), vec![b'P'; 8 * 1024])).await;
        adds.push_str(&format!(
            "{{\"add\":{{\"path\":\"{name}\",\"size\":8192,\"dataChange\":true}}}}\n"
        ));
    }
    send(
        put(
            &format!("{table}/_delta_log/{:020}.json", 0),
            adds.into_bytes(),
        )
        .header("if-none-match", "*"),
    )
    .await;
    // the store is busy for this one (the demo's store says so once): the client tries again
    send(put(
        sonde_core::demo::BUSY_ONCE,
        br#"{"rows":36,"files":3}"#.to_vec(),
    ))
    .await;
    // and lists what it wrote, a few keys a page
    let mut token: Option<String> = None;
    for _ in 0..20 {
        let page = send(
            S3Request::new("ListObjectsV2", http::Method::GET)
                .bucket("demo")
                .query("list-type", "2")
                .query("prefix", "tour/")
                .query("max-keys", "2")
                .query_opt("continuation-token", token.take()),
        )
        .await;
        token = page
            .and_then(|r| s3raw::xml::list_objects_v2(&r.text()).ok())
            .and_then(|l| l.next_continuation_token);
        if token.is_none() {
            break;
        }
    }
    let file = started.path.clone();
    drop(app);
    drop(started);
    let _ = sonde_proxy::session::compress(&file);
    eprintln!(
        "recorded the demo program in {}\n  sonde show {}    the session again\n  sonde run -- <your program>    your own program's requests\n",
        file.display(),
        file.display()
    );
    show(
        s,
        &ShowArgs {
            file: Some(file),
            summary: true,
            errors: false,
            requests: false,
            all: false,
            app: a.app,
            by: None,
            format: Format::Table,
        },
    )
    .await?;
    Ok(0)
}
