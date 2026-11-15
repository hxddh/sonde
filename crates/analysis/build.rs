//! Embeds the web viewer a shared run is shown with (the site `nsh package
//! App.tsx --target web` makes in `viewer/`) from `$SONDE_VIEWER_DIR` or
//! `viewer/.nsh/out/sonde-viewer`, when it was built: each file once, its
//! gzipped copy where Native Shell made one (the page carries it so and the
//! browser unpacks it). Without it, `sonde share` says how to build it.

use std::path::PathBuf;

fn main() {
    viewer();
}

fn viewer() {
    println!("cargo:rerun-if-env-changed=SONDE_VIEWER_DIR");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("viewer_files.rs");
    let dir = std::env::var_os("SONDE_VIEWER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../viewer/.nsh/out/sonde-viewer")
        });
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files = Vec::new();
    let mut stack = vec![dir.clone()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if !matches!(p.extension().and_then(|x| x.to_str()), Some("gz" | "br")) {
                files.push(p);
            }
        }
    }
    files.sort();
    // (path, gzipped, bytes)
    let mut code = String::from("pub const VIEWER: &[(&str, bool, &[u8])] = &[\n");
    for f in &files {
        let rel = f
            .strip_prefix(&dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let gz = PathBuf::from(format!("{}.gz", f.display()));
        // (the page itself is read here as text: never packed)
        let (packed, from) = if gz.is_file() && rel != "index.html" {
            (true, &gz)
        } else {
            (false, f)
        };
        code.push_str(&format!(
            "    ({rel:?}, {packed}, include_bytes!({:?})),\n",
            from.canonicalize().unwrap()
        ));
    }
    code.push_str("];\n");
    std::fs::write(out, code).unwrap();
}
