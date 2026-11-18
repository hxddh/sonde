//! Fixes in the engine's own words: when a finding's requests are a table
//! format's or a tool's (SlateDB, Delta, Lance, Iceberg; DuckDB, rclone,
//! s5cmd, Trino, Spark's s3a), the one setting of that engine that acts on
//! it, with the value to give it ([`Setting`]), and where the program's
//! folder sets it now ([`locate`]).
//!
//! Each setting is the engine's own name, as its documentation or source
//! says it (sources in the comments). A finding no setting of its engine
//! acts on gets none: an SDK's code is no fix for a program that never
//! calls the SDK itself (a SlateDB manifest listed again and again is not
//! fixed with a paginator).

use std::path::Path;

use reqlog::Record;
use serde::Serialize;
use sonde_proxy::part::{Format, Part, part_of};

use crate::fixes::Fix;
use crate::phrase::Phrase;

/// One setting of an engine to change: what an agent edits.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Setting {
    /// The engine or tool, as it is named (`SlateDB DbReader`, `DuckDB`).
    pub engine: String,
    /// The setting's own name (`manifest_poll_interval`, `--s3-no-head-object`).
    pub key: String,
    /// The value to give it, as the engine reads it (`"10s"`, `true`).
    pub value: String,
    /// Where the program's folder sets it now (`conf/slatedb.toml:4`), when
    /// sonde finds it there ([`locate`]).
    #[serde(rename = "where", skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// Other spellings of the key looked for by [`locate`] (a config
    /// file's name for a flag, an environment variable).
    #[serde(skip)]
    pub also: Vec<&'static str>,
}

impl Setting {
    /// The setting in one line: `manifest_poll_interval = 10s (SlateDB
    /// DbReader) · src/main.rs:4`.
    pub fn line(&self) -> String {
        let mut line = format!("{} = {} ({})", self.key, self.value, self.engine);
        if let Some(at) = &self.at {
            line.push_str(&format!(" · {at}"));
        }
        line
    }
}

/// What sent a finding's requests: a table format's files, and the tool
/// that named itself in the user agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Engine {
    pub format: Option<Format>,
    pub tool: Option<Tool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    DuckDB,
    Rclone,
    S5cmd,
    Trino,
    /// Hadoop's s3a (Spark's too).
    S3a,
}

impl Engine {
    /// Whether the program sends its requests through an engine, not an
    /// SDK it calls itself: an SDK's code is then no fix.
    pub fn is_some(&self) -> bool {
        self.format.is_some() || self.tool.is_some()
    }
}

fn user_agent(r: &Record) -> Option<&str> {
    r.request_headers()
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("user-agent"))
        .map(|h| h.value.as_str())
}

/// The tool a user agent names: Trino's AWS SDK adds `app/Trino`
/// (`s3.application-id`), the rest their own product.
fn tool_of(ua: &str) -> Option<Tool> {
    if ua
        .split_whitespace()
        .any(|p| p.eq_ignore_ascii_case("app/Trino"))
    {
        return Some(Tool::Trino);
    }
    let first = |name: &str| {
        ua.split_whitespace()
            .filter_map(|p| p.split_once('/'))
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
    };
    match sonde_proxy::callers::client(ua).as_deref() {
        Some("DuckDB") => Some(Tool::DuckDB),
        Some("rclone") => Some(Tool::Rclone),
        Some("s5cmd") => Some(Tool::S5cmd),
        Some("Spark") => Some(Tool::S3a),
        _ if first("hadoop-aws") || ua.starts_with("Hadoop ") || first("Hadoop") => Some(Tool::S3a),
        _ => None,
    }
}

/// The engine of a finding's requests: the format most of its operations
/// are a step of (`formats`, by request), and the tool most of its requests
/// name.
pub fn engine_of(rs: &[&Record], formats: &[(Option<Format>, usize)]) -> Engine {
    let total: usize = formats.iter().map(|f| f.1).sum();
    let mut by_format: Vec<(Format, usize)> = Vec::new();
    for (f, n) in formats {
        if let Some(f) = f {
            match by_format.iter_mut().find(|(g, _)| g == f) {
                Some(e) => e.1 += n,
                None => by_format.push((*f, *n)),
            }
        }
    }
    let format = by_format
        .into_iter()
        .max_by_key(|f| f.1)
        .filter(|f| f.1 * 2 >= total.max(1))
        .map(|f| f.0);
    let mut tools: Vec<(Tool, usize)> = Vec::new();
    for t in rs.iter().filter_map(|r| user_agent(r)).filter_map(tool_of) {
        match tools.iter_mut().find(|(u, _)| *u == t) {
            Some(e) => e.1 += 1,
            None => tools.push((t, 1)),
        }
    }
    let tool = tools
        .into_iter()
        .max_by_key(|t| t.1)
        .filter(|t| t.1 * 2 >= rs.len().max(1))
        .map(|t| t.0);
    Engine { format, tool }
}

/// A gap in time as a setting writes it: `100ms`, `2s`, `1.5s`.
fn duration_text(ms: f64) -> String {
    if ms < 1000.0 {
        format!("{}ms", ms.round() as u64)
    } else if (ms / 1000.0).fract().abs() < 0.05 {
        format!("{}s", (ms / 1000.0).round() as u64)
    } else {
        format!("{:.1}s", ms / 1000.0)
    }
}

/// How often requests came, the middle gap between one and the next (ms):
/// the period of a poll or a flush.
fn period_ms(rs: &[&Record]) -> Option<f64> {
    let mut times: Vec<i64> = rs.iter().map(|r| r.time.timestamp_micros()).collect();
    times.sort_unstable();
    let mut gaps: Vec<i64> = times.windows(2).map(|w| w[1] - w[0]).collect();
    if gaps.is_empty() {
        return None;
    }
    gaps.sort_unstable();
    Some(gaps[gaps.len() / 2] as f64 / 1000.0)
}

/// The folder a table's files are under, as a URI: the path up to its
/// format's own folder (`_delta_log/`, `…​.lance/`), else the folder the
/// keys share, without `name=value/` partition folders.
fn table_uri(rs: &[&Record], format: Format) -> Option<String> {
    let r = rs.iter().find(|r| r.bucket.is_some() && r.key.is_some())?;
    let bucket = r.bucket.as_deref()?;
    let mut common: Option<String> = None;
    for r in rs.iter().filter(|r| r.bucket.as_deref() == Some(bucket)) {
        let Some(k) = r.key.as_deref() else { continue };
        let root = match format {
            Format::Delta => k.find("_delta_log/").map(|i| k[..i].to_string()),
            Format::Lance => k.find(".lance/").map(|i| k[..i + 6].to_string()),
            _ => None,
        };
        if let Some(root) = root {
            return Some(format!("s3://{bucket}/{}", root.trim_end_matches('/')));
        }
        let folder = &k[..k.rfind('/').map_or(0, |i| i + 1)];
        common = Some(match common {
            None => folder.to_string(),
            Some(c) => {
                let n = c
                    .split('/')
                    .zip(folder.split('/'))
                    .take_while(|(a, b)| a == b)
                    .count();
                c.split('/').take(n).collect::<Vec<_>>().join("/") + "/"
            }
        });
    }
    let mut root: Vec<&str> = common
        .as_deref()
        .unwrap_or("")
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    while root.last().is_some_and(|s| s.contains('=') || *s == "data") {
        root.pop();
    }
    Some(format!("s3://{bucket}/{}", root.join("/")))
}

fn setting(engine: &str, key: &str, value: &str, also: &[&'static str]) -> Setting {
    Setting {
        engine: engine.into(),
        key: key.into(),
        value: value.into(),
        at: None,
        also: also.to_vec(),
    }
}

fn fix(code: String, setting: Option<Setting>) -> Fix {
    Fix {
        sdk: "config",
        code: code.into(),
        setting,
    }
}

/// Whether the run wrote any of a SlateDB's own files: a `Db` (a writer),
/// else a `DbReader`.
fn slate_writes(records: &[Record]) -> bool {
    records.iter().any(|r| {
        r.method == "PUT"
            && r.key
                .as_deref()
                .and_then(part_of)
                .is_some_and(|p| p.format() == Format::SlateDB)
    })
}

/// The engine's fix for a finding (`said`, its first phrase) whose
/// requests are `rs`, among the run's `records`; none when no setting of
/// the engine acts on it.
pub fn fix_for(said: &Phrase, engine: Engine, rs: &[&Record], records: &[Record]) -> Option<Fix> {
    let arg = |n: &str| said.args.get(n).map(|v| v.to_string());
    let key = said.key;
    match (engine.format, engine.tool) {
        // slatedb/src/config.rs: `Settings::manifest_poll_interval` (a Db's,
        // 1 s by default), `DbReaderOptions::manifest_poll_interval` (10 s);
        // a reader also looks for new WAL files at that interval
        (Some(Format::SlateDB), _) if matches!(key, "obs.listed" | "obs.hot") => {
            // the polls: the manifest listings (obs.listed) or reads (obs.hot)
            let polls: Vec<&Record> = rs
                .iter()
                .copied()
                .filter(|r| {
                    if key == "obs.listed" {
                        r.kind.starts_with("ListObjects")
                            && crate::episode::query(r, "prefix")
                                .is_some_and(|p| p.trim_end_matches('/').ends_with("manifest"))
                    } else {
                        r.method == "GET"
                            && matches!(
                                r.key.as_deref().and_then(part_of),
                                Some(Part::SlateManifest(_))
                            )
                    }
                })
                .collect();
            if polls.len() < 2 {
                return None;
            }
            let period = period_ms(&polls)?;
            let (who, default_ms, default) = if slate_writes(records) {
                ("SlateDB Db", 1000.0, "1s")
            } else {
                ("SlateDB DbReader", 10_000.0, "10s")
            };
            // already as slow as its default: no value to give it
            if period >= default_ms * 0.9 {
                return None;
            }
            let code = if who == "SlateDB Db" {
                format!(
                    "// {who} polls the manifest every {}: raise manifest_poll_interval (1s is its default)\nlet settings = Settings {{ manifest_poll_interval: Duration::from_secs(1), ..Default::default() }};\n// or in SlateDb.toml: manifest_poll_interval = \"1s\"",
                    duration_text(period)
                )
            } else {
                format!(
                    "// {who} polls the manifest every {}: raise manifest_poll_interval (10s is its default)\nlet options = DbReaderOptions {{ manifest_poll_interval: Duration::from_secs(10), ..Default::default() }};",
                    duration_text(period)
                )
            };
            Some(fix(
                code,
                Some(setting(
                    who,
                    "manifest_poll_interval",
                    default,
                    &["SLATEDB_MANIFEST_POLL_INTERVAL"],
                )),
            ))
        }
        // `Settings::flush_interval` (100 ms by default): each flush writes a
        // WAL file, one PUT; a longer interval writes fewer, larger ones,
        // and a write waits longer to be durable
        (Some(Format::SlateDB), _) if key == "obs.small" => {
            let wal: Vec<&Record> = rs
                .iter()
                .copied()
                .filter(|r| matches!(r.key.as_deref().and_then(part_of), Some(Part::SlateWal(_))))
                .collect();
            if wal.len() * 2 < rs.len().max(1) {
                return None;
            }
            let period = period_ms(&wal)?;
            if period >= 500.0 {
                return None;
            }
            Some(fix(
                format!(
                    "// SlateDB flushes its WAL every {}, a PUT each: raise flush_interval (a write waits up to it to be durable)\nlet settings = Settings {{ flush_interval: Some(Duration::from_secs(1)), ..Default::default() }};\n// or in SlateDb.toml: flush_interval = \"1s\"",
                    duration_text(period)
                ),
                Some(setting(
                    "SlateDB Db",
                    "flush_interval",
                    "1s",
                    &["SLATEDB_FLUSH_INTERVAL"],
                )),
            ))
        }
        // delta-rs: `DeltaTable.vacuum` deletes files no version names, older
        // than the retention (7 days, `delta.deletedFileRetentionDuration`);
        // Spark: `VACUUM`
        (Some(Format::Delta), tool) if key == "obs.orphans" => {
            let uri = table_uri(rs, Format::Delta)?;
            let code = if tool == Some(Tool::S3a) {
                format!(
                    "-- files no commit names: the table's own cleanup removes them (older than 7 days)\nVACUUM delta.`{uri}` RETAIN 168 HOURS"
                )
            } else {
                format!(
                    "# files no commit names: the table's own cleanup removes them (older than 7 days)\nfrom deltalake import DeltaTable\nDeltaTable(\"{uri}\").vacuum(retention_hours=168, dry_run=False)"
                )
            };
            Some(fix(code, None))
        }
        // pylance: `cleanup_old_versions` deletes a failed write's files once
        // they are 7 days old (`delete_unverified=True`: at once)
        (Some(Format::Lance), _) if key == "obs.orphans" => {
            let uri = table_uri(rs, Format::Lance)?;
            Some(fix(
                format!(
                    "# files no version names: the dataset's cleanup removes a failed write's once 7 days old\nimport lance\nlance.dataset(\"{uri}\").cleanup_old_versions()"
                ),
                None,
            ))
        }
        // Iceberg: the Spark procedure (pyiceberg has none); its table is
        // the catalog's name, which no request says
        (Some(Format::Iceberg), _) if key == "obs.orphans" => {
            let uri = table_uri(rs, Format::Iceberg)?;
            Some(fix(
                format!(
                    "-- files no snapshot names: Iceberg's cleanup removes them (older than 3 days by default)\n-- (the table as your catalog names the one at {uri})\nCALL catalog.system.remove_orphan_files(table => 'db.table')"
                ),
                None,
            ))
        }
        // duckdb.org/docs/configuration/overview: `enable_http_metadata_cache`
        // (false by default) keeps what HEAD said between queries;
        // `enable_external_file_cache` (DuckDB 1.3+, on by default) keeps
        // the files' data
        (_, Some(Tool::DuckDB)) if key == "obs.headget" => Some(fix(
            "-- DuckDB sent a HEAD before each read: keep what it says between queries\nSET enable_http_metadata_cache = true;".into(),
            Some(setting("DuckDB", "enable_http_metadata_cache", "true", &[])),
        )),
        (_, Some(Tool::DuckDB)) if key == "obs.hot" => {
            let what = arg("key").unwrap_or_default();
            Some(fix(
                format!(
                    "-- DuckDB read {what} again and again: keep files' data in memory (DuckDB 1.3+, where it is on by default; before 1.3, upgrade)\nSET enable_external_file_cache = true;"
                ),
                Some(setting("DuckDB", "enable_external_file_cache", "true", &[])),
            ))
        }
        // rclone.org/s3: `--s3-no-head-object` (`no_head_object` in
        // rclone.conf), `--s3-upload-cutoff` (200Mi by default), and the
        // global `--tpslimit` (requests a second; off by default)
        (_, Some(Tool::Rclone)) if key == "obs.headget" => Some(fix(
            "# rclone sent a HEAD before each GET: skip it\nrclone copy … --s3-no-head-object\n# or in rclone.conf, under the remote: no_head_object = true".into(),
            Some(setting(
                "rclone",
                "--s3-no-head-object",
                "true",
                &["no_head_object", "RCLONE_S3_NO_HEAD_OBJECT"],
            )),
        )),
        (_, Some(Tool::Rclone)) if key == "obs.small.parts" => Some(fix(
            "# rclone uploaded small files in parts: one PUT under the cutoff (its default)\nrclone copy … --s3-upload-cutoff 200Mi\n# or in rclone.conf, under the remote: upload_cutoff = 200Mi".into(),
            Some(setting(
                "rclone",
                "--s3-upload-cutoff",
                "200Mi",
                &["upload_cutoff", "RCLONE_S3_UPLOAD_CUTOFF"],
            )),
        )),
        (_, Some(Tool::Rclone)) if key.starts_with("obs.throttled") => {
            let mut times: Vec<i64> = records.iter().map(|r| r.time.timestamp_micros()).collect();
            times.sort_unstable();
            let peak = crate::profile::peak_second(&times);
            if peak < 2 {
                return None;
            }
            let limit = (peak / 2).max(1);
            Some(fix(
                format!(
                    "# rclone sent up to {peak} requests a second and was throttled: about half as many\nrclone copy … --tpslimit {limit}"
                ),
                Some(setting(
                    "rclone",
                    "--tpslimit",
                    &limit.to_string(),
                    &["RCLONE_TPSLIMIT"],
                )),
            ))
        }
        // s5cmd's global `--numworkers` (256 by default): requests at once
        (_, Some(Tool::S5cmd)) if key == "obs.throttled" => {
            let safe = arg("safe")?;
            Some(fix(
                format!(
                    "# s5cmd was throttled with {safe} or more requests in flight: fewer workers (256 by default)\ns5cmd --numworkers {safe} cp …"
                ),
                Some(setting("s5cmd", "--numworkers", &safe, &[])),
            ))
        }
        // Trino's native S3 file system: `s3.retry-mode` (LEGACY by
        // default; STANDARD, ADAPTIVE), in the catalog's properties file
        (_, Some(Tool::Trino)) if key.starts_with("obs.throttled") => Some(fix(
            "# Trino was throttled: adaptive retries pace its requests to the store's (native S3 file system; LEGACY by default)\n# etc/catalog/<catalog>.properties\ns3.retry-mode=ADAPTIVE".into(),
            Some(setting("Trino", "s3.retry-mode", "ADAPTIVE", &[])),
        )),
        // Hadoop's s3a (hadoop-aws Constants.java, committers.md), whether
        // Spark sent it or not: its user agent says Hadoop either way
        _ if key == "obs.rename" => Some(fix(
            "# the S3A magic committer (Hadoop 3.3+): no rename\n# Spark (spark-defaults.conf), with its two committer classes:\nspark.hadoop.fs.s3a.committer.name=magic\nspark.sql.sources.commitProtocolClass=org.apache.spark.internal.io.cloud.PathOutputCommitProtocol\nspark.sql.parquet.output.committer.class=org.apache.spark.internal.io.cloud.BindingParquetOutputCommitter\n# Hadoop alone (core-site.xml): fs.s3a.committer.name = magic".into(),
            Some(setting("Hadoop s3a", "fs.s3a.committer.name", "magic", &[])),
        )),
        _ if key == "obs.markers" => Some(fix(
            "# keep markers: no delete after each file written beneath one (the default from Hadoop 3.4.0;\n# every client of the bucket marker-aware: Hadoop 3.2.2+ or 3.3.1+)\nspark.hadoop.fs.s3a.directory.marker.retention=keep\n# Hadoop alone (core-site.xml): fs.s3a.directory.marker.retention = keep".into(),
            Some(setting(
                "Hadoop s3a",
                "fs.s3a.directory.marker.retention",
                "keep",
                &[],
            )),
        )),
        _ => None,
    }
}

/// A finding's fixes once its engine is known: the engine's own first,
/// and an SDK's code only when the program calls an SDK itself.
pub fn apply(
    o: &mut crate::episode::Observation,
    engine: Engine,
    rs: &[&Record],
    records: &[Record],
) {
    let Some(said) = o.phrases.first() else {
        return;
    };
    let own = fix_for(said, engine, rs, records);
    if engine.is_some() {
        // the program calls no SDK itself: an SDK's code is no fix
        o.fixes.retain(|f| f.sdk == "config");
    }
    if let Some(f) = own {
        // the engine's setting in place of a store's or a general one
        if f.setting.is_some() || said.key == "obs.orphans" {
            o.fixes.retain(|g| g.sdk != "config");
        }
        o.fixes.insert(0, f);
    }
}

/// Files looked at for where a setting is set: the program's own sources
/// and its config, not what was installed or built beside them.
const LOCATE_FILES: usize = 4000;
const LOCATE_BYTES: u64 = 512 * 1024;
const LOCATE_DEPTH: usize = 5;

/// Where in the folder the program ran in (`cwd`) a setting is set now,
/// as `file:line` relative to it: the first line naming its key (or one of
/// its other spellings), in a file a person edits. Folders a build or an
/// install fills (`target`, `node_modules`, a virtualenv, a hidden one) are
/// not looked in.
pub fn locate(cwd: &Path, s: &Setting) -> Option<String> {
    let names: Vec<&str> = std::iter::once(s.key.as_str())
        .chain(s.also.iter().copied())
        .collect();
    let mut stack = vec![(cwd.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    let mut found: Option<(String, u32)> = None;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                let skipped = name.starts_with('.')
                    || matches!(
                        name.as_str(),
                        "target" | "node_modules" | "venv" | "__pycache__" | "build" | "dist"
                    )
                    || e.path().join("pyvenv.cfg").exists();
                if !skipped && depth < LOCATE_DEPTH {
                    stack.push((e.path(), depth + 1));
                }
                continue;
            }
            if !t.is_file() || name.ends_with(".sonde") || name.ends_with(".jsonl") {
                continue;
            }
            seen += 1;
            if seen > LOCATE_FILES {
                return found.map(|(f, l)| format!("{f}:{l}"));
            }
            if e.metadata().map_or(true, |m| m.len() > LOCATE_BYTES) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(e.path()) else {
                continue;
            };
            let Some(line) = text
                .lines()
                .position(|l| names.iter().any(|n| names_key(l, n)))
            else {
                continue;
            };
            let rel = e
                .path()
                .strip_prefix(cwd)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or(name);
            // the shallowest file, then by name: the same answer every time
            let better = match &found {
                None => true,
                Some((f, _)) => (rel.matches('/').count(), &rel) < (f.matches('/').count(), f),
            };
            if better {
                found = Some((rel, line as u32 + 1));
            }
        }
    }
    found.map(|(f, l)| format!("{f}:{l}"))
}

/// Whether a line names `key` as a word of its own: not `flush_interval`
/// inside `wal_flush_interval`.
fn names_key(line: &str, key: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    line.match_indices(key).any(|(i, _)| {
        let before = line[..i].chars().next_back();
        let after = line[i + key.len()..].chars().next();
        // (a dotted key under a prefix, `spark.hadoop.fs.s3a.…`, is the key)
        before.is_none_or(|c| !word(c)) && after.is_none_or(|c| !word(c))
    })
}

/// Fills in where each finding's setting is set, in the folder its program
/// ran in.
pub fn locate_all(findings: &mut [crate::episode::Observation], cwd: Option<&str>) {
    let Some(cwd) = cwd.map(Path::new).filter(|p| p.is_dir()) else {
        return;
    };
    for o in findings {
        for f in &mut o.fixes {
            if let Some(s) = &mut f.setting
                && s.at.is_none()
            {
                s.at = locate(cwd, s);
            }
        }
    }
}
