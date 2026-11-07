//! `gen-slatedb <dir | s3://bucket>`: a SlateDB database under `db/` with
//! WAL SSTs, L0 SSTs from memtable flushes, a writer restart (a new writer
//! epoch, fencing the first) and a detached checkpoint. With `s3://bucket` it
//! writes through object_store's S3 client, configured from the environment
//! (`sonde run` sets it).

use std::sync::Arc;
use std::time::Duration;

use slatedb::Db;
use slatedb::admin::AdminBuilder;
use slatedb::config::{CheckpointOptions, FlushOptions, FlushType};
use slatedb::object_store::ObjectStore;
use slatedb::object_store::aws::AmazonS3Builder;
use slatedb::object_store::local::LocalFileSystem;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let target = std::env::args().nth(1).expect("usage: gen-slatedb <dir | s3://bucket>");
    let store: Arc<dyn ObjectStore> = match target.strip_prefix("s3://") {
        Some(bucket) => Arc::new(AmazonS3Builder::from_env().with_bucket_name(bucket).build()?),
        None => {
            std::fs::create_dir_all(&target)?;
            Arc::new(LocalFileSystem::new_with_prefix(&target)?)
        }
    };
    let path = "db";
    for session in 0..2 {
        let db = Db::open(path, store.clone()).await?;
        for batch in 0..4 {
            for i in 0..50 {
                let k = format!("key-{session}-{batch}-{i:03}");
                db.put(k.as_bytes(), format!("value {session} {batch} {i}").as_bytes()).await?;
            }
            db.flush_with_options(FlushOptions { flush_type: FlushType::Wal }).await?;
            if batch % 2 == 1 {
                db.flush_with_options(FlushOptions { flush_type: FlushType::MemTable }).await?;
            }
        }
        db.close().await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let admin = AdminBuilder::new(path, store.clone()).build();
    let cp = admin.create_detached_checkpoint(&CheckpointOptions { lifetime: Some(Duration::from_secs(30 * 86400)), ..Default::default() }).await?;
    println!("checkpoint {} at manifest {}", cp.id, cp.manifest_id);
    Ok(())
}
