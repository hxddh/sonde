//! A program's terminal output as lines with times, for lining it up with
//! the requests it made.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::session::LogChunk;

/// One line of output: when it began, its text without escape codes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogLine {
    pub time: DateTime<Utc>,
    pub text: String,
}

/// Removes ANSI escape sequences (CSI, OSC, and two-byte escapes).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('[') => {
                // CSI: parameters and intermediates, then a final byte @..~
                for d in it.by_ref() {
                    if ('@'..='~').contains(&d) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC: until BEL or ESC \
                while let Some(d) = it.next() {
                    if d == '\u{7}' {
                        break;
                    }
                    if d == '\u{1b}' && it.peek() == Some(&'\\') {
                        it.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Splits output chunks into lines. A line's time is when its first
/// character arrived; a carriage return without a newline starts the line
/// over (progress bars keep their last state); empty lines are dropped.
pub fn lines(chunks: &[LogChunk]) -> Vec<LogLine> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut started: Option<DateTime<Utc>> = None;
    let mut push = |text: &mut String, started: &mut Option<DateTime<Utc>>| {
        let t = strip_ansi(text);
        let t = t.trim_end();
        if let (false, Some(time)) = (t.trim().is_empty(), *started) {
            out.push(LogLine {
                time,
                text: t.to_string(),
            });
        }
        text.clear();
        *started = None;
    };
    for chunk in chunks {
        let mut rest = chunk.text.as_str();
        while !rest.is_empty() {
            match rest.find(['\n', '\r']) {
                Some(i) => {
                    if i > 0 && started.is_none() {
                        started = Some(chunk.time);
                    }
                    current.push_str(&rest[..i]);
                    let sep = rest.as_bytes()[i];
                    let crlf = sep == b'\r' && rest.as_bytes().get(i + 1) == Some(&b'\n');
                    if sep == b'\n' || crlf {
                        push(&mut current, &mut started);
                        rest = &rest[i + if crlf { 2 } else { 1 }..];
                    } else {
                        // a lone CR: the line is drawn again from its start
                        current.clear();
                        rest = &rest[i + 1..];
                    }
                }
                None => {
                    if started.is_none() {
                        started = Some(chunk.time);
                    }
                    current.push_str(rest);
                    rest = "";
                }
            }
        }
    }
    push(&mut current, &mut started);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(ms: i64, text: &str) -> LogChunk {
        LogChunk {
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + ms).unwrap(),
            text: text.into(),
        }
    }

    #[test]
    fn splits_strips_and_times() {
        let ls = lines(&[
            chunk(0, "\u{1b}[32mwriting\u{1b}[0m batch 1\r\nwrit"),
            chunk(10, "ing batch 2\r\n"),
            chunk(20, "progress 10%\rprogress 100%\n\n"),
            chunk(30, "\u{1b}]0;title\u{7}done"),
        ]);
        let texts: Vec<&str> = ls.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "writing batch 1",
                "writing batch 2",
                "progress 100%",
                "done"
            ]
        );
        assert_eq!(
            ls[1].time,
            chunk(0, "").time,
            "a line starts when its first character came"
        );
        assert_eq!(ls[2].time, chunk(20, "").time);
        assert_eq!(ls[3].time, chunk(30, "").time);
    }
}
