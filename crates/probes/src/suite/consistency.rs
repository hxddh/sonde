use async_trait::async_trait;
use s3raw::ops::ListV2;

use super::meta;
use crate::Probe;
use crate::cx::{ProbeCx, data};
use crate::model::{Category, Judgement, ProbeMeta, Steps, Verdict};

pub fn probes() -> Vec<Box<dyn Probe>> {
    vec![
        Box::new(ListAfterWrite),
        Box::new(ReadAfterOverwrite),
        Box::new(ReadAfterDelete),
    ]
}

const N: usize = 20;

pub struct ListAfterWrite;

#[async_trait]
impl Probe for ListAfterWrite {
    fn meta(&self) -> ProbeMeta {
        meta(
            "consistency.list-after-write",
            Category::Consistency,
            "List after write",
            "Writes 20 keys, then lists them at once.",
            "Strong consistency (since December 2020): every acknowledged PUT appears in the next listing.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let p = cx.key("law/");
        for i in 0..N {
            cx.put(
                &format!("put-{i}"),
                &format!("{p}{i:02}"),
                data(4, i as u8),
                &[],
            )
            .await;
        }
        let r = cx
            .client
            .list_objects_v2(
                &cx.bucket,
                &ListV2 {
                    prefix: Some(p),
                    ..Default::default()
                },
            )
            .await;
        match r {
            Ok((page, resp)) => {
                cx.record("list", Ok(resp));
                cx.note("listed", page.contents.len().to_string());
            }
            Err(e) => {
                cx.record("list", Err(e));
            }
        }
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let puts =
            s.0.iter()
                .filter(|x| x.label.starts_with("put-") && x.ok())
                .count();
        let listed: usize = s.at("listed").body.parse().unwrap_or(0);
        if !s.at("list").ok() {
            return Judgement::new(Verdict::Error, "the listing failed", s);
        }
        if listed == puts {
            Judgement::new(
                Verdict::Pass,
                format!("all {puts} writes listed at once"),
                s,
            )
        } else {
            Judgement::new(
                Verdict::Fail,
                format!(
                    "only {listed} of {puts} acknowledged writes listed: listings are eventually consistent"
                ),
                s,
            )
        }
    }
}

pub struct ReadAfterOverwrite;

#[async_trait]
impl Probe for ReadAfterOverwrite {
    fn meta(&self) -> ProbeMeta {
        meta(
            "consistency.read-after-overwrite",
            Category::Consistency,
            "Read after overwrite",
            "Overwrites a key and reads it at once, five times.",
            "Strong read-after-write: every GET returns the latest body.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("rao");
        for i in 0..5u8 {
            cx.put(&format!("put-{i}"), &k, format!("version-{i}"), &[])
                .await;
            cx.get(&format!("get-{i}"), &k, &[]).await;
        }
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let stale: Vec<u8> = (0..5u8)
            .filter(|i| s.at(&format!("get-{i}")).body != format!("version-{i}"))
            .collect();
        if stale.is_empty() {
            Judgement::new(Verdict::Pass, "every read saw the latest write", s)
        } else {
            Judgement::new(
                Verdict::Fail,
                format!("{} of 5 reads returned an older body", stale.len()),
                s,
            )
        }
    }
}

pub struct ReadAfterDelete;

#[async_trait]
impl Probe for ReadAfterDelete {
    fn meta(&self) -> ProbeMeta {
        meta(
            "consistency.read-after-delete",
            Category::Consistency,
            "Read after delete",
            "Deletes a key and reads it at once.",
            "GET and HEAD answer 404 NoSuchKey right after DeleteObject; the key leaves listings.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let k = cx.key("rad");
        cx.put("put", &k, data(4, 9), &[]).await;
        cx.delete("delete", &k).await;
        cx.get("get", &k, &[]).await;
        cx.head("head", &k, &[]).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (get, head) = (s.at("get"), s.at("head"));
        if get.ok() || head.ok() {
            Judgement::new(Verdict::Fail, "the deleted object was still readable", s)
        } else if get.is(404) && head.is(404) {
            Judgement::new(Verdict::Pass, "404 at once", s)
        } else {
            Judgement::new(
                Verdict::Differs,
                format!(
                    "answered {:?} / {:?} instead of 404",
                    get.status, head.status
                ),
                s,
            )
        }
    }
}
