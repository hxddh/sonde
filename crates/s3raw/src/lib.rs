//! A raw S3 client for diagnosis.
//!
//! Unlike general-purpose clients, `s3raw` hides nothing: every request
//! keeps its full request and response headers, its status, its raw error
//! body, its retries and its timing, all logged (redacted) into a
//! [`reqlog::RequestLog`]. It signs with its own SigV4 ([`sigv4`]) so the
//! canonical request and string to sign can be shown, addresses buckets
//! path-style or virtual-hosted-style, trusts custom CAs, retries
//! `503 SlowDown` with exponential backoff and honors a shared
//! [`throttle::Throttle`] (concurrency and request rate).

pub mod checksum;
pub mod client;
pub mod credentials;
pub mod error;
pub mod etag;
pub mod ops;
pub mod sigv4;
pub mod throttle;
pub mod xml;

pub use client::{Addressing, Body, ClientConfig, RetryPolicy, S3Client, S3Request, S3Response};
pub use client::{fill_ids, has_dot_segment, header_pairs, is_dot_segment_refusal};
pub use credentials::{Credentials, CredentialsProvider, Secret, StaticProvider};
pub use error::{ErrorKind, S3Error};
pub use throttle::Throttle;
