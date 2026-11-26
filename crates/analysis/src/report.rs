//! A run made safe to hand to someone outside (`sonde share --redact`):
//! names of buckets and keys become stable tokens, and everything a request
//! carried besides its shape is dropped. The page itself is the viewer's
//! ([`crate::share`]).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::run::RunFile;

/// Path segments that say what a layout is, not what the data is: kept.
const STRUCTURAL: &[&str] = &[
    "_delta_log",
    "_last_checkpoint",
    "metadata",
    "data",
    "manifest",
    "manifests",
    "wal",
    "compacted",
    "compactions",
    "snapshots",
    "_lakefs",
    "tmp",
];

/// File types a key keeps when redacted: the kind of data, not its name.
/// Anything else after a dot (`john.smith`) is part of the name.
const FILE_TYPES: &[&str] = &[
    "parquet",
    "orc",
    "avro",
    "arrow",
    "ipc",
    "json",
    "jsonl",
    "ndjson",
    "csv",
    "tsv",
    "txt",
    "log",
    "xml",
    "yaml",
    "yml",
    "md",
    "html",
    "sst",
    "wal",
    "manifest",
    "metadata",
    "checkpoint",
    "crc",
    "idx",
    "db",
    "sqlite",
    "pb",
    "bin",
    "dat",
    "gz",
    "zst",
    "lz4",
    "snappy",
    "bz2",
    "xz",
    "zip",
    "tar",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "webp",
    "mov",
    "mp4",
    "pdf",
];

/// Query parameters whose values say how, not what: kept as they are.
const PLAIN_QUERY: &[&str] = &[
    "list-type",
    "max-keys",
    "max-uploads",
    "max-parts",
    "delimiter",
    "encoding-type",
    "fetch-owner",
    "partNumber",
    "part-number-marker",
    "uploads",
    "versions",
    "versioning",
    "x-id",
    "acl",
    "tagging",
    "lifecycle",
    "location",
    "policy",
    "cors",
    "delete",
    "attributes",
    "restore",
    "object-lock",
    "retention",
    "legal-hold",
    "torrent",
    "select",
    "select-type",
];

struct Redactor {
    salt: u64,
}

impl Redactor {
    fn token(&self, s: &str) -> String {
        let mut h = DefaultHasher::new();
        self.salt.hash(&mut h);
        s.hash(&mut h);
        format!("{:08x}", h.finish() as u32)
    }

    fn segment(&self, seg: &str) -> String {
        if seg.is_empty()
            || STRUCTURAL.contains(&seg)
            || seg
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '_')
        {
            return seg.to_string();
        }
        // keep what kind of file it is: `.parquet`, `.json`, `.sst`
        match seg.rsplit_once('.') {
            Some((stem, ext))
                if !stem.is_empty() && FILE_TYPES.contains(&ext.to_ascii_lowercase().as_str()) =>
            {
                // a numbered file keeps its number: 00000000000000000010.json
                if stem.chars().all(|c| c.is_ascii_digit()) {
                    seg.to_string()
                } else {
                    format!("{}.{ext}", self.token(stem))
                }
            }
            _ => self.token(seg),
        }
    }

    fn path(&self, p: &str) -> String {
        p.split('/')
            .map(|s| self.segment(s))
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// Makes a run safe to hand on: bucket names and every key segment
/// become tokens (the same name, the same token, within this run; the
/// layout's own names and file types are kept), and headers, bodies,
/// request ids, URLs, the program's output and its arguments are dropped.
pub fn redact(s: &mut RunFile) {
    redact_all(&mut [s]);
}

/// [`redact`] of runs shown together (a run and its baseline): the same
/// name, the same token, in each, so what they share still compares.
pub fn redact_all(runs: &mut [&mut RunFile]) {
    let r = Redactor {
        salt: rand::random(),
    };
    for s in runs {
        redact_with(s, &r);
    }
}

fn redact_with(s: &mut RunFile, r: &Redactor) {
    s.header.url = String::new();
    s.header.endpoint = "endpoint".into();
    s.header.region = String::new();
    s.header.cwd = None;
    s.header.ca_file = None;
    // the commit says which change; a branch's name may say whose
    if let Some(g) = &mut s.header.git {
        g.branch = None;
    }
    // the program's name only: never a variable set before it (a key, say)
    s.header.command = s
        .header
        .command
        .as_deref()
        .map(|c| crate::program::program_name(c).unwrap_or_default());
    s.logs.clear();
    for rec in &mut s.records {
        rec.bucket = rec
            .bucket
            .as_deref()
            .map(|b| format!("bucket-{}", r.token(b)).into());
        rec.key = rec.key.as_deref().map(|k| r.path(k).into());
        if rec.lists.is_some() {
            let l = rec.lists_mut();
            for (k, v) in &mut l.query {
                if matches!(
                    k.as_str(),
                    "prefix" | "start-after" | "marker" | "key-marker"
                ) {
                    *v = r.path(v).into();
                } else if !PLAIN_QUERY.contains(&k.as_str()) && !v.is_empty() {
                    // ids, tokens, response-* overrides (a file name), an app's own
                    *v = r.token(v).into();
                }
            }
            l.request_headers.clear();
            l.response_headers.clear();
        }
        let x = rec.rare_mut();
        x.endpoint = "endpoint".into();
        x.url = String::new();
        x.job = None;
        // what failed, not where: the message names the URL
        x.transport_error = x
            .transport_error
            .as_ref()
            .map(|_| "the request did not reach the store".to_string());
        x.request_body = None;
        x.request_id = None;
        x.host_id = None;
        // who asked (an IAM ARN: account and user) and from where
        x.requester = None;
        x.region = None;
        // the S3 error code says what went wrong; the message may name things
        x.error_body = x.error_body.as_deref().and_then(|b| {
            let code = b.split("<Code>").nth(1)?.split("</Code>").next()?;
            Some(format!("<Error><Code>{code}</Code></Error>"))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::RequestPrices;
    use crate::run::RunHeader;
    use chrono::DateTime;
    use reqlog::Record;

    fn run() -> RunFile {
        let rec = |id: u64, key: &str| {
            Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 10).unwrap(),
            method: "GET".into(),
            kind: "GetObject".into(),
            bucket: Some("acme-payroll".into()),
            key: Some(key.into()),
            status: Some(404),
            latency_ms: 4.0,
            rare: reqlog::Rare {
                url: "https://acme-payroll.s3.amazonaws.com/x".into(),
                error_body: Some("<Error><Code>NoSuchKey</Code><Message>acme-payroll/2026/alice.parquet</Message></Error>".into()),
                request_id: Some("REQ".into()),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }
        };
        RunFile {
            header: RunHeader::new(
                "prod",
                "https://s3.amazonaws.com",
                "us-east-1",
                Some("/opt/etl/run --secret=x".into()),
            ),
            records: vec![
                rec(1, "2026/alice.parquet"),
                rec(2, "2026/alice.parquet"),
                rec(3, "tables/t/_delta_log/00000000000000000010.json"),
            ],
            logs: vec![],
            afresh: false,
        }
    }

    #[test]
    fn a_redacted_run_keeps_its_shape_and_none_of_its_names() {
        let mut s = run();
        redact(&mut s);
        let page =
            serde_json::to_string(&crate::share::run_json(&s, &RequestPrices::default())).unwrap();
        for secret in [
            "acme",
            "alice",
            "payroll",
            "prod",
            "--secret",
            "REQ",
            "amazonaws",
        ] {
            assert!(!page.contains(secret), "{secret} leaked");
        }
        // the same key, the same token; the layout's names and numbers kept
        assert_eq!(s.records[0].key, s.records[1].key);
        assert!(s.records[0].key.as_deref().unwrap().starts_with("2026/"));
        assert!(s.records[0].key.as_deref().unwrap().ends_with(".parquet"));
        assert!(
            s.records[2]
                .key
                .as_deref()
                .unwrap()
                .ends_with("/_delta_log/00000000000000000010.json")
        );
        assert_eq!(
            s.records[0].rare().error_body.as_deref(),
            Some("<Error><Code>NoSuchKey</Code></Error>")
        );
        assert_eq!(s.header.command.as_deref(), Some("run"));
    }

    #[test]
    fn nothing_that_names_the_data_survives_redaction() {
        let mut s = run();
        let r = &mut s.records[0];
        r.key = Some("hr/john.smith".into());
        r.rare_mut().job = Some("payroll-alice".into());
        r.rare_mut().transport_error = Some(
            "error sending request for url (https://acme-payroll.s3.amazonaws.com/hr/alice)".into(),
        );
        r.lists_mut().query = vec![
            (
                "response-content-disposition".into(),
                "attachment; filename=alice-salary.csv".into(),
            ),
            ("list-type".into(), "2".into()),
        ];
        redact(&mut s);
        let doc = serde_json::to_string(&s.records).unwrap();
        for secret in ["smith", "alice", "payroll", "amazonaws"] {
            assert!(!doc.contains(secret), "{secret} leaked: {doc}");
        }
        assert!(doc.contains("\"list-type\",\"2\""));
    }

    #[test]
    fn who_sent_a_request_and_from_which_region_do_not_survive_redaction() {
        let mut s = run();
        s.records[0].rare_mut().requester = Some("arn:aws:iam::123456789012:user/alice".into());
        s.records[0].rare_mut().region = Some("eu-west-3".into());
        s.header.region = "eu-west-3".into();
        redact(&mut s);
        let doc =
            serde_json::to_string(&crate::share::run_json(&s, &RequestPrices::default())).unwrap();
        for secret in ["123456789012", "alice", "eu-west-3"] {
            assert!(!doc.contains(secret), "{secret} leaked");
        }
    }
}
