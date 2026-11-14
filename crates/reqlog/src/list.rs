//! [`List`]: a record's headers or query, shared by the records that hold the same.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{Header, Text};

/// A record's headers or query parameters: a list that records holding the
/// same one share ([`List::shared`]). A session of a million requests, read
/// for its analysis, keeps of most requests the same user agent alone: held
/// once, not a million times. Read as a slice; changed whole.
#[derive(Clone)]
pub struct List<T>(Option<Arc<[T]>>);

impl<T> List<T> {
    pub fn new() -> List<T> {
        List(None)
    }

    pub fn as_slice(&self) -> &[T] {
        self.0.as_deref().unwrap_or(&[])
    }
}

impl<T: Clone> List<T> {
    /// Its items to change: a list shared with other records is copied first.
    pub fn make_mut(&mut self) -> &mut [T] {
        match &mut self.0 {
            None => &mut [],
            Some(items) => {
                if Arc::get_mut(items).is_none() {
                    *items = Arc::from(items.to_vec());
                }
                Arc::get_mut(items).expect("held alone")
            }
        }
    }

    pub fn push(&mut self, item: T) {
        let mut items = self.as_slice().to_vec();
        items.push(item);
        *self = List::from(items);
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        if self.iter().all(&mut keep) {
            return;
        }
        *self = self.iter().filter(|t| keep(t)).cloned().collect();
    }
}

impl<T> Default for List<T> {
    fn default() -> Self {
        List(None)
    }
}

impl<T> std::ops::Deref for List<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T> From<Vec<T>> for List<T> {
    fn from(items: Vec<T>) -> Self {
        if items.is_empty() {
            List(None)
        } else {
            List(Some(Arc::from(items)))
        }
    }
}

impl<T> FromIterator<T> for List<T> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        List::from(items.into_iter().collect::<Vec<T>>())
    }
}

impl<'a, T> IntoIterator for &'a List<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<T: PartialEq> PartialEq for List<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: PartialEq> PartialEq<Vec<T>> for List<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Eq> Eq for List<T> {}

impl<T: std::fmt::Debug> std::fmt::Debug for List<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

impl<T: Serialize> Serialize for List<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.as_slice().serialize(s)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for List<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Vec::<T>::deserialize(d).map(List::from)
    }
}

/// How many lists a thread remembers to share (the newest at each place).
const SHARED: usize = 1024;

thread_local! {
    static HEADERS: std::cell::RefCell<Vec<Option<Arc<[Header]>>>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static QUERIES: std::cell::RefCell<Vec<Option<Arc<[(Text, Text)]>>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn share<T: Hash + PartialEq>(
    recent: &'static std::thread::LocalKey<std::cell::RefCell<Vec<Option<Arc<[T]>>>>>,
    list: List<T>,
) -> List<T> {
    let Some(items) = list.0 else {
        return List(None);
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    items.hash(&mut h);
    let at = h.finish() as usize % SHARED;
    recent
        .try_with(|recent| {
            let mut recent = recent.borrow_mut();
            if recent.is_empty() {
                recent.resize(SHARED, None);
            }
            match &recent[at] {
                Some(seen) if **seen == *items => List(Some(seen.clone())),
                _ => {
                    recent[at] = Some(items.clone());
                    List(Some(items.clone()))
                }
            }
        })
        .unwrap_or(List(Some(items)))
}

impl List<Header> {
    /// The list a recent equal one is kept as, when there is one.
    pub fn shared(self) -> Self {
        share(&HEADERS, self)
    }
}

impl List<(Text, Text)> {
    /// The list a recent equal one is kept as, when there is one.
    pub fn shared(self) -> Self {
        share(&QUERIES, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_lists_are_held_once() {
        let ua = || List::from(vec![Header::new("user-agent", "boto3")]);
        let (a, b) = (ua().shared(), ua().shared());
        assert!(Arc::ptr_eq(a.0.as_ref().unwrap(), b.0.as_ref().unwrap()));
        assert_eq!(a, b);
        // changed, a shared list is copied: the other keeps what it had
        let mut c = b.clone();
        c.make_mut()[0].value = "curl".into();
        c.push(Header::new("range", "bytes=0-1"));
        assert_eq!(a[0].value, "boto3");
        assert_eq!((c.len(), &*c[0].value), (2, "curl"));
        c.retain(|h| h.name == "range");
        assert_eq!(c.len(), 1);
        assert_eq!(std::mem::size_of::<List<Header>>(), 16);
        assert!(List::<Header>::new().is_empty());
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"[{"name":"range","value":"bytes=0-1"}]"#
        );
    }
}
