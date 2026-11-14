//! [`Name`]: a word a session says over and over, in two bytes.

use std::collections::HashMap;
use std::num::NonZeroU16;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// A word records say over and over (a request's kind, its method, its
/// bucket), held as two bytes: its text is kept once, for as long as the
/// process runs. A session of a million requests names a few dozen kinds
/// and buckets; a record's three names take 6 bytes, not 48. It reads as a
/// `str` everywhere, and is written to a file as one.
///
/// Names are for a small vocabulary: past [`Name::MAX`] different ones (or
/// past [`Name::MAX_LEN`] bytes), a name reads as [`Name::OTHER`]'s text.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Name(NonZeroU16);

/// Each name's text, by number (the first is unused: a name is never 0).
static TEXTS: [OnceLock<&'static str>; 1 << 16] = [const { OnceLock::new() }; 1 << 16];
/// Each text's number, and the next to give.
static NUMBERS: Mutex<Option<(HashMap<&'static str, NonZeroU16>, u32)>> = Mutex::new(None);

/// How many names a thread remembers (the newest at each place).
const RECENT: usize = 256;

thread_local! {
    static SEEN: std::cell::RefCell<[Option<Name>; RECENT]> =
        const { std::cell::RefCell::new([None; RECENT]) };
}

impl Name {
    /// How many different names there can be (besides none and [`Name::OTHER`]).
    pub const MAX: usize = (1 << 16) - 3;
    /// The longest name's text, in bytes.
    pub const MAX_LEN: usize = 255;
    /// What a name past the vocabulary's room reads as.
    pub const OTHER: &'static str = "…";

    /// `s` as a name.
    pub fn new(s: &str) -> Name {
        let s = if s.len() > Name::MAX_LEN {
            Name::OTHER
        } else {
            s
        };
        // FNV-1a
        let mut h: u32 = 0x811c_9dc5;
        for b in s.as_bytes() {
            h = (h ^ u32::from(*b)).wrapping_mul(0x0100_0193);
        }
        let at = h as usize % RECENT;
        let seen = SEEN
            .try_with(|seen| seen.borrow()[at].filter(|n| n.as_str() == s))
            .ok()
            .flatten();
        if let Some(n) = seen {
            return n;
        }
        let n = Name::numbered(s);
        let _ = SEEN.try_with(|seen| seen.borrow_mut()[at] = Some(n));
        n
    }

    fn numbered(s: &str) -> Name {
        let mut numbers = NUMBERS.lock().unwrap_or_else(|e| e.into_inner());
        let (index, next) = numbers.get_or_insert_with(|| {
            // the two names there always are: none, and the one past the room
            let mut index = HashMap::new();
            for (n, text) in [(1, ""), (2, Name::OTHER)] {
                let _ = TEXTS[n as usize].set(text);
                index.insert(text, NonZeroU16::new(n).expect("not 0"));
            }
            (index, 3)
        });
        if let Some(n) = index.get(s) {
            return Name(*n);
        }
        if *next as usize > Name::MAX + 2 {
            return Name(index[Name::OTHER]);
        }
        let n = NonZeroU16::new(*next as u16).expect("names start at 1");
        let text: &'static str = Box::leak(s.into());
        let _ = TEXTS[usize::from(n.get())].set(text);
        index.insert(text, n);
        *next += 1;
        Name(n)
    }

    pub fn as_str(&self) -> &'static str {
        TEXTS[usize::from(self.0.get())]
            .get()
            .copied()
            .unwrap_or_default()
    }
}

impl Default for Name {
    fn default() -> Self {
        Name::new("")
    }
}

impl std::ops::Deref for Name {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for Name {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::borrow::Borrow<str> for Name {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

// (as its text hashes and orders: a map of names is asked by a `str`)
impl std::hash::Hash for Name {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.as_str().hash(h)
    }
}

impl PartialOrd for Name {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Name {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.0 == other.0 {
            return std::cmp::Ordering::Equal;
        }
        self.as_str().cmp(other.as_str())
    }
}

impl std::fmt::Debug for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), f)
    }
}

impl std::fmt::Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for Name {
    fn from(s: &str) -> Self {
        Name::new(s)
    }
}

impl From<String> for Name {
    fn from(s: String) -> Self {
        Name::new(&s)
    }
}

impl From<&String> for Name {
    fn from(s: &String) -> Self {
        Name::new(s)
    }
}

impl From<Name> for String {
    fn from(n: Name) -> Self {
        n.as_str().to_owned()
    }
}

impl PartialEq<str> for Name {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Name {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for Name {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<Name> for str {
    fn eq(&self, other: &Name) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<Name> for &str {
    fn eq(&self, other: &Name) -> bool {
        *self == other.as_str()
    }
}

impl PartialEq<Name> for String {
    fn eq(&self, other: &Name) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Serialize for Name {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Name {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = Name;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a string")
            }
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Name, E> {
                Ok(Name::new(s))
            }
        }
        d.deserialize_str(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_its_text_in_two_bytes() {
        assert_eq!(std::mem::size_of::<Name>(), 2);
        assert_eq!(std::mem::size_of::<Option<Name>>(), 2);
        let (a, b) = (
            Name::new("GetObject"),
            Name::from(String::from("GetObject")),
        );
        assert_eq!(a, b);
        assert_eq!(a, "GetObject");
        assert_ne!(a, Name::new("PutObject"));
        assert!(Name::new("Abc") < Name::new("abc"));
        assert_eq!(Name::default(), "");
        // the same name from another thread
        let c = std::thread::spawn(|| Name::new("GetObject"))
            .join()
            .unwrap();
        assert_eq!(a.0, c.0);
        assert_eq!(serde_json::to_string(&a).unwrap(), "\"GetObject\"");
        let d: Name = serde_json::from_str("\"GetObject\"").unwrap();
        assert_eq!(d, a);
    }

    #[test]
    fn a_name_too_long_reads_as_other() {
        assert_eq!(Name::new(&"x".repeat(Name::MAX_LEN + 1)), Name::OTHER);
        assert_eq!(Name::new(&"x".repeat(Name::MAX_LEN)).len(), Name::MAX_LEN);
    }
}
