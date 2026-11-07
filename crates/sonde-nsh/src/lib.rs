//! `sonde/native`: what TSX imports from Rust.
//!
//! Every function converts types and calls `sonde-core`, `probes` or
//! `decoders`; work runs on the runtime's threads (`background`), never
//! on the engine thread. Credentials never cross: endpoints are exposed
//! as ids and display names ([`sonde_core::EndpointView`]), and the
//! request records are redacted before they are stored.
//!
//! Errors reach TSX as `Error` objects whose message is JSON
//! (`{ "message", "code", "status", "requestId", "recordId", "kind" }`),
//! so the UI can show a readable message with the request id and jump to
//! the record in the request inspector.

mod api;
mod listing;
pub mod plugins;
mod run;
mod sessions;
mod state;

pub use api::Sonde;
pub use listing::Listing;
pub use sessions::Sessions;
pub use state::{install, sonde};

use nsh_core::prelude::*;

/// The UI set TSX imports as `sonde/native`.
#[native_module("sonde/native")]
fn native(registry: &mut Registry) {
    registry
        .module::<Sonde>()
        .module::<Sessions>()
        .class::<Listing>();
}
