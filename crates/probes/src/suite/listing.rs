use async_trait::async_trait;
use s3raw::ops::ListV2;

use super::meta;
use crate::Probe;
use crate::cx::{ProbeCx, data};
use crate::model::{Category, Judgement, ProbeMeta, Steps, Verdict};

pub fn probes() -> Vec<Box<dyn Probe>> {
    vec![
        Box::new(SpecialKeys),
        Box::new(TokenStability),
        Box::new(StartAfter),
        Box::new(DeleteBatchLimit),
    ]
}

/// Keys that trip encoders: spaces, `+`, `%`, reserved characters, dot
/// segments, doubled slashes, non-ASCII, a trailing space.
pub const SPECIAL: &[&str] = &[
    "a b",
    "plus+sign",
    "percent%20",
    "amp&eq=semi;",
    "日本語/ファイル",
    "emoji-🦀",
    "ümlaut-é",
    "double//slash",
    "./dot",
    "a/../b",
    "trailing ",
    "tilde~star*",
    "quote'\"",
    "#hash?q",
];

pub struct SpecialKeys;

#[async_trait]
impl Probe for SpecialKeys {
    fn meta(&self) -> ProbeMeta {
        meta(
            "keys.special",
            Category::Keys,
            "Special characters and Unicode in keys",
            "PUTs, GETs and lists 14 keys with spaces, +, %, reserved characters, dot segments, // and non-ASCII.",
            "Keys are opaque UTF-8: every key round-trips through PUT, GET and ListObjectsV2 (with encoding-type=url) exactly.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let p = cx.key("special/");
        for (i, k) in SPECIAL.iter().enumerate() {
            cx.put(
                &format!("put-{i}"),
                &format!("{p}{k}"),
                format!("body-{i}"),
                &[],
            )
            .await;
            cx.get(&format!("get-{i}"), &format!("{p}{k}"), &[]).await;
        }
        match cx
            .client
            .list_objects_v2(
                &cx.bucket,
                &ListV2 {
                    prefix: Some(p.clone()),
                    url_encoding: true,
                    ..Default::default()
                },
            )
            .await
        {
            Ok((page, resp)) => {
                cx.record("list", Ok(resp));
                let keys: Vec<String> = page
                    .contents
                    .iter()
                    .map(|o| o.key.strip_prefix(&p).unwrap_or(&o.key).to_string())
                    .collect();
                cx.note("listed", serde_json::to_string(&keys).unwrap_or_default());
            }
            Err(e) => {
                cx.record("list", Err(e));
            }
        }
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let listed: Vec<String> = serde_json::from_str(&s.at("listed").body).unwrap_or_default();
        let mut bad = vec![];
        for (i, k) in SPECIAL.iter().enumerate() {
            let put = s.at(&format!("put-{i}"));
            let get = s.at(&format!("get-{i}"));
            if !put.ok() {
                bad.push(format!("{k:?}: PUT {:?}", put.status));
            } else if get.body != format!("body-{i}") {
                bad.push(format!("{k:?}: GET {:?}", get.status));
            } else if !listed.iter().any(|l| l == k) {
                bad.push(format!("{k:?}: not listed as such"));
            }
        }
        if bad.is_empty() {
            Judgement::new(
                Verdict::Pass,
                format!("all {} keys round-trip", SPECIAL.len()),
                s,
            )
        } else {
            let mut j = Judgement::new(
                if bad.len() == SPECIAL.len() {
                    Verdict::Fail
                } else {
                    Verdict::Differs
                },
                format!(
                    "{} of {} keys do not round-trip: {}",
                    bad.len(),
                    SPECIAL.len(),
                    bad.join("; ")
                ),
                s,
            );
            j.observations.push(format!("listed: {listed:?}"));
            j
        }
    }
}

pub struct TokenStability;

#[async_trait]
impl Probe for TokenStability {
    fn meta(&self) -> ProbeMeta {
        meta(
            "list.token-stability",
            Category::Listing,
            "Continuation token stability",
            "Lists 30 keys 10 at a time; between pages, adds keys before and after the position, then reuses the first token.",
            "A continuation token is a position: later pages continue after the last key returned, include keys written after it, never repeat keys, and the same token can be used again.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let p = cx.key("tok/");
        for i in 0..30 {
            cx.put(
                &format!("put-{i}"),
                &format!("{p}k{:02}", i * 2),
                data(1, 0),
                &[],
            )
            .await;
        }
        let mut keys = vec![];
        let mut token: Option<String> = None;
        let mut first_token = None;
        for page_no in 0..10 {
            let r = cx
                .client
                .list_objects_v2(
                    &cx.bucket,
                    &ListV2 {
                        prefix: Some(p.clone()),
                        max_keys: Some(10),
                        continuation_token: token.clone(),
                        ..Default::default()
                    },
                )
                .await;
            let Some((page, resp)) = r.ok() else {
                cx.note("error", "listing failed");
                break;
            };
            cx.record(&format!("page-{page_no}"), Ok(resp));
            keys.extend(
                page.contents
                    .iter()
                    .map(|o| o.key.strip_prefix(&p).unwrap_or(&o.key).to_string()),
            );
            if page_no == 0 {
                first_token = page.next_continuation_token.clone();
                // one key before the position, one after
                cx.put("insert-before", &format!("{p}k01"), data(1, 0), &[])
                    .await;
                cx.put("insert-after", &format!("{p}k99"), data(1, 0), &[])
                    .await;
            }
            if !page.is_truncated {
                break;
            }
            token = page.next_continuation_token;
        }
        cx.note("keys", serde_json::to_string(&keys).unwrap_or_default());
        if let Some(t) = first_token {
            let r = cx
                .client
                .list_objects_v2(
                    &cx.bucket,
                    &ListV2 {
                        prefix: Some(p.clone()),
                        max_keys: Some(10),
                        continuation_token: Some(t),
                        ..Default::default()
                    },
                )
                .await;
            match r {
                Ok((page, resp)) => {
                    cx.record("reuse", Ok(resp));
                    cx.note(
                        "reused-first",
                        page.contents
                            .first()
                            .map(|o| o.key.strip_prefix(&p).unwrap_or(&o.key).to_string())
                            .unwrap_or_default(),
                    );
                }
                Err(e) => {
                    cx.record("reuse", Err(e));
                }
            }
        }
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let keys: Vec<String> = serde_json::from_str(&s.at("keys").body).unwrap_or_default();
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        if sorted.len() != keys.len() {
            return Judgement::new(Verdict::Fail, "pages repeated keys", s);
        }
        if sorted != keys {
            return Judgement::new(Verdict::Fail, "pages out of order", s);
        }
        let has_after = keys.iter().any(|k| k == "k99");
        let has_before = keys.iter().any(|k| k == "k01");
        let reuse_ok = s.at("reuse").ok() && s.at("reused-first").body == "k20";
        match (has_after, has_before, reuse_ok) {
            (true, false, true) => Judgement::new(
                Verdict::Pass,
                "positional tokens: later pages continue after the position and the token can be reused",
                s,
            ),
            (false, _, _) => Judgement::new(
                Verdict::Differs,
                "a key written after the position was not returned: the listing is a snapshot",
                s,
            ),
            (_, true, _) => Judgement::new(
                Verdict::Differs,
                "a key before the position appeared in a later page",
                s,
            ),
            (_, _, false) => Judgement::new(
                Verdict::Differs,
                format!(
                    "reusing the first token answered {:?} starting at {:?}",
                    s.at("reuse").status,
                    s.at("reused-first").body
                ),
                s,
            ),
        }
    }
}

pub struct StartAfter;

#[async_trait]
impl Probe for StartAfter {
    fn meta(&self) -> ProbeMeta {
        meta(
            "list.start-after",
            Category::Listing,
            "ListObjectsV2 StartAfter",
            "Lists with StartAfter set to an existing key, to a key between two keys, and to a key outside the prefix.",
            "Keys strictly after StartAfter, in UTF-8 binary order; StartAfter is echoed in the response.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let p = cx.key("sa/");
        for k in ["a", "b", "c", "d"] {
            cx.put(&format!("put-{k}"), &format!("{p}{k}"), data(1, 0), &[])
                .await;
        }
        for (label, after) in [
            ("existing", format!("{p}b")),
            ("between", format!("{p}bb")),
            ("before-prefix", cx.prefix.clone()),
        ] {
            match cx
                .client
                .list_objects_v2(
                    &cx.bucket,
                    &ListV2 {
                        prefix: Some(p.clone()),
                        start_after: Some(after),
                        ..Default::default()
                    },
                )
                .await
            {
                Ok((page, resp)) => {
                    cx.record(label, Ok(resp));
                    cx.note(
                        &format!("{label}-keys"),
                        page.contents
                            .iter()
                            .map(|o| o.key.strip_prefix(&p).unwrap_or(&o.key).to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                }
                Err(e) => {
                    cx.record(label, Err(e));
                }
            }
        }
    }
    fn judge(&self, s: &Steps) -> Judgement {
        if let Some(j) = super::setup_failed(s, &["put-a", "put-b", "put-c", "put-d"]) {
            return j;
        }
        let got = |l: &str| s.at(&format!("{l}-keys")).body;
        let ok = got("existing") == "c,d"
            && got("between") == "c,d"
            && got("before-prefix") == "a,b,c,d";
        if ok {
            Judgement::new(Verdict::Pass, "keys strictly after StartAfter", s)
        } else if !s.at("existing").ok() {
            Judgement::new(Verdict::Unsupported, "StartAfter refused", s)
        } else {
            Judgement::new(
                Verdict::Fail,
                format!(
                    "existing → {:?}, between → {:?}, before prefix → {:?}",
                    got("existing"),
                    got("between"),
                    got("before-prefix")
                ),
                s,
            )
        }
    }
}

pub struct DeleteBatchLimit;

#[async_trait]
impl Probe for DeleteBatchLimit {
    fn meta(&self) -> ProbeMeta {
        meta(
            "delete.batch-limit",
            Category::Delete,
            "DeleteObjects batch limit",
            "DeleteObjects with 1000 keys, then with 1001 (keys that do not exist: nothing is deleted for real).",
            "1000 keys: 200, each key reported deleted (deleting a missing key succeeds). 1001 keys: 400 MalformedXML.",
            &[],
        )
    }
    async fn run(&self, cx: &ProbeCx) {
        let keys = |n: usize| -> Vec<(String, Option<String>)> {
            (0..n)
                .map(|i| (cx.key(&format!("nope-{i:04}")), None))
                .collect()
        };
        let send = |label: &'static str, objs: Vec<(String, Option<String>)>| async move {
            let body = s3raw::xml::delete_body(&objs, false);
            let md5 = {
                use base64::Engine as _;
                use md5::Digest as _;
                base64::engine::general_purpose::STANDARD.encode(md5::Md5::digest(body.as_bytes()))
            };
            let req = s3raw::S3Request::new("DeleteObjects", http::Method::POST)
                .bucket(&cx.bucket)
                .query("delete", "")
                .header("content-md5", md5)
                .header("content-type", "application/xml")
                .body(body);
            let r = cx.send(label, req).await;
            if let Some(r) = r {
                cx.note(
                    &format!("{label}-deleted"),
                    s3raw::xml::delete_result(&r.text())
                        .map(|d| d.deleted.len())
                        .unwrap_or(0)
                        .to_string(),
                );
            }
        };
        send("batch-1000", keys(1000)).await;
        send("batch-1001", keys(1001)).await;
    }
    fn judge(&self, s: &Steps) -> Judgement {
        let (ok, over) = (s.at("batch-1000"), s.at("batch-1001"));
        let deleted: usize = s.at("batch-1000-deleted").body.parse().unwrap_or(0);
        if ok.not_implemented() {
            return Judgement::new(Verdict::Unsupported, "DeleteObjects is not implemented", s);
        }
        if !ok.ok() {
            return Judgement::new(
                Verdict::Fail,
                format!("1000 keys refused: {}", ok.describe()),
                s,
            );
        }
        if over.ok() {
            Judgement::new(
                Verdict::Differs,
                "1001 keys accepted (no 1000-key limit)",
                s,
            )
        } else if over.code.as_deref() == Some("MalformedXML") && deleted == 1000 {
            Judgement::new(Verdict::Pass, "1000 accepted, 1001 MalformedXML", s)
        } else {
            Judgement::new(
                Verdict::Differs,
                format!(
                    "1001 keys refused with {:?} {:?}; {deleted} of 1000 reported deleted",
                    over.status, over.code
                ),
                s,
            )
        }
    }
}
