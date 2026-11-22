//! What Rust says in English, from the one place every phrase lives in every
//! language: the app's locale file (`app/locales/en.json`, built in). A
//! phrase of a run is `phrase.<key>` there (`sonde-proxy`), one of sonde's
//! services `core.<key>` (decoders, sonde-core, probes); `%{name}` takes the
//! argument of that name, as in the app.

use std::collections::HashMap;
use std::sync::OnceLock;

const EN: &str = include_str!("../../../app/locales/en.json");

struct Words {
    phrase: HashMap<String, String>,
    core: HashMap<String, String>,
}

fn words() -> &'static Words {
    static WORDS: OnceLock<Words> = OnceLock::new();
    WORDS.get_or_init(|| {
        let all: HashMap<String, String> =
            serde_json::from_str(EN).expect("app/locales/en.json maps keys to words");
        let mut w = Words {
            phrase: HashMap::new(),
            core: HashMap::new(),
        };
        for (k, v) in all {
            if let Some(k) = k.strip_prefix("phrase.") {
                w.phrase.insert(k.to_string(), v);
            } else if let Some(k) = k.strip_prefix("core.") {
                w.core.insert(k.to_string(), v);
            }
        }
        w
    })
}

/// The English of a run's phrase `key` (`phrase.<key>`).
pub fn phrase(key: &str) -> Option<&'static str> {
    words().phrase.get(key).map(String::as_str)
}

/// A run's phrase `key` as the locale keeps it, and its English: a key
/// read back as text (a verdict's) said again as a phrase.
pub fn phrase_entry(key: &str) -> Option<(&'static str, &'static str)> {
    words()
        .phrase
        .get_key_value(key)
        .map(|(k, v)| (k.as_str(), v.as_str()))
}

/// The English of a service's phrase `key` (`core.<key>`).
pub fn core(key: &str) -> Option<&'static str> {
    words().core.get(key).map(String::as_str)
}

/// `template` with each `%{name}` replaced by `arg(name)`; a name with no
/// argument stays as it is written. One pass: an argument's own text (a key
/// named "%{n}.json") is never read as a placeholder.
pub fn fill<'a>(template: &str, arg: impl Fn(&str) -> Option<&'a str>) -> String {
    fill_with(template, |name| arg(name).map(std::borrow::Cow::Borrowed))
}

/// [`fill`] with arguments that may be made as they are asked for.
pub fn fill_with<'a>(
    template: &str,
    arg: impl Fn(&str) -> Option<std::borrow::Cow<'a, str>>,
) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("%{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        match after
            .find('}')
            .and_then(|close| Some((close, arg(&after[..close])?)))
        {
            Some((close, value)) => {
                out.push_str(&value);
                rest = &after[close + 1..];
            }
            None => {
                out.push_str("%{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_argument_is_never_read_as_a_placeholder() {
        let args = |n: &str| match n {
            "key" => Some("b/%{n}.bin"),
            "n" => Some("2"),
            _ => None,
        };
        assert_eq!(
            fill("%{key} · %{n} · %{size} · 50%", args),
            "b/%{n}.bin · 2 · %{size} · 50%"
        );
    }

    #[test]
    fn the_words_are_the_locales() {
        assert_eq!(phrase("mpu.open"), Some(" (not completed)"));
        assert_eq!(core("none"), Some("none"));
        assert_eq!(
            phrase("run.title"),
            None,
            "the app's own words are not a phrase"
        );
    }
}
