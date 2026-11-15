//! What sonde's services say, as data: a phrase is a key and its arguments,
//! said in the reader's language from `core.<key>` of the app's locale files,
//! by the app as by the CLI (in English, [`sonde_fmt::words`]). A text is
//! phrases said one after the other. The type is here because every crate
//! that says something depends on this one (`sonde-core` and `probes` use it
//! as their own).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Phrase {
    pub key: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
}

impl Phrase {
    pub fn new(key: impl Into<String>, args: &[(&str, String)]) -> Phrase {
        Phrase {
            key: key.into(),
            args: args
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }
}

// a single phrase where a text is wanted
impl From<Phrase> for Vec<Phrase> {
    fn from(p: Phrase) -> Self {
        vec![p]
    }
}

/// `"s"` after a count other than one, for English plurals (`%{s}`).
pub fn plural(n: usize) -> String {
    if n == 1 { String::new() } else { "s".into() }
}

/// A phrase: `ph!("key", name = value, …)`, each value by its `Display`.
#[macro_export]
macro_rules! ph {
    ($key:expr $(, $name:ident = $val:expr)* $(,)?) => {
        $crate::phrase::Phrase::new($key, &[$((stringify!($name), $val.to_string())),*])
    };
}

/// `ph!` here too, beside the type, for `phrase::ph`.
pub use crate::ph;

/// Words sonde does not translate (a key, an error a library worded).
pub fn raw(text: impl Into<String>) -> Phrase {
    Phrase::new("raw", &[("text", text.into())])
}

/// A fact's label: the key is the label itself under `fact.`.
pub fn fact_label(label: &str) -> Phrase {
    Phrase::new(format!("fact.{label}"), &[])
}

/// A phrase in English, from `core.<key>` of the app's locale file: every
/// key Rust says has one.
pub fn say(p: &Phrase) -> String {
    let t = sonde_fmt::words::core(&p.key);
    debug_assert!(t.is_some(), "core.{} is not in app/locales/en.json", p.key);
    sonde_fmt::words::fill(t.unwrap_or(""), |name| p.args.get(name).map(String::as_str))
}

/// A text in English: its phrases one after the other.
pub fn en(text: &[Phrase]) -> String {
    text.iter().map(say).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phrase_read_back_from_json_says_the_same() {
        let x = raw("a %{text}");
        let back: Phrase = serde_json::from_str(&serde_json::to_string(&x).unwrap()).unwrap();
        assert_eq!(back, x);
        assert_eq!(say(&back), "a %{text}");
        assert_eq!(say(&fact_label("Bytes")), "Bytes");
    }
}
