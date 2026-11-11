//! sonde's domain logic, free of any UI framework.
//!
//! [`Sonde`] owns the configuration, the request log, the jobs, the
//! destructive-operation guard and one [`s3raw::S3Client`] per endpoint.
//! The services ([`services`]) implement each diagnosis; `sonde-nsh`
//! exports them to TSX and `sonde-cli` to scripts.

pub mod config;
pub mod credentials;
pub mod demo;
pub mod guard;
pub mod jobs;
pub mod paths;
pub mod phrase;
pub mod services;
pub mod sonde;
pub mod vendor;

pub use config::{Config, CredentialSource, EndpointConfig, Settings};
pub use guard::{Action, Guard, Plan, PlanItem};
pub use jobs::{JobCtx, JobEvent, JobInfo, JobManager, JobState, Progress};
pub use paths::Paths;
pub use phrase::Phrase;
pub use s3raw::{ErrorKind, S3Error};
pub use sonde::{EndpointView, Sonde};
pub use vendor::Vendor;
