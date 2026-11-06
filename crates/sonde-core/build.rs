//! Embeds the decoder samples (real SlateDB, Iceberg, lakeFS and Woodpecker
//! layouts) for the demo server's `demo-layouts` bucket.

use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.path());
    for e in entries {
        if e.path().is_dir() {
            walk(&e.path(), out);
        } else {
            out.push(e.path());
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../decoders/samples");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = vec![];
    walk(&root, &mut files);
    let mut code = String::from("pub static SAMPLES: &[(&str, &[u8])] = &[\n");
    for f in files {
        let key = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        code.push_str(&format!(
            "    ({key:?}, include_bytes!({:?})),\n",
            f.canonicalize().unwrap()
        ));
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("samples.rs");
    std::fs::write(out, code).unwrap();
}
