//! The layout lens through S3: layouts written by SlateDB and pyiceberg
//! (`SONDE_IT_LAYOUTS`, else the committed samples) are uploaded to each
//! target and decoded through ListObjectsV2 and GET.

mod common;

use std::path::{Path, PathBuf};

use bytes::Bytes;
use decoders::{Format, Severity};
use sonde_core::services::layout::S3Source;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().is_dir() {
            walk(&e.path(), out);
        } else {
            out.push(e.path());
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn layouts_through_s3() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../decoders/samples");
    let generated = std::env::var_os("SONDE_IT_LAYOUTS").map(PathBuf::from);
    for t in common::targets() {
        let c = t.sonde.client(&t.name).unwrap();
        let b = common::bucket_name("layouts");
        c.create_bucket(&b, false).await.unwrap();
        let mut roots: Vec<(String, PathBuf)> = vec![("samples/".into(), samples.clone())];
        if let Some(g) = &generated {
            roots.push(("generated/".into(), g.clone()));
        }
        for (prefix, dir) in &roots {
            let mut files = vec![];
            walk(dir, &mut files);
            for f in files {
                let key = format!(
                    "{prefix}{}",
                    f.strip_prefix(dir)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                );
                c.put_object(&b, &key, Bytes::from(std::fs::read(&f).unwrap()), &[])
                    .await
                    .unwrap_or_else(|e| panic!("{}: put {key}: {e}", t.name));
            }
        }
        let src = S3Source {
            client: c.clone(),
            bucket: b.clone(),
            max_objects: 100_000,
            ctx: None,
        };
        let found = decoders::detect(&src, "").await.unwrap();
        for want in [
            Format::Slatedb,
            Format::Iceberg,
            Format::Lakefs,
            Format::Woodpecker,
        ] {
            assert!(
                found
                    .iter()
                    .any(|d| d.format == want && d.root.starts_with("samples/")),
                "{}: {want:?} not detected in {found:?}",
                t.name
            );
        }
        for d in found.iter().filter(|d| d.format != Format::Generic) {
            let l = decoders::analyze(&src, d.format, &d.root).await.unwrap();
            let errors: Vec<_> = l
                .issues
                .iter()
                .filter(|i| i.severity == Severity::Error)
                .collect();
            assert!(
                errors.is_empty(),
                "{}: {:?} at {}: {errors:?}",
                t.name,
                d.format,
                d.root
            );
            println!("{}: {:?} {} — {}", t.name, d.format, d.root, l.headline);
        }
        if generated.is_some() {
            assert!(
                found
                    .iter()
                    .any(|d| d.format == Format::Slatedb && d.root.starts_with("generated/")),
                "{}: generated SlateDB",
                t.name
            );
            assert!(
                found
                    .iter()
                    .any(|d| d.format == Format::Iceberg && d.root.starts_with("generated/")),
                "{}: generated Iceberg",
                t.name
            );
        }
        // remove the bucket
        loop {
            let objs = c.list_all(&b, "", 1000).await.unwrap();
            if objs.is_empty() {
                break;
            }
            let keys: Vec<(String, Option<String>)> =
                objs.into_iter().map(|o| (o.key, None)).collect();
            c.delete_objects(&b, &keys, true).await.unwrap();
        }
        let _ = c.delete_bucket(&b).await;
    }
}
