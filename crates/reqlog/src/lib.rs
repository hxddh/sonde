//! Request records shared by every sonde module.
//!
//! Every HTTP exchange `s3raw` performs becomes a [`Record`]: already
//! redacted ([`redact`]) before it is stored, so no secret ever reaches the
//! buffer, the SQLite file, an export or the UI. The [`RequestLog`] is a
//! bounded ring buffer (5000 records by default) with optional persistence
//! ([`sqlite`]), filters ([`Filter`]), exports ([`export`]) and
//! "copy as" renderings ([`copy`]).

pub mod copy;
pub mod export;
pub mod redact;
mod record;
mod ring;
#[cfg(feature = "sqlite")]
pub mod sqlite;

pub use record::{Header, Record, RecordSummary};
pub use ring::{Filter, RequestLog, StatusFilter, DEFAULT_CAPACITY};
