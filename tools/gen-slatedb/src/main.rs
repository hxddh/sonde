//! `gen-slatedb <dir>`: a SlateDB database under `<dir>/db` with WAL SSTs,
//! L0 SSTs from memtable flushes, a writer restart (a new writer epoch,
//! fencing the first) and a detached checkpoint.

use std::sync::Arc;
use std::time::Duration;

use slatedb::Db;
use slatedb::admin::AdminBuilder;
use slatedb::config::{CheckpointOptions, FlushOptions, FlushType};
use slatedb::object_store::local::LocalFileSystem;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: gen-slatedb <dir>");
    std::fs::create_dir_all(&dir)?;
    let store = Arc::new(LocalFileSystem::new_with_prefix(&dir)?);
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
