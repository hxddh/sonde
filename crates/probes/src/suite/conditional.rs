use async_trait::async_trait;
use s3raw::S3Request;

use super::{meta, setup_failed};
use crate::Probe;
use crate::cx::{ProbeCx, data, etag_of};
use crate::model::{Category, Judgement, ProbeMeta, Steps, Verdict};

pub fn probes() -> Vec<Box<dyn Probe>> {
    vec![
        Box::new(PutIfNoneMatch),
        Box::new(PutIfMatch),
        Box::new(CopyIfMatch),
        Box::new(CompleteIfNoneMatch),
    ]
}

/// A 501 / NotImplemented, or a write that ignored the condition.
fn unsupported_or(steps: &Steps, j: Judgement) -> Judgement {
    if let Some(s) = steps.any_not_implemented() {
        return Judgement::new(
            Verdict::Unsupported,
            format!("not implemented ({})", s.describe()),
            steps,
        );
    }
    j
}

pub struct PutIfNoneMatch;

#[async_trait]
impl Probe for PutIfNoneMatch {
    fn meta(&self) -> ProbeMeta {
        meta(
            "cond.put-if-none-match",
            Category::ConditionalWrites,
            "PUT If-None-Match: *",
            "Create-only writes: a PUT with If-None-Match: * succeeds on a new key and is refused when the key exists.",
            "First PUT 200; second PUT 412 PreconditionFailed (409 ConditionalRequestConflict while a concurrent write is in flight). The object keeps its first body.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("inm");
        cx.put("first", &k, data(16, 1), &[("if-none-match", "*")])
            .await;
        cx.put("second", &k, data(16, 2), &[("if-none-match", "*")])
            .await;
        cx.get("read", &k, &[]).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (first, second, read) = (s.at("first"), s.at("second"), s.at("read"));
        let j = if !first.ok() {
            Judgement::new(
                Verdict::Fail,
                "a create-only PUT of a new key was refused",
                s,
            )
        } else if second.ok() {
            Judgement::new(
                Verdict::Fail,
                "If-None-Match: * was ignored: the existing object was overwritten",
                s,
            )
        } else if second.is(412) {
            Judgement::new(Verdict::Pass, "refused with 412 as AWS does", s)
        } else if second.is(409) {
            Judgement::new(Verdict::Differs, "refused with 409 instead of 412", s)
        } else {
            Judgement::new(
                Verdict::Differs,
                format!("refused with {:?} {:?}", second.status, second.code),
                s,
            )
        };
        let j = if j.verdict == Verdict::Pass
            && read.ok()
            && read.body != String::from_utf8_lossy(&data(16, 1))
        {
            Judgement::new(Verdict::Fail, "refused, yet the body changed", s)
        } else {
            j
        };
        unsupported_or(s, j)
    }
}

pub struct PutIfMatch;

#[async_trait]
impl Probe for PutIfMatch {
    fn meta(&self) -> ProbeMeta {
        meta(
            "cond.put-if-match",
            Category::ConditionalWrites,
            "PUT If-Match",
            "Compare-and-swap writes: a PUT with If-Match succeeds only while the object still has that ETag.",
            "PUT with a stale ETag: 412 PreconditionFailed; with the current ETag: 200. A missing key with If-Match: 404 NoSuchKey.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("im");
        let etag = cx
            .put("create", &k, data(8, 1), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        cx.put(
            "stale",
            &k,
            data(8, 2),
            &[("if-match", "\"00000000000000000000000000000000\"")],
        )
        .await;
        cx.put("current", &k, data(8, 3), &[("if-match", etag.as_str())])
            .await;
        cx.put(
            "missing",
            &cx.key("im-missing"),
            data(8, 4),
            &[("if-match", etag.as_str())],
        )
        .await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        if let Some(j) = setup_failed(s, &["create"]) {
            return j;
        }
        let (stale, current, missing) = (s.at("stale"), s.at("current"), s.at("missing"));
        let j = if stale.ok() {
            Judgement::new(
                Verdict::Fail,
                "If-Match was ignored: a stale ETag overwrote the object",
                s,
            )
        } else if !current.ok() {
            Judgement::new(Verdict::Fail, "the current ETag was refused", s)
        } else if missing.ok() {
            // an object written that must not be: the guarantee broke
            Judgement::new(
                Verdict::Fail,
                "If-Match was ignored on a missing key: the object was created",
                s,
            )
        } else if stale.is(412) && missing.is(404) {
            Judgement::new(
                Verdict::Pass,
                "412 on a stale ETag, 404 on a missing key",
                s,
            )
        } else if stale.is(412) {
            Judgement::new(
                Verdict::Differs,
                format!("missing key answered {:?} instead of 404", missing.status),
                s,
            )
        } else {
            Judgement::new(
                Verdict::Differs,
                format!("stale ETag answered {:?} instead of 412", stale.status),
                s,
            )
        };
        unsupported_or(s, j)
    }
}

pub struct CopyIfMatch;

#[async_trait]
impl Probe for CopyIfMatch {
    fn meta(&self) -> ProbeMeta {
        meta(
            "cond.copy-if-match",
            Category::ConditionalWrites,
            "Conditional CopyObject",
            "CopyObject with x-amz-copy-source-if-match (on the source) and If-None-Match: * (on the destination).",
            "Wrong source ETag: 412; destination exists with If-None-Match: *: 412; otherwise 200.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let src = cx.key("copy-src");
        let dst = cx.key("copy-dst");
        let etag = cx
            .put("source", &src, data(32, 1), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        let source = format!("{}/{}", cx.bucket, s3raw::client::encode_key(&src));
        let copy = |label: &'static str, hs: Vec<(&'static str, String)>| {
            let mut req = S3Request::new("CopyObject", http::Method::PUT)
                .bucket(&cx.bucket)
                .key(&dst)
                .header("x-amz-copy-source", source.clone());
            for (k, v) in hs {
                req = req.header(k, v);
            }
            cx.send(label, req)
        };
        copy(
            "wrong-source",
            vec![(
                "x-amz-copy-source-if-match",
                "\"00000000000000000000000000000000\"".into(),
            )],
        )
        .await;
        copy(
            "right-source",
            vec![("x-amz-copy-source-if-match", etag.clone())],
        )
        .await;
        copy("dest-exists", vec![("if-none-match", "*".into())]).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        if let Some(j) = setup_failed(s, &["source"]) {
            return j;
        }
        let (wrong, right, exists) = (
            s.at("wrong-source"),
            s.at("right-source"),
            s.at("dest-exists"),
        );
        let j = if wrong.ok() {
            Judgement::new(Verdict::Fail, "copy-source-if-match was ignored", s)
        } else if !right.ok() {
            Judgement::new(Verdict::Fail, "a copy with the right source ETag failed", s)
        } else if exists.ok() {
            Judgement::new(
                Verdict::Differs,
                "If-None-Match: * on the copy destination is ignored (AWS refuses with 412)",
                s,
            )
        } else if wrong.is(412) && exists.is(412) {
            Judgement::new(Verdict::Pass, "both conditions enforced with 412", s)
        } else {
            Judgement::new(
                Verdict::Differs,
                format!(
                    "conditions enforced with {:?} / {:?}",
                    wrong.status, exists.status
                ),
                s,
            )
        };
        if exists.not_implemented() && j.verdict == Verdict::Pass {
            j
        } else {
            unsupported_or(s, j)
        }
    }
}

pub struct CompleteIfNoneMatch;

#[async_trait]
impl Probe for CompleteIfNoneMatch {
    fn meta(&self) -> ProbeMeta {
        meta(
            "cond.complete-if-none-match",
            Category::ConditionalWrites,
            "Conditional CompleteMultipartUpload",
            "CompleteMultipartUpload with If-None-Match: * when the key was created in the meantime.",
            "412 PreconditionFailed; the existing object is kept. The upload can still be aborted.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("cinm");
        let Some(up) = cx.create_upload("create-upload", &k, &[]).await else {
            return;
        };
        let etag = cx
            .upload_part("part", &k, &up, 1, data(1024, 3), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        cx.put("racing-put", &k, data(8, 1), &[]).await;
        cx.complete(
            "complete",
            &k,
            &up,
            &[(1, etag, None)],
            &[("if-none-match", "*")],
        )
        .await;
        cx.head("head", &k, &[]).await;
        let _ = cx.client.abort_multipart_upload(&cx.bucket, &k, &up).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        // without the racing object there is nothing for the condition to refuse
        if let Some(j) = setup_failed(s, &["create-upload", "part", "racing-put"]) {
            return j;
        }
        let (complete, head) = (s.at("complete"), s.at("head"));
        let j = if complete.ok() {
            Judgement::new(
                Verdict::Fail,
                "If-None-Match: * was ignored: the upload replaced the existing object",
                s,
            )
        } else if complete.is(412) && head.header("content-length") == Some("8") {
            Judgement::new(
                Verdict::Pass,
                "refused with 412; the existing object kept",
                s,
            )
        } else if complete.is(412) {
            Judgement::new(Verdict::Fail, "refused, but the existing object changed", s)
        } else {
            Judgement::new(
                Verdict::Differs,
                format!("refused with {:?} {:?}", complete.status, complete.code),
                s,
            )
        };
        unsupported_or(s, j)
    }
}
