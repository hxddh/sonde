//! Production read through its logs: `sonde import s3://bucket/prefix`,
//! what arrived since the last import of it, as one run.

use std::io::IsTerminal;
use std::sync::Arc;

use reqlog::Record;
use sonde_core::Sonde;
use sonde_proxy::episode::episodes;
use sonde_proxy::session::{Session, SessionHeader};

use crate::output;

use super::record::paint;

/// Where an import of logs got to: the newest log file's time read, and
/// the files read since a little before it, each with when it was written
/// (what its forgetting goes by).
#[derive(serde::Serialize, serde::Deserialize)]
struct LogsMark {
    watermark: chrono::DateTime<chrono::Utc>,
    seen: std::collections::BTreeMap<String, chrono::DateTime<chrono::Utc>>,
    /// The requests of uploads started and not finished yet: an upload
    /// whose completion is in a later pass's logs is read with it, not
    /// called unfinished in the pass it started in ([`hold_unfinished`]).
    #[serde(default)]
    pending: Vec<Record>,
}

/// How long an upload is waited for, in the logs' own time: past it, one
/// still not completed is unfinished.
const UPLOAD_WAITED: chrono::Duration = chrono::Duration::hours(24);

/// Takes out of `records` the requests of each multipart upload not
/// completed in them and started less than [`UPLOAD_WAITED`] before their
/// newest: what a later pass may finish. `records` are put in time order,
/// numbered from 1.
fn hold_unfinished(records: &mut Vec<Record>) -> Vec<Record> {
    records.sort_by_key(|r| r.time);
    for (i, r) in records.iter_mut().enumerate() {
        r.id = i as u64 + 1;
    }
    let Some(newest) = records.last().map(|r| r.time) else {
        return Vec::new();
    };
    let held: std::collections::HashSet<u64> = episodes(records)
        .iter()
        .filter(|e| {
            e.shape == sonde_proxy::episode::Shape::Multipart
                && e.title().ends_with("(not completed)")
                && newest - e.start < UPLOAD_WAITED
        })
        .flat_map(|e| e.records.iter().copied())
        .collect();
    let (out, kept): (Vec<Record>, Vec<Record>) = std::mem::take(records)
        .into_iter()
        .partition(|r| held.contains(&r.id));
    *records = kept;
    out
}

/// How long before the mark a file is still listed, and so still remembered.
const LOGS_GRACE: chrono::Duration = chrono::Duration::hours(2);

impl LogsMark {
    /// Whether a file written at `at` is to be read: new enough to be looked
    /// at, and not read yet.
    fn wants(&self, key: &str, at: chrono::DateTime<chrono::Utc>) -> bool {
        at > self.watermark - LOGS_GRACE && !self.seen.contains_key(key)
    }

    /// A file read (or given up on): not read again.
    fn read(&mut self, key: &str, at: chrono::DateTime<chrono::Utc>) {
        self.seen.insert(key.to_string(), at);
    }

    /// Forgets the files too old to be listed again, and only those: one
    /// still within the grace would be read a second time.
    fn forget_old(&mut self) {
        let keep_after = self.watermark - LOGS_GRACE;
        self.seen.retain(|_, at| *at > keep_after);
    }
}

/// What a first import of a prefix reads: the logs written in the last day,
/// not its whole history.
const FIRST_IMPORT: chrono::Duration = chrono::Duration::hours(24);

/// What an import exits with: the listing failed, or files were listed and
/// not one could be read (nothing was read: 70), else 0. "None read" is also
/// a prefix with no files, or none a log.
fn exit_of(listing_failed: bool, none_read: bool) -> i32 {
    if listing_failed || none_read { 70 } else { 0 }
}

/// `sonde import s3://bucket/prefix`: the log files written since the last
/// import of it (through the endpoint's credentials, GET and LIST only),
/// kept as a run of the program `logs of s3://…` and shown. Where it got
/// to is kept, so the next import goes on from there.
pub(super) async fn import_logs(
    s: &Arc<Sonde>,
    endpoint: Option<&str>,
    src: &str,
) -> anyhow::Result<i32> {
    let Some((bucket, prefix)) = src
        .strip_prefix("s3://")
        .map(|r| r.split_once('/').unwrap_or((r, "")))
        .filter(|(b, _)| !b.is_empty())
    else {
        return Err(super::usage("logs are read from s3://bucket/prefix"));
    };
    let ep = super::endpoint(s, endpoint)?;
    let client = s.client(&ep)?;
    let dir = s.paths().runs();
    std::fs::create_dir_all(&dir)?;
    let key = format!("{}s3://{bucket}/{prefix}", sonde_proxy::program::LOGS_OF);
    // where the last import of the source got to, as the folder's index
    // keeps it: the newest file's time read, and the files read near it
    // (logs arrive late, and prefixes of regions or days list in any order,
    // so a time and the files seen, not the last key listed)
    let mut mark: LogsMark = sonde_proxy::index::of(&dir)
        .folder
        .marks
        .get(&key)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| LogsMark {
            watermark: chrono::Utc::now() - FIRST_IMPORT,
            seen: Default::default(),
            pending: Vec::new(),
        });
    let color = std::io::stderr().is_terminal();
    let bang = paint("!", "33", color);
    // the files written since the mark (less a grace for late ones), not read yet
    let mut keys: Vec<(String, u64, chrono::DateTime<chrono::Utc>)> = Vec::new();
    let mut token = None;
    let mut files = 0usize;
    let listed = loop {
        let page = client
            .list_objects_v2(
                bucket,
                &s3raw::ops::ListV2 {
                    prefix: Some(prefix.to_string()),
                    continuation_token: token.clone(),
                    url_encoding: true,
                    ..Default::default()
                },
            )
            .await;
        let (page, _) = match page {
            Ok(p) => p,
            Err(e) => break Err(e),
        };
        files += page.contents.len();
        keys.extend(page.contents.iter().filter_map(|o| {
            let at = o
                .last_modified
                .as_deref()
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())?
                .with_timezone(&chrono::Utc);
            mark.wants(&o.key, at).then(|| (o.key.clone(), o.size, at))
        }));
        token = page.next_continuation_token.filter(|_| page.is_truncated);
        if token.is_none() {
            break Ok(());
        }
    };
    let listing_failed = listed.is_err();
    if let Err(e) = listed {
        eprintln!("{bang} listing {src}: {e}");
        keys.clear();
    }
    let mut records = Vec::new();
    // of the files asked for: how many answered, how many did not
    let (mut downloaded, mut unread, mut logs) = (0usize, 0usize, 0usize);
    for (k, size, at) in &keys {
        // a log file is small; something this large is not one
        if *size > 256 << 20 {
            mark.read(k, *at);
            continue;
        }
        let r = match client.get_object(bucket, k, None).await {
            Ok(r) => r,
            Err(e) => {
                // gone (a lifecycle rule) or not reachable now: tried again next time
                eprintln!("{bang} {k}: {e}");
                unread += 1;
                continue;
            }
        };
        downloaded += 1;
        let text = sonde_proxy::import::text_of(&r.body)
            .unwrap_or_else(|_| String::from_utf8_lossy(&r.body).into_owned());
        if let Ok(i) = sonde_proxy::import::import_counted(&text, k) {
            logs += 1;
            if i.malformed > 0 {
                eprintln!(
                    "{bang} {k}: {} line{} could not be read",
                    i.malformed,
                    if i.malformed == 1 { "" } else { "s" }
                );
            }
            records.extend(i.session.records);
        }
        mark.read(k, *at);
        if *at > mark.watermark {
            mark.watermark = *at;
        }
    }
    // every file failed: an import that read nothing did not find nothing
    // (no permission to GET, say); one file gone among many is not that
    let mut none_read = unread > 0 && downloaded == 0;
    if none_read {
        eprintln!(
            "{bang} none of the {unread} log file{} listed under {src} could be read",
            if unread == 1 { "" } else { "s" }
        );
    } else if !listing_failed && files == 0 {
        // a prefix with nothing under it is a wrong prefix, not a quiet hour
        none_read = true;
        eprintln!("{bang} no files under {src}: is it where the logs are delivered?");
    } else if downloaded > 0 && logs == 0 {
        none_read = true;
        eprintln!(
            "{bang} none of the {} read under {src} is an S3 access log or CloudTrail file",
            output::count(downloaded, "file")
        );
    } else if !listing_failed && keys.is_empty() {
        eprintln!("nothing new under {src} since the last import");
    }
    // uploads the imports before left open are read with this one's; the
    // ones still open wait for the next
    if !records.is_empty() {
        records.splice(0..0, std::mem::take(&mut mark.pending));
        mark.pending = hold_unfinished(&mut records);
    }
    // the seen files old enough to be past the grace are forgotten
    mark.forget_old();
    if let Ok(v) = serde_json::to_value(&mark) {
        let _ = sonde_proxy::index::update(&dir, |_| {
            (
                vec![(key.clone(), sonde_proxy::index::Change::Mark(Some(v)))],
                (),
            )
        });
    }
    let code = exit_of(listing_failed, none_read);
    if records.is_empty() {
        return Ok(code);
    }
    // the requests are the endpoint's: priced and checked as its
    for r in records.iter_mut() {
        r.endpoint = ep.as_str().into();
    }
    records.sort_by_key(|r| r.time);
    for (i, r) in records.iter_mut().enumerate() {
        r.id = i as u64 + 1;
    }
    let mut header = SessionHeader::new(&ep, "", "", Some(key.clone()));
    header.created = chrono::Utc::now();
    let path = sonde_proxy::session::fresh_path(
        &dir,
        &format!(
            "{}-logs-{bucket}",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ),
    );
    let session = Session {
        header,
        records,
        logs: Vec::new(),
    };
    sonde_proxy::import::save(&session, &path)?;
    drop(session);
    super::record::show_file(s, &path).await?;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonde_proxy::episode::observations;

    /// One import of the logs at `src` through `endpoint`.
    async fn import(s: &Arc<Sonde>, endpoint: &str, src: &str) -> i32 {
        import_logs(s, Some(endpoint), src).await.unwrap()
    }

    #[tokio::test]
    async fn watching_logs_keeps_each_pass_as_a_run_of_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap());
        sonde_core::demo::install(&s).await.unwrap();
        // an access log written into the bucket
        let log = "79a59df900b949e55d96a1e698fbacedfd6e09d98eacf8f8d5218e7cd47ef2be demo [06/Feb/2026:00:00:38 +0000] 192.0.2.3 - 3E57427F3EXAMPLE REST.GET.OBJECT data/a.csv \"GET /demo/data/a.csv HTTP/1.1\" 200 - 113 113 7 6 \"-\" \"boto3/1.34\" - x SigV4 ECDHE TLSv1.2 - -\n\
                   79a59df900b949e55d96a1e698fbacedfd6e09d98eacf8f8d5218e7cd47ef2be demo [06/Feb/2026:00:00:39 +0000] 192.0.2.3 - 3E57427F3EXAMPLF REST.HEAD.OBJECT data/missing \"HEAD /demo/data/missing HTTP/1.1\" 404 NoSuchKey - - 4 4 \"-\" \"boto3/1.34\" - x SigV4 ECDHE TLSv1.2 - -\n";
        s.client("demo")
            .unwrap()
            .put_object(
                "demo",
                "logs/2026-02-06-00-00-40-AB",
                bytes::Bytes::from(log),
                &[],
            )
            .await
            .unwrap();
        assert_eq!(import(&s, "demo", "s3://demo/logs/").await, 0);
        let sessions: Vec<_> = std::fs::read_dir(s.paths().runs())
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "sonde"))
            .collect();
        assert_eq!(sessions.len(), 1);
        let session = Session::open(&sessions[0]).unwrap();
        assert_eq!(session.records.len(), 2);
        assert_eq!(
            session.header.command.as_deref(),
            Some("logs of s3://demo/logs/")
        );
        // a second pass has nothing new: no second session
        assert_eq!(import(&s, "demo", "s3://demo/logs/").await, 0);
        let count = || {
            std::fs::read_dir(s.paths().runs())
                .unwrap()
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "sonde"))
                .count()
        };
        assert_eq!(count(), 1);
        // a file that lists before the ones read (another region, a late
        // delivery) is still read
        s.client("demo")
            .unwrap()
            .put_object(
                "demo",
                "logs/0-earlier/2026-02-06-00-00-41-CD",
                bytes::Bytes::from(log),
                &[],
            )
            .await
            .unwrap();
        assert_eq!(import(&s, "demo", "s3://demo/logs/").await, 0);
        assert_eq!(count(), 2);
    }

    /// An upload started in one pass's logs and completed in the next is
    /// read in the second, whole: never warned of as unfinished.
    #[tokio::test]
    async fn an_upload_completed_in_a_later_pass_is_not_unfinished() {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap());
        sonde_core::demo::install(&s).await.unwrap();
        let line = |at: &str, op: &str, uri: &str| {
            format!(
                "79a5 demo [06/Feb/2026:00:{at} +0000] 192.0.2.3 - 3E57 {op} big.bin \"{uri} HTTP/1.1\" 200 - 100 100 7 6 \"-\" \"boto3/1.34\" - x SigV4 ECDHE TLSv1.2 - -\n"
            )
        };
        let put = |name: &str, text: String| {
            let c = s.client("demo").unwrap();
            let name = name.to_string();
            async move {
                c.put_object("demo", &name, bytes::Bytes::from(text), &[])
                    .await
                    .unwrap();
            }
        };
        put(
            "uplogs/2026-02-06-00-01-00-AB",
            line("00:38", "REST.POST.UPLOADS", "POST /demo/big.bin?uploads")
                + &line(
                    "00:39",
                    "REST.PUT.PART",
                    "PUT /demo/big.bin?partNumber=1&uploadId=u1",
                )
                + &line("00:40", "REST.GET.OBJECT", "GET /demo/other"),
        )
        .await;
        assert_eq!(import(&s, "demo", "s3://demo/uplogs/").await, 0);
        let sessions = || {
            let mut all: Vec<_> = std::fs::read_dir(s.paths().runs())
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "sonde"))
                .collect();
            all.sort();
            all
        };
        let unfinished = |path: &std::path::Path| {
            let session = Session::open(path).unwrap();
            let eps = episodes(&session.records);
            let obs = observations(&session.records, &eps);
            (
                session.records.len(),
                obs.iter().any(|o| o.phrases[0].key == "obs.unfinished"),
            )
        };
        let first = sessions();
        assert_eq!(first.len(), 1);
        assert_eq!(unfinished(&first[0]), (1, false), "the upload waits");
        put(
            "uplogs/2026-02-06-00-02-00-CD",
            line(
                "01:10",
                "REST.POST.UPLOAD",
                "POST /demo/big.bin?uploadId=u1",
            ),
        )
        .await;
        assert_eq!(import(&s, "demo", "s3://demo/uplogs/").await, 0);
        let all = sessions();
        assert_eq!(all.len(), 2);
        let second = all.iter().find(|p| **p != first[0]).unwrap();
        assert_eq!(unfinished(second), (3, false), "read whole, finished");
    }

    #[test]
    fn hold_unfinished_lets_go_of_an_upload_waited_for_a_day() {
        let at = |h: i64| chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::hours(h);
        let rec = |h: i64, method: &str, op: &str, q: &[(&str, &str)]| Record {
            time: at(h),
            method: method.into(),
            kind: op.into(),
            bucket: Some("b".into()),
            key: Some("big".into()),
            status: Some(200),
            query: q.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
            ..Default::default()
        };
        let started = || rec(0, "POST", "CreateMultipartUpload", &[("uploads", "")]);
        let mut fresh = vec![started(), rec(1, "GET", "GetObject", &[])];
        assert_eq!(hold_unfinished(&mut fresh).len(), 1);
        assert_eq!(fresh.len(), 1);
        let mut stale = vec![started(), rec(30, "GET", "GetObject", &[])];
        assert!(
            hold_unfinished(&mut stale).is_empty(),
            "a day on: unfinished"
        );
        assert_eq!(stale.len(), 2);
    }

    #[test]
    fn an_import_of_logs_answers_for_itself() {
        assert_eq!(exit_of(false, false), 0);
        assert_eq!(exit_of(true, false), 70);
        assert_eq!(exit_of(false, true), 70);
    }

    #[test]
    fn a_log_file_read_is_remembered_until_it_is_too_old_to_be_listed() {
        let t = |h: i64| chrono::DateTime::from_timestamp(1_770_000_000 + h * 3600, 0).unwrap();
        let mut mark = LogsMark {
            watermark: t(0),
            seen: Default::default(),
            pending: Vec::new(),
        };
        // more files than the old limit, all within the grace
        for i in 0..50_010 {
            mark.read(&format!("logs/{i:06}"), t(0));
        }
        // a pass with one new file, an hour on
        assert!(mark.wants("logs/new", t(1)));
        mark.read("logs/new", t(1));
        mark.watermark = t(1);
        mark.forget_old();
        assert_eq!(mark.seen.len(), 50_011);
        assert!(!mark.wants("logs/000000", t(0)), "read a second time");
        // two hours past them they are not listed, so not remembered
        mark.read("logs/later", t(3));
        mark.watermark = t(3);
        mark.forget_old();
        assert_eq!(mark.seen.len(), 1);
        assert!(!mark.wants("logs/000000", t(0)));
    }

    #[tokio::test]
    async fn a_single_pass_that_could_not_list_the_logs_does_not_exit_0() {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap());
        sonde_core::demo::install(&s).await.unwrap();
        let src = "s3://no-such-bucket-of-logs/logs/";
        assert_eq!(import(&s, "demo", src).await, 70);
    }

    /// A store that lists `n` log files (0 or 2) and refuses to hand over any.
    async fn store_that_lists_and_refuses_gets(n: u8) -> std::net::SocketAddr {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    loop {
                        let mut chunk = [0u8; 4096];
                        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            match sock.read(&mut chunk).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                            }
                        }
                        let head = String::from_utf8_lossy(&buf).into_owned();
                        buf.clear();
                        let (status, body) = if head.contains("list-type=2") {
                            let now = chrono::Utc::now().to_rfc3339();
                            let file = |n: u8| {
                                format!(
                                    "<Contents><Key>logs/2026-02-06-00-00-4{n}-AB</Key><LastModified>{now}</LastModified><ETag>\"x\"</ETag><Size>300</Size></Contents>"
                                )
                            };
                            (
                                "200 OK",
                                format!(
                                    "<ListBucketResult><Name>lake</Name><Prefix>logs/</Prefix><KeyCount>{n}</KeyCount><IsTruncated>false</IsTruncated>{}</ListBucketResult>",
                                    (0..n).map(file).collect::<String>()
                                ),
                            )
                        } else {
                            (
                                "403 Forbidden",
                                "<Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>".to_string(),
                            )
                        };
                        let resp = format!(
                            "HTTP/1.1 {status}\r\ncontent-type: application/xml\r\ncontent-length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        if sock.write_all(resp.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn a_single_pass_that_could_read_none_of_the_files_listed_does_not_exit_0() {
        for n in [2, 0] {
            assert_eq!(pass_over(n).await, 70, "{n} files");
        }
    }

    async fn pass_over(n: u8) -> i32 {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap());
        let addr = store_that_lists_and_refuses_gets(n).await;
        let mut e = sonde_core::EndpointConfig::new(
            "refusing",
            "refusing",
            &format!("http://{addr}"),
            "us-east-1",
            sonde_core::Vendor::Minio,
        );
        e.path_style = true;
        e.credentials = sonde_core::CredentialSource::Static {
            access_key_id: "k".into(),
            secret_access_key: "s".into(),
        };
        s.add_transient_endpoint(e).unwrap();
        let src = "s3://lake/logs/";
        import(&s, "refusing", src).await
    }
}
