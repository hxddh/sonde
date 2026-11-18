//! What a run leaves stored: the bytes it wrote and did not delete, each by
//! the request that wrote them; and, on a bucket that keeps versions, the
//! versions its overwrites and deletes made noncurrent (still stored, and
//! billed, until a lifecycle rule expires them).
//!
//! A store says a bucket keeps versions in a write's answer: its
//! `x-amz-version-id` (other than `null`, which a bucket whose versioning is
//! suspended answers) or `x-amz-delete-marker`.

use std::collections::HashMap;

use reqlog::Record;

/// What a run leaves stored.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stored {
    /// Bytes written and left (not deleted by the run), each by the place
    /// of the request that wrote them in the records.
    pub left: Vec<(usize, u64)>,
    /// A write's answer said the bucket keeps versions.
    pub versioned: bool,
    /// Versions the run's overwrites and deletes made noncurrent, on a
    /// bucket that keeps versions …
    pub noncurrent: usize,
    /// … and the bytes of those whose size the run saw (written or read whole
    /// in it before).
    pub noncurrent_bytes: u64,
}

impl Stored {
    pub fn bytes(&self) -> u64 {
        self.left.iter().map(|(_, b)| b).sum()
    }
}

fn response<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.header(name)
}

fn request<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.request_headers()
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn query<'a>(r: &'a Record, name: &str) -> Option<&'a str> {
    r.query()
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// The answer says the bucket keeps versions.
fn says_versioned(r: &Record) -> bool {
    response(r, "x-amz-version-id").is_some_and(|v| !v.is_empty() && v != "null")
        || response(r, "x-amz-delete-marker").is_some_and(|v| v == "true")
}

/// The object's whole size from a ranged read's `content-range`
/// (`bytes 0-99/12345`).
fn total_of(r: &Record) -> Option<u64> {
    response(r, "content-range")?
        .rsplit_once('/')?
        .1
        .trim()
        .parse()
        .ok()
}

/// `bucket/key` of a copy's source.
fn copy_source(r: &Record) -> Option<(String, String)> {
    let src = request(r, "x-amz-copy-source")?;
    let src = percent_encoding::percent_decode_str(src.split('?').next().unwrap_or(""))
        .decode_utf8_lossy()
        .into_owned();
    let (b, k) = src.trim_start_matches('/').split_once('/')?;
    Some((b.to_string(), k.to_string()))
}

/// What `records` (in the order they were sent) leave stored.
pub fn stored(records: &[Record]) -> Stored {
    let ok = |r: &Record| !r.is_error() && r.status.is_some_and(|s| s < 300);
    let writes = |r: &Record| matches!(r.method.as_str(), "PUT" | "POST" | "DELETE");
    let versioned = records
        .iter()
        .any(|r| writes(r) && ok(r) && says_versioned(r));
    let mut out = Stored {
        versioned,
        ..Stored::default()
    };
    // each key the run wrote (or, on a versioned bucket, saw the size of):
    // its size when known, and the request that wrote it when the run did
    let mut keys: HashMap<(&str, &str), (Option<u64>, Option<usize>)> = HashMap::new();
    let mut parts: HashMap<&str, u64> = HashMap::new();
    for (i, r) in records.iter().enumerate() {
        if r.kind == "UploadPart"
            && ok(r)
            && let Some(id) = query(r, "uploadId")
        {
            *parts.entry(id).or_default() += r.request_bytes.get().unwrap_or(0);
            continue;
        }
        let (Some(b), Some(k)) = (r.bucket.as_deref(), r.key.as_deref()) else {
            continue;
        };
        if !ok(r) || k.is_empty() {
            continue;
        }
        let written = match r.kind.as_str() {
            "PutObject" => Some(r.request_bytes.get()),
            "CopyObject" => Some(copy_source(r).and_then(|(sb, sk)| {
                keys.get(&(sb.as_str(), sk.as_str()))
                    .and_then(|(size, _)| *size)
            })),
            "CompleteMultipartUpload" => Some(query(r, "uploadId").and_then(|id| parts.remove(id))),
            _ => None,
        };
        if let Some(size) = written {
            let before = keys.insert((b, k), (size, Some(i)));
            if versioned && let Some((was, _)) = before {
                out.noncurrent += 1;
                out.noncurrent_bytes += was.unwrap_or(0);
            }
            continue;
        }
        match r.method.as_str() {
            "DELETE" if r.kind == "DeleteObject" && query(r, "versionId").is_none() => {
                let before = keys.remove(&(b, k));
                // on a versioned bucket a delete leaves a marker: what was
                // there is noncurrent, not gone
                if versioned && says_versioned(r) {
                    out.noncurrent += 1;
                    out.noncurrent_bytes += before.and_then(|(s, _)| s).unwrap_or(0);
                }
            }
            // (sizes of what the run did not write are wanted only to say
            // what its versions keep)
            "GET" if versioned && r.kind == "GetObject" => {
                let size = total_of(r).or_else(|| {
                    request(r, "range")
                        .is_none()
                        .then(|| r.response_bytes.get())
                        .flatten()
                });
                if let Some(size) = size {
                    keys.entry((b, k)).or_insert((Some(size), None));
                }
            }
            _ => {}
        }
    }
    out.left = keys
        .into_values()
        .filter_map(|(size, by)| Some((by?, size?)))
        .filter(|(_, b)| *b > 0)
        .collect();
    out.left.sort_unstable();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqlog::Header;

    fn rec(id: u64, method: &str, kind: &str, key: &str, up: u64, down: u64) -> Record {
        Record {
            id,
            time: chrono::DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64).unwrap(),
            method: method.into(),
            kind: kind.into(),
            bucket: Some("b".into()),
            key: Some(key.into()),
            status: Some(if method == "DELETE" { 204 } else { 200 }),
            request_bytes: Some(up).into(),
            response_bytes: Some(down).into(),
            ..Default::default()
        }
    }

    fn answered(mut r: Record, name: &str, value: &str) -> Record {
        r.lists_mut()
            .response_headers
            .push(Header::new(name, value));
        r
    }

    #[test]
    fn what_a_run_writes_and_keeps_is_stored_and_what_it_deletes_is_not() {
        let mut part = rec(3, "PUT", "UploadPart", "big", 8 << 20, 0);
        part.lists_mut().query = vec![("uploadId".into(), "u1".into())];
        let mut done = rec(4, "POST", "CompleteMultipartUpload", "big", 200, 300);
        done.lists_mut().query = vec![("uploadId".into(), "u1".into())];
        let records = [
            rec(1, "PUT", "PutObject", "a", 1000, 0),
            rec(2, "PUT", "PutObject", "tmp", 500, 0),
            part,
            done,
            rec(5, "DELETE", "DeleteObject", "tmp", 0, 0),
            // a key written again: its last write is what is stored
            rec(6, "PUT", "PutObject", "a", 1200, 0),
        ];
        let s = stored(&records);
        assert_eq!(s.left, vec![(3, 8 << 20), (5, 1200)]);
        assert_eq!(s.bytes(), (8 << 20) + 1200);
        // no answer said versions: nothing noncurrent
        assert!(!s.versioned);
        assert_eq!((s.noncurrent, s.noncurrent_bytes), (0, 0));
    }

    #[test]
    fn on_a_versioned_bucket_overwrites_and_deletes_keep_what_was_there() {
        let v = |r: Record, id: &str| answered(r, "x-amz-version-id", id);
        let records = [
            // read whole before: its size is known
            rec(1, "GET", "GetObject", "old", 0, 4096),
            v(rec(2, "PUT", "PutObject", "old", 100, 0), "v2"),
            v(rec(3, "PUT", "PutObject", "new", 300, 0), "v3"),
            v(rec(4, "PUT", "PutObject", "new", 400, 0), "v4"),
            answered(
                rec(5, "DELETE", "DeleteObject", "new", 0, 0),
                "x-amz-delete-marker",
                "true",
            ),
        ];
        let s = stored(&records);
        assert!(s.versioned);
        // three versions kept: old's 4096, new's 300 and its 400
        assert_eq!((s.noncurrent, s.noncurrent_bytes), (3, 4096 + 300 + 400));
        assert_eq!(s.left, vec![(1, 100)]);
        // a suspended bucket's `null` is no versioning
        let records = [
            answered(
                rec(1, "PUT", "PutObject", "k", 10, 0),
                "x-amz-version-id",
                "null",
            ),
            answered(
                rec(2, "PUT", "PutObject", "k", 10, 0),
                "x-amz-version-id",
                "null",
            ),
        ];
        assert_eq!(stored(&records).noncurrent, 0);
    }
}
