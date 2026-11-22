//! Every `-f json` output is what its JSON Schema in docs/schemas says
//! (docs/contract.md): the real binary, the demo's run and a small S3 in
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
        trial::save(&run, std::slice::from_ref(&now)).unwrap();
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
        holds("show", &shown);
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
    let probed = b
        .json(
            &["endpoints", "test", "a", "--probes", "data", "-f", "json"],
            // (the small store breaks a guarantee or two)
            1,
        )
        .await;
    holds("endpoints-probes", &probed);
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
