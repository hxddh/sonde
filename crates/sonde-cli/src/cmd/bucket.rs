//! `sonde bucket`: the store side of a bucket — its size and why, and its
//! incomplete uploads (aborted, or a lifecycle rule added, each a dry run
//! until confirmed). `du` and `mpu` before 18.0.

use std::sync::Arc;

use clap::{Args, Subcommand};
use sonde_core::Sonde;

use crate::Cli;

#[derive(Args)]
pub struct BucketArgs {
    #[command(subcommand)]
    action: BucketAction,
}

#[derive(Subcommand)]
enum BucketAction {
    /// What a bucket holds and why it is so big: versions, classes, prefixes, sizes.
    Size(super::du::DuArgs),
    #[command(flatten)]
    Uploads(super::mpu::MpuAction),
}

pub async fn run(s: &Arc<Sonde>, cli: &Cli, a: &BucketArgs) -> anyhow::Result<i32> {
    match &a.action {
        BucketAction::Size(d) => super::du::run(s, cli, d).await,
        BucketAction::Uploads(m) => super::mpu::act(s, m).await,
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn the_bucket_command_has_size_uploads_and_their_fixes() {
        let cli = crate::Cli::command();
        let bucket = cli.find_subcommand("bucket").expect("bucket");
        let names: Vec<&str> = bucket.get_subcommands().map(|c| c.get_name()).collect();
        for n in ["size", "uploads", "abort-uploads", "lifecycle"] {
            assert!(names.contains(&n), "{names:?}");
        }
    }
}
