//! Output formats: JSON for machines, aligned text tables for people.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::ValueEnum;

/// Whoever read the output has gone (`sonde … | head`): nothing more is
/// written, and the command runs on to its own exit code.
static CLOSED: AtomicBool = AtomicBool::new(false);

/// Standard output, where a closed pipe is no error: what is written after
/// it is dropped ([`CLOSED`]). `println!` and `print!` write through it.
pub struct Stdout;

impl Stdout {
    fn on<T>(r: std::io::Result<T>, done: T) -> std::io::Result<T> {
        match r {
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                CLOSED.store(true, Ordering::Relaxed);
                Ok(done)
            }
            r => r,
        }
    }
}

impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if CLOSED.load(Ordering::Relaxed) {
            return Ok(buf.len());
        }
        Stdout::on(std::io::stdout().write(buf), buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if CLOSED.load(Ordering::Relaxed) {
            return Ok(());
        }
        Stdout::on(std::io::stdout().flush(), ())
    }
}

/// What `println!` and `print!` do (main.rs): as std's, but a closed pipe
/// ends the output, not the command.
/// Its control characters (a key's, a program's) are written out, never
/// sent: only sonde's own colours and line breaks reach the terminal
/// ([`sonde_fmt::terminal`]).
pub fn print_out(args: std::fmt::Arguments<'_>, newline: bool) {
    let mut out = Stdout;
    let written = out.write_all(said(args, newline).as_bytes());
    if let Err(e) = written {
        panic!("failed printing to stdout: {e}");
    }
}

/// What `eprintln!` and `eprint!` do (main.rs): as std's, with control
/// characters written out as [`print_out`] writes them.
pub fn print_err(args: std::fmt::Arguments<'_>, newline: bool) {
    // (a closed stderr is no reason to stop)
    let _ = std::io::stderr().write_all(said(args, newline).as_bytes());
}

fn said(args: std::fmt::Arguments<'_>, newline: bool) -> String {
    let s = args.to_string();
    let mut text = match sonde_fmt::terminal(&s) {
        std::borrow::Cow::Borrowed(_) => s,
        std::borrow::Cow::Owned(o) => o,
    };
    if newline {
        text.push('\n');
    }
    text
}

/// What every command writes: text for people, JSON for machines (`sonde
/// check` writes two more of its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum Format {
    #[default]
    Text,
    Json,
}

/// A count with its noun: `1 request`, `3 requests`, `10,004 requests`.
pub fn count(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{} {one}s", grouped(n as u64))
    }
}

/// A whole number as every command writes one: 1234567 as `1,234,567`
/// ([`sonde_fmt::count`]: one way, everywhere).
pub fn grouped(n: u64) -> String {
    sonde_fmt::count(n)
}

/// A length of time as every command writes one: `0.4 ms`, `217 ms`,
/// `2.35 s`, `2m 26s`, `1h 40m` ([`sonde_fmt::duration`]).
pub fn duration(ms: f64) -> String {
    sonde_fmt::duration(ms)
}

/// Prints rows as an aligned table (wide characters counted as two).
pub fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    // a cell is one line: a key's newline or tab is written out, never a
    // row of its own (sonde's colours stay)
    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.iter().map(|c| one_line(c)).collect())
        .collect();
    let rows = &rows[..];
    let width = sonde_fmt::width;
    let mut w: Vec<usize> = headers.iter().map(|h| width(h)).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            if i < w.len() {
                w[i] = w[i].max(width(c));
            }
        }
    }
    let line = |cells: Vec<&str>| -> String {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                format!(
                    "{c}{}",
                    " ".repeat(w.get(i).copied().unwrap_or(0).saturating_sub(width(c)))
                )
            })
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    let mut out = vec![line(headers.to_vec())];
    out.push(
        w.iter()
            .map(|n| "─".repeat(*n))
            .collect::<Vec<_>>()
            .join("  "),
    );
    for r in rows {
        out.push(line(r.iter().map(String::as_str).collect()));
    }
    out.join("\n")
}

/// A table's cell on one line: its newlines, returns and tabs written out
/// (`\n`), what else it holds as it is.
fn one_line(c: &str) -> String {
    if !c.contains(['\n', '\r', '\t']) {
        return c.to_string();
    }
    c.replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// `label  value` lines, aligned.
pub fn kv(rows: &[(String, String)]) -> String {
    let w = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(0);
    rows.iter()
        .map(|(k, v)| format!("{k}{}  {v}", " ".repeat(w - k.chars().count())))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `v`'s fractions rounded, as `show` and `check` write them: a time (its
/// key ends `Ms`, or is `ms`) to a tenth of a millisecond, any other figure
/// to 6 significant digits.
pub fn rounded(v: &mut serde_json::Value, key: &str) {
    match v {
        serde_json::Value::Object(m) => m.iter_mut().for_each(|(k, x)| rounded(x, k)),
        serde_json::Value::Array(a) => a.iter_mut().for_each(|x| rounded(x, key)),
        serde_json::Value::Number(n) if n.is_f64() => {
            let f = n.as_f64().unwrap_or_default();
            let r = if key.ends_with("Ms") || key == "ms" {
                (f * 10.0).round() / 10.0
            } else {
                format!("{f:.5e}").parse().unwrap_or(f)
            };
            if let Some(r) = serde_json::Number::from_f64(r) {
                *n = r;
            }
        }
        _ => {}
    }
}

/// JSON as every command writes it: pretty, and what serde_json leaves raw
/// in a string that a terminal would act on (DEL, C1, the marks that turn
/// text's direction) as `\uXXXX`. The same text is then valid JSON and
/// safe on a terminal: [`sonde_fmt::terminal`] finds nothing in it to write
/// out.
pub fn json(v: &impl serde::Serialize) -> String {
    let text = serde_json::to_string_pretty(v).unwrap_or_default();
    match json_safe(&text) {
        std::borrow::Cow::Borrowed(_) => text,
        std::borrow::Cow::Owned(o) => o,
    }
}

/// [`json`]'s value on stdout, as it is: JSON never passes through
/// [`print_out`], whose escapes are not JSON's.
pub fn print_json(v: &impl serde::Serialize) {
    let mut text = json(v);
    text.push('\n');
    if let Err(e) = Stdout.write_all(text.as_bytes()) {
        panic!("failed printing to stdout: {e}");
    }
}

/// Serialized JSON with what a terminal acts on escaped as JSON escapes it.
/// (serde_json escapes C0 itself; what is left can only be in a string.)
fn json_safe(text: &str) -> std::borrow::Cow<'_, str> {
    let acts = |c: char| c == '\u{7f}' || (!c.is_ascii() && sonde_fmt::controls(c));
    if !text.chars().any(acts) {
        return text.into();
    }
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        if acts(c) {
            out.push_str(&format!("\\u{:04x}", c as u32));
        } else {
            out.push(c);
        }
    }
    out.into()
}

/// A writer of JSON that escapes as [`json`] does, for JSON written as it
/// is made (`show -f json`). A character cut between two writes waits for
/// its rest.
pub struct JsonWriter<W: Write> {
    out: W,
    tail: Vec<u8>,
}

impl<W: Write> JsonWriter<W> {
    pub fn new(out: W) -> Self {
        JsonWriter {
            out,
            tail: Vec::new(),
        }
    }
}

impl<W: Write> Write for JsonWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.tail.extend_from_slice(buf);
        let whole = match std::str::from_utf8(&self.tail) {
            Ok(_) => self.tail.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            // not UTF-8 at all: not serde_json's, written as it is
            Err(_) => {
                self.out.write_all(&self.tail)?;
                self.tail.clear();
                return Ok(buf.len());
            }
        };
        let text = std::str::from_utf8(&self.tail[..whole]).expect("checked above");
        self.out.write_all(json_safe(text).as_bytes())?;
        self.tail.drain(..whole);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.write_all(&std::mem::take(&mut self.tail))?;
        self.out.flush()
    }
}

/// Rows as an aligned table, as `print!` writes them.
pub fn print(headers: &[&str], rows: &[Vec<String>]) {
    println!("{}", table(headers, rows));
}

pub fn bytes(b: u64) -> String {
    sonde_fmt::bytes(b)
}

#[cfg(test)]
mod tests {
    /// 27.0 wrote a key's DEL and U+202E as `\x7f` and `\u{202e}`: no JSON.
    #[test]
    fn json_escapes_what_a_terminal_acts_on_as_json_does() {
        let key = "a\u{7f}b\u{202e}c\u{9b}d\u{2066}é";
        let text = super::json(&serde_json::json!({ key: key }));
        assert!(
            !text.contains(['\u{7f}', '\u{202e}', '\u{9b}', '\u{2066}']),
            "{text}"
        );
        assert_eq!(sonde_fmt::terminal(&text), text);
        let back: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back[key], key);
        // written as it is made, a character cut between writes
        use std::io::Write;
        let mut w = super::JsonWriter::new(Vec::new());
        let raw = serde_json::to_vec(&key).unwrap();
        for b in &raw {
            w.write_all(std::slice::from_ref(b)).unwrap();
        }
        w.flush().unwrap();
        let out = String::from_utf8(w.out).unwrap();
        assert_eq!(out, super::json(&key));
    }

    /// 24.0 broke a `--by key` row at a key's newline.
    #[test]
    fn a_key_s_newline_is_written_out_in_a_table() {
        let t = super::table(
            &["key", "requests"],
            &[vec!["b/cfg/x\nbreak.json".into(), "12".into()]],
        );
        assert_eq!(t.lines().count(), 3, "{t}");
        assert!(t.contains("b/cfg/x\\nbreak.json  12"), "{t}");
    }

    #[test]
    fn a_dash_takes_one_column() {
        let t = super::table(
            &["p95", "cost"],
            &[
                vec!["—".into(), "1%".into()],
                vec!["12 ms".into(), "2%".into()],
            ],
        );
        let at: Vec<usize> = t
            .lines()
            .filter(|l| l.ends_with('%'))
            .map(|l| sonde_fmt::width(l.split('%').next().unwrap()))
            .collect();
        assert_eq!(at[0], at[1], "{t}");
    }

    #[test]
    fn aligned() {
        let t = super::table(&["a", "bb"], &[vec!["xxx".into(), "y".into()]]);
        assert_eq!(t, "a    bb\n───  ──\nxxx  y");
    }
}
