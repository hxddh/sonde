//! Every `-f json` output is what its JSON Schema in docs/schemas says
//! (docs/contract.md): the real binary, the demo's run and a small S3 in
//! this process. A field a schema requires that goes, or changes its type,
//! fails here. The schemas leave objects open (a reader takes a field it does
//! not know); this test reads them strictly, both ways: a field an output
//! holds that its schema does not name fails, and so does a property a schema
//! names that no output here holds (or [`UNEXERCISED`] lists, with why). So a
//! field added is described in the same commit, and one described is real.
//!
//! The schemas keep to a part of JSON Schema 2020-12 that this file checks
//! itself: `type`, `properties`, `required`, `items`, `prefixItems`, `enum`,
//! `const`, `anyOf`, `$ref` into `$defs`, `additionalProperties` and the
//! annotations. A keyword outside it is an error, so no schema says more
//! than is checked.

#[path = "../../proxy/tests/common/mod.rs"]
mod common;

use std::collections::{BTreeMap, BTreeSet};
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

/// What checking outputs against a schema found: where one is not what the
/// schema says, and, for the strict reading, what the outputs hold that the
/// schema does not name and which of the properties it names they held.
#[derive(Default, Clone)]
struct Seen {
    /// Where an output is not what its schema says, each with its place.
    errors: Vec<String>,
    /// A property no schema names: its place with array positions left out
    /// (`$.findings[].fix.patch`) → the first place and output it was seen in.
    undocumented: BTreeMap<String, String>,
    /// The schema's properties an output held, as JSON pointers into the
    /// schema (`/properties/since/properties/unsaved`).
    exercised: BTreeSet<String>,
}

impl Seen {
    fn merge(&mut self, other: Seen) {
        self.errors.extend(other.errors);
        for (k, v) in other.undocumented {
            self.undocumented.entry(k).or_insert(v);
        }
        self.exercised.extend(other.exercised);
    }
}

/// A place with its array positions left out: one line a property, however
/// many items hold it.
fn every(at: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in at.chars() {
        match c {
            '[' => {
                inside = true;
                out.push('[');
            }
            ']' => {
                inside = false;
                out.push(']');
            }
            _ if inside => {}
            c => out.push(c),
        }
    }
    out
}

/// Checks `v` (at `at`) against `schema` (at the JSON pointer `ptr` of
/// `root`), as the schema is written and as a strict reader would take it:
/// an object's property the schema does not name is noted unless the schema
/// says the object is a map (`additionalProperties`). The published schemas
/// stay open; the strict reading is this test's, so no output gains a field
/// without its schema saying so.
fn check(schema: &Value, root: &Value, v: &Value, at: &str, ptr: &str, out: &mut Seen) {
    let Some(s) = schema.as_object() else {
        if schema == &Value::Bool(false) {
            out.errors.push(format!("{at}: nothing is allowed here"));
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
            out.errors
                .push(format!("{at}: keyword {k} is not one this test checks"));
        }
    }
    // (one way to say what an object holds a node: the strict reading asks
    // each node alone)
    let ways = ["$ref", "anyOf", "properties"]
        .iter()
        .filter(|k| s.contains_key(**k))
        .count();
    if ways > 1 {
        out.errors.push(format!(
            "{at}: schema {ptr} mixes $ref, anyOf and properties, which this test does not read"
        ));
    }
    if let Some(r) = s.get("$ref").and_then(Value::as_str) {
        let name = r.strip_prefix("#/$defs/").expect("a $ref into $defs");
        match root["$defs"].get(name) {
            Some(d) => check(d, root, v, at, &format!("/$defs/{name}"), out),
            None => out.errors.push(format!("{at}: no $defs/{name}")),
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
            out.errors.push(format!("{at}: {shown} is not {types:?}"));
            return;
        }
    }
    if let Some(e) = s.get("enum").and_then(Value::as_array)
        && !e.contains(v)
    {
        out.errors.push(format!("{at}: {v} is not one of {e:?}"));
    }
    if let Some(c) = s.get("const")
        && c != v
    {
        out.errors.push(format!("{at}: {v} is not {c}"));
    }
    if let Some(any) = s.get("anyOf").and_then(Value::as_array) {
        // the branches that fit; of those, the ones that name the most of
        // what is there say what was exercised and what no branch names
        let fits: Vec<Seen> = any
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let mut e = Seen::default();
                check(a, root, v, at, &format!("{ptr}/anyOf/{i}"), &mut e);
                e.errors.is_empty().then_some(e)
            })
            .collect();
        match fits.iter().map(|f| f.undocumented.len()).min() {
            None => out.errors.push(format!("{at}: none of anyOf fits")),
            Some(least) => fits
                .into_iter()
                .filter(|f| f.undocumented.len() == least)
                .for_each(|f| out.merge(f)),
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
                out.errors.push(format!("{at}: no {r}"));
            }
        }
        // (a node that only points elsewhere leaves the naming to where it points)
        let elsewhere = s.contains_key("$ref") || s.contains_key("anyOf");
        for (k, x) in o {
            let place = format!("{at}.{k}");
            match props.and_then(|p| p.get(k)) {
                Some(p) => {
                    let named = format!("{ptr}/properties/{k}");
                    out.exercised.insert(named.clone());
                    check(p, root, x, &place, &named, out);
                }
                None => match s.get("additionalProperties") {
                    Some(extra) => check(
                        extra,
                        root,
                        x,
                        &place,
                        &format!("{ptr}/additionalProperties"),
                        out,
                    ),
                    None if elsewhere => {}
                    None => {
                        out.undocumented.entry(every(&place)).or_insert(place);
                    }
                },
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
                Some(p) => check(p, root, x, &place, &format!("{ptr}/prefixItems/{i}"), out),
                None => {
                    if let Some(items) = s.get("items") {
                        check(items, root, x, &place, &format!("{ptr}/items"), out);
                    }
                }
            }
        }
    }
}

/// Every property a schema names, as JSON pointers into it.
fn named(schema: &Value, ptr: &str, out: &mut BTreeSet<String>) {
    match schema {
        Value::Object(o) => {
            for (k, v) in o {
                let at = format!("{ptr}/{k}");
                if k == "properties"
                    && let Some(props) = v.as_object()
                {
                    for (name, p) in props {
                        let at = format!("{at}/{name}");
                        out.insert(at.clone());
                        named(p, &at, out);
                    }
                } else if !ANNOTATIONS.contains(&k.as_str()) || k == "$defs" {
                    if k == "$defs" {
                        for (name, d) in v.as_object().into_iter().flatten() {
                            named(d, &format!("{at}/{name}"), out);
                        }
                    } else {
                        named(v, &at, out);
                    }
                }
            }
        }
        Value::Array(a) => {
            for (i, v) in a.iter().enumerate() {
                named(v, &format!("{ptr}/{i}"), out);
            }
        }
        _ => {}
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

/// What the strict reading found in every output checked so far, a schema:
/// [`holds`] adds to it, [`strictly`] judges it once all are in.
static STRICT: parking_lot::Mutex<BTreeMap<String, Seen>> =
    parking_lot::Mutex::new(BTreeMap::new());

/// The command whose output is being checked, for the strict reading's messages.
static COMMAND: parking_lot::Mutex<String> = parking_lot::Mutex::new(String::new());

fn holds(name: &str, doc: &Value) {
    // (SONDE_SCHEMA_DUMP=dir keeps each output, to write a schema from)
    if let Ok(d) = std::env::var("SONDE_SCHEMA_DUMP") {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let _ = std::fs::write(
            Path::new(&d).join(format!("{name}.{n}.json")),
            serde_json::to_vec_pretty(doc).unwrap(),
        );
    }
    let s = schema(name);
    let mut seen = Seen::default();
    check(&s, &s, doc, "$", "", &mut seen);
    assert!(
        seen.errors.is_empty(),
        "{name}:\n{}",
        seen.errors.join("\n")
    );
    let command = COMMAND.lock().clone();
    for first in seen.undocumented.values_mut() {
        *first = format!("{first} of `{command}`");
    }
    STRICT
        .lock()
        .entry(name.to_string())
        .or_default()
        .merge(seen);
}

/// The schema's properties no output of this test holds, each with why: a
/// property is exercised here, or listed here. One listed that an output
/// does hold, or that no schema names, fails too: the list says what is so.
const UNEXERCISED: &[(&str, &str, &str)] = &[
    (
        "show",
        "/$defs/diagnosis/properties/also",
        "no run recorded here has a diagnosis of two findings",
    ),
    (
        "show",
        "/$defs/diagnosis/properties/serial",
        "no run recorded here has requests one after another that join another finding's diagnosis",
    ),
    (
        "replay",
        "/$defs/relied/properties/seenText",
        "only a batch delete is said in words, and the run replayed here sends none",
    ),
    (
        "replay-stores",
        "/$defs/relied/properties/seenText",
        "only a batch delete is said in words, and the run replayed here sends none",
    ),
    (
        "endpoints-test",
        "/properties/identity/properties/Arn",
        "who the credentials are is STS's to say, and only an AWS endpoint has one: none here",
    ),
    (
        "endpoints-test",
        "/properties/identity/properties/UserId",
        "who the credentials are is STS's to say, and only an AWS endpoint has one: none here",
    ),
    (
        "endpoints-test",
        "/properties/identity/properties/Account",
        "who the credentials are is STS's to say, and only an AWS endpoint has one: none here",
    ),
    (
        "endpoints-probes",
        "/$defs/record/properties/firstByteMs",
        "sonde's own client does not time the first byte: a probe's record has none",
    ),
    (
        "endpoints-probes",
        "/$defs/record/properties/injected",
        "a probe's request is sent to the store, never failed in its place",
    ),
    (
        "endpoints-probes",
        "/$defs/record/properties/requestBodyTruncated",
        "no probe sends a body longer than a record keeps",
    ),
    (
        "endpoints-probes",
        "/$defs/record/properties/requester",
        "who sent a request is an imported log's to say",
    ),
];

/// What is wrong in the strict reading of `seen`, the outputs checked
/// against schema `name`: each line names the schema, the place and the
/// property, and what to do about it.
fn strict_findings(
    name: &str,
    schema: &Value,
    seen: &Seen,
    exempt: &[(&str, &str, &str)],
) -> Vec<String> {
    let mut wrong = Vec::new();
    for (place, first) in &seen.undocumented {
        let (object, property) = place.rsplit_once('.').unwrap_or(("$", place));
        wrong.push(format!(
            "{name}: undocumented property `{property}` of {object} (first seen at {first}): describe it in docs/schemas/{name}.schema.json"
        ));
    }
    let mut all = BTreeSet::new();
    named(schema, "", &mut all);
    let listed = |ptr: &str| exempt.iter().any(|(n, p, _)| *n == name && *p == ptr);
    for ptr in all.difference(&seen.exercised) {
        if !listed(ptr) {
            let property = ptr.rsplit('/').next().unwrap_or(ptr);
            wrong.push(format!(
                "{name}: unexercised property `{property}` (docs/schemas/{name}.schema.json#{ptr}): no output in crates/sonde-cli/tests/schemas.rs holds it; make one that does, or list it in UNEXERCISED with why"
            ));
        }
    }
    for (n, ptr, why) in exempt.iter().filter(|(n, _, _)| *n == name) {
        if !all.contains(*ptr) {
            wrong.push(format!(
                "{n}: UNEXERCISED lists {ptr} ({why}), which docs/schemas/{n}.schema.json does not name"
            ));
        } else if seen.exercised.contains(*ptr) {
            wrong.push(format!(
                "{n}: UNEXERCISED lists {ptr} ({why}), which an output now holds: take it off the list"
            ));
        }
    }
    wrong
}

/// Every output checked so far, read strictly: nothing in them the schemas
/// do not name, and nothing the schemas name that none of them held.
fn strictly(checked: &[&str]) {
    let seen = STRICT.lock().clone();
    let mut wrong = Vec::new();
    for name in checked {
        let found = seen.get(*name).cloned().unwrap_or_default();
        wrong.extend(strict_findings(name, &schema(name), &found, UNEXERCISED));
    }
    for (n, ptr, _) in UNEXERCISED {
        if !checked.contains(n) {
            wrong.push(format!(
                "UNEXERCISED lists {ptr} of {n}, which is no schema"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} place(s) where the outputs and docs/schemas disagree:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
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
        self.json_of(args, &[code]).await
    }

    /// [`Bench::json`], exiting with one of `codes`.
    async fn json_of(&self, args: &[&str], codes: &[i32]) -> Value {
        *COMMAND.lock() = format!("sonde {}", args.join(" "));
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
        assert!(
            o.status.code().is_some_and(|c| codes.contains(&c)),
            "{args:?}: {:?}, not one of {codes:?}: {}",
            o.status.code(),
            String::from_utf8_lossy(&o.stderr)
        );
        // (what a terminal acts on is escaped as JSON escapes it, never sent)
        let text = String::from_utf8_lossy(&o.stdout);
        assert!(
            !text.contains(['\u{7f}', '\u{202e}', '\u{9b}']),
            "{args:?}: {text}"
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

/// What a terminal acts on, in a key: DEL and the mark that turns text's
/// direction.
const PLANTED: &str = "logs/\u{202e}gpj\u{7f}.";

/// A store with a bucket, an object (and one with [`PLANTED`] in its key)
/// and an upload never completed.
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
        S3Request::new("PutObject", http::Method::PUT)
            .bucket("data")
            .key(&format!("{PLANTED}json"))
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

/// A store that lists one object and no uploads, and refuses everything
/// else, its versions too (403, with the ids and the body S3 sends): its URL.
async fn refusing() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut c, _)) = l.accept().await {
            tokio::spawn(async move {
                let mut seen = Vec::new();
                let mut buf = [0u8; 4096];
                while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                    match c.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => seen.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&seen);
                let line = head.lines().next().unwrap_or_default();
                let (status, body) = if line.starts_with("GET") && line.contains("uploads") {
                    (
                        "200 OK",
                        "<ListMultipartUploadsResult><Bucket>b</Bucket><IsTruncated>false</IsTruncated></ListMultipartUploadsResult>",
                    )
                } else if line.starts_with("GET") && line.contains("list-type=2") {
                    (
                        "200 OK",
                        "<ListBucketResult><Name>b</Name><KeyCount>1</KeyCount><IsTruncated>false</IsTruncated><Contents><Key>logs/a.json</Key><LastModified>2026-03-01T00:00:00.000Z</LastModified><ETag>\"e\"</ETag><Size>2048</Size><StorageClass>STANDARD</StorageClass></Contents></ListBucketResult>",
                    )
                } else {
                    (
                        "403 Forbidden",
                        "<Error><Code>AccessDenied</Code><Message>Access Denied</Message><RequestId>REQ1</RequestId><HostId>HOST1</HostId></Error>",
                    )
                };
                let answer = format!(
                    "HTTP/1.1 {status}\r\nx-amz-request-id: REQ1\r\nx-amz-id-2: HOST1\r\ncontent-type: application/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = c.write_all(answer.as_bytes()).await;
            });
        }
    });
    url
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
    // the demo's store (versions, delete markers, uploads with parts, a
    // lifecycle it takes), and one that refuses all but a listing of uploads
    let demo_store = sonde_core::demo::start().await.unwrap();
    let refusing = refusing().await;
    std::fs::write(
        b.path("config/config.toml"),
        format!(
            "{}{}{}{}{}",
            endpoint("a", &b.store.url),
            endpoint("b", &other.url),
            endpoint("d", &demo_store.url()),
            endpoint("x", &refusing),
            endpoint("c", "https://store.example").replace(
                "path-style = true",
                "path-style = true\nca-file = \"/etc/ssl/store.pem\""
            ),
        ),
    )
    .unwrap();
    // (the tour runs in a folder of its own: where its code would be)
    std::fs::create_dir_all(b.path("tour")).unwrap();
    let demo = tokio::process::Command::new(env!("CARGO_BIN_EXE_sonde"))
        .current_dir(b.path("tour"))
        .env("SONDE_CONFIG_DIR", b.path("config"))
        .env("SONDE_DATA_DIR", b.path("data"))
        .args(["run", "--demo"])
        .output()
        .await
        .unwrap();
    assert_eq!(demo.status.code(), Some(0));
    let run = std::fs::read_dir(b.path("data/runs"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "sonde"))
        .unwrap();
    let s = run.to_str().unwrap();
    // the run tried under a fault, and its program tried before: a verdict
    // with its history
    {
        use sonde_analysis::trial;
        let clean = sonde_analysis::run::RunFile::open(&run).unwrap();
        let slowdown = sonde_proxy::fault::scenario("slowdown").unwrap();
        let mut now = trial::judge(&slowdown, &clean.records, 0, &clean.records, 0);
        // one that degraded: the request it turns on, and what to change
        now.standing = trial::Standing::Degraded;
        now.finding = Some(trial::TrialFinding {
            why: vec![vec![trial::Said {
                key: "trial.why.reads".into(),
                args: [("n", "1"), ("s", ""), ("it", "it")]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            }]],
            at: Some(trial::Where {
                kind: "PutObject".into(),
                key: "b/state".into(),
                caller: Some("etl.py:12 in save".into()),
            }),
            sdk: Some("boto3 1.34".into()),
            advice: Some(trial::Said {
                key: "trial.advice.retry".into(),
                args: Default::default(),
            }),
        });
        // every figure a verdict may hold, so each is in the schema
        now.not_retryable = 1;
        now.read_again = 1;
        now.dropped_reads = 1;
        now.dropped_writes = 1;
        now.lost_writes = 1;
        now.checked = 1;
        now.reuploaded = 1;
        now.held = 2;
        now.waited = 1;
        now.stale = 3;
        now.relisted = 1;
        now.missed = 1;
        now.rewrote = 1;
        now.cut = 2;
        now.resumed = 1;
        now.reread = 1;
        now.races = 2;
        now.read_back = 1;
        now.yielded = 1;
        now.fenced = true;
        now.backoff_ms = Some([10.0, 20.0, 40.0]);
        now.jitter = Some(false);
        now.jitter_too_short = true;
        now.delayed = 1;
        now.left_uploads = 1;
        now.wrote_twice = 1;
        now.run = Some("tour.slowdown.sonde".into());
        // and one pressed harder than once (`slowdown:3`)
        let mut pressed = now.clone();
        pressed.times = Some(3);
        trial::save(&run, &[now.clone(), pressed]).unwrap();
        // the line of code its finding names, where the program ran
        let cwd = clean.header.cwd.clone().expect("the tour's folder");
        std::fs::write(
            Path::new(&cwd).join("etl.py"),
            format!(
                "{}    s3.put_object(Bucket=b, Key=\"state\")\n",
                "\n".repeat(11)
            ),
        )
        .unwrap();
        let mut then = now.clone();
        then.standing = trial::Standing::Broke;
        let dir = run.parent().unwrap();
        let day = chrono::Utc::now() - chrono::Duration::days(1);
        trial::record(dir, "tour", "earlier.sonde", day, &[then]).unwrap();
        let name = run.file_name().unwrap().to_string_lossy();
        trial::record(dir, "tour", &name, chrono::Utc::now(), &[now]).unwrap();
    }

    // a run of the tour's program where nothing reached sonde: `since.empty`, not compared
    {
        let empty = b.path("empty.sonde");
        let e = empty.to_str().unwrap();
        let o = tokio::process::Command::new(env!("CARGO_BIN_EXE_sonde"))
            .current_dir(b.dir.path())
            .env("SONDE_CONFIG_DIR", b.path("config"))
            .env("SONDE_DATA_DIR", b.path("data"))
            .args(["run", "--demo", "--name", "tour", "-o", e, "--", "true"])
            .stdin(Stdio::null())
            .output()
            .await
            .unwrap();
        assert_eq!(
            o.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        let shown = b.json(&["show", e, "-f", "json"], 0).await;
        assert_eq!(
            shown["since"]["empty"],
            Value::Bool(true),
            "{}",
            shown["since"]
        );
        holds("show", &shown);
    }
    // reading a run
    holds("show", &b.json(&["show", s, "-f", "json"], 0).await);
    holds(
        "show",
        &b.json(&["show", s, "--requests", "-f", "json"], 0).await,
    );
    // one with nothing found: no what-if, as the schema says
    {
        let quiet = b.path("quiet.log");
        std::fs::write(
            &quiet,
            "79a5 lake [06/Feb/2031:00:00:38 +0000] 192.0.2.3 - 3E57 REST.GET.OBJECT t/a \"GET /lake/t/a HTTP/1.1\" 200 - 4096 4096 12 10 \"-\" \"boto3/1.34\" - x SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -\n",
        )
        .unwrap();
        let q = b.path("quiet.sonde");
        let text = std::fs::read_to_string(&quiet).unwrap();
        let (_, run) = sonde_analysis::import::import(&text, "quiet").unwrap();
        sonde_analysis::import::save(&run, &q).unwrap();
        let q = q.to_str().unwrap();
        let shown = b.json(&["show", q, "-f", "json"], 0).await;
        assert!(shown["whatIf"].is_null(), "{}", shown["whatIf"]);
        // a log's time is no program's: no time account
        assert!(shown["time"].is_null(), "{}", shown["time"]);
        holds("show", &shown);
        holds(
            "show",
            &b.json(&["show", q, "--all", "-f", "json"], 0).await,
        );
    }
    // one kind of request of many: not few, so `few` is left out
    {
        let line = "79a5 lake [06/Feb/2031:00:00:38 +0000] 192.0.2.3 - 3E57 REST.GET.OBJECT t/a \"GET /lake/t/a HTTP/1.1\" 200 - 4096 4096 12 10 \"-\" \"boto3/1.34\" - x SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -\n";
        let (_, run) = sonde_analysis::import::import(&line.repeat(30), "many").unwrap();
        let m = b.path("many.sonde");
        sonde_analysis::import::save(&run, &m).unwrap();
        let m = m.to_str().unwrap();
        let summary = b.json(&["show", m, "-f", "json"], 0).await;
        // one table of the kinds of request: `waiting.byKind`
        assert!(summary["summary"].get("kinds").is_none(), "{summary}");
        holds("show", &summary);
        // an imported log's month, at the rate of its span
        assert!(summary["month"].is_null() || summary["month"]["from"] == "log");
    }
    // more keys than the table of a split holds: the rest is `other`
    {
        let lines: String = (0..14)
            .map(|i| format!("79a5 lake [06/Feb/2031:00:{:02}:38 +0000] 192.0.2.3 arn:aws:iam::1:user/etl 3E57 REST.GET.OBJECT t/k{i} \"GET /lake/t/k{i} HTTP/1.1\" 200 - 4096 4096 12 10 \"-\" \"boto3/1.34\" - x SigV4 ECDHE AuthHeader lake.s3.amazonaws.com TLSv1.3 - -\n", 10 + i))
            .collect();
        let (_, run) = sonde_analysis::import::import(&lines, "keys").unwrap();
        let k = b.path("keys.sonde");
        sonde_analysis::import::save(&run, &k).unwrap();
        let k = k.to_str().unwrap();
        let split = b.json(&["show", k, "--by", "key", "-f", "json"], 0).await;
        let last = split["split"]["rows"].as_array().unwrap().last().unwrap();
        assert_eq!(last["other"], true, "{last}");
        holds("show-by", &split);
        // a log's month, at the rate of its span; and who sent its requests
        assert_eq!(split["run"]["wholeSeconds"], true, "{}", split["run"]);
        let all = b.json(&["show", k, "--requests", "-f", "json"], 0).await;
        assert!(
            all["requests"][0]["requester"].is_string(),
            "{}",
            all["requests"][0]
        );
        assert_eq!(all["month"]["from"], "log", "{}", all["month"]);
        holds("show", &all);
    }
    // a trial's own run: its scenario, the clean run it is of, the requests
    // sonde failed in the store's place, and a body sent that was cut
    {
        let mut tried = sonde_analysis::run::RunFile::open(&run).unwrap();
        tried.header.scenario = Some("slowdown".into());
        tried.header.trial_of = Some(run.file_name().unwrap().to_string_lossy().into_owned());
        tried.header.git = Some(sonde_analysis::run::GitStamp {
            commit: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".into(),
            branch: Some("main".into()),
            dirty: true,
        });
        for r in tried.records.iter_mut().take(2) {
            let mut v = serde_json::to_value(&*r).unwrap();
            v["injected"] = "slowdown".into();
            v["status"] = 503.into();
            v["requestBody"] = "<Delete><Object><Key>a</Key></Object></Delete>".into();
            v["requestBodyTruncated"] = true.into();
            *r = serde_json::from_value(v).unwrap();
        }
        let file = b.path("tried.sonde");
        sonde_analysis::import::save(&tried, &file).unwrap();
        let t = file.to_str().unwrap();
        let shown = b.json(&["show", t, "--requests", "-f", "json"], 0).await;
        assert_eq!(shown["run"]["scenario"], "slowdown", "{}", shown["run"]);
        assert_eq!(shown["requests"][0]["injected"], "slowdown");
        holds("show", &shown);
        holds(
            "show",
            &b.json(&["show", t, "--all", "-f", "json"], 0).await,
        );
        holds(
            "show-by",
            &b.json(&["show", t, "--by", "kind", "-f", "json"], 0).await,
        );
        holds(
            "check",
            &b.json(&["check", t, "--max-requests", "100000", "-f", "json"], 0)
                .await,
        );
    }
    holds(
        "show",
        &b.json(&["show", s, "--errors", "-f", "json"], 0).await,
    );
    holds(
        "show",
        &b.json(&["show", s, "--all", "-f", "json"], 0).await,
    );
    let summary = b.json(&["show", s, "-f", "json"], 0).await;
    assert_eq!(summary["trials"][0]["history"].as_array().unwrap().len(), 2);
    // the plan (--all: by place): every finding in one diagnosis; where the time went
    let whole = b.json(&["show", s, "--all", "-f", "json"], 0).await;
    let findings = whole["findings"].as_array().unwrap().len();
    let mut placed: Vec<u64> = whole["diagnoses"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|d| {
            d["findings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i.as_u64().unwrap())
        })
        .collect();
    placed.sort_unstable();
    assert_eq!(
        placed,
        (0..findings as u64).collect::<Vec<_>>(),
        "{}",
        whole["diagnoses"]
    );
    assert!(summary["time"]["wallMs"].is_number(), "{}", summary["time"]);
    // a verdict is its table's row: why, where and what to change are a
    // finding of the run's, with its caller and its fix
    let t = &summary["trials"][0];
    for gone in ["where", "advice", "reasons", "reasonsText", "finding"] {
        assert!(t.get(gone).is_none(), "{gone}: {t}");
    }
    let found = summary["diagnoses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["identities"][0] == "trial|slowdown")
        .unwrap_or_else(|| panic!("{}", summary["diagnoses"]));
    assert_eq!(found["kind"], "trial", "{found}");
    assert_eq!(found["at"][0]["place"], "etl.py:12 in save", "{found}");
    assert_eq!(
        found["at"][0]["code"], "s3.put_object(Bucket=b, Key=\"state\")",
        "{found}"
    );
    assert!(
        found["text"]
            .as_str()
            .is_some_and(|x| x.starts_with("degraded under slowdown: gave up on 1 failed read")),
        "{found}"
    );
    assert!(found.get("savingText").is_none(), "{found}");
    // and in its fix loop: degraded before, degraded now
    let open = summary["since"]["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|f| f["identity"] == "trial|slowdown");
    assert_eq!(
        open.map(|f| &f["state"]),
        Some(&Value::from("open")),
        "{}",
        summary["since"]
    );
    // a finding an engine's setting acts on: the setting, and where the
    // program's folder sets it (a SlateDB reader polling its manifest every
    // 100 ms, from the recording in analysis' fixtures)
    {
        let fixture = sonde_analysis::run::RunFile::open(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../analysis/tests/fixtures/slatedb.sonde"),
        )
        .unwrap();
        let list = fixture
            .records
            .iter()
            .find(|r| r.kind == "ListObjectsV2")
            .unwrap();
        let mut reader = sonde_analysis::run::RunFile {
            header: fixture.header.clone(),
            records: Vec::new(),
            logs: Vec::new(),
            afresh: false,
        };
        for i in 0..30u64 {
            let mut r = list.clone();
            r.id = i + 1;
            r.time += chrono::Duration::milliseconds(i as i64 * 100);
            reader.records.push(r);
        }
        let program = b.path("reader");
        std::fs::create_dir_all(&program).unwrap();
        std::fs::write(
            program.join("main.rs"),
            "// the reader\nlet o = DbReaderOptions { manifest_poll_interval: Duration::from_millis(100), ..Default::default() };\n",
        )
        .unwrap();
        reader.header.cwd = Some(program.to_string_lossy().into_owned());
        let file = b.path("reader.sonde");
        sonde_analysis::import::save(&reader, &file).unwrap();
        let shown = b
            .json(&["show", file.to_str().unwrap(), "-f", "json"], 0)
            .await;
        let listed = shown["diagnoses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["kind"] == "obs.listed")
            .unwrap_or_else(|| panic!("{}", shown["diagnoses"]));
        let s = &listed["fix"]["setting"];
        assert_eq!(s["key"], "manifest_poll_interval", "{listed}");
        assert_eq!(s["value"], "10s", "{listed}");
        assert_eq!(s["engine"], "SlateDB DbReader", "{listed}");
        assert_eq!(s["where"], "main.rs:2", "{listed}");
        holds("show", &shown);
    }
    // a key with what a terminal acts on (DEL, a mark that turns text's
    // direction): every output is still JSON, the key in it as it is (27.0
    // wrote `\x7f` and `\u{202e}`, which no JSON reader takes)
    {
        let mut planted = sonde_analysis::run::RunFile::open(&run).unwrap();
        for r in planted.records.iter_mut().filter(|r| r.key.is_some()) {
            r.key = Some(format!("{PLANTED}{}", r.id).into());
        }
        let file = b.path("data/runs/planted.sonde");
        sonde_analysis::import::save(&planted, &file).unwrap();
        let p = file.to_str().unwrap();
        let split = b.json(&["show", p, "--by", "key", "-f", "json"], 0).await;
        assert!(split.to_string().contains(PLANTED), "{split}");
        holds("show-by", &split);
        holds(
            "show",
            &b.json(&["show", p, "--all", "-f", "json"], 0).await,
        );
        holds(
            "check",
            &b.json(&["check", p, "--baseline", s, "-f", "json"], 0)
                .await,
        );
        let since = b.json(&["show", p, "--since", s, "-f", "json"], 0).await;
        assert!(since["since"]["change"].is_object(), "{}", since["since"]);
        holds("show", &since);
        holds("runs", &b.json(&["runs", "-f", "json"], 0).await);
    }
    for by in ["kind", "prefix", "size", "caller", "key"] {
        holds(
            "show-by",
            &b.json(&["show", s, "--by", by, "-f", "json"], 0).await,
        );
    }
    // the run's cost split: its requests, its bytes out (said, not counted, when
    // where they were read from is not known) and what it left stored
    let cost = &summary["cost"];
    assert!(cost["requests"].is_number(), "{cost}");
    assert!(
        ["charged", "free", "unknown"].contains(&cost["transfer"]["egress"].as_str().unwrap()),
        "{cost}"
    );
    assert!(cost["storage"]["perMonth"].is_number(), "{cost}");
    // a budget on the bytes out: its own row, in bytes
    let held = b
        .json(&["check", s, "--max-transfer", "1B", "-f", "json"], 1)
        .await;
    assert_eq!(held["check"]["verdicts"][0]["what"], "transfer", "{held}");
    assert!(held["run"]["bytesOut"].as_u64().unwrap() > 1, "{held}");
    holds("check", &held);
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
    // a finding kept on purpose, with why, in `show` and `check`
    {
        let kept = b.path("etl.sonde");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../analysis/tests/fixtures/etl-1-base.sonde"),
            &kept,
        )
        .unwrap();
        std::fs::write(
            b.path("sonde.toml"),
            "[program]\nname = \"etl\"\nruns = \"etl\"\n[check]\nmax-requests = 100000\nexpect = { \"obs.serial\" = \"kept\" }\n",
        )
        .unwrap();
        let k = kept.to_str().unwrap();
        let shown = b.json(&["show", k, "-f", "json"], 0).await;
        assert!(
            shown["diagnoses"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["expected"]["obs.serial|"] == "kept"),
            "{}",
            shown["diagnoses"]
        );
        // its requests one after another counted, not only said
        let at_once = shown["diagnoses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| {
                d["identities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|i| i == "obs.serial|")
            })
            .map(|d| d["atOnce"].clone())
            .unwrap();
        assert_eq!(at_once["concurrency"], 8, "{at_once}");
        assert!(at_once["requests"].as_u64().unwrap() >= 10, "{at_once}");
        assert!(at_once["savesMs"].as_f64().unwrap() > 0.0, "{at_once}");
        holds("show", &shown);
        // each finding with why it is kept
        let whole = b.json(&["show", k, "--all", "-f", "json"], 0).await;
        assert!(
            whole["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["expected"] == "kept"),
            "{}",
            whole["findings"]
        );
        holds("show", &whole);
        let checked = b.json(&["check", k, "-f", "json"], 0).await;
        assert_eq!(checked["check"]["expected"][0]["expected"], "kept");
        holds("check", &checked);
        std::fs::remove_file(b.path("sonde.toml")).unwrap();
    }
    // `check --init -f json` writes JSON (27.0 wrote text), the run's
    // findings known by count and its own command; `check` then fails on a
    // finding it does not know
    {
        // (the tour ran elsewhere: its project is the one named)
        let toml_path = b.path("sonde.toml");
        let config = toml_path.to_str().unwrap();
        let init = b
            .json(&["check", s, "--config", config, "--init", "-f", "json"], 0)
            .await;
        holds("check-init", &init);
        assert_eq!(init["runArgs"], "--demo", "{init}");
        assert!(
            !init["command"].as_str().unwrap().contains("<your command>"),
            "{init}"
        );
        std::fs::remove_file(&toml_path).unwrap();
        // a run with findings (the etl fixture, on moto): known by count
        let etl = b.path("etl.sonde");
        let s = etl.to_str().unwrap();
        let init = b
            .json(&["check", s, "--config", config, "--init", "-f", "json"], 0)
            .await;
        holds("check-init", &init);
        // the store as the run was told it: no placeholder to fill in
        assert_eq!(init["runArgs"], "-e moto", "{init}");
        let known = init["findings"].as_object().unwrap();
        assert!(!known.is_empty(), "{init}");
        let toml = std::fs::read_to_string(b.path("sonde.toml")).unwrap();
        assert!(toml.contains("[check.findings]"), "{toml}");
        assert!(!toml.contains("tolerance"), "{toml}");
        let passed = b
            .json(&["check", s, "--config", config, "-f", "json"], 0)
            .await;
        holds("check", &passed);
        // one known with a smaller count, and one left out: grown and new
        let mut ids = known.keys();
        let (grown, gone) = (ids.next().unwrap().clone(), ids.next().cloned());
        let mut cut = toml.replace(
            &format!(
                "{} = {}",
                serde_json::to_string(&grown).unwrap(),
                known[&grown]
            ),
            &format!("{} = 0", serde_json::to_string(&grown).unwrap()),
        );
        if let Some(g) = &gone {
            cut = cut.replace(
                &format!("{} = {}\n", serde_json::to_string(g).unwrap(), known[g]),
                "",
            );
        }
        // one known with more than the run has: what the file may tighten
        let slack = toml.replace(
            &format!(
                "{} = {}",
                serde_json::to_string(&grown).unwrap(),
                known[&grown]
            ),
            &format!("{} = 99999", serde_json::to_string(&grown).unwrap()),
        );
        std::fs::write(b.path("sonde.toml"), slack).unwrap();
        let loose = b
            .json(&["check", s, "--config", config, "-f", "json"], 0)
            .await;
        assert_eq!(loose["check"]["loose"][0]["known"], 99999, "{loose}");
        holds("check", &loose);
        std::fs::write(b.path("sonde.toml"), cut).unwrap();
        let failed = b
            .json(&["check", s, "--config", config, "-f", "json"], 1)
            .await;
        holds("check", &failed);
        let gated = failed["check"]["findings"].as_array().unwrap();
        assert!(
            gated
                .iter()
                .any(|f| f["identity"] == grown.as_str() && f["known"] == 0),
            "{failed}"
        );
        if let Some(g) = gone {
            assert!(
                gated
                    .iter()
                    .any(|f| f["identity"] == g.as_str() && f.get("known").is_none()),
                "{failed}"
            );
        }
        std::fs::remove_file(b.path("sonde.toml")).unwrap();
    }
    holds(
        "show",
        &b.json(&["show", s, "--since", s, "-f", "json"], 0).await,
    );
    holds("runs", &b.json(&["runs", "-f", "json"], 0).await);

    // a program's runs one after another (`job`, each a few reads): steady,
    // then twice the work, then under another command on another store
    let job = |n: i64, keys: u64, slower_ms: i64, command: &str, endpoint: &str| {
        let mut f = sonde_analysis::run::RunFile::open(&run).unwrap();
        let get = f
            .records
            .iter()
            .find(|r| r.kind == "GetObject")
            .unwrap()
            .clone();
        let created =
            chrono::Utc::now() - chrono::Duration::hours(30) + chrono::Duration::minutes(n);
        f.header.name = Some("job".into());
        f.header.command = Some(command.into());
        f.header.created = created;
        f.header.endpoint = endpoint.into();
        // where it ran, its project says how often a month
        let project = b.path("job");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("sonde.toml"),
            "[program]\nruns-per-month = 200\n",
        )
        .unwrap();
        f.header.cwd = Some(project.to_string_lossy().into_owned());
        f.header.git = Some(sonde_analysis::run::GitStamp {
            commit: format!("{n:040x}"),
            branch: Some("main".into()),
            dirty: false,
        });
        f.logs.clear();
        f.records = (0..keys)
            .map(|i| {
                let mut r = get.clone();
                r.id = i + 1;
                r.key = Some(format!("in/part-{i}.json").into());
                let last = if i + 1 == keys { slower_ms } else { 0 };
                r.time = created + chrono::Duration::milliseconds(i as i64 * 100 + last);
                r
            })
            .collect();
        let file = b.path(&format!("data/runs/job-{n}.sonde"));
        sonde_analysis::import::save(&f, &file).unwrap();
        file.to_string_lossy().into_owned()
    };
    {
        let endpoint = sonde_analysis::run::RunFile::open(&run)
            .unwrap()
            .header
            .endpoint;
        let steady: Vec<String> = [0, 100, 0, 50]
            .iter()
            .enumerate()
            .map(|(i, slower)| job(i as i64 + 1, 12, *slower, "job", &endpoint))
            .collect();
        let mut last = Value::Null;
        for j in &steady {
            last = b.json(&["show", j, "-f", "json"], 0).await;
            holds("show", &last);
        }
        assert_eq!(last["month"]["from"], "runs", "{}", last["month"]);
        // the fourth moved by less than the three before it vary
        let change = &last["since"]["change"];
        assert!(change["noiseMs"].is_number(), "{}", last["since"]);
        assert_eq!(change["withinNoise"], true, "{change}");
        // more than half as many objects again: the workload's doing
        let more = job(5, 30, 0, "job", &endpoint);
        let shown = b.json(&["show", &more, "-f", "json"], 0).await;
        assert!(
            shown["since"]["change"]["workloadChanged"].is_string(),
            "{}",
            shown["since"]
        );
        holds("show", &shown);
        // another command, another store
        let other = job(6, 30, 0, "job --limit 20", "elsewhere");
        let shown = b.json(&["show", &other, "-f", "json"], 0).await;
        let change = &shown["since"]["change"];
        assert!(change["commandChanged"].is_string(), "{}", shown["since"]);
        holds("show", &shown);
        let since = b
            .json(&["show", &other, "--since", &steady[0], "-f", "json"], 0)
            .await;
        let change = &since["since"]["change"];
        assert_eq!(change["endpoints"][1], "elsewhere", "{change}");
        holds("show", &since);
        // a folder whose index cannot be written: the run is shown, and why
        // its program's findings were not kept
        let locked = b.path("locked");
        std::fs::create_dir_all(locked.join(".index.jsonl")).unwrap();
        std::fs::copy(&steady[3], locked.join("job.sonde")).unwrap();
        let shown = b
            .json(
                &[
                    "show",
                    locked.join("job.sonde").to_str().unwrap(),
                    "-f",
                    "json",
                ],
                0,
            )
            .await;
        assert!(shown["since"]["unsaved"].is_string(), "{}", shown["since"]);
        holds("show", &shown);
        // past its baseline since the fifth run: the commits on either side
        let past = b
            .json(
                &["check", &other, "--baseline", &steady[0], "-f", "json"],
                1,
            )
            .await;
        assert_eq!(past["since"]["runsAgo"], 1, "{}", past["since"]);
        assert!(
            past["since"]["lastWithinCommit"]["commit"].is_string(),
            "{}",
            past["since"]
        );
        holds("check", &past);
    }
    // a run whose program failed: not compared, and the check says its exit
    {
        let failed = b.path("failed.sonde");
        let f = failed.to_str().unwrap();
        let o = tokio::process::Command::new(env!("CARGO_BIN_EXE_sonde"))
            .current_dir(b.dir.path())
            .env("SONDE_CONFIG_DIR", b.path("config"))
            .env("SONDE_DATA_DIR", b.path("data"))
            .args(["run", "--demo", "--name", "tour", "-o", f, "--"])
            .args(["sh", "-c", "exit 3"])
            .stdin(Stdio::null())
            .output()
            .await
            .unwrap();
        assert_eq!(o.status.code(), Some(3));
        let shown = b.json(&["show", f, "-f", "json"], 0).await;
        assert_eq!(shown["since"]["failed"], 3, "{}", shown["since"]);
        holds("show", &shown);
        let since = b.json(&["show", f, "--since", s, "-f", "json"], 0).await;
        assert_eq!(since["since"]["change"]["failed"], 3, "{}", since["since"]);
        holds("show", &since);
        let checked = b
            .json(&["check", f, "--max-requests", "100000", "-f", "json"], 1)
            .await;
        assert_eq!(checked["programExit"], 3, "{checked}");
        holds("check", &checked);
        // a baseline asked for where none is set: said, and the budget checked
        let none = b
            .json(
                &[
                    "check",
                    s,
                    "--baseline",
                    "auto",
                    "--max-requests",
                    "100000",
                    "-f",
                    "json",
                ],
                0,
            )
            .await;
        assert!(none["noBaseline"].is_string(), "{none}");
        holds("check", &none);
    }
    // the etl job run by run (fixtures recorded on moto): findings fixed,
    // less and more between two runs, each as it read before
    {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../analysis/tests/fixtures");
        std::fs::create_dir_all(b.path("etl")).unwrap();
        let etl: Vec<String> = [
            "etl-1-base",
            "etl-2-again",
            "etl-3-no-head",
            "etl-4-no-small",
            "etl-5-undone",
        ]
        .iter()
        .map(|n| {
            let file = b.path(&format!("etl/{n}.sonde"));
            std::fs::copy(fixtures.join(format!("{n}.sonde")), &file).unwrap();
            file.to_string_lossy().into_owned()
        })
        .collect();
        let mut was = false;
        for pair in etl.windows(2) {
            let since = b
                .json(&["show", &pair[1], "--since", &pair[0], "-f", "json"], 0)
                .await;
            was |= since["since"]["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["was"]["text"].is_string());
            holds("show", &since);
            holds(
                "check",
                // (within its baseline or past it, as the fixture is)
                &b.json_of(
                    &["check", &pair[1], "--baseline", &pair[0], "-f", "json"],
                    &[0, 1],
                )
                .await,
            );
            holds(
                "show",
                &b.json(&["show", &pair[1], "--all", "-f", "json"], 0).await,
            );
        }
        assert!(was, "a finding that reads otherwise than before");
        // what each engine's and SDK's recording shows of the plan
        for n in [
            "delta",
            "iceberg",
            "lance",
            "slatedb",
            "sdk-java",
            "sdk-python",
        ] {
            let file = b.path(&format!("etl/{n}.sonde"));
            std::fs::copy(fixtures.join(format!("{n}.sonde")), &file).unwrap();
            let f = file.to_str().unwrap();
            holds("show", &b.json(&["show", f, "-f", "json"], 0).await);
            holds(
                "show",
                &b.json(&["show", f, "--all", "-f", "json"], 0).await,
            );
            holds(
                "show-by",
                &b.json(&["show", f, "--by", "caller", "-f", "json"], 0)
                    .await,
            );
        }
        // a run that failed against a baseline: its exit, in the change
        let failed = b.path("failed.sonde");
        let past = b
            .json_of(
                &[
                    "check",
                    failed.to_str().unwrap(),
                    "--baseline",
                    s,
                    "-f",
                    "json",
                ],
                &[0, 1],
            )
            .await;
        assert_eq!(past["change"]["failed"], 3, "{}", past["change"]);
        holds("check", &past);
    }
    // the same megabyte of three objects read a dozen times each, one
    // request after another: the bytes reading each once saves, and the
    // requests one after another said with the finding that owns them
    {
        let mut f = sonde_analysis::run::RunFile::open(&run).unwrap();
        let get = f
            .records
            .iter()
            .find(|r| r.kind == "GetObject")
            .unwrap()
            .clone();
        f.header.name = Some("again".into());
        f.header.command = Some("again".into());
        f.header.cwd = None;
        f.logs.clear();
        f.records = (0..36u64)
            .map(|i| {
                let mut v = serde_json::to_value(&get).unwrap();
                v["id"] = (i + 1).into();
                v["key"] = format!("in/part-{}.parquet", i % 3).into();
                v["responseBytes"] = (1024 * 1024).into();
                v["status"] = 206.into();
                v["requestHeaders"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!({ "name": "range", "value": "bytes=0-1048575" }));
                let mut r: reqlog::Record = serde_json::from_value(v).unwrap();
                r.time = f.header.created + chrono::Duration::milliseconds(i as i64 * 100);
                r
            })
            .collect();
        let file = b.path("again.sonde");
        sonde_analysis::import::save(&f, &file).unwrap();
        let a = file.to_str().unwrap();
        holds("show", &b.json(&["show", a, "-f", "json"], 0).await);
        holds(
            "show",
            &b.json(&["show", a, "--all", "-f", "json"], 0).await,
        );
    }
    // a read written back: a fault `check --init` never takes as known
    {
        let mut f = sonde_analysis::run::RunFile::open(&run).unwrap();
        let get = f
            .records
            .iter()
            .find(|r| r.kind == "GetObject")
            .unwrap()
            .clone();
        let mut put = f
            .records
            .iter()
            .find(|r| r.kind == "PutObject")
            .unwrap()
            .clone();
        put.key = get.key.clone();
        put.bucket = get.bucket;
        f.header.name = Some("rmw".into());
        f.header.command = Some("rmw".into());
        f.logs.clear();
        f.records = (0..6u64)
            .map(|i| {
                let mut r = if i % 2 == 0 { get.clone() } else { put.clone() };
                r.id = i + 1;
                r.time = f.header.created + chrono::Duration::milliseconds(i as i64 * 50);
                r
            })
            .collect();
        let file = b.path("rmw.sonde");
        sonde_analysis::import::save(&f, &file).unwrap();
        let toml = b.path("rmw.toml");
        let init = b
            .json(
                &[
                    "check",
                    file.to_str().unwrap(),
                    "--config",
                    toml.to_str().unwrap(),
                    "--init",
                    "-f",
                    "json",
                ],
                0,
            )
            .await;
        assert!(init["notKnown"][0].is_string(), "{init}");
        holds("check-init", &init);
    }

    // replaying it on the small store (what the run read is written there
    // first: nothing differs)
    let replayed = b.path("replay.sonde");
    let r = replayed.to_str().unwrap();
    holds(
        "replay",
        &b.json(
            &[
                "replay",
                s,
                "-e",
                "a",
                "--bucket",
                "demo=data",
                "--yes",
                "-o",
                r,
                "-f",
                "json",
            ],
            0,
        )
        .await,
    );
    holds(
        "replay-stores",
        &b.json(
            &[
                "replay",
                s,
                "-e",
                "a",
                "-e",
                "b",
                "--bucket",
                "demo=data",
                "--yes",
                "-f",
                "json",
            ],
            0,
        )
        .await,
    );

    // a read the program's store failed and this one answers: an answer
    // that differs; and a create only if the key is not there
    // (`If-None-Match: *`): what the program relies on, probed on the store
    {
        let mut cond = sonde_analysis::run::RunFile::open(&run).unwrap();
        for r in cond.records.iter_mut().filter(|r| r.kind == "PutObject") {
            let mut v = serde_json::to_value(&*r).unwrap();
            v["requestHeaders"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({ "name": "if-none-match", "value": "*" }));
            v["status"] = 412.into();
            v["errorBody"] = "<Error><Code>PreconditionFailed</Code><Message>At least one of the pre-conditions you specified did not hold</Message></Error>".into();
            *r = serde_json::from_value(v).unwrap();
        }
        let read = cond
            .records
            .iter_mut()
            .find(|r| r.kind == "GetObject")
            .unwrap();
        let mut v = serde_json::to_value(&*read).unwrap();
        v["status"] = 500.into();
        v["errorBody"] =
            "<Error><Code>InternalError</Code><Message>We encountered an internal error</Message></Error>"
                .into();
        *read = serde_json::from_value(v).unwrap();
        let file = b.path("cond.sonde");
        sonde_analysis::import::save(&cond, &file).unwrap();
        let c = file.to_str().unwrap();
        let one = b
            .json(
                &[
                    "replay",
                    c,
                    "-e",
                    "a",
                    "--bucket",
                    "demo=data",
                    "--yes",
                    "-f",
                    "json",
                ],
                2,
            )
            .await;
        assert_eq!(
            one["comparison"]["differences"][0]["kind"], "GetObject",
            "{}",
            one["comparison"]
        );
        assert_eq!(
            one["relies"][0]["probe"], "cond.put-if-none-match",
            "{}",
            one["relies"]
        );
        holds("replay", &one);
        let two = b
            .json(
                &[
                    "replay",
                    c,
                    "-e",
                    "a",
                    "-e",
                    "b",
                    "--bucket",
                    "demo=data",
                    "--yes",
                    "-f",
                    "json",
                ],
                2,
            )
            .await;
        holds("replay-stores", &two);
    }

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
        "layout",
        &b.json(
            &[
                "layout", "-e", "a", "-b", "data", "--kind", "generic", "-f", "json",
            ],
            0,
        )
        .await,
    );
    // real layouts (SlateDB, Iceberg, lakeFS): their snapshots, events and issues
    holds(
        "layout",
        &b.json_of(
            &["layout", "-e", "d", "-b", "demo-layouts", "-f", "json"],
            &[0, 1, 4],
        )
        .await,
    );
    // a bucket with versions, delete markers and uploads; then with an
    // abort rule of sonde's own (its dry run, then --yes)
    holds(
        "bucket-size",
        &b.json(
            &["bucket", "size", "-e", "d", "-b", "demo", "-f", "json"],
            0,
        )
        .await,
    );
    for (args, code) in [
        (
            &["bucket", "lifecycle", "-e", "d", "-b", "demo", "-p", "tmp/"][..],
            4,
        ),
        (
            &[
                "bucket",
                "lifecycle",
                "-e",
                "d",
                "-b",
                "demo",
                "-p",
                "tmp/",
                "--yes",
            ],
            0,
        ),
    ] {
        let o = tokio::process::Command::new(env!("CARGO_BIN_EXE_sonde"))
            .env("SONDE_CONFIG_DIR", b.path("config"))
            .env("SONDE_DATA_DIR", b.path("data"))
            .args(args)
            .output()
            .await
            .unwrap();
        assert_eq!(o.status.code(), Some(code), "{args:?}");
    }
    let uploads = b
        .json(
            &["bucket", "uploads", "-e", "d", "-b", "demo", "-f", "json"],
            4,
        )
        .await;
    assert_eq!(uploads["lifecycle"]["rules"][0]["days"], 7, "{uploads}");
    holds("bucket-uploads", &uploads);
    // a store that refuses: the error, whole
    let refused = b.json(&["endpoints", "test", "x", "-f", "json"], 70).await;
    assert_eq!(refused["error"]["code"], "AccessDenied", "{refused}");
    holds("endpoints-test", &refused);
    let unread = b
        .json(
            &["bucket", "uploads", "-e", "x", "-b", "b", "-f", "json"],
            4,
        )
        .await;
    assert_eq!(unread["lifecycle"]["status"], "unknown", "{unread}");
    holds("bucket-uploads", &unread);
    // and its versions not listed: the objects counted, the rest not known
    let listed = b
        .json(&["bucket", "size", "-e", "x", "-b", "b", "-f", "json"], 0)
        .await;
    assert!(listed["versionsUnknown"].is_string(), "{listed}");
    holds("bucket-size", &listed);
    let probed = b
        .json(
            &["endpoints", "test", "a", "--probes", "data", "-f", "json"],
            // (the small store breaks a guarantee or two)
            1,
        )
        .await;
    holds("endpoints-probes", &probed);
    // a run with a line in its middle that is no record: listed as one that
    // does not read (made last: nothing above takes it for the newest run)
    {
        let whole = sonde_analysis::run::RunFile::open(&run).unwrap();
        let header = serde_json::to_string(&whole.header).unwrap();
        let records: Vec<String> = whole
            .records
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect();
        std::fs::write(
            b.path("data/runs/broken.sonde"),
            format!("{header}\n{{\"id\": \"x\"}}\n{}\n", records.join("\n")),
        )
        .unwrap();
        let listed = b.json(&["runs", "-f", "json"], 0).await;
        assert!(
            listed
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["unreadable"].is_string()),
            "{listed}"
        );
        holds("runs", &listed);
    }
    strictly(CHECKED);
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
        let mut e = Seen::default();
        check(&s, &s, &v, "$", "", &mut e);
        e.errors
    };
    assert!(errs(serde_json::json!({ "a": 1, "b": ["x"], "c": 0 })).is_empty());
    assert_eq!(errs(serde_json::json!({})), ["$: no a"]);
    assert_eq!(errs(serde_json::json!({ "a": 1.5 })).len(), 1);
    assert_eq!(errs(serde_json::json!({ "a": 1, "b": ["y"] })).len(), 1);
    assert_eq!(errs(serde_json::json!({ "a": 1, "b": ["x", 2] })).len(), 1);
    let loose = serde_json::json!({ "minimum": 1 });
    let mut e = Seen::default();
    check(&loose, &loose, &serde_json::json!(2), "$", "", &mut e);
    assert!(e.errors[0].contains("minimum"), "{:?}", e.errors);
}

/// The schemas, each checked against real outputs above.
const CHECKED: &[&str] = &[
    "show",
    "show-by",
    "check",
    "check-init",
    "runs",
    "replay",
    "replay-stores",
    "endpoints-list",
    "endpoints-test",
    "endpoints-probes",
    "bucket-size",
    "bucket-uploads",
    "layout",
];

/// The strict reading: a property an output holds and its schema does not
/// name, one a schema names that no output held, a map said to be one, and
/// a list of the unexercised that says what is so.
#[test]
fn the_strict_reading_names_what_is_undocumented_and_what_is_unexercised() {
    let s = serde_json::json!({
        "type": "object",
        "properties": {
            "run": { "$ref": "#/$defs/run" },
            "rows": { "type": "array", "items": { "type": "object", "properties": {
                "n": { "type": "integer" }, "never": { "type": "string" } } } },
            "args": { "type": "object", "additionalProperties": { "type": "string" } },
            "either": { "anyOf": [
                { "type": "null" },
                { "type": "object", "properties": { "a": { "type": "integer" } } }
            ] }
        },
        "$defs": { "run": { "type": "object", "properties": { "name": { "type": "string" } } } }
    });
    let doc = serde_json::json!({
        "run": { "name": "etl", "cwd": "/x" },
        "rows": [{ "n": 1 }, { "n": 2, "extra": true }],
        "args": { "anything": "goes" },
        "either": { "a": 1, "b": 2 },
        "new": 1
    });
    let mut seen = Seen::default();
    check(&s, &s, &doc, "$", "", &mut seen);
    // as the schema is written it holds: the published schemas stay open
    assert!(seen.errors.is_empty(), "{:?}", seen.errors);
    let found = strict_findings("t", &s, &seen, &[]);
    assert_eq!(
        found,
        [
            "t: undocumented property `b` of $.either (first seen at $.either.b): describe it in docs/schemas/t.schema.json",
            "t: undocumented property `new` of $ (first seen at $.new): describe it in docs/schemas/t.schema.json",
            "t: undocumented property `extra` of $.rows[] (first seen at $.rows[1].extra): describe it in docs/schemas/t.schema.json",
            "t: undocumented property `cwd` of $.run (first seen at $.run.cwd): describe it in docs/schemas/t.schema.json",
            "t: unexercised property `never` (docs/schemas/t.schema.json#/properties/rows/items/properties/never): no output in crates/sonde-cli/tests/schemas.rs holds it; make one that does, or list it in UNEXERCISED with why",
        ]
    );
    // listed with why, it is let be; a list that says what is not so is not
    let listed = [(
        "t",
        "/properties/rows/items/properties/never",
        "no such row here",
    )];
    assert_eq!(strict_findings("t", &s, &seen, &listed).len(), 4);
    let stale = [
        (
            "t",
            "/properties/rows/items/properties/n",
            "held all the same",
        ),
        ("t", "/properties/gone", "no such property"),
        (
            "t",
            "/properties/rows/items/properties/never",
            "no such row here",
        ),
    ];
    let found = strict_findings("t", &s, &seen, &stale);
    assert!(found[4].contains("which an output now holds"), "{found:?}");
    assert!(found[5].contains("does not name"), "{found:?}");
    // a node that says an object two ways is not read
    let mixed = serde_json::json!({ "$ref": "#/$defs/x", "properties": {}, "$defs": { "x": {} } });
    let mut seen = Seen::default();
    check(&mixed, &mixed, &serde_json::json!({}), "$", "", &mut seen);
    assert!(seen.errors[0].contains("mixes"), "{:?}", seen.errors);
}

/// Every schema is one the test above reads, and each says what it is of.
#[test]
fn every_schema_is_checked_and_documented() {
    let checked = CHECKED;
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
/// (`…Text`) and `layout`'s display tables say so, those tables are
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
                        let layout = name == "layout"
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
