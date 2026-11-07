//! Embeds the web viewer a shared session is shown with (the site `nsh
//! package App.tsx --target web` makes in `viewer/`) from `$SONDE_VIEWER_DIR`
//! or `viewer/.nsh/out/sonde-viewer`, when it was built; the precompressed
//! copies are left out (any web server compresses). Without it, sharing
//! writes the session's data alone.

use std::path::PathBuf;

fn main() {
    viewer();
}

fn viewer() {
    println!("cargo:rerun-if-env-changed=SONDE_VIEWER_DIR");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("viewer.rs");
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
    let mut code = String::from("pub const VIEWER: &[(&str, &[u8])] = &[\n");
    for f in &files {
        let rel = f
            .strip_prefix(&dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        code.push_str(&format!(
            "    ({rel:?}, include_bytes!({:?})),\n",
            f.canonicalize().unwrap()
        ));
    }
    code.push_str("];\n");
    std::fs::write(out, code).unwrap();
}
