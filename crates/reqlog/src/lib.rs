//! Request records shared by every sonde module.
//!
//! Every HTTP exchange `s3raw` performs becomes a [`Record`]: already
//! redacted ([`redact`]) before it is stored, so no secret ever reaches the
//! buffer, an export or the UI. The [`RequestLog`] is a bounded ring buffer
//! (5000 records) with a sink (a run file), filters ([`Filter`]),
//! exports ([`export`]) and "copy as" renderings ([`copy`]).

pub mod copy;
pub mod export;
mod list;
mod name;
pub mod private;
mod record;
pub mod redact;
mod ring;
mod size;

pub use list::Lists;
pub use name::Name;
pub use record::{
    Header, KEYS_HEADER, KeyCounter, MAX_LATENCY_MS, MAX_REQUEST_BODY, Rare, Record, RecordSummary,
    Text, is_control_operation, is_listing,
};
pub use ring::{DEFAULT_CAPACITY, Filter, RequestLog, Sink, StatusFilter};
pub use size::{Millis, Size};
