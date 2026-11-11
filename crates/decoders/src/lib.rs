//! Storage layout lenses.
//!
//! [`detect`] recognizes the layouts under a prefix; [`analyze`] decodes
//! one into a [`Layout`]: facts, issues, a timeline, tables, and snapshots
//! of the logical state over time for replay ([`Layout::state_at`]).
//! Decoders read through a [`Source`] and never write.

pub mod cadence;
pub mod fb;
pub mod iceberg;
pub mod lakefs;
pub mod model;
pub mod phrase;
pub mod slatedb;
pub mod source;
pub mod sstable;
pub mod woodpecker;

pub use model::{Detection, Event, Fact, Format, Issue, Layout, Severity, Snapshot, Table};
pub use phrase::Phrase;
pub use source::{DirSource, MemSource, ObjectMeta, Source, SourceError};

/// Layouts found under `prefix`, decodable formats first.
pub async fn detect(src: &dyn Source, prefix: &str) -> Result<Vec<Detection>, SourceError> {
    let objects = src.list(prefix).await?;
    let mut out = vec![];
    out.extend(slatedb::detect(&objects));
    out.extend(iceberg::detect(&objects));
    out.extend(lakefs::detect(&objects));
    out.extend(woodpecker::detect(&objects));
    out.extend(cadence::detect(&objects));
    Ok(out)
}

/// Decodes the layout `format` rooted at `root`.
pub async fn analyze(src: &dyn Source, format: Format, root: &str) -> Result<Layout, SourceError> {
    match format {
        Format::Slatedb => slatedb::analyze(src, root).await,
        Format::Iceberg => iceberg::analyze(src, root).await,
        Format::Lakefs => lakefs::analyze(src, root).await,
        Format::Woodpecker => woodpecker::analyze(src, root).await,
        Format::Turbopuffer | Format::Warpstream | Format::Generic => {
            cadence::analyze(src, root, format).await
        }
    }
}
