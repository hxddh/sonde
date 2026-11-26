//! What sonde says about a run, as data: a phrase is a key and its
//! arguments, said in the reader's language from `phrase.<key>` of the app's
//! locale files, by the app and the shared page as by the CLI (in English,
//! [`sonde_fmt::words`]). A text is phrases said one after the other.

use std::collections::BTreeMap;

use reqlog::Text;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Phrase {
    pub key: &'static str,
    pub args: Args,
}

/// A phrase's arguments by name, in the order of their names: a map as it is
/// written and read, kept as the two or three pairs it is. Every operation
/// of a run has a title, and a run may have a million: its values
/// are shared text (an operation's name, a key its requests name already).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args(Vec<(&'static str, Text)>);

impl Args {
    pub fn get(&self, name: &str) -> Option<&Text> {
        self.0.iter().find(|(k, _)| *k == name).map(|(_, v)| v)
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn values(&self) -> impl Iterator<Item = &Text> {
        self.0.iter().map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &Text)> {
        self.0.iter().map(|(k, v)| (*k, v))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::ops::Index<&str> for Args {
    type Output = Text;
    fn index(&self, name: &str) -> &Text {
        self.get(name).expect("an argument of that name")
    }
}

impl Args {
    /// Sets argument `name`, in its place among the names.
    fn set(&mut self, name: &'static str, value: Text) {
        match self.0.iter_mut().find(|(have, _)| *have == name) {
            Some(slot) => slot.1 = value,
            None => {
                let at = self.0.partition_point(|(have, _)| *have < name);
                self.0.insert(at, (name, value));
            }
        }
    }
}

impl<V: Into<Text>> FromIterator<(&'static str, V)> for Args {
    /// A name given twice keeps its last value, as a map would.
    fn from_iter<I: IntoIterator<Item = (&'static str, V)>>(pairs: I) -> Self {
        let mut all: Vec<(&'static str, Text)> = Vec::new();
        for (k, v) in pairs {
            let v = v.into();
            match all.iter_mut().find(|(have, _)| *have == k) {
                Some(slot) => slot.1 = v,
                None => all.push((k, v)),
            }
        }
        all.sort_by(|a, b| a.0.cmp(b.0));
        all.shrink_to_fit();
        Args(all)
    }
}

impl Serialize for Args {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.0.iter().map(|(k, v)| (k, v)))
    }
}

/// A phrase: `key`'s template with `args`.
pub fn p(key: &'static str, args: &[(&'static str, String)]) -> Phrase {
    Phrase {
        key,
        args: args.iter().cloned().collect(),
    }
}

/// [`p`] of text shared already (an operation's name, a target its
/// requests' key is in): a million operations hold it once.
pub fn p_shared(key: &'static str, args: &[(&'static str, Text)]) -> Phrase {
    Phrase {
        key,
        args: args.iter().cloned().collect(),
    }
}

/// `"s"` after a count other than one, for English plurals (`%{s}`).
pub fn plural(n: usize) -> String {
    if n == 1 { String::new() } else { "s".into() }
}

/// The English template of `key`: every key Rust says has one.
fn template(key: &str) -> &'static str {
    let t = sonde_fmt::words::phrase(key);
    debug_assert!(t.is_some(), "phrase.{key} is not in app/locales/en.json");
    t.unwrap_or("")
}

/// The arguments that are counts: a figure of four digits or more among
/// them is said grouped, as every count is ("listed 87,139 times", beside
/// "87,139 requests"); ids, years and keys are other arguments, left as
/// written. The app's set (`app/src/phrases.ts`), and a fix's predicted
/// saving.
const COUNTS: &[&str] = &[
    "n", "of", "count", "requests", "reads", "writes", "objects", "total", "was", "clean",
    // a fix's predicted saving (`fix.saved`)
    "p",
];

/// `template` filled with `arg`, its counts grouped ([`COUNTS`]).
fn fill<'a>(template: &str, arg: impl Fn(&str) -> Option<&'a str>) -> String {
    use std::borrow::Cow;
    sonde_fmt::words::fill_with(template, |name| {
        let v = arg(name)?;
        let count = COUNTS.contains(&name) && v.len() >= 4 && v.bytes().all(|b| b.is_ascii_digit());
        // what it names from outside (a key) is one line of text: a
        // newline or a control in it is written out ([`sonde_fmt::escape`])
        Some(match v.parse::<u64>() {
            Ok(n) if count => Cow::Owned(sonde_fmt::count(n)),
            _ => sonde_fmt::escape(v),
        })
    })
}

impl Phrase {
    /// The phrase with one more argument (or another value for one it has):
    /// one its template may not say, read beside it (a finding's buckets,
    /// [`crate::profile::finding_identity`]).
    pub fn with(mut self, name: &'static str, value: impl Into<Text>) -> Self {
        self.args.set(name, value.into());
        self
    }

    /// The phrase in English.
    pub fn en(&self) -> String {
        fill(template(self.key), |name| self.args.get(name).map(|t| &**t))
    }
}

/// A phrase in English from a key and arguments kept as text (a verdict
/// read back from its file).
pub fn en_of(key: &str, args: &BTreeMap<String, String>) -> String {
    fill(template(key), |name| args.get(name).map(String::as_str))
}

/// A text in English: its phrases one after the other.
pub fn en(text: &[Phrase]) -> String {
    text.iter().map(Phrase::en).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 24.0 broke a finding's line at a key's newline.
    #[test]
    fn a_key_s_newline_is_written_out_in_a_phrase() {
        let x = p(
            "obs.hot",
            &[
                ("key", "b/cfg/x\nbreak.json".into()),
                ("n", "12".into()),
                ("pct", "100".into()),
            ],
        );
        assert_eq!(
            x.en(),
            "b/cfg/x\\nbreak.json was read 12 times (100% of all requests)"
        );
        // the phrase keeps the key as it is (the app's, JSON's)
        let key: &str = x.args.get("key").unwrap().as_ref();
        assert_eq!(key, "b/cfg/x\nbreak.json");
    }

    #[test]
    fn a_phrase_is_said_from_the_locale() {
        let x = p(
            "mpu",
            &[
                ("key", "b/%{n}%{s}.bin".into()),
                ("n", "2".into()),
                ("s", plural(2)),
                ("size", "200 B".into()),
            ],
        );
        assert_eq!(x.en(), "multipart upload b/%{n}%{s}.bin · 2 parts · 200 B");
        assert_eq!(
            en(&[x, p("mpu.open", &[])]),
            "multipart upload b/%{n}%{s}.bin · 2 parts · 200 B (not completed)"
        );
        assert_eq!(p("size", &[("size", "1 B".into())]).en(), " · 1 B");
        // a count is grouped, as the app says it; an id is not
        assert_eq!(
            p(
                "obs.listed",
                &[("prefix", "b/p/".into()), ("n", "87139".into())]
            )
            .en(),
            "b/p/ was listed 87,139 times"
        );
        assert_eq!(
            p(
                "slate.walprobe",
                &[("lo", "1000".into()), ("hi", "1128".into())]
            )
            .en(),
            "looked for the newest WAL: probed ids 1000–1128"
        );
    }
}
