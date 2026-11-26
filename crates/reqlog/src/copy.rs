//! "Copy as aws cli" and "copy as curl".
//!
//! Both renderings come from a redacted record, so they never carry a
//! secret. The curl command has no signature: it is marked as needing one,
//! with curl's own `--aws-sigv4` option shown as the way to add it.
//!
//! A record's values come from a run, and a run from anywhere: nothing in
//! them may become a flag, a second command or text that reads as something
//! else once pasted. So:
//!
//! - the command is built as an argv first, each value of the record in one
//!   argument of its own that the tool cannot read as an option (`aws`:
//!   `--name=value`; curl: after `-H`, or after `--`), then written for one
//!   [`Shell`] by that shell's quoting;
//! - a URL is `http://` or `https://` or the request is not copied; curl's
//!   URL globbing is off (`-g`);
//! - a value `aws` would read a file or fetch a URL for (`file://`,
//!   `fileb://`, `http://`, `https://`) goes in `--cli-input-json`, which is
//!   taken as it is;
//! - control characters and BiDi controls are left out of every value, and
//!   a comment says so;
//! - a comment never holds a value of the record.

use crate::Record;

/// The shell a copied command is written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// POSIX `sh` (bash, zsh, dash): single quotes, with a backslash
    /// outside them, so fish (where one escapes inside them) reads the same.
    Posix,
    /// PowerShell: single quotes doubled, `curl.exe` and `aws.exe` (`curl`
    /// is an alias there, and a `.cmd` would hand the arguments to cmd.exe
    /// to read again).
    PowerShell,
    /// cmd.exe's prompt as it starts (not a batch file, where `%` is read
    /// otherwise, nor with delayed expansion on, where `!` is): the C
    /// runtime's quoting, every character cmd reads escaped with `^`.
    Cmd,
}

impl Shell {
    /// This platform's: PowerShell on Windows, POSIX sh elsewhere.
    pub const fn platform() -> Shell {
        if cfg!(windows) {
            Shell::PowerShell
        } else {
            Shell::Posix
        }
    }

    /// A shell by name (`sh`, `powershell`, `cmd`); the platform's for
    /// `platform` or no name; none for a name that is no shell's.
    pub fn named(name: &str) -> Option<Shell> {
        match name.to_ascii_lowercase().as_str() {
            "" | "platform" => Some(Shell::platform()),
            "sh" | "posix" | "bash" | "zsh" => Some(Shell::Posix),
            "powershell" | "pwsh" => Some(Shell::PowerShell),
            "cmd" | "cmd.exe" => Some(Shell::Cmd),
            _ => None,
        }
    }

    /// What starts a comment's line.
    fn comment(self) -> &'static str {
        match self {
            Shell::Cmd => "rem",
            _ => "#",
        }
    }
}

/// Whether `c` is left out of a copied value: C0 and C1 controls (a
/// newline ends a command, an escape recolours or moves what a terminal
/// shows), the line separators, and the BiDi controls (the text would not
/// read in the order it runs).
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{2028}'
                | '\u{2029}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{061c}'
        )
}

/// One argument: sonde's own word, or one made from the record.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Arg {
    Own(&'static str),
    Value(String),
}

impl Arg {
    fn as_str(&self) -> &str {
        match self {
            Arg::Own(s) => s,
            Arg::Value(s) => s,
        }
    }
}

/// A command: comment lines of fixed text, then the argv, in the lines it
/// is written on (curl: a header a line).
#[derive(Default)]
struct Command {
    notes: Vec<String>,
    lines: Vec<Vec<Arg>>,
    /// A value had characters left out ([`hidden`]).
    inexact: bool,
}

impl Command {
    /// A value of the record without what is [`hidden`].
    fn clean(&mut self, s: &str) -> String {
        if s.chars().any(hidden) {
            self.inexact = true;
            s.chars().filter(|c| !hidden(*c)).collect()
        } else {
            s.to_string()
        }
    }

    #[cfg(test)]
    fn argv(&self) -> Vec<String> {
        self.lines
            .iter()
            .flatten()
            .map(|a| a.as_str().to_string())
            .collect()
    }

    fn render(mut self, shell: Shell) -> String {
        if self.inexact {
            self.notes
                .push("NOT EXACT: control characters in the request's values were left out".into());
        }
        let mut out = String::new();
        for n in &self.notes {
            out.push_str(&format!("{} {n}\n", shell.comment()));
        }
        let quote = |a: &Arg| match (a, shell) {
            (Arg::Own(s), _) => s.to_string(),
            (Arg::Value(s), Shell::Posix) => posix_quote(s),
            (Arg::Value(s), Shell::PowerShell) => powershell_quote(s),
            (Arg::Value(s), Shell::Cmd) => cmd_quote(s),
        };
        let lines: Vec<String> = self
            .lines
            .iter()
            .map(|l| l.iter().map(quote).collect::<Vec<_>>().join(" "))
            .collect();
        match shell {
            Shell::Posix => out.push_str(&lines.join(" \\\n  ")),
            // one line: a prompt reads a paste a line at a time
            Shell::Cmd => out.push_str(&lines.join(" ")),
            Shell::PowerShell => {
                let body = lines.join(" `\n  ");
                let guarded = self
                    .lines
                    .iter()
                    .flatten()
                    .any(|a| legacy_powershell_breaks(a.as_str()));
                if guarded {
                    out.push_str(&format!(
                        "{POWERSHELL_GUARD}\n{body}\n{POWERSHELL_GUARD_END}"
                    ));
                } else {
                    out.push_str(&body);
                }
            }
        }
        out
    }
}

/// The most cmd.exe reads in one line, in UTF-16 units: a longer paste is
/// cut, and what runs is another command.
const CMD_LINE: usize = 8191;

impl Command {
    /// [`Command::render`], or why not when cmd.exe could not read it whole.
    fn render_or_refuse(self, shell: Shell) -> String {
        let out = self.render(shell);
        let long = shell == Shell::Cmd && out.lines().any(|l| l.encode_utf16().count() >= CMD_LINE);
        if long {
            refused(
                shell,
                "it is longer than cmd.exe reads in one line (8191 characters); copy it for PowerShell",
            )
        } else {
            out
        }
    }
}

/// A request that is not copied: why, as a comment.
fn refused(shell: Shell, why: &str) -> String {
    format!("{} sonde: not copied: {why}", shell.comment())
}

fn posix_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@,+%".contains(c))
    {
        s.to_string()
    } else {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('\'');
        for c in s.chars() {
            match c {
                // out of the quotes, escaped, and in again
                '\'' => out.push_str("'\\''"),
                '\\' => out.push_str("'\\\\'"),
                c => out.push(c),
            }
        }
        out.push('\'');
        out
    }
}

/// The characters PowerShell reads as a single quote: each ends a
/// single-quoted string unless doubled.
const POWERSHELL_QUOTES: [char; 5] = ['\'', '\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}'];

/// Always quoted: a bare word starting with `-` or holding `.`, `,`, `@`
/// or `;` is PowerShell's to read.
fn powershell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if POWERSHELL_QUOTES.contains(&c) {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// Whether PowerShell before 7.3 (and a newer one told to pass arguments
/// the `Legacy` way) would hand `arg` to a program as something else: it
/// does not escape a `"`, nor (Windows PowerShell) a `\` before the quote
/// it closes an argument with, and drops an empty one. No way of writing
/// such an argument is right for both, so a command holding one runs only
/// where it is passed whole ([`POWERSHELL_GUARD`]).
fn legacy_powershell_breaks(arg: &str) -> bool {
    arg.is_empty() || arg.contains('"') || arg.ends_with('\\')
}

const POWERSHELL_GUARD: &str = "if ($PSNativeCommandArgumentPassing -in 'Standard','Windows') {";
const POWERSHELL_GUARD_END: &str = "} else { Write-Error 'sonde: an argument of this command holds a double quote or ends with a backslash, which PowerShell before 7.3 hands on as something else: run it in PowerShell 7.3 or newer' }";

/// An argument as the C runtime (and `CommandLineToArgvW`) reads one: in
/// double quotes, a `"` behind a backslash, the backslashes before a quote
/// doubled.
fn windows_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let mut backslashes = 0;
    for c in s.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            c => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// [`windows_quote`], then every character cmd.exe reads (the quotes too,
/// so cmd is never inside a quoted stretch, where `^` stops escaping)
/// behind a `^`. A `^%` keeps `%NAME%` from being a variable at the prompt:
/// the name it would look up ends in `^`.
fn cmd_quote(s: &str) -> String {
    let mut out = String::new();
    for c in windows_quote(s).chars() {
        if "()%!^\"<>&|".contains(c) {
            out.push('^');
        }
        out.push(c);
    }
    out
}

fn kebab(op: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = op.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let prev_lower =
                i > 0 && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit());
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            let prev_upper = i > 0 && chars[i - 1].is_ascii_uppercase();
            if i > 0 && (prev_lower || (prev_upper && next_lower)) {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(*c);
        }
    }
    out
}

/// A record's URL when it is one a command may be given: `http` or `https`
/// with a host, written again by the parser (so ASCII: anything else is
/// percent-encoded, a host in punycode), without a password.
fn web_url(url: &str) -> Option<url::Url> {
    let mut u = url::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none_or(str::is_empty) {
        return None;
    }
    let _ = u.set_password(None);
    Some(u)
}

/// The endpoint part of a URL (`scheme://host[:port]`).
fn endpoint_url(url: &str) -> Option<String> {
    let parsed = web_url(url)?;
    let mut out = format!("{}://{}", parsed.scheme(), parsed.host_str()?);
    if let Some(p) = parsed.port() {
        out.push_str(&format!(":{p}"));
    }
    Some(out)
}

fn query<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.query()
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn req_header<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.request_headers()
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

/// A region as one is written (`us-east-1`), or none.
fn region_of(r: &Record) -> Option<&str> {
    r.rare().region.as_deref().filter(|reg| {
        !reg.is_empty()
            && reg.len() <= 64
            && !reg.starts_with('-')
            && reg
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}

/// Whether `aws` would take `value` for a file to read or a URL to fetch
/// instead of the value (its `file://` and `fileb://`; version 1 fetches
/// `http://` and `https://` too).
fn aws_reads_elsewhere(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    ["file://", "fileb://", "http://", "https://"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// `--version-id` as `--cli-input-json` names it: `VersionId`.
fn member(flag: &str) -> String {
    flag.trim_start_matches('-')
        .split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// A record's query parameters and the `aws s3api` options they are.
const QUERY_OPTIONS: &[(&str, &str)] = &[
    ("prefix", "--prefix"),
    ("delimiter", "--delimiter"),
    ("start-after", "--start-after"),
    ("continuation-token", "--continuation-token"),
    ("max-keys", "--max-keys"),
    ("versionId", "--version-id"),
    ("uploadId", "--upload-id"),
    ("partNumber", "--part-number"),
    ("key-marker", "--key-marker"),
    ("version-id-marker", "--version-id-marker"),
    ("upload-id-marker", "--upload-id-marker"),
    ("part-number-marker", "--part-number-marker"),
    ("max-uploads", "--max-uploads"),
    ("max-parts", "--max-parts"),
];

/// A record's request headers and the `aws s3api` options they are.
const HEADER_OPTIONS: &[(&str, &str)] = &[
    ("range", "--range"),
    ("if-match", "--if-match"),
    ("if-none-match", "--if-none-match"),
    ("x-amz-checksum-mode", "--checksum-mode"),
    ("x-amz-copy-source", "--copy-source"),
    ("x-amz-storage-class", "--storage-class"),
];

/// The options `--cli-input-json` takes as numbers.
const NUMBERS: &[&str] = &[
    "--max-keys",
    "--part-number",
    "--max-uploads",
    "--max-parts",
    "--part-number-marker",
];

/// An `aws s3api` command doing what the record did, written for `shell`.
pub fn as_aws_cli(r: &Record, shell: Shell) -> String {
    match aws_cli(r, shell) {
        Ok(c) => c.render_or_refuse(shell),
        Err(why) => refused(shell, why),
    }
}

fn aws_cli(r: &Record, shell: Shell) -> Result<Command, &'static str> {
    let mut c = Command::default();
    let aws = Arg::Own(if shell == Shell::Posix {
        "aws"
    } else {
        "aws.exe"
    });
    let region = region_of(r).map(|reg| Arg::Value(format!("--region={reg}")));
    if r.kind == "GetCallerIdentity" {
        let mut line = vec![aws, Arg::Own("sts"), Arg::Own("get-caller-identity")];
        line.extend(region);
        c.lines.push(line);
        return Ok(c);
    }
    let op = match r.kind.as_str() {
        "ListObjectsV2" => "list-objects-v2".to_string(),
        other => kebab(other),
    };
    if op.is_empty()
        || op.starts_with('-')
        || !op
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("its operation is not one aws s3api has");
    }
    // each parameter: its option and the record's value
    let mut params: Vec<(&str, String)> = Vec::new();
    if let Some(b) = &r.bucket {
        params.push(("--bucket", c.clean(b)));
    }
    if let Some(k) = &r.key {
        params.push(("--key", c.clean(k)));
    }
    for (q, flag) in QUERY_OPTIONS {
        // a record's query is decoded already: a `%` in it is the key's own
        if let Some(v) = query(r, q) {
            params.push((flag, c.clean(v)));
        }
    }
    for (h, flag) in HEADER_OPTIONS {
        if let Some(v) = req_header(r, h) {
            params.push((flag, c.clean(v)));
        }
    }
    let mut line = vec![aws, Arg::Own("s3api"), Arg::Value(op.clone())];
    if params.iter().any(|(_, v)| aws_reads_elsewhere(v)) {
        // as JSON no value is a file's name or a URL to aws
        let doc: serde_json::Map<String, serde_json::Value> = params
            .iter()
            .map(|(flag, v)| {
                let value = match v.parse::<u64>() {
                    Ok(n) if NUMBERS.contains(flag) => n.into(),
                    _ => v.as_str().into(),
                };
                (member(flag), value)
            })
            .collect();
        line.push(Arg::Value(format!(
            "--cli-input-json={}",
            serde_json::Value::Object(doc)
        )));
    } else {
        // `--name=value`, one argument: a value starting with `-` is not
        // the next option
        line.extend(
            params
                .iter()
                .map(|(flag, v)| Arg::Value(format!("{flag}={v}"))),
        );
    }
    if op == "get-object" {
        line.push(Arg::Own(if shell == Shell::Posix {
            "/dev/null"
        } else {
            "NUL"
        }));
    }
    if let Some(e) = endpoint_url(&r.rare().url)
        && !e.ends_with(".amazonaws.com")
    {
        line.push(Arg::Value(format!("--endpoint-url={e}")));
    }
    line.extend(region);
    c.lines.push(line);
    if matches!(r.method.as_str(), "PUT" | "POST") && r.request_bytes.get().unwrap_or(0) > 0 {
        c.notes
            .push("request body not reproduced: add --body FILE".into());
    }
    Ok(c)
}

/// Whether `name` is a header's name (RFC 9110's token): what curl's `-H`
/// takes for one, never its `@file`.
fn header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

/// A curl command with the record's method, URL and headers, minus every
/// signing header, written for `shell`. It needs a signature: curl adds
/// one with `--aws-sigv4`.
pub fn as_curl(r: &Record, shell: Shell) -> String {
    match curl(r, shell) {
        Ok(c) => c.render_or_refuse(shell),
        Err(why) => refused(shell, why),
    }
}

fn curl(r: &Record, shell: Shell) -> Result<Command, &'static str> {
    let skip = [
        "authorization",
        "x-amz-date",
        "x-amz-security-token",
        "x-amz-content-sha256",
        "host",
        "content-length",
        "user-agent",
        "amz-sdk-invocation-id",
        "amz-sdk-request",
    ];
    let mut c = Command::default();
    let Some(mut url) = web_url(&r.rare().url) else {
        return Err("its URL is not an http or https one");
    };
    if r.method.is_empty() || !r.method.chars().all(|c| c.is_ascii_uppercase()) {
        return Err("its method is not an HTTP method's name");
    }
    if url
        .query_pairs()
        .any(|(k, _)| k.eq_ignore_ascii_case("X-Amz-Signature"))
    {
        // a presigned URL: drop the presign query, curl signs anew
        let kept: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(k, _)| !k.to_ascii_lowercase().starts_with("x-amz-"))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        url.set_query(None);
        if !kept.is_empty() {
            url.query_pairs_mut().extend_pairs(kept);
        }
    }
    let user = match shell {
        Shell::Posix => "\"$AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY\"",
        Shell::PowerShell => "\"${env:AWS_ACCESS_KEY_ID}:${env:AWS_SECRET_ACCESS_KEY}\"",
        Shell::Cmd => "\"%AWS_ACCESS_KEY_ID%:%AWS_SECRET_ACCESS_KEY%\"",
    };
    c.notes = vec![
        "UNSIGNED: sonde never exports a signature. Sign it with curl itself:".to_string(),
        format!(
            "  --aws-sigv4 aws:amz:{}:s3 --user {user}",
            region_of(r).unwrap_or("us-east-1")
        ),
    ];
    let program = Arg::Own(if shell == Shell::Posix {
        "curl"
    } else {
        "curl.exe"
    });
    // -g: a `[1-9]` or `{a,b}` in the URL is the key's, not a range of requests
    c.lines.push(if r.method == "HEAD" {
        vec![program, Arg::Own("-sS"), Arg::Own("-g"), Arg::Own("-I")]
    } else {
        vec![
            program,
            Arg::Own("-sS"),
            Arg::Own("-g"),
            Arg::Own("-i"),
            Arg::Own("-X"),
            Arg::Value(r.method.as_str().to_string()),
        ]
    });
    let mut unnamed = false;
    for h in r.request_headers() {
        if skip.contains(&h.name.to_ascii_lowercase().as_str()) {
            continue;
        }
        if !header_name(&h.name) {
            unnamed = true;
            continue;
        }
        let value = c.clean(&h.value);
        let value = value.trim();
        c.lines.push(vec![
            Arg::Own("-H"),
            // (`name;` is how curl sends a header with no value)
            Arg::Value(if value.is_empty() {
                format!("{};", h.name)
            } else {
                format!("{}: {value}", h.name)
            }),
        ]);
    }
    if unnamed {
        c.notes
            .push("NOT EXACT: a header whose name is not a header's name was left out".into());
    }
    // after `--` nothing is an option
    c.lines
        .push(vec![Arg::Value("--".into()), Arg::Value(url.into())]);
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Header;

    fn rec() -> Record {
        Record {
            kind: "ListObjectsV2".into(),
            method: "GET".into(),
            bucket: Some("bucket".into()),
            rare: crate::Rare {
                url: "http://127.0.0.1:9000/bucket?list-type=2&prefix=logs%2F&delimiter=%2F".into(),
                region: Some("us-east-1".into()),
                ..Default::default()
            }
            .into(),
            lists: crate::Lists {
                query: vec![
                    ("list-type".into(), "2".into()),
                    // decoded, as s3raw and the proxy keep a record's query
                    ("prefix".into(), "logs/".into()),
                    ("delimiter".into(), "/".into()),
                ],
                request_headers: vec![
                    Header::new(
                        "Authorization",
                        "AWS4-HMAC-SHA256 Credential=AKIAX/s, SignedHeaders=host, Signature=zzz",
                    ),
                    Header::new("x-amz-date", "20240101T000000Z"),
                    Header::new("x-amz-security-token", "TOKEN"),
                    Header::new("x-amz-request-payer", "requester"),
                ],
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }
        .redacted()
    }

    #[test]
    fn kebab_case() {
        assert_eq!(kebab("HeadObject"), "head-object");
        assert_eq!(
            kebab("GetBucketLifecycleConfiguration"),
            "get-bucket-lifecycle-configuration"
        );
        assert_eq!(kebab("ListMultipartUploads"), "list-multipart-uploads");
        assert_eq!(kebab("UploadPartCopy"), "upload-part-copy");
    }

    #[test]
    fn aws_cli_for_each_shell() {
        let cmd = as_aws_cli(&rec(), Shell::Posix);
        assert_eq!(
            cmd,
            "aws s3api list-objects-v2 --bucket=bucket --prefix=logs/ --delimiter=/ --endpoint-url=http://127.0.0.1:9000 --region=us-east-1"
        );
        assert_eq!(
            as_aws_cli(&rec(), Shell::PowerShell),
            "aws.exe s3api 'list-objects-v2' '--bucket=bucket' '--prefix=logs/' '--delimiter=/' '--endpoint-url=http://127.0.0.1:9000' '--region=us-east-1'"
        );
        assert_eq!(
            as_aws_cli(&rec(), Shell::Cmd),
            "aws.exe s3api ^\"list-objects-v2^\" ^\"--bucket=bucket^\" ^\"--prefix=logs/^\" ^\"--delimiter=/^\" ^\"--endpoint-url=http://127.0.0.1:9000^\" ^\"--region=us-east-1^\""
        );
    }

    #[test]
    fn a_percent_sign_in_a_prefix_is_copied_as_it_is() {
        let mut r = rec();
        r.lists_mut().query = vec![("prefix".into(), "sale-50%25off/a%2Fb".into())];
        let cmd = as_aws_cli(&r, Shell::Posix);
        assert!(cmd.contains("--prefix=sale-50%25off/a%2Fb "), "{cmd}");
    }

    #[test]
    fn curl_has_no_secrets() {
        let cmd = as_curl(&rec(), Shell::Posix);
        assert!(cmd.contains("UNSIGNED"));
        assert!(cmd.contains("--aws-sigv4"));
        assert!(!cmd.contains("zzz") && !cmd.contains("TOKEN"));
        assert!(!cmd.to_lowercase().contains("authorization:"));
        assert!(cmd.contains("x-amz-request-payer: requester"));
        assert!(cmd.contains("curl -sS -g -i -X GET"));
    }

    #[test]
    fn curl_strips_presign() {
        let mut r = rec();
        r.rare_mut().url = "https://h/b/k?versionId=1&X-Amz-Signature=abc&X-Amz-Date=x".into();
        let cmd = as_curl(&r.redacted(), Shell::Posix);
        assert!(cmd.contains("https://h/b/k?versionId=1"), "{cmd}");
        assert!(!cmd.contains("X-Amz-Signature"));
    }

    #[test]
    fn a_shell_by_name() {
        assert_eq!(Shell::named(""), Some(Shell::platform()));
        assert_eq!(Shell::named("platform"), Some(Shell::platform()));
        assert_eq!(Shell::named("sh"), Some(Shell::Posix));
        assert_eq!(Shell::named("PowerShell"), Some(Shell::PowerShell));
        assert_eq!(Shell::named("cmd"), Some(Shell::Cmd));
        assert_eq!(Shell::named("fish?"), None);
    }

    // --- each shell, read back as that shell reads it ---------------------------

    /// What POSIX sh makes of `text`: each command's argv. Anything the
    /// shell would act on outside quotes (an expansion, a second command,
    /// a redirection, a glob) fails the test.
    fn sh(text: &str) -> Vec<Vec<String>> {
        let mut commands = Vec::new();
        let mut argv: Vec<String> = Vec::new();
        let mut word: Option<String> = None;
        let mut it = text.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                '#' if word.is_none() => {
                    while it.peek().is_some_and(|c| *c != '\n') {
                        it.next();
                    }
                }
                '\n' => {
                    argv.extend(word.take());
                    if !argv.is_empty() {
                        commands.push(std::mem::take(&mut argv));
                    }
                }
                ' ' | '\t' => argv.extend(word.take()),
                '\\' => match it.next() {
                    Some('\n') => {}
                    Some(c) => word.get_or_insert_default().push(c),
                    None => panic!("a backslash ends {text:?}"),
                },
                '\'' => {
                    let w = word.get_or_insert_default();
                    loop {
                        match it.next() {
                            Some('\'') => break,
                            Some(c) => w.push(c),
                            None => panic!("an open quote in {text:?}"),
                        }
                    }
                }
                c if c.is_ascii_alphanumeric() || "-_./:=@,+%".contains(c) => {
                    word.get_or_insert_default().push(c)
                }
                c => panic!("{c:?} is the shell's to read in {text:?}"),
            }
        }
        argv.extend(word.take());
        if !argv.is_empty() {
            commands.push(argv);
        }
        commands
    }

    /// What fish makes of `text`: as [`sh`], but inside single quotes a
    /// backslash escapes a quote or another backslash.
    fn fish(text: &str) -> Vec<Vec<String>> {
        let mut commands = Vec::new();
        let mut argv: Vec<String> = Vec::new();
        let mut word: Option<String> = None;
        let mut it = text.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                '#' if word.is_none() => {
                    while it.peek().is_some_and(|c| *c != '\n') {
                        it.next();
                    }
                }
                '\n' => {
                    argv.extend(word.take());
                    if !argv.is_empty() {
                        commands.push(std::mem::take(&mut argv));
                    }
                }
                ' ' | '\t' => argv.extend(word.take()),
                '\\' => match it.next() {
                    Some('\n') => {}
                    Some(c @ ('\'' | '\\')) => word.get_or_insert_default().push(c),
                    other => panic!("a backslash before {other:?} in {text:?}"),
                },
                '\'' => {
                    let w = word.get_or_insert_default();
                    loop {
                        match it.next() {
                            Some('\'') => break,
                            Some('\\') => match it.peek().copied() {
                                Some(e @ ('\'' | '\\')) => {
                                    w.push(e);
                                    it.next();
                                }
                                _ => w.push('\\'),
                            },
                            Some(c) => w.push(c),
                            None => panic!("an open quote in {text:?}"),
                        }
                    }
                }
                c if c.is_ascii_alphanumeric() || "-_./:=@,+%".contains(c) => {
                    word.get_or_insert_default().push(c)
                }
                c => panic!("{c:?} is fish's to read in {text:?}"),
            }
        }
        argv.extend(word.take());
        if !argv.is_empty() {
            commands.push(argv);
        }
        commands
    }

    /// What PowerShell makes of `text` (without its guard): each command's
    /// arguments as strings, before they are handed to a program.
    fn powershell(text: &str) -> Vec<Vec<String>> {
        let mut commands = Vec::new();
        let mut argv: Vec<String> = Vec::new();
        let mut word: Option<String> = None;
        let mut it = text.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                '#' if word.is_none() => {
                    while it.peek().is_some_and(|c| *c != '\n') {
                        it.next();
                    }
                }
                '\n' => {
                    argv.extend(word.take());
                    if !argv.is_empty() {
                        commands.push(std::mem::take(&mut argv));
                    }
                }
                ' ' | '\t' => argv.extend(word.take()),
                '`' => match it.next() {
                    Some('\n') => {}
                    other => panic!("a backtick before {other:?} in {text:?}"),
                },
                c if POWERSHELL_QUOTES.contains(&c) => {
                    let w = word.get_or_insert_default();
                    loop {
                        match it.next() {
                            // a quote of any kind ends it, unless another follows:
                            // then the second is the character
                            Some(q) if POWERSHELL_QUOTES.contains(&q) => match it.peek().copied() {
                                Some(n) if POWERSHELL_QUOTES.contains(&n) => {
                                    w.push(n);
                                    it.next();
                                }
                                _ => break,
                            },
                            Some(c) => w.push(c),
                            None => panic!("an open quote in {text:?}"),
                        }
                    }
                }
                c if c.is_ascii_alphanumeric() || c == '-' || c == '.' => {
                    word.get_or_insert_default().push(c)
                }
                c => panic!("{c:?} is PowerShell's to read in {text:?}"),
            }
        }
        argv.extend(word.take());
        if !argv.is_empty() {
            commands.push(argv);
        }
        commands
    }

    /// The command line cmd.exe's prompt hands to the program for each
    /// line of `text`: `%` only where no variable's name can be, `^`
    /// escapes taken off, and nothing cmd would act on left bare.
    fn cmd(text: &str) -> Vec<String> {
        let mut lines = Vec::new();
        for line in text.lines() {
            if line == "rem" || line.starts_with("rem ") {
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            // variables are read first: between two `%` a name. Each `%`
            // here follows a `^`, so every name ends in `^`: none is set
            for (i, c) in chars.iter().enumerate() {
                assert!(
                    *c != '%' || (i > 0 && chars[i - 1] == '^'),
                    "a bare % in {line:?}"
                );
            }
            let mut out = String::new();
            let mut it = chars.into_iter();
            while let Some(c) = it.next() {
                match c {
                    '^' => out.push(it.next().expect("a ^ ends the line")),
                    c if "()%!\"<>&|".contains(c) => {
                        panic!("{c:?} is cmd's to read in {line:?}")
                    }
                    c => out.push(c),
                }
            }
            lines.push(out);
        }
        lines
    }

    /// A Windows command line as the C runtime splits it into argv.
    fn windows_argv(line: &str) -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        let mut argv = Vec::new();
        let mut i = 0;
        loop {
            while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            if i >= chars.len() {
                return argv;
            }
            let mut arg = String::new();
            let mut quoted = false;
            while i < chars.len() {
                let mut backslashes = 0;
                while i < chars.len() && chars[i] == '\\' {
                    backslashes += 1;
                    i += 1;
                }
                if i < chars.len() && chars[i] == '"' {
                    arg.push_str(&"\\".repeat(backslashes / 2));
                    if backslashes % 2 == 1 {
                        arg.push('"');
                    } else if quoted && chars.get(i + 1) == Some(&'"') {
                        arg.push('"');
                        i += 1;
                    } else {
                        quoted = !quoted;
                    }
                    i += 1;
                    continue;
                }
                arg.push_str(&"\\".repeat(backslashes));
                if i >= chars.len() {
                    break;
                }
                if !quoted && (chars[i] == ' ' || chars[i] == '\t') {
                    break;
                }
                arg.push(chars[i]);
                i += 1;
            }
            argv.push(arg);
        }
    }

    /// The command line PowerShell before 7.3 builds from arguments: one
    /// with white space in quotes, nothing in it escaped; with `doubling`
    /// as PowerShell 6 to 7.2 does it (the backslashes before the closing
    /// quote doubled), without as Windows PowerShell 5.1.
    fn legacy_powershell_line(argv: &[String], doubling: bool) -> String {
        argv.iter()
            .filter(|a| !a.is_empty())
            .map(|a| {
                if a.chars().any(char::is_whitespace) {
                    let trailing = a.len() - a.trim_end_matches('\\').len();
                    format!(
                        "\"{a}{}\"",
                        "\\".repeat(if doubling { trailing } else { 0 })
                    )
                } else {
                    a.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The one command `text` is for `shell`, as the program gets it; none
    /// when it is comments only.
    fn read_back(text: &str, shell: Shell) -> Option<Vec<String>> {
        let mut commands = match shell {
            Shell::Posix => {
                assert_eq!(fish(text), sh(text), "fish reads another command: {text}");
                sh(text)
            }
            Shell::Cmd => cmd(text).iter().map(|l| windows_argv(l)).collect(),
            Shell::PowerShell => {
                let lines: Vec<&str> = text.lines().collect();
                let notes = lines.iter().take_while(|l| l.starts_with("# ")).count();
                let guarded = lines.get(notes) == Some(&POWERSHELL_GUARD);
                let body = if guarded {
                    assert_eq!(lines.last(), Some(&POWERSHELL_GUARD_END), "{text}");
                    lines[notes + 1..lines.len() - 1].join("\n")
                } else {
                    lines[notes..].join("\n")
                };
                let commands = powershell(&body);
                for argv in &commands {
                    assert_eq!(
                        guarded,
                        argv.iter().any(|a| legacy_powershell_breaks(a)),
                        "{text}"
                    );
                    if !guarded {
                        // an old PowerShell hands the program the same argv
                        for doubling in [false, true] {
                            assert_eq!(
                                &windows_argv(&legacy_powershell_line(argv, doubling)),
                                argv,
                                "{text}"
                            );
                        }
                    }
                }
                commands
            }
        };
        assert!(commands.len() <= 1, "more than one command in {text:?}");
        commands.pop()
    }

    /// The argv is one no value of the record steers: curl's.
    fn curl_argv_is_safe(a: &[String]) {
        assert!(a[0] == "curl" || a[0] == "curl.exe", "{a:?}");
        assert_eq!(&a[1..3], ["-sS", "-g"], "{a:?}");
        let mut i = if a[3] == "-I" {
            4
        } else {
            assert_eq!(&a[3..5], ["-i", "-X"], "{a:?}");
            assert!(a[5].chars().all(|c| c.is_ascii_uppercase()), "{a:?}");
            6
        };
        while a[i] == "-H" {
            let h = &a[i + 1];
            let name = h.split([':', ';']).next().unwrap();
            assert!(header_name(name), "{a:?}");
            assert!(
                h[name.len()..].starts_with(": ") || &h[name.len()..] == ";",
                "{a:?}"
            );
            i += 2;
        }
        assert_eq!(a[i], "--", "{a:?}");
        assert_eq!(a.len(), i + 2, "{a:?}");
        let url = &a[i + 1];
        assert!(
            url.starts_with("http://") || url.starts_with("https://"),
            "{a:?}"
        );
        assert!(url.chars().all(|c| c.is_ascii_graphic()), "{a:?}");
    }

    /// The same of `aws`'s: after the operation, every argument is one
    /// `--option=value` of sonde's own options (or where the object goes),
    /// and no value is one aws reads from elsewhere.
    fn aws_argv_is_safe(a: &[String]) {
        assert!(a[0] == "aws" || a[0] == "aws.exe", "{a:?}");
        if a[1] == "sts" {
            assert_eq!(a[2], "get-caller-identity");
        } else {
            assert_eq!(a[1], "s3api");
            assert!(
                a[2].chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                    && !a[2].starts_with('-'),
                "{a:?}"
            );
        }
        for arg in &a[3..] {
            if arg == "/dev/null" || arg == "NUL" {
                continue;
            }
            let (flag, value) = arg.split_once('=').unwrap_or_else(|| panic!("{a:?}"));
            match flag {
                "--endpoint-url" => assert!(value.starts_with("http"), "{a:?}"),
                "--region" => {}
                "--cli-input-json" => {
                    let doc: serde_json::Value = serde_json::from_str(value).unwrap();
                    assert!(doc.is_object(), "{a:?}");
                }
                flag => {
                    assert!(
                        flag == "--bucket"
                            || flag == "--key"
                            || QUERY_OPTIONS.iter().any(|(_, f)| *f == flag)
                            || HEADER_OPTIONS.iter().any(|(_, f)| *f == flag),
                        "{a:?}"
                    );
                    assert!(!aws_reads_elsewhere(value), "{a:?}");
                }
            }
        }
    }

    /// Both commands of `r` for every shell: read back as that shell reads
    /// them, each is the argv it was built as, and that argv is safe.
    fn holds(r: &Record) {
        for shell in [Shell::Posix, Shell::PowerShell, Shell::Cmd] {
            let texts = [
                (curl(r, shell).map(|c| c.argv()), as_curl(r, shell)),
                (aws_cli(r, shell).map(|c| c.argv()), as_aws_cli(r, shell)),
            ];
            for (i, (argv, text)) in texts.into_iter().enumerate() {
                // what is pasted is what it looks like
                assert!(
                    text.chars().all(|c| c == '\n' || !hidden(c)),
                    "{shell:?}: {text:?}"
                );
                for line in text.lines() {
                    assert!(!line.trim().is_empty(), "{shell:?}: {text:?}");
                }
                let back = read_back(&text, shell);
                match argv {
                    Err(_) => {
                        assert_eq!(back, None, "{shell:?}: {text}");
                        assert_eq!(text.lines().count(), 1, "{text}");
                    }
                    Ok(argv) => {
                        assert_eq!(back.as_ref(), Some(&argv), "{shell:?}: {text}");
                        if i == 0 {
                            curl_argv_is_safe(&argv)
                        } else {
                            aws_argv_is_safe(&argv)
                        }
                    }
                }
            }
        }
    }

    /// A request made of two hostile values, everywhere a run's file could
    /// hold them.
    fn hostile(v: &str, w: &str) -> Record {
        Record {
            kind: "GetObject".into(),
            method: "GET".into(),
            bucket: Some(v.into()),
            key: Some(w.into()),
            rare: crate::Rare {
                url: format!("http://127.0.0.1:9000/{v}/{w}?versionId={w}&x={v}"),
                region: Some("us-east-1".into()),
                ..Default::default()
            }
            .into(),
            lists: crate::Lists {
                query: vec![
                    ("versionId".into(), w.into()),
                    ("prefix".into(), v.into()),
                    ("max-keys".into(), w.into()),
                ],
                request_headers: vec![
                    Header::new("range", v),
                    Header::new("if-match", w),
                    Header::new("x-amz-copy-source", v),
                    Header::new("x-amz-meta-a", w),
                    Header::new(v, w),
                    Header::new(w, v),
                ],
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }
    }

    const HOSTILE: &[&str] = &[
        "",
        "-",
        "--",
        "--output",
        "--output=/etc/passwd",
        "-o/tmp/x",
        "-K/etc/passwd",
        "--endpoint-url=http://evil.example",
        "--cli-input-json={}",
        "a b",
        " lead",
        "trail ",
        "a'b",
        "'",
        "''",
        "a\"b",
        "\"",
        "\"\"",
        "\"etag\"",
        "x\" --output \"C:\\evil",
        "$(touch x)",
        "`touch x`",
        "${IFS}",
        "$HOME",
        "a;touch x",
        "a&calc",
        "a|b",
        "a&&b||c",
        "a>b<c",
        "(a)",
        "%PATH%",
        "%",
        "%%",
        "^",
        "a^b",
        "^%PATH^%",
        "!PATH!",
        "a\\",
        "a b\\",
        "\\",
        "\\\\",
        "\\\"",
        "a\\\\\"b c",
        "C:\\Users\\x\\",
        "x\ny",
        "x\r\ntouch y",
        "x\n#!/bin/sh\nrm -rf ~",
        "\u{1b}[31mred\u{1b}[0m",
        "\u{1b}]0;title\u{7}",
        "\u{9b}31m",
        "\u{85}",
        "\u{0}",
        "a\tb",
        "\u{202e}gpj.exe",
        "\u{2066}a\u{2069}",
        "\u{200f}\u{200e}\u{61c}",
        "\u{202a}\u{202b}\u{202c}\u{202d}",
        "\u{2028}\u{2029}",
        "it\u{2019}s",
        "\u{2018}a\u{2019}",
        "\u{201a}\u{201b}'",
        "'\u{2019}; calc; '",
        "file:///etc/passwd",
        "fileb:///etc/passwd",
        "FILE://x",
        "http://169.254.169.254/latest",
        "https://example.com/k",
        "@/etc/passwd",
        "@",
        "a,b",
        "~",
        "~root",
        "*",
        "?",
        "{a,b}",
        "[1-1000000]",
        "#x",
        "a #b",
        "é 日本 語",
        "a\u{a0}b",
        "a=b",
        "/dev/null",
        "..\\..",
        "a%20b",
        "name: value",
        "x:\r\nInjected: 1",
        "javascript:alert(1)",
        "ftp://h/k",
        "//h/k",
        "\\\\host\\share",
        "`",
        "a`nb",
        "$env:PATH",
        "@('a')",
        "a b c d",
    ];

    #[test]
    fn hostile_values_stay_one_argument_in_every_shell() {
        for (i, v) in HOSTILE.iter().enumerate() {
            for w in [HOSTILE[(i + 1) % HOSTILE.len()], "k", v] {
                let r = hostile(v, w);
                holds(&r);
                holds(&r.clone().redacted());
                // the same as the URL whole, the method, the operation, the region
                let mut u = hostile("b", "k");
                u.rare_mut().url = v.to_string();
                holds(&u);
                let mut u = hostile(w, "k");
                u.method = (*v).into();
                holds(&u);
                let mut u = hostile("b", w);
                u.kind = (*v).into();
                holds(&u);
                let mut u = hostile(w, v);
                u.rare_mut().region = Some(v.to_string());
                u.method = "PUT".into();
                u.request_bytes = 3.into();
                holds(&u);
                u.method = "HEAD".into();
                u.kind = "GetCallerIdentity".into();
                holds(&u);
            }
        }
    }

    #[test]
    fn values_made_of_what_shells_read_stay_one_argument() {
        const PIECES: &[&str] = &[
            "'", "\"", "\\", " ", "%", "^", "&", "|", "<", ">", "(", ")", "!", "$", "`", ";", "-",
            "--", "=", "@", "#", "~", "*", "{", "}", "[", "]", ",", ".", "/", ":", "\n", "\r",
            "\t", "\u{1b}", "\u{202e}", "\u{2019}", "\u{2018}", "a", "B", "7", "é", "PATH",
            "file://", "$(", "\\\"", "^\"", "''",
        ];
        // (a fixed sequence: the same values every run)
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % n as u64) as usize
        };
        let value = |next: &mut dyn FnMut(usize) -> usize| {
            (0..next(9))
                .map(|_| PIECES[next(PIECES.len())])
                .collect::<String>()
        };
        for _ in 0..3000 {
            let (v, w) = (value(&mut next), value(&mut next));
            holds(&hostile(&v, &w));
        }
    }

    #[test]
    fn a_value_is_never_the_tools_next_option() {
        let mut r = hostile("-b", "--output");
        r.lists_mut().query = vec![("versionId".into(), "-v".into())];
        r.lists_mut().request_headers = vec![Header::new("range", "--debug")];
        r.rare_mut().url = "http://127.0.0.1:9000/-b/--output?versionId=-v".into();
        assert_eq!(
            read_back(&as_aws_cli(&r, Shell::Posix), Shell::Posix).unwrap(),
            [
                "aws",
                "s3api",
                "get-object",
                "--bucket=-b",
                "--key=--output",
                "--version-id=-v",
                "--range=--debug",
                "/dev/null",
                "--endpoint-url=http://127.0.0.1:9000",
                "--region=us-east-1",
            ]
        );
        assert_eq!(
            read_back(&as_curl(&r, Shell::Posix), Shell::Posix).unwrap(),
            [
                "curl",
                "-sS",
                "-g",
                "-i",
                "-X",
                "GET",
                "-H",
                "range: --debug",
                "--",
                "http://127.0.0.1:9000/-b/--output?versionId=-v",
            ]
        );
    }

    #[test]
    fn a_value_aws_would_read_from_a_file_or_fetch_goes_as_json() {
        for key in ["file:///etc/passwd", "fileb://x", "https://example.com/k"] {
            let mut r = hostile("b", key);
            r.lists_mut().query = vec![("max-keys".into(), "5".into())];
            r.lists_mut().request_headers.clear();
            let argv = read_back(&as_aws_cli(&r, Shell::Posix), Shell::Posix).unwrap();
            let json = argv[3].strip_prefix("--cli-input-json=").unwrap();
            let doc: serde_json::Value = serde_json::from_str(json).unwrap();
            assert_eq!(
                doc,
                serde_json::json!({"Bucket": "b", "Key": key, "MaxKeys": 5})
            );
            assert!(!argv.iter().any(|a| a.starts_with("--key")), "{argv:?}");
        }
    }

    #[test]
    fn only_an_http_url_and_a_methods_name_are_copied_as_curl() {
        for url in [
            "file:///etc/passwd",
            "ftp://h/k",
            "-K/etc/passwd",
            "javascript:alert(1)",
            "",
            "http://",
        ] {
            let mut r = rec();
            r.rare_mut().url = url.into();
            assert_eq!(
                as_curl(&r, Shell::Posix),
                "# sonde: not copied: its URL is not an http or https one"
            );
            assert_eq!(
                as_curl(&r, Shell::Cmd),
                "rem sonde: not copied: its URL is not an http or https one"
            );
            // aws is given no endpoint from such a URL
            assert!(
                !as_aws_cli(&r, Shell::Posix).contains("--endpoint-url"),
                "{url}"
            );
        }
        let mut r = rec();
        r.method = "GET;touch${IFS}x".into();
        assert_eq!(
            as_curl(&r, Shell::Posix),
            "# sonde: not copied: its method is not an HTTP method's name"
        );
        r.kind = "Get$(id)".into();
        assert_eq!(
            as_aws_cli(&r, Shell::Posix),
            "# sonde: not copied: its operation is not one aws s3api has"
        );
    }

    #[test]
    fn a_comment_holds_nothing_of_the_record_and_no_line_of_its_own() {
        let mut r = rec();
        r.rare_mut().region = Some("us-east-1\ntouch x".into());
        r.lists_mut()
            .request_headers
            .push(Header::new("x-note", "a\r\nrm -rf ~ \u{202e}"));
        for shell in [Shell::Posix, Shell::PowerShell, Shell::Cmd] {
            let curl = as_curl(&r, shell);
            assert!(!curl.contains("touch"), "{curl}");
            assert!(curl.contains("--aws-sigv4 aws:amz:us-east-1:s3"), "{curl}");
            assert!(curl.contains("NOT EXACT"), "{curl}");
            assert!(curl.contains("x-note: arm -rf ~"), "{curl}");
            let cli = as_aws_cli(&r, shell);
            assert!(!cli.contains("touch") && !cli.contains("--region"), "{cli}");
        }
    }

    #[test]
    fn a_url_is_asked_for_once_whatever_its_key_looks_like() {
        let mut r = rec();
        r.rare_mut().url = "http://h/b/part-[1-100000]{a,b}?prefix={x,y}[1-9]".into();
        let argv = read_back(&as_curl(&r, Shell::Posix), Shell::Posix).unwrap();
        assert!(argv.contains(&"-g".to_string()), "{argv:?}");
        // a URL's password is not copied
        r.rare_mut().url = "https://me:hunter2@h/b/k".into();
        assert!(!as_curl(&r, Shell::Posix).contains("hunter2"));
    }

    #[test]
    fn powershell_runs_what_an_old_one_would_split_only_where_it_is_passed_whole() {
        let mut r = rec();
        r.lists_mut()
            .request_headers
            .push(Header::new("If-Match", "\"etag\" --output \"C:\\x"));
        let text = as_curl(&r, Shell::PowerShell);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[2], POWERSHELL_GUARD, "{text}");
        assert_eq!(lines[lines.len() - 1], POWERSHELL_GUARD_END, "{text}");
        assert!(
            lines[3].starts_with("curl.exe -sS -g -i -X 'GET' `"),
            "{text}"
        );
        assert!(
            text.contains("-H 'If-Match: \"etag\" --output \"C:\\x' `"),
            "{text}"
        );
        // cmd: the quotes escaped for the program, and for cmd
        let text = as_curl(&r, Shell::Cmd);
        assert!(
            text.contains("-H ^\"If-Match: \\^\"etag\\^\" --output \\^\"C:\\x^\""),
            "{text}"
        );
        // without such a value there is no guard
        assert!(!as_curl(&rec(), Shell::PowerShell).contains("if ("));
    }

    /// The test's reading of sh is sh's own: `/bin/sh` given the command,
    /// with `curl` and `aws` as functions that print their arguments.
    #[cfg(unix)]
    #[test]
    fn sh_itself_reads_the_command_as_the_argv_it_was_built_as() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        for (i, v) in HOSTILE.iter().enumerate() {
            let r = hostile(v, HOSTILE[(i + 7) % HOSTILE.len()]);
            for (name, argv, text) in [
                (
                    "curl",
                    curl(&r, Shell::Posix).unwrap().argv(),
                    as_curl(&r, Shell::Posix),
                ),
                (
                    "aws",
                    aws_cli(&r, Shell::Posix).unwrap().argv(),
                    as_aws_cli(&r, Shell::Posix),
                ),
            ] {
                let script = format!("{name}() {{ printf '%s\\0' \"$@\"; }}\n{text}\n");
                let mut sh = std::process::Command::new("/bin/sh")
                    .arg("-s")
                    .current_dir(dir.path())
                    .env_clear()
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                sh.stdin
                    .take()
                    .unwrap()
                    .write_all(script.as_bytes())
                    .unwrap();
                let out = sh.wait_with_output().unwrap();
                assert!(out.status.success() && out.stderr.is_empty(), "{script}");
                let got: Vec<String> = String::from_utf8(out.stdout)
                    .unwrap()
                    .split_terminator('\0')
                    .map(str::to_string)
                    .collect();
                assert_eq!(got, argv[1..], "{script}");
            }
        }
        // nothing ran but the two functions: no file was made
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn cmd_refuses_a_line_it_would_cut() {
        let mut r = rec();
        r.kind = "GetObject".into();
        // a key of quotes and percents: each written longer for cmd.exe
        let key = "\"%".repeat(2000);
        r.key = Some(key.into());
        r.rare_mut().url = format!("http://127.0.0.1:9000/bucket/{}", "%22%25".repeat(2000));
        for text in [as_curl(&r, Shell::Cmd), as_aws_cli(&r, Shell::Cmd)] {
            assert!(
                text.starts_with("rem sonde: not copied: it is longer than cmd.exe"),
                "{text}"
            );
        }
        assert!(!as_curl(&r, Shell::PowerShell).contains("not copied"));
        assert!(!as_curl(&rec(), Shell::Cmd).contains("not copied"));
    }
}
