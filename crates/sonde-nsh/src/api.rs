use nsh_core::JsonValue;
use nsh_core::prelude::*;
use serde_json::json;
use sonde_core::services::{browse, du, exec, layout, mpu, versions};
use sonde_core::{CredentialSource, EndpointConfig, JobEvent, JobState, Vendor};

use crate::state::{err, err_msg, run, sonde, to_json};

/// sonde's core, as TSX calls it. Every value is plain JSON; credentials
/// never cross.
#[module]
pub struct Sonde;

fn parse<T: serde::de::DeserializeOwned>(v: JsonValue, what: &str) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| err_msg(format!("{what}: {e}")))
}

/// The id of the newest record in the request log (0 when empty).
fn latest_id() -> f64 {
    sonde()
        .log()
        .list(&reqlog::Filter::default(), 1)
        .first()
        .map(|r| r.id as f64)
        .unwrap_or(0.0)
}

/// What the endpoint form sends: everything but secrets.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EndpointInput {
    id: String,
    name: String,
    url: String,
    #[serde(default)]
    region: String,
    #[serde(default)]
    path_style: bool,
    #[serde(default)]
    ca_file: Option<String>,
    #[serde(default)]
    vendor: Option<String>,
    #[serde(default)]
    buckets: Vec<String>,
    /// `default`, `profile`, `env`, `imds`, `web-identity`, `process`,
    /// `anonymous`, or `keep` (the configured source, unchanged).
    credentials: String,
    /// A new endpoint: refused when its id is taken, never replacing one.
    #[serde(default)]
    adding: bool,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    access_key_var: Option<String>,
    #[serde(default)]
    secret_key_var: Option<String>,
    #[serde(default)]
    role_arn: Option<String>,
    #[serde(default)]
    token_file: Option<String>,
    #[serde(default)]
    command: Option<String>,
}

#[module]
impl Sonde {
    // --- endpoints -------------------------------------------------------

    /// The configured endpoints: ids, names, URLs, how credentials are
    /// obtained (never the credentials).
    pub fn endpoints() -> JsonValue {
        to_json(sonde().endpoints())
    }

    /// The vendors sonde knows: id, label, URL template, default region, path style.
    pub fn vendors() -> JsonValue {
        JsonValue::Array(
            sonde_core::vendor::ALL
                .iter()
                .map(|v| json!({"id": v.id(), "label": v.label(), "urlTemplate": v.url_template(), "region": v.default_region(), "pathStyle": v.default_path_style(), "stsIdentity": v.has_sts_identity()}))
                .collect(),
        )
    }

    /// Adds or replaces an endpoint. Static keys cannot be set from the UI:
    /// they stay in the configuration file, and `credentials: "keep"` keeps them.
    pub fn save_endpoint(input: JsonValue) -> Result<JsonValue, String> {
        let i: EndpointInput = parse(input, "endpoint")?;
        let s = sonde();
        if i.id.trim().is_empty() {
            return Err(err_msg("an endpoint needs an id"));
        }
        let existing = s.endpoint_config(&i.id);
        if i.adding && existing.is_some() {
            return Err(err_msg(format!(
                "an endpoint with the id \"{}\" exists: choose another id",
                i.id
            )));
        }
        let vendor = i
            .vendor
            .as_deref()
            .and_then(Vendor::from_id)
            .unwrap_or_else(|| Vendor::guess(&i.url));
        let credentials = match i.credentials.as_str() {
            "keep" => s
                .endpoint_config(&i.id)
                .map(|e| e.credentials)
                .unwrap_or_default(),
            "default" => CredentialSource::Default,
            "profile" => CredentialSource::Profile {
                name: i
                    .profile
                    .clone()
                    .filter(|p| !p.is_empty())
                    .ok_or_else(|| err_msg("a profile name is required"))?,
            },
            "env" => {
                // an edit keeps the variables the configuration names
                let kept = match existing.as_ref().map(|e| &e.credentials) {
                    Some(CredentialSource::Env {
                        access_key_id_var,
                        secret_access_key_var,
                        session_token_var,
                    }) => Some((
                        access_key_id_var.clone(),
                        secret_access_key_var.clone(),
                        session_token_var.clone(),
                    )),
                    _ => None,
                };
                CredentialSource::Env {
                    access_key_id_var: i
                        .access_key_var
                        .clone()
                        .or_else(|| kept.as_ref().map(|k| k.0.clone()))
                        .unwrap_or_else(|| "AWS_ACCESS_KEY_ID".into()),
                    secret_access_key_var: i
                        .secret_key_var
                        .clone()
                        .or_else(|| kept.as_ref().map(|k| k.1.clone()))
                        .unwrap_or_else(|| "AWS_SECRET_ACCESS_KEY".into()),
                    session_token_var: kept
                        .map(|k| k.2)
                        .unwrap_or_else(|| Some("AWS_SESSION_TOKEN".into())),
                }
            }
            "imds" => CredentialSource::Imds,
            "web-identity" => CredentialSource::WebIdentity {
                role_arn: i
                    .role_arn
                    .clone()
                    .ok_or_else(|| err_msg("a role ARN is required"))?,
                token_file: i
                    .token_file
                    .clone()
                    .ok_or_else(|| err_msg("a token file is required"))?
                    .into(),
                session_name: None,
            },
            "process" => CredentialSource::Process {
                command: i
                    .command
                    .clone()
                    .ok_or_else(|| err_msg("a command is required"))?,
            },
            "anonymous" => CredentialSource::Anonymous,
            other => return Err(err_msg(format!("unknown credential source {other}"))),
        };
        // an edit changes what the form shows and keeps the rest of the
        // configuration (static keys, settings the form has no field for)
        let mut e = match existing {
            Some(mut e) => {
                e.name = i.name.clone();
                e.url = i.url.clone();
                e.region = i.region.clone();
                e.vendor = vendor;
                e
            }
            None => EndpointConfig::new(&i.id, &i.name, &i.url, &i.region, vendor),
        };
        e.path_style = i.path_style;
        e.ca_file = i.ca_file.filter(|c| !c.is_empty()).map(Into::into);
        e.buckets = i.buckets;
        e.credentials = credentials;
        s.upsert_endpoint(e).map(to_json).map_err(err_msg)
    }

    /// Removes an endpoint from the configuration.
    pub fn remove_endpoint(id: String) -> Result<(), String> {
        sonde().remove_endpoint(&id).map_err(err_msg)
    }

    /// Endpoints for the AWS profiles not configured yet (not saved).
    pub fn importable_profiles() -> JsonValue {
        to_json(
            sonde()
                .importable_profiles()
                .iter()
                .map(sonde_core::EndpointView::from)
                .collect::<Vec<_>>(),
        )
    }

    /// Adds the endpoints of the named AWS profiles (each under an id of
    /// its own: an endpoint that exists is never replaced); how many.
    pub fn import_profiles(names: Vec<String>) -> Result<u32, String> {
        sonde().import_profiles(&names).map_err(err_msg)
    }

    /// Lists buckets and asks STS who the credentials are (where the vendor has it).
    pub async fn test_endpoint(id: String) -> Result<JsonValue, String> {
        run(async move { Ok::<_, sonde_core::S3Error>(sonde().test_endpoint(&id).await) }).await
    }

    /// Starts the built-in demo server and adds its endpoint (`demo`, not saved).
    pub async fn start_demo() -> Result<JsonValue, String> {
        run(async move {
            sonde_core::demo::install(&sonde())
                .await
                .map_err(sonde_core::S3Error::config)
        })
        .await
    }

    // --- configuration -------------------------------------------------------

    /// Why `config.toml` did not load, or null. While it stands the app
    /// runs with no endpoints and default settings, and every save
    /// (settings, endpoints, imports) is refused: the file is never
    /// written over. Show it, with [`Self::paths`]' `config`.
    pub fn config_error() -> Option<String> {
        sonde().config_error()
    }

    /// Reads `config.toml` again (after it was fixed, or edited outside).
    /// Rejects with why it still does not load.
    pub fn reload_config() -> Result<(), String> {
        sonde().reload_config().map_err(err_msg)
    }

    // --- settings ------------------------------------------------------------

    /// The settings (`read-only`, limits, prices, request log, theme, language).
    pub fn settings() -> JsonValue {
        to_json(sonde().settings())
    }

    /// Replaces the settings.
    pub fn save_settings(settings: JsonValue) -> Result<(), String> {
        let s: sonde_core::Settings = parse(settings, "settings")?;
        sonde().set_settings(s).map_err(err_msg)
    }

    /// Turns read-only mode on or off.
    pub fn set_read_only(on: bool) -> Result<(), String> {
        let s = sonde();
        let mut st = s.settings();
        st.read_only = on;
        s.set_settings(st).map_err(err_msg)
    }

    /// Where the configuration, data, exports and downloads go.
    pub fn paths() -> JsonValue {
        let s = sonde();
        let st = s.settings();
        json!({
            "config": s.paths().config_file(),
            "data": s.paths().data_dir,
            "exports": st.exports_dir.clone().unwrap_or_else(|| s.paths().default_exports()),
            "downloads": st.downloads_dir.clone().unwrap_or_else(|| s.paths().default_downloads()),
            "audit": s.paths().audit_log(),
        })
    }

    // --- browsing --------------------------------------------------------------

    /// The endpoint's buckets.
    pub async fn buckets(endpoint: String) -> Result<JsonValue, String> {
        run(async move { browse::list_buckets(&sonde(), &endpoint).await }).await
    }

    /// One ListObjectsV2 page.
    pub async fn list_page(
        endpoint: String,
        bucket: String,
        prefix: String,
        delimiter: Option<String>,
        token: Option<String>,
    ) -> Result<JsonValue, String> {
        run(async move {
            browse::list_page(
                &sonde(),
                &endpoint,
                &bucket,
                &prefix,
                delimiter.as_deref(),
                token,
                None,
            )
            .await
        })
        .await
    }

    /// HEAD of an object: headers, metadata, ETag and checksum analysis, SSE.
    pub async fn object_detail(
        endpoint: String,
        bucket: String,
        key: String,
        version: Option<String>,
    ) -> Result<JsonValue, String> {
        run(async move {
            browse::object_detail(&sonde(), &endpoint, &bucket, &key, version.as_deref()).await
        })
        .await
    }

    /// A presigned GET URL valid for `seconds`.
    pub async fn presign(
        endpoint: String,
        bucket: String,
        key: String,
        seconds: u32,
        version: Option<String>,
    ) -> Result<String, String> {
        match background(async move {
            browse::presign(
                &sonde(),
                &endpoint,
                &bucket,
                &key,
                seconds as u64,
                version.as_deref(),
            )
            .await
        })
        .await
        {
            Ok(Ok(u)) => Ok(u),
            Ok(Err(e)) => Err(err(e)),
            Err(e) => Err(err_msg(e)),
        }
    }

    /// Downloads an object into the downloads directory (a job; its id).
    /// Never replaces a file: the result's `path` is where it went.
    pub async fn download(endpoint: String, bucket: String, key: String) -> Result<String, String> {
        let s = sonde();
        let st = s.settings();
        let dir = st
            .downloads_dir
            .clone()
            .unwrap_or_else(|| s.paths().default_downloads());
        // a name taken already gets ` (2)`: the job's result says the path
        let dest = dir.join(browse::download_name(&key));
        spawn(move || browse::download(&sonde(), &endpoint, &bucket, &key, dest)).await
    }

    /// Searches keys under `prefix` whose name contains `needle` (a job).
    pub async fn search(
        endpoint: String,
        bucket: String,
        prefix: String,
        needle: String,
    ) -> Result<String, String> {
        spawn(move || browse::search(&sonde(), &endpoint, &bucket, &prefix, &needle, 5000)).await
    }

    // --- jobs ------------------------------------------------------------------

    /// Every job: kind, title, state, progress, limits.
    pub fn jobs() -> JsonValue {
        to_json(sonde().jobs().list())
    }

    /// One job's state.
    pub fn job(id: String) -> JsonValue {
        to_json(sonde().jobs().info(&id))
    }

    /// Cancels a job: its requests stop.
    pub fn cancel_job(id: String) -> bool {
        sonde().jobs().cancel(&id)
    }

    /// Follows a job: `listener` gets each event (progress, partial
    /// results); resolves with the result, rejects with the error.
    pub async fn watch_job(
        id: String,
        #[args(event)] listener: Callback<JsonValue>,
    ) -> Result<JsonValue, String> {
        let s = sonde();
        let Some(mut rx) = s.jobs().subscribe(&id) else {
            return Err(err_msg(format!("no job {id}")));
        };
        loop {
            if let Some(info) = s.jobs().info(&id) {
                match info.state {
                    JobState::Finished => {
                        return Ok(s.jobs().result(&id).unwrap_or(JsonValue::Null));
                    }
                    JobState::Failed => {
                        return Err(err(info
                            .error
                            .unwrap_or_else(|| sonde_core::S3Error::config("failed"))));
                    }
                    JobState::Cancelled => return Err(err(sonde_core::S3Error::cancelled())),
                    JobState::Running => {}
                }
            }
            match rx.recv().await {
                Ok(JobEvent::Finished { result, .. }) => return Ok(result),
                Ok(JobEvent::Failed { error, .. }) => return Err(err(error)),
                Ok(JobEvent::Cancelled { .. }) => {
                    return Err(err(sonde_core::S3Error::cancelled()));
                }
                Ok(ev) => listener.post(to_json(ev)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err(err_msg("the job vanished"));
                }
            }
        }
    }

    // --- request inspector ---------------------------------------------------------

    /// Records matching `filter` (`status`, `operation`, `endpoint`, `text`,
    /// `retried`), newest first: summaries.
    pub fn requests(filter: JsonValue, limit: u32) -> Result<JsonValue, String> {
        let f: reqlog::Filter = parse(filter, "filter")?;
        Ok(to_json(
            sonde()
                .log()
                .list(&f, limit as usize)
                .iter()
                .map(reqlog::Record::summary)
                .collect::<Vec<_>>(),
        ))
    }

    /// Listens to `event`, so views follow the engine instead of asking it
    /// on a timer: `"request"` gets the newest record's id as records come
    /// (several at once come as one), `"jobs"` the number of jobs running
    /// as any job starts, moves on or ends. Each gets the current value at
    /// once. Returns the function that stops listening.
    pub fn on(
        event: String,
        listener: Callback<JsonValue>,
        cx: CallCx<'_>,
    ) -> Result<Stop, String> {
        let held = std::rc::Rc::new(std::cell::RefCell::new(Some(listener)));
        let task = held.clone();
        let alive = move || task.borrow().clone();
        // stopping ends the task at once, not at the next change
        let (stop_tx, mut stop) = tokio::sync::watch::channel(false);
        match event.as_str() {
            "request" => {
                let mut rx = sonde().log().subscribe();
                cx.start(async move {
                    let mut last = latest_id();
                    if let Some(l) = alive() {
                        l.post(json!(last));
                    }
                    loop {
                        let next = tokio::select! {
                            _ = stop.changed() => return,
                            r = rx.recv() => r,
                        };
                        let id = match next {
                            Ok(id) => id,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                latest_id() as u64
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                        };
                        // the ones that came meanwhile, as one
                        let mut newest = id as f64;
                        while let Ok(more) = rx.try_recv() {
                            newest = more as f64;
                        }
                        if newest == last {
                            continue;
                        }
                        last = newest;
                        let Some(l) = alive() else { return };
                        l.post(json!(newest));
                    }
                });
            }
            "jobs" => {
                let mut rx = sonde().jobs().changes();
                cx.start(async move {
                    loop {
                        // marked seen before counting: a change while counting wakes the next turn
                        rx.borrow_and_update();
                        let running = sonde()
                            .jobs()
                            .list()
                            .iter()
                            .filter(|j| j.state == JobState::Running)
                            .count();
                        let Some(l) = alive() else { return };
                        l.post(json!(running));
                        tokio::select! {
                            _ = stop.changed() => return,
                            r = rx.changed() => if r.is_err() { return },
                        }
                    }
                });
            }
            other => return Err(err_msg(format!("no Sonde event `{other}`"))),
        }
        Ok(Stop::new(move || {
            drop(held.take());
            let _ = stop_tx.send(true);
        }))
    }

    /// The id of the newest record (0 when empty): poll it to refresh.
    pub fn latest_request() -> f64 {
        latest_id()
    }

    /// One record in full (redacted headers, error body, timing).
    pub fn request(id: f64) -> JsonValue {
        to_json(sonde().log().get(id as u64))
    }

    /// The record of a request id the service sent (for "show request").
    pub fn find_request(request_id: String) -> JsonValue {
        to_json(sonde().log().find_request_id(&request_id))
    }

    /// Operation names in the log, for the filter.
    pub fn request_operations() -> JsonValue {
        to_json(sonde().log().operations())
    }

    /// The record as an `aws s3api` command.
    pub fn copy_as_aws_cli(id: f64) -> Option<String> {
        sonde()
            .log()
            .get(id as u64)
            .map(|r| reqlog::copy::as_aws_cli(&r))
    }

    /// The record as a curl command, unsigned (curl's `--aws-sigv4` signs it).
    pub fn copy_as_curl(id: f64) -> Option<String> {
        sonde()
            .log()
            .get(id as u64)
            .map(|r| reqlog::copy::as_curl(&r))
    }

    /// Writes the matching records as `json` or `har`; the file's path.
    pub fn export_requests(format: String, filter: JsonValue) -> Result<String, String> {
        let f: reqlog::Filter = parse(filter, "filter")?;
        let s = sonde();
        let mut records = s.log().list(&f, usize::MAX);
        records.reverse();
        let doc = if format == "har" {
            reqlog::export::to_har(&records)
        } else {
            reqlog::export::to_json(&records)
        };
        write_export(
            &format!(
                "requests-{}.{}",
                stamp(),
                if format == "har" { "har" } else { "json" }
            ),
            &serde_json::to_vec_pretty(&doc).unwrap_or_default(),
        )
    }

    /// Empties the request log.
    pub fn clear_requests() {
        sonde().log().clear();
    }

    // --- multipart -----------------------------------------------------------------

    /// Scans incomplete uploads (a job; result: the report).
    pub async fn mpu_scan(
        endpoint: String,
        bucket: String,
        prefix: String,
    ) -> Result<String, String> {
        spawn(move || mpu::scan(&sonde(), &endpoint, &bucket, &prefix)).await
    }

    /// A dry run of aborting `uploads` (rows of a scan).
    pub fn plan_abort(
        endpoint: String,
        bucket: String,
        uploads: JsonValue,
    ) -> Result<JsonValue, String> {
        let rows: Vec<mpu::UploadRow> = parse(uploads, "uploads")?;
        Ok(to_json(mpu::plan_abort(
            &sonde(),
            &endpoint,
            &bucket,
            &rows,
        )))
    }

    /// A dry run of adding an AbortIncompleteMultipartUpload rule.
    pub async fn plan_lifecycle(
        endpoint: String,
        bucket: String,
        prefix: String,
        days: u32,
    ) -> Result<JsonValue, String> {
        run(async move { mpu::plan_lifecycle_rule(&sonde(), &endpoint, &bucket, &prefix, days).await }).await
    }

    // --- versions ------------------------------------------------------------------------

    /// The bucket's versioning, MFA delete and Object Lock state.
    pub async fn versioning(endpoint: String, bucket: String) -> Result<JsonValue, String> {
        run(async move { versions::versioning(&sonde(), &endpoint, &bucket).await }).await
    }

    /// A key's versions and delete markers, newest first.
    pub async fn timeline(
        endpoint: String,
        bucket: String,
        key: String,
    ) -> Result<JsonValue, String> {
        run(async move { versions::timeline(&sonde(), &endpoint, &bucket, &key).await }).await
    }

    /// A dry run of restoring a deleted key (removing its delete marker).
    pub async fn plan_restore(
        endpoint: String,
        bucket: String,
        key: String,
    ) -> Result<JsonValue, String> {
        run(async move { versions::plan_restore(&sonde(), &endpoint, &bucket, &key).await }).await
    }

    /// A dry run of permanently deleting versions: `[{ key, versionId, size }]`.
    pub fn plan_delete_versions(
        endpoint: String,
        bucket: String,
        targets: JsonValue,
    ) -> Result<JsonValue, String> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct T {
            key: String,
            version_id: String,
            size: Option<u64>,
        }
        let t: Vec<T> = parse(targets, "targets")?;
        let t: Vec<(String, String, Option<u64>)> = t
            .into_iter()
            .map(|x| (x.key, x.version_id, x.size))
            .collect();
        Ok(to_json(versions::plan_delete_versions(
            &sonde(),
            &endpoint,
            &bucket,
            &t,
        )))
    }

    // --- destructive operations ------------------------------------------------------------

    /// Runs a dry-run plan: refused while read-only mode is on or when
    /// `confirmation` is not what the plan asks for. Audited.
    pub async fn execute_plan(plan: String, confirmation: String) -> Result<JsonValue, String> {
        run(async move { exec::execute(&sonde(), &plan, &confirmation).await }).await
    }

    /// Drops a plan without running it.
    pub fn discard_plan(plan: String) {
        sonde().guard().discard(&plan);
    }

    /// The newest audit log entries.
    pub fn audit_log(limit: u32) -> JsonValue {
        to_json(sonde().guard().audit_entries(limit as usize))
    }

    // --- capacity --------------------------------------------------------------------------

    /// Scans a bucket or prefix (a job; result: the report, also cached).
    pub async fn du_scan(
        endpoint: String,
        bucket: String,
        prefix: String,
        multipart: bool,
    ) -> Result<String, String> {
        spawn(move || du::scan(&sonde(), &endpoint, &bucket, &prefix, multipart)).await
    }

    /// The last cached scan, or null.
    pub fn du_cached(endpoint: String, bucket: String, prefix: String) -> JsonValue {
        to_json(du::load_cache(&sonde(), &endpoint, &bucket, &prefix))
    }

    /// Exports the cached scan as `json` or `csv`; the file's path.
    pub fn du_export(
        endpoint: String,
        bucket: String,
        prefix: String,
        format: String,
    ) -> Result<String, String> {
        let r = du::load_cache(&sonde(), &endpoint, &bucket, &prefix)
            .ok_or_else(|| err_msg("no scan to export: scan first"))?;
        let (ext, body) = if format == "csv" {
            ("csv", du::to_csv(&r).into_bytes())
        } else {
            ("json", serde_json::to_vec_pretty(&r).unwrap_or_default())
        };
        write_export(&format!("du-{}-{}.{ext}", r.bucket, stamp()), &body)
    }

    // --- probes ----------------------------------------------------------------------------

    /// Every probe: id, category, title, description, AWS's behavior.
    pub fn probe_catalog() -> JsonValue {
        to_json(probes::catalog())
    }

    /// Runs probes on several endpoints in parallel: `options` is
    /// `{ bucket, allowTempBuckets, includeSlow, only }`. The job ids, in
    /// order. Refused (kind `refused`) while read-only mode is on, as every
    /// write is; each run is in the audit log.
    pub async fn run_probes(
        endpoints: Vec<String>,
        options: JsonValue,
    ) -> Result<Vec<String>, String> {
        let o: probes::runner::RunOptions = parse(options, "options")?;
        probes::runner::allowed(&sonde()).map_err(err)?;
        match background(async move { probes::runner::spawn(&sonde(), &endpoints, o) }).await {
            Ok(ids) => Ok(ids),
            Err(e) => Err(err_msg(e)),
        }
    }

    /// Saved runs, newest first.
    pub fn probe_runs() -> JsonValue {
        to_json(probes::store::list(sonde().paths()))
    }

    /// One saved run with every result and its exchanges.
    pub fn probe_run(id: String, endpoint: String) -> JsonValue {
        to_json(probes::store::load(sonde().paths(), &id, &endpoint))
    }

    /// The vendor × probe matrix of the newest run of each endpoint (or of `runs`, `["id:endpoint"]`).
    pub fn probe_matrix(runs: Option<Vec<String>>) -> JsonValue {
        to_json(probes::report::matrix(&load_runs(runs)))
    }

    /// What changed between two saved runs.
    pub fn probe_diff(before: String, after: String) -> Result<JsonValue, String> {
        let r = load_runs(Some(vec![before, after]));
        if r.len() != 2 {
            return Err(err_msg("both runs must exist"));
        }
        Ok(to_json(probes::store::diff(&r[0], &r[1])))
    }

    /// Writes a chart's rows (the app's CSV of what the chart draws) into the
    /// exports folder as `<name>-<stamp>.csv`; the file's path. Native
    /// Shell has no save dialog: the file goes where every export goes.
    pub fn export_csv(name: String, text: String) -> Result<String, String> {
        write_export(&format!("{}-{}.csv", csv_name(&name), stamp()), text.as_bytes())
    }

    /// Writes runs (default: the newest of each endpoint) as `markdown` or `json`; the path.
    pub fn export_probes(format: String, runs: Option<Vec<String>>) -> Result<String, String> {
        let r = load_runs(runs);
        if format == "markdown" {
            write_export(
                &format!("probes-{}.md", stamp()),
                probes::report::markdown(&r).as_bytes(),
            )
        } else {
            write_export(
                &format!("probes-{}.json", stamp()),
                &serde_json::to_vec_pretty(&r).unwrap_or_default(),
            )
        }
    }

    // --- layouts ---------------------------------------------------------------------------

    /// Detects layouts under a prefix (a job; result: `[{ format, root, evidence }]`).
    pub async fn detect_layouts(
        endpoint: String,
        bucket: String,
        prefix: String,
    ) -> Result<String, String> {
        spawn(move || layout::detect(&sonde(), &endpoint, &bucket, &prefix)).await
    }

    /// Decodes a layout (a job; result: the layout with its timeline and snapshots).
    pub async fn analyze_layout(
        endpoint: String,
        bucket: String,
        format: String,
        root: String,
    ) -> Result<String, String> {
        let f = decoders::Format::from_id(&format)
            .ok_or_else(|| err_msg(format!("unknown layout {format}")))?;
        spawn(move || layout::analyze(&sonde(), &endpoint, &bucket, f, &root)).await
    }

    /// The logical state at `time` (ms since the epoch) of a finished layout job.
    pub fn layout_state(job: String, time: f64) -> JsonValue {
        let s = sonde();
        let Some(v) = s.jobs().result(&job) else {
            return JsonValue::Null;
        };
        let Ok(l) = serde_json::from_value::<decoders::Layout>(v) else {
            return JsonValue::Null;
        };
        let t =
            chrono::DateTime::from_timestamp_millis(time as i64).unwrap_or_else(chrono::Utc::now);
        to_json(l.state_at(t))
    }
}

/// Starts a job on the runtime's threads (where jobs spawn their tasks).
async fn spawn(f: impl FnOnce() -> String + Send + 'static) -> Result<String, String> {
    background(async move { f() }).await.map_err(err_msg)
}

fn load_runs(specs: Option<Vec<String>>) -> Vec<probes::Run> {
    let s = sonde();
    match specs {
        None => probes::store::latest(s.paths()),
        Some(v) => v
            .iter()
            .filter_map(|spec| {
                spec.split_once(':')
                    .and_then(|(id, ep)| probes::store::load(s.paths(), id, ep))
            })
            .collect(),
    }
}

fn stamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// A file name's stem from a chart's id: letters, digits and `-` only, never empty.
fn csv_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .take(48)
        .collect();
    let s = s.trim_matches('-');
    if s.is_empty() { "chart".into() } else { s.into() }
}

fn write_export(name: &str, bytes: &[u8]) -> Result<String, String> {
    let s = sonde();
    let dir = s
        .settings()
        .exports_dir
        .clone()
        .unwrap_or_else(|| s.paths().default_exports());
    std::fs::create_dir_all(&dir).map_err(|e| err_msg(format!("{}: {e}", dir.display())))?;
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| err_msg(format!("{}: {e}", path.display())))?;
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_charts_csv_is_named_from_its_id_and_nothing_else() {
        assert_eq!(super::csv_name("latency"), "latency");
        assert_eq!(super::csv_name("../../etc/passwd"), "etc-passwd");
        assert_eq!(super::csv_name("错误"), "chart");
        assert_eq!(super::csv_name("error rate"), "error-rate");
    }
}
