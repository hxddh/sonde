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
    /// Told each time a page has come in (or failed): a `load_all` waits on
    /// it for a page `load_more` is loading.
    idle: Arc<tokio::sync::Notify>,
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
            idle: Arc::default(),
            closed: Arc::default(),
        }
    }

    /// Loads the next page; resolves with `{ count, done, pages, recordId }`.
    pub async fn load_more(this: Instance<Self>) -> Result<JsonValue, String> {
        if this.is_disposed() {
            return Err(err_msg("the listing was closed"));
        }
        let (ep, b, p, d, state, idle) = this.with(|l| {
            (
                l.endpoint.clone(),
                l.bucket.clone(),
                l.prefix.clone(),
                l.delimiter.clone(),
                l.state.clone(),
                l.idle.clone(),
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
        idle.notify_waiters();
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
        let (ep, b, p, d, state, idle, closed) = this.with(|l| {
            (
                l.endpoint.clone(),
                l.bucket.clone(),
                l.prefix.clone(),
                l.delimiter.clone(),
                l.state.clone(),
                l.idle.clone(),
                l.closed.clone(),
            )
        });
        let st = state.clone();
        let r = background(async move {
            loop {
                let Some(token) = next_turn(&st, &idle, &closed, max_rows as usize).await else {
                    return Ok::<(), sonde_core::S3Error>(());
                };
                let page =
                    browse::list_page(&sonde(), &ep, &b, &p, d.as_deref(), token, None).await;
                let mut s = st.lock();
                s.loading = false;
                idle.notify_waiters();
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

/// The next page's turn for `load_all`: its token once no other page is
/// loading (a page `load_more` has under way is waited for, not taken as the
/// end), or `None` when every page is in, `max_rows` are, or the listing went.
async fn next_turn(
    state: &Mutex<State>,
    idle: &tokio::sync::Notify,
    closed: &std::sync::atomic::AtomicBool,
    max_rows: usize,
) -> Option<Option<String>> {
    loop {
        // listening before looking: a page that ends in between is heard
        let ended = idle.notified();
        tokio::pin!(ended);
        ended.as_mut().enable();
        {
            let mut s = state.lock();
            if s.done
                || s.rows.len() >= max_rows
                || closed.load(std::sync::atomic::Ordering::SeqCst)
            {
                return None;
            }
            if !s.loading {
                s.loading = true;
                return Some(s.token.clone());
            }
        }
        ended.await;
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

    #[tokio::test]
    async fn loading_all_waits_for_the_page_under_way_then_goes_on() {
        let (state, idle, closed) = (
            Mutex::new(State {
                loading: true,
                ..State::default()
            }),
            tokio::sync::Notify::new(),
            std::sync::atomic::AtomicBool::new(false),
        );
        let turn = next_turn(&state, &idle, &closed, 1000);
        tokio::pin!(turn);
        // a page is loading: not the end, a wait
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), turn.as_mut())
                .await
                .is_err()
        );
        {
            let mut s = state.lock();
            s.loading = false;
            s.token = Some("page-2".into());
        }
        idle.notify_waiters();
        assert_eq!(turn.await, Some(Some("page-2".into())));
        assert!(state.lock().loading, "its own page now");
        state.lock().done = true;
        assert_eq!(next_turn(&state, &idle, &closed, 1000).await, None);
    }

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
