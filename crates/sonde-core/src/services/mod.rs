//! The diagnoses, one module each.

pub mod browse;
pub mod du;
pub mod exec;
pub mod mpu;
pub mod versions;

use chrono::{DateTime, Utc};

/// Parses an S3 timestamp (`2024-01-01T00:00:00.000Z`, or RFC 1123 in headers).
pub fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .or_else(|_| DateTime::parse_from_rfc2822(s))
        .ok()
        .map(|d| d.with_timezone(&Utc))
}
