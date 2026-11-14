//! How sonde writes a size, a count and a length of time: one way, in every
//! crate, and the same as the app (`app/src/format.ts`): its `bytes`,
//! `count` and `duration` take the same numbers to the same figures.
//! `cases.json` beside this crate is the table both are tested against.
//!
//! - [`bytes`]: `0 B`, `999 B`, `1.0 KiB`, `1.5 MiB`, `512 MiB`: one decimal
//!   under 100, none from 100, never four digits (1020 bytes is `1.0 KiB`,
//!   never `1020 B`), and the unit chosen after rounding (1,048,575 bytes is
//!   `1.0 MiB`, never `1024 KiB`).
//! - [`count`]: `1,234,567`, separated in thousands always, in prose and in
//!   tables alike.
//! - [`duration`]: `0.4 ms`, `84 ms`, `2.35 s`, `41.2 s`, `3m 05s`,
//!   `1h 40m`: the unit chosen after rounding, so nothing reads `1000 ms` or
//!   `60.0 s`. The app writes the same figures in its own words (`3 min 05 s`,
//!   `1 h 40 min`).
//!
//! Figures are rounded as JavaScript's `toFixed` rounds: half away from zero
//! on the exact value, so `1.25` is `1.3` here as in the app.

const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

/// A count of bytes: `1.0 KiB` for 1024. See the crate's documentation.
pub fn bytes(n: u64) -> String {
    let n = u128::from(n);
    // the unit under 1000 of it: no figure has four digits
    let mut i = 0;
    while i < UNITS.len() - 1 && n >= 1000u128 << (10 * i) {
        i += 1;
    }
    if i == 0 {
        return format!("{n} B");
    }
    // the unit after rounding: 999.96 KiB is 1.0 MiB
    if i < UNITS.len() - 1 {
        let (r, decimals) = scaled(n, i);
        if r >= if decimals == 0 { 1000 } else { 10000 } {
            i += 1;
        }
    }
    let (r, decimals) = scaled(n, i);
    if decimals == 0 {
        format!("{r} {}", UNITS[i])
    } else {
        format!("{}.{} {}", r / 10, r % 10, UNITS[i])
    }
}

/// `n` in unit `i`, rounded half up: whole from 100 of it, else in tenths
/// (the second value says which).
fn scaled(n: u128, i: usize) -> (u128, u32) {
    let unit = 1u128 << (10 * i);
    if n >= 100 * unit {
        ((2 * n + unit) / (2 * unit), 0)
    } else {
        ((20 * n + unit) / (2 * unit), 1)
    }
}

/// A size that is not a whole count of bytes (a mean, a rate): rounded to
/// one first, then as [`bytes`]. A negative or non-finite size is `0 B`.
pub fn bytes_f64(b: f64) -> String {
    if b.is_finite() && b > 0.0 {
        bytes(round(b) as u64)
    } else {
        "0 B".into()
    }
}

/// 1234567 as `1,234,567`.
pub fn count(n: u64) -> String {
    let d = n.to_string();
    let mut out = String::with_capacity(d.len() + d.len() / 3);
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The columns `s` takes in a terminal: one a character, two for the East
/// Asian wide ones (CJK, Hangul, full-width forms, emoji). `—`, `…` and `→`
/// take one.
pub fn width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let c = c as u32;
            let wide = matches!(c,
                0x1100..=0x115F
                | 0x2E80..=0x303E
                | 0x3041..=0x33FF
                | 0x3400..=0x4DBF
                | 0x4E00..=0x9FFF
                | 0xA000..=0xA4CF
                | 0xAC00..=0xD7A3
                | 0xF900..=0xFAFF
                | 0xFE30..=0xFE4F
                | 0xFF00..=0xFF60
                | 0xFFE0..=0xFFE6
                | 0x1F300..=0x1F64F
                | 0x1F900..=0x1F9FF
                | 0x20000..=0x3FFFD);
            if wide { 2 } else { 1 }
        })
        .sum()
}

/// An amount of money to the cent, never `-0.00`: a cost a rounding left
/// below zero is none.
pub fn cents(v: f64) -> String {
    let t = format!("{:.2}", v);
    if t == "-0.00" { "0.00".into() } else { t }
}

/// A share as a percentage: `<0.1%`, `0.4%`, `12%`; never `100%` for less
/// than all, nor `0%` for more than none.
pub fn percent(share: f64) -> String {
    let p = share * 100.0;
    if p > 0.0 && p < 0.1 {
        "<0.1%".into()
    } else if p < 10.0 {
        format!("{p:.1}%")
    } else if (99.5..100.0).contains(&p) {
        if p >= 99.95 {
            ">99.9%".into()
        } else {
            format!("{p:.1}%")
        }
    } else {
        format!("{p:.0}%")
    }
}

/// A count that can fall: `-1,234`, `+56` with `signed`.
pub fn count_signed(n: i64, plus: bool) -> String {
    let sign = if n < 0 {
        "-"
    } else if plus && n > 0 {
        "+"
    } else {
        ""
    };
    format!("{sign}{}", count(n.unsigned_abs()))
}

/// A count with its noun: `1 request`, `0 requests`, `10,004 requests`.
pub fn counted(n: u64, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{} {one}s", count(n))
    }
}

/// A length of time in milliseconds, as measured. See the crate's
/// documentation.
pub fn duration(ms: f64) -> String {
    if !ms.is_finite() {
        return "—".into();
    }
    let a = ms.abs();
    if a == 0.0 {
        return "0 ms".into();
    }
    if a < 9.95 {
        return format!("{} ms", to_fixed(ms, 1));
    }
    if a < 999.5 {
        return format!("{} ms", round(ms));
    }
    if a < 9995.0 {
        return format!("{} s", to_fixed(ms / 1000.0, 2));
    }
    if a < 59950.0 {
        return format!("{} s", to_fixed(ms / 1000.0, 1));
    }
    let s = round(a / 1000.0) as u64;
    let sign = if ms < 0.0 { "−" } else { "" };
    if s < 3600 {
        return format!("{sign}{}m {:02}s", s / 60, s % 60);
    }
    let m = round(s as f64 / 60.0) as u64;
    format!("{sign}{}h {:02}m", m / 60, m % 60)
}

/// JavaScript's `Math.round`: half toward positive infinity.
fn round(v: f64) -> f64 {
    let r = v.round();
    // (Rust's takes it away from zero: -2.5 to -3, where JavaScript has -2)
    if v - r == 0.5 { r + 1.0 } else { r }
}

/// JavaScript's `Number.prototype.toFixed` for a number under 10²¹: the
/// exact value rounded to `digits` decimals, a tie away from zero (Rust's
/// own formatting takes a tie to the even digit).
pub fn to_fixed(v: f64, digits: usize) -> String {
    // the exact value, far enough past `digits` that a tie shows as one: a
    // double's expansion that is not a tie differs from one well within
    // these places at the magnitudes a figure has
    let long = format!("{:.*}", digits + 30, v.abs());
    let cut = long.len() - 30;
    let (head, tail) = long.split_at(cut);
    let tie = tail.starts_with('5') && tail[1..].bytes().all(|b| b == b'0');
    let mut text = if tie {
        round_up(head)
    } else {
        format!("{:.*}", digits, v.abs())
    };
    if text.ends_with('.') {
        text.pop();
    }
    // (as JavaScript: -0.04 is "-0.0", and -0 is "0")
    if v < 0.0 {
        text.insert(0, '-');
    }
    text
}

/// A decimal string with its last digit raised by one (and carried).
fn round_up(s: &str) -> String {
    let mut d: Vec<u8> = s.bytes().collect();
    let mut i = d.len();
    loop {
        if i == 0 {
            d.insert(0, b'1');
            break;
        }
        i -= 1;
        match d[i] {
            b'.' => continue,
            b'9' => d[i] = b'0',
            c => {
                d[i] = c + 1;
                break;
            }
        }
    }
    String::from_utf8(d).expect("ascii digits")
}

/// A character that does something to a terminal rather than show itself:
/// C0 (tab and newline among them), DEL, C1, and the marks that turn text's
/// direction (a key can be made to read as another).
fn controls(c: char) -> bool {
    c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

fn escape_into(out: &mut String, c: char) {
    use std::fmt::Write;
    let _ = match c {
        '\n' => write!(out, "\\n"),
        '\r' => write!(out, "\\r"),
        '\t' => write!(out, "\\t"),
        '\u{1b}' => write!(out, "\\e"),
        c if (c as u32) < 0x80 => write!(out, "\\x{:02x}", c as u32),
        c => write!(out, "\\u{{{:x}}}", c as u32),
    };
}

/// Text from outside sonde (a key, a bucket, a program's command or
/// output, a log's line) as it may be shown: every control character
/// written out (`\e`, `\x07`, `\n`, `\u{9b}`), so that none moves the
/// cursor, sets the window's title or starts a line of its own.
pub fn escape(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.chars().any(controls) {
        return text.into();
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if controls(c) {
            escape_into(&mut out, c);
        } else {
            out.push(c);
        }
    }
    out.into()
}

/// What sonde writes to a terminal, whatever is in it: as [`escape`], but
/// lines and tabs stay, and so do sonde's own colours (`ESC [ … m`) and
/// its progress line's erase (`\r ESC [2K`). Everything sonde prints passes
/// through it.
pub fn terminal(text: &str) -> std::borrow::Cow<'_, str> {
    // (the common case: nothing to look at)
    if !text.chars().any(|c| controls(c) && c != '\n' && c != '\t') {
        return text.into();
    }
    let mut out = String::with_capacity(text.len() + 8);
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let kept = match c {
            '\n' | '\t' => 1,
            '\r' if rest[1..].starts_with("\u{1b}[2K") => 1,
            '\u{1b}' => own_sequence(rest),
            _ => 0,
        };
        if kept > 0 {
            out.push_str(&rest[..kept]);
            rest = &rest[kept..];
            continue;
        }
        if controls(c) {
            escape_into(&mut out, c);
        } else {
            out.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    out.into()
}

/// The length of the sequence of sonde's own at the start of `text`: a
/// colour (`ESC [` digits and `;` `m`) or the erase of a line (`ESC [2K`).
fn own_sequence(text: &str) -> usize {
    let Some(body) = text.strip_prefix("\u{1b}[") else {
        return 0;
    };
    if body.starts_with("2K") {
        return 4;
    }
    let params = body
        .bytes()
        .take_while(|b| b.is_ascii_digit() || *b == b';')
        .count();
    if params <= 12 && body.as_bytes().get(params) == Some(&b'm') {
        2 + params + 1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_shares_and_cents_at_their_edges() {
        assert_eq!(width("—"), 1);
        assert_eq!(width("a → b…"), 6);
        assert_eq!(width("操作"), 4);
        assert_eq!(percent(0.995), "99.5%");
        assert_eq!(percent(0.9999), ">99.9%");
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(0.0004), "<0.1%");
        assert_eq!(percent(0.5), "50%");
        assert_eq!(cents(-0.001), "0.00");
        assert_eq!(cents(-0.0), "0.00");
        assert_eq!(cents(1.234), "1.23");
    }

    #[test]
    fn a_tie_goes_away_from_zero_as_in_javascript() {
        assert_eq!(to_fixed(1.25, 1), "1.3");
        assert_eq!(to_fixed(-1.25, 1), "-1.3");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(9.995, 2), "9.99"); // 9.99499999… in binary
        assert_eq!(to_fixed(0.05, 1), "0.1"); // 0.05000000000000000277…
        assert_eq!(to_fixed(9.95, 1), "9.9"); // 9.949999…
        assert_eq!(to_fixed(99.95, 1), "100.0");
        assert_eq!(to_fixed(0.0, 2), "0.00");
        assert_eq!(to_fixed(-0.04, 1), "-0.0");
        assert_eq!(to_fixed(-0.0, 1), "0.0");
        assert_eq!(to_fixed(2.0, 0), "2");
    }

    #[test]
    fn javascripts_round_takes_a_negative_half_up() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(-2.6), -3.0);
    }

    #[test]
    fn the_largest_count_of_bytes_is_written() {
        assert_eq!(bytes(u64::MAX), "16384 PiB");
        assert_eq!(bytes_f64(-3.0), "0 B");
        assert_eq!(bytes_f64(f64::NAN), "0 B");
        assert_eq!(bytes_f64(1535.6), "1.5 KiB");
    }

    #[test]
    fn counts_with_a_sign_and_a_noun() {
        assert_eq!(count_signed(-1234, false), "-1,234");
        assert_eq!(count_signed(56, true), "+56");
        assert_eq!(count_signed(0, true), "0");
        assert_eq!(counted(1, "request"), "1 request");
        assert_eq!(counted(10_004, "request"), "10,004 requests");
    }

    #[test]
    fn control_characters_are_written_out_never_sent() {
        let planted = "logs/\u{1b}]0;pwned\u{7}x\u{9b}2J\u{7f}\u{202e}fdp.exe";
        assert_eq!(
            escape(planted),
            "logs/\\e]0;pwned\\x07x\\u{9b}2J\\x7f\\u{202e}fdp.exe"
        );
        assert_eq!(escape("a\nb\tc\rd"), "a\\nb\\tc\\rd");
        assert!(matches!(escape("plain/日本/ü"), std::borrow::Cow::Borrowed(_)));
        // what sonde prints: its lines, colours and progress erase stay
        let line = format!("\u{1b}[31mbroke\u{1b}[0m at {planted}\n\r\u{1b}[2K3/4\t");
        assert_eq!(
            terminal(&line),
            "\u{1b}[31mbroke\u{1b}[0m at logs/\\e]0;pwned\\x07x\\u{9b}2J\\x7f\\u{202e}fdp.exe\n\r\u{1b}[2K3/4\t"
        );
        // what is not a colour is not let through
        assert_eq!(terminal("\u{1b}[2J\u{1b}[1;31m"), "\\e[2J\u{1b}[1;31m");
        assert_eq!(terminal("a\rb"), "a\\rb");
        assert_eq!(terminal("\u{1b}"), "\\e");
    }
}
