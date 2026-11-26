//! Executing confirmed plans, with the audit trail.

use chrono::Utc;
use s3raw::S3Error;
use serde_json::json;

use crate::Sonde;
use crate::guard::{Action, AuditEntry, Outcome, Plan};

/// Runs plan `id` if `confirmation` matches.
pub async fn execute(s: &Sonde, id: &str, confirmation: &str) -> Result<Outcome, S3Error> {
    let plan = s.guard().take(id, confirmation)?;
    run(s, plan).await
}

/// Runs a plan already taken from the guard ([`crate::Guard::take`], or a
/// terminal's [`crate::Guard::take_saved`]): it is no longer there to run again.
pub async fn run(s: &Sonde, plan: Plan) -> Result<Outcome, S3Error> {
    let c = s.client(&plan.endpoint)?;
    // a lifecycle PUT replaces the whole configuration, made from the one
    // read at plan time: if it changed since, what changed would be lost
    if plan.action == Action::PutLifecycle
        && c.lifecycle_raw(&plan.bucket).await? != plan.previous_document
    {
        return Err(S3Error::refused(format!(
            "the lifecycle configuration of {} changed since the plan: nothing was done; plan it again",
            plan.bucket
        )));
    }
    let mut entry = AuditEntry {
        time: Utc::now(),
        plan: plan.id.clone(),
        action: plan.action.clone(),
        endpoint: plan.endpoint.clone(),
        bucket: plan.bucket.clone(),
        items: plan.items.len(),
        succeeded: 0,
        failed: 0,
        errors: vec![],
        request_ids: vec![],
        targets: plan
            .items
            .iter()
            .map(|i| match &i.id {
                Some(id) => format!("{}@{id}", i.key),
                None => i.key.clone(),
            })
            .collect(),
        stage: Some("started".into()),
    };
    // written before the first request: a run cut short is on record, and
    // one that cannot be recorded does not start
    s.guard().audit(&entry).map_err(|e| {
        S3Error::refused(format!(
            "the audit log {} cannot be written ({e}): nothing was done",
            s.paths().audit_log().display()
        ))
    })?;
    let mut out = Outcome::default();
    let mut request_ids = Vec::new();
    let mut note = |r: Result<s3raw::S3Response, S3Error>, out: &mut Outcome| match r {
        Ok(resp) => {
            out.succeeded += 1;
            if let Some(id) = resp.request_id {
                request_ids.push(id);
            }
        }
        Err(e) => {
            out.failed += 1;
            out.errors.push(e);
        }
    };
    match plan.action {
        Action::AbortUploads => {
            for i in &plan.items {
                let r = c
                    .abort_multipart_upload(
                        &plan.bucket,
                        &i.key,
                        i.id.as_deref().unwrap_or_default(),
                    )
                    .await;
                note(r, &mut out);
            }
        }
        Action::DeleteVersions | Action::RestoreObject => {
            for i in &plan.items {
                let r = c.delete_object(&plan.bucket, &i.key, i.id.as_deref()).await;
                note(r, &mut out);
            }
        }
        Action::DeleteObjects => {
            for chunk in plan.items.chunks(1000) {
                let objs: Vec<(String, Option<String>)> =
                    chunk.iter().map(|i| (i.key.clone(), None)).collect();
                match c.delete_objects(&plan.bucket, &objs, false).await {
                    Ok(r) => {
                        out.succeeded += r.deleted.len();
                        out.failed += r.error.len();
                        for e in r.error {
                            let mut err =
                                S3Error::new(s3raw::ErrorKind::Http, e.message.unwrap_or_default());
                            err.code = e.code;
                            out.errors.push(err);
                        }
                    }
                    Err(e) => {
                        out.failed += chunk.len();
                        out.errors.push(e);
                    }
                }
            }
        }
        Action::RunProbes => {
            return Err(S3Error::config("probe runs are not plans"));
        }
        Action::PutLifecycle => {
            let doc = plan.document.clone().unwrap_or_default();
            let r = c.put_lifecycle(&plan.bucket, &doc).await;
            note(r, &mut out);
        }
    }
    out.detail = json!({"plan": plan.id, "action": plan.action, "bucket": plan.bucket});
    entry.time = Utc::now();
    entry.stage = None;
    entry.succeeded = out.succeeded;
    entry.failed = out.failed;
    entry.errors = out.errors.iter().map(|e| e.to_string()).collect();
    entry.request_ids = request_ids;
    // what was done is done: the caller says that its record is not whole
    if let Err(e) = s.guard().audit(&entry) {
        tracing::error!("audit log: {e}");
        out.audit_error = Some(format!("{}: {e}", s.paths().audit_log().display()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::PlanItem;

    #[tokio::test]
    async fn a_plan_does_not_start_when_the_audit_log_cannot_be_written() {
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        // counts the requests that arrive: there must be none
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h = hits.clone();
        tokio::spawn(async move {
            while l.accept().await.is_ok() {
                h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        });
        let mut e = crate::EndpointConfig::new("e", "e", &url, "us-east-1", crate::Vendor::Other);
        e.credentials = crate::CredentialSource::Anonymous;
        e.path_style = true;
        s.add_transient_endpoint(e).unwrap();
        // a directory where the log file should be
        std::fs::create_dir_all(s.paths().audit_log()).unwrap();
        let item = PlanItem::new("k", Some("v".into()), None, vec![]);
        let plan = s
            .guard()
            .plan(Action::DeleteVersions, "e", "b", vec![item], None, None);
        let err = execute(&s, &plan.id, &plan.confirm).await.unwrap_err();
        assert!(err.message.contains("audit log"), "{err}");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_run_is_on_record_before_its_first_request() {
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        let mut e = crate::EndpointConfig::new(
            "e",
            "e",
            "http://127.0.0.1:1",
            "us-east-1",
            crate::Vendor::Other,
        );
        e.credentials = crate::CredentialSource::Anonymous;
        s.add_transient_endpoint(e).unwrap();
        let item = PlanItem::new("k", Some("v".into()), None, vec![]);
        let plan = s
            .guard()
            .plan(Action::DeleteVersions, "e", "b", vec![item], None, None);
        let out = execute(&s, &plan.id, &plan.confirm).await.unwrap();
        assert_eq!(out.failed, 1, "nothing listens there");
        let log = s.guard().audit_entries(10);
        assert_eq!(log.len(), 2);
        assert_eq!(log[1].stage.as_deref(), Some("started"));
        assert_eq!(log[1].targets, vec!["k@v"]);
        assert_eq!((log[0].stage.as_deref(), log[0].failed), (None, 1));
    }

    /// 32.0 logged it where a terminal never looks, and the app not at all:
    /// the outcome that could not be written is said in the outcome itself.
    #[tokio::test]
    async fn an_outcome_the_audit_log_could_not_take_is_said() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        // a store that, as the request arrives (after `started` is written),
        // puts a folder where the log is
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let log = s.paths().audit_log();
        let started = std::sync::Arc::new(parking_lot::Mutex::new(String::new()));
        let seen = started.clone();
        tokio::spawn(async move {
            while let Ok((mut c, _)) = l.accept().await {
                let mut buf = [0u8; 4096];
                let _ = c.read(&mut buf).await;
                if log.is_file() {
                    *seen.lock() = std::fs::read_to_string(&log).unwrap();
                    std::fs::remove_file(&log).unwrap();
                    std::fs::create_dir(&log).unwrap();
                }
                let _ = c
                    .write_all(b"HTTP/1.1 204 No Content\r\nx-amz-request-id: R1\r\nconnection: close\r\n\r\n")
                    .await;
            }
        });
        let mut e = crate::EndpointConfig::new("e", "e", &url, "us-east-1", crate::Vendor::Other);
        e.credentials = crate::CredentialSource::Anonymous;
        e.path_style = true;
        s.add_transient_endpoint(e).unwrap();
        let item = PlanItem::new("k", Some("v".into()), None, vec![]);
        let plan = s
            .guard()
            .plan(Action::DeleteVersions, "e", "b", vec![item], None, None);
        let out = execute(&s, &plan.id, &plan.confirm).await.unwrap();
        assert_eq!((out.succeeded, out.failed), (1, 0));
        assert!(started.lock().contains("\"started\""), "{}", started.lock());
        let why = out.audit_error.expect("said");
        assert!(why.contains("audit.jsonl"), "{why}");
        // and an outcome that was written says nothing of it
        let v = serde_json::to_value(Outcome::default()).unwrap();
        assert!(v.get("auditError").is_none(), "{v}");
    }

    /// A lifecycle PUT replaces the whole configuration: a rule added
    /// between the plan and its run would be lost, so the run is refused.
    #[tokio::test]
    async fn a_lifecycle_changed_since_its_plan_is_not_written_over() {
        let dir = tempfile::tempdir().unwrap();
        let s = Sonde::open(crate::Paths::rooted(dir.path())).unwrap();
        let server = crate::demo::start().await.unwrap();
        let mut e =
            crate::EndpointConfig::new("d", "d", &server.url(), "us-east-1", crate::Vendor::Other);
        e.path_style = true;
        e.credentials = crate::CredentialSource::Static {
            access_key_id: "AKIADEMODEMODEMO0000".into(),
            secret_access_key: "demo".into(),
        };
        s.add_transient_endpoint(e).unwrap();
        let plan = crate::services::mpu::plan_lifecycle_rule(&s, "d", "demo", "tmp/", 7)
            .await
            .unwrap();
        // meanwhile, someone adds a rule of their own
        let theirs = "<LifecycleConfiguration><Rule><ID>theirs</ID><Filter><Prefix>logs/</Prefix>\
                      </Filter><Status>Enabled</Status><Expiration><Days>30</Days></Expiration>\
                      </Rule></LifecycleConfiguration>";
        let c = s.client("d").unwrap();
        c.put_lifecycle("demo", theirs).await.unwrap();
        let err = execute(&s, &plan.id, &plan.confirm).await.unwrap_err();
        assert!(err.message.contains("changed since"), "{err}");
        let now = c.lifecycle_raw("demo").await.unwrap().unwrap();
        assert!(now.contains("theirs") && !now.contains("tmp/"), "{now}");
        // planned again, it keeps their rule
        let plan = crate::services::mpu::plan_lifecycle_rule(&s, "d", "demo", "tmp/", 7)
            .await
            .unwrap();
        let out = execute(&s, &plan.id, &plan.confirm).await.unwrap();
        assert_eq!((out.succeeded, out.failed), (1, 0));
        let now = c.lifecycle_raw("demo").await.unwrap().unwrap();
        assert!(now.contains("theirs") && now.contains("tmp/"), "{now}");
    }
}
