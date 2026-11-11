//! Checksums: each algorithm on a single PUT (and a wrong value), in
//! composite and full-object multipart uploads, and as a trailing checksum.

use async_trait::async_trait;
use s3raw::checksum::{ALL, Algorithm};

use super::meta;
use crate::Probe;
use crate::cx::{ProbeCx, data, etag_of};
use crate::model::{Category, Judgement, ProbeMeta, Steps, Verdict};
use crate::phrase::{ph, raw};

pub fn probes() -> Vec<Box<dyn Probe>> {
    let mut v: Vec<Box<dyn Probe>> = Vec::new();
    for a in ALL {
        v.push(Box::new(Single(a)));
        v.push(Box::new(Multipart {
            alg: a,
            full_object: false,
        }));
        if a.supports_full_object() {
            v.push(Box::new(Multipart {
                alg: a,
                full_object: true,
            }));
        }
    }
    v.push(Box::new(Trailing));
    v
}

fn lower(a: Algorithm) -> String {
    a.name().to_ascii_lowercase()
}

pub struct Single(pub Algorithm);

#[async_trait]
impl Probe for Single {
    fn meta(&self) -> ProbeMeta {
        let n = self.0.name();
        meta(
            &format!("checksum.{}.single", lower(self.0)),
            Category::Checksums,
            &format!("{n} on PutObject"),
            &format!(
                "PUT with x-amz-checksum-{}: the right value is stored and returned by HEAD (checksum mode); a wrong value is refused.",
                lower(self.0)
            ),
            "Right value: 200, echoed in the response and in HEAD with x-amz-checksum-mode: ENABLED (type FULL_OBJECT). Wrong value: 400 BadDigest, nothing stored.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let body = data(1000, 7);
        let good = self.0.compute(&body);
        let bad = self.0.compute(b"something else");
        let k = cx.key(&format!("ck-{}", lower(self.0)));
        let h = self.0.header();
        cx.put("good", &k, body.clone(), &[(h.as_str(), good.as_str())])
            .await;
        cx.head("head", &k, &[("x-amz-checksum-mode", "ENABLED")])
            .await;
        cx.put(
            "bad",
            &cx.key(&format!("ck-bad-{}", lower(self.0))),
            body,
            &[(h.as_str(), bad.as_str())],
        )
        .await;
        cx.note("expected", good);
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (good, head, bad) = (s.at("good"), s.at("head"), s.at("bad"));
        let expected = s.at("expected").body;
        let h = self.0.header();
        if let Some(j) = super::write_refused(s, &["good"]) {
            return j;
        }
        if !good.ok() {
            return if good.status == Some(400) || good.not_implemented() {
                Judgement::say(
                    Verdict::Unsupported,
                    ph!("judge.checksum.unsupported", p1 = self.0.name()),
                    s,
                )
            } else {
                Judgement::say(Verdict::Error, ph!("judge.checksum.error"), s)
            };
        }
        if bad.ok() {
            return Judgement::say(Verdict::Fail, ph!("judge.checksum.fail"), s);
        }
        match head.header(&h) {
            Some(v) if v == expected => {
                if bad.code.as_deref() == Some("BadDigest")
                    || bad.code.as_deref() == Some("XAmzContentChecksumMismatch")
                    || bad.code.as_deref() == Some("InvalidDigest")
                {
                    Judgement::say(Verdict::Pass, ph!("judge.checksum.pass"), s)
                } else {
                    Judgement::say(
                        Verdict::Differs,
                        ph!(
                            "judge.checksum.differs",
                            p1 = format!("{:?}", bad.status),
                            p2 = format!("{:?}", bad.code)
                        ),
                        s,
                    )
                }
            }
            Some(v) => Judgement::say(
                Verdict::Fail,
                ph!("judge.checksum.fail.2", v = v, expected = expected),
                s,
            ),
            None => Judgement::say(Verdict::Differs, ph!("judge.checksum.differs.2"), s),
        }
    }
}

pub struct Multipart {
    pub alg: Algorithm,
    pub full_object: bool,
}

const PART: usize = 5 * 1024 * 1024;

#[async_trait]
impl Probe for Multipart {
    fn meta(&self) -> ProbeMeta {
        let mode = if self.full_object {
            "full-object"
        } else {
            "composite"
        };
        let aws = if self.full_object {
            "CreateMultipartUpload with x-amz-checksum-type: FULL_OBJECT; HEAD returns the CRC of the whole object (no -N suffix), type FULL_OBJECT."
        } else {
            "CreateMultipartUpload with x-amz-checksum-type: COMPOSITE; HEAD returns the checksum of the parts' checksums with a -N suffix, type COMPOSITE."
        };
        meta(
            &format!("checksum.{}.mpu-{mode}", lower(self.alg)),
            Category::Checksums,
            &format!("{} multipart, {mode}", self.alg.name()),
            &format!(
                "A two-part upload with {} part checksums, completed in {mode} mode.",
                self.alg.name()
            ),
            aws,
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key(&format!("mpu-ck-{}-{}", lower(self.alg), self.full_object));
        let kind = if self.full_object {
            "FULL_OBJECT"
        } else {
            "COMPOSITE"
        };
        let alg_name = self.alg.name();
        let Some(up) = cx
            .create_upload(
                "create",
                &k,
                &[
                    ("x-amz-checksum-algorithm", alg_name),
                    ("x-amz-checksum-type", kind),
                ],
            )
            .await
        else {
            return;
        };
        let p1 = data(PART, 1);
        let p2 = data(1024, 2);
        let h = self.alg.header();
        let c1 = self.alg.compute(&p1);
        let c2 = self.alg.compute(&p2);
        let e1 = cx
            .upload_part(
                "part-1",
                &k,
                &up,
                1,
                p1.clone(),
                &[(h.as_str(), c1.as_str())],
            )
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        let e2 = cx
            .upload_part(
                "part-2",
                &k,
                &up,
                2,
                p2.clone(),
                &[(h.as_str(), c2.as_str())],
            )
            .await
            .map(|r| etag_of(&r))
            .unwrap_or_default();
        let el = self.alg.element();
        let parts = vec![(1, e1, Some((el.clone(), c1))), (2, e2, Some((el, c2)))];
        let whole: Vec<u8> = [p1.as_ref(), p2.as_ref()].concat();
        let full = self.alg.compute(&whole);
        let mut hs: Vec<(&str, &str)> = vec![];
        if self.full_object {
            hs.push((h.as_str(), full.as_str()));
        }
        cx.complete("complete", &k, &up, &parts, &hs).await;
        cx.head("head", &k, &[("x-amz-checksum-mode", "ENABLED")])
            .await;
        let expected = if self.full_object {
            full.clone()
        } else {
            self.alg.composite(&[p1.as_ref(), p2.as_ref()])
        };
        cx.note("expected", expected);
        let _ = cx.client.abort_multipart_upload(&cx.bucket, &k, &up).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (create, complete, head) = (s.at("create"), s.at("complete"), s.at("head"));
        let expected = s.at("expected").body;
        let h = self.alg.header();
        // the upload and its parts are what the checksum is read from
        let failing = [create, s.at("part-1"), s.at("part-2"), complete]
            .into_iter()
            .find(|x| !x.ok());
        if let Some(failing) = failing {
            if let Some(j) = super::write_refused(s, &[failing.label.as_str()]) {
                return j;
            }
            return if failing.status == Some(400) || failing.not_implemented() {
                Judgement::say(
                    Verdict::Unsupported,
                    ph!("judge.checksum.unsupported.2", p1 = failing.describe()),
                    s,
                )
            } else {
                Judgement::say(
                    Verdict::Error,
                    ph!("judge.checksum.error.2", p1 = failing.describe()),
                    s,
                )
            };
        }
        let want_type = if self.full_object {
            "FULL_OBJECT"
        } else {
            "COMPOSITE"
        };
        match head.header(&h) {
            Some(v) if v == expected => match head.header("x-amz-checksum-type") {
                Some(t) if t == want_type => Judgement::say(
                    Verdict::Pass,
                    ph!("judge.checksum.pass.2", wanttype = want_type),
                    s,
                ),
                Some(t) => {
                    Judgement::say(Verdict::Differs, ph!("judge.checksum.differs.3", t = t), s)
                }
                None => Judgement::say(Verdict::Differs, ph!("judge.checksum.differs.4"), s),
            },
            Some(v) => {
                let composite_shape = v.contains('-');
                Judgement::say(
                    Verdict::Differs,
                    ph!(
                        "judge.checksum.differs.5",
                        v = v,
                        p1 = if composite_shape {
                            "composite"
                        } else {
                            "full-object"
                        },
                        expected = expected
                    ),
                    s,
                )
            }
            None => Judgement::say(Verdict::Differs, ph!("judge.checksum.differs.6"), s),
        }
    }
}

pub struct Trailing;

#[async_trait]
impl Probe for Trailing {
    fn meta(&self) -> ProbeMeta {
        meta(
            "checksum.trailing",
            Category::Checksums,
            "Trailing checksum (aws-chunked)",
            "PUT with Content-Encoding: aws-chunked, STREAMING-UNSIGNED-PAYLOAD-TRAILER and an x-amz-checksum-crc32 trailer: what current AWS SDKs send by default.",
            "200; the trailer's CRC32 is verified, stored and returned by HEAD; the stored body is the decoded payload (no chunk framing).",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let body = data(3000, 5);
        let crc = Algorithm::Crc32.compute(&body);
        let k = cx.key("trailing");
        let req = s3raw::S3Request::new("PutObject", http::Method::PUT)
            .bucket(&cx.bucket)
            .key(&k)
            .body(s3raw::Body::ChunkedTrailer {
                data: body,
                trailer_name: "x-amz-checksum-crc32".into(),
                trailer_value: crc.clone(),
            });
        cx.send("put", req).await;
        cx.head("head", &k, &[("x-amz-checksum-mode", "ENABLED")])
            .await;
        cx.note("expected", crc);
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (put, head) = (s.at("put"), s.at("head"));
        let expected = s.at("expected").body;
        if let Some(j) = super::write_refused(s, &["put"]) {
            return j;
        }
        if !put.ok() {
            return if put.not_implemented() || put.status == Some(400) || put.status == Some(403) {
                Judgement::say(
                    Verdict::Unsupported,
                    ph!("judge.checksum.trailing.unsupported", p1 = put.describe()),
                    s,
                )
            } else {
                Judgement::say(Verdict::Error, raw(put.describe()), s)
            };
        }
        if head.header("content-length") != Some("3000") {
            return Judgement::say(
                Verdict::Fail,
                ph!(
                    "judge.checksum.trailing.fail",
                    p1 = format!("{:?}", head.header("content-length"))
                ),
                s,
            );
        }
        match head.header("x-amz-checksum-crc32") {
            Some(v) if v == expected => {
                Judgement::say(Verdict::Pass, ph!("judge.checksum.trailing.pass"), s)
            }
            Some(v) => Judgement::say(
                Verdict::Fail,
                ph!("judge.checksum.trailing.fail.2", v = v, expected = expected),
                s,
            ),
            None => Judgement::say(Verdict::Differs, ph!("judge.checksum.trailing.differs"), s),
        }
    }
}
