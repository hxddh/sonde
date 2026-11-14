//! Every `-f json` output is what its JSON Schema in docs/schemas says
//! (docs/contract.md): the real binary, the demo's session and a small S3 in
//! this process. A field a schema requires that goes, or changes its type,
//! fails here; a field added does not (schemas leave objects open).
//!
//! The schemas keep to a part of JSON Schema 2020-12 that this file checks
//! itself: `type`, `properties`, `required`, `items`, `prefixItems`, `enum`,
//! `const`, `anyOf`, `$ref` into `$defs`, `additionalProperties` and the
//! annotations. A keyword outside it is an error, so no schema says more
//! than is checked.

#[path = "../../proxy/tests/common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use common::MiniS3;
use s3raw::{Body, ClientConfig, Credentials, S3Client, S3Request, StaticProvider};
use serde_json::Value;

fn schemas_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/schemas")
}

const ANNOTATIONS: &[&str] = &[
    "$schema",
    "$id",
    "$comment",
    "title",
    "description",
    "examples",
    "$defs",
];

/// Where `v` is not what `schema` says, each with its place (`at`).
fn check(schema: &Value, root: &Value, v: &Value, at: &str, out: &mut Vec<String>) {
    let Some(s) = schema.as_object() else {
        if schema == &Value::Bool(false) {
            out.push(format!("{at}: nothing is allowed here"));
        }
        return;
    };
    for k in s.keys() {
        let known = ANNOTATIONS.contains(&k.as_str())
            || [
                "type",
                "properties",
                "required",
                "items",
                "prefixItems",
                "enum",
                "const",
                "anyOf",
                "$ref",
                "additionalProperties",
            ]
            .contains(&k.as_str());
        if !known {
            out.push(format!("{at}: keyword {k} is not one this test checks"));
        }
    }
    if let Some(r) = s.get("$ref").and_then(Value::as_str) {
        let name = r.strip_prefix("#/$defs/").expect("a $ref into $defs");
        match root["$defs"].get(name) {
            Some(d) => check(d, root, v, at, out),
            None => out.push(format!("{at}: no $defs/{name}")),
        }
    }
    if let Some(t) = s.get("type") {
        let types: Vec<&str> = match t {
            Value::String(t) => vec![t],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        let is = |t: &str| match t {
            "null" => v.is_null(),
            "boolean" => v.is_boolean(),
            "integer" => v.is_i64() || v.is_u64(),
            "number" => v.is_number(),
            "string" => v.is_string(),
            "array" => v.is_array(),
            "object" => v.is_object(),
            _ => false,
        };
        if !types.iter().any(|t| is(t)) {
            let shown: String = v.to_string().chars().take(80).collect();
            out.push(format!("{at}: {shown} is not {types:?}"));
            return;
        }
    }
    if let Some(e) = s.get("enum").and_then(Value::as_array)
        && !e.contains(v)
    {
        out.push(format!("{at}: {v} is not one of {e:?}"));
    }
    if let Some(c) = s.get("const")
        && c != v
    {
        out.push(format!("{at}: {v} is not {c}"));
    }
    if let Some(any) = s.get("anyOf").and_then(Value::as_array) {
        let fits = any.iter().any(|a| {
            let mut e = Vec::new();
            check(a, root, v, at, &mut e);
            e.is_empty()
        });
        if !fits {
            out.push(format!("{at}: none of anyOf fits"));
        }
    }
    if let Some(o) = v.as_object() {
        let props = s.get("properties").and_then(Value::as_object);
        for r in s
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let r = r.as_str().unwrap();
            if !o.contains_key(r) {
                out.push(format!("{at}: no {r}"));
            }
        }
        for (k, x) in o {
            match props.and_then(|p| p.get(k)) {
                Some(p) => check(p, root, x, &format!("{at}.{k}"), out),
                None => {
                    if let Some(extra) = s.get("additionalProperties") {
                        check(extra, root, x, &format!("{at}.{k}"), out);
                    }
                }
            }
        }
    }
    if let Some(a) = v.as_array() {
        let first = s
            .get("prefixItems")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (i, x) in a.iter().enumerate() {
            let place = format!("{at}[{i}]");
            match first.get(i) {
                Some(p) => check(p, root, x, &place, out),
                None => {
                    if let Some(items) = s.get("items") {
                        check(items, root, x, &place, out);
                    }
                }
            }
        }
    }
}

fn schema(name: &str) -> Value {
    let path = schemas_dir().join(format!("{name}.schema.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let s: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    assert_eq!(
        s["$schema"], "https://json-schema.org/draft/2020-12/schema",
        "{name}"
    );
    assert_eq!(
        s["$id"],
        format!(
            "https://raw.githubusercontent.com/hxddh/sonde/main/docs/schemas/{name}.schema.json"
        ),
        "{name}"
    );
    s
}

fn holds(name: &str, doc: &Value) {
    // (SONDE_SCHEMA_DUMP=dir keeps each output, to write a schema from)
    if let Ok(d) = std::env::var("SONDE_SCHEMA_DUMP") {
        let _ = std::fs::write(
            Path::new(&d).join(format!("{name}.json")),
            serde_json::to_vec_pretty(doc).unwrap(),
        );
    }
    let s = schema(name);
    let mut errs = Vec::new();
    check(&s, &s, doc, "$", &mut errs);
    assert!(errs.is_empty(), "{name}:\n{}", errs.join("\n"));
}

struct Bench {
    store: MiniS3,
    dir: tempfile::TempDir,
}

impl Bench {
    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    /// `sonde <args>`'s stdout, as JSON; it must succeed (or exit `code`).
    async fn json(&self, args: &[&str], code: i32) -> Value {
        let o = tokio::process::Command::new(env!("CARGO_BIN_EXE_sonde"))
            .current_dir(self.dir.path())
            .env("SONDE_CONFIG_DIR", self.path("config"))
            .env("SONDE_DATA_DIR", self.path("data"))
            .env("AWS_EC2_METADATA_DISABLED", "true")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await
            .unwrap();
        assert_eq!(
            o.status.code(),
            Some(code),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
            panic!(
                "{args:?}: {e}\n{}",
                String::from_utf8_lossy(&o.stdout)
                    .chars()
                    .take(400)
                    .collect::<String>()
            )
        });
        v
    }
}

/// A store with a bucket, an object and an upload never completed.
async fn store() -> MiniS3 {
    let store = MiniS3::start(true).await;
    let cfg = ClientConfig::new("t", &store.url, "us-east-1").unwrap();
    let c = S3Client::new(
        cfg,
        Arc::new(StaticProvider(Some(Credentials::new("k", "s", None)))),
        reqlog::RequestLog::default(),
    )
    .unwrap();
    for r in [
        S3Request::new("CreateBucket", http::Method::PUT).bucket("data"),
        S3Request::new("PutObject", http::Method::PUT)
            .bucket("data")
            .key("logs/a.json")
            .body(Body::from("{}")),
        S3Request::new("CreateMultipartUpload", http::Method::POST)
            .bucket("data")
            .key("big")
            .query("uploads", ""),
    ] {
        c.send_raw(r).await.unwrap();
    }
    store
}

#[tokio::test(flavor = "multi_thread")]
async fn every_json_output_is_as_its_schema_says() {
    let b = Bench {
        store: store().await,
        dir: tempfile::tempdir().unwrap(),
    };
    // two stores: a replay on several at once writes the same prefix on each
    let other = store().await;
    std::fs::create_dir_all(b.path("config")).unwrap();
    let endpoint = |id: &str, url: &str| {
        format!(
            "[[endpoint]]\nid = \"{id}\"\nname = \"{id}\"\nurl = \"{url}\"\nregion = \"us-east-1\"\npath-style = true\ncredentials = {{ kind = \"static\", access-key-id = \"k\", secret-access-key = \"s\" }}\n\n"
        )
    };
    std::fs::write(
        b.path("config/config.toml"),
        format!(
            "{}{}",
            endpoint("a", &b.store.url),
            endpoint("b", &other.url)
        ),
    )
    .unwrap();
    let demo = tokio::process::Command::new(env!("CARGO_BIN_EXE_sonde"))
        .env("SONDE_CONFIG_DIR", b.path("config"))
        .env("SONDE_DATA_DIR", b.path("data"))
        .args(["run", "--demo"])
        .output()
        .await
        .unwrap();
    assert_eq!(demo.status.code(), Some(0));
    let session = std::fs::read_dir(b.path("data/sessions"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "sonde"))
        .unwrap();
    let s = session.to_str().unwrap();
    // the run tried under a fault, and its program tried before: a verdict
    // with its history
    {
        use sonde_proxy::trial;
        let clean = sonde_proxy::session::Session::open(&session).unwrap();
        let slowdown = sonde_proxy::fault::scenario("slowdown").unwrap();
        let now = trial::judge(&slowdown, &clean.records, 0, &clean.records, 0);
        trial::save(&session, std::slice::from_ref(&now)).unwrap();
        let mut then = now.clone();
        then.standing = trial::Standing::Broke;
        let dir = session.parent().unwrap();
        let day = chrono::Utc::now() - chrono::Duration::days(1);
        trial::record(dir, "sonde run --demo", "earlier.sonde", day, &[then]).unwrap();
        let name = session.file_name().unwrap().to_string_lossy();
        trial::record(dir, "sonde run --demo", &name, chrono::Utc::now(), &[now]).unwrap();
    }

    // reading a session
    holds("show", &b.json(&["show", s, "-f", "json"], 0).await);
    holds(
        "show",
        &b.json(&["show", s, "--errors", "-f", "json"], 0).await,
    );
    let summary = b.json(&["show", s, "--summary", "-f", "json"], 0).await;
    assert_eq!(summary["trials"][0]["history"].as_array().unwrap().len(), 2);
    holds("show-summary", &summary);
    for by in ["kind", "prefix", "size", "caller", "sdk", "status"] {
        holds(
            "show-by",
            &b.json(&["show", s, "--by", by, "-f", "json"], 0).await,
        );
    }
    holds(
        "check",
        &b.json(&["check", s, "--max-requests", "1", "-f", "json"], 1)
            .await,
    );
    holds(
        "check",
        &b.json(&["check", s, "--baseline", s, "-f", "json"], 0)
            .await,
    );
    holds(
        "compare",
        &b.json(&["compare", s, s, "-f", "json"], 0).await,
    );
    holds("sessions", &b.json(&["sessions", "-f", "json"], 0).await);

    // replaying it on the small store, and the two compared
    let replayed = b.path("replay.sonde");
    let r = replayed.to_str().unwrap();
    holds(
        "compare-replay",
        &b.json(
            &[
                "compare",
                s,
                "--replay",
                "a",
                "--bucket",
                "demo=data",
                "--yes",
                "-o",
                r,
                "-f",
                "json",
            ],
            2,
        )
        .await,
    );
    holds(
        "compare-answers",
        &b.json(&["compare", s, r, "-f", "json"], 2).await,
    );
    holds(
        "compare-replay-stores",
        &b.json(
            &[
                "compare",
                s,
                "--replay",
                "a",
                "--replay",
                "b",
                "--bucket",
                "demo=data",
                "--yes",
                "-f",
                "json",
            ],
            2,
        )
        .await,
    );

    // the store itself
    holds(
        "endpoints-list",
        &b.json(&["endpoints", "list", "-f", "json"], 0).await,
    );
    holds(
        "endpoints-test",
        &b.json(&["endpoints", "test", "a", "-f", "json"], 0).await,
    );
    holds(
        "bucket-size",
        &b.json(
            &["bucket", "size", "-e", "a", "-b", "data", "-f", "json"],
            0,
        )
        .await,
    );
    holds(
        "bucket-uploads",
        &b.json(
            &["bucket", "uploads", "-e", "a", "-b", "data", "-f", "json"],
            4,
        )
        .await,
    );
    holds(
        "layout-inspect",
        &b.json(
            &[
                "layout", "inspect", "-e", "a", "-b", "data", "--kind", "generic", "-f", "json",
            ],
            0,
        )
        .await,
    );
    holds(
        "probe-list",
        &b.json(&["probe", "list", "-f", "json"], 0).await,
    );
    let run = b
        .json(
            &[
                "probe",
                "-e",
                "a",
                "-b",
                "data",
                "--only",
                "list.",
                "--no-slow",
                "-f",
                "json",
            ],
            // (the small store breaks a guarantee or two)
            1,
        )
        .await;
    holds("probe", &run);
    let runs = b.json(&["probe", "runs", "-f", "json"], 0).await;
    holds("probe-runs", &runs);
    let one = format!("{}:a", runs[0]["id"].as_str().unwrap());
    holds(
        "probe-diff",
        &b.json(&["probe", "diff", &one, &one, "-f", "json"], 0)
            .await,
    );
}

/// The checker says what is wrong, and refuses a keyword it does not check.
#[test]
fn the_checker_finds_what_does_not_hold() {
    let s = serde_json::json!({
        "type": "object",
        "properties": { "a": { "type": "integer" }, "b": { "$ref": "#/$defs/pair" } },
        "required": ["a"],
        "$defs": { "pair": { "type": "array", "prefixItems": [{ "enum": ["x"] }], "items": false } }
    });
    let errs = |v: Value| {
        let mut e = Vec::new();
        check(&s, &s, &v, "$", &mut e);
        e
    };
    assert!(errs(serde_json::json!({ "a": 1, "b": ["x"], "c": 0 })).is_empty());
    assert_eq!(errs(serde_json::json!({})), ["$: no a"]);
    assert_eq!(errs(serde_json::json!({ "a": 1.5 })).len(), 1);
    assert_eq!(errs(serde_json::json!({ "a": 1, "b": ["y"] })).len(), 1);
    assert_eq!(errs(serde_json::json!({ "a": 1, "b": ["x", 2] })).len(), 1);
    let loose = serde_json::json!({ "minimum": 1 });
    let mut e = Vec::new();
    check(&loose, &loose, &serde_json::json!(2), "$", &mut e);
    assert!(e[0].contains("minimum"), "{e:?}");
}

/// Every schema is one the test above reads, and each says what it is of.
#[test]
fn every_schema_is_checked_and_documented() {
    let checked = [
        "show",
        "show-summary",
        "show-by",
        "check",
        "compare",
        "sessions",
        "compare-replay",
        "compare-replay-stores",
        "compare-answers",
        "endpoints-list",
        "endpoints-test",
        "bucket-size",
        "bucket-uploads",
        "layout-inspect",
        "probe-list",
        "probe",
        "probe-runs",
        "probe-diff",
    ];
    let mut found: Vec<String> = std::fs::read_dir(schemas_dir())
        .unwrap()
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_suffix(".schema.json")
                .map(String::from)
        })
        .collect();
    found.sort();
    let mut want: Vec<String> = checked.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(found, want);
    for name in checked {
        let s = schema(name);
        assert!(
            s["title"].is_string() && s["description"].is_string(),
            "{name}"
        );
    }
}

/// The contract promises figures and ids, not wording: every phrase array
/// (`…Text`) and `layout inspect`'s display tables say so, those tables are
/// not required, and no output names an S3 call `operation` (it is a
/// `kind`; an operation is a program's).
#[test]
fn display_text_is_marked_and_an_s3_call_is_a_kind() {
    fn walk(v: &Value, name: &str, out: &mut Vec<String>) {
        match v {
            Value::Object(o) => {
                if let Some(Value::Object(props)) = o.get("properties") {
                    let required: Vec<&str> = o
                        .get("required")
                        .and_then(Value::as_array)
                        .map(|r| r.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_default();
                    for (k, p) in props {
                        let said = p["description"]
                            .as_str()
                            .is_some_and(|d| d.starts_with("Display text, not stable"));
                        let layout = name == "layout-inspect"
                            && ["headline", "facts", "tables"].contains(&k.as_str());
                        if (k.ends_with("Text") || layout) && !said {
                            out.push(format!("{name}: {k} is not marked display text"));
                        }
                        if layout && required.contains(&k.as_str()) {
                            out.push(format!("{name}: {k} is required"));
                        }
                        if k == "operation" {
                            out.push(format!("{name}: an S3 call named `operation`"));
                        }
                    }
                }
                for x in o.values() {
                    walk(x, name, out);
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, name, out)),
            _ => {}
        }
    }
    let mut wrong = Vec::new();
    for e in std::fs::read_dir(schemas_dir()).unwrap().flatten() {
        let file = e.file_name().to_string_lossy().into_owned();
        let Some(name) = file.strip_suffix(".schema.json") else {
            continue;
        };
        walk(&schema(name), name, &mut wrong);
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
