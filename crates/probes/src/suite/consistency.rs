use async_trait::async_trait;
use s3raw::ops::ListV2;

use super::{meta, setup_failed};
use crate::Probe;
use crate::cx::{ProbeCx, data};
use crate::model::{Category, Judgement, ProbeMeta, Steps, Verdict};
use crate::phrase::ph;

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
            return Judgement::say(
                Verdict::Error,
                ph!("judge.consistency.list-after-write.error"),
                s,
            );
        }
        // a pass needs writes to list: none acknowledged is "could not run"
        if puts == 0 {
            return Judgement::say(
                Verdict::Error,
                ph!(
                    "judge.consistency.list-after-write.error.2",
                    p1 = s.at("put-0").describe()
                ),
                s,
            );
        }
        if listed == puts {
            Judgement::say(
                Verdict::Pass,
                ph!("judge.consistency.list-after-write.pass", puts = puts),
                s,
            )
        } else {
            Judgement::say(
                Verdict::Fail,
                ph!(
                    "judge.consistency.list-after-write.fail",
                    listed = listed,
                    puts = puts
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
        let puts: Vec<String> = (0..5u8).map(|i| format!("put-{i}")).collect();
        let puts: Vec<&str> = puts.iter().map(String::as_str).collect();
        if let Some(j) = setup_failed(s, &puts) {
            return j;
        }
        let stale: Vec<u8> = (0..5u8)
            .filter(|i| s.at(&format!("get-{i}")).body != format!("version-{i}"))
            .collect();
        if stale.is_empty() {
            Judgement::say(
                Verdict::Pass,
                ph!("judge.consistency.read-after-overwrite.pass"),
                s,
            )
        } else {
            Judgement::say(
                Verdict::Fail,
                ph!(
                    "judge.consistency.read-after-overwrite.fail",
                    p1 = stale.len()
                ),
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
        // a 404 says nothing when there was never an object to delete
        if let Some(j) = setup_failed(s, &["put", "delete"]) {
            return j;
        }
        let (get, head) = (s.at("get"), s.at("head"));
        if get.ok() || head.ok() {
            Judgement::say(
                Verdict::Fail,
                ph!("judge.consistency.read-after-delete.fail"),
                s,
            )
        } else if get.is(404) && head.is(404) {
            Judgement::say(
                Verdict::Pass,
                ph!("judge.consistency.read-after-delete.pass"),
                s,
            )
        } else {
            Judgement::say(
                Verdict::Differs,
                ph!(
                    "judge.consistency.read-after-delete.differs",
                    p1 = format!("{:?}", get.status),
                    p2 = format!("{:?}", head.status)
                ),
                s,
            )
        }
    }
}
