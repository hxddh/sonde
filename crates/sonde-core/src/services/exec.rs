//! Executing confirmed plans, with the audit trail.

use chrono::Utc;
use s3raw::S3Error;
use serde_json::json;

use crate::Sonde;
use crate::guard::{Action, AuditEntry, Outcome};

/// Runs plan `id` if read-only is off and `confirmation` matches.
pub async fn execute(s: &Sonde, id: &str, confirmation: &str) -> Result<Outcome, S3Error> {
    let plan = s.guard().take(id, s.read_only(), confirmation)?;
    let c = s.client(&plan.endpoint)?;
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
    if let Err(e) = s.guard().audit(&entry) {
        tracing::error!("audit log: {e}");
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
        let mut st = s.settings();
        st.read_only = false;
        s.set_settings(st).unwrap();
        // a directory where the log file should be
        std::fs::create_dir_all(s.paths().audit_log()).unwrap();
        let item = PlanItem {
            key: "k".into(),
            id: Some("v".into()),
            bytes: None,
            detail: String::new(),
        };
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
        let mut st = s.settings();
        st.read_only = false;
        s.set_settings(st).unwrap();
        let item = PlanItem {
            key: "k".into(),
            id: Some("v".into()),
            bytes: None,
            detail: String::new(),
        };
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
}
