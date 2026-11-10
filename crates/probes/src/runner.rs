//! Runs probes against endpoints as jobs, one job per endpoint, all in
//! parallel. Each probe gets its own prefix under `sonde-probe/<run>/`,
//! everything it wrote is removed afterwards (versions, delete markers,
//! uploads, temporary buckets), and its requests are kept with its result.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use s3raw::{S3Client, S3Error};
use serde::{Deserialize, Serialize};
use sonde_core::guard::AuditEntry;
use sonde_core::{Action, JobCtx, Progress, Sonde};

use crate::model::{Needs, ProbeResult, Run, Steps, Verdict};
use crate::{Probe, ProbeCx};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RunOptions {
    /// The scratch bucket (must exist). Probes write under `sonde-probe/`.
    pub bucket: String,
    /// Probes may create (and remove) buckets of their own.
    pub allow_temp_buckets: bool,
    /// Run probes that wait in real time.
    pub include_slow: bool,
    /// Probe ids or id prefixes (`checksum.`); empty: all.
    pub only: Vec<String>,
    /// Per-probe time limit.
    pub timeout_secs: u64,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            bucket: String::new(),
            allow_temp_buckets: false,
            include_slow: true,
            only: vec![],
            timeout_secs: 120,
        }
    }
}

impl RunOptions {
    pub fn selects(&self, id: &str) -> bool {
        self.only.is_empty()
            || self
                .only
                .iter()
                .any(|o| id == o || id.starts_with(o.as_str()))
    }
}

/// Refuses while read-only mode is on: a run writes and deletes objects
/// under `sonde-probe/` (and, when allowed, creates and removes buckets).
/// What a UI calls before [`spawn`].
pub fn allowed(s: &Sonde) -> Result<(), S3Error> {
    if s.read_only() {
        return Err(S3Error::refused(
            "read-only mode is on: turn it off in Settings (`read-only`) to run probes (a run writes and deletes objects under sonde-probe/ in the scratch bucket, and buckets of its own when allowed)",
        ));
    }
    Ok(())
}

/// Starts one job per endpoint; their ids, in order.
pub fn spawn(s: &Arc<Sonde>, endpoints: &[String], opts: RunOptions) -> Vec<String> {
    endpoints
        .iter()
        .map(|ep| {
            let (s2, ep2, o) = (s.clone(), ep.clone(), opts.clone());
            // cooperative: a cancelled run still removes what its probe wrote
            s.jobs().spawn_cooperative(
                "probe",
                &format!("Probes on {ep}"),
                4,
                s.settings().rate_limit,
                move |ctx| async move {
                    let run = run_endpoint(&s2, &ep2, &o, &ctx).await?;
                    let _ = crate::store::save(s2.paths(), &run);
                    Ok(serde_json::to_value(run).unwrap_or_default())
                },
            )
        })
        .collect()
}

pub async fn run_endpoint(
    s: &Sonde,
    endpoint: &str,
    opts: &RunOptions,
    ctx: &JobCtx,
) -> Result<Run, S3Error> {
    let cfg = s
        .endpoint_config(endpoint)
        .ok_or_else(|| S3Error::config(format!("no endpoint {endpoint}")))?;
    if opts.bucket.is_empty() {
        return Err(S3Error::config("probes need a scratch bucket"));
    }
    let base = s
        .job_client(endpoint, ctx)?
        .with_retry(s3raw::RetryPolicy::none());
    let head = tokio::select! {
        r = base.head_bucket(&opts.bucket) => r,
        _ = ctx.cancel.cancelled() => return Err(S3Error::cancelled()),
    };
    head.map_err(|e| S3Error {
        message: format!("scratch bucket {}: {}", opts.bucket, e.message),
        ..e
    })?;
    let run_id = format!(
        "{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        &uuid::Uuid::new_v4().simple().to_string()[..6]
    );
    let started = Utc::now().to_rfc3339();
    let probes: Vec<Box<dyn Probe>> = crate::suite::all()
        .into_iter()
        .filter(|p| opts.selects(&p.meta().id))
        .collect();
    let total = probes.len() as u64;
    // a probe run writes and deletes: on record before it starts, as plans are
    let mut entry = AuditEntry {
        time: Utc::now(),
        plan: run_id.clone(),
        action: Action::RunProbes,
        endpoint: endpoint.into(),
        bucket: opts.bucket.clone(),
        items: probes.len(),
        succeeded: 0,
        failed: 0,
        errors: vec![],
        request_ids: vec![],
        targets: std::iter::once(format!("sonde-probe/{run_id}/"))
            .chain(
                opts.allow_temp_buckets
                    .then(|| format!("buckets sonde-probe-{}-*", run_id[..8].to_lowercase())),
            )
            .collect(),
        stage: Some("started".into()),
    };
    s.guard().audit(&entry).map_err(|e| {
        S3Error::refused(format!(
            "the audit log {} cannot be written ({e}): no probe was run",
            s.paths().audit_log().display()
        ))
    })?;
    let mut results = Vec::new();
    let mut server = None;
    // what cleanup could not remove, for the result, the job and the audit log
    let mut leftovers: Vec<String> = Vec::new();
    let mut cancelled = false;
    for (i, probe) in probes.iter().enumerate() {
        if ctx.is_cancelled() {
            cancelled = true;
            break;
        }
        let meta = probe.meta();
        ctx.progress(Progress {
            done: i as u64,
            total: Some(total),
            bytes: 0,
            message: meta.id.clone(),
        });
        let tag = format!("{}:{}", ctx.id, meta.id);
        let started = Instant::now();
        let result = if meta.needs.contains(&Needs::Slow) && !opts.include_slow {
            skipped(&meta, "slow probes are off")
        } else {
            let client = base.for_job(Some(tag.clone()), Some(ctx.throttle.clone()));
            let prefix = format!("sonde-probe/{run_id}/{}/", meta.id);
            let temp = opts
                .allow_temp_buckets
                .then(|| format!("sonde-probe-{}", run_id[..8].to_lowercase()));
            let cx = ProbeCx::new(client.clone(), &opts.bucket, &prefix, temp);
            // a cancel stops the probe where it stands; the cleanup below runs all the same
            let timed_out = tokio::select! {
                r = tokio::time::timeout(Duration::from_secs(opts.timeout_secs), probe.run(&cx)) => {
                    Some(r.is_err())
                }
                _ = ctx.cancel.cancelled() => None,
            };
            if timed_out.is_none() {
                ctx.progress(Progress {
                    done: i as u64,
                    total: Some(total),
                    bytes: 0,
                    message: format!("cancelled: cleaning up after {}", meta.id),
                });
            }
            let steps = cx.steps();
            let left = cleanup(&client, &opts.bucket, &prefix, &cx.temp_buckets()).await;
            leftovers.extend(left.iter().cloned());
            let Some(timed_out) = timed_out else {
                cancelled = true;
                break;
            };
            let mut j = probe.judge(&steps);
            if timed_out {
                j.verdict = Verdict::Error;
                j.summary = format!("timed out after {} s; {}", opts.timeout_secs, j.summary);
            }
            j.observations
                .extend(left.iter().map(|l| format!("left behind: {l}")));
            record_fixture(cfg.vendor.id(), &meta.id, j.verdict, &steps);
            let exchanges = s
                .log()
                .list(
                    &reqlog::Filter {
                        job: Some(tag.clone()),
                        ..Default::default()
                    },
                    500,
                )
                .into_iter()
                .rev()
                .collect::<Vec<_>>();
            if server.is_none() {
                server = exchanges
                    .iter()
                    .find_map(|r| r.header("server").map(String::from));
            }
            ProbeResult {
                id: meta.id.clone(),
                category: meta.category,
                verdict: j.verdict,
                summary: j.summary,
                observations: j.observations,
                steps: steps.0,
                exchanges,
                duration_ms: 0.0,
            }
        };
        results.push(ProbeResult {
            duration_ms: started.elapsed().as_secs_f64() * 1000.0,
            ..result
        });
    }
    entry.time = Utc::now();
    entry.stage = None;
    entry.failed = results
        .iter()
        .filter(|r| r.verdict == Verdict::Error)
        .count();
    entry.succeeded = results
        .iter()
        .filter(|r| !matches!(r.verdict, Verdict::Error | Verdict::Skipped))
        .count();
    entry.errors = leftovers
        .iter()
        .map(|l| format!("left behind: {l}"))
        .collect();
    if cancelled {
        entry.errors.insert(0, "cancelled".into());
    }
    if let Err(e) = s.guard().audit(&entry) {
        tracing::error!("audit log: {e}");
    }
    for l in &leftovers {
        tracing::warn!("probe run {run_id} left behind: {l}");
    }
    if cancelled {
        // a cancelled job has no result: its last message says what stayed
        if !leftovers.is_empty() {
            ctx.progress(Progress {
                done: results.len() as u64,
                total: Some(total),
                bytes: 0,
                message: format!("cancelled; left behind: {}", leftovers.join("; ")),
            });
        }
        return Err(S3Error::cancelled());
    }
    ctx.progress(Progress {
        done: total,
        total: Some(total),
        bytes: 0,
        message: if leftovers.is_empty() {
            "done".into()
        } else {
            format!("done; left behind: {}", leftovers.join("; "))
        },
    });
    Ok(Run {
        id: run_id,
        endpoint: endpoint.into(),
        endpoint_name: cfg.name.clone(),
        vendor: cfg.vendor.id().into(),
        server,
        started,
        finished: Utc::now().to_rfc3339(),
        sonde_version: env!("CARGO_PKG_VERSION").into(),
        results,
    })
}

fn skipped(meta: &crate::ProbeMeta, why: &str) -> ProbeResult {
    ProbeResult {
        id: meta.id.clone(),
        category: meta.category,
        verdict: Verdict::Skipped,
        summary: why.into(),
        observations: vec![],
        steps: vec![],
        exchanges: vec![],
        duration_ms: 0.0,
    }
}

/// Removes everything under `prefix` (every version and upload) and the
/// temporary buckets. What could not be removed, in words (empty: all gone).
pub async fn cleanup(c: &S3Client, bucket: &str, prefix: &str, temp: &[String]) -> Vec<String> {
    let mut left = purge(c, bucket, prefix).await;
    for b in temp {
        // Object Lock buckets keep locked versions: governance bypass
        let inside = purge(c, b, "").await;
        match c.delete_bucket(b).await {
            Ok(_) => {}
            // the objects inside are why: one line says both
            Err(e) if inside.is_empty() => left.push(format!("bucket {b} ({e})")),
            Err(e) => left.push(format!("bucket {b} with {} ({e})", inside.join(", "))),
        }
    }
    left
}

async fn purge(c: &S3Client, bucket: &str, prefix: &str) -> Vec<String> {
    let mut left = Vec::new();
    let mut stuck_uploads = 0usize;
    for _ in 0..20 {
        let Ok(l) = c
            .list_multipart_uploads(bucket, Some(prefix), None, None)
            .await
        else {
            break;
        };
        stuck_uploads = 0;
        for u in &l.upload {
            if c.abort_multipart_upload(bucket, &u.key, &u.upload_id)
                .await
                .is_err()
            {
                stuck_uploads += 1;
            }
        }
        if !l.is_truncated {
            break;
        }
    }
    if stuck_uploads > 0 {
        left.push(format!(
            "{stuck_uploads} multipart uploads under {bucket}/{prefix}"
        ));
    }
    for _ in 0..1000 {
        let objs: Vec<(String, Option<String>)> = match c
            .list_object_versions(bucket, Some(prefix), None, None, None)
            .await
        {
            Ok(v) => v
                .version
                .iter()
                .map(|x| (x.key.clone(), x.version_id.clone()))
                .chain(
                    v.delete_marker
                        .iter()
                        .map(|d| (d.key.clone(), d.version_id.clone())),
                )
                .collect(),
            Err(_) => match c.list_all(bucket, prefix, 1000).await {
                Ok(o) => o.into_iter().map(|o| (o.key, None)).collect(),
                Err(_) => break,
            },
        };
        if objs.is_empty() {
            break;
        }
        let mut progressed = false;
        let mut why = String::new();
        for chunk in objs.chunks(1000) {
            for (k, v) in chunk {
                let mut req = s3raw::S3Request::new("DeleteObject", http::Method::DELETE)
                    .bucket(bucket)
                    .key(k)
                    .header("x-amz-bypass-governance-retention", "true");
                if let Some(v) = v {
                    req = req.query("versionId", v.as_str());
                }
                match c.send(req).await {
                    Ok(_) => progressed = true,
                    Err(e) => why = e.to_string(),
                }
            }
        }
        if !progressed {
            // none of what is listed could be deleted: it stays
            left.push(format!(
                "{} objects or versions under {bucket}/{prefix} ({why})",
                objs.len()
            ));
            break;
        }
    }
    left
}

/// With `SONDE_RECORD_FIXTURES=<dir>`, writes each probe's steps as a
/// fixture for the judge tests.
fn record_fixture(vendor: &str, probe: &str, verdict: Verdict, steps: &Steps) {
    let Ok(dir) = std::env::var("SONDE_RECORD_FIXTURES") else {
        return;
    };
    let dir = std::path::Path::new(&dir).join(probe);
    let _ = std::fs::create_dir_all(&dir);
    let f = crate::store::Fixture {
        probe: probe.into(),
        vendor: vendor.into(),
        expect: verdict,
        note: String::new(),
        steps: steps
            .0
            .iter()
            .map(|s| crate::model::Step {
                record_id: None,
                headers: s
                    .headers
                    .iter()
                    .filter(|(k, _)| keep_header(k))
                    .cloned()
                    .collect(),
                ..s.clone()
            })
            .collect(),
    };
    let _ = std::fs::write(
        dir.join(format!("{vendor}.json")),
        serde_json::to_string_pretty(&f).unwrap_or_default(),
    );
}

fn keep_header(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    k.starts_with("x-amz-checksum")
        || k.starts_with("x-amz-meta")
        || k == "content-length"
        || k == "content-range"
        || k == "content-type"
        || k == "etag"
        || k == "x-amz-version-id"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicBool, Ordering};

    use parking_lot::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A store that keeps keys: PUT, DELETE and listings. A GET of an
    /// object never answers while `stall` is on; DELETE is refused while
    /// `deny_delete` is.
    #[derive(Default)]
    struct Fake {
        keys: Mutex<BTreeSet<String>>,
        stall: AtomicBool,
        deny_delete: AtomicBool,
    }

    async fn serve(fake: Arc<Fake>) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((c, _)) = l.accept().await {
                tokio::spawn(connection(fake.clone(), c));
            }
        });
        url
    }

    async fn connection(fake: Arc<Fake>, mut c: tokio::net::TcpStream) {
        let mut buf = Vec::new();
        loop {
            let end = loop {
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
                let mut chunk = [0u8; 8192];
                match c.read(&mut chunk).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            };
            let head = String::from_utf8_lossy(&buf[..end]).to_string();
            let length: usize = head
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse().ok())?
                })
                .unwrap_or(0);
            while buf.len() < end + length {
                let mut chunk = [0u8; 8192];
                match c.read(&mut chunk).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
            buf.drain(..end + length);
            let mut first = head.lines().next().unwrap_or("").split(' ');
            let (method, target) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            let path = path.trim_end_matches('/');
            let is_object = path.trim_start_matches('/').contains('/');
            let (status, body) = match (method, is_object) {
                ("PUT", true) => {
                    fake.keys.lock().insert(path.to_string());
                    (200, String::new())
                }
                ("DELETE", true) if fake.deny_delete.load(Ordering::SeqCst) => (
                    403,
                    "<Error><Code>AccessDenied</Code><Message>no</Message></Error>".to_string(),
                ),
                ("DELETE", true) => {
                    fake.keys.lock().remove(path);
                    (204, String::new())
                }
                ("GET" | "HEAD", true) => {
                    if fake.stall.load(Ordering::SeqCst) {
                        std::future::pending::<()>().await;
                    }
                    (200, "version-0".to_string())
                }
                ("GET", false) if query.contains("uploads") => (
                    200,
                    "<ListMultipartUploadsResult></ListMultipartUploadsResult>".to_string(),
                ),
                // no versions here: cleanup falls back to ListObjectsV2
                ("GET", false) if query.contains("versions") => (
                    501,
                    "<Error><Code>NotImplemented</Code><Message>no</Message></Error>".to_string(),
                ),
                ("GET", false) => {
                    let keys = fake.keys.lock();
                    let rows: String = keys
                        .iter()
                        .filter_map(|k| k.strip_prefix(&format!("{path}/")))
                        .map(|k| format!("<Contents><Key>{k}</Key><Size>1</Size></Contents>"))
                        .collect();
                    (200, format!("<ListBucketResult>{rows}</ListBucketResult>"))
                }
                _ => (200, String::new()),
            };
            let body = if method == "HEAD" {
                String::new()
            } else {
                body
            };
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-length: {}\r\netag: \"e\"\r\n\r\n{body}",
                body.len()
            );
            if c.write_all(reply.as_bytes()).await.is_err() {
                return;
            }
        }
    }

    async fn sonde_on(url: &str, dir: &std::path::Path) -> Arc<Sonde> {
        let s = Sonde::open(sonde_core::Paths::rooted(dir)).unwrap();
        let mut e = sonde_core::EndpointConfig::new(
            "fake",
            "fake",
            url,
            "us-east-1",
            sonde_core::Vendor::Other,
        );
        e.path_style = true;
        e.credentials = sonde_core::CredentialSource::Static {
            access_key_id: "AKIAFAKE".into(),
            secret_access_key: "fake".into(),
        };
        s.add_transient_endpoint(e).unwrap();
        s
    }

    fn one_probe() -> RunOptions {
        RunOptions {
            bucket: "scratch".into(),
            only: vec!["consistency.read-after-overwrite".into()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn probes_are_refused_while_read_only_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap();
        let e = allowed(&s).unwrap_err();
        assert_eq!(e.kind, s3raw::ErrorKind::Refused);
        assert!(e.message.contains("read-only"), "{e}");
        let mut st = s.settings();
        st.read_only = false;
        s.set_settings(st).unwrap();
        assert!(allowed(&s).is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_cancelled_run_removes_what_its_probe_wrote() {
        let fake = Arc::new(Fake::default());
        fake.stall.store(true, Ordering::SeqCst);
        let dir = tempfile::tempdir().unwrap();
        let s = sonde_on(&serve(fake.clone()).await, dir.path()).await;
        let ids = spawn(&s, &["fake".to_string()], one_probe());
        // the probe wrote its object and waits on the GET that never answers
        for _ in 0..500 {
            if !fake.keys.lock().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(fake.keys.lock().len(), 1, "the probe wrote");
        assert!(s.jobs().cancel(&ids[0]));
        let e = tokio::time::timeout(Duration::from_secs(20), s.jobs().wait(&ids[0]))
            .await
            .expect("the job ended")
            .unwrap_err();
        assert_eq!(e.kind, s3raw::ErrorKind::Cancelled);
        assert!(fake.keys.lock().is_empty(), "nothing left behind");
        // on record: the start, then the cut-short outcome
        let log = s.guard().audit_entries(10);
        assert_eq!(log.len(), 2);
        assert_eq!(log[1].stage.as_deref(), Some("started"));
        assert_eq!(log[0].action, Action::RunProbes);
        assert_eq!(log[0].errors, vec!["cancelled"]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn what_cleanup_cannot_remove_is_reported() {
        let fake = Arc::new(Fake::default());
        fake.deny_delete.store(true, Ordering::SeqCst);
        let dir = tempfile::tempdir().unwrap();
        let s = sonde_on(&serve(fake.clone()).await, dir.path()).await;
        let ids = spawn(&s, &["fake".to_string()], one_probe());
        let v = tokio::time::timeout(Duration::from_secs(30), s.jobs().wait(&ids[0]))
            .await
            .expect("the job ended")
            .unwrap();
        let run: Run = serde_json::from_value(v).unwrap();
        let r = &run.results[0];
        assert!(
            r.observations
                .iter()
                .any(|o| o
                    .starts_with("left behind: 1 objects or versions under scratch/sonde-probe/")),
            "{:?}",
            r.observations
        );
        assert!(
            s.jobs()
                .info(&ids[0])
                .unwrap()
                .progress
                .message
                .contains("left behind"),
            "the job says so too"
        );
        assert!(s.guard().audit_entries(1)[0].errors[0].starts_with("left behind"));
    }
}
