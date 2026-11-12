//! A sample table of the decoders' tests as a bucket in memory, with some of
//! its files replaced by the fuzzer's bytes: the decoder follows the real
//! files' paths to the one it is given.

use std::path::Path;
use std::sync::OnceLock;

use decoders::{Format, MemSource};

/// Every file under `crates/decoders/samples/<sub>`, keyed by its path there.
pub fn sample(sub: &str) -> &'static Vec<(String, Vec<u8>)> {
    static ICEBERG: OnceLock<Vec<(String, Vec<u8>)>> = OnceLock::new();
    assert_eq!(sub, "iceberg");
    ICEBERG.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/decoders/samples");
        let mut out = Vec::new();
        let mut stack = vec![root.join(sub)];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir)
                .expect("the decoders' samples")
                .flatten()
            {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let key = p.strip_prefix(root.join(sub)).unwrap();
                    let key = key.to_string_lossy().replace('\\', "/");
                    out.push((key, std::fs::read(&p).unwrap()));
                }
            }
        }
        out
    })
}

/// The Iceberg sample with each file `swap` picks given `data` instead.
pub fn iceberg_with(data: &[u8], swap: impl Fn(&str) -> bool) {
    // the decoder catches apache-avro's panics on a malformed schema; the
    // fuzzer's hook would abort on them first. A panic it does not catch
    // still ends the run, and is a crash.
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| drop(std::panic::take_hook()));
    let mut src = MemSource::default();
    for (k, v) in sample("iceberg") {
        let body = if swap(k) { data.to_vec() } else { v.clone() };
        src.put(k, body, None);
    }
    let _ = futures::executor::block_on(decoders::analyze(
        &src,
        Format::Iceberg,
        "warehouse/demo.db/events/",
    ));
}
