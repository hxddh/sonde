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

pub fn json(v: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
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
