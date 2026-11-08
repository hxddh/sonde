//! Output formats: JSON for machines, aligned text tables for people.

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum Format {
    #[default]
    Table,
    Json,
    Markdown,
    Csv,
}

/// Prints rows as an aligned table (wide characters counted as two).
pub fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let width = |s: &str| {
        s.chars()
            .map(|c| {
                if (c as u32) > 0x1100 && !c.is_ascii() {
                    2
                } else {
                    1
                }
            })
            .sum::<usize>()
    };
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

#[cfg(test)]
mod tests {
    #[test]
    fn aligned() {
        let t = super::table(&["a", "bb"], &[vec!["xxx".into(), "y".into()]]);
        assert_eq!(t, "a    bb\n───  ──\nxxx  y");
    }
}

/// Rows in the format asked for: an aligned table, a Markdown table or CSV.
/// (JSON is each command's own, as it carries more than the rows.)
pub fn render(format: Format, headers: &[&str], rows: &[Vec<String>]) -> String {
    match format {
        Format::Markdown => {
            let line = |cells: Vec<String>| format!("| {} |\n", cells.join(" | "));
            let esc = |c: &str| c.replace('|', "\\|");
            let mut s = line(headers.iter().map(|h| esc(h)).collect());
            s.push_str(&line(headers.iter().map(|_| "---".to_string()).collect()));
            for r in rows {
                s.push_str(&line(r.iter().map(|c| esc(c)).collect()));
            }
            s
        }
        Format::Csv => {
            let cell = |c: &str| {
                if c.contains([',', '"', '\n']) {
                    format!("\"{}\"", c.replace('"', "\"\""))
                } else {
                    c.to_string()
                }
            };
            let mut s = headers
                .iter()
                .map(|h| cell(h))
                .collect::<Vec<_>>()
                .join(",");
            s.push('\n');
            for r in rows {
                s.push_str(&r.iter().map(|c| cell(c)).collect::<Vec<_>>().join(","));
                s.push('\n');
            }
            s
        }
        _ => format!("{}\n", table(headers, rows)),
    }
}

pub fn print(format: Format, headers: &[&str], rows: &[Vec<String>]) {
    print!("{}", render(format, headers, rows));
}

pub fn bytes(b: u64) -> String {
    sonde_core::guard::human_bytes(b)
}

#[cfg(test)]
mod render_tests {
    use super::*;

    #[test]
    fn markdown_and_csv_are_real() {
        let rows = vec![vec!["a|b".to_string(), "x,\"y\"".to_string()]];
        assert_eq!(
            render(Format::Markdown, &["k", "v"], &rows),
            "| k | v |\n| --- | --- |\n| a\\|b | x,\"y\" |\n"
        );
        assert_eq!(
            render(Format::Csv, &["k", "v"], &rows),
            "k,v\na|b,\"x,\"\"y\"\"\"\n"
        );
    }
}
