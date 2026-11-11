use async_trait::async_trait;

use super::meta;
use crate::Probe;
use crate::cx::ProbeCx;
use crate::model::{Category, Judgement, ProbeMeta, Steps, Verdict};
use crate::phrase::{ph, raw};

pub fn probes() -> Vec<Box<dyn Probe>> {
    vec![
        Box::new(Bounds),
        Box::new(MultiRange),
        Box::new(Unsatisfiable),
    ]
}

const BODY: &str = "0123456789";

pub struct Bounds;

#[async_trait]
impl Probe for Bounds {
    fn meta(&self) -> ProbeMeta {
        meta(
            "range.bounds",
            Category::Range,
            "Range boundaries",
            "GETs bytes=0-0, bytes=9-9, bytes=-3 (suffix), bytes=7- (open) and bytes=5-100 (past the end) of a 10-byte object.",
            "206 Partial Content for each, with Content-Range bytes a-b/10; a range past the end is cut to the object (bytes 5-9/10).",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("range");
        cx.put("put", &k, BODY, &[]).await;
        for (label, r) in [
            ("first", "bytes=0-0"),
            ("last", "bytes=9-9"),
            ("suffix", "bytes=-3"),
            ("open", "bytes=7-"),
            ("past-end", "bytes=5-100"),
        ] {
            cx.get(label, &k, &[("range", r)]).await;
        }
    }
    fn judge(&self, s: &Steps) -> Judgement {
        if let Some(j) = super::setup_failed(s, &["put"]) {
            return j;
        }
        let want = [
            ("first", "0", "bytes 0-0/10"),
            ("last", "9", "bytes 9-9/10"),
            ("suffix", "789", "bytes 7-9/10"),
            ("open", "789", "bytes 7-9/10"),
            ("past-end", "56789", "bytes 5-9/10"),
        ];
        let mut wrong_body = vec![];
        let mut other = vec![];
        for (label, body, cr) in want {
            let st = s.at(label);
            if st.ok() && st.body != body {
                wrong_body.push(label);
            } else if !st.is(206) || st.header("content-range") != Some(cr) {
                other.push(format!(
                    "{label}: {:?} {:?}",
                    st.status,
                    st.header("content-range")
                ));
            }
        }
        if !wrong_body.is_empty() {
            Judgement::say(
                Verdict::Fail,
                ph!("judge.range.bounds.fail", p1 = wrong_body.join(", ")),
                s,
            )
        } else if other.is_empty() {
            Judgement::say(Verdict::Pass, ph!("judge.range.bounds.pass"), s)
        } else {
            Judgement::say(Verdict::Differs, raw(other.join("; ")), s)
        }
    }
}

pub struct MultiRange;

#[async_trait]
impl Probe for MultiRange {
    fn meta(&self) -> ProbeMeta {
        meta(
            "range.multi",
            Category::Range,
            "Multiple ranges",
            "GET with Range: bytes=0-1,4-5.",
            "S3 does not serve multiple ranges: it ignores such a header and answers 200 with the whole object.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("multirange");
        cx.put("put", &k, BODY, &[]).await;
        cx.get("get", &k, &[("range", "bytes=0-1,4-5")]).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        if let Some(j) = super::setup_failed(s, &["put"]) {
            return j;
        }
        let g = s.at("get");
        if g.is(200) && g.body == BODY {
            Judgement::say(Verdict::Pass, ph!("judge.range.multi.pass"), s)
        } else if g.is(206)
            && g.header("content-type")
                .is_some_and(|c| c.starts_with("multipart/byteranges"))
        {
            Judgement::say(Verdict::Differs, ph!("judge.range.multi.differs"), s)
        } else if g.is(206) {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.range.multi.differs.2",
                    p1 = format!("{:?}", g.header("content-range"))
                ),
                s,
            )
        } else {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.range.multi.differs.3",
                    p1 = format!("{:?}", g.status),
                    p2 = format!("{:?}", g.code)
                ),
                s,
            )
        }
    }
}

pub struct Unsatisfiable;

#[async_trait]
impl Probe for Unsatisfiable {
    fn meta(&self) -> ProbeMeta {
        meta(
            "range.unsatisfiable",
            Category::Range,
            "Range past the end",
            "GET bytes=100-200 of a 10-byte object, and bytes=0- of an empty object.",
            "416 InvalidRange with Content-Range: bytes */10; on an empty object 416 as well.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("unsat");
        cx.put("put", &k, BODY, &[]).await;
        cx.get("past", &k, &[("range", "bytes=100-200")]).await;
        let e = cx.key("empty");
        cx.put("put-empty", &e, "", &[]).await;
        cx.get("empty", &e, &[("range", "bytes=0-")]).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        if let Some(j) = super::setup_failed(s, &["put", "put-empty"]) {
            return j;
        }
        let (past, empty) = (s.at("past"), s.at("empty"));
        if past.ok() {
            return Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.range.unsatisfiable.differs",
                    p1 = format!("{:?}", past.status)
                ),
                s,
            );
        }
        if past.is(416) && past.code.as_deref() == Some("InvalidRange") && empty.is(416) {
            Judgement::say(Verdict::Pass, ph!("judge.range.unsatisfiable.pass"), s)
        } else {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.range.unsatisfiable.differs.2",
                    p1 = format!("{:?}", past.status),
                    p2 = format!("{:?}", past.code),
                    p3 = format!("{:?}", empty.status)
                ),
                s,
            )
        }
    }
}
