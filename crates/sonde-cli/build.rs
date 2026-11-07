//! With feature `tui`, embeds the app's bundle (`nsh build`'s
//! `bundle.js`, `capabilities.json`, `app.json`) from `$SONDE_BUNDLE_DIR`
//! or `app/.nsh/out`, so `sonde tui` is one file. Without a bundle the
//! binary needs `sonde tui --bundle <bundle.js>`.
//!
//! (The web viewer `sonde share` writes is embedded by sonde-proxy.)

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=SONDE_BUNDLE_DIR");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("bundle.rs");
    let dir = std::env::var_os("SONDE_BUNDLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/.nsh/out"));
    let js = dir.join("bundle.js");
    println!("cargo:rerun-if-changed={}", js.display());
    let code = if std::env::var_os("CARGO_FEATURE_TUI").is_some() && js.exists() {
        let caps = dir.join("capabilities.json");
        let map = dir.join("bundle.js.map");
        println!("cargo:rerun-if-changed={}", caps.display());
        format!(
            "pub const BUNDLE: Option<&str> = Some(include_str!({:?}));\npub const CAPABILITIES: Option<&str> = {};\npub const SOURCE_MAP: Option<&str> = {};\n",
            js.canonicalize().unwrap(),
            if caps.exists() {
                format!("Some(include_str!({:?}))", caps.canonicalize().unwrap())
            } else {
                "None".into()
            },
            if map.exists() {
                format!("Some(include_str!({:?}))", map.canonicalize().unwrap())
            } else {
                "None".into()
            },
        )
    } else {
        "pub const BUNDLE: Option<&str> = None;\npub const CAPABILITIES: Option<&str> = None;\npub const SOURCE_MAP: Option<&str> = None;\n".into()
    };
    std::fs::write(out, code).unwrap();
}
