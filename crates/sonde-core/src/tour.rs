//! The tour's job: a small export job, written out as the Python it stands
//! for (boto3), that sonde plays request by request against the built-in
//! demo store. Each request names the line of the job that sends it, as
//! sonde's hooks name a real program's, so its finding says where to
//! change the code; the job with that change applied sends what the fixed
//! code would. `sonde run --demo` and the app's demo both play it.

use s3raw::{Body, S3Client, S3Request};

/// The job's file, where a tour writes it.
pub const FILE: &str = "export.py";

/// The command that plays the job in `FILE` (in the folder it runs in)
/// against `AWS_ENDPOINT_URL`: what the app's demo runs, and the CLI's
/// hidden `demo-job`.
pub const COMMAND: &str = "sonde demo-job export.py";

/// The name the job's runs are kept under, so that a run of it fixed is
/// compared with the one before.
pub const PROGRAM: &str = "tour";

/// The job, as a person would write it with boto3. The marks (`# @head`)
/// say which line sends which request.
pub const JOB: &str = r#"# The tour's export job: what sonde plays, request by request, to its
# built-in demo store, as boto3 sends them.
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

/// The same job with its finding's fix: the GET alone answers whether the
/// report is there.
pub fn fixed_job() -> String {
    JOB.replace(
        "        if s3.head_object(Bucket=\"demo\", Key=key):  # @head\n            rows += ",
        "        rows += ",
    )
    .replace("tour/first", "tour/fixed")
}

/// Whether `job` is the job fixed (it sends no HEAD before its GETs).
pub fn is_fixed(job: &str) -> bool {
    !job.contains("@head")
}

/// The line of `job` that holds `mark`, and the function it is in, as
/// sonde's hooks name a caller: `export.py:18 read_reports`.
pub fn caller(job: &str, file: &str, mark: &str) -> String {
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

/// The time between the starts of the job's requests.
const BEAT: std::time::Duration = std::time::Duration::from_millis(100);
/// Half the slice (`profile::Timeline::bin_ms`, 20 ms for a run this long) a
/// request after the first starts past its beat.
const HALF_SLICE: std::time::Duration = std::time::Duration::from_millis(10);

/// Plays `job` (from file `file`) with `app`, each request naming its
/// line in `caller_header`; `say` is told each request as it is answered,
/// a line each (`export.py:18 read_reports  HEAD demo/reports/2026-q1.csv → 200`).
pub async fn play(
    app: &S3Client,
    job: &str,
    file: &str,
    caller_header: &str,
    mut say: impl FnMut(String),
) {
    let at = |mark: &str| caller(job, file, mark);
    // each request starts on the job's own beat, not when the one before
    // answered: the run's timeline is the same picture each time it is played.
    // The beats count from the first request's start as the store saw it (its
    // answer, less the store's wait): not from before it, when its connection
    // was still being made, a time no two machines take alike
    let mut start: Option<tokio::time::Instant> = None;
    let mut beat = 0u32;
    let mut send = async |r: S3Request, mark: &str| {
        // (half a slice past the beat: mid-slice in the run's timeline, a
        // millisecond either side the same slice)
        if let Some(start) = start {
            crate::demo::until(start + BEAT * beat + HALF_SLICE).await;
        }
        beat += 1;
        let place = at(mark);
        let target = r.key.clone().unwrap_or_else(|| {
            let prefix = r.query.iter().find(|(k, _)| k == "prefix");
            format!("?prefix={}", prefix.map_or("", |(_, v)| v.as_str()))
        });
        let what = format!("{} demo/{target}", r.method);
        let got = app.send_raw(r.header(caller_header, place.clone())).await;
        let status = match &got {
            Ok(r) => r.status.to_string(),
            Err(e) => e
                .status
                .map_or_else(|| "no answer".into(), |s| s.to_string()),
        };
        if start.is_none() {
            // the first request is a HEAD (the job's settings)
            let now = tokio::time::Instant::now();
            start = Some(now.checked_sub(crate::demo::BRIEF).unwrap_or(now));
        }
        say(format!("{place}  {what} → {status}"));
        got.ok()
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
    let fixed = is_fixed(job);
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
}

/// The user agent the job's requests carry: boto3's, so that each fix is
/// said in its language.
pub const USER_AGENT: &str = "Boto3/1.35.0 md/Botocore#1.35.0 lang/python#3.12 sonde-tour";

/// A client to `url` with the keys a recording gave, as the job's boto3 is.
pub fn client(url: &str, key: &str, secret: &str) -> Result<S3Client, s3raw::S3Error> {
    use s3raw::{ClientConfig, Credentials, StaticProvider};
    let mut cfg = ClientConfig::new("tour", url, "us-east-1")?;
    cfg.user_agent = USER_AGENT.into();
    S3Client::new(
        cfg,
        std::sync::Arc::new(StaticProvider(Some(Credentials::new(
            key.to_string(),
            secret,
            None,
        )))),
        reqlog::RequestLog::default(),
    )
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
        let at = caller(JOB, FILE, "@head");
        let (place, function) = at.split_once(' ').unwrap();
        assert_eq!(function, "read_reports");
        let n: usize = place.rsplit(':').next().unwrap().parse().unwrap();
        assert!(line(JOB, n).contains("s3.head_object"), "{at}");
        let fixed = fixed_job();
        assert!(is_fixed(&fixed) && !is_fixed(JOB));
        assert!(!fixed.contains("head_object(Bucket=\"demo\", Key=key)"));
        let at = caller(&fixed, FILE, "@get");
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
        // the command names the file
        assert!(COMMAND.ends_with(FILE));
    }
}
