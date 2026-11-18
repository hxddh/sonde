use async_trait::async_trait;
use s3raw::S3Request;

use super::{meta, setup_failed};
use crate::Probe;
use crate::cx::{ProbeCx, data, etag_of};
use crate::model::{Category, Judgement, ProbeMeta, Steps, Verdict};
use crate::phrase::ph;

pub fn probes() -> Vec<Box<dyn Probe>> {
    vec![
        Box::new(PutIfNoneMatch),
        Box::new(PutIfMatch),
        Box::new(CopyIfMatch),
        Box::new(CopyDestIfMatch),
        Box::new(CompleteIfNoneMatch),
        Box::new(DeleteIfMatch),
    ]
}

/// A 501 / NotImplemented, or a write that ignored the condition.
fn unsupported_or(steps: &Steps, j: Judgement) -> Judgement {
    if let Some(s) = steps.any_not_implemented() {
        return Judgement::say(
            Verdict::Unsupported,
            ph!("judge.cond.notImplemented", p1 = s.describe()),
            steps,
        );
    }
    j
}

pub struct PutIfNoneMatch;

#[async_trait]
impl Probe for PutIfNoneMatch {
    fn meta(&self) -> ProbeMeta {
        meta("cond.put-if-none-match", Category::ConditionalWrites, &[])
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
        if let Some(j) = super::write_refused(s, &["first"]) {
            return j;
        }
        let j = if !first.ok() {
            Judgement::say(Verdict::Fail, ph!("judge.cond.put-if-none-match.fail"), s)
        } else if second.ok() {
            Judgement::say(Verdict::Fail, ph!("judge.cond.put-if-none-match.fail.2"), s)
        } else if second.is(412) {
            Judgement::say(Verdict::Pass, ph!("judge.cond.put-if-none-match.pass"), s)
        } else if second.is(409) {
            Judgement::say(
                Verdict::Differs,
                ph!("judge.cond.put-if-none-match.differs"),
                s,
            )
        } else {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.cond.put-if-none-match.differs.2",
                    p1 = crate::model::status_text(second.status),
                    p2 = crate::model::or_none(second.code.as_deref())
                ),
                s,
            )
        };
        let j = if j.verdict == Verdict::Pass
            && read.ok()
            && read.body != String::from_utf8_lossy(&data(16, 1))
        {
            Judgement::say(Verdict::Fail, ph!("judge.cond.put-if-none-match.fail.3"), s)
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
        meta("cond.put-if-match", Category::ConditionalWrites, &[])
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
            Judgement::say(Verdict::Fail, ph!("judge.cond.put-if-match.fail"), s)
        } else if !current.ok() {
            Judgement::say(Verdict::Fail, ph!("judge.cond.put-if-match.fail.2"), s)
        } else if missing.ok() {
            // an object written that must not be: the guarantee broke
            Judgement::say(Verdict::Fail, ph!("judge.cond.put-if-match.fail.3"), s)
        } else if stale.is(412) && missing.is(404) {
            Judgement::say(Verdict::Pass, ph!("judge.cond.put-if-match.pass"), s)
        } else if stale.is(412) {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.cond.put-if-match.differs",
                    p1 = crate::model::status_text(missing.status)
                ),
                s,
            )
        } else {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.cond.put-if-match.differs.2",
                    p1 = crate::model::status_text(stale.status)
                ),
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
        meta("cond.copy-if-match", Category::ConditionalWrites, &[])
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
            Judgement::say(Verdict::Fail, ph!("judge.cond.copy-if-match.fail"), s)
        } else if !right.ok() {
            Judgement::say(Verdict::Fail, ph!("judge.cond.copy-if-match.fail.2"), s)
        } else if exists.ok() {
            Judgement::say(Verdict::Differs, ph!("judge.cond.copy-if-match.differs"), s)
        } else if wrong.is(412) && exists.is(412) {
            Judgement::say(Verdict::Pass, ph!("judge.cond.copy-if-match.pass"), s)
        } else {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.cond.copy-if-match.differs.2",
                    p1 = crate::model::status_text(wrong.status),
                    p2 = crate::model::status_text(exists.status)
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

/// An ETag no object has.
const STALE: &str = "\"00000000000000000000000000000000\"";

/// CopyObject's own If-Match, on the destination (botocore 1.40.61): the copy
/// replaces only the version the writer read.
pub struct CopyDestIfMatch;

#[async_trait]
impl Probe for CopyDestIfMatch {
    fn meta(&self) -> ProbeMeta {
        meta("cond.copy-dest-if-match", Category::ConditionalWrites, &[])
    }
    async fn run(&self, cx: &ProbeCx) {
        let src = cx.key("cdim-src");
        let dst = cx.key("cdim-dst");
        cx.put("source", &src, data(32, 1), &[]).await;
        let etag = cx
            .put("dest", &dst, data(8, 2), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        let source = format!("{}/{}", cx.bucket, s3raw::client::encode_key(&src));
        let copy = |label: &'static str, key: String, if_match: String| {
            let req = S3Request::new("CopyObject", http::Method::PUT)
                .bucket(&cx.bucket)
                .key(&key)
                .header("x-amz-copy-source", source.clone())
                .header("if-match", if_match);
            cx.send(label, req)
        };
        copy("stale", dst.clone(), STALE.into()).await;
        copy("current", dst.clone(), etag.clone()).await;
        copy("missing", cx.key("cdim-missing"), etag).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        if let Some(j) = setup_failed(s, &["source", "dest"]) {
            return j;
        }
        let (stale, current, missing) = (s.at("stale"), s.at("current"), s.at("missing"));
        let j = if stale.ok() {
            Judgement::say(Verdict::Fail, ph!("judge.cond.copy-dest-if-match.fail"), s)
        } else if !current.ok() {
            Judgement::say(Verdict::Fail, ph!("judge.cond.put-if-match.fail.2"), s)
        } else if missing.ok() {
            Judgement::say(
                Verdict::Fail,
                ph!("judge.cond.copy-dest-if-match.fail.2"),
                s,
            )
        } else if stale.is(412) && missing.is(404) {
            Judgement::say(Verdict::Pass, ph!("judge.cond.put-if-match.pass"), s)
        } else if stale.is(412) {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.cond.put-if-match.differs",
                    p1 = crate::model::status_text(missing.status)
                ),
                s,
            )
        } else {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.cond.put-if-match.differs.2",
                    p1 = crate::model::status_text(stale.status)
                ),
                s,
            )
        };
        unsupported_or(s, j)
    }
}

/// DeleteObject's If-Match, and an ETag on each object of a DeleteObjects
/// (botocore 1.35.67): a delete takes only the version the writer read.
pub struct DeleteIfMatch;

#[async_trait]
impl Probe for DeleteIfMatch {
    fn meta(&self) -> ProbeMeta {
        meta("cond.delete-if-match", Category::ConditionalWrites, &[])
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("dim");
        let etag = cx
            .put("create", &k, data(8, 1), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        let delete = |label: &'static str, if_match: String| {
            let req = S3Request::new("DeleteObject", http::Method::DELETE)
                .bucket(&cx.bucket)
                .key(&k)
                .header("if-match", if_match);
            cx.send(label, req)
        };
        delete("stale", STALE.into()).await;
        cx.head("kept", &k, &[]).await;
        delete("current", etag).await;
        cx.head("gone", &k, &[]).await;
        // the batch: one object, its ETag stale
        let b = cx.key("dim-batch");
        cx.put("batch-create", &b, data(8, 2), &[]).await;
        let body = format!(
            "<Delete xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Object><Key>{}</Key><ETag>{}</ETag></Object></Delete>",
            s3raw::xml::escape(&b),
            s3raw::xml::escape(STALE)
        );
        let md5 = {
            use base64::Engine as _;
            use md5::Digest as _;
            base64::engine::general_purpose::STANDARD.encode(md5::Md5::digest(body.as_bytes()))
        };
        let req = S3Request::new("DeleteObjects", http::Method::POST)
            .bucket(&cx.bucket)
            .query("delete", "")
            .header("content-md5", md5)
            .header("content-type", "application/xml")
            .body(body);
        cx.send("batch-stale", req).await;
        cx.head("batch-kept", &b, &[]).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        if let Some(j) = setup_failed(s, &["create"]) {
            return j;
        }
        let (stale, kept, current, gone) =
            (s.at("stale"), s.at("kept"), s.at("current"), s.at("gone"));
        let (batch, batch_kept) = (s.at("batch-stale"), s.at("batch-kept"));
        let batched = s.at("batch-create").ok();
        let j = if stale.ok() || kept.is(404) {
            Judgement::say(Verdict::Fail, ph!("judge.cond.delete-if-match.fail"), s)
        } else if !current.ok() {
            Judgement::say(Verdict::Fail, ph!("judge.cond.put-if-match.fail.2"), s)
        } else if gone.ok() {
            Judgement::say(Verdict::Fail, ph!("judge.cond.delete-if-match.fail.2"), s)
        } else if batched && batch.ok() && batch_kept.is(404) {
            Judgement::say(Verdict::Fail, ph!("judge.cond.delete-if-match.fail.3"), s)
        } else if !stale.is(412) {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.cond.put-if-match.differs.2",
                    p1 = crate::model::status_text(stale.status)
                ),
                s,
            )
        } else if !batched || !batch.ok() || !batch.body.contains("PreconditionFailed") {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.cond.delete-if-match.differs",
                    p1 = crate::model::status_text(batch.status)
                ),
                s,
            )
        } else {
            Judgement::say(Verdict::Pass, ph!("judge.cond.delete-if-match.pass"), s)
        };
        unsupported_or(s, j)
    }
}

pub struct CompleteIfNoneMatch;

#[async_trait]
impl Probe for CompleteIfNoneMatch {
    fn meta(&self) -> ProbeMeta {
        meta(
            "cond.complete-if-none-match",
            Category::ConditionalWrites,
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
            Judgement::say(
                Verdict::Fail,
                ph!("judge.cond.complete-if-none-match.fail"),
                s,
            )
        } else if complete.is(412) && head.header("content-length") == Some("8") {
            Judgement::say(
                Verdict::Pass,
                ph!("judge.cond.complete-if-none-match.pass"),
                s,
            )
        } else if complete.is(412) {
            Judgement::say(
                Verdict::Fail,
                ph!("judge.cond.complete-if-none-match.fail.2"),
                s,
            )
        } else {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.cond.complete-if-none-match.differs",
                    p1 = crate::model::status_text(complete.status),
                    p2 = crate::model::or_none(complete.code.as_deref())
                ),
                s,
            )
        };
        unsupported_or(s, j)
    }
}
