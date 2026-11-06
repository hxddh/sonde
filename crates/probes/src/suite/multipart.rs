use async_trait::async_trait;
use bytes::Bytes;

use super::meta;
use crate::Probe;
use crate::cx::{ProbeCx, data, etag_of};
use crate::model::{Category, Judgement, ProbeMeta, Steps, Verdict};

pub fn probes() -> Vec<Box<dyn Probe>> {
    vec![
        Box::new(MinPartSize),
        Box::new(OutOfOrder),
        Box::new(DuplicateComplete),
        Box::new(ConcurrentUploads),
        Box::new(UploadPartCopy),
    ]
}

const MIB: usize = 1024 * 1024;

pub struct MinPartSize;

#[async_trait]
impl Probe for MinPartSize {
    fn meta(&self) -> ProbeMeta {
        meta(
            "mpu.min-part-size",
            Category::Multipart,
            "Minimum part size",
            "Completes an upload whose first part is 1 MiB (every part but the last must be at least 5 MiB).",
            "400 EntityTooSmall at CompleteMultipartUpload; a 5 MiB first part completes.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("minpart");
        let Some(up) = cx.create_upload("create", &k, &[]).await else {
            return;
        };
        let e1 = cx
            .upload_part("small-part", &k, &up, 1, data(MIB, 1), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        let e2 = cx
            .upload_part("last-part", &k, &up, 2, data(10, 2), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        cx.complete(
            "complete-small",
            &k,
            &up,
            &[(1, e1, None), (2, e2, None)],
            &[],
        )
        .await;
        let _ = cx.client.abort_multipart_upload(&cx.bucket, &k, &up).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let c = s.at("complete-small");
        if c.ok() {
            Judgement::new(
                Verdict::Differs,
                "a 1 MiB non-final part was accepted (no 5 MiB minimum)",
                s,
            )
        } else if c.code.as_deref() == Some("EntityTooSmall") {
            Judgement::new(Verdict::Pass, "400 EntityTooSmall", s)
        } else {
            Judgement::new(
                Verdict::Differs,
                format!(
                    "refused with {:?} {:?} instead of EntityTooSmall",
                    c.status, c.code
                ),
                s,
            )
        }
    }
}

pub struct OutOfOrder;

#[async_trait]
impl Probe for OutOfOrder {
    fn meta(&self) -> ProbeMeta {
        meta(
            "mpu.out-of-order",
            Category::Multipart,
            "Parts out of order",
            "Uploads part 2 before part 1, completes with the parts listed in descending order, then in ascending order.",
            "Parts may be uploaded in any order. A Complete listing parts out of ascending order: 400 InvalidPartOrder. Ascending: 200, the body in part-number order.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("ooo");
        let Some(up) = cx.create_upload("create", &k, &[]).await else {
            return;
        };
        let e2 = cx
            .upload_part(
                "part-2-first",
                &k,
                &up,
                2,
                Bytes::from_static(b"WORLD"),
                &[],
            )
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        let e1 = cx
            .upload_part("part-1-second", &k, &up, 1, data(5 * MIB, 1), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        cx.complete(
            "complete-descending",
            &k,
            &up,
            &[(2, e2.clone(), None), (1, e1.clone(), None)],
            &[],
        )
        .await;
        cx.complete(
            "complete-ascending",
            &k,
            &up,
            &[(1, e1, None), (2, e2, None)],
            &[],
        )
        .await;
        cx.get("tail", &k, &[("range", "bytes=-5")]).await;
        let _ = cx.client.abort_multipart_upload(&cx.bucket, &k, &up).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (desc, asc, tail) = (
            s.at("complete-descending"),
            s.at("complete-ascending"),
            s.at("tail"),
        );
        if !asc.ok() && !desc.ok() {
            return Judgement::new(
                Verdict::Fail,
                "parts uploaded out of order could not be completed",
                s,
            );
        }
        if asc.ok() && tail.body != "WORLD" {
            return Judgement::new(
                Verdict::Fail,
                "the completed body is not in part-number order",
                s,
            );
        }
        if desc.ok() {
            Judgement::new(
                Verdict::Differs,
                "a descending part list was accepted (AWS: InvalidPartOrder)",
                s,
            )
        } else if desc.code.as_deref() == Some("InvalidPartOrder") {
            Judgement::new(
                Verdict::Pass,
                "out-of-order uploads complete; a descending list is InvalidPartOrder",
                s,
            )
        } else {
            Judgement::new(
                Verdict::Differs,
                format!(
                    "a descending list is refused with {:?} {:?}",
                    desc.status, desc.code
                ),
                s,
            )
        }
    }
}

pub struct DuplicateComplete;

#[async_trait]
impl Probe for DuplicateComplete {
    fn meta(&self) -> ProbeMeta {
        meta(
            "mpu.duplicate-complete",
            Category::Multipart,
            "Complete twice",
            "Sends the same CompleteMultipartUpload twice, as a client retrying after a lost response would.",
            "The retry answers 200 with the same ETag (S3 treats a repeated Complete of a just-completed upload as success); afterwards the upload id is gone (NoSuchUpload).",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("dup");
        let Some(up) = cx.create_upload("create", &k, &[]).await else {
            return;
        };
        let e = cx
            .upload_part("part", &k, &up, 1, data(1024, 3), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        cx.complete("first", &k, &up, &[(1, e.clone(), None)], &[])
            .await;
        cx.complete("second", &k, &up, &[(1, e, None)], &[]).await;
        cx.send(
            "list-parts",
            s3raw::S3Request::new("ListParts", http::Method::GET)
                .bucket(&cx.bucket)
                .key(&k)
                .query("uploadId", up.as_str()),
        )
        .await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (first, second) = (s.at("first"), s.at("second"));
        if !first.ok() {
            return Judgement::new(Verdict::Error, "the first Complete failed", s);
        }
        let etag = |st: &crate::model::Step| {
            st.body
                .split("<ETag>")
                .nth(1)
                .and_then(|r| r.split("</ETag>").next())
                .map(String::from)
        };
        if second.ok() && etag(&first) == etag(&second) {
            Judgement::new(Verdict::Pass, "the retry succeeded with the same ETag", s)
        } else if second.ok() {
            Judgement::new(Verdict::Fail, "the retry succeeded with another ETag", s)
        } else if second.code.as_deref() == Some("NoSuchUpload") {
            Judgement::new(
                Verdict::Differs,
                "the retry fails with NoSuchUpload: clients retrying a lost response see an error for a write that succeeded",
                s,
            )
        } else {
            Judgement::new(
                Verdict::Differs,
                format!("the retry answered {:?} {:?}", second.status, second.code),
                s,
            )
        }
    }
}

pub struct ConcurrentUploads;

#[async_trait]
impl Probe for ConcurrentUploads {
    fn meta(&self) -> ProbeMeta {
        meta(
            "mpu.concurrent-same-key",
            Category::Multipart,
            "Concurrent uploads of one key",
            "Starts two uploads of the same key, completes A, then B.",
            "Two distinct upload ids; both complete; the last Complete wins (the key holds B's body).",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("concurrent");
        let (Some(a), Some(b)) = (
            cx.create_upload("create-a", &k, &[]).await,
            cx.create_upload("create-b", &k, &[]).await,
        ) else {
            return;
        };
        cx.note("ids", format!("{a} {b}"));
        let ea = cx
            .upload_part("part-a", &k, &a, 1, Bytes::from_static(b"AAAA"), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        let eb = cx
            .upload_part("part-b", &k, &b, 1, Bytes::from_static(b"BBBB"), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        cx.complete("complete-a", &k, &a, &[(1, ea, None)], &[])
            .await;
        cx.complete("complete-b", &k, &b, &[(1, eb, None)], &[])
            .await;
        cx.get("read", &k, &[]).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let ids = s.at("ids").body;
        let mut it = ids.split(' ');
        if it.next() == it.next() {
            return Judgement::new(Verdict::Fail, "both uploads got the same upload id", s);
        }
        let (ca, cb, read) = (s.at("complete-a"), s.at("complete-b"), s.at("read"));
        if ca.ok() && cb.ok() && read.body == "BBBB" {
            Judgement::new(
                Verdict::Pass,
                "independent uploads; the last Complete wins",
                s,
            )
        } else if ca.ok() && cb.ok() {
            Judgement::new(
                Verdict::Fail,
                format!("both completed but the key reads {:?}", read.body),
                s,
            )
        } else {
            Judgement::new(
                Verdict::Differs,
                "the second upload could not complete after the first",
                s,
            )
        }
    }
}

pub struct UploadPartCopy;

#[async_trait]
impl Probe for UploadPartCopy {
    fn meta(&self) -> ProbeMeta {
        meta(
            "mpu.upload-part-copy",
            Category::Multipart,
            "UploadPartCopy",
            "Builds an object from a byte range of another with UploadPartCopy, plus an uploaded part.",
            "200 with <CopyPartResult>; the completed object holds the copied range followed by the uploaded part.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let src = cx.key("upc-src");
        let dst = cx.key("upc-dst");
        cx.put("source", &src, data(6 * MIB, 4), &[]).await;
        let Some(up) = cx.create_upload("create", &dst, &[]).await else {
            return;
        };
        let source = format!("{}/{}", cx.bucket, s3raw::client::encode_key(&src));
        let r = cx
            .client
            .upload_part_copy(
                &cx.bucket,
                &dst,
                &up,
                1,
                &source,
                Some(&format!("bytes=0-{}", 5 * MIB - 1)),
            )
            .await;
        let copied = cx.record("copy-part", r);
        let e1 = copied
            .as_ref()
            .and_then(|r| s3raw::xml::copy_result(&r.text()).ok())
            .and_then(|c| c.etag)
            .unwrap_or_default();
        let e2 = cx
            .upload_part("part-2", &dst, &up, 2, Bytes::from_static(b"END"), &[])
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        cx.complete("complete", &dst, &up, &[(1, e1, None), (2, e2, None)], &[])
            .await;
        cx.head("head", &dst, &[]).await;
        let _ = cx
            .client
            .abort_multipart_upload(&cx.bucket, &dst, &up)
            .await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (copy, complete, head) = (s.at("copy-part"), s.at("complete"), s.at("head"));
        if copy.not_implemented() {
            return Judgement::new(Verdict::Unsupported, "UploadPartCopy is not implemented", s);
        }
        if !copy.ok() {
            return Judgement::new(
                Verdict::Fail,
                format!("UploadPartCopy failed: {}", copy.describe()),
                s,
            );
        }
        let want = (5 * MIB + 3).to_string();
        if complete.ok() && head.header("content-length") == Some(want.as_str()) {
            Judgement::new(Verdict::Pass, "copied range and uploaded part combined", s)
        } else {
            Judgement::new(
                Verdict::Fail,
                format!(
                    "completed object length {:?}, expected {want}",
                    head.header("content-length")
                ),
                s,
            )
        }
    }
}
