//! Runs probes against endpoints as jobs, one job per endpoint, all in
//! parallel. Each probe gets its own prefix under `sonde-probe/<run>/`,
//! everything it wrote is removed afterwards (versions, delete markers,
//! uploads, temporary buckets), and its requests are kept with its result.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use s3raw::{S3Client, S3Error};
use serde::{Deserialize, Serialize};
use sonde_core::{JobCtx, Progress, Sonde};

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

/// Starts one job per endpoint; their ids, in order.
pub fn spawn(s: &Arc<Sonde>, endpoints: &[String], opts: RunOptions) -> Vec<String> {
    endpoints
        .iter()
        .map(|ep| {
            let (s2, ep2, o) = (s.clone(), ep.clone(), opts.clone());
            s.jobs().spawn(
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
    base.head_bucket(&opts.bucket).await.map_err(|e| S3Error {
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
    let mut results = Vec::new();
    let mut server = None;
    for (i, probe) in probes.iter().enumerate() {
        ctx.check()?;
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
            let timed_out =
                tokio::time::timeout(Duration::from_secs(opts.timeout_secs), probe.run(&cx))
                    .await
                    .is_err();
            let steps = cx.steps();
            let mut j = probe.judge(&steps);
            if timed_out {
                j.verdict = Verdict::Error;
                j.summary = format!("timed out after {} s; {}", opts.timeout_secs, j.summary);
            }
            cleanup(&client, &opts.bucket, &prefix, &cx.temp_buckets()).await;
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
    ctx.progress(Progress {
        done: total,
        total: Some(total),
        bytes: 0,
        message: "done".into(),
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

/// Removes everything under `prefix` (every version and upload) and the temporary buckets.
pub async fn cleanup(c: &S3Client, bucket: &str, prefix: &str, temp: &[String]) {
    purge(c, bucket, prefix).await;
    for b in temp {
        // Object Lock buckets keep locked versions: governance bypass
        purge(c, b, "").await;
        let _ = c.delete_bucket(b).await;
    }
}

async fn purge(c: &S3Client, bucket: &str, prefix: &str) {
    for _ in 0..20 {
        let Ok(l) = c
            .list_multipart_uploads(bucket, Some(prefix), None, None)
            .await
        else {
            break;
        };
        for u in &l.upload {
            let _ = c.abort_multipart_upload(bucket, &u.key, &u.upload_id).await;
        }
        if !l.is_truncated {
            break;
        }
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
        for chunk in objs.chunks(1000) {
            for (k, v) in chunk {
                let mut req = s3raw::S3Request::new("DeleteObject", http::Method::DELETE)
                    .bucket(bucket)
                    .key(k)
                    .header("x-amz-bypass-governance-retention", "true");
                if let Some(v) = v {
                    req = req.query("versionId", v.as_str());
                }
                if c.send(req).await.is_ok() {
                    progressed = true;
                }
            }
        }
        if !progressed {
            break;
        }
    }
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
