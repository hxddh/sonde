//! Where a layout's objects come from: an object store (sonde-core's S3
//! source), a local directory, or memory.

use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    pub last_modified: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[error("{0}")]
pub struct SourceError(pub String);

/// Read-only access to objects. Decoders never write.
#[async_trait]
pub trait Source: Send + Sync {
    /// Every object under `prefix` (recursively), sorted by key.
    async fn list(&self, prefix: &str) -> Result<Vec<ObjectMeta>, SourceError>;
    /// The object's bytes; `range` is `(start, end_inclusive)`.
    async fn get(&self, key: &str, range: Option<(u64, u64)>) -> Result<Bytes, SourceError>;
}

/// Objects in memory (tests).
#[derive(Debug, Default, Clone)]
pub struct MemSource {
    pub objects: BTreeMap<String, (Bytes, Option<DateTime<Utc>>)>,
}

impl MemSource {
    pub fn put(&mut self, key: &str, data: impl Into<Bytes>, t: Option<DateTime<Utc>>) {
        self.objects.insert(key.into(), (data.into(), t));
    }
}

fn slice(b: &Bytes, range: Option<(u64, u64)>) -> Bytes {
    match range {
        None => b.clone(),
        Some((s, e)) => {
            let s = (s as usize).min(b.len());
            let e = ((e as usize) + 1).min(b.len());
            b.slice(s..e.max(s))
        }
    }
}

#[async_trait]
impl Source for MemSource {
    async fn list(&self, prefix: &str) -> Result<Vec<ObjectMeta>, SourceError> {
        Ok(self
            .objects
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, (b, t))| ObjectMeta {
                key: k.clone(),
                size: b.len() as u64,
                last_modified: *t,
            })
            .collect())
    }
    async fn get(&self, key: &str, range: Option<(u64, u64)>) -> Result<Bytes, SourceError> {
        self.objects
            .get(key)
            .map(|(b, _)| slice(b, range))
            .ok_or_else(|| SourceError(format!("no object {key}")))
    }
}

/// A local directory as a bucket: keys are paths relative to it, `/`-separated.
#[derive(Debug, Clone)]
pub struct DirSource {
    pub root: PathBuf,
}

impl DirSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        DirSource { root: root.into() }
    }
}

fn walk(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<ObjectMeta>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, base, out);
        } else if let Ok(m) = e.metadata() {
            let key = p
                .strip_prefix(base)
                .unwrap_or(&p)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/");
            out.push(ObjectMeta {
                key,
                size: m.len(),
                last_modified: m.modified().ok().map(DateTime::<Utc>::from),
            });
        }
    }
}

#[async_trait]
impl Source for DirSource {
    async fn list(&self, prefix: &str) -> Result<Vec<ObjectMeta>, SourceError> {
        let mut out = vec![];
        walk(&self.root, &self.root, &mut out);
        out.retain(|o| o.key.starts_with(prefix));
        out.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(out)
    }
    async fn get(&self, key: &str, range: Option<(u64, u64)>) -> Result<Bytes, SourceError> {
        if key.split('/').any(|c| c == "..") {
            return Err(SourceError("'..' in key".into()));
        }
        let b =
            std::fs::read(self.root.join(key)).map_err(|e| SourceError(format!("{key}: {e}")))?;
        Ok(slice(&Bytes::from(b), range))
    }
}
