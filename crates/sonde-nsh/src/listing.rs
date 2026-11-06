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
        }
    }

    /// Loads the next page; resolves with `{ count, done, pages, recordId }`.
    pub async fn load_more(this: Instance<Self>) -> Result<JsonValue, String> {
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
