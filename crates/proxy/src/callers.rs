//! Who in the program sent a request: the place in its code (the
//! `x-sonde-caller` header a program, or sonde's Python hook, adds:
//! `file:line function`), the SDK and its version (the `user-agent`), and
//! the trace it belongs to (`traceparent`). A finding names the callers of
//! its requests, so it says what to change and where.

use std::collections::HashMap;

use reqlog::Record;
use serde::Serialize;

/// The header a program adds to say where a request comes from. sonde reads
/// it and never forwards it to the store.
pub const CALLER_HEADER: &str = "x-sonde-caller";

fn header<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.request_headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.trim())
        .filter(|v| !v.is_empty())
}

/// The place in the program's code that sent `r`, when it says.
pub fn caller(r: &Record) -> Option<String> {
    header(r, CALLER_HEADER).map(|v| unescape(v).chars().take(200).collect())
}

/// A header is ASCII: the hook writes other characters of a path or a
/// function name as `\uXXXX` (or `\xXX`, `\UXXXXXXXX`); they read as written.
fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut rest = v;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        let width = match tail.as_bytes().first() {
            Some(b'x') => 2,
            Some(b'u') => 4,
            Some(b'U') => 8,
            _ => 0,
        };
        let code = (width > 0)
            .then(|| tail.get(1..1 + width))
            .flatten()
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .and_then(char::from_u32);
        match code {
            Some(c) => {
                out.push(c);
                rest = &tail[1 + width..];
            }
            None => {
                out.push('\\');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The SDK that sent `r` and its version, from its user agent: `boto3 1.34`,
/// `aws-sdk-java 2.21`, `object_store 0.14`, `aws-cli 2.15`.
pub fn sdk(r: &Record) -> Option<String> {
    let ua = header(r, "user-agent")?;
    // the most telling product of the agent: an SDK's own name first
    const KNOWN: &[&str] = &[
        "Boto3",
        "botocore",
        "aws-cli",
        "aws-sdk-java",
        "aws-sdk-go-v2",
        "aws-sdk-go",
        "aws-sdk-js",
        "aws-sdk-rust",
        "aws-sdk-cpp",
        "aws-sdk-dotnet",
        "aws-sdk-php",
        "aws-sdk-ruby",
        "object_store",
        "minio-go",
        "MinIO",
        "s3fs",
        "hadoop-aws",
        "Hadoop",
        "DuckDB",
        "rclone",
        "curl",
    ];
    let products: Vec<(&str, &str)> = ua
        .split_whitespace()
        .filter_map(|p| p.split_once('/'))
        .collect();
    let (name, version) = KNOWN
        .iter()
        .find_map(|k| {
            products
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(k))
                .copied()
        })
        .or_else(|| products.first().copied())?;
    let version: String = version
        .split(['.', '-', '+'])
        .take(2)
        .collect::<Vec<_>>()
        .join(".");
    Some(format!("{} {version}", name.to_ascii_lowercase()))
}

/// The trace `r` belongs to (W3C `traceparent`'s trace id), when the program
/// propagates one.
pub fn trace(r: &Record) -> Option<String> {
    let tp = header(r, "traceparent")?;
    let id = tp.split('-').nth(1)?;
    (id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())).then(|| id.to_string())
}

/// The hook that names callers in a Python program (boto3 / botocore), as
/// the `sitecustomize` module Python loads at start.
pub const PYTHON_HOOK: &str = include_str!("callers_hook.py");

/// The hook for a Node.js program (the AWS SDK for JavaScript v3), loaded
/// with `NODE_OPTIONS=--require`.
pub const NODE_HOOK: &str = include_str!("callers_hook.cjs");

/// The Java agent (`-javaagent`) that adds [`JAVA_CALLER`] to a program's
/// class path when the AWS SDK for Java 2 is on it. Built from `hooks/java`.
pub const JAVA_AGENT: &[u8] = include_bytes!("hooks/sonde-agent.jar");

/// The AWS SDK for Java 2 interceptor that names callers.
pub const JAVA_CALLER: &[u8] = include_bytes!("hooks/sonde-caller.jar");

/// The environment that loads [`PYTHON_HOOK`] into a Python program: the
/// hook written into `dir`, which goes first on `PYTHONPATH`.
pub fn python_env(dir: &std::path::Path) -> std::io::Result<(String, String)> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("sitecustomize.py"), PYTHON_HOOK)?;
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut path = dir.to_string_lossy().into_owned();
    if let Ok(rest) = std::env::var("PYTHONPATH")
        && !rest.is_empty()
    {
        path = format!("{path}{sep}{rest}");
    }
    Ok(("PYTHONPATH".into(), path))
}

/// The environment that names callers in a Python, Node.js or Java program:
/// each language's hook written into `dir`, and the variable that loads it,
/// after what the variable held. A program of another language, or one
/// without its SDK, runs as it would.
pub fn caller_env(dir: &std::path::Path) -> std::io::Result<Vec<(String, String)>> {
    let mut env = vec![python_env(dir)?];
    std::fs::write(dir.join("sonde-caller.cjs"), NODE_HOOK)?;
    std::fs::write(dir.join("sonde-agent.jar"), JAVA_AGENT)?;
    std::fs::write(dir.join("sonde-caller.jar"), JAVA_CALLER)?;
    // both read their options split at spaces, a quoted one whole
    let quoted = |p: std::path::PathBuf| {
        let p = p.to_string_lossy().into_owned();
        (!p.contains('"')).then(|| format!("\"{p}\""))
    };
    let after = |var: &str, add: String| match std::env::var(var) {
        Ok(rest) if !rest.trim().is_empty() => format!("{} {add}", rest.trim()),
        _ => add,
    };
    if let Some(p) = quoted(dir.join("sonde-caller.cjs")) {
        env.push((
            "NODE_OPTIONS".into(),
            after("NODE_OPTIONS", format!("--require {p}")),
        ));
    }
    if let Some(p) = quoted(dir.join("sonde-agent.jar")) {
        env.push((
            "JAVA_TOOL_OPTIONS".into(),
            after("JAVA_TOOL_OPTIONS", format!("-javaagent:{p}")),
        ));
    }
    Ok(env)
}

/// A place in the program, with how many of a finding's requests it sent.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Caller {
    pub place: String,
    pub requests: usize,
}

/// The callers of `records`, most first (three at most), and their SDK.
pub fn of(records: &[&Record]) -> (Vec<Caller>, Option<String>) {
    let mut places: HashMap<String, usize> = HashMap::new();
    let mut sdks: HashMap<String, usize> = HashMap::new();
    for r in records {
        if let Some(c) = caller(r) {
            *places.entry(c).or_default() += 1;
        }
        if let Some(s) = sdk(r) {
            *sdks.entry(s).or_default() += 1;
        }
    }
    let mut callers: Vec<Caller> = places
        .into_iter()
        .map(|(place, requests)| Caller { place, requests })
        .collect();
    callers.sort_by(|a, b| b.requests.cmp(&a.requests).then(a.place.cmp(&b.place)));
    callers.truncate(3);
    let sdk = sdks
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(s, _)| s);
    (callers, sdk)
}

/// A finding's code place (`etl/load.py:212 read_manifest`) as a file and
/// line: the last `:digits` before a space or the end (a path may hold spaces
/// and a Windows drive's colon).
pub fn code_place(place: &str) -> Option<(String, u32)> {
    let bytes = place.as_bytes();
    let mut at = None;
    for (i, c) in place.char_indices() {
        if c != ':' {
            continue;
        }
        let digits = place[i + 1..]
            .chars()
            .take_while(|d| d.is_ascii_digit())
            .count();
        let end = i + 1 + digits;
        if digits > 0 && (end == place.len() || bytes[end] == b' ') {
            at = Some((i, end));
        }
    }
    let (colon, end) = at?;
    let file = place[..colon].trim();
    if file.is_empty() {
        return None;
    }
    Some((file.to_string(), place[colon + 1..end].parse().ok()?))
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_language_gets_its_hook_and_keeps_its_own_options() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("with space");
        let env = caller_env(&hooks).unwrap();
        for f in [
            "sitecustomize.py",
            "sonde-caller.cjs",
            "sonde-agent.jar",
            "sonde-caller.jar",
        ] {
            assert!(hooks.join(f).is_file(), "{f}");
        }
        let get = |k: &str| {
            env.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        // a path with a space is one option, quoted
        assert!(get("NODE_OPTIONS").ends_with(&format!(
            "--require \"{}\"",
            hooks.join("sonde-caller.cjs").display()
        )));
        assert!(get("JAVA_TOOL_OPTIONS").ends_with(&format!(
            "-javaagent:\"{}\"",
            hooks.join("sonde-agent.jar").display()
        )));
        assert!(get("PYTHONPATH").starts_with(&hooks.to_string_lossy().into_owned()));
        // the agent says how it starts; the interceptor how the SDK finds it
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        assert!(JAVA_AGENT.starts_with(b"PK"));
        assert!(
            text(JAVA_CALLER)
                .contains("software/amazon/awssdk/global/handlers/execution.interceptors")
        );
    }

    use super::*;
    use reqlog::Header;

    fn rec(headers: &[(&str, &str)]) -> Record {
        Record {
            request_headers: headers.iter().map(|(k, v)| Header::new(*k, *v)).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_place_outside_ascii_reads_as_written() {
        let r = rec(&[(
            "x-sonde-caller",
            r"数\u636e/load.py:3 \u8bfb\u53d6 caf\xe9 \q",
        )]);
        assert_eq!(caller(&r).as_deref(), Some(r"数据/load.py:3 读取 café \q"));
    }

    #[test]
    fn a_request_says_where_it_came_from() {
        let r = rec(&[
            (
                "User-Agent",
                "Boto3/1.34.12 md/Botocore#1.34.12 ua/2.0 os/linux Python/3.12.1 Botocore/1.34.12",
            ),
            ("x-sonde-caller", "etl/load.py:212 read_manifest"),
            (
                "traceparent",
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            ),
        ]);
        assert_eq!(sdk(&r).as_deref(), Some("boto3 1.34"));
        assert_eq!(caller(&r).as_deref(), Some("etl/load.py:212 read_manifest"));
        assert_eq!(
            trace(&r).as_deref(),
            Some("4bf92f3577b34da6a3ce929d0e0e4736")
        );
        assert_eq!(
            sdk(&rec(&[("user-agent", "object_store/0.14.2")])).as_deref(),
            Some("object_store 0.14")
        );
        let a = rec(&[("x-sonde-caller", "a.py:1")]);
        let b = rec(&[("x-sonde-caller", "b.py:2")]);
        let (callers, _) = of(&[&a, &b, &b]);
        assert_eq!(
            callers[0],
            Caller {
                place: "b.py:2".into(),
                requests: 2
            }
        );
    }
}
