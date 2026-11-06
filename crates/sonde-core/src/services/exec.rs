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
        Action::PutLifecycle => {
            let doc = plan.document.clone().unwrap_or_default();
            let r = c.put_lifecycle(&plan.bucket, &doc).await;
            note(r, &mut out);
        }
    }
    out.detail = json!({"plan": plan.id, "action": plan.action, "bucket": plan.bucket});
    let entry = AuditEntry {
        time: Utc::now(),
        plan: plan.id.clone(),
        action: plan.action.clone(),
        endpoint: plan.endpoint.clone(),
        bucket: plan.bucket.clone(),
        items: plan.items.len(),
        succeeded: out.succeeded,
        failed: out.failed,
        errors: out.errors.iter().map(|e| e.to_string()).collect(),
        request_ids,
        targets: plan
            .items
            .iter()
            .map(|i| match &i.id {
                Some(id) => format!("{}@{id}", i.key),
                None => i.key.clone(),
            })
            .collect(),
    };
    if let Err(e) = s.guard().audit(&entry) {
        tracing::error!("audit log: {e}");
    }
    Ok(out)
}
