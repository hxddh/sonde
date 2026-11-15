//! [`Lists`]: a record's query and headers, shared by the records that hold the same.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::{Header, Text};

/// A record's query parameters and headers. A record holds them behind one
/// pointer, which the records holding the same lists share ([`Lists::shared`]): a
/// run of a million requests, read for its analysis, keeps of most of
/// them the same user agent and nothing else, held once.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Lists {
    /// Query parameters, masked like the URL.
    pub query: Vec<(Text, Text)>,
    pub request_headers: Vec<Header>,
    pub response_headers: Vec<Header>,
}

impl Lists {
    pub fn is_empty(&self) -> bool {
        self.query.is_empty() && self.request_headers.is_empty() && self.response_headers.is_empty()
    }
}

/// A record's lists as a field: `lists: Lists { … }.into()` (none when empty).
impl From<Lists> for Option<Arc<Lists>> {
    fn from(l: Lists) -> Self {
        (!l.is_empty()).then(|| Arc::new(l))
    }
}

/// How many lists a thread remembers to share (the newest at each place).
const SHARED: usize = 1024;

thread_local! {
    static RECENT: std::cell::RefCell<Vec<Option<Arc<Lists>>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl Lists {
    /// `lists` as a recent equal one is held, when there is one; else as they
    /// are, each list no longer than what it holds.
    pub fn shared(lists: Option<Arc<Lists>>) -> Option<Arc<Lists>> {
        let mut lists = lists?;
        if lists.is_empty() {
            return None;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        lists.hash(&mut h);
        let at = h.finish() as usize % SHARED;
        let seen = RECENT
            .try_with(|recent| {
                let mut recent = recent.borrow_mut();
                if recent.is_empty() {
                    recent.resize(SHARED, None);
                }
                match &recent[at] {
                    Some(seen) if *seen == lists => Some(seen.clone()),
                    _ => {
                        if let Some(l) = Arc::get_mut(&mut lists) {
                            l.query.shrink_to_fit();
                            l.request_headers.shrink_to_fit();
                            l.response_headers.shrink_to_fit();
                        }
                        recent[at] = Some(lists.clone());
                        None
                    }
                }
            })
            .ok()
            .flatten();
        Some(seen.unwrap_or(lists))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_lists_are_held_once() {
        let ua = || {
            Lists {
                request_headers: vec![Header::new("user-agent", "boto3")],
                ..Default::default()
            }
            .into()
        };
        let (a, b) = (Lists::shared(ua()).unwrap(), Lists::shared(ua()).unwrap());
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(Lists::shared(Lists::default().into()), None);
        assert_eq!(Lists::shared(Some(Arc::new(Lists::default()))), None);
    }
}
