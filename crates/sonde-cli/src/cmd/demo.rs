//! `sonde run --demo` without a command: the tour. A small export job,
//! written out as the Python it stands for, is played by sonde request by
//! request to the built-in demo store and recorded; its finding is shown with
//! the line of code that sent it and the fix; then the job with that fix is
//! played and recorded again, and the fix loop says what the fix saved.

use std::path::PathBuf;
use std::sync::Arc;

use reqlog::RequestLog;
use s3raw::{Body, S3Request};
use sonde_core::Sonde;
use sonde_proxy::session::Session;

use super::record::*;

/// The command a tour's runs say they ran.
pub const TOUR: &str = "sonde run --demo";

/// The program the tour's runs are of: a name of its own, so that the fixed
/// run is compared with the first.
const PROGRAM: &str = "tour";

/// The job, as a person would write it with boto3. Each request the tour
/// sends names the line here that sends it (`x-sonde-caller`), as sonde's
/// own hook names a real program's.
const JOB: &str = r#"# The tour's export job: what `sonde run --demo` plays, request by request,
# to sonde's demo store, as boto3 sends them.
import boto3

s3 = boto3.client("s3")

def find_settings():
    for name in ["sonde-tour.toml", "tour.toml", "tour.yaml", "tour.json"]:
        try:
            return s3.head_object(Bucket="demo", Key=f"tour/{name}")  # @settings
        except s3.exceptions.ClientError:
            pass

def read_reports():
    rows = []
    for q in (1, 2, 3):
        key = f"reports/2026-q{q}.csv"
        if s3.head_object(Bucket="demo", Key=key):  # @head
            rows += s3.get_object(Bucket="demo", Key=key)["Body"].read().splitlines()  # @get
    return rows

def export(rows, place):
    s3.put_object(Bucket="demo", Key=f"{place}/export.csv", Body=b"".join(rows))  # @put
    pages = s3.get_paginator("list_objects_v2")
    return list(pages.paginate(Bucket="demo", Prefix=f"{place}/", PaginationConfig={"PageSize": 2}))  # @list

find_settings()
export(read_reports(), "tour/first")
"#;

/// The same job with the finding's fix: the GET alone answers whether the
/// report is there.
fn fixed_job() -> String {
    JOB.replace(
        "        if s3.head_object(Bucket=\"demo\", Key=key):  # @head\n            rows += ",
        "        rows += ",
    )
    .replace("tour/first", "tour/fixed")
}

/// The line of `job` that holds `mark`, and the function it is in.
fn caller(job: &str, file: &str, mark: &str) -> String {
    let mut function = "<module>";
    for (n, line) in job.lines().enumerate() {
        if let Some(name) = line.strip_prefix("def ") {
            function = name.split('(').next().unwrap_or(name);
        }
        if line.contains(mark) {
            return format!("{file}:{} {function}", n + 1);
        }
    }
    format!("{file}:? <module>")
}

/// A tour in one command: the job and the job fixed, each recorded as any
/// program's run is, the first shown with its findings, the second against
/// the first.
pub async fn tour(s: &Arc<Sonde>, out: Option<PathBuf>) -> anyhow::Result<i32> {
    let ep = sonde_core::demo::install(s)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let dir = s.paths().data_dir.join("tour");
    std::fs::create_dir_all(&dir)?;
    let (first_py, fixed_py) = (dir.join("export.py"), dir.join("export-fixed.py"));
    std::fs::write(&first_py, JOB)?;
    std::fs::write(&fixed_py, fixed_job())?;
    println!(
        "The tour: a small export job ({}), played by sonde request by request as boto3 sends them, to sonde's built-in demo store.",
        first_py.display()
    );
    let fixed_out = out.as_ref().map(|o| {
        let stem = o
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "tour".into());
        o.with_file_name(format!("{stem}-fixed.sonde"))
    });
    let first = play(s, &ep.id, out, JOB, "export.py").await?;
    let color = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let session = Session::open(&first).map_err(|e| anyhow::anyhow!(e))?;
    let analysis =
        sonde_proxy::view::Analysis::of(&session, &request_prices_for(s, &session.header.endpoint));
    print_summary_of(&analysis, &session.header, &first, color, true, false, None);
    drop(session);
    println!(
        "\nThe same job with that fix ({}), played and recorded again:",
        fixed_py.display()
    );
    let fixed = play(s, &ep.id, fixed_out, &fixed_job(), "export-fixed.py").await?;
    let session = Session::open(&fixed).map_err(|e| anyhow::anyhow!(e))?;
    let analysis =
        sonde_proxy::view::Analysis::of(&session, &request_prices_for(s, &session.header.endpoint));
    let since = super::fixloop::since_last(s, &fixed, &session.header, &session.records, &analysis);
    println!("run: {}", fixed.display());
    if let Some(since) = &since {
        for line in super::fixloop::since_lines(since, color) {
            println!("{line}");
        }
    }
    Ok(0)
}

/// One run of `job` (from file `file`), to endpoint `ep`, recorded: the
/// run's path.
async fn play(
    s: &Arc<Sonde>,
    ep: &str,
    out: Option<PathBuf>,
    job: &str,
    file: &str,
) -> anyhow::Result<PathBuf> {
    use s3raw::{ClientConfig, Credentials, S3Client, StaticProvider};
    let rec = Recording {
        endpoint: Some(ep.to_string()),
        // the demo store is sonde's own, made for this tour
        policy: Some("allow".into()),
        out,
        tls: false,
    };
    let started = start_under(
        s,
        &rec,
        "127.0.0.1:0".parse()?,
        Some(TOUR.into()),
        false,
        &Of {
            name: Some(PROGRAM.into()),
            ..Default::default()
        },
    )
    .await?;
    let (key, secret) = started.proxy.client_keys();
    let mut cfg = ClientConfig::new("tour", &started.proxy.url(), "us-east-1")?;
    // the SDK the job is written for, so that each fix is said in its language
    cfg.user_agent = "Boto3/1.35.0 md/Botocore#1.35.0 lang/python#3.12 sonde-tour".into();
    let app = S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new(key, secret, None)))),
        RequestLog::default(),
    )?;
    let at = |mark: &str| caller(job, file, mark);
    let send = |r: S3Request, mark: &str| {
        let app = &app;
        let r = r.header(sonde_proxy::callers::CALLER_HEADER, at(mark));
        async move { app.send_raw(r).await.ok() }
    };
    let on =
        |op: &str, method: http::Method, k: &str| S3Request::new(op, method).bucket("demo").key(k);
    for name in ["sonde-tour.toml", "tour.toml", "tour.yaml", "tour.json"] {
        send(
            on("HeadObject", http::Method::HEAD, &format!("tour/{name}")),
            "@settings",
        )
        .await;
    }
    let fixed = !job.contains("@head");
    for q in 1..=3 {
        let k = format!("reports/2026-q{q}.csv");
        if !fixed {
            send(on("HeadObject", http::Method::HEAD, &k), "@head").await;
        }
        send(on("GetObject", http::Method::GET, &k), "@get").await;
    }
    // the export, under the run's own place
    let place = if fixed { "tour/fixed" } else { "tour/first" };
    send(
        on(
            "PutObject",
            http::Method::PUT,
            &format!("{place}/export.csv"),
        )
        .body(Body::from(vec![b'r'; 36 * 1024])),
        "@put",
    )
    .await;
    // and lists what it wrote, two keys a page
    let mut token: Option<String> = None;
    for _ in 0..20 {
        let page = send(
            S3Request::new("ListObjectsV2", http::Method::GET)
                .bucket("demo")
                .query("list-type", "2")
                .query("prefix", format!("{place}/"))
                .query("max-keys", "2")
                .query_opt("continuation-token", token.take()),
            "@list",
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
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the tour says of its job is true of the files it writes: each
    /// line it names holds the request it sends, and the fixed job is the
    /// job without its HEAD.
    #[test]
    fn the_tours_code_lines_are_the_lines_of_its_job() {
        let line = |job: &str, n: usize| job.lines().nth(n - 1).unwrap().to_string();
        let at = caller(JOB, "export.py", "@head");
        let (place, function) = at.split_once(' ').unwrap();
        assert_eq!(function, "read_reports");
        let n: usize = place.rsplit(':').next().unwrap().parse().unwrap();
        assert!(line(JOB, n).contains("s3.head_object"), "{at}");
        let fixed = fixed_job();
        assert!(!fixed.contains("head_object(Bucket=\"demo\", Key=key)"));
        let at = caller(&fixed, "export-fixed.py", "@get");
        let n: usize = at
            .split(' ')
            .next()
            .unwrap()
            .rsplit(':')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(line(&fixed, n).contains("s3.get_object"), "{at}");
    }
}
