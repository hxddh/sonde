//! The fix code sonde prints, checked as code: every piece of it
//! (`fixes::catalogue`, made from the phrases themselves, so a fix added to
//! any is here) in every SDK's language.
//!
//! Three depths, each said when it is passed over:
//!
//! - always, with nothing installed: each fix's brackets and strings close
//!   (and Rust's parse, with `syn`);
//! - with the language's own tools on the PATH (`python3`, `node`, `gofmt`,
//!   `java`): its parser reads each fix, and the fixes whose meaning matters
//!   (an upload's parts numbered and completed, a copy kept and dropped, a
//!   file of lines read) are run against a small client of the test's own;
//! - with the SDK itself there (`tools/fix-check`: the Go modules, Maven's
//!   jars, `node_modules`, the crates, each fetched beforehand; boto3 for
//!   Python): each fix is compiled against it (`go build`, `javac`, `tsc`,
//!   `cargo check`), boto3's run on the real client with its transport
//!   answered here.
//!
//! No test reaches the network. `SONDE_FIXES_STRICT=1` (CI's) makes a tool or
//! an SDK that is not there a failure instead of a line on stderr.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sonde_analysis::fixes::{Snippet, catalogue};

// ---- what is there

/// Said where `cargo test` shows it, passed or not: a test's own output is
/// kept back when it passes.
fn say(line: &str) {
    let _ = writeln!(std::io::stderr(), "fix_code: {line}");
}

/// A check passed over for want of a tool or an SDK: said, and a failure
/// when `SONDE_FIXES_STRICT` is set.
fn skipped(check: &str, why: &str) {
    assert!(
        std::env::var_os("SONDE_FIXES_STRICT").is_none_or(|v| v.is_empty() || v == "0"),
        "{check}: {why} (SONDE_FIXES_STRICT is set)"
    );
    say(&format!("SKIPPED {check}: {why}"));
}

/// Whether `tool` runs here.
fn have(tool: &str, arg: &str) -> bool {
    Command::new(tool)
        .arg(arg)
        .stdin(Stdio::null())
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `tools/fix-check`: each SDK's manifest, its versions pinned.
fn manifests() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/fix-check")
}

struct Ran {
    ok: bool,
    said: String,
}

fn ran(c: &mut Command) -> Ran {
    let o = match c.stdin(Stdio::null()).output() {
        Ok(o) => o,
        // (a tool that is not here did not succeed)
        Err(e) => {
            return Ran {
                ok: false,
                said: format!("error: {c:?}: {e}"),
            };
        }
    };
    Ran {
        ok: o.status.success(),
        said: format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
    }
}

/// `program` given `input`: whether it succeeded, and what it said.
fn fed(c: &mut Command, input: &str) -> Ran {
    let mut child = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("{c:?}: {e}"));
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    Ran {
        ok: o.status.success(),
        said: format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
    }
}

// ---- the fixes

fn of(sdk: &[&str]) -> Vec<Snippet> {
    catalogue()
        .into_iter()
        .filter(|s| sdk.contains(&s.fix.sdk))
        .collect()
}

fn fix(id: &str) -> String {
    catalogue()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("no fix {id}"))
        .fix
        .code
        .into_owned()
}

/// A fix's lines numbered, for a failure.
fn numbered(code: &str) -> String {
    code.lines()
        .enumerate()
        .map(|(i, l)| format!("{:4} {l}\n", i + 1))
        .collect()
}

/// Whether a fix declares (functions, what they keep) rather than does: a
/// line at the margin that begins a function.
fn declares(sdk: &str, code: &str) -> bool {
    code.lines().any(|l| match sdk {
        "go" => l.starts_with("func "),
        "rust" | "rust-aws" => l.starts_with("async fn ") || l.starts_with("fn "),
        "java" => {
            let first = l.split(['(', ' ']).next().unwrap_or("");
            l.ends_with('{')
                && l.contains('(')
                && !l.starts_with([' ', '}', '/'])
                && !matches!(first, "for" | "if" | "while" | "try" | "switch" | "do")
                && !l.contains(" = ")
                && !l.contains("->")
        }
        _ => false,
    })
}

/// A fix that ends on an expression (a client made with a setting), as a
/// statement.
fn closed(code: &str) -> String {
    match code.trim_end().ends_with([';', '}']) {
        true => code.to_string(),
        false => format!("{code};"),
    }
}

// ---- always: brackets and strings close

/// What is wrong with a fix's brackets or strings, if anything: comments and
/// strings passed over as its language writes them.
fn unbalanced(sdk: &str, code: &str) -> Option<String> {
    // (a setting's comments are a shell's)
    let python = matches!(sdk, "python" | "config");
    let rust = matches!(sdk, "rust" | "rust-aws");
    let c: Vec<char> = code.chars().collect();
    let mut open: Vec<(char, usize)> = Vec::new();
    let mut line = 1;
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        match ch {
            '\n' => line += 1,
            '#' if python => {
                while i + 1 < c.len() && c[i + 1] != '\n' {
                    i += 1;
                }
            }
            '/' if !python && next == Some('/') => {
                while i + 1 < c.len() && c[i + 1] != '\n' {
                    i += 1;
                }
            }
            // a lifetime or a label is no string
            '\'' if rust && c.get(i + 2) != Some(&'\'') && next != Some('\\') => {}
            '"' | '\'' | '`' if ch != '`' || !python => {
                let from = line;
                loop {
                    i += 1;
                    match c.get(i) {
                        None => return Some(format!("line {from}: a string is not closed")),
                        Some('\\') => i += 1,
                        Some('\n') if ch != '`' => {
                            return Some(format!("line {from}: a string is not closed"));
                        }
                        Some('\n') => line += 1,
                        Some(x) if *x == ch => break,
                        _ => {}
                    }
                }
            }
            '(' | '[' | '{' => open.push((ch, line)),
            ')' | ']' | '}' => {
                let want = match ch {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                match open.pop() {
                    Some((o, _)) if o == want => {}
                    Some((o, at)) => {
                        return Some(format!("line {line}: {ch} closes the {o} of line {at}"));
                    }
                    None => return Some(format!("line {line}: {ch} closes nothing")),
                }
            }
            _ => {}
        }
        i += 1;
    }
    open.pop()
        .map(|(o, at)| format!("line {at}: {o} is not closed"))
}

#[test]
fn the_catalogue_holds_every_fix_and_names_each_once() {
    let all = catalogue();
    let ids: BTreeSet<&str> = all.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids.len(), all.len(), "a fix named twice");
    // a finding's, in each form its requests give it; a trial's advice
    for id in [
        "obs.hot/python",
        "obs.hot[rewritten]/go",
        "obs.rangeagain[rewritten]/js",
        "obs.small[records]/java",
        "obs.smallreads[media]/python",
        "obs.serial[PART]/rust",
        "obs.serial[LIST]/rust-aws",
        "obs.deletes/rust-aws",
        "obs.rename/config",
        "trial.advice.cut/python",
        "trial.advice.retries/rust-aws",
    ] {
        assert!(ids.contains(id), "{id} is not in {ids:#?}");
    }
    for s in &all {
        let known = ["python", "java", "go", "js", "rust", "rust-aws", "config"];
        assert!(known.contains(&s.fix.sdk), "{}", s.id);
        // its first line a comment that says the change
        let comment = if matches!(s.fix.sdk, "python" | "config") {
            "# "
        } else {
            "// "
        };
        assert!(s.fix.code.starts_with(comment), "{}:\n{}", s.id, s.fix.code);
        assert!(!s.fix.code.ends_with('\n'), "{}", s.id);
        assert!(!s.fix.code.contains('\t'), "{}: a tab", s.id);
    }
    // every fix a finding is given is one of them: the forms `for_finding`
    // makes are the catalogue's
    let r = |kind: &str, method: &str, key: &str| reqlog::Record {
        kind: kind.into(),
        method: method.into(),
        key: Some(key.into()),
        ..Default::default()
    };
    let codes: BTreeSet<&str> = all.iter().map(|s| s.fix.code.as_ref()).collect();
    for key in [
        "obs.hot",
        "obs.rangeagain",
        "obs.small",
        "obs.smallreads",
        "obs.serial",
        "obs.listed",
    ] {
        for rs in [
            vec![r("GetObject", "GET", "a.json")],
            vec![r("PutObject", "PUT", "a.png")],
            vec![r("HeadObject", "HEAD", "a")],
            vec![r("DeleteObject", "DELETE", "a")],
            vec![r("ListObjectsV2", "GET", "")],
            vec![r("UploadPart", "PUT", "a")],
        ] {
            for rewritten in [false, true] {
                let rs: Vec<&reqlog::Record> = rs.iter().collect();
                for f in sonde_analysis::fixes::for_finding(key, &rs, rewritten) {
                    assert!(codes.contains(f.code.as_ref()), "{key}: {}", f.code);
                }
            }
        }
    }
}

#[test]
fn every_fix_closes_its_brackets_and_strings() {
    let all = catalogue();
    assert!(all.len() > 100, "{} fixes", all.len());
    for s in &all {
        if let Some(wrong) = unbalanced(s.fix.sdk, &s.fix.code) {
            panic!("{}: {wrong}\n{}", s.id, numbered(&s.fix.code));
        }
    }
    // the check itself: what it must not pass, and what it must
    assert!(unbalanced("js", "f(a[1)]").is_some());
    assert!(unbalanced("go", "s := \"a").is_some());
    assert!(unbalanced("python", "x = {  # }\n").is_some());
    assert!(unbalanced("python", "x = \"}\"  # it's\n").is_none());
    assert!(unbalanced("rust", "fn f<'a>(x: &'a str) -> char { '}' }").is_none());
    assert!(unbalanced("js", "const k = `${a}\n${b}`; // (").is_none());
}

#[test]
fn every_rust_fix_parses() {
    for s in of(&["rust", "rust-aws"]) {
        let code = s.fix.code.as_ref();
        let file = match declares(s.fix.sdk, code) {
            true => code.to_string(),
            false => format!("async fn fix() {{ let _it = {{\n{code}\n}}; }}"),
        };
        if let Err(e) = syn::parse_file(&file) {
            panic!("{}: {e}\n{}", s.id, numbered(code));
        }
    }
    // an engine's setting said as Rust (SlateDB's) is checked where it is
    // made: tests/engines.rs
}

/// A store's or an engine's setting: each line a comment, a property
/// (`name=value`, nothing after it) or sonde's own command.
#[test]
fn every_setting_is_lines_of_properties_or_a_command() {
    for s in of(&["config"]) {
        for line in s.fix.code.lines().filter(|l| !l.starts_with('#')) {
            let property = line
                .split_once('=')
                .is_some_and(|(k, v)| !k.contains(' ') && !v.is_empty() && !v.contains(' '));
            assert!(
                property || line.starts_with("sonde bucket lifecycle --bucket "),
                "{}: {line}",
                s.id
            );
        }
    }
}

// ---- with the language's parser

#[test]
fn every_python_fix_parses() {
    if !have("python3", "--version") {
        return skipped("Python's parser", "no python3");
    }
    let all: Vec<(String, String)> = of(&["python"])
        .into_iter()
        .map(|s| (s.id, s.fix.code.into_owned()))
        .collect();
    let r = fed(
        Command::new("python3").args(["-I", "-c", PY_PARSE]),
        &serde_json::to_string(&all).unwrap(),
    );
    assert!(r.ok, "{}", r.said);
    assert!(
        r.said.contains(&format!("parsed {}", all.len())),
        "{}",
        r.said
    );
}

const PY_PARSE: &str = r#"
import ast, json, sys
bad = 0
fixes = json.load(sys.stdin)
for name, code in fixes:
    try:
        compile(ast.parse(code), name, "exec")
    except SyntaxError as e:
        bad += 1
        print(f"{name}: line {e.lineno}: {e.msg}\n{code}", file=sys.stderr)
print("parsed", len(fixes))
sys.exit(1 if bad else 0)
"#;

/// The JavaScript fixes as one module, each in a function of its own (they
/// await).
fn js_module(all: &[Snippet], prelude: &str) -> String {
    let mut out = String::from(prelude);
    for (i, s) in all.iter().enumerate() {
        out.push_str(&format!(
            "\n// {}\nasync function fix{i}() {{\n{}\n}}\n",
            s.id, s.fix.code
        ));
    }
    out
}

#[test]
fn every_js_fix_parses() {
    if !have("node", "--version") {
        return skipped("JavaScript's parser", "no node");
    }
    let t = tempfile::tempdir().unwrap();
    let all = of(&["js"]);
    for s in &all {
        // (one at a time: a failure names its fix)
        let file = t.path().join("fix.mjs");
        std::fs::write(&file, js_module(std::slice::from_ref(s), "")).unwrap();
        let r = ran(Command::new("node").arg("--check").arg(&file));
        assert!(r.ok, "{}: {}\n{}", s.id, r.said, numbered(&s.fix.code));
    }
    assert!(all.len() > 20);
}

const GO_IMPORTS: &str = r#"
import (
	"archive/tar"
	"bufio"
	"bytes"
	"context"
	"fmt"
	"hash/crc32"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/aws/aws-sdk-go-v2/aws"
	awshttp "github.com/aws/aws-sdk-go-v2/aws/transport/http"
	"github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/feature/s3/manager"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	"github.com/aws/aws-sdk-go-v2/service/s3/types"
	"golang.org/x/sync/errgroup"
)

// (each import is used, whatever the fix uses)
var (
	_ = tar.NewReader
	_ = bufio.NewScanner
	_ = bytes.NewReader
	_ = context.Background
	_ = fmt.Sprintf
	_ = crc32.ChecksumIEEE
	_ = io.ReadAll
	_ = http.DefaultClient
	_ = strings.HasPrefix
	_ sync.Mutex
	_ = time.Second
	_ = aws.String
	_ = awshttp.NewBuildableClient
	_ = config.LoadDefaultConfig
	_ = manager.NewUploader
	_ = s3.NewFromConfig
	_ types.Object
	_ = errgroup.WithContext
)
"#;

/// What a Go fix names and does not declare: the program's own.
const GO_NAMES: &str = r#"
var (
	s3c                                           *s3.Client
	cfg                                           aws.Config
	b, k, key, prefix, name, uploadID, batchID    string
	keys, prefixes                                []string
	bodies, files                                 map[string][]byte
	chunks, batch                                 [][]byte
	start, end                                    int64
	changed                                       func(io.Reader) io.Reader
)
"#;

/// A Go fix as a file of package `pkg`: what declares at the package's
/// level, what does in a function.
fn go_file(pkg: &str, code: &str) -> String {
    match declares("go", code) {
        true => format!("package {pkg}\n{GO_IMPORTS}{GO_NAMES}\n{code}\n"),
        false => format!(
            "package {pkg}\n{GO_IMPORTS}{GO_NAMES}\nfunc fix(ctx context.Context) error {{\n{code}\nreturn nil\n}}\n"
        ),
    }
}

#[test]
fn every_go_fix_parses() {
    if !have("gofmt", "-l") {
        return skipped("Go's parser", "no gofmt");
    }
    let t = tempfile::tempdir().unwrap();
    for s in of(&["go"]) {
        let file = t.path().join("fix.go");
        std::fs::write(&file, go_file("fix", &s.fix.code)).unwrap();
        let r = ran(Command::new("gofmt").arg("-e").arg(&file));
        assert!(r.ok, "{}: {}\n{}", s.id, r.said, numbered(&s.fix.code));
    }
}

const JAVA_IMPORTS: &str = r#"
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.*;
import java.util.concurrent.*;
import java.util.stream.*;
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream;
import software.amazon.awssdk.awscore.retry.AwsRetryStrategy;
import software.amazon.awssdk.core.*;
import software.amazon.awssdk.core.async.*;
import software.amazon.awssdk.core.retry.RetryMode;
import software.amazon.awssdk.core.sync.*;
import software.amazon.awssdk.http.apache5.Apache5HttpClient;
import software.amazon.awssdk.services.s3.*;
import software.amazon.awssdk.services.s3.model.*;
import software.amazon.awssdk.transfer.s3.S3TransferManager;
"#;

/// What a Java fix names and does not declare: the program's own.
const JAVA_NAMES: &str = r#"
    S3Client s3;
    S3AsyncClient s3Async;
    String b, k, key, prefix, p, name, uploadId, batchId;
    List<String> keys, prefixes;
    Map<String, byte[]> bodies;
    List<byte[]> chunks;
    List<Object> batch;
    Object files;
    long start, end;
    interface Spark { Object sql(String statement); }
    Spark spark;
    byte[] changed(byte[] read) { return read; }
    byte[] tarOf(Object files) { return new byte[0]; }
"#;

fn java_file(class: &str, code: &str) -> String {
    match declares("java", code) {
        true => format!("{JAVA_IMPORTS}\nclass {class} {{\n{JAVA_NAMES}\n{code}\n}}\n"),
        false => format!(
            "{JAVA_IMPORTS}\nclass {class} {{\n{JAVA_NAMES}\n    void fix() throws Exception {{\n{}\n    }}\n}}\n",
            closed(code)
        ),
    }
}

/// Each Java fix as a class in a file of its own under `dir`.
fn java_files(dir: &Path) -> Vec<(Snippet, PathBuf)> {
    of(&["java"])
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let class = format!("Fix{i}");
            let file = dir.join(format!("{class}.java"));
            std::fs::write(&file, java_file(&class, &s.fix.code)).unwrap();
            (s, file)
        })
        .collect()
}

/// What javac said, with each fix it names.
fn java_said(said: &str, files: &[(Snippet, PathBuf)]) -> String {
    let mut out = said.to_string();
    for (s, file) in files {
        let name = file.file_name().unwrap().to_string_lossy();
        if said.contains(name.as_ref()) {
            out.push_str(&format!(
                "\n{name} is {}:\n{}",
                s.id,
                numbered(&std::fs::read_to_string(file).unwrap())
            ));
        }
    }
    out
}

/// javac's parser alone, no class looked up: what reads a fix where the
/// SDK's jars are not.
const JAVA_PARSE: &str = r#"
import com.sun.source.util.JavacTask;
import java.util.List;
import javax.tools.*;

public class Parse {
    public static void main(String[] files) throws Exception {
        JavaCompiler javac = ToolProvider.getSystemJavaCompiler();
        DiagnosticCollector<JavaFileObject> said = new DiagnosticCollector<>();
        try (StandardJavaFileManager fm = javac.getStandardFileManager(said, null, null)) {
            JavacTask task = (JavacTask) javac.getTask(null, fm, said, List.of("-proc:none"), null, fm.getJavaFileObjects(files));
            task.parse();
        }
        boolean bad = false;
        for (Diagnostic<? extends JavaFileObject> d : said.getDiagnostics()) {
            if (d.getKind() == Diagnostic.Kind.ERROR) {
                System.err.println(d);
                bad = true;
            }
        }
        System.out.println("parsed " + files.length);
        System.exit(bad ? 1 : 0);
    }
}
"#;

#[test]
fn every_java_fix_parses() {
    if !have("javac", "-version") || !have("java", "-version") {
        return skipped("Java's parser", "no JDK (java and javac)");
    }
    let t = tempfile::tempdir().unwrap();
    let files = java_files(t.path());
    let parse = t.path().join("Parse.java");
    std::fs::write(&parse, JAVA_PARSE).unwrap();
    let r = ran(Command::new("java")
        .arg(&parse)
        .args(files.iter().map(|(_, f)| f)));
    assert!(r.ok, "{}", java_said(&r.said, &files));
    assert!(
        r.said.contains(&format!("parsed {}", files.len())),
        "{}",
        r.said
    );
}

// ---- with the SDK: compiled

/// A copy of one of `tools/fix-check`'s manifests' files in `to`.
fn manifest(lang: &str, files: &[&str], to: &Path) {
    for f in files {
        let from = manifests().join(lang).join(f);
        std::fs::copy(&from, to.join(f)).unwrap_or_else(|e| panic!("{}: {e}", from.display()));
    }
}

fn go(dir: &Path) -> Command {
    let mut c = Command::new("go");
    // (never the network: what is not in the module cache is not there)
    c.current_dir(dir)
        .env("GOPROXY", "off")
        .env("GOFLAGS", "-mod=mod");
    c
}

/// A folder with the Go manifest whose modules are all here, or none (said).
fn go_module(check: &str) -> Option<tempfile::TempDir> {
    if !have("go", "version") {
        skipped(check, "no go");
        return None;
    }
    let t = tempfile::tempdir().unwrap();
    manifest("go", &["go.mod", "go.sum", "deps.go"], t.path());
    let r = ran(go(t.path()).args(["build", "./..."]));
    if !r.ok {
        skipped(
            check,
            &format!(
                "the Go modules of tools/fix-check/go are not in the module cache (go mod download there): {}",
                r.said.lines().next().unwrap_or("")
            ),
        );
        return None;
    }
    Some(t)
}

/// What `go build` said but for what a fix leaves to the program around it:
/// a result it names and does not use.
fn go_errors(said: &str) -> Vec<&str> {
    said.lines()
        .filter(|l| !l.starts_with('#') && !l.contains("declared and not used"))
        .filter(|l| !l.contains("too many errors"))
        .collect()
}

#[test]
fn every_go_fix_compiles_against_the_sdk() {
    let Some(t) = go_module("Go fixes compiled") else {
        return;
    };
    let all = of(&["go"]);
    for (i, s) in all.iter().enumerate() {
        let dir = t.path().join(format!("fix{i}"));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("fix.go"), go_file(&format!("fix{i}"), &s.fix.code)).unwrap();
    }
    let r = ran(go(t.path()).args(["build", "-gcflags=-e", "./..."]));
    let wrong = go_errors(&r.said);
    if !wrong.is_empty() {
        let mut said = wrong.join("\n");
        for (i, s) in all.iter().enumerate() {
            if wrong.iter().any(|l| l.contains(&format!("fix{i}/"))) {
                let file = go_file(&format!("fix{i}"), &s.fix.code);
                said.push_str(&format!("\nfix{i} is {}:\n{}", s.id, numbered(&file)));
            }
        }
        panic!("{said}");
    }
    say(&format!(
        "compiled {} Go fixes against aws-sdk-go-v2",
        all.len()
    ));
}

#[test]
fn every_java_fix_compiles_against_the_sdk() {
    let check = "Java fixes compiled";
    if !have("javac", "-version") {
        return skipped(check, "no javac");
    }
    if !have("mvn", "-v") {
        return skipped(check, "no mvn to name the SDK's jars");
    }
    let t = tempfile::tempdir().unwrap();
    manifest("java", &["pom.xml"], t.path());
    let jars = t.path().join("classpath");
    // (offline: what is not in ~/.m2 is not there)
    let r = ran(Command::new("mvn")
        .current_dir(t.path())
        .args(["-o", "-q", "-B", "dependency:build-classpath"])
        .arg(format!("-Dmdep.outputFile={}", jars.display())));
    let Some(classpath) = std::fs::read_to_string(&jars).ok().filter(|_| r.ok) else {
        return skipped(
            check,
            "the jars of tools/fix-check/java are not in ~/.m2 (mvn dependency:go-offline there)",
        );
    };
    let src = t.path().join("src");
    std::fs::create_dir(&src).unwrap();
    let files = java_files(&src);
    let out = t.path().join("classes");
    let r = ran(Command::new("javac")
        .args([
            "-proc:none",
            "-nowarn",
            "-Xmaxerrs",
            "1000",
            "--release",
            "17",
            "-cp",
        ])
        .arg(classpath.trim())
        .arg("-d")
        .arg(&out)
        .args(files.iter().map(|(_, f)| f)));
    assert!(r.ok, "{}", java_said(&r.said, &files));
    say(&format!(
        "compiled {} Java fixes against the AWS SDK for Java 2",
        files.len()
    ));
}

const TS_NAMES: &str = r#"
import { createHash } from "node:crypto";
import {
  CompleteMultipartUploadCommand, DeleteObjectCommand, DeleteObjectsCommand, GetObjectCommand, HeadObjectCommand,
  PutObjectCommand, S3Client, UploadPartCommand, paginateListObjectsV2, waitUntilObjectExists,
} from "@aws-sdk/client-s3";
import { Upload } from "@aws-sdk/lib-storage";
import { NodeHttpHandler } from "@smithy/node-http-handler";
import pLimit from "p-limit";

// what a fix names and does not declare: the program's own
declare const s3: S3Client;
declare const b: string, k: string, key: string, p: string, prefix: string, name: string;
declare const batchId: string, uploadId: string, start: number, end: number;
declare const keys: string[], prefixes: string[], batch: unknown[], files: unknown;
declare const bodies: Record<string, Uint8Array>, chunks: Uint8Array[];
declare const params: { Bucket: string; Key: string; Body: Uint8Array };
declare function changed(read: string): string;
declare function tarOf(files: unknown): Uint8Array;
declare function untar(body: unknown): unknown;
"#;

#[test]
fn every_js_fix_type_checks_against_the_sdk() {
    let check = "JavaScript fixes type-checked";
    if !have("node", "--version") {
        return skipped(check, "no node");
    }
    let modules = manifests().join("node/node_modules");
    let tsc = modules.join("typescript/bin/tsc");
    if !tsc.exists() || !modules.join("@aws-sdk/client-s3").exists() {
        return skipped(
            check,
            "tools/fix-check/node has no node_modules (npm ci there)",
        );
    }
    let t = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&modules, t.path().join("node_modules")).unwrap();
    #[cfg(not(unix))]
    return skipped(check, "no link to node_modules on this platform");
    let all = of(&["js"]);
    let module = js_module(&all, TS_NAMES);
    std::fs::write(t.path().join("fixes.ts"), &module).unwrap();
    std::fs::write(t.path().join("package.json"), r#"{"type": "module"}"#).unwrap();
    std::fs::write(
        t.path().join("tsconfig.json"),
        r#"{"compilerOptions": {"target": "es2022", "module": "nodenext", "moduleResolution": "nodenext", "strict": false, "noEmit": true, "skipLibCheck": true, "types": ["node"]}, "files": ["fixes.ts"]}"#,
    )
    .unwrap();
    let r = ran(Command::new("node")
        .current_dir(t.path())
        .arg(&tsc)
        .args(["-p", "."]));
    assert!(r.ok, "{}\n{}", r.said, numbered(&module));
    say(&format!(
        "type-checked {} JavaScript fixes against the AWS SDK for JavaScript 3",
        all.len()
    ));
}

const RUST_HEAD: &str = r#"
#![allow(unused, clippy::all)]
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures::{StreamExt, TryStreamExt};

type Failed = Box<dyn Error + Send + Sync>;
"#;

const RUST_AWS_USES: &str = r#"
    use super::*;
    use aws_config::retry::RetryConfig;
    use aws_config::timeout::TimeoutConfig;
    use aws_config::BehaviorVersion;
    use aws_sdk_s3::client::Waiters;
    use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart, Delete, ObjectIdentifier};
    use aws_sdk_s3::Client;
    fn changed(read: Bytes) -> Bytes { read }
"#;

/// What a fix for the AWS SDK for Rust names and does not declare.
const RUST_AWS_NAMES: &str = "s3: &Client, b: &str, k: &str, key: &str, p: &str, prefix: &str, name: &str, upload_id: &str, batch_id: &str, keys: Vec<String>, prefixes: Vec<String>, bodies: &HashMap<String, Bytes>, chunks: Vec<Bytes>, batch: Vec<String>, start: u64, end: u64";

const RUST_STORE_USES: &str = r#"
    use super::*;
    use object_store::path::Path;
    use object_store::{ObjectStore, ObjectStoreExt};
"#;

/// What a fix for object_store names and does not declare.
const RUST_STORE_NAMES: &str = "store: Arc<dyn ObjectStore>, path: Path, prefix: Path, paths: Vec<Path>, payloads: &HashMap<Path, Bytes>, chunks: Vec<Bytes>";

#[test]
fn every_rust_fix_compiles_against_its_crate() {
    let check = "Rust fixes compiled";
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let t = tempfile::tempdir().unwrap();
    manifest("rust", &["Cargo.toml", "Cargo.lock"], t.path());
    std::fs::create_dir(t.path().join("src")).unwrap();
    let all = of(&["rust", "rust-aws"]);
    let mut lib = String::from(RUST_HEAD);
    for (i, s) in all.iter().enumerate() {
        let (uses, names) = match s.fix.sdk {
            "rust" => (RUST_STORE_USES, RUST_STORE_NAMES),
            _ => (RUST_AWS_USES, RUST_AWS_NAMES),
        };
        let code = s.fix.code.as_ref();
        let body = match declares(s.fix.sdk, code) {
            true => code.to_string(),
            false => format!(
                "async fn fix({names}) -> Result<(), Failed> {{\nlet _it = {{\n{code}\n}};\nOk(())\n}}"
            ),
        };
        lib.push_str(&format!("\n// {}\nmod fix{i} {{{uses}\n{body}\n}}\n", s.id));
    }
    std::fs::write(t.path().join("src/lib.rs"), &lib).unwrap();
    // its dependencies built once and kept, beside this test's own
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fix-check-rust");
    let mut c = Command::new(cargo);
    c.current_dir(t.path())
        .args(["check", "--offline", "--locked", "--message-format=short"])
        .env("CARGO_TARGET_DIR", &target)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS");
    let r = ran(&mut c);
    if !r.ok && !r.said.contains("src/lib.rs") {
        // (not the fixes' doing: a crate that is not here)
        return skipped(
            check,
            &format!(
                "the crates of tools/fix-check/rust are not here (cargo fetch there): {}",
                r.said
                    .lines()
                    .find(|l| l.starts_with("error"))
                    .unwrap_or("")
            ),
        );
    }
    assert!(r.ok, "{}\n{}", r.said, numbered(&lib));
    say(&format!(
        "compiled {} Rust fixes against aws-sdk-s3 and object_store",
        all.len()
    ));
}

/// boto3's fixes run on boto3's own client: its parameters checked by
/// botocore, its answers parsed by it, the transport answered here.
const PY_BOTO3: &str = r#"
import io, json, os, sys, tempfile
import boto3
from botocore.awsrequest import AWSResponse

ANSWERS = {
    "ListObjectsV2": b"<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>p/a</Key></Contents></ListBucketResult>",
    "DeleteObjects": b"<DeleteResult></DeleteResult>",
    "CompleteMultipartUpload": b"<CompleteMultipartUploadResult><ETag>&quot;e&quot;</ETag></CompleteMultipartUploadResult>",
    "CreateMultipartUpload": b"<InitiateMultipartUploadResult><UploadId>u</UploadId></InitiateMultipartUploadResult>",
    "GetObject": b'{"a": 1}\n{"a": 2}\n',
}

class Raw(io.BytesIO):
    def stream(self, amt=1024, decode_content=None):
        while chunk := self.read(amt):
            yield chunk
    def release_conn(self):
        pass

def answered(sent):
    def send(request, event_name=None, **_):
        op = event_name.rsplit(".", 1)[-1]
        sent.append(op)
        body = ANSWERS.get(op, b"")
        if op == "GetObject" and ".tar" in request.url:
            body = bytes(10240)  # a tar with no member
        if op == "GetObject" and "_manifest.json" in request.url:
            body = b'["p/a"]'
        return AWSResponse(request.url, 200, {"ETag": '"e"', "Content-Length": str(len(body))}, Raw(body))
    return send

real = boto3.client
def client(*a, **kw):
    # (a fix that makes its own client: answered here too)
    c = real(*a, **kw)
    c.meta.events.register("before-send.s3", answered(SENT))
    return c
boto3.client = client

bad, ran, passed = 0, 0, []
fixes = json.load(sys.stdin)
tmp = tempfile.NamedTemporaryFile(delete=False)
tmp.write(b"x" * 100)
tmp.close()
for name, code in fixes:
    SENT = []
    scope = {
        "boto3": boto3, "s3": client("s3"), "b": "demo", "k": "k", "key": "k", "p": "p/", "prefix": "p/",
        "name": "n", "keys": ["a", "b"], "prefixes": ["p/", "q/"], "batch": [{"a": 1}], "batch_id": "1",
        "bodies": {"a": b"x", "b": b"y"}, "chunks": [b"1", b"2", b"3"], "upload_id": "u", "files": [("f", tmp.name)],
        "path": tmp.name, "changed": lambda read: read, "start": 0, "end": 9, "uri": "s3://demo/t",
    }
    try:
        exec(compile(code, name, "exec"), scope)
        for f in ("read", "read_range"):
            if f in scope:
                args = ("k",) if f == "read" else ("k", 0, 9)
                before = len(SENT)
                assert scope[f](*args) == scope[f](*args)
                assert len(SENT) == before + 1, f"{f} read twice: {SENT}"
        if "write" in scope:
            scope["write"]("k", b"new")
        ran += 1
    except ImportError as e:
        if e.name in ("boto3", "botocore"):
            raise
        passed.append(f"{name} (needs {e.name})")
    except Exception as e:
        bad += 1
        print(f"{name}: {type(e).__name__}: {e}\n{code}", file=sys.stderr)
os.unlink(tmp.name)
print("ran", ran, "of", len(fixes), "on boto3", boto3.__version__, "; not run:", ", ".join(passed) or "none")
sys.exit(1 if bad else 0)
"#;

/// Whether this boto3 knows the newest parameter a fix sends (a write
/// only over what was read): read from the model it validates with.
const PY_BOTO3_KNOWS: &str = r#"
import sys, botocore.session
put = botocore.session.get_session().get_service_model("s3").operation_model("PutObject")
sys.exit(0 if "IfMatch" in put.input_shape.members else 1)
"#;

#[test]
fn every_boto3_fix_runs_on_boto3_itself() {
    let check = "boto3 fixes run on boto3";
    if !have("python3", "--version") {
        return skipped(check, "no python3");
    }
    if !ran(Command::new("python3").args(["-c", "import boto3"])).ok {
        return skipped(check, "no boto3 for python3 (pip install boto3)");
    }
    // a boto3 from before conditional writes (a system package years old)
    // does not know the fixes' parameters: it is not the SDK they are for,
    // asked of its own model, not of a version number
    if !ran(Command::new("python3").args(["-c", PY_BOTO3_KNOWS])).ok {
        return skipped(
            check,
            "this python3's boto3 is older than the fixes' API (its PutObject has no IfMatch): pip install -U boto3",
        );
    }
    let all: Vec<(String, String)> = of(&["python"])
        .into_iter()
        .map(|s| (s.id, s.fix.code.into_owned()))
        .collect();
    let mut c = Command::new("python3");
    c.args(["-c", PY_BOTO3])
        .env("AWS_ACCESS_KEY_ID", "test")
        .env("AWS_SECRET_ACCESS_KEY", "test")
        .env("AWS_DEFAULT_REGION", "us-east-1")
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .env("AWS_ENDPOINT_URL", "http://127.0.0.1:9")
        .env_remove("AWS_PROFILE");
    let r = fed(&mut c, &serde_json::to_string(&all).unwrap());
    assert!(r.ok, "{}", r.said);
    say(r.said.lines().last().unwrap_or(""));
}

// ---- what a fix does: run against a client of the test's own

/// A boto3 client as the fixes below use it, each request kept in `sent`:
/// the objects it is given, an upload's parts by their number (S3's start at
/// 1, and it names each by its ETag).
const PY_CLIENT: &str = r#"
import sys
class Body:
    def __init__(self, data): self.data = data
    def read(self): return self.data
    def iter_lines(self): return self.data.splitlines()
class S3:
    def __init__(self, objects=None):
        self.objects, self.sent, self.parts, self.completed = dict(objects or {}), [], {}, None
    def get_object(self, Bucket, Key, Range=None):
        self.sent.append(("GET", Key, Range))
        return {"Body": Body(self.objects[Key]), "ETag": '"e"'}
    def put_object(self, Bucket, Key, Body):
        self.sent.append(("PUT", Key))
        self.objects[Key] = Body
        return {}
    def upload_part(self, Bucket, Key, UploadId, PartNumber, Body):
        assert 1 <= PartNumber <= 10000, f"part number {PartNumber}"
        self.parts[PartNumber] = Body
        return {"ETag": f'"etag-of-{Body.decode()}"'}
    def complete_multipart_upload(self, Bucket, Key, UploadId, MultipartUpload):
        self.completed = MultipartUpload
b, key, upload_id = "demo", "big", "u"
"#;

/// `program` run by python3, or none where there is none (said).
fn python(check: &str, program: &str) -> Option<Ran> {
    if !have("python3", "--version") {
        skipped(check, "no python3");
        return None;
    }
    Some(ran(Command::new("python3").args(["-I", "-c", program])))
}

#[test]
fn boto3_parts_are_numbered_from_one_and_completed_with_their_etags_in_order() {
    let program = format!(
        r#"{PY_CLIENT}
s3 = S3()
chunks = [b"a", b"b", b"c", b"d", b"e", b"f", b"g", b"h", b"i", b"j", b"k"]
{}
assert s3.parts == {{n + 1: c for n, c in enumerate(chunks)}}, s3.parts
assert s3.completed == {{"Parts": [{{"PartNumber": n + 1, "ETag": f'"etag-of-{{c.decode()}}"'}} for n, c in enumerate(chunks)]}}, s3.completed
"#,
        fix("obs.serial[PART]/python")
    );
    if let Some(r) = python("boto3's parts at once", &program) {
        assert!(r.ok, "{}\n{program}", r.said);
    }
}

#[test]
fn boto3_reads_kept_are_read_once_and_again_after_the_programs_own_write() {
    for (id, read) in [
        ("obs.hot", "read(\"conf\")"),
        ("obs.rangeagain", "read_range(\"conf\", 0, 9)"),
    ] {
        // kept for the run: one request, however many reads
        let kept = format!(
            "{PY_CLIENT}\ns3 = S3({{\"conf\": b\"one\"}})\n{}\nassert {read} == b\"one\" and {read} == b\"one\"\nassert len(s3.sent) == 1, s3.sent\n",
            fix(&format!("{id}/python"))
        );
        // written by the program between its reads: the copy is dropped
        // with the write, and the read after it is the store's
        let rewritten = format!(
            "{PY_CLIENT}\ns3 = S3({{\"conf\": b\"one\", \"other\": b\"x\"}})\n{}\nassert {read} == b\"one\" and {read} == b\"one\"\nassert len(s3.sent) == 1, s3.sent\nwrite(\"other\", b\"y\")\nassert {read} == b\"one\" and len(s3.sent) == 2, s3.sent\nwrite(\"conf\", b\"two\")\nassert {read} == b\"two\", \"a stale read after the program's own write\"\nassert {read} == b\"two\" and len(s3.sent) == 4, s3.sent\n",
            fix(&format!("{id}[rewritten]/python"))
        );
        for program in [kept, rewritten] {
            if let Some(r) = python("boto3's reads kept", &program) {
                assert!(r.ok, "{id}: {}\n{program}", r.said);
            }
        }
    }
}

#[test]
fn boto3_lines_are_read_whatever_the_file_ends_with() {
    for body in [
        "{\\\"a\\\": 1}\\n{\\\"a\\\": 2}",
        "{\\\"a\\\": 1}\\n{\\\"a\\\": 2}\\n",
    ] {
        let program = format!(
            "{PY_CLIENT}\ns3 = S3({{\"p/1.jsonl\": b\"{body}\"}})\nprefix, batch_id = \"p/\", 1\n{}\nassert records == [{{\"a\": 1}}, {{\"a\": 2}}], records\n",
            fix("obs.smallreads[records]/python")
        );
        if let Some(r) = python("boto3's lines read", &program) {
            assert!(r.ok, "{}\n{program}", r.said);
        }
    }
    // and what the write's fix writes, the read's fix reads
    let program = format!(
        "{PY_CLIENT}\ns3 = S3()\nprefix, batch_id, batch = \"p/\", 1, [{{\"a\": 1}}, {{\"a\": 2}}]\n{}\n{}\nassert records == batch, records\n",
        fix("obs.small[records]/python"),
        fix("obs.smallreads[records]/python")
    );
    if let Some(r) = python("boto3's lines written and read", &program) {
        assert!(r.ok, "{}\n{program}", r.said);
    }
}

/// The AWS SDK for JavaScript's client as the fixes below use it: `send`
/// of a command, each kept in `sent`.
const JS_CLIENT: &str = r#"
import assert from "node:assert/strict";
const command = (name) => class { constructor(input) { this.name = name; this.input = input; } };
const [GetObjectCommand, PutObjectCommand, UploadPartCommand, CompleteMultipartUploadCommand] =
  ["GET", "PUT", "PART", "COMPLETE"].map(command);
const text = (body) => typeof body === "string" ? body : new TextDecoder().decode(body);
const s3 = {
  objects: new Map(), sent: [], parts: new Map(), completed: null, open: 0,
  async send({ name, input }) {
    this.sent.push([name, input.Key, input.Range]);
    await new Promise((done) => setTimeout(done, name === "PART" ? 5 - (input.PartNumber % 5) : 0));
    if (name === "GET") {
      assert.ok(this.objects.has(input.Key), `no ${input.Key}`);
      const data = this.objects.get(input.Key);
      this.open++; // (a body not read keeps its connection)
      const read = (as) => async () => { this.open--; return as(data); };
      return { ETag: '"e"', Body: { transformToByteArray: read((d) => new TextEncoder().encode(text(d))), transformToString: read(text) } };
    }
    if (name === "PUT") this.objects.set(input.Key, input.Body);
    if (name === "PART") {
      assert.ok(Number.isInteger(input.PartNumber) && input.PartNumber >= 1, `part number ${input.PartNumber}`);
      assert.ok(input.Body !== undefined, `part ${input.PartNumber} has no body`);
      this.parts.set(input.PartNumber, input.Body);
      return { ETag: `"etag-of-${input.Body}"` };
    }
    if (name === "COMPLETE") this.completed = input.MultipartUpload;
    return {};
  },
};
// (p-limit's: at most `n` at once)
const pLimit = (n) => {
  let running = 0;
  const waiting = [];
  return async (f) => {
    while (running >= n) await new Promise((go) => waiting.push(go));
    running++;
    try { return await f(); } finally { running--; waiting.shift()?.(); }
  };
};
const b = "demo", key = "big", uploadId = "u";
"#;

/// `program` (a module) run by node, or none where there is none (said).
fn node(check: &str, program: &str) -> Option<Ran> {
    if !have("node", "--version") {
        skipped(check, "no node");
        return None;
    }
    let t = tempfile::tempdir().unwrap();
    let file = t.path().join("fix.mjs");
    std::fs::write(&file, program).unwrap();
    Some(ran(Command::new("node").arg(&file)))
}

#[test]
fn js_parts_are_numbered_from_one_and_completed_with_their_etags_in_order() {
    let program = format!(
        r#"{JS_CLIENT}
const chunks = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"];
{}
assert.deepEqual([...s3.parts].sort((x, y) => x[0] - y[0]), chunks.map((c, i) => [i + 1, c]));
assert.deepEqual(s3.completed, {{ Parts: chunks.map((c, i) => ({{ PartNumber: i + 1, ETag: `"etag-of-${{c}}"` }})) }});
"#,
        fix("obs.serial[PART]/js")
    );
    if let Some(r) = node("JavaScript's parts at once", &program) {
        assert!(r.ok, "{}\n{program}", r.said);
    }
}

#[test]
fn js_reads_kept_are_read_once_and_again_after_the_programs_own_write() {
    for (id, read) in [
        ("obs.hot", "read(\"conf\")"),
        ("obs.rangeagain", "readRange(\"conf\", \"bytes=0-9\")"),
    ] {
        let got = format!("text(await {read})");
        let kept = format!(
            "{JS_CLIENT}\ns3.objects.set(\"conf\", \"one\");\n{}\nassert.equal({got}, \"one\");\nassert.equal({got}, \"one\");\nassert.equal(s3.sent.length, 1);\n",
            fix(&format!("{id}/js"))
        );
        let rewritten = format!(
            "{JS_CLIENT}\ns3.objects.set(\"conf\", \"one\").set(\"other\", \"x\");\n{}\nassert.equal({got}, \"one\");\nassert.equal({got}, \"one\");\nassert.equal(s3.sent.length, 1);\nawait write(\"other\", \"y\");\nassert.equal({got}, \"one\");\nassert.equal(s3.sent.length, 2);\nawait write(\"conf\", \"two\");\nassert.equal({got}, \"two\", \"a stale read after the program's own write\");\nassert.equal({got}, \"two\");\nassert.equal(s3.sent.length, 4);\n",
            fix(&format!("{id}[rewritten]/js"))
        );
        for program in [kept, rewritten] {
            if let Some(r) = node("JavaScript's reads kept", &program) {
                assert!(r.ok, "{id}: {}\n{program}", r.said);
            }
        }
    }
}

#[test]
fn js_lines_are_read_whatever_the_file_ends_with_and_each_body_at_once_is_read() {
    for body in ["{\"a\": 1}\\n{\"a\": 2}", "{\"a\": 1}\\n{\"a\": 2}\\n"] {
        let program = format!(
            "{JS_CLIENT}\ns3.objects.set(\"p/1.jsonl\", '{body}');\nconst prefix = \"p/\", batchId = 1;\n{}\nassert.deepEqual(records, [{{ a: 1 }}, {{ a: 2 }}]);\n",
            fix("obs.smallreads[records]/js")
        );
        if let Some(r) = node("JavaScript's lines read", &program) {
            assert!(r.ok, "{}\n{program}", r.said);
        }
    }
    // twenty GETs at once: every body read (one left unread keeps its
    // connection, and the SDK has 50)
    let program = format!(
        "{JS_CLIENT}\nconst keys = Array.from({{ length: 20 }}, (_, i) => `k${{i}}`);\nfor (const k of keys) s3.objects.set(k, k);\n{}\nassert.deepEqual(results.map(text), keys);\nassert.equal(s3.open, 0, \"bodies left unread\");\n",
        fix("obs.serial[GET]/js")
    );
    if let Some(r) = node("JavaScript's GETs at once", &program) {
        assert!(r.ok, "{}\n{program}", r.said);
    }
}

/// The Go fixes whose meaning matters, run on the SDK's own client, its
/// transport answered here: what the client sent is what is looked at.
const GO_MAIN: &str = r#"
type sent struct{ method, path, body string }

type store struct {
	mu      sync.Mutex
	sent    []sent
	objects map[string]string
}

func (s *store) RoundTrip(r *http.Request) (*http.Response, error) {
	var body []byte
	if r.Body != nil {
		body, _ = io.ReadAll(r.Body)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.sent = append(s.sent, sent{r.Method, r.URL.Path + "?" + r.URL.RawQuery, string(body)})
	h := http.Header{"Etag": {`"etag-of-` + string(body) + `"`}}
	answer := ""
	switch {
	case r.Method == "GET":
		answer = s.objects[r.URL.Path]
	case r.Method == "PUT" && !r.URL.Query().Has("partNumber"):
		s.objects[r.URL.Path] = string(body)
	case r.Method == "POST":
		answer = `<CompleteMultipartUploadResult><ETag>"e"</ETag></CompleteMultipartUploadResult>`
	}
	return &http.Response{StatusCode: 200, Header: h, Body: io.NopCloser(strings.NewReader(answer)), ContentLength: int64(len(answer)), Request: r}, nil
}

func client(s *store) *s3.Client {
	return s3.New(s3.Options{
		Region:       "us-east-1",
		BaseEndpoint: aws.String("http://store.test"),
		UsePathStyle: true,
		HTTPClient:   &http.Client{Transport: s},
		Credentials:  aws.CredentialsProviderFunc(func(context.Context) (aws.Credentials, error) { return aws.Credentials{AccessKeyID: "a", SecretAccessKey: "s"}, nil }),
		// (bodies as they are given: no trailer to tell from a part's bytes)
		RequestChecksumCalculation: aws.RequestChecksumCalculationWhenRequired,
	})
}

func must(ok bool, what string, a ...any) {
	if !ok {
		fmt.Printf("FAILED: "+what+"\n", a...)
		os.Exit(1)
	}
}
"#;

fn go_run(check: &str, imports: &str, package_level: &str, main: &str) {
    let Some(t) = go_module(check) else {
        return;
    };
    let dir = t.path().join("run");
    std::fs::create_dir(&dir).unwrap();
    let program = format!(
        "package main\n\nimport (\n\t\"context\"\n\t\"fmt\"\n\t\"io\"\n\t\"net/http\"\n\t\"os\"\n\t\"strings\"\n\t\"sync\"\n\n\t\"github.com/aws/aws-sdk-go-v2/aws\"\n\t\"github.com/aws/aws-sdk-go-v2/service/s3\"\n{imports})\n\nvar _ = strings.HasPrefix\n{GO_MAIN}\n{package_level}\n\nfunc main() {{\n\tctx := context.Background()\n{main}\n\tfmt.Println(\"passed\")\n}}\n"
    );
    std::fs::write(dir.join("main.go"), &program).unwrap();
    let r = ran(go(t.path()).args(["run", "./run"]));
    assert!(
        r.ok && r.said.contains("passed"),
        "{check}: {}\n{}",
        r.said,
        numbered(&program)
    );
    say(&format!("ran {check} on aws-sdk-go-v2"));
}

#[test]
fn go_reads_kept_are_read_once_and_again_after_the_programs_own_write() {
    for (id, read) in [
        ("obs.hot", "read(ctx, \"conf\")"),
        ("obs.rangeagain", "readRange(ctx, \"conf\", \"bytes=0-9\")"),
    ] {
        let state = "var st = &store{objects: map[string]string{\"/demo/conf\": \"one\", \"/demo/other\": \"x\"}}\nvar s3c = client(st)\nvar b = \"demo\"\n";
        let twice = format!(
            "\tfor range 2 {{\n\t\tgot, err := {read}\n\t\tmust(err == nil && string(got) == \"one\", \"read %q, %v\", got, err)\n\t}}\n\tmust(len(st.sent) == 1, \"%d requests for two reads: the copy was not kept\", len(st.sent))\n"
        );
        go_run(
            &format!("Go's {id} kept"),
            "",
            &format!("{state}\n{}", fix(&format!("{id}/go"))),
            &twice,
        );
        let after = format!(
            "{twice}\tmust(write(ctx, \"other\", []byte(\"y\")) == nil, \"write\")\n\tgot, err := {read}\n\tmust(err == nil && string(got) == \"one\" && len(st.sent) == 2, \"another key's write dropped the copy: %d requests\", len(st.sent))\n\tmust(write(ctx, \"conf\", []byte(\"two\")) == nil, \"write\")\n\tgot, err = {read}\n\tmust(err == nil && string(got) == \"two\", \"a stale read after the program's own write: %q, %v\", got, err)\n\tgot, err = {read}\n\tmust(string(got) == \"two\" && len(st.sent) == 4, \"%d requests\", len(st.sent))\n"
        );
        go_run(
            &format!("Go's {id} kept until written"),
            "\t\"bytes\"\n",
            &format!("{state}\n{}", fix(&format!("{id}[rewritten]/go"))),
            &after,
        );
    }
}

#[test]
fn go_parts_are_numbered_from_one_and_completed_with_their_etags_in_order() {
    let state = "var st = &store{objects: map[string]string{}}\nvar s3c = client(st)\nvar b, key, uploadID = \"demo\", \"big\", \"u\"\nvar chunks = [][]byte{[]byte(\"a\"), []byte(\"b\"), []byte(\"c\"), []byte(\"d\"), []byte(\"e\"), []byte(\"f\"), []byte(\"g\"), []byte(\"h\"), []byte(\"i\"), []byte(\"j\"), []byte(\"k\")}\n";
    let upload = format!(
        "func upload(ctx context.Context) error {{\n{}\nreturn err\n}}\n",
        fix("obs.serial[PART]/go")
    );
    let main = r#"	err := upload(ctx)
	must(err == nil, "the upload: %v (a completion sent on the group's context is cancelled)", err)
	parts, completion := map[string]string{}, ""
	for _, r := range st.sent {
		if r.method == "PUT" {
			q := r.path[strings.Index(r.path, "?")+1:]
			for _, kv := range strings.Split(q, "&") {
				if n, ok := strings.CutPrefix(kv, "partNumber="); ok {
					parts[n] = r.body
				}
			}
		} else {
			completion = r.body
		}
	}
	want := ""
	for i, c := range chunks {
		n := fmt.Sprint(i + 1)
		must(parts[n] == string(c), "part %s is %q, not %q", n, parts[n], c)
		want += fmt.Sprintf(`<Part><ETag>&#34;etag-of-%s&#34;</ETag><PartNumber>%s</PartNumber></Part>`, c, n)
	}
	must(len(parts) == len(chunks), "%d parts", len(parts))
	must(strings.Contains(completion, want), "the completion names %s, not %s", completion, want)
"#;
    go_run(
        "Go's parts at once",
        "\t\"bytes\"\n\n\t\"github.com/aws/aws-sdk-go-v2/service/s3/types\"\n\t\"golang.org/x/sync/errgroup\"\n",
        &format!("{state}\n{upload}"),
        main,
    );
}
