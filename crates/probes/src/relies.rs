//! What a program relies on, and how a store answers: the probes that match
//! the features a run's requests used (checksums, conditional writes,
//! ranges, multipart, batch deletes, SSE-C, presigned URLs, paged listings),
//! run against one store. A replay ends with them; `probe` is the whole
//! catalogue.

use std::collections::BTreeMap;
use std::sync::Arc;

use reqlog::Record;
use serde::{Deserialize, Serialize};
use sonde_core::Sonde;

use crate::model::{ProbeMeta, Run, Verdict};
use crate::phrase::{Phrase, en, ph, raw};
use crate::runner::RunOptions;

/// A feature a run used, and the probe that checks it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reliance {
    pub probe: String,
    /// The kind of request that showed it first, and how many requests did.
    pub kind: String,
    pub requests: usize,
    /// What showed it: a header as sent, a batch's size.
    pub seen: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen_text: Vec<Phrase>,
}

/// A [`Reliance`] and the store's verdict on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relied {
    #[serde(flatten)]
    pub on: Reliance,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub title_text: Vec<Phrase>,
    pub verdict: Verdict,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_text: Vec<Phrase>,
}

fn header<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.request_headers()
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn has_query(r: &Record, name: &str) -> bool {
    r.query().iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
}

const ALGORITHMS: [&str; 5] = ["crc32", "crc32c", "crc64nvme", "sha1", "sha256"];

/// The probes `r` calls for, each with what showed it.
fn features(r: &Record) -> Vec<(String, Phrase)> {
    let mut out: Vec<(String, Phrase)> = Vec::new();
    let mut add = |id: String, seen: Phrase| out.push((id, seen));
    let said = |name: &str| header(r, name).map(|v| raw(format!("{name}: {v}")));
    let kind = r.kind.as_str();
    // checksums: a single object's, a trailing one, a multipart upload's
    if kind == "PutObject" || kind == "UploadPart" {
        if let Some(t) = header(r, "x-amz-trailer") {
            add(
                "checksum.trailing".into(),
                raw(format!("x-amz-trailer: {t}")),
            );
        } else if kind == "PutObject" {
            for a in ALGORITHMS {
                let name = format!("x-amz-checksum-{a}");
                if header(r, &name).is_some() {
                    add(format!("checksum.{a}.single"), raw(name));
                }
            }
        }
    }
    if kind == "CreateMultipartUpload"
        && let Some(a) = header(r, "x-amz-checksum-algorithm")
    {
        let a = a.trim().to_ascii_lowercase();
        let t = header(r, "x-amz-checksum-type").map(|t| t.trim().to_ascii_uppercase());
        // AWS's own default: full-object for CRC64NVME, composite for the rest
        let full = match t.as_deref() {
            Some(t) => t == "FULL_OBJECT",
            None => a == "crc64nvme",
        };
        let seen = match &t {
            Some(t) => format!(
                "x-amz-checksum-algorithm: {}, x-amz-checksum-type: {t}",
                a.to_ascii_uppercase()
            ),
            None => format!("x-amz-checksum-algorithm: {}", a.to_ascii_uppercase()),
        };
        let mode = if full { "full-object" } else { "composite" };
        add(format!("checksum.{a}.mpu-{mode}"), raw(seen));
    }
    // conditional writes
    match kind {
        "PutObject" => {
            if let Some(s) = said("if-none-match") {
                add("cond.put-if-none-match".into(), s);
            }
            if let Some(s) = said("if-match") {
                add("cond.put-if-match".into(), s);
            }
        }
        "CopyObject" => {
            if let Some(s) = said("x-amz-copy-source-if-match") {
                add("cond.copy-if-match".into(), s);
            }
        }
        "CompleteMultipartUpload" => {
            if let Some(s) = said("if-none-match") {
                add("cond.complete-if-none-match".into(), s);
            }
        }
        _ => {}
    }
    // ranges
    if kind == "GetObject"
        && let Some(v) = header(r, "range")
    {
        let id = if v.contains(',') {
            "range.multi"
        } else {
            "range.bounds"
        };
        add(id.into(), raw(format!("range: {v}")));
    }
    // multipart
    match kind {
        "CreateMultipartUpload" => add("mpu.min-part-size".into(), raw(kind)),
        "UploadPartCopy" => add("mpu.upload-part-copy".into(), raw(kind)),
        _ => {}
    }
    // a batch delete, by its size
    if kind == "DeleteObjects" {
        let n = r
            .rare()
            .request_body
            .as_deref()
            .map_or(0, |b| b.matches("<Key>").count());
        add(
            "delete.batch-limit".into(),
            ph!("relies.batch", n = n, s = crate::phrase::plural(n)),
        );
    }
    // encryption with the client's own key
    if let Some(a) = header(r, "x-amz-server-side-encryption-customer-algorithm") {
        add(
            "sse-c".into(),
            raw(format!(
                "x-amz-server-side-encryption-customer-algorithm: {a}"
            )),
        );
    }
    // a presigned URL: its signature in the query
    if has_query(r, "X-Amz-Signature") {
        add("presign.clock-skew".into(), raw("X-Amz-Signature"));
    }
    // a listing read page by page
    if reqlog::is_listing(kind) {
        if has_query(r, "continuation-token") {
            add("list.token-stability".into(), raw("continuation-token"));
        }
        if has_query(r, "start-after") {
            add("list.start-after".into(), raw("start-after"));
        }
    }
    out
}

/// What `records` rely on: each probe that matches a feature they used,
/// once, in the catalogue's order, with the kind of request that showed it first
/// and how many requests did. A batch delete says its largest batch.
pub fn relied_on(records: &[Record]) -> Vec<Reliance> {
    let catalog = crate::catalog();
    let mut found: BTreeMap<String, Reliance> = BTreeMap::new();
    for r in records {
        for (id, seen) in features(r) {
            if !catalog.iter().any(|m| m.id == id) {
                continue;
            }
            let first = !found.contains_key(&id);
            let e = found.entry(id.clone()).or_insert_with(|| Reliance {
                probe: id,
                kind: r.kind.to_string(),
                requests: 0,
                seen: String::new(),
                seen_text: Vec::new(),
            });
            e.requests += 1;
            // a batch says its largest; the rest, what showed it first
            let n = |p: &Phrase| p.args.get("n").and_then(|n| n.parse::<usize>().ok());
            let larger = seen.key == "relies.batch" && e.seen_text.first().and_then(n) < n(&seen);
            if first || larger {
                e.seen = en(std::slice::from_ref(&seen));
                // words only where there are words to translate: a header is itself
                e.seen_text = if seen.key == "raw" {
                    Vec::new()
                } else {
                    vec![seen]
                };
            }
        }
    }
    catalog.iter().filter_map(|m| found.remove(&m.id)).collect()
}

/// Runs the probes of `relies` against `endpoint`, in `bucket` (under
/// `sonde-probe/`, removed after; audited as any probe run), and gives each
/// its verdict.
pub async fn answer(
    s: &Arc<Sonde>,
    endpoint: &str,
    bucket: &str,
    relies: &[Reliance],
) -> Result<Vec<Relied>, String> {
    if relies.is_empty() {
        return Ok(Vec::new());
    }
    let opts = RunOptions {
        bucket: bucket.to_string(),
        include_slow: false,
        only: relies.iter().map(|r| r.probe.clone()).collect(),
        ..Default::default()
    };
    let ids = crate::runner::spawn(s, &[endpoint.to_string()], opts);
    let id = ids.first().ok_or("no probe run")?;
    let v = s.jobs().wait(id).await.map_err(|e| e.message)?;
    let run: Run = serde_json::from_value(v).map_err(|e| e.to_string())?;
    Ok(verdicts(relies, &run, &crate::catalog()))
}

/// Each reliance with its probe's result in `run` (one that did not run is
/// skipped).
fn verdicts(relies: &[Reliance], run: &Run, catalog: &[ProbeMeta]) -> Vec<Relied> {
    relies
        .iter()
        .map(|on| {
            let meta = catalog.iter().find(|m| m.id == on.probe);
            let result = run.results.iter().find(|x| x.id == on.probe);
            Relied {
                on: on.clone(),
                title: meta.map(|m| m.title.clone()).unwrap_or_default(),
                title_text: meta.map(|m| m.title_text.clone()).unwrap_or_default(),
                verdict: result.map_or(Verdict::Skipped, |x| x.verdict),
                summary: result.map(|x| x.summary.clone()).unwrap_or_default(),
                summary_text: result.map(|x| x.summary_text.clone()).unwrap_or_default(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(kind: &str, headers: &[(&str, &str)]) -> Record {
        let mut r = Record {
            kind: kind.into(),
            method: "PUT".into(),
            bucket: Some("b".into()),
            key: Some("k".into()),
            status: Some(200),
            ..Default::default()
        };
        r.lists_mut().request_headers = headers
            .iter()
            .map(|(n, v)| reqlog::Header::new(n, v))
            .collect();
        r
    }

    #[test]
    fn a_crc32_composite_multipart_upload_relies_on_its_probe() {
        let runs = [
            rec(
                "CreateMultipartUpload",
                &[
                    ("x-amz-checksum-algorithm", "CRC32"),
                    ("x-amz-checksum-type", "COMPOSITE"),
                ],
            ),
            rec("UploadPart", &[("x-amz-checksum-crc32", "AAAAAA==")]),
            rec("UploadPart", &[("x-amz-checksum-crc32", "AAAAAA==")]),
        ];
        let r = relied_on(&runs);
        let ids: Vec<&str> = r.iter().map(|x| x.probe.as_str()).collect();
        assert_eq!(ids, ["checksum.crc32.mpu-composite", "mpu.min-part-size"]);
        assert_eq!(r[0].kind, "CreateMultipartUpload");
        assert_eq!(
            r[0].seen,
            "x-amz-checksum-algorithm: CRC32, x-amz-checksum-type: COMPOSITE"
        );
    }

    #[test]
    fn only_what_the_run_used_is_listed_once_each() {
        let mut del = rec("DeleteObjects", &[]);
        del.rare_mut().request_body = Some(
            "<Delete><Object><Key>a</Key></Object><Object><Key>b</Key></Object></Delete>".into(),
        );
        let runs = [
            rec(
                "PutObject",
                &[("if-none-match", "*"), ("x-amz-checksum-crc32c", "x")],
            ),
            rec("PutObject", &[("if-none-match", "*")]),
            rec("GetObject", &[("range", "bytes=0-99")]),
            rec("GetObject", &[]),
            del,
        ];
        let r = relied_on(&runs);
        let ids: Vec<&str> = r.iter().map(|x| x.probe.as_str()).collect();
        assert_eq!(
            ids,
            [
                "cond.put-if-none-match",
                "checksum.crc32c.single",
                "range.bounds",
                "delete.batch-limit"
            ]
        );
        assert_eq!(r[0].requests, 2);
        assert_eq!(r[3].seen, "2 keys in one DeleteObjects at most");
        assert!(relied_on(&[rec("GetObject", &[])]).is_empty());
    }
}
