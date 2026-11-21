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

/// The objects under a prefix, sorted by key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub objects: Vec<ObjectMeta>,
    /// The listing stopped before its end (a source's cap): what is not in
    /// it may still exist.
    pub cut: bool,
}

impl From<Vec<ObjectMeta>> for Listing {
    fn from(objects: Vec<ObjectMeta>) -> Self {
        Listing {
            objects,
            cut: false,
        }
    }
}

/// Read-only access to objects. Decoders never write.
#[async_trait]
pub trait Source: Send + Sync {
    /// Every object under `prefix` (recursively), sorted by key.
    async fn list(&self, prefix: &str) -> Result<Listing, SourceError>;
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
    async fn list(&self, prefix: &str) -> Result<Listing, SourceError> {
        Ok(self
            .objects
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, (b, t))| ObjectMeta {
                key: k.clone(),
                size: b.len() as u64,
                last_modified: *t,
            })
            .collect::<Vec<_>>()
            .into())
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

/// Whether `key` names a file under the folder: relative, with no `..`, no
/// root and no drive (`/etc/passwd` or `C:/x` would leave it).
fn inside(key: &str) -> bool {
    use std::path::Component;
    let drive = key.split(['/', '\\']).next().is_some_and(|first| {
        first.len() == 2 && first.ends_with(':') && first.as_bytes()[0].is_ascii_alphabetic()
    });
    !drive
        && !key.split(['/', '\\']).any(|c| c == "..")
        && std::path::Path::new(key)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

#[async_trait]
impl Source for DirSource {
    async fn list(&self, prefix: &str) -> Result<Listing, SourceError> {
        let mut out = vec![];
        walk(&self.root, &self.root, &mut out);
        out.retain(|o| o.key.starts_with(prefix));
        out.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(out.into())
    }
    async fn get(&self, key: &str, range: Option<(u64, u64)>) -> Result<Bytes, SourceError> {
        if !inside(key) {
            return Err(SourceError(format!("{key}: not a path inside the folder")));
        }
        let b =
            std::fs::read(self.root.join(key)).map_err(|e| SourceError(format!("{key}: {e}")))?;
        Ok(slice(&Bytes::from(b), range))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_folder_gives_nothing_outside_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("in")).unwrap();
        std::fs::write(dir.path().join("in/x"), b"x").unwrap();
        std::fs::write(dir.path().join("secret"), b"s").unwrap();
        let src = DirSource::new(dir.path().join("in"));
        assert_eq!(src.get("x", None).await.unwrap(), Bytes::from("x"));
        let outside = dir.path().join("secret").display().to_string();
        for key in [
            outside.as_str(),
            "../secret",
            "a/../../secret",
            "C:/secret",
            "c:\\x",
        ] {
            assert!(src.get(key, None).await.is_err(), "{key}");
        }
    }
}
