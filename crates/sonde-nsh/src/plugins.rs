//! Plugins: signed Native Shell packages (`.nshpkg`) holding TSX views of
//! decoders and probes.
//!
//! - Installing verifies the package against the trusted roots, the
//!   revocation list and rollback protection (`nsh-package`), and refuses
//!   a package that declares any capability but `host` calls named
//!   `sonde.read.*`: no network, files, processes, clipboard, web views.
//! - A plugin runs in an engine of its own (the terminal host's second
//!   view), so it shares nothing with sonde's VM; its only way out is
//!   `host.call("sonde.read.…")`, answered by [`host_call`] with read-only
//!   data of the bucket and prefix the user opened it on. Credentials are
//!   never reachable: the calls take keys, not endpoints.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use nsh_package::{PublicKey, Revocations, TrustStore};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sonde_core::Sonde;

pub const HOST_PREFIX: &str = "sonde.read.";
/// The host calls a plugin may make.
pub const HOST_CALLS: &[&str] = &[
    "sonde.read.context",
    "sonde.read.list",
    "sonde.read.get",
    "sonde.read.head",
    "sonde.read.layout",
];
/// Largest `sonde.read.get` answer.
pub const MAX_GET: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub id: String,
    pub label: String,
    pub key: String,
    pub added: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub build: u64,
    pub sha256: String,
    pub key_id: String,
    pub enabled: bool,
    pub revoked: bool,
    pub installed: String,
    pub host_calls: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Registry {
    roots: Vec<Root>,
    plugins: Vec<Installed>,
}

pub struct Plugins {
    dir: PathBuf,
}

impl Plugins {
    pub fn open(s: &Sonde) -> Plugins {
        Plugins {
            dir: s.paths().plugins(),
        }
    }

    pub fn at(dir: &Path) -> Plugins {
        Plugins {
            dir: dir.to_path_buf(),
        }
    }

    fn registry(&self) -> Registry {
        std::fs::read(self.dir.join("registry.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self, r: &Registry) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        std::fs::write(
            self.dir.join("registry.json"),
            serde_json::to_vec_pretty(r).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    fn trust(&self, r: &Registry) -> Result<TrustStore, String> {
        let mut t = TrustStore::open(self.dir.join("trust")).map_err(|e| e.to_string())?;
        for root in &r.roots {
            t.add_root(PublicKey::from_base64(&root.key).map_err(|e| e.to_string())?);
        }
        Ok(t)
    }

    fn package_path(&self, id: &str) -> PathBuf {
        self.dir.join("packages").join(format!("{id}.nshpkg"))
    }

    pub fn list(&self) -> Vec<Installed> {
        self.registry().plugins
    }

    pub fn roots(&self) -> Vec<Root> {
        self.registry().roots
    }

    pub fn add_root(&self, label: &str, key: &str) -> Result<Root, String> {
        let k = PublicKey::from_base64(key.trim()).map_err(|e| format!("not a public key: {e}"))?;
        let mut r = self.registry();
        let root = Root {
            id: k.id(),
            label: label.into(),
            key: k.to_base64(),
            added: chrono::Utc::now().to_rfc3339(),
        };
        r.roots.retain(|x| x.id != root.id);
        r.roots.push(root.clone());
        self.save(&r)?;
        Ok(root)
    }

    pub fn remove_root(&self, id: &str) -> Result<(), String> {
        let mut r = self.registry();
        r.roots.retain(|x| x.id != id);
        self.save(&r)?;
        self.recheck()
    }

    /// Verifies `path` and installs it.
    pub fn install(&self, path: &Path) -> Result<Installed, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut r = self.registry();
        if r.roots.is_empty() {
            return Err("no trusted root: add the publisher's root key first".into());
        }
        let mut trust = self.trust(&r)?;
        let pkg = trust
            .verify(&bytes, "")
            .map_err(|e| format!("refused: {e}"))?;
        let host_calls = check_capabilities(pkg.capabilities.as_ref())?;
        trust.accept(&pkg).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(self.dir.join("packages")).map_err(|e| e.to_string())?;
        std::fs::write(self.package_path(&pkg.app.id), &bytes).map_err(|e| e.to_string())?;
        let p = Installed {
            id: pkg.app.id.clone(),
            name: pkg.app.name.clone(),
            version: pkg.app.version.clone(),
            build: pkg.app.build,
            sha256: pkg.sha256.clone(),
            key_id: pkg.key_id.clone(),
            enabled: true,
            revoked: false,
            installed: chrono::Utc::now().to_rfc3339(),
            host_calls,
        };
        r.plugins.retain(|x| x.id != p.id);
        r.plugins.push(p.clone());
        self.save(&r)?;
        Ok(p)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        let mut r = self.registry();
        let p = r
            .plugins
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("no plugin {id}"))?;
        if enabled && p.revoked {
            return Err(format!("{id} was revoked by its publisher"));
        }
        p.enabled = enabled;
        self.save(&r)
    }

    pub fn uninstall(&self, id: &str) -> Result<(), String> {
        let mut r = self.registry();
        r.plugins.retain(|p| p.id != id);
        let _ = std::fs::remove_file(self.package_path(id));
        self.save(&r)
    }

    pub fn apply_revocations(&self, path: &Path) -> Result<Value, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let list =
            Revocations::from_json(&text).map_err(|e| format!("not a revocation list: {e}"))?;
        let r = self.registry();
        let mut trust = self.trust(&r)?;
        let seq = trust
            .update_revocations(list)
            .map_err(|e| format!("refused: {e}"))?;
        self.recheck()?;
        Ok(
            json!({ "sequence": seq, "revoked": self.list().iter().filter(|p| p.revoked).map(|p| p.id.clone()).collect::<Vec<_>>() }),
        )
    }

    /// Verifies every installed package again (after root or revocation changes).
    fn recheck(&self) -> Result<(), String> {
        let mut r = self.registry();
        let trust = self.trust(&r)?;
        for p in &mut r.plugins {
            let ok = std::fs::read(self.package_path(&p.id))
                .ok()
                .is_some_and(|b| trust.verify(&b, "").is_ok());
            if !ok {
                p.revoked = true;
                p.enabled = false;
            }
        }
        self.save(&r)
    }

    /// The package bytes of an enabled plugin, verified again now.
    pub fn load(&self, id: &str) -> Result<Vec<u8>, String> {
        let r = self.registry();
        let p = r
            .plugins
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("no plugin {id}"))?;
        if !p.enabled {
            return Err(format!("{id} is disabled"));
        }
        let bytes = std::fs::read(self.package_path(id)).map_err(|e| e.to_string())?;
        self.trust(&r)?
            .verify(&bytes, "")
            .map_err(|e| format!("refused: {e}"))?;
        Ok(bytes)
    }

    /// The trust store with the roots (for the host's own verification).
    pub fn trust_store(&self) -> Result<TrustStore, String> {
        self.trust(&self.registry())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// The host calls a package declares; refuses every other capability.
pub fn check_capabilities(caps: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(caps) = caps else {
        return Err("a plugin must declare its capabilities (only `host` with sonde.read.* names); an undeclared app may do everything".into());
    };
    let obj = caps.as_object().ok_or("capabilities must be an object")?;
    let mut calls = vec![];
    for (k, v) in obj {
        match k.as_str() {
            "host" => {
                let names = v.as_array().ok_or("`host` must list names")?;
                for n in names {
                    let n = n.as_str().ok_or("`host` names are strings")?;
                    if !n.starts_with(HOST_PREFIX) || n.contains("..") {
                        return Err(format!(
                            "host call {n:?} is not allowed: plugins may only call {HOST_PREFIX}*"
                        ));
                    }
                    calls.push(n.to_string());
                }
            }
            _ if grants_nothing(v) => {}
            other => {
                return Err(format!(
                    "capability `{other}` is not allowed in a plugin (no network, files, processes, clipboard or web views)"
                ));
            }
        }
    }
    Ok(calls)
}

/// `false`, `null`, empty lists, and objects of those: a key that grants nothing
/// (`nsh publish` writes every key of the declaration).
fn grants_nothing(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => true,
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.values().all(grants_nothing),
        _ => false,
    }
}

// --- the host side ---------------------------------------------------------

static HOST: Mutex<Option<Arc<dyn Fn(String, Value) + Send + Sync>>> = Mutex::new(None);

/// Installed by a host that can show plugin views (the terminal host).
pub fn set_host(f: Arc<dyn Fn(String, Value) + Send + Sync>) {
    *HOST.lock() = Some(f);
}

pub fn host_available() -> bool {
    HOST.lock().is_some()
}

pub fn request_open(id: &str, context: Value) -> Result<(), String> {
    match HOST.lock().clone() {
        Some(f) => {
            f(id.to_string(), context);
            Ok(())
        }
        None => Err(
            "this host cannot show plugin views: open them in the terminal UI (`sonde tui`)".into(),
        ),
    }
}

/// The bucket and prefix a plugin was opened on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub endpoint: String,
    pub bucket: String,
    #[serde(default)]
    pub prefix: String,
}

fn within(ctx: &Context, key: &str) -> Result<(), String> {
    if key.starts_with(&ctx.prefix) && !key.split('/').any(|c| c == "..") {
        Ok(())
    } else {
        Err(format!(
            "{key:?} is outside the prefix the plugin was opened on ({:?})",
            ctx.prefix
        ))
    }
}

/// Answers a plugin's `host.call`. Read-only, confined to the context's
/// bucket and prefix; never returns credentials or endpoint details.
pub async fn host_call(
    s: &Sonde,
    ctx: &Context,
    name: &str,
    data: &Value,
) -> Result<Value, String> {
    let client = s.client(&ctx.endpoint).map_err(|e| e.to_string())?;
    match name {
        "sonde.read.context" => {
            let ep = s
                .endpoint_config(&ctx.endpoint)
                .map(|e| e.vendor.label().to_string())
                .unwrap_or_default();
            Ok(
                json!({ "bucket": ctx.bucket, "prefix": ctx.prefix, "vendor": ep, "sonde": env!("CARGO_PKG_VERSION") }),
            )
        }
        "sonde.read.list" => {
            let prefix = data["prefix"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| ctx.prefix.clone());
            within(ctx, &prefix)?;
            let limit = data["limit"].as_u64().unwrap_or(1000).min(10_000) as usize;
            let objs = client
                .list_all(&ctx.bucket, &prefix, limit)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Value::Array(objs.into_iter().map(|o| json!({ "key": o.key, "size": o.size, "lastModified": o.last_modified, "etag": o.etag })).collect()))
        }
        "sonde.read.head" => {
            let key = data["key"].as_str().ok_or("key")?;
            within(ctx, key)?;
            let h = client
                .head_object(&ctx.bucket, key, None)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "size": h.size, "etag": h.etag, "metadata": h.metadata }))
        }
        "sonde.read.get" => {
            let key = data["key"].as_str().ok_or("key")?;
            within(ctx, key)?;
            let start = data["start"].as_u64().unwrap_or(0);
            let end = data["end"]
                .as_u64()
                .unwrap_or(start + MAX_GET - 1)
                .min(start + MAX_GET - 1);
            let r = client
                .get_object(&ctx.bucket, key, Some(&format!("bytes={start}-{end}")))
                .await
                .map_err(|e| e.to_string())?;
            Ok(
                json!({ "base64": base64::engine::general_purpose::STANDARD.encode(&r.body), "size": r.body.len() }),
            )
        }
        "sonde.read.layout" => {
            let src = sonde_core::services::layout::S3Source {
                client,
                bucket: ctx.bucket.clone(),
                max_objects: 50_000,
                ctx: None,
            };
            let found = decoders::detect(&src, &ctx.prefix).await.map_err(|e| e.0)?;
            Ok(serde_json::to_value(found).unwrap_or_default())
        }
        other => Err(format!("unknown host call {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_policy() {
        assert!(check_capabilities(None).is_err());
        assert_eq!(
            check_capabilities(Some(
                &json!({"host": ["sonde.read.list", "sonde.read.get"]})
            ))
            .unwrap()
            .len(),
            2
        );
        assert!(
            check_capabilities(Some(&json!({"host": ["sonde.write.delete"]})))
                .unwrap_err()
                .contains("not allowed")
        );
        assert!(check_capabilities(Some(&json!({"host": ["sonde.read..x"]}))).is_err());
        assert!(
            check_capabilities(Some(&json!({"network": {"hosts": ["*"]}})))
                .unwrap_err()
                .contains("network")
        );
        assert!(check_capabilities(Some(&json!({"fs": {"read": ["/"]}}))).is_err());
        assert!(check_capabilities(Some(&json!({"clipboard": true}))).is_err());
        // what `nsh publish` writes for a plugin that asks only for host calls
        let published = json!({"network": {"hosts": [], "http": []}, "process": {"execute": [], "exit": false, "env": []}, "fs": {"read": [], "write": []}, "statusBar": false, "clipboard": false, "picker": false, "openUrl": false, "host": ["sonde.read.list"]});
        assert_eq!(
            check_capabilities(Some(&published)).unwrap(),
            vec!["sonde.read.list"]
        );
        assert!(
            check_capabilities(Some(
                &json!({"network": {"hosts": ["evil.example"]}, "host": []})
            ))
            .is_err()
        );
    }

    #[test]
    fn confined_to_prefix() {
        let c = Context {
            endpoint: "e".into(),
            bucket: "b".into(),
            prefix: "db/".into(),
        };
        assert!(within(&c, "db/manifest/1").is_ok());
        assert!(within(&c, "other/x").is_err());
        assert!(within(&c, "db/../secret").is_err());
    }

    #[test]
    fn install_refuses_without_roots() {
        let dir = tempfile::tempdir().unwrap();
        let p = Plugins::at(dir.path());
        std::fs::write(dir.path().join("x.nshpkg"), b"junk").unwrap();
        assert!(
            p.install(&dir.path().join("x.nshpkg"))
                .unwrap_err()
                .contains("no trusted root")
        );
        let key = nsh_package::SecretKey::generate()
            .unwrap()
            .public()
            .to_base64();
        let root = p.add_root("test", &key).unwrap();
        assert_eq!(p.roots()[0].id, root.id);
        assert!(
            p.install(&dir.path().join("x.nshpkg"))
                .unwrap_err()
                .starts_with("refused")
        );
        p.remove_root(&root.id).unwrap();
        assert!(p.roots().is_empty());
    }
}
