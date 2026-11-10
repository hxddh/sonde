use std::sync::Arc;

use nsh_core::JsonValue;
use nsh_core::prelude::*;
use parking_lot::Mutex;
use sonde_core::services::browse::{self, Entry};

use crate::state::{err, err_msg, sonde, to_json};

#[derive(Default)]
struct State {
    rows: Vec<Entry>,
    token: Option<String>,
    done: bool,
    loading: bool,
    pages: u32,
    filter: String,
    /// Indexes into `rows` that match `filter` (all when empty).
    view: Option<Vec<usize>>,
}

/// One prefix of a bucket, listed page by page (ListObjectsV2) and kept in
/// Rust: TSX asks for the rows it shows, so 100 000 keys never cross at once.
#[class]
pub struct Listing {
    endpoint: String,
    bucket: String,
    prefix: String,
    delimiter: Option<String>,
    state: Arc<Mutex<State>>,
    /// Set as the listing goes: a `load_all` under way stops at its next page.
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Listing {
    fn drop(&mut self) {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn entry_json(e: &Entry) -> JsonValue {
    to_json(e)
}

#[class]
impl Listing {
    /// A listing of `bucket` under `prefix`, split by `delimiter` (`/`; null for flat).
    pub fn new(
        endpoint: String,
        bucket: String,
        prefix: String,
        delimiter: Option<String>,
    ) -> Self {
        Listing {
            endpoint,
            bucket,
            prefix,
            delimiter,
            state: Arc::new(Mutex::new(State::default())),
            closed: Arc::default(),
        }
    }

    /// Loads the next page; resolves with `{ count, done, pages, recordId }`.
    pub async fn load_more(this: Instance<Self>) -> Result<JsonValue, String> {
        if this.is_disposed() {
            return Err(err_msg("the listing was closed"));
        }
        let (ep, b, p, d, state) = this.with(|l| {
            (
                l.endpoint.clone(),
                l.bucket.clone(),
                l.prefix.clone(),
                l.delimiter.clone(),
                l.state.clone(),
            )
        });
        let token = {
            let mut s = state.lock();
            if s.done || s.loading {
                return Ok(
                    serde_json::json!({ "count": count(&s), "done": s.done, "pages": s.pages }),
                );
            }
            s.loading = true;
            s.token.clone()
        };
        let page = background(async move {
            browse::list_page(&sonde(), &ep, &b, &p, d.as_deref(), token, None).await
        })
        .await;
        let mut s = state.lock();
        s.loading = false;
        match page {
            Ok(Ok(page)) => {
                s.pages += 1;
                s.done = !page.is_truncated || page.next_token.is_none();
                s.token = page.next_token;
                s.rows.extend(page.entries);
                if !s.filter.is_empty() {
                    let f = s.filter.clone();
                    refilter(&mut s, &f);
                }
                Ok(
                    serde_json::json!({ "count": count(&s), "done": s.done, "pages": s.pages, "recordId": page.record_id }),
                )
            }
            Ok(Err(e)) => Err(err(e)),
            Err(e) => Err(err_msg(e)),
        }
    }

    /// Loads every remaining page (at most `max_rows` rows in all), off the
    /// engine thread; resolves with `{ count, done, pages }`.
    pub async fn load_all(this: Instance<Self>, max_rows: u32) -> Result<JsonValue, String> {
        if this.is_disposed() {
            return Err(err_msg("the listing was closed"));
        }
        let (ep, b, p, d, state, closed) = this.with(|l| {
            (
                l.endpoint.clone(),
                l.bucket.clone(),
                l.prefix.clone(),
                l.delimiter.clone(),
                l.state.clone(),
                l.closed.clone(),
            )
        });
        let st = state.clone();
        let r = background(async move {
            loop {
                let token = {
                    let mut s = st.lock();
                    if s.done || s.loading || s.rows.len() >= max_rows as usize {
                        return Ok::<(), sonde_core::S3Error>(());
                    }
                    s.loading = true;
                    s.token.clone()
                };
                let page =
                    browse::list_page(&sonde(), &ep, &b, &p, d.as_deref(), token, None).await;
                let mut s = st.lock();
                s.loading = false;
                let page = page?;
                s.pages += 1;
                s.done = !page.is_truncated || page.next_token.is_none();
                s.token = page.next_token;
                s.rows.extend(page.entries);
                // the listing was dropped: stop (counting the state's owners
                // cannot tell: this call holds two of them itself)
                if closed.load(std::sync::atomic::Ordering::SeqCst) {
                    return Ok(());
                }
            }
        })
        .await;
        let mut s = state.lock();
        if !s.filter.is_empty() {
            let f = s.filter.clone();
            refilter(&mut s, &f);
        }
        match r {
            Ok(Ok(())) => {
                Ok(serde_json::json!({ "count": count(&s), "done": s.done, "pages": s.pages }))
            }
            Ok(Err(e)) => Err(err(e)),
            Err(e) => Err(err_msg(e)),
        }
    }

    /// Rows `start .. start + count` of what is shown (after the filter).
    pub fn rows(&self, start: u32, count: u32) -> JsonValue {
        let s = self.state.lock();
        let (start, end) = (start as usize, start as usize + count as usize);
        let rows: Vec<JsonValue> = match &s.view {
            Some(v) => v
                .iter()
                .skip(start)
                .take(end - start)
                .map(|i| entry_json(&s.rows[*i]))
                .collect(),
            None => s
                .rows
                .iter()
                .skip(start)
                .take(end - start)
                .map(entry_json)
                .collect(),
        };
        JsonValue::Array(rows)
    }

    /// One row by its index in what is shown.
    pub fn row(&self, index: u32) -> JsonValue {
        let s = self.state.lock();
        let i = match &s.view {
            Some(v) => v.get(index as usize).copied(),
            None => Some(index as usize),
        };
        i.and_then(|i| s.rows.get(i))
            .map(entry_json)
            .unwrap_or(JsonValue::Null)
    }

    /// How many rows are shown (after the filter).
    #[getter]
    pub fn count(&self) -> u32 {
        count(&self.state.lock()) as u32
    }

    /// Every page is loaded.
    #[getter]
    pub fn done(&self) -> bool {
        self.state.lock().done
    }

    /// Keeps the rows whose name contains `text` (case-insensitive); empty shows all.
    pub fn set_filter(&mut self, text: String) -> u32 {
        let mut s = self.state.lock();
        refilter(&mut s, &text);
        count(&s) as u32
    }

    /// Bytes and objects loaded so far: `{ objects, prefixes, bytes }`.
    pub fn totals(&self) -> JsonValue {
        let s = self.state.lock();
        let objects = s.rows.iter().filter(|e| !e.is_prefix).count();
        serde_json::json!({ "objects": objects, "prefixes": s.rows.len() - objects, "bytes": s.rows.iter().map(|e| e.size).sum::<u64>() })
    }
}

fn count(s: &State) -> usize {
    s.view.as_ref().map(|v| v.len()).unwrap_or(s.rows.len())
}

fn refilter(s: &mut State, text: &str) {
    s.filter = text.to_string();
    if text.is_empty() {
        s.view = None;
    } else {
        let t = text.to_lowercase();
        s.view = Some(
            s.rows
                .iter()
                .enumerate()
                .filter(|(_, e)| e.name.to_lowercase().contains(&t))
                .map(|(i, _)| i)
                .collect(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_that_goes_tells_the_load_under_way() {
        let l = Listing::new("e".into(), "b".into(), String::new(), None);
        // what load_all holds while it runs: the flag, and the state twice over
        let (closed, state) = (l.closed.clone(), l.state.clone());
        let _in_the_task = state.clone();
        assert!(!closed.load(std::sync::atomic::Ordering::SeqCst));
        drop(l);
        assert!(closed.load(std::sync::atomic::Ordering::SeqCst));
        assert!(
            Arc::strong_count(&state) > 1,
            "which the count never showed"
        );
    }
}
